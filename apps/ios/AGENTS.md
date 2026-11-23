# `apps/ios/` 局部工程规范

在遵守 [`../AGENTS.md`](../AGENTS.md) 与 [`docs/CLIENT_BRIDGE_ARCHITECTURE.md`](../../docs/CLIENT_BRIDGE_ARCHITECTURE.md) 的基础上，改动 iOS 客户端时必须满足：

1. **UniFFI 绑定同步**：若修改了 `crates/orbit-client-core` 或 `crates/orbit-ffi-uniffi` 中的暴露接口/DTO，动手前先执行 `bash scripts/sync-bindings.sh` 刷新 `apps/ios/Generated/orbit_ffi_uniffi.swift`。
2. **XcodeGen 项目文件管理**：`project.yml` 是 Xcode 工程真实定义；增删 Swift 文件、修改版本号（`MARKETING_VERSION` / `CURRENT_PROJECT_VERSION`）或构建配置后，执行 `cd apps/ios && xcodegen generate`；不要手改生成的 `.pbxproj` 脱离 `project.yml`。
3. **数据存储安全**：数据库与 sidecar 路径必须经由 `OrbitStorageProtection`（如 `prepareProtectedDatabaseURL`）配置备份排除与文件保护，禁止弱化保护等级。
4. **签名与发布**：云端 TestFlight 签名流程遵循 [`deploy/TESTFLIGHT.md`](../../deploy/TESTFLIGHT.md)，严禁将个人 Apple 证书、描述文件或 API Key 提交进仓库。
5. **本地验证（macOS arm64 Simulator）**：
   ```bash
   ./apps/ios/scripts/build-rust-xcframework.sh
   cd apps/ios && xcodegen generate
   xcodebuild -project OrbitMobile.xcodeproj -scheme OrbitMobile -configuration Debug \
     -destination "platform=iOS Simulator,id=<SIM_ID>" ARCHS=arm64 ONLY_ACTIVE_ARCH=YES \
     -only-testing:OrbitMobileLogicTests CODE_SIGN_ALLOWED=NO test
   ```
