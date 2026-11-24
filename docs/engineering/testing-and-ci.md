# 测试策略、E2E 回归门禁与 CI/Release 并行矩阵

本文档说明 Orbit 的本地测试体系、端到端踩坑回归套件（E2E）、Headless Chrome 运行时烟测与 CI/Release 工作流规范。

---

## 1. CI 与 Release 并行流水线结构

为兼顾高覆盖度与短执行时长，[`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) 与 [`.github/workflows/release.yml`](../../.github/workflows/release.yml) 将全量 Rust 校验拆为**单元测试**与 **4 个并行 E2E 分片**：

1. **`rust-quality-and-unit`**（`Rust Format, Clippy & Unit Tests`）：
   - `deploy/install.sh` / `deploy/install-desktop-linux.sh` Shell 语法检查（`bash -n` & `sh -n`）
   - 文档索引与体积防膨胀检查（`bash scripts/check-docs-index.sh`）
   - Workspace 与 `apps/desktop/src-tauri` 格式检查（`cargo fmt --all -- --check`）
   - 严格静态检查（`cargo clippy --workspace --all-targets -- -D warnings`）
   - WASM ViewModel 编译校验（`cargo check -p orbit-client-wasm --target wasm32-unknown-unknown`）
   - 单元测试与二进制内部测试（`cargo test --workspace --lib --bins`）
2. **`e2e-regression-matrix`**（4 分片并发执行，共享 `Swatinem/rust-cache@v2` `shared-key: "orbit-rust-e2e"`，固定使用 `TMPDIR=/tmp ... -- --test-threads=4`）：
   - `Pitfall Regressions`：`-p orbit-cli --test e2e_pitfall_regressions`
   - `Relay E2EE & Isolation`：`-p orbit-cli --test e2e_relay_and_pty --test e2e_multi_user_isolation --test e2e_browser_noise`
   - `Orchestration & Fleet Mesh`：`-p orbit-cli --test e2e_orchestration --test e2e_voice`
     此分片同时验证插件安装/启停/调用/条件 MCP 目录以及本地离线语音控制面端到端流程。
   - `Workspace Sync & Worktree`：`-p orbit-cli --test e2e_workspace_sync_and_worktree`
3. **跨端构建与烟测任务**：
   - `desktop-frontend-build`（`Desktop Frontend Typecheck & Build`）：Node unit tests + `npm run build` + `npm run test:smoke`
   - `tauri-rust-check`（macOS）：Tauri crate fmt / clippy / tests
   - `windows-rust-and-desktop-check`（Windows）：`cargo build -p orbit-cli --bin orbit` + `cargo test --workspace -- --skip live_devbox_ssh_one_click_e2e` + Desktop check
   - `ios-build-and-simulator-smoke`（macOS）：XCFramework + `xcodegen` + 并行唤醒模拟器与单遍 Simulator `xcodebuild test` + Launch 截图烟测
   - Android UniFFI & Debug APK build（含 `testDebugUnitTest` + `lintDebug` + `assembleDebug`）

In Release ([`release.yml`](../../.github/workflows/release.yml)) 与 TestFlight ([`testflight.yml`](../../.github/workflows/testflight.yml))：

- **全量门禁后置并行**：`release.yml` 中所有跨平台构建任务（Linux/macOS/Windows CLI、`*-desktop-bin` 桌面二进制、Android、iOS、HarmonyOS）与 `rust-quality-and-unit`、`e2e-regression-matrix`、`desktop-frontend-quality` 从 `t = 0` **并发启动**，由最终的 `publish-github-release` 通过 `needs:` 统一拦截——任何质量、E2E 或前端烟测失败均不会创建 GitHub Release。
- **CLI 与桌面二进制并行编译 + 纯打包拆分（`--no-bundle` / `tauri bundle`）**：
  - Windows 与 macOS 将 CLI（`orbit-cli`）与 Desktop（`tauri build --no-bundle`）拆到两个独立 Runner 从 `t = 0` 并行冷编，完成后分别流转给轻量的 `tauri bundle`（Windows MSI/NSIS、macOS `.app`/`.dmg` + 公证签名）Job 组装最终安装包。
  - Linux 桌面端（`x64` / `arm64`）直接复用 `build-host-binaries` 编出的零依赖 musl 静态 `orbit-cli` 作为 sidecar，消除两次重复冷编后再执行 `tauri bundle --bundles deb`。
  - `publish-github-release` 在计算 `SHA256SUMS.txt` 前清理 `_internal-*` 中间产物，确保公开发布资产清单纯净。
- **轻量 UniFFI Bindgen 与 Release 精简切片**：独立 `crates/orbit-uniffi-bindgen` 负责生成 Swift/Kotlin 绑定，避免移动构建时重编宿主 debug `orbit-ffi-uniffi`；`release.yml` 与 `testflight.yml` 通过 `./apps/ios/scripts/build-rust-xcframework.sh --device-only` 仅构建真机 `aarch64-apple-ios` 切片，Android Release 通过 `./apps/android/scripts/build-rust-android.sh --arm64-only` 搭配 `-PorbitAndroidAbis=arm64-v8a` 仅打 `arm64-v8a` 真机 APK（`ci.yml` 仍保留 iOS Simulator 与 Android 双 ABI 检查）。
- **TestFlight Ubuntu 凭据预检**：`testflight.yml` 先在 `ubuntu-latest` 执行 `check-credentials`，缺失 Apple Secrets 时直接跳过，避免空占 `macos-latest` 队列。
- **统一编译并发与精准缓存治理**：CI/Release/TestFlight 统一设置 `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`（保留 `lto = "thin"`）；仅在 `ci.yml` 与 `release.yml` Profile 匹配的 Rust Job 保留 `Swatinem/rust-cache@v2`（限定 `save-if: ${{ github.ref == 'refs/heads/main' }}`），移除独立 Release/Desktop Job 中跨工作区命中率极低（~1.5%）的本地磁盘 `sccache` 与无效 `rust-cache` 恢复，并在 `build-harmony-hap` 将 `ErBWs/setup-ohos@v2` 设为 `cache: false`，防止每次 `v*` Tag 写入 4.1 GiB SDK 缓存把 `main` Rust 缓存挤出 10 GB 配额。

---

## 2. `e2e_pitfall_regressions` 历史踩坑回归清单

[`crates/orbit-cli/tests/e2e_pitfall_regressions.rs`](../../crates/orbit-cli/tests/e2e_pitfall_regressions.rs) 固化了过往生产故障的自动化端到端拦截（其中基于 Unix Python mock / 进程组的 6 条通过 `#[cfg(unix)]` 门控，跨平台 3 条在 Linux/macOS/Windows 均运行）：

| 用例名 | 平台 | 覆盖的历史生产故障 |
| --- | --- | --- |
| `pitfall_01_claude_no_off_by_one_on_multi_turn_interrupt_and_queue_flush` | Unix | Claude 多轮打断与排队消息冲刷后回复与轮次严格对齐，不返回上一轮滞后消息 |
| `pitfall_02_codex_no_off_by_one_on_multi_turn_steer_and_interrupt` | Unix | Codex 多轮 `steer` 与打断后当前轮响应精准对齐 |
| `pitfall_03_tail_first_pagination_fetch_before_and_lazy_tool_diff` | 全平台 | 大话会尾部分页（`SubscribeTimeline.tail_limit`、`FetchTimelineBefore`、`TimelineHistoryPage`、`has_older_history`）与超长 Tool Diff 懒加载/截断 |
| `pitfall_04_off_lan_relay_propagation_and_fleet_configure_host_relay` | 全平台 | 非局域网下经 `ConfigureHostRelay` / `apply_relay_to_fleet` 实现跨端 Relay 配置下发与连接对齐 |
| `pitfall_05_mission_workspace_parallel_worktree_isolation_and_handle_alias` | Unix | Mission Workspace 并行开发 worktree 自动隔离，以及 `UpdateAgent` 等路径改名后历史 `@handle` 别名解析 |
| `pitfall_06_late_assistant_finalization_cross_turn_dedup_over_ws` | 全平台 | WebSocket 层跨轮次迟到 assistant finalize 消息去重 |
| `pitfall_07_archive_agent_terminates_only_target_shim_pgid_without_signaling_siblings` | Unix | 归档会话终止目标 `orbit shim` / PGID 时仅杀目标进程组，绝不因缺少 `--` 误触发 `kill(-1)` 波及同级会话或无关进程 |
| `pitfall_08_multi_agent_non_interrupting_delivery_and_stall_recovery` | Unix | Antigravity + Codex + Claude 多代理非打断异步投递（`0 session/cancel` + `Queued -> Dispatched -> Settled` FIFO）、纯思考停滞检测（`latest_visible_progress_ms < latest_activity_ms`）+ 保队列打断继续（`interrupt_agent_and_continue`）及人类抢占重排优先顺序 |
| `pitfall_09_antigravity_interrupt_steer_bg_isolation_subagent_order_and_stderr` | Unix | Antigravity 会话标题实时同步（`session_info_update`）、多命令 Shell 与后台任务隔离防劫持、打断/Steer 事务回滚上下文交接 + 原位 `bg_task` 收敛 + 抢占轮次用量补记（`usage_ledger`）、空队列 `InterruptAndDrainQueue` 干净回 `Idle`、原生子代理前后台实时轮询 + 中途取消 + 重启回放 `seq` 保序及致命 `stderr` 错误提示上浮 |

> **编写规则**：凡修复涉及消息流转、分端状态同步、Relay 建连或并行协作的生产 Bug，必须在同一 PR 中新增最小复现回归用例。

---

`cargo test -p orbit-cli --test e2e_voice` 使用临时目录和已配对的 Daemon 验证语音配置持久化、CLI WAV 转写、客户端回退与缺失授权请求的拒绝。模型完整性与兼容音频端点测试使用本地数据和回环服务。

## 3. Desktop Chrome Runtime Smoke（防前端白屏/React `#185`）

[`apps/desktop/scripts/smoke-render.mjs`](../../apps/desktop/scripts/smoke-render.mjs) 启动真实静态资源服务器与无头 Chrome CDP，校验 0 console/pageerror 与非空 UI 渲染，当前固定验证 3 个场景：

1. `bare-browser-empty-host`
2. `mocked-tauri-active-session`
3. `mocked-tauri-paginated-large-session`（模拟 `total_turn_count: 120` 仅下发尾部 3 轮窗口 `turn_index` 117..119 的分页大图渲染）

---

## 4. 修改 CI Job 时的 Ruleset 锁步要求

**硬性禁止**在未同步更新 GitHub Branch Ruleset 的情况下修改或删除 `ci.yml` 中的任何必填 Check 名称！
若需增删或重命名 `rust-quality-and-unit`、`e2e-regression-matrix` 标签或 `desktop-frontend-build`，**必须在同一 PR 合入前后同步调整 GitHub Ruleset `protect-main-and-master`（ID `27444583`）的 `required_status_checks`**，否则所有 PR 将被永久挂起。

The Rust quality job runs `bash scripts/check-ohos-dts-sync.sh` to verify every N-API method against the checked-in ArkTS declarations, including factories, optional arguments and promise results. After changing the Rust exports, run the script with `--write` to regenerate `index.d.ts`.

[Orbit Patrol’s opt-in workflow](../../.github/workflows/orbit-pr-review.yml.example) runs on read-only pull request events. It builds the reviewer at the trusted base SHA and reads the fork head as Git objects. Download the JSON report from the run artifacts.
