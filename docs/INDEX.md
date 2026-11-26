# Orbit 全仓权威文档总索引（Single Source of Truth Registry）

本索引是 Orbit 仓库**唯一权威文档目录与按任务路由登记表**。所有规则文档、架构设计、设计系统 Skill、部署指南与目录级 `AGENTS.md` 均在此登记；CI 通过 [`scripts/check-docs-index.sh`](../scripts/check-docs-index.sh) 自动校验“无未登记孤儿文档、无索引死链、根 `AGENTS.md` 不膨胀”。

---

## 使用铁律

1. **按场景先读再改**：开始任何编码或配置任务前，根据下表“何时必须先读”先阅读对应权威文档/Skill 与子目录 `AGENTS.md`。
2. **单一权威边界**：每个主题只在一份文档中做权威阐述（见“权威边界”列），其他文档仅做链接引用，严禁多处复制粘贴导致文档漂移。
3. **代码与文档同 PR 演进**：改动涉及其管辖范围且改变了对外契约、流程、UI 规则或 CI 门禁时，必须在同一 PR 中同步更新对应文档与本索引。

---

## 1. 核心宪法与项目总览（根目录）

| 路径 | 管辖范围 (Scope Globs) | 何时必须先读 (Mandatory Pre-Read Trigger) | 权威边界 (Source of Truth) | 同步更新触发器 (Keep-in-Sync) |
| --- | --- | --- | --- | --- |
| `AGENTS.md` | `**` | 每次进入仓库开始任何任务时必读 | 6 条核心架构与工程红线（含全局同源排查与联动预检）、PR-First 流程铁律、任务触发路由表（硬上限 ≤ 140 行 / 8 KB） | 修改核心红线、发版流程或新增专题/领域文档入口 |
| `README.md` | `README.md` | 对外介绍能力矩阵、快速安装与源码构建命令时 | 项目对外总览（英文）、Quick Start 与各端构建命令索引 | 增减支持平台、主流 Provider 或变更仓库总览文档链接 |
| `README.zh-CN.md` | `README.zh-CN.md` | 同步更新中文对外项目总览、Quick Start 与多机拓扑说明时 | 项目对外总览（简体中文镜像，与 `README.md` 保持能力与链接一致） | 修改 `README.md` 能力矩阵、支持平台、截图路径或构建命令 |
| `ARCHITECTURE.md` | `crates/**`, `apps/**` | 首次理解全栈分层与 13 个 crate / 4 个 app 拓扑关系时 | 宏观系统拓扑图与四端/多 crate 职责边界总览 | 新增/移除 crate 或 app、调整系统核心协议或 UI 基础库选型 |
| `CONTRIBUTING.md` | `**` | 提交 PR、检查环境依赖或核对开源协作规范时 | PR 规范、本地环境前置依赖与验证命令清单 | 修改构建先决条件、CI 检查门禁或 PR 提交规范 |
| `SECURITY.md` | `crates/orbit-crypto/**`, `crates/orbit-relay/**`, `crates/orbit-daemon/**`, `deploy/install*.sh` | 修改认证、`pairing_secret`、Noise 握手、Relay 准入、自更新校验或私钥处理前 | 语音批准与端点凭据绑定、威胁模型、密码学基线、供应链校验与漏洞私密披露流程 | 变更加密序言/原语、鉴权方式、安全边界或支持版本矩阵 |
| `CODE_OF_CONDUCT.md` | `CODE_OF_CONDUCT.md` | 处理社区行为规范与治理争议时 | 开源社区行为准则（Contributor Covenant v2.1） | 变更社区治理联系方式或行为准则条款 |

---

## 2. 专题架构、设计与工程规范（`docs/` & `.agents/skills/`）

