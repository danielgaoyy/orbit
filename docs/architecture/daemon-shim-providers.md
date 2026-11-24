# Daemon、Shim、Provider 驱动与大会话性能架构

本文档管辖 `orbit-daemon`、`orbit-shim`、`orbit-providers`、`orbit-protocol` 及跨主机长连接/分页的核心工程契约。改动下列任意机制前必须先通读本规范。

---

## 1. Daemon 与 Shim 进程隔离红线

- **独立进程壳**：`crates/orbit-shim` 通过 `setsid` 创建独立会话组，借助 `portable-pty` 与 stdio pipe 托管长寿命有状态 Agent 进程（`claude`、`codex`、`agy`、`pi`、ACP 引擎）与用户交互终端 `PTY`。
- **Unix Socket 通信**：Daemon 通过 `$ORBIT_HOME/shims/<shim-id>.sock` 与 Shim 双向通信并记录游标偏移，确保升级或重启 `orbit daemon` 时零中断重新附着正在运行的会话。
- **Unix 进程组终止安全不变式**：在 `unsafe_code = "deny"` 约束下调用外部 `/bin/kill` 终止子进程或进程组（`crates/orbit-shim`、`crates/orbit-daemon/src/platform.rs`、`crates/orbit-providers`）时，**必须**先校验 `pid > 1` / `pgid > 1`，并在负进程组 ID 前强制传入 `--` 选项终止符（如 `/bin/kill -s TERM -- -<pgid>`），严禁省略 `--`（Linux `procps-ng 4.0.4` 的 `getopt_long` 会把未加 `--` 且以 `1` 开头的 `-1xxxxxx` 误解析为 `kill(-1, ...)`，误杀同 UID 下包括 `systemd --user` 与桌面会话在内的全部用户进程；由 `pitfall_07` 回归拦截）。
- **单二进制内聚**：全部 `daemon` / `shim` / `relay` / CLI 子命令打包在单一静态 `orbit` 可执行文件（`crates/orbit-cli`）中，不得依赖外部 Node.js / Python 运行时。

---

## 2. Provider 驱动与消息零错位契约（Zero Off-By-One）

- **Native 驱动**（`crates/orbit-providers/src/`）：
  - `claude.rs`（`stream-json` 控制协议，私钥 MCP 配置写入 `$ORBIT_HOME/runtime/claude-mcp-*.json` `0600`）
  - `codex.rs`（`app-server` JSON-RPC 2.0）
  - `antigravity.rs`（`agy` ACP stdio + 字节偏移 transcript 流）
  - `pi.rs`（`pi-rpc`，私钥 MCP 配置写入 `$ORBIT_HOME/runtime/mcp-pi-*.json` `0600`，经环境变量与 `cleanup_paths` 托管，零 `argv` 凭据泄露）
- **通用 ACP 生态与客户端回调**：`acp_registry.rs`、`acp_providers.rs`、`acp_generic.rs`、`acp_callbacks.rs` 与 `readiness.rs`（34+ ACP Agent 目录，统一通过 `acp_callbacks::AcpCallbackHost` 声明并实现工作区隔离的 `fs/read_text_file`、`fs/write_text_file` 及独立进程组 `terminal/*` 回调，支持 `Drop` 自动清理）。
- **Daemon 异步隔离、OS 文件监听与 SEP-1865 MCP Apps**：
  - `crates/orbit-daemon/src/manager.rs` 全部异步路径通过 `with_store` / `try_store`（`tokio::task::spawn_blocking`）将同步 SQLite 读写移出 Tokio worker 线程。
  - `crates/orbit-daemon/src/workspace_watcher.rs` 基于 `notify` 提供 250ms 防抖、忽略构建/对象目录并限流的文件与 `.git/HEAD`/`index` 监听，自动广播 `WireMessage::WorkspaceFilesChanged` 并失效符号/规则缓存。
  - `crates/orbit-daemon/src/mcp_servers.rs` 与 `artifacts.rs` 支持 SEP-1865 `ui://` 资源 `resources/read` 自动拉取与严格绑定归属 MCP Server 的 `tools/call`（`WireMessage::ReadMcpResource` / `CallArtifactMcpTool`）。
- **防轮次错位与跨轮去重不变式**：
  1. 在并发 `steer`（改向）、`interrupt`（打断）或排队消息自动冲刷时，每一条 assistant 响应与 finalize 事件必须严格绑定并校验当前活跃 turn 上下文，严禁下一轮错收到上一轮残留输出（off-by-one lag）。
  2. 延迟到达的跨轮 finalize 消息必须在持久化与 WebSocket 广播路径（`crates/orbit-daemon/src/manager.rs` / `lib.rs`）正确归位或去重。
