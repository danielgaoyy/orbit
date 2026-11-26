# Mission Workspace 并行协作、Worktree 隔离与 Handle 别名架构

本文档定义多 Agent 在同一 Mission Workspace 内安全并发修改代码的工程约束。涉及协作看板、Worktree 隔离、Peer 通讯与定时器改动的 PR 必须遵守下列不变量。

---

## 1. 并行优先（Parallel-First）与 Worktree 自动隔离

- **禁止无故阻塞**：当多个 Peer 被分配同一项目内不同职责时，应基于同一基线尽早分化独立 `git worktree` 并行推进，而非串行排队等他人结束，也不得在同一检出目录直接混改彼此的未提交文件。
- **Worktree 隔离标签**：在精编 Mission Workspace 中通过 `orbit_agent(action="fork", worktree=true)` 分叉出的新成员应保留在同一个 `workspace_id` 下继续协同，并在 `labels` 中携带 `orbit.worktree_branch` 与 `orbit.worktree_source_cwd`（通过 `resolve_git_main_checkout_root` 规范化主仓库根目录以支持嵌套 worktree 分叉），发出 `PeerJoined` 事件（实现位于 `crates/orbit-daemon/src/manager.rs`、`tools.rs`、`mcp.rs`）。
- **双向 Handoff**：工作树修改完成后，通过 `worktree_handoff.rs` 提供的安全备份 ref 与冲突感知策略（`apply` / `move` / `dry_run`）合并回目标本地工作区；`PreviewWorktreeHandoff` / `ExecuteWorktreeHandoff` 支持传入 `source_agent_id` 以精确绑定源 Peer 的隔离工作树。

---

## 2. 共享看板 `BOARD.md`、双向目标同步与阶段重置（`reset_board`）

- 每个 Mission Workspace 维护共享 `BOARD.md`（由 `crates/orbit-daemon/src/workspace_board.rs` 与 `crates/orbit-store/src/workspace_sync.rs` 管理）。
- **分节局部更新 + 乐观并发控制**：修改特定 `## Section` 时应传入 `section` 与 `expected_revision`，通过 Compare-And-Swap（CAS）避免并发全量覆盖他人刚写入的决策与进展。
- **`mission_goal` 与 `## Goal` 原子双向同步**：`update_workspace_goal_and_board` 与 `upsert_workspace_board_with_cas` 在同一 SQLite 事务内保持 `workspaces.mission_goal` 与 `BOARD.md` 中 `## Goal` 的一致性，避免创建期目标锁定。
- **长期复用阶段重置（`reset_board` + `BOARD.history.md`）**：长期复用的 Workspace 可通过 `orbit_workspace(action="reset_board")` 或 `WireMessage::ResetWorkspaceBoard` 开启新阶段——若旧看板包含实质笔记，先追加备份至 `$ORBIT_HOME/workspaces/<ws_id>/BOARD.history.md`，再写入干净的 `BOARD.md`（新目标 + 可选保留约定）并推进 `workspaces.board_reset_seq` 文件编辑/冲突水位线，**同时保持左侧边栏中的历史会话完全不变**。

---

## 3. `@handle` 重命名历史别名解析（Historical Handle Alias）

Mission Workspace 中会话标题与 `@handle` 会根据进展自动或手动更新，**严禁改名后导致旧消息、旧路由或旧定时器的 `@old-handle` 查找失败**：

1. **原子持久化别名表**：
   - `crates/orbit-store/src/workspace_sync.rs` 中的 `set_agent_handle`、`set_agent_semantic_identity_with_options` 与 `crates/orbit-store/src/orchestration.rs` 中的 `update_agent_fields`（对应 `WireMessage::UpdateAgent`、`OrbitAppCore::update_agent_with_handle`、`orbit_agent(action="update")` MCP 调用）在替换 `handle` 时，**必须**在同一 SQLite 事务中调用 `record_previous_handle_alias` 把非空旧 handle 追加入 `labels["orbit.handle_aliases"]`。
2. **全链路别名解析**：
   - Peer 解析函数与 `ConditionalTimerEngine`（`crates/orbit-daemon/src/timers.rs` 的 `sender:<handle>`）必须同时匹配当前 `handle` 与 `orbit.handle_aliases`。

---

## 4. Peer 通信与防反复唤醒原则

