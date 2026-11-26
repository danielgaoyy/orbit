# Daemon、Shim、Provider 驱动与大会话性能架构

本文档管辖 `orbit-daemon`、`orbit-shim`、`orbit-providers`、`orbit-protocol` 及跨主机长连接/分页的核心工程契约。改动下列任意机制前必须先通读本规范。

---

## 1. Daemon 与 Shim 进程隔离红线

- **独立进程壳**：`crates/orbit-shim` 通过 `setsid` 创建独立会话组，借助 `portable-pty` 与 stdio pipe 托管长寿命有状态 Agent 进程（`claude`、`codex`、`agy`、`pi`、ACP 引擎）与用户交互终端 `PTY`。`run_pty_shim` 采用独立写线程隔离阻塞 I/O 以保证 `ShimFrame::Kill` 永不饿死，并在退出时经 `ORBIT_SHIM_SESSION_ID` 与 async `child.wait()` 清理全部子进程（零 `<defunct>` / 孤儿 `zsh`）；Daemon 持有 `daemon.lock` 文件锁、`daemon.pid` 与活体 `daemon.relay`，自重启或 `--listen` 变更时严格保持原 binary 路径与监听参数并杜绝双 Daemon 竞争。
- **Unix Socket 通信**：Daemon 通过 `$ORBIT_HOME/shims/<shim-id>.sock` 与 Shim 双向通信并记录游标偏移，确保升级或重启 `orbit daemon` 时零中断重新附着正在运行的会话；终端重连二进制帧 `Snapshot` 在线协议边界统一前缀归零清屏序列（不篡改客户端原始字节流合同）。
- **Unix 进程组终止安全不变式**：在 `unsafe_code = "deny"` 约束下调用外部 `/bin/kill` 终止子进程或进程组（`crates/orbit-shim`、`crates/orbit-daemon/src/platform.rs`、`crates/orbit-providers`）时，**必须**先校验 `pid > 1` / `pgid > 1`，并在负进程组 ID 前强制传入 `--` 选项终止符（如 `/bin/kill -s TERM -- -<pgid>`），严禁省略 `--`（Linux `procps-ng 4.0.4` 的 `getopt_long` 会把未加 `--` 且以 `1` 开头的 `-1xxxxxx` 误解析为 `kill(-1, ...)`，误杀同 UID 下包括 `systemd --user` 与桌面会话在内的全部用户进程；由 `pitfall_07` 回归拦截）。
- **单二进制内聚**：全部 `daemon` / `shim` / `relay` / CLI 子命令打包在单一静态 `orbit` 可执行文件（`crates/orbit-cli`）中，不得依赖外部 Node.js / Python 运行时。

---

## 2. Provider 驱动与消息零错位契约（Zero Off-By-One）

- **Native 驱动**（`crates/orbit-providers/src/`）：
  - `claude.rs`（`stream-json` 控制协议，私钥 MCP 配置写入 `$ORBIT_HOME/runtime/claude-mcp-*.json` `0600`）
  - `codex.rs`（`app-server` JSON-RPC 2.0）
  - `antigravity.rs` + `agy_watcher.rs`（`agy` ACP stdio + 字节偏移 `transcript_full.jsonl` 流：进程级单实例 `notify` 托管于独立 `orbit-agy-watcher` 控制线程与无锁路由表隔离以根除 `notify` 回调与 `watch()`/`unwatch()` 间的 AB-BA 死锁、非递归叶子目录事件监听替代 200ms 轮询、`wakeup_idle_settled` / `has_in_progress_planner` 迟到唤醒回复零丢包恢复、`persisted_initializing` 跨重启重连脱离卡死并补发未送达提示、以及 `PostWakeupAcpReplayFilter` + `session/load` `0ms` 非阻塞清空的跨轮 ACP 重放幂等去重）
  - `pi.rs`（`pi-rpc`，私钥 MCP 配置写入 `$ORBIT_HOME/runtime/mcp-pi-*.json` `0600`，经环境变量与 `cleanup_paths` 托管，零 `argv` 凭据泄露）