- **消息源解耦（`PromptOrigin`）与非打断排队生命周期**（Issue #52）：
  1. 驱动命令 `DriverCommand::SendPrompt` 显式区分 `PromptOrigin::Human` 与 `PromptOrigin::Agent`。仅人类主动改向（`Human + Steer`）可在 ACP / Antigravity 驱动上触发 `session/cancel` 抢占当前轮；来自子代理 / Peer 的异步汇报与唤醒（`Agent`）在 ACP / Antigravity 上强制走非打断 FIFO 排队（`Steer` 自动降级为 `Queue`），在 `Codex` / `Claude Code` / `Pi` 上统一通过本地/原生队列顺序投递且绝不在活跃轮中途取消正在执行的工作。
  2. 新增保队列中断 `DriverCommand::InterruptAndDrainQueue`（对应 `WireMessage::InterruptTurn { preserve_queue: true }`）：在人类判定上游长考停滞时终止当前卡死轮，但**保留**所有已排队消息继续冲刷；硬停止（`DriverCommand::Interrupt`）则取消当前轮并丢弃未投递队列。
  3. 全 6 个 Provider 驱动统一通过 `DriverEvent::PromptDeliveryUpdated` 上报权威状态流 `Queued -> Dispatched -> Settled / Dropped`（`Settled` 必须在 `StatusChanged(Idle)` 之前发射），由 `AgentOrchestrator` 持久化隐私安全日志（仅含 ID / turn ID / 时长）与时间线状态更新事件。
  - **必跑验证**：改动此区域必跑 `pitfall_01`、`pitfall_02`、`pitfall_06`、`pitfall_08`（见 `docs/engineering/testing-and-ci.md`）。

---

## 3. 大会话尾部分页与 Stage 载荷瘦身

为彻底消除千轮/万级条目会话在 iOS 与 Desktop 打开和流式输出时的卡顿，时间线采用**尾部分页 + 按需回溯 + 懒加载 Diff**：

1. **Wire 分页契约**：
   - `WireMessage::SubscribeTimeline` 支持携带 `tail_limit`，首次订阅只下发尾部窗口并返回 `has_older_history` 标记（分离 Wire 层历史分片与前端内存窗口状态）。
   - 向上滚动回溯历史时通过 `WireMessage::FetchTimelineBefore` 拉取更早区块，由 Daemon 返回 `WireMessage::TimelineHistoryPage`。
2. **Stage 投影与大块内容裁剪**（`crates/orbit-client-core/src/stage.rs`）：
   - 对超大 Tool Diff、终端输出与思考块做默认折叠与尺寸上限控制，按需懒加载展开，避免高频 token 增量触发整页重序列化。
   - **必跑验证**：改动分页/Stage/Store 序列查询必跑 `pitfall_03` 及 Desktop `mocked-tauri-paginated-large-session` Smoke 检查。

---

## 4. 非局域网（Off-LAN）Relay 长连接与多机同步

- **传输层与心跳**（`crates/orbit-relay-transport` / `crates/orbit-client-core` / `crates/orbit-daemon`）：
  - 远程流量经 `Noise_NKpsk0_25519_ChaChaPoly_SHA256`（序言 `orbit-relay-e2ee/2`）端到端加密，控制信道采用 `Noise_NK` Proof-of-Possession（`orbit-relay-pop/1`）防伪造顶号。
  - 客户端与 Relay 间保持可控心跳探测与重连（20s/65s 窗口）并在非桌面端（iOS/Android/HarmonyOS）提供无阻塞路由切换与 `401` 鉴权失效恢复。
- **跨主机 Relay 配置传播**：
  - 通过 `WireMessage::ConfigureHostRelay` 与 `apply_relay_to_fleet` 同步各主机的 Relay 地址并写入云端加密 Vault，使脱离局域网的移动端无需回到 Mac 旁也能与已配置机群重建连接。
  - **必跑验证**：改动连接池、Relay 配置分发或移动网络切换逻辑必跑 `pitfall_04`。

## 跨 Provider 项目记忆

Daemon 以本机规范化的工作目录隔离项目记忆。每条记录保存在 `.orbit/memory/<id 或 slug>.md`，YAML frontmatter 保存 ID、分类、审核状态、置信度、绑定的相对路径、标签、来源及时间戳，正文可直接用编辑器修改。相同的 Markdown ID 可以出现在不同 checkout。首次访问会从磁盘重建索引，后续访问检查变动文件。Watcher 处理编辑、改名与删除，并在绑定的源文件变化时把已确认记录改为 `needs_revalidation`，同时写回 Markdown。旧版 `~/.hermes/memories/*.md` 以只读假设导入，带 `workspace_root` 的条目属于指定目录，未指定的属于 `~/.hermes`。

