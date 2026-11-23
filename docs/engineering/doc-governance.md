# Orbit 文档治理与四层索引规范

本规范定义如何避免根 `AGENTS.md` 上下文膨胀与多份文档漂移。

---

## 1. 四层治理模型

1. **第 1 层：根 [`AGENTS.md`](../../AGENTS.md)（全球宪法与路由入口）**
   - 只放：5 条核心架构与安全红线、PR-First 流程铁律、任务触发必读路由表、常用全局命令。
   - **体积极限**：≤ 140 行 且 ≤ 8192 字节（由 [`scripts/check-docs-index.sh`](../../scripts/check-docs-index.sh) 在 CI 中强制阻断）。
2. **第 2 层：[`docs/INDEX.md`](../INDEX.md) + `docs/**`（全局总索引与深度专题）**
   - `docs/INDEX.md` 是仓库全部规范文档的唯一注册表，声明每份文档的 **管辖范围、触发先读条件、权威边界与同步更新触发器**。
   - `docs/architecture/**`、`docs/design/**`、`docs/engineering/**` 及 `deploy/**` 承载详细架构契约、设计总入口、发布清单与运维手册。
3. **第 3 层：子目录 `AGENTS.md`（按目录作用域自动继承的局部守则）**
   - 置于 `crates/AGENTS.md`、`apps/AGENTS.md`、`apps/desktop/AGENTS.md`、`apps/ios/AGENTS.md`、`apps/android/AGENTS.md`、`apps/harmony/AGENTS.md`、`.github/AGENTS.md`。
   - 仅当 Agent/开发者进入该子目录修改代码时才按目录继承生效，不占用无关任务的上下文。
4. **第 4 层：`.agents/skills/*/SKILL.md`（按需渐进加载的领域工作流）**
   - 仅注入简要名称与触发描述，由具体任务激活后全量按需读取（例如 UI/交互修改时激活 `.agents/skills/interface-design/SKILL.md`）。

---

## 2. 单一权威源原则（No Duplication）

- 禁止将同一份详细规范同时抄写在根 `AGENTS.md`、`CONTRIBUTING.md` 和 `docs/` 中；应在最精准的专题文档维护唯一正文，其他文件仅用一句话+相对链接指向。
- 涉及对外已链接路径（如 `docs/CLIENT_BRIDGE_ARCHITECTURE.md`、`docs/cloud-sync.md`、`deploy/*.md`）保持原位不破坏外链，新增长文放到对应的 `docs/architecture/`、`docs/design/` 或 `docs/engineering/` 下。

---

## 3. 自动化防漂移门禁（[`scripts/check-docs-index.sh`](../../scripts/check-docs-index.sh)）

`ci.yml` 的 `rust-quality-and-unit` 在每次 Push 和 PR 强制运行 `bash scripts/check-docs-index.sh`，校验 5 项：

1. 根 `AGENTS.md` 行数 ≤ 140、字节 ≤ 8192。
2. 根 `AGENTS.md` 必须同时引用 `docs/INDEX.md` 与 `.agents/skills/interface-design/SKILL.md`。
3. `apps/AGENTS.md` 必须存在且引用 `.agents/skills/interface-design/SKILL.md`。
4. `docs/INDEX.md` 中声明的所有本地 `.md` 路径必须真实存在（排除可选生成文件 `.interface-design/system.md`）。
5. 全仓根 `*.md`、`docs/**/*.md`、`deploy/*.md`、`.agents/skills/*/SKILL.md` 与所有子目录 `AGENTS.md`（排除构建/第三方缓存及 PR 模板、Daemon 运行时内置模板）均已在 `docs/INDEX.md` 登记，杜绝“写了新文档却没人知道读”。