- **通用 ACP 生态与客户端回调**：`acp_registry.rs`、`acp_providers.rs`、`acp_generic.rs`、`acp_callbacks.rs` 与 `readiness.rs`（34+ ACP Agent 目录，统一通过 `acp_callbacks::AcpCallbackHost` 声明并实现工作区隔离的 `fs/read_text_file`、`fs/write_text_file` 及独立进程组 `terminal/*` 回调，支持 `Drop` 自动清理）。
- **Daemon 异步隔离、OS 文件监听与 SEP-1865 MCP Apps**：
  - `crates/orbit-daemon/src/manager.rs` 全部异步路径通过 `with_store` / `try_store`（`tokio::task::spawn_blocking`）将同步 SQLite 读写移出 Tokio worker 线程。
  - `crates/orbit-daemon/src/workspace_watcher.rs` 基于 `notify` 提供 250ms 防抖、忽略构建/对象目录并限流的文件与 `.git/HEAD`/`index` 监听，自动广播 `WireMessage::WorkspaceFilesChanged` 并失效符号/规则缓存。
  - `crates/orbit-daemon/src/mcp_servers.rs` 与 `artifacts.rs` 支持 SEP-1865 `ui://` 资源 `resources/read` 自动拉取与严格绑定归属 MCP Server 的 `tools/call`（`WireMessage::ReadMcpResource` / `CallArtifactMcpTool`）。
  - **按需延迟工具发现（`orbit_tool_search`）与严格 MCP Schema 门禁**（`crates/orbit-daemon/src/tools.rs`、`mcp.rs`、`guardian.rs`）：会话启动始终锁定精简 `Core` Pack（含 `orbit_tool_search`，零首轮提示词关键词猜测、零 `tools/list` 中途变更以保护 Prompt Cache；当会话存在活跃/暂停 `/goal` 时自动随 `resolve_effective_packs` 暴露 `orbit_goal`），7 个扩展能力 Pack（`orbit_browser`、`orbit_schedule`、`orbit_workflow`、`orbit_goal`、`orbit_terminal`、`orbit_artifact`、`orbit_lsp`）对齐 Claude Code `ToolSearch` / Codex `tool_search` 语义通过 `orbit_tool_search(action="search", query=...)` 按需检索 Schema 并经 `orbit_tool_search(action="call", tool=..., arguments=...)` 透传执行（严格复用目标工具的 `McpToolPolicy`、`PreToolUse`/`PostToolUse` Hook 与权限校验，Guardian 仅放行只读 `search` 与 `orbit_goal` 内部记账动作 `get_goal`/`update_goal`/`pause_goal`/`complete_goal`）。全部内置 MCP 工具受 `all_mcp_tool_schemas_satisfy_strict_provider_validation` 递归单测门禁约束（兼容 Gemini / OpenAI 严格规范：数组必填 `items`、对象必填非空 `properties`、禁止无 `type` 空 Schema）。
  - **自主目标引擎（`/goal`）与双轨完备性门禁**（`crates/orbit-daemon/src/goals.rs`、`crates/orbit-protocol/src/goals.rs`、`crates/orbit-store/src/goals.rs`）：
    1. **原生气泡呈现与草稿直发**：用户发送 `/goal <目标>`（含新建草稿会话通过 `CreateAgent` `initial_badges` 原子注册）始终以干净的用户消息气泡呈现并附 `/goal` 标签，隐藏 `<GOAL_DIRECTIVE>` / `<WORKFLOW_INSTRUCTION>` 仅下发给模型，彻底消除折叠 Heartbeat 卡片与内部 `Budget:` 脚手架泄露；仅后台自主续跑/校验失败/在线修订使用折叠 `Target` 节点。
    2. **默认无墙钟硬杀与可恢复 `Paused` 状态机**：默认 `max_duration_secs = 0`（无默认 1 小时墙钟强制中断，持续累计活跃耗时 `active_duration_secs` 并按人类可读时长展示），默认轮次预算 25（可上限 200）；达到轮次/显式时长上限、连续 3 次同源校验失败、用户中断（`InterruptTurn`）或 Agent 运行错误（`AgentStatus::Error`）时一律转入可恢复 `GoalStatus::Paused`（记录 `pause_reason` 与 `paused_by_user`），点击 Resume 自动重置失败计数并顺延预算，系统自动暂停状态在用户补发消息时自动恢复（手动暂停除外），存在排队人类消息时延迟续跑。
    3. **在线编辑、替换与双轨完成校验**：支持 `WireMessage::UpdateGoalRun` 在线修订目标/验收条件/校验命令（以 `<untrusted_objective>` 引导运行中会话）、`StartGoalRun.replace_existing` 原子替换、`orbit_goal` 新增 `get_goal` / `pause_goal`；配置 `verify_command` 时执行确定性命令校验，未配置时调用独立零工具 LLM 评估器（`HardenedCliGoalEvaluator`，随机 nonce 防注入 + 严格 `{ok, impossible, reason}` 裁决）防止模型提前宣称完成。
