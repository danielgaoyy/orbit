# `apps/harmony/` 局部工程规范

在遵守 [`../AGENTS.md`](../AGENTS.md) 与 [`docs/CLIENT_BRIDGE_ARCHITECTURE.md`](../../docs/CLIENT_BRIDGE_ARCHITECTURE.md) 的基础上，改动 HarmonyOS 客户端时必须满足：

1. **平台与 N-API 契约**：面向 HarmonyOS 7+（API 26，`ArkTS` + `ArkUI V2` Stage 模型），通过 `crates/orbit-ffi-ohos` 编译 `liborbit_ohos.so` 并与 `entry/src/main/cpp/types/liborbit_ohos/index.d.ts` 保持 Contract v2 JSON DTO 严格一致。
2. **严禁提交本机签名配置**：DevEco Studio 会在 `build-profile.json5` 写入本机绝对签名路径，提交前务必检查 `git diff`；CI 构建签名行为遵循 [`deploy/HARMONY_SIGNING.md`](../../deploy/HARMONY_SIGNING.md) 与 `scripts/strip-signing.py`。
3. **发版版本号规范**：同步更新 `AppScope/app.json5` 的 `versionCode` 与 `versionName`。
4. **Rust N-API 构建检查**：
   ```bash
   ./apps/harmony/scripts/build-rust-ohos.sh
   ```

5. **声明与 DTO**。运行 `bash scripts/check-ohos-dts-sync.sh --print-count` 检查 N-API 声明（既有导出顺序由脚本内的 `legacy_core_order` 固定，新导出自动追加在末尾，勿手工改写该列表）。运行 `cargo run -p orbit-ffi-ohos --bin export_dto_fixtures` 从 Rust view 生成样本，再运行 `node scripts/check-harmony-dto-contract.mjs` 核对 ArkTS 顶层字段。迁移期间支持 `model/OrbitTypes.ets` 和 `model/dto/*.ets`。
6. **流式性能回放**。仅 perf 构建可用，使用 `./apps/harmony/scripts/build-rust-ohos.sh --arch arm64 --features perf-hooks`。先订阅事件，再调用 `debugReplayStageDeltasJson(hostId, agentId, framesJson)`。录制数组上限 8 MiB、5000 帧，内置 `scripts/perf/fixtures/stream-500.json` 包含 100 轮、500 帧。回放按 40 帧/秒发出 `timeline_changed`，通过同一 host/agent 的常规 stage delta 接口拉取。回放结束后传入 `[]` 释放该会话的录制状态。普通构建调用返回 `perf hooks disabled`。
7. **本机 Hvigor 打包与 JRE 兜底**：macOS 本机未配置全局 JDK 时，`hvigorw` 的 `PackageHap` 需要 DevEco 内置 JBR：
   ```bash
   export DEVECO="/Applications/DevEco-Studio.app/Contents"
   export JAVA_HOME="${JAVA_HOME:-$DEVECO/jbr/Contents/Home}"
   export DEVECO_SDK_HOME="$DEVECO/sdk"
   export OHOS_NDK_HOME="$DEVECO_SDK_HOME/default/openharmony"
   export PATH="$JAVA_HOME/bin:$DEVECO/tools/ohpm/bin:$DEVECO/tools/node/bin:$DEVECO/sdk/default/openharmony/toolchains:$PATH"
   ```

8. **状态拉取与恢复**。Fleet、Host、Sidebar 和 Stage 的大载荷使用异步 N-API。Stage 按 `(host_id, agent_id)` 保存两个 revision 与窗口起点，`aux_unchanged` 时复用五个辅助字段。普通 `fleet_changed` 只刷新 Fleet 与 Briefing；收到 `resync_required` 时按可见 scope 重新拉取。SQLite 以单键 `orbit.drafts.v1` 保存整个草稿包，包内使用会话键 `${hostId}::${agentId}` 或新会话键 `draft::${hostId}`。详见[客户端桥接契约](../../docs/CLIENT_BRIDGE_ARCHITECTURE.md#harmonyos-n-api)。
9. **三语资源与静态子代理头像生成**。文案由 `node scripts/gen-harmony-strings.mjs` 从桌面三语词典 + `apps/harmony/i18n-keys.txt` + `apps/harmony/i18n-harmony-only.json` 生成，使用 `node scripts/gen-harmony-strings.mjs --check` 与 `bash scripts/check-harmony-i18n.sh` 校验（禁止遗留术语与裸英文）；子代理静态 SVG 头像与深浅底色由 `node scripts/gen-harmony-avatars.mjs` 从 `apps/desktop/src/lib/subagent-avatar.ts` 生成并使用 `--check` 校验。
