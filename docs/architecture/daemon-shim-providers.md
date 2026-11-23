# Daemon、Shim、Provider 驱动与大会话性能架构

本文档管辖 `orbit-daemon`、`orbit-shim`、`orbit-providers`、`orbit-protocol` 及跨主机长连接/分页的核心工程契约。改动下列任意机制前必须先通读本规范。

---

## 1. Daemon 与 Shim 进程隔离红线

- **独立进程壳**：`crates/orbit-shim` 通过 `setsid` 创建独立会话组，借助 `portable-pty` 与 stdio pipe 托管长寿命有状态 Agent 进程（`claude`、`codex`、`agy`、`pi`、ACP 引擎）与用户交互终端 `PTY`。
- **Unix Socket 通信**：Daemon 通过 `$ORBIT_HOME/shims/<shim-id>.sock` 与 Shim 双向通信并记录游标偏移，确保升级或重启 `orbit daemon` 时零中断重新附着正在运行的会话。
- **单二进制内聚**：全部 `daemon` / `shim` / `relay` / CLI 子命令打包在单一静态 `orbit` 可执行文件（`crates/orbit-cli`）中，不得依赖外部 Node.js / Python 运行时。

---

## 2. Provider 驱动与消息零错位契约（Zero Off-By-One）

- **Native 驱动**（`crates/orbit-providers/src/`）：
  - `claude.rs`（`stream-json` 控制协议）
  - `codex.rs`（`app-server` JSON-RPC 2.0）
  - `antigravity.rs`（`agy` ACP stdio + 字节偏移 transcript 流）
  - `pi.rs`（`pi-rpc`）
- **通用 ACP 生态**：`acp_registry.rs`、`acp_providers.rs`、`acp_generic.rs` 与 `readiness.rs`（34+ ACP Agent 目录及实时 `initialize` 探测）。
- **防轮次错位与跨轮去重不变式**：
  1. 在并发 `steer`（改向）、`interrupt`（打断）或排队消息自动冲刷时，每一条 assistant 响应与 finalize 事件必须严格绑定并校验当前活跃 turn 上下文，严禁下一轮错收到上一轮残留输出（off-by-one lag）。
  2. 延迟到达的跨轮 finalize 消息必须在持久化与 WebSocket 广播路径（`crates/orbit-daemon/src/manager.rs` / `lib.rs`）正确归位或去重。
  - **必跑验证**：改动此区域必跑 `pitfall_01`、`pitfall_02`、`pitfall_06`（见 `docs/engineering/testing-and-ci.md`）。

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