- **防轮次错位与跨轮去重不变式**：
  1. 在并发 `steer`（改向）、`interrupt`（打断）或排队消息自动冲刷时，每一条 assistant 响应与 finalize 事件必须严格绑定并校验当前活跃 turn 上下文，严禁下一轮错收到上一轮残留输出（off-by-one lag）。
  2. 延迟到达的跨轮 finalize 消息必须在持久化与 WebSocket 广播路径（`crates/orbit-daemon/src/manager.rs` / `lib.rs`）正确归位或去重。
- **消息源解耦（`PromptOrigin`）、权限闸门与非打断排队/引导生命周期**（Issue #52）：
  1. 驱动命令 `DriverCommand::SendPrompt` 显式区分 `PromptOrigin::Human` 与 `PromptOrigin::Agent`。**`ActiveTurnBehavior::Steer`（用户引导）在全 6 个 Provider 上均为非破坏性操作，严禁发送 `session/cancel` / `turn/interrupt` / `abort` 强杀运行中的子代理或后台常驻任务**：
     - **权限/问题/计划闸门统一契约（Option A）**：当会话存在未决的权限审批、提问（`QuestionRequest` / `elicitation/create`）或 Codex 合成计划闸门（`pending_synthetic_plan_gate`）时，全 6 个 Provider 驱动对 `ActiveTurnBehavior::Steer` **一律不丢弃、不向底层写入 mid-gate 引导帧、不取消审批卡**，而是无损排入高优人类队列（`pending_human_steers`，优先于常规队列 `queued_prompts` / `pending_prompts`），待全部闸门放行且当前轮结算后自动顺位冲刷；仅显式 `ActiveTurnBehavior::Interrupt`、`DriverCommand::Interrupt` 或 `DriverCommand::InterruptAndDrainQueue` 才允许拒绝/取消未决闸门（并立即解除 Codex 合成计划闸门或直接冲刷替代轮，严禁死锁）。
     - **多闸门并发状态不变式**：Claude、Codex、Antigravity、通用 ACP 与 Pi 在单条审批/问题/计划闸门解析后，**仅当全部未决权限与问题计数归零**才恢复 `Running` / `AutonomousWakeup` / `Idle`；若仍有剩余闸门，必须保持 `AwaitingPermission` 并指向下一条待审批 `request_id`。
     - `Antigravity`（`agy acp`）对人类 `Steer` 使用原生并发 `session/prompt` 注入（记录旧 RPC 于 `superseded_prompt_rpcs`，零 `session/cancel`、零清理 `bg_tracker` / `subagent_monitors`；若旧版/严格服务端报错拒绝并发则自动回退到高优人类队列）。在仅等待后台任务（`autonomous_turn_id = bgwait_*`，`is_background_waiting() == true`）或 open-RPC 后台定时器等待（`foreground_bg_wait`）时直接投递新 `session/prompt` 且保留全部后台跟踪；若前轮在 early thinking 且尚无任何 assistant 输出时被 `DriverCommand::Interrupt` 打断，切换全新干净 ACP 会话并用已提交轮次重播 handoff 以彻底隔离取消轮污染；在活跃后置自主唤醒（`autonomous_turn_id = wakeup_*`，`is_foreground_busy() == true`）期间，`Queue` / Agent `Steer` 入队等待唤醒结算而不静默吞掉未读转录，`Human + Steer` 先 flush 发射未读唤醒转录增量再并发注入，`Interrupt` / `InterruptAndDrainQueue` 真正下发 `session/cancel`。
     - `Claude Code` 区分人类与 Agent 引导：`PromptOrigin::Human` 的 `Steer` 串行排入 `pending_human_steers` 并在前台活跃时发一次 `cancel_queued: false` 中断以彻底消除连续多次人类引导的 `"result"` 错位，同时通过 `/internal/pre-tool-use`（`orbit internal-pre-tool-use`）保证显式 `Deny` 规则与 `PreToolUse` Hook 在自放行命令前生效；在前台轮结束（`"result"`）但仍有运行中后台任务（`bg_bash_tasks`）时，统一对齐 `waiting:*` 契约发射 `AgentStatus::AutonomousWakeup { turn_id: "bgwait_*", source: "waiting:..." }`。
     - `Codex` 在开启严格工具门控时强制传入 `"untrusted"` 审批策略以拦截自放行命令、`Pi` 在无未决闸门且处于活跃前台轮时走各自原生非破坏性 mid-turn steer（Codex 在 `turn/started` 前的启动窗口内将人类 `Steer` 排入 `pending_human_steers` 优先冲刷）；通用 ACP 驱动（`acp_generic.rs` / `acp_providers.rs`）因无原生并发 steer，统一将人类 `Steer` 降级为高优非破坏性队列（优先于 Agent 队列顺位投递，零 `session/cancel`）。
     - 跨 `/compact` 与 rewind 的上下文 handoff（`build_handoff_context_from_store` / `compact_agent`）强制保留 `timeline_epoch.max(1)` 之后的全部用户原话。
     - 来自子代理 / Peer 的异步汇报与唤醒（`PromptOrigin::Agent`）在 ACP / Antigravity 上强制走非打断 FIFO 排队（`Steer` 自动降级为 `Queue`）；`dispatch_coalesced_batch` 在活跃前台 `AutonomousWakeup`（`!is_background_waiting()`）时同样填写 `blocking_turn_id`。
     - 队列条“引导”（`flush_queued_user_prompt_now`）在无阻塞权限/问题闸门（`has_pending_human_gates == false`）时走 `ActiveTurnBehavior::Steer`，仅在存在未决权限或问题闸门时走 `ActiveTurnBehavior::Interrupt` 解除阻塞。
  2. **Daemon 服务端自动冲刷与错误隔离不变式**（`crates/orbit-daemon/src/manager.rs`）：
     - `has_pending_human_gates` 统一检查 `pending_permissions` 与 `pending_questions`，用于 `enqueue_user_prompt`、`flush_queued_user_prompt_now`、自动压缩、空闲清理及自动冲刷守卫；进入 `AgentStatus::Error` 时必须同步清理残留的 `pending_permissions` / `pending_questions`，避免后续入队被陈旧闸门永久阻塞。
     - 服务端队列自动冲刷（`Idle` / `waiting:*` 触发）必须按顺序执行：① 复核无人类闸门、未被 `suppress_auto_drain` 抑制且仍为前台空闲；② 预检 `PrePromptSubmit` Hook（拦截时保留队列项并写入时间线告警）；③ 经 `get_or_resume_driver` 确保驱动存活后再原子消费 SQLite 队列行；④ 若最终派发瞬时失败，通过 `requeue_agent_prompt_front` 原位回插队首，严禁静默丢消息。
  3. 新增保队列中断 `DriverCommand::InterruptAndDrainQueue`（对应 `WireMessage::InterruptTurn { preserve_queue: true }`）：在人类判定上游长考停滞时终止当前卡死轮，但**保留**所有已排队消息继续冲刷；硬停止（`DriverCommand::Interrupt`）则取消当前轮并丢弃驱动内未投递队列（且仅在存在活跃轮/闸门时设置 `suppress_auto_drain`）。
  4. 全 6 个 Provider 驱动统一通过 `DriverEvent::PromptDeliveryUpdated` 上报权威状态流 `Queued -> Dispatched -> Settled / Dropped`（`Settled` 必须在 `StatusChanged(Idle)` 之前发射），由 `AgentOrchestrator` 持久化隐私安全日志（仅含 ID / turn ID / 时长）与时间线状态更新事件。
  - **必跑验证**：改动此区域必跑 `pitfall_01`、`pitfall_02`、`pitfall_06`、`pitfall_08`、`pitfall_09`、`pitfall_11`、`pitfall_12`、`pitfall_13`、`pitfall_14`（见 `docs/engineering/testing-and-ci.md`）。

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