| 路径 | 管辖范围 (Scope Globs) | 何时必须先读 (Mandatory Pre-Read Trigger) | 权威边界 (Source of Truth) | 同步更新触发器 (Keep-in-Sync) |
| --- | --- | --- | --- | --- |
| `docs/INDEX.md` | `**/*.md`, `**/AGENTS.md`, `**/SKILL.md` | 新增、迁移、拆分或归档任何仓库文档前 | 全仓文档/Skill/子目录 `AGENTS.md` 注册表与路由边界 | 仓库新增、重命名、迁移或删除任何受管文档 |
| `docs/CLIENT_BRIDGE_ARCHITECTURE.md` | `crates/orbit-client-core/**`, `crates/orbit-client-wasm/**`, `crates/orbit-ffi-uniffi/**`, `crates/orbit-ffi-ohos/**`, `apps/**` | 增删 FFI/IPC 接口、修改 Stage 增量投影、Sidebar 树、图渲染或五端行为对齐前 | 按 Host 的语音 RPC 与客户端回退、FIM / PR Patrol IPC、Client Bridge v2 数据流、FFI/IPC 契约表、会话 `plan_steps` / 全量 `outline_prompts` 投影与各端绑定覆盖、AgentCard 创建时间、SQLite `client_thread_prefs`、移动端持久发件箱、Web PWA / WASM 加密传输与五端原生绑定矩阵 | 变更 `contract.rs`、`orbit-ffi-*`、Tauri IPC 命令或跨端交互语义 |
| `docs/cloud-sync.md` | `crates/orbit-client-core/src/cloud_sync.rs`, `crates/orbit-crypto/**`, `crates/orbit-cli/**` | 修改 GitHub Gist Cloud Sync 加密、OAuth Device Flow 或 LWW 合并逻辑前 | 硬件密钥信封、离线 dirty 标记、零知识 BYOS 云同步协议、`ORBIT_GITHUB_CLIENT_ID` 配置与使用流程 | 变更 vault 格式、OAuth Client ID / scope 或同步合并规则 |
| `docs/architecture/daemon-shim-providers.md` | `crates/orbit-daemon/**`, `crates/orbit-shim/**`, `crates/orbit-providers/**`, `crates/orbit-protocol/**`, `crates/orbit-relay-transport/**`, `crates/orbit-store/src/memory.rs` | 修改 Daemon 生命周期、Shim socket、`claude`/`codex`/`agy`/ACP 驱动、消息防错位、尾部分页或 Relay 心跳前 | FIM / PR Patrol 协议、Daemon/Shim 解耦、驱动矩阵、Turn 对齐与延迟 finalize 去重、尾部分页、直连移动推送（APNs/FCM/HMS）、跨 Provider 项目记忆（Markdown、FTS5、RRF 与 Core 工具）、离线语音模型与推理链、Off-LAN Relay 同步、工具沙箱、审计链、诊断导出、用量预算、IDE 迁移与 Windows 服务 | 变更 Wire 协议、Provider 状态机、分页参数或 Relay 断线重连策略 |
| `docs/architecture/mission-workspace-collab.md` | `crates/orbit-daemon/src/{workspace_board,workspace_organizer,workspace_cluster,worktree_handoff,timers,messaging}.rs`, `crates/orbit-store/src/{workspace_sync,orchestration,timers}.rs` | 修改 Mission Workspace、共享 `BOARD.md`、Worktree 隔离/handoff、`@handle` 别名或条件定时器前 | 并行优先协作、CAS 看板、Worktree 隔离标签、`orbit.handle_aliases` 历史解析与唤醒防风暴约束 | 变更协作工具 Schema、别名保留字段（含 `UpdateAgent`）或计时器语义 |
| `docs/design/README.md` | `apps/**`, `.agents/skills/interface-design/**`, `.agents/skills/shadcn/**` | 进行任何 UI 视觉设计、交互重构、动效调整、设计 Tokens、弹窗/表单或三语文案修改前 | 全端 UX 九大原则（系统吸收复杂性、缩放与字号正交组合）、以状态变化驱动的苹果级动效宪法七大准则、设计工作流路由（`interface-design` / `shadcn` Skill、`.interface-design/system.md`）、桌面全页设置导航、作用域与搜索规则入口，跨端边界与验证门禁 | 调整通用 UX/动效准则、设计系统约束、i18n 语种或 UI 验收命令 |
| `docs/design/side-panel.md` | `apps/desktop/src/state/side-panel-scope.ts`, `apps/desktop/src/components/side-panel/**`, `apps/desktop/src/lib/workspace-layout.ts` | 修改桌面端右侧边栏每会话状态隔离、自动打开策略、宽窄窗口折叠、持久化 Schema 或关闭标签重开前 | 桌面端右侧边栏每会话（`hostId::rootAgentId` / `draft::hostId`）状态模型、切换/晋升/归档生命周期、`user`/`auto` 打开策略矩阵、宽度钳制、`orbit.desktop.sidePanel.v1` 持久化与 `⌘⇧T` 快捷键契约 | 变更右侧边栏作用域规则、自动打开策略、布局钳制参数或本地持久化 Schema |
| `.agents/skills/interface-design/SKILL.md` | `apps/**` | 动手设计、实现、审查或打磨 `apps/` 下任何界面与交互前（必读 Skill） | 界面质感与交互设计方法论、层级/留白/色彩/动效规范及自检标准 | 更新或升级 `interface-design` Skill 版本（同步 `skills-lock.json`） |
| `.agents/skills/shadcn/SKILL.md` | `apps/desktop/**` | 在 Desktop 中新增、组合、重构或排查 shadcn/ui 组件、表单与样式前（必读 Skill） | 官方 shadcn/ui（Base UI）组件组合、表单 Field 原语、Styling 规范与 Registry 使用指南 | 更新或升级 `shadcn` Skill 版本（同步 `skills-lock.json`） |
| `docs/engineering/git-pr-release-workflow.md` | `.github/**`, `Cargo.toml`, `apps/*/package.json`, `apps/ios/project.yml`, `apps/android/app/build.gradle.kts`, `apps/harmony/AppScope/app.json5` | 提交代码、创建 PR、修改分支保护、切版本号或打 `v*` Tag 发版前 | PR-First 强制工作流、GitHub Rulesets、13 文件版本清单与 Tag/Yank 规范 | 修改发版涉及的清单文件、Ruleset 策略或 Tag 触发条件 |
| `docs/architecture/plugins.md` | `crates/orbit-{protocol,daemon,cli}`, `examples/plugins/` | 变更插件协议、能力或 ABI 前 | WASM 资源边界、声明式 UI、MCP 与 CLI | 变更插件运行时、清单或宿主导入 |
| `docs/engineering/testing-and-ci.md` | `.github/workflows/**`, `crates/orbit-cli/tests/**`, `apps/desktop/scripts/smoke-*.mjs` | 修改 CI/Release 工作流、调整并行矩阵、或新增/排查 E2E 与 Smoke 测试前 | HarmonyOS N-API 声明门禁、PR Patrol 只读 CI 模板、并行 CI/Release 分片结构、历史踩坑 E2E 说明、Chrome 单机及多主机设置交互烟测与 Ruleset 联动要求 | 增删或重命名 CI Job、E2E 测试文件或 Smoke 检查场景 |
| `docs/engineering/doc-governance.md` | `AGENTS.md`, `**/AGENTS.md`, `docs/**`, `scripts/check-docs-index.sh` | 准备新增文档、下沉局部规则或扩写根 `AGENTS.md` 时 | 四层文档治理模型、体积预算控制与自动化防漂移校验准则 | 调整层级划分、体积预算或 `check-docs-index.sh` 校验规则 |

