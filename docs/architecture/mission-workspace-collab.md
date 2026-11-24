# Mission Workspace 并行协作、Worktree 隔离与 Handle 别名架构

本文档定义多 Agent 在同一 Mission Workspace 内安全并发修改代码的工程约束。涉及协作看板、Worktree 隔离、Peer 通讯与定时器改动的 PR 必须遵守下列不变量。

---

## 1. 并行优先（Parallel-First）与 Worktree 自动隔离

- **禁止无故阻塞**：当多个 Peer 被分配同一项目内不同职责时，应基于同一基线尽早分化独立 `git worktree` 并行推进，而非串行排队等他人结束，也不得在同一检出目录直接混改彼此的未提交文件。
- **Worktree 隔离标签**：在精编 Mission Workspace 中通过 `orbit_agent(action="fork", worktree=true)` 分叉出的新成员应保留在同一个 `workspace_id` 下继续协同，并在 `labels` 中携带 `orbit.worktree_branch` 与 `orbit.worktree_source_cwd`，发出 `PeerJoined` 事件（实现位于 `crates/orbit-daemon/src/manager.rs`、`tools.rs`、`mcp.rs`）。
- **双向 Handoff**：工作树修改完成后，通过 `worktree_handoff.rs` 提供的安全备份 ref 与冲突感知策略（`apply` / `move` / `dry_run`）合并回目标本地工作区。

---

## 2. 共享看板 `BOARD.md` 与乐观锁（CAS）

- 每个 Mission Workspace 维护共享 `BOARD.md`（由 `crates/orbit-daemon/src/workspace_board.rs` 管理）。
- **分节局部更新 + 乐观并发控制**：修改特定 `## Section` 时应传入 `section` 与 `expected_revision`，通过 Compare-And-Swap（CAS）避免并发全量覆盖他人刚写入的决策与进展。

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
- **必跑验证**：改动上述任何模块必跑 `pitfall_05`（`e2e_pitfall_regressions`）及 `e2e_workspace_sync_and_worktree`、`e2e_orchestration`。

---

## 5. Personal Router（常驻分工师）与跨主机项目上下文编排

- **一等公民角色与全局主主机选择**：`AgentRole::PersonalRouter`（`orbit-protocol/src/router.rs`）允许用户在多主机 Fleet 中显式选择**任意一台在线主机**（例如远程常驻 Mac mini / 云端 Devbox 或本机）作为全局唯一主分工师（持久化于客户端 `primary_router_selection` 并通过 Cloud Sync preferences 跨设备同步），通过 `DiscoverFleetRouterHosts` 结合权威 `is_local` 身份标识探测各主机的 `hermes` 安装状态与已有 `~/.hermes/memories/` 记忆，且不在 ACP 模式强制开启 `fast_mode`。
- **跨主机项目发现、Peer-Bridge 连通性与上下文注入**：主分工师所在的 Host A 通过 `PersonalRouterManager`（`crates/orbit-daemon/src/personal_router.rs`）聚合全部在线远端主机的项目与活跃 Worker 注入 `<orbit-fleet-state>` / `<orbit-fleet-delta>`，并通过 Core Pack 的 `orbit_control(action="list_projects" | "inspect_project_context")` 在派单前免 SSH 拉取任意远端仓库的 `AGENTS.md` / `README.md` / 目录结构与 Git 分支上下文；无法直连或上 Relay 的内网隔绝主机可通过已配对节点的 `/ws/e2ee` Peer-Bridge 与 SSH Forward 隧道连通。
- **后台定时器（Timer）空闲可对话不变量**：仅持有关闭后待触发定时器（`schedule` / `orbit_timer` / `waiting:N timer`）的会话与子代理保持 Idle 可随时继续对话（`is_running == false`，`has_background_tasks == true`，`has_armed_timers == true`），禁止在对话输入框或侧边栏显示阻塞性停止按钮或旋转加载圈。
- **三级风控与飞书双向闭环**：Tier-1 低危由 `GuardianManager` 自动放行；Tier-2 中危可由分工师结合上下文预审；Tier-3 高危（`High` / `Critical`）在协议层硬禁止分工师自动放行并强制交由人类在原生端或飞书交互卡片拍板。飞书通道支持 `ImRoutingMode::PersonalRouter` 直接路由至唯一常驻分工师会话。