Personal Agent permission recommendations and auto-handled decision logs share the same signed audit chain and live permission snapshot:
- `PendingPermissionSummary` carries an optional backward-compatible `router_recommendation: Option<RouterRecommendation>` (`decision`, `reason`, `created_at`).
- `orbit_control(action="recommend_permission", agent_id, request_id, decision, reason, host_id?)` validates that the caller spawned the target worker, attaches the non-binding recommendation to the in-memory pending entry, and broadcasts `PendingPermissionsSnapshot` (forwarding across hosts via `WireMessage::RecommendPermission`). In `assist_only` mode or for Tier-3 (`High`/`Critical`, `human_only`, or plan approval) gates, the Personal Agent recommends rather than resolving directly.
- Successful automatic decisions by the Personal Agent (`respond_to_permission` locally or remotely, logged on the Personal Agent host with target `host_id`), Guardian Tier-1 (`AutoAllow` / `AutoDeny`), and persistent permission rules append `category = "personal_agent_decision"` entries to `security_audit_chain`. Clients query them newest-first (capped at 200) via `WireMessage::ListPersonalAgentActivity { request_id, limit, since_ms }` -> `WireMessage::PersonalAgentActivitySnapshot { request_id, entries }`.

Model prices are estimates in USD per million tokens. At settlement the host reloads optional `model-pricing.json` entries keyed by model or `provider/model`. Each entry contains `input_per_mtok`, `output_per_mtok`, `cache_read_per_mtok`, `cache_write_per_mtok` and `context_window_tokens`. The Gemini Pro table uses a 200K input boundary. GPT-5.4 uses its published 272K boundary. Set positive `daily_budget_usd` and `monthly_budget_usd` in `config.json` for UTC-period notifications. `auto_compact_threshold_pct` accepts 50 through 95. Omitting it disables automatic compaction. `CompactAgentContext` requests compaction for a specific idle agent.