---

## 3. 目录级局部 `AGENTS.md`（按目录作用域自动继承）

| 路径 | 管辖范围 (Scope Globs) | 何时必须先读 (Mandatory Pre-Read Trigger) | 权威边界 (Source of Truth) | 同步更新触发器 (Keep-in-Sync) |
| --- | --- | --- | --- | --- |
| `crates/AGENTS.md` | `crates/**` | 修改任意 Rust crate 前 | Rust workspace 编码红线、向后兼容原则与提交前检查链 | 修改 workspace lint 等级、共有 FFI/Wire 兼容策略 |
| `apps/AGENTS.md` | `apps/**` | 修改桌面端或任意移动端界面与客户端代码前 | 全端共用 UI 必读 Skill 约束与 Thin-UI 业务逻辑禁区 | 调整跨端共享原则或 UI Skill 挂载路径 |
| `apps/desktop/AGENTS.md` | `apps/desktop/**` | 修改 React 前端、Tauri Rust 后端或构建/测试脚本前 | React 19 `useSyncExternalStore` 防 `#185` 规范、13px 根字号换算、平行视图同源与容器联动自检、三语 i18n、Tauri IPC 同步与 Smoke 门禁 | 变更 Desktop 前端架构、状态订阅约束或必需验证命令 |
| `apps/ios/AGENTS.md` | `apps/ios/**` | 修改 SwiftUI 视图、`OrbitIosStore.swift`、`project.yml` 或绑定脚本前 | iOS UniFFI 绑定刷新、`xcodegen generate` 与 arm64 Simulator 逻辑测试规范 | 变更 iOS 构建脚本、Simulator 目标或存储安全类约定 |
| `apps/android/AGENTS.md` | `apps/android/**` | 修改 Kotlin / Compose UI、`build.gradle.kts` 或 Android JNI 绑定前 | Android NDK/UniFFI 绑定同步、API 29–36 构建与 Contract v2 对齐约定 | 变更 Android SDK 基线或 NDK 构建脚本 |
| `apps/harmony/AGENTS.md` | `apps/harmony/**` | 修改 HarmonyOS ArkTS / ArkUI V2 或 `ohos-rs` N-API 绑定前 | HarmonyOS 7+ (API 26) 构建、防提交本机签名配置与 N-API 对齐约定 | 变更 HarmonyOS 构建/剥离签名脚本或目标 API 级别 |
| `.github/AGENTS.md` | `.github/**` | 修改 CI/Release/TestFlight 工作流或 PR 模板前 | CI Job 名称与 GitHub Ruleset `protect-main-and-master` 强绑定锁步规范 | 变更 GitHub Actions 触发条件、Required Check 名或模板 |

