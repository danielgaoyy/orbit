# `.github/` 局部工程规范（CI / Release / PR 治理）

动手修改 `.github/workflows/*.yml` 或 `.github/PULL_REQUEST_TEMPLATE.md` 前，**必须先读取**：
- [`docs/engineering/testing-and-ci.md`](../docs/engineering/testing-and-ci.md)
- [`docs/engineering/git-pr-release-workflow.md`](../docs/engineering/git-pr-release-workflow.md)

## 硬性约束

1. **CI Check 名称与 Ruleset 强锁定**：
   - GitHub Ruleset `protect-main-and-master`（ID `27444583`）严格校验 6 个 Required Status Check 精确名称：`Rust Format, Clippy & Unit Tests`、`E2E (Pitfall Regressions)`、`E2E (Relay E2EE & Isolation)`、`E2E (Orchestration & Fleet Mesh)`、`E2E (Workspace Sync & Worktree)`、`Desktop Frontend Typecheck & Build`。
   - **严禁**在未同步更新 GitHub Ruleset 前重命名或删除这些 Job 名称，否则将导致全部 PR 卡死无法合入。
2. **禁止弱化 CI 门禁**：
   - 不得移除 `rust-quality-and-unit` 中的 `bash scripts/check-docs-index.sh` 文档索引门禁，也不得移除 `release.yml` 各平台构建对 `rust-quality-and-unit` 与 `e2e-regression-matrix` 的 `needs:` 依赖。
3. **PR 模板与工作流一致**：任何新增提交前自检要求必须同步反映到 `PULL_REQUEST_TEMPLATE.md`。