`orbit migrate` previews local CLI and IDE imports. `--apply` writes the files in the report. `--from` selects Claude Code, Codex, Antigravity, Cursor or VS Code. `--cwd` selects the project. `--conflict skip|overwrite|rename` resolves name conflicts. The authenticated inspection and execution RPCs authorize project paths against the host's registered workspaces. Project MCP entries stay in project scope. The import report contains names and paths only.

On Windows, `orbit daemon up` registers an interactive-token task for the current Windows user. `orbit daemon up --service` and `orbit relay up` install LocalService SCM services with restart recovery. Service installation requires permission to manage the SCM. The SCM Stop handler signals the daemon shutdown path so detached shims can continue running.

Built-in token estimates use standard API rates. Local model overrides take precedence for negotiated contracts and regional deployments. [Anthropic pricing](https://platform.claude.com/docs/en/about-claude/pricing), [GPT-5.4 pricing](https://developers.openai.com/api/docs/models/gpt-5.4), [Gemini pricing](https://ai.google.dev/gemini-api/docs/pricing), [DeepSeek pricing](https://api-docs.deepseek.com/quick_start/pricing/), [Qwen pricing](https://www.alibabacloud.com/help/en/model-studio/model-pricing).

## WASM 插件运行时

WASM 插件由 Daemon 在独立的限额阻塞任务中执行。插件 RPC、宿主能力、执行预算与按需 MCP 工具发现见 [Plugin SDK](plugins.md)。

## Host-local voice RPCs

Each paired host owns its voice settings in `$ORBIT_HOME/voice.json`. Voice is disabled by default. Enable it with `UpdateVoiceConfig` or set `"enabled": true` in the host file. `GetVoiceConfig`, updates, downloads and deletions return the current settings, model inventory and available host executables. These operations use the existing authenticated direct and relay transports. The CLI uses the same RPCs.

```sh
orbit voice status
orbit voice models
orbit voice download whisper-base-q5
orbit voice transcribe recording.wav
```

Capture RPCs accept base64 little-endian mono PCM16. The CLI also reads mono/stereo PCM16 WAV. Inputs are limited to 10 MiB, 120 seconds and 8–96 kHz. The daemon resamples to 16 kHz and evaluates 20 ms frames using RMS, adaptive noise floor, zero crossings and high-frequency energy. A ring retains the 300 ms immediately preceding onset. The default silence hangover is 750 ms. The optional Silero v5 ONNX file is available in the catalog for external inference runtimes. The built-in detector runs without weights.

| Local backend | Download ID | Optional executable |
| --- | --- | --- |
| Whisper.cpp | `whisper-base-q5` | `whisper-cli` or `whisper.cpp` |
| SenseVoice | `sensevoice-small-int8` | `sherpa-onnx-offline` |
| Kokoro | `kokoro-82m-q8` | `kokoro-tts` with ONNX support |

Set the backend and the corresponding model ID together. Each catalog entry pins the primary weight and every required sidecar. Downloads stage the bytes in private temporary files, check length and SHA-256, and publish by rename under `$ORBIT_HOME/models/voice/<id>/`. Inventory and local inference recheck the installed files.

Local recognition falls back to a configured OpenAI-compatible audio endpoint and finally to client STT. Selecting `open_ai_compatible` starts at the endpoint. Selecting `system_client_fallback` hands capture directly back to the client. Local synthesis tries the configured endpoint and then the host's WAV writer. Native output uses macOS `say`, Windows PowerShell SAPI or Linux `espeak-ng` / `espeak`. The result contains WAV bytes for playback on the requesting device. A host with only a playback-only synthesizer delegates playback to the client. Speech text is reduced to at most 2,000 characters after Markdown cleanup.

The endpoint accepts a base such as `http://127.0.0.1:8000/v1`. Orbit posts multipart WAV to `audio/transcriptions` and requests WAV from `audio/speech`. With a local model ID the remote defaults are `whisper-1` and `tts-1`. With the OpenAI-compatible backend the model ID may name a model served by the endpoint. Credential binding is documented in [SECURITY.md](../../SECURITY.md).

## Direct mobile push

Place provider settings in a private `$ORBIT_HOME/push.json` file or set the corresponding uppercase `ORBIT_` environment variables. Environment values override the file. Each platform with configured direct credentials uses its vendor endpoint. Other platforms can use the existing push gateway. Provider keys belong to the daemon operator.

| Platform | Configuration fields | Delivery |
| --- | --- | --- |
| APNs | `apns_key_path`, `apns_key_id`, `apns_team_id`, `apns_bundle_id`, `apns_sandbox` | HTTP/2, ES256 provider token refreshed after 50 minutes. The device token receives alerts. An additional ActivityKit token receives updates under the Live Activity topic. |
| FCM | `fcm_service_account_path` | Firebase Messaging scope, RS256 assertion, OAuth token exchange and HTTP v1 project endpoint. |
| HarmonyOS NEXT | `hms_service_account_path` | Huawei service-account PS256 JWT, v3 notification endpoint with `push-type: 0` and the `WORK` category. |
| Huawei client credentials | `hms_client_id`, `hms_client_secret`, `hms_project_id` | OAuth client-credentials exchange for deployments accepting that bearer. NEXT requires service-account JWT authentication. |

Register or rotate an ActivityKit token using the host and device IDs. Passing null to the dedicated token-update RPC clears it. The ActivityKit `content-state` uses the `PushDispatchPayload` fields. Provider acceptance is reported per device. The notification privacy preference controls optional previews. [Huawei authentication](https://developer.huawei.com/consumer/en/doc/harmonyos-guides/push-jwt-token/) · [Apple ActivityKit updates](https://developer.apple.com/documentation/activitykit/starting-and-updating-live-activities-with-activitykit-push-notifications)

## Inline prediction and PR Patrol

FIM is off by default. Each host stores its policy in `fim-config.json` under Orbit home. `GetFimConfig` and `UpdateFimConfig` return `FimConfigState`. The optional wire `request_id` correlates concurrent configuration calls. A prediction accepts up to 16 KiB before the cursor and 8 KiB after it. The daemon sends that text to the configured Ollama or OpenAI-compatible endpoint, or to the isolated CLI when fallback is enabled and the endpoint is absent. The timeout covers DNS, transport and inference. Subtle and eager control client scheduling.

`RunPrPatrol` compares immutable commits from their merge base. The daemon reads base-commit rules, reviews added lines, and verifies candidates against head blobs. The report records the resolved commits, skipped files, discovery method and discarded candidates. An authenticated Claude Code CLI with an empty tool list, strict MCP configuration, and safe mode can assist discovery and independently assess model-only claims. The read-only static pass also works offline. Posting requires the PR number and the reviewed PR revisions. Each inline comment carries a stable marker. See [the opt-in CI template](../../.github/workflows/orbit-pr-review.yml.example).