---

## 4. 运维、部署与签名指南（`deploy/`）

| 路径 | 管辖范围 (Scope Globs) | 何时必须先读 (Mandatory Pre-Read Trigger) | 权威边界 (Source of Truth) | 同步更新触发器 (Keep-in-Sync) |
| --- | --- | --- | --- | --- |
| `deploy/README.md` | `deploy/install*.sh`, `crates/orbit-relay/**` | 部署自托管 BYOR `orbit relay`、系统服务守护或写安装指南前 | 自托管 Zero-Knowledge Relay 安装、`--allow-pubkey` 与快速配置 | 变更 Relay 默认端口、CLI 子命令或安装脚本参数 |
| `deploy/MACOS_SIGNING.md` | `.github/workflows/release.yml`, `apps/desktop/src-tauri/tauri.conf.json` | 配置或排查 macOS `.app` / `.dmg` Developer ID 签名与 Apple 公证前 | macOS 云端签名/公证 Secrets 与环境变量说明 | 变更 macOS 签名 Secrets 名称或 `release.yml` 公证步骤 |
| `deploy/TESTFLIGHT.md` | `.github/workflows/testflight.yml`, `apps/ios/**` | 配置或排查 iOS TestFlight 自动上传流水线前 | App Store Connect API Key 配置与云端 TestFlight 接入流程 | 变更 `testflight.yml` Secrets 或 Bundle ID / Team 配置方式 |
| `deploy/HARMONY_SIGNING.md` | `.github/workflows/release.yml`, `apps/harmony/scripts/strip-signing.py` | 配置 HarmonyOS `.hap` 签名或排查 CI 剥离签名脚本前 | HarmonyOS 签名 Secrets、未签名包回退与 `strip-signing.py` 机制 | 变更 HarmonyOS 签名工具链或云端 Secrets 列表 |

> **附录（Daemon 内置运行时 Skill，不纳入人工/编码 PR 路由）**：`crates/orbit-daemon/skills/` 下包含随 Daemon 随包分发的运行时 Skill 模板（`orbit`、`orbit-advisor`、`orbit-committee`、`orbit-handoff`），仅属于 `orbit-daemon` 运行时资源，不在上述人工/Agent 开发规范目录内。
