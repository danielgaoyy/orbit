# Git、PR 与 Release 强制工作流（PR-First Policy）

本文档定义 Orbit 仓库的**唯一合法代码合入与版本发布流程**。所有开发者（包括仓库维护者 / Admin）与 AI 编程助手必须严格遵守。

---

## 1. 核心铁律

1. **禁止直接向 `main` 提交或推送任何内容**：
   - 无论是功能开发、Bug 修复、文档/规范修订、CI 修改，还是发版版本号更新（`chore(release): prepare vX.Y.Z`），**一律必须**在独立分支（多人/多会话并行时搭配独立 `git worktree`）完成并创建 Pull Request 到 `main`。
   - GitHub Branch Ruleset `protect-main-and-master`（ID `27444583`）已配置 `bypass_actors: []`，服务端硬性拦截一切绕过 PR 的直推，包括 Admin。
2. **先走 PR 合入 `main` 并等 CI 全绿，再打 Tag 发布**：
   - 严禁在未合并到 `main` 的个人分支上直接打 `v*` Tag。
   - 仅当发布准备 PR 已通过全部 CI 门禁并合并至 `main` 后，才允许基于 `main` 上的该合并提交创建带附注的 Tag（`git tag -a vX.Y.Z -m "Release vX.Y.Z"`）并推送到 `origin`。
3. **分支保护与 Required Status Checks 联动**：
   - 当前 Ruleset `protect-main-and-master` 强制要求以下 6 项 CI Check 全部绿灯后才允许合并 PR：
     - `Rust Format, Clippy & Unit Tests`
     - `E2E (Pitfall Regressions)`
     - `E2E (Relay E2EE & Isolation)`
     - `E2E (Orchestration & Fleet Mesh)`
     - `E2E (Workspace Sync & Worktree)`
     - `Desktop Frontend Typecheck & Build`
   - Ruleset `protect-release-tags`（ID `27444584`）保护 `refs/tags/v*` 禁止非授权创建、篡改与删除（仅维护者具备 tag 创建 bypass）。

---

## 2. 日常研发 PR 流程

1. **创建隔离分支 / Worktree**：
   ```bash
   git fetch origin --tags
   # 单会话：
   git checkout -b feat/your-topic origin/main
   # 多会话并行协作（推荐，避免污染主目录）：
   git worktree add ../orbit-your-topic -b feat/your-topic origin/main
   ```
2. **动手前先读文档**：按根 [`AGENTS.md`](../../AGENTS.md) 路由表及目标目录 `AGENTS.md` 读取规范；若涉及架构/协议/UI 契约/CI 变更，在同一个分支同步更新对应文档与 [`docs/INDEX.md`](../INDEX.md)。
3. **本地验证**：运行变更覆盖层的全部检查（如 `bash scripts/check-docs-index.sh`、Rust `fmt`/`clippy`/`test`、Desktop `tsc`/`test`/`build`/`test:smoke`、iOS `OrbitMobileLogicTests`）。
4. **提 PR 并合入**：
   - `git push -u origin <your-branch>`
   - 通过 `gh pr create` 或 GitHub 页面按 [`.github/PULL_REQUEST_TEMPLATE.md`](../../.github/PULL_REQUEST_TEMPLATE.md) 创建 PR，等待全部 Required CI Checks 通过后合并进 `main`，然后清理特性分支与临时 worktree。

---

## 3. 版本发布（Release）标准 Checklist

禁止任何“本地顺手 `cargo` 改完直接推 `main` + 打 tag”的操作。发版必须走两段式门禁：

### 阶段 A：通过 PR 完成版本号升级与 Lockfile 刷新
1. 基于最新 `origin/main` 拉出发布分支 `chore/release-vX.Y.Z`。
2. **统一同步以下全部 15 个文件**（`versionName`/`MARKETING_VERSION` 升至 `X.Y.Z`，Android/Harmony/iOS 整数版本号 `versionCode`/`CURRENT_PROJECT_VERSION` 按 `MAJOR * 1_000_000 + MINOR * 1000 + PATCH` 递增，例如 `0.9.8` -> `9008`、`0.10.4` -> `10004`、`0.11.0` -> `11000`）：
   - `Cargo.toml`（`[workspace.package].version`）
   - `Cargo.lock`（通过 `cargo check --workspace` 刷新）
   - `apps/desktop/package.json`（`version`）
   - `apps/desktop/package-lock.json`（通过 `cd apps/desktop && npm install --package-lock-only` 刷新）
   - `apps/docs/package.json` 与 `apps/docs/package-lock.json`（`version`，通过 `cd apps/docs && npm install --package-lock-only` 刷新）
   - `apps/desktop/src-tauri/Cargo.toml`（`[package].version`）
   - `apps/desktop/src-tauri/Cargo.lock`（通过 `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml` 刷新）
   - `apps/desktop/src-tauri/tauri.conf.json`（`version`）
   - `apps/desktop/src/components/settings/general-section.tsx`（回退显示版本号）
   - `apps/desktop/src/lib/orbit-ipc.ts`（非 Tauri 环境 `getAppVersion` 回退值）
   - `apps/desktop/scripts/smoke-render.mjs`（mock 版本号字符串）
   - `apps/android/app/build.gradle.kts`（`versionCode` & `versionName`）
   - `apps/harmony/AppScope/app.json5`（`versionCode` & `versionName`）
   - `apps/ios/project.yml`（`MARKETING_VERSION` & `CURRENT_PROJECT_VERSION`）与 `apps/ios/Sources/Store/OrbitIosStore.swift`（`appShortVersion` 回退值）
3. 提交 `chore(release): prepare vX.Y.Z`，推分支并创建 PR 到 `main`。
4. 等待 PR 的 CI 检查全部绿灯通过后，正式合入 `main`。

### 阶段 B：基于已合并的 `main` 提交打 Tag 触发 Release 流水线
1. 拉取最新 `origin/main`，确认目标提交就是刚合入的 `chore(release): prepare vX.Y.Z`。
2. 仅在该 `origin/main` 提交上创建并推送附注 Tag：
   ```bash
   git fetch origin
   git checkout --detach origin/main
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin vX.Y.Z
   ```
3. 观察 `.github/workflows/release.yml` 与 `.github/workflows/testflight.yml`（在允许环境下）完成全平台产物的编译、校验和 `SHA256SUMS.txt` 发布。

---

## 4. 事故回滚 / Yank 规范

- 若已发布版本发现启动崩溃等阻断性缺陷（如历史上的 `v0.9.2`）：
  1. 立即将该 GitHub Release 标为 Pre-release / 在标题与说明顶部加 `Yanked / Do Not Use` 警示（**严禁复用/覆盖原 Tag**）。
  2. 在独立修复分支编写修复 + **同步新增对应回归测试（E2E 或 CDP Smoke）**，按阶段 A/B 流程走 PR 合入 `main` 并发布新的补丁版本 `vX.Y.(Z+1)`。
