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
   - 单元测试与二进制内部测试（`cargo test --workspace --lib --bins`）
2. **`e2e-regression-matrix`**（4 分片并发执行，共享 `Swatinem/rust-cache@v2` `shared-key: "orbit-rust-e2e"`，固定使用 `TMPDIR=/tmp ... -- --test-threads=4`）：
   - `Pitfall Regressions`：`-p orbit-cli --test e2e_pitfall_regressions`
   - `Relay E2EE & Isolation`：`-p orbit-cli --test e2e_relay_and_pty --test e2e_multi_user_isolation`
   - `Orchestration & Fleet Mesh`：`-p orbit-cli --test e2e_orchestration`
   - `Workspace Sync & Worktree`：`-p orbit-cli --test e2e_workspace_sync_and_worktree`
3. **跨端构建与烟测任务**：
   - `desktop-frontend-build`（`Desktop Frontend Typecheck & Build`）：Node unit tests + `npm run build` + `npm run test:smoke`
   - `tauri-rust-check`（macOS）：Tauri crate fmt / clippy / tests
   - `windows-rust-and-desktop-check`（Windows）：`cargo test --workspace -- --skip live_devbox_ssh_one_click_e2e` + Desktop check
   - `ios-build-and-simulator-smoke`（macOS）：XCFramework + `xcodegen` + Simulator UI/Logic smoke
   - Android UniFFI & Debug APK build

In Release ([`release.yml`](../../.github/workflows/release.yml)) 与 TestFlight ([`testflight.yml`](../../.github/workflows/testflight.yml))：

- **全量门禁后置并行**：`release.yml` 中所有跨平台构建任务（Linux CLI、macOS/Linux/Windows Desktop、Android、iOS、HarmonyOS）与 `rust-quality-and-unit`、`e2e-regression-matrix`、`desktop-frontend-quality` 从 `t = 0` **并发启动**，由最终的 `publish-github-release` 通过 `needs:` 统一拦截——任何质量、E2E 或前端烟测失败均不会创建 GitHub Release。
- **合并 macOS / Windows CLI 与 Desktop 产物**：`build-macos-desktop` 与 `build-windows-desktop` 构建 `orbit` sidecar 后顺带上传 `orbit-cli-macos-arm64` 与 `orbit-windows-x86_64.exe`，消除重复冷编译并减少 1 个 macOS Runner 槽位占用；`build-host-binaries` 仅负责 Linux (`x86_64` / `aarch64`) 静态二进制。
- **轻量 UniFFI Bindgen 与 Release 精简切片**：独立 `crates/orbit-uniffi-bindgen` 负责生成 Swift/Kotlin 绑定，避免移动构建时重编宿主 debug `orbit-ffi-uniffi`；`release.yml` 与 `testflight.yml` 通过 `./apps/ios/scripts/build-rust-xcframework.sh --device-only` 仅构建真机 `aarch64-apple-ios` 切片，Android Release 通过 `./apps/android/scripts/build-rust-android.sh --arm64-only` 搭配 `-PorbitAndroidAbis=arm64-v8a` 仅打 `arm64-v8a` 真机 APK（`ci.yml` 仍保留 iOS Simulator 与 Android 双 ABI 检查）。
- **TestFlight Ubuntu 凭据预检**：`testflight.yml` 先在 `ubuntu-latest` 执行 `check-credentials`，缺失 Apple Secrets 时直接跳过，避免空占 `macos-latest` 队列。
- **统一编译并发与双层缓存**：CI/Release/TestFlight 统一设置 `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`（保留 `lto = "thin"`）；各 Rust Job 标准化 `Swatinem/rust-cache@v2` 的 `shared-key` 并限定 `save-if: ${{ github.ref == 'refs/heads/main' }}`，避免 `v*` Tag 缓存互相不可读及撑爆 10 GB 配额；Desktop 打包 Job 与 Windows CI 额外通过 `mozilla-actions/sccache-action` 使用本地临时 `SCCACHE_DIR` 在同 Job 的 `orbit-cli` 与 `apps/desktop/src-tauri`（对齐 `[profile.release]`）间复用 ~405 个重叠 crate 的编译缓存。

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

## 3. Desktop Chrome Runtime Smoke（防前端白屏/React `#185`）

[`apps/desktop/scripts/smoke-render.mjs`](../../apps/desktop/scripts/smoke-render.mjs) 启动真实静态资源服务器与无头 Chrome CDP，校验 0 console/pageerror 与非空 UI 渲染，当前固定验证 3 个场景：

1. `bare-browser-empty-host`
2. `mocked-tauri-active-session`
3. `mocked-tauri-paginated-large-session`（模拟 `total_turn_count: 120` 仅下发尾部 3 轮窗口 `turn_index` 117..119 的分页大图渲染）

---

## 4. 修改 CI Job 时的 Ruleset 锁步要求

**硬性禁止**在未同步更新 GitHub Branch Ruleset 的情况下修改或删除 `ci.yml` 中的任何必填 Check 名称！
若需增删或重命名 `rust-quality-and-unit`、`e2e-regression-matrix` 标签或 `desktop-frontend-build`，**必须在同一 PR 合入前后同步调整 GitHub Ruleset `protect-main-and-master`（ID `27444583`）的 `required_status_checks`**，否则所有 PR 将被永久挂起。
