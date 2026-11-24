# `apps/` 局部工程规范（全客户端共享约束）

本文件适用于 `apps/desktop`、`apps/web`、`apps/ios`、`apps/android` 与 `apps/harmony`。

1. **UI / 交互改动必读 Skill**：
   - 修改 `apps/` 下任何页面、布局、样式、弹窗或交互前，**必须先完整读取并遵循 [`.agents/skills/interface-design/SKILL.md`](../.agents/skills/interface-design/SKILL.md)** 与 [`docs/design/README.md`](../docs/design/README.md)（若仓库存在 `.interface-design/system.md` 需一并读取）。
2. **Thin-UI 架构原则（先读 [`docs/CLIENT_BRIDGE_ARCHITECTURE.md`](../docs/CLIENT_BRIDGE_ARCHITECTURE.md)）**：
   - 客户端仅负责原生视图渲染、平台生命周期/权限、本地输入框草稿、图片/文件选择器与 GPU 动效。
   - **严禁**在 JS / Swift / Kotlin / ArkTS 中重复实现 Sidebar 树分组、Stage 增量折叠、模型/Slash/Mention 排名、连接路由竞速或权限/加密业务逻辑——一律复用 `orbit-client-core` 契约并在对应目标目录下的 `AGENTS.md` 对齐绑定。
3. **多主机一等公民**：一切 UI 状态与本地缓存必须以 `(host_id, agent_id)` 或 `host_id` 为作用域，严禁写死单机逻辑。
