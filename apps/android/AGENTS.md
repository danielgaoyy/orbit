# `apps/android/` 局部工程规范

在遵守 [`../AGENTS.md`](../AGENTS.md) 与 [`docs/CLIENT_BRIDGE_ARCHITECTURE.md`](../../docs/CLIENT_BRIDGE_ARCHITECTURE.md) 的基础上，改动 Android 客户端时必须满足：

1. **SDK 基线与 Contract v2 对齐**：保持与 `app/build.gradle.kts`（`minSdk 29`、`targetSdk 36`）及 Jetpack Compose Material 3 一致，所有领域状态经 UniFFI Contract v2 与 `orbit-client-core` 对齐。
2. **UniFFI 绑定与 JNI 构建**：涉及 Rust Core / `orbit-ffi-uniffi` DTO 调整时，先运行 `bash scripts/sync-bindings.sh` 与 `./apps/android/scripts/build-rust-android.sh` 更新 Kotlin 绑定与 `.so`。
3. **发版版本号规范**：版本升级需同步更新 `app/build.gradle.kts` 的 `versionCode` 与 `versionName`（遵循 [`docs/engineering/git-pr-release-workflow.md`](../../docs/engineering/git-pr-release-workflow.md)）。
4. **本地验证**：
   ```bash
   ./apps/android/scripts/build-rust-android.sh
   cd apps/android && gradle assembleDebug --no-daemon
   ```
