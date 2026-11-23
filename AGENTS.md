# AGENTS.md — Orbit 工程宪法与文档强制路由

本文件仅保留**不可违背的全局红线、PR-First 发布铁律与任务路由表**。详细架构、开发规范与全仓索引已下沉至 [`docs/INDEX.md`](docs/INDEX.md) 及各子目录 `AGENTS.md`，禁止将长篇说明或实现细节堆入本文件。

---

## 1. 核心架构与安全红线

1. **单二进制多角色（Single Static Binary）**：`orbit`（`crates/orbit-cli`）必须保持零外部运行时依赖（无需 Node.js / Python），内聚集成 `daemon`、`shim`、`relay`、`pair`、`doctor` 等全部子命令，支持 Linux (`x86_64`/`aarch64`) 与 macOS (`Apple Silicon`) 单文件拷即用。
2. **进程热升级隔离（Shim Isolation）**：任何有状态 AI CLI 会话（`agy`、`codex`、`claude`、`pi` 及 ACP 会话）与用户交互式 `PTY` 必须由独立的 `orbit shim` 子进程托管（`setsid` 脱离会话组）并通过 `$ORBIT_HOME/shims/*.sock` 通信，严禁重启/升级 `orbit daemon` 时杀死运行中的 Agent。
3. **多主机一等公民（Multi-Host Fleet First）**：协议层与 `orbit-client-core` 中一切状态、`(HostId, CursorSeq)` 游标与连接池均需支持多台主机同时在线，严禁客户端硬编码单主机假设。
4. **私有零知识安全边界（Zero-Knowledge & Loopback-by-Default）**：Daemon 默认绑定 `127.0.0.1:6868` 并校验持久化 `pairing_secret` 与严格 `Origin`；远程穿透流量经 `Noise_NKpsk0_25519_ChaChaPoly_SHA256`（`orbit-relay-e2ee/2`）端到端加密，`orbit relay` 支持 `--allow-pubkey` 白名单且不接触明文；严禁通过进程 `argv` 传递敏感凭据。
5. **零 `unsafe` 与禁止生产 `.unwrap()`**：Workspace 强制 `unsafe_code = "deny"` 与 `clippy::unwrap_used = "warn"`，生产路径统一通过 `thiserror` / `anyhow` 显式传递错误。

---

## 2. 强制 Git / PR / Release 流程铁律（PR-First）

- **严禁直推 `main`**：任何改动（功能、修 Bug、文档、CI、以及发版版本号 Bump）**一律必须**在独立分支 / worktree 完成并提交 PR 到 `main`；GitHub Ruleset `protect-main-and-master` 已关闭全员（含 Admin/Maintain）bypass 并强制全部 CI 门禁通过。
- **先合入 `main`，后打 Tag 发布**：仅允许基于**已通过 PR + CI 合入 `main` 且 CI 全绿的提交**创建并推送 `v*` Tag；严禁未走 PR 提前打 Tag 或直接向 `main` 推送发版提交。
- **分支与工作区隔离**：并行会话协作或修改独立特性时，优先新建隔离 git worktree，禁止覆盖他人未提交工作区；完整发版清单见 [`docs/engineering/git-pr-release-workflow.md`](docs/engineering/git-pr-release-workflow.md)。

---

## 3. 任务触发必读路由表（Mandatory Pre-Read）

**固定要求**：动手修改任何代码或配置前，**必须**按下列场景先读取对应的权威文档 / Skill 及目标目录的局部 `AGENTS.md`。凡修改架构边界、线协议 / FFI / Store 契约、UI 设计系统或 CI 任务，**必须在同一个 PR 中同步更新对应文档与 [`docs/INDEX.md`](docs/INDEX.md)**。

| 你准备做什么（触发场景） | 动手前**必须**先读的权威文档 / Skill |
| --- | --- |
| 任何提交、创建 PR、版本号升级或切 Tag 发版 | [`docs/engineering/git-pr-release-workflow.md`](docs/engineering/git-pr-release-workflow.md) |
| 任何 UI / 布局 / 样式 / 交互组件改动（`apps/`） | [`.agents/skills/interface-design/SKILL.md`](.agents/skills/interface-design/SKILL.md) + [`docs/design/README.md`](docs/design/README.md)（若存在 `.interface-design/system.md` 需一并读取） |
| 修改跨端 FFI / Tauri IPC / Stage / Sidebar / 共享状态 | [`docs/CLIENT_BRIDGE_ARCHITECTURE.md`](docs/CLIENT_BRIDGE_ARCHITECTURE.md) |
| 修改 Daemon / Shim / Provider 驱动 / 消息防错位 / 尾部分页 | [`docs/architecture/daemon-shim-providers.md`](docs/architecture/daemon-shim-providers.md) |
| 修改 Mission Workspace 多 Agent 协作 / Board / Worktree / 定时器 | [`docs/architecture/mission-workspace-collab.md`](docs/architecture/mission-workspace-collab.md) |
| 修改安全边界、加密握手、Relay 鉴权、Cloud Sync 或安装升级脚本 | [`SECURITY.md`](SECURITY.md) + [`docs/cloud-sync.md`](docs/cloud-sync.md) |
| 修改 CI / Release 流水线、Ruleset 或补齐端到端回归测试 | [`docs/engineering/testing-and-ci.md`](docs/engineering/testing-and-ci.md) |
| 修改 Relay 部署、macOS 公证、TestFlight 或 HarmonyOS 签名 | [`deploy/README.md`](deploy/README.md) 及对应 `deploy/*_SIGNING.md` / [`deploy/TESTFLIGHT.md`](deploy/TESTFLIGHT.md) |
| 查阅全仓文档地图、或新增 / 调整 / 归档任何文档 | [`docs/INDEX.md`](docs/INDEX.md) + [`docs/engineering/doc-governance.md`](docs/engineering/doc-governance.md) |

---

## 4. 常用全局验证命令

```bash
bash scripts/check-docs-index.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
TMPDIR=/tmp cargo test --workspace -- --test-threads=4
cargo run -p orbit-cli -- doctor
```