SQLite 维持 `user_version = 6`。FTS5 的 BM25 排名与余弦相似度排名采用 RRF（常数 60）合并，再以状态、近期更新和访问情况微调。内置 256 维特征哈希支持英文词、标识符和中文字符片段。新建记忆与已有本地记录的余弦相似度高于 0.92 时合并。默认召回与首轮注入仅使用 `confirmed` 记录。初始问题宽泛时优先选取置信度高的架构与约定条目。注入内容只进入 Provider 的首轮输入，字符预算包含 `<orbit_project_memory>` 标签。

Core Pack 提供一个 `orbit_memory`，由调用 Agent 的工作目录确定作用域。四个 action 为 `recall`、`remember`、`confirm`、`forget`。AI 新建记录从 `hypothesis` 开始，显式 `confirm` 后参与首轮注入。`recall` 可传 `include_unconfirmed: true` 查看待核验记录。已配对客户端可按 `request_id` 调用 `ListProjectMemories`、`UpsertProjectMemory`、`ConfirmProjectMemory`、`DeleteProjectMemory`、`RecallProjectMemories`。

默认内置索引离线运行。可在本机 `$ORBIT_HOME/config.json` 中设置以下字段并重启 Daemon。启用后，Daemon 将记忆的标题、正文、标签及召回查询发送到配置的端点。密钥通过 `api_key_env` 指定的环境变量读取，本地无鉴权端点可省略此字段。请求超时为 3 秒，端点不可用时自动使用本地向量。

```json
{
  "memory_embedding": {
    "provider": "open_ai_compatible",
    "endpoint_url": "http://127.0.0.1:11434/v1/embeddings",
    "model": "nomic-embed-text",
    "auto_inject_budget_tokens": 1200
  }
}
```

## Host security and operations

The optional `sandbox_policy` in `config.json` applies to new agent processes, PTYs, ACP terminals, command hooks, goal commands and local MCP stdio servers. Authenticated clients can read and replace it with `GetSandboxConfig` and `UpdateSandboxConfig`. Modes are `off`, `workspace_write` and `strict_read_only`. An omitted policy preserves legacy execution. Network access defaults to disabled. When enabled, `allowed_domains` and `denied_domains` accept exact hosts and `*.example.com`. Explicit read exclusions extend the mandatory credential exclusions.

`GetAuditChainStatus` verifies the signed host history. `ExportAuditChain` returns ascending JSONL pages and a continuation sequence. `ExportDiagnosticBundle` returns redacted JSON directly to the authenticated caller. `orbit doctor --bundle PATH` also works on a stopped host. Local OpenMetrics scraping uses the paired HTTP bearer credential. Enable OTLP log batches with `OTEL_EXPORTER_OTLP_ENDPOINT` or `telemetry_endpoint` in the host configuration. The exporter appends `/v1/logs` to the collector base URL. `ORBIT_LOG_FORMAT=json` enables structured local logs.

Model prices are estimates in USD per million tokens. At settlement the host reloads optional `model-pricing.json` entries keyed by model or `provider/model`. Each entry contains `input_per_mtok`, `output_per_mtok`, `cache_read_per_mtok`, `cache_write_per_mtok` and `context_window_tokens`. The Gemini Pro table uses a 200K input boundary. GPT-5.4 uses its published 272K boundary. Set positive `daily_budget_usd` and `monthly_budget_usd` in `config.json` for UTC-period notifications. `auto_compact_threshold_pct` accepts 50 through 95. Omitting it disables automatic compaction. `CompactAgentContext` requests compaction for a specific idle agent.

`orbit migrate` previews local CLI and IDE imports. `--apply` writes the files in the report. `--from` selects Claude Code, Codex, Antigravity, Cursor or VS Code. `--cwd` selects the project. `--conflict skip|overwrite|rename` resolves name conflicts. The authenticated inspection and execution RPCs authorize project paths against the host's registered workspaces. Project MCP entries stay in project scope. The import report contains names and paths only.

On Windows, `orbit daemon up` registers an interactive-token task for the current Windows user. `orbit daemon up --service` and `orbit relay up` install LocalService SCM services with restart recovery. Service installation requires permission to manage the SCM. The SCM Stop handler signals the daemon shutdown path so detached shims can continue running.

Built-in token estimates use standard API rates. Local model overrides take precedence for negotiated contracts and regional deployments. [Anthropic pricing](https://platform.claude.com/docs/en/about-claude/pricing), [GPT-5.4 pricing](https://developers.openai.com/api/docs/models/gpt-5.4), [Gemini pricing](https://ai.google.dev/gemini-api/docs/pricing), [DeepSeek pricing](https://api-docs.deepseek.com/quick_start/pricing/), [Qwen pricing](https://www.alibabacloud.com/help/en/model-studio/model-pricing).

## WASM 插件运行时

WASM 插件由 Daemon 在独立的限额阻塞任务中执行。插件 RPC、宿主能力、执行预算与按需 MCP 工具发现见 [Plugin SDK](plugins.md)。
