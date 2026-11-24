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
