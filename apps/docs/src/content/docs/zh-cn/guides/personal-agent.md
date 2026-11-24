---
title: Personal Agent 与全局调度
description: 通过唯一常驻的 Personal Agent 跨主机调度 Claude Code、Codex、Antigravity 与 ACP 子代理，并经零公网 Webhook 长连接直连飞书、Slack 与 Discord。
---

Orbit 最核心的范式革新是 **Personal Router（`AgentRole::PersonalRouter` / 常驻分工师）**——一个跨越多台机器、全天候在线的 **Personal Agent**，充当你的 AI 工程总参谋。

与其在本地终端、SSH 窗口和浏览器之间充当「人工剪贴板」，你只需与这**一个常驻总控**对话：它能直接看到全部在线主机上的项目与子代理状态，自动按特长并行派单、挂起条件唤醒定时器，并且只在需要人类拍板的高风险节点推送决策。

## 全局主主机选举与跨机上下文注入

- **任意在线主机皆可担任总控**：在多主机舰队中显式指定某一台常驻主机（如 Mac Studio、家用 Mac mini 或云端 Devbox）作为全局唯一 Personal Agent，配置保存于 `primary_router_selection` 并经加密云同步在各端一致生效。
- **增量舰队态 `<orbit-fleet-state>` / `<orbit-fleet-delta>`**：`PersonalRouterManager` 持续聚合所有在线主机的仓库、活跃 Mission Workspace、Worker 进度与待审批检查点，仅以紧凑增量注入提示词，不浪费上下文预算。
- **免 SSH 远端上下文探测**：派单前，Personal Agent 直接通过 Core Pack 的 `orbit_control(action="list_projects" | "inspect_project_context")` 拉取任意远端主机的 `AGENTS.md`、`README.md`、目录树与当前 Git 分支，无需人工登录 SSH。
- **无缝承接既有记忆**：默认开启 `share_host_hermes_memory = true`，无需在 `orbit` 内嵌入 Python / Node.js 即可兼容读写宿主已有的 `~/.hermes/memories/` 长期记忆。

## 并行 Mission Workspace 与非打断消息投递

当需求横跨多个模块或物理机时，Personal Agent 基于 Orbit 的并行优先不变量编排子代理：

1. **Git Worktree 自动隔离**：通过 `orbit_agent(action="fork", worktree=true)` 在独立工作树分化并行专家，自动标记 `orbit.worktree_branch` 与 `orbit.worktree_source_cwd`，彻底消除多 Agent 在同一目录互相覆盖未提交文件的风险。
2. **共享看板 `BOARD.md` 乐观锁（CAS）**：成员通过分节局部更新 + `expected_revision` 乐观并发控制协同读写架构决策与进度，绝不发生全量覆盖冲突。
3. **消息源解耦（`PromptOrigin`）与非打断排队**：只有人类主动改向（`PromptOrigin::Human`）会抢占当前执行轮；子代理完成汇报与 Peer 异步消息（`PromptOrigin::Agent`）强制走非打断 FIFO 排队（`Queued -> Dispatched -> Settled`），绝不在总控思考中途粗暴打断。
4. **条件定时器空闲可对话不变量**：仅持有待触发唤醒定时器（`has_armed_timers: true`）的会话与子代理保持 Idle 可随时继续插话，绝不阻塞输入框。
5. **`@handle` 历史别名解析**：重命名 Agent Handle 时，在同一 SQLite 事务内将旧 Handle 原子追加入 `labels["orbit.handle_aliases"]`，确保旧消息路由与 `sender:<handle>` 条件定时器永不失效。

## 三级 Guardian 风险审批闸门

为了兼顾高度自治与绝对安全，Orbit 在协议层强制执行三级风险分层：

| 级别 | 风险分类 | 处理路径 |
| --- | --- | --- |
| **Tier 1** | 低危（只读探索、工作区测试、白名单命令） | 由 `GuardianManager` 自动放行并记录审计日志。 |
| **Tier 2** | 中危（受限文件改动、依赖安装、分支操作） | 由 Personal Agent 或 LLM Tier-2 分类器结合工作区上下文预审。 |
| **Tier 3** | 高危 / 极危（凭据读取、破坏性命令、生产发布、密钥轮换） | **协议层硬禁止自动放行**，强制交由人类在原生 DecisionDock 或 IM 交互卡片拍板。 |

## 零公网 Webhook 的 IM 实时长连接

Orbit 内置纯 Rust 主动出站 WebSocket 运行器（`crates/orbit-daemon/src/im_longconn.rs`），即使你的机器位于家庭宽带或企业内网 NAT 之后，**无需公网 IP、无需反向代理、无需暴露任何 Webhook 端口**即可双向直连主流 IM：

- **飞书 / Lark 长连接**：主动连接 `/callback/ws/endpoint`，内置紧凑二进制 `pbbp2.Frame` Protobuf 编解码、分片重组、`< 3s` 极速 ACK 与事件去重；支持 `ImRoutingMode::PersonalRouter` 直达常驻总控，并将 **Tier-3 审批**渲染为可点选的飞书交互卡片。
- **Slack Socket Mode（WSS）**：主动调用 `/api/apps.connections.open`（`xapp-...`），支持 `events_api` 与 `interactive` 交互载荷并即时回执 `envelope_id`。
- **Discord Gateway v10（WSS）**：完整实现 `HELLO` / 抖动 `HEARTBEAT` 看门狗与 `IDENTIFY` / `RESUME` 断线续传，并在开启 HTTP Webhook 时执行严格的 `ed25519-dalek` 失败关闭验签。

所有长连接 URL 中的查询凭据（如飞书 `access_key` 或 Slack `ticket`）在写入日志与状态前均会自动脱敏。