- 向同工作区伙伴发消息时，按实际需求设置 `notify_on_finish`；避免无意义互相 `@` 触发无穷唤醒回声链。
- **非打断默认排队（Non-Interrupting Async Delivery）**：`message_peer`、`send_agent_prompt` 与带 `ORBIT_AGENT_ID` 的 `orbit send` 在未显式传入 `active_turn_behavior` 时**一律默认使用 `queue`**；持久化完成唤醒（`finish_subscriptions.rs`）同样以 `Queue` 合批投递并仅在入队时记录 `Enqueued ... notification`（绝不提前宣称 `Delivered`，也不将消息正文写入日志），绝不自动取消父 Agent 正在进行的思考或工具回合。
- **必跑验证**：改动上述任何模块必跑 `pitfall_05`、`pitfall_08`（`e2e_pitfall_regressions`）及 `e2e_workspace_sync_and_worktree`、`e2e_orchestration`。

---

## 5. 会话主权分层（`SessionTier`）与 Personal Router（常驻分工师）

- **严格会话主权（Zero Surprise Sidebar Threads）**：`SessionTier`（`orbit-protocol/src/router.rs`，持久化于 `labels["orbit.session_tier"]`）将所有会话划分为 `Conversation`、`Subagent`、`PersonalRouter`、`Automation`。**仅人类与 `PersonalRouter` 可创建侧边栏可见的 `Conversation`**；普通 `Conversation` 或 `Subagent` 通过 `create_agent`、`spawn_workspace_peer`、`orbit_agent(action="fork")` 或 `orbit run` 派生的本机/跨主机子会话一律锁定为 `SessionTier::Subagent`，在侧边栏、全局总控台与自动归类中严格隐藏（跨主机/多级子代理待审批时将审批徽章冒泡至根 `Conversation`）；归档父会话时绝不自动调用 `detach_agent` 将子代理提升为顶层会话（仅用户显式执行 `DetachAgent` 时才提升为 `Conversation`）。
- **`routed_by` 与手动收回会话（Manual Reclaim Only）**：`PersonalRouter` 通过 `dispatch_conversation`（或 `create_agent`）派发的本机/远端会话统一标记 `labels["orbit.routed_by_agent"]`（`parent_agent_id = None`，`SessionTier::Conversation`），确保本机与远端派单一致显示在侧边栏并支持 `finish_subscriptions` 与 Tier-2 预审归属校验。已完成会话仅在用户发出明确指令或点击 UI「收回 / Reclaim」按钮（`reclaim_conversation` / `WireMessage::HarvestConversation`）时才提炼结论归档，禁止分工师擅自自动归档。
- **一等公民角色与持久化 Router 宪法**：`AgentRole::PersonalRouter` 允许用户在多主机 Fleet 中显式选择任意在线主机作为全局唯一主分工师（持久化于 `primary_router_selection` 并自动同步全网 `ConnectedHostsManager`）。启动时必须预先注入角色标签并通过 `SessionLaunchConfig.system_prompt_append` 跨回合持久挂载精炼分工师宪法：保留原生只读侦察/技能能力以精准拆解任务，禁止直接修改业务仓库代码或调用 Harness 黑盒内部子代理，强制使用高信噪比短回复汇报路由坐标。
- **跨主机项目 + Workspace 目标全景地图（Fleet Atlas）**：`PersonalRouterManager`（`crates/orbit-daemon/src/personal_router.rs`）聚合全部在线远端主机的能力摘要、项目、精编 Workspace（含 `mission_goal`）与活跃 `Conversation` 注入 `<orbit-fleet-state>` / `<orbit-fleet-delta>`，配合 `orbit_control(action="list_projects" | "inspect_project_context")` 实现零盲区跨设备精准派单。
- **后台定时器（Timer）空闲可对话不变量**：仅持有关闭后待触发定时器（`schedule` / `orbit_timer` / `waiting:N timer`）的会话与子代理保持 Idle 可随时继续对话（`is_running == false`，`has_background_tasks == true`，`has_armed_timers == true`），禁止在对话输入框或侧边栏显示阻塞性停止按钮或旋转加载圈。
- **三级风控与飞书双向闭环**：Tier-1 低危由 `GuardianManager`（需底层认证就绪，且默认放行只读 Orbit MCP / 看板工具）自动放行；Tier-2 中危可由分工师结合上下文预审（严格校验目标会话归属当前分工师 `parent_agent_id` 或 `orbit.routed_by_agent`）；Tier-3 高危（`High` / `Critical`）在协议层硬禁止分工师自动放行并剥离 `allow_always`，强制交由人类在原生端或飞书交互卡片拍板。飞书通道支持 `ImRoutingMode::PersonalRouter` 直接路由至唯一常驻分工师会话。
