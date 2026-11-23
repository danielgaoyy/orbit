# `apps/harmony/` 局部工程规范

在遵守 [`../AGENTS.md`](../AGENTS.md) 与 [`docs/CLIENT_BRIDGE_ARCHITECTURE.md`](../../docs/CLIENT_BRIDGE_ARCHITECTURE.md) 的基础上，改动 HarmonyOS 客户端时必须满足：

1. **平台与 N-API 契约**：面向 HarmonyOS 7+（API 26，`ArkTS` + `ArkUI V2` Stage 模型），通过 `crates/orbit-ffi-ohos` 编译 `liborbit_ohos.so` 并与 `entry/src/main/cpp/types/liborbit_ohos/index.d.ts` 保持 Contract v2 JSON DTO 严格一致。
2. **严禁提交本机签名配置**：DevEco Studio 会在 `build-profile.json5` 写入本机绝对签名路径，提交前务必检查 `git diff`；CI 构建签名行为遵循 [`deploy/HARMONY_SIGNING.md`](../../deploy/HARMONY_SIGNING.md) 与 `scripts/strip-signing.py`。
3. **发版版本号规范**：同步更新 `AppScope/app.json5` 的 `versionCode` 与 `versionName`。
4. **Rust N-API 构建检查**：
   ```bash
   ./apps/harmony/scripts/build-rust-ohos.sh
   ```
