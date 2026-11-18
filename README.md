# Orbit

**Orbit** 是一个基于纯 Rust 核心构建的超轻量、极低延迟、原生支持多主机舰队（Multi-Host Fleet）的本地 AI Coding Agent 编排与全平台远程控制面。

无论你的 AI 编程会话运行在云端 **Devbox**、**Linux 工作站**，还是家里的 **Mac mini** 上，你都可以通过：
- **桌面端（macOS / Linux / Windows）**：基于 **Tauri v2** + **React 19 / TypeScript / Tailwind CSS v4 / shadcn/ui / `@xterm/xterm` WebGL** 的轻量级原生桌面控制台（`apps/desktop`）；
- **移动端第一方原生 UI**：
  - **iOS (`16.4+`)**：纯原生 **Swift + SwiftUI** 客户端（`apps/ios`，经 UniFFI 嵌入 Rust 内核）；
  - **Android (`minSdk 29` / `targetSdk 36`)**：纯原生 **Kotlin 2.1 + Jetpack Compose Material 3** 客户端（`apps/android`，经 UniFFI 嵌入 Rust 内核）；
  - **HarmonyOS (`7.0.0(26)`，仅支持鸿蒙 7 及以上)**：纯原生 **ArkTS + ArkUI V2 (`@ComponentV2` / `@ObservedV2`)** 客户端（`apps/harmony`，经 `ohos-rs` 嵌入 Rust 内核）；
- 或 **`orbit` 单文件 CLI**

随时随地以毫秒级响应查看流式进展、审批工具权限、回答澄清提问、审查 Git Diff、操控原生远程终端以及监控各账号实时额度。

---

## 核心特性

1. **单静态二进制四合一（Single Binary Everything）**：
   同一个 `orbit` 可执行文件同时包含 `daemon`（守护进程）、`shim`（热升级不死宿主壳）、`relay`（零知识端到端加密中继）和 `cli`（命令行客户端）。零 Node.js / Python 运行时依赖。
2. **共享纯 Rust 客户端引擎（`orbit-client-core`）**：
   多主机连接池、Happy-Eyeballs 智能路由、Noise NK 端到端加密、SQLite WAL 离线秒开缓存、流式 Markdown AST 解析（`pulldown-cmark`）、纯 Rust 语法高亮（`syntect`）、结构化 Git Diff（`similar`）以及 VT100 终端屏幕网格（`vt100`）全部在纯 Rust 内核统一完成。
3. **三大王牌 CLI 原生纯 Rust 驱动**：
   - **Antigravity (`agy`)**：原生对接 `agy acp --stdio` + `transcript_full.jsonl` 字节偏移增量流。
   - **Codex (`codex`)**：原生对接 `codex app-server` 双向 JSON-RPC 2.0 协议。
   - **Claude Code (`claude`)**：纯 Rust 实现双向 `stream-json` Control Protocol。
4. **私有零知识白名单中继**：
   `orbit relay --allow-pubkey <KEY>` 采用 `Noise_NK_25519_ChaChaPoly_SHA256` 端到端加密与公钥白名单硬拦截，配合 Zstd 实时帧压缩降低 75%+ 跨国流量。

---

## 快速构建指南

### 1. Rust 服务端与全量测试
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### 2. 桌面端（macOS / Linux / Windows — Tauri v2 + shadcn/ui）
```bash
cd apps/desktop
npm install
npm run tauri dev     # 启动桌面开发窗口（自动发现本机 127.0.0.1:6868 Daemon）
npm run tauri build   # 构建当前桌面平台的原生安装包 (.dmg / .deb / .msi)
```

### 3. iOS 原生端（SwiftUI，兼容 iOS 16.4+）
```bash
./apps/ios/scripts/build-rust-xcframework.sh
cd apps/ios && xcodegen generate
open OrbitMobile.xcodeproj
```

### 4. Android 原生端（Jetpack Compose Material 3，minSdk 29 / targetSdk 36）
```bash
./apps/android/scripts/build-rust-android.sh
# 使用 Android Studio 打开 apps/android 或运行 gradle assembleRelease
```

### 5. HarmonyOS 7+ 原生端（ArkTS + ArkUI V2，compatibleSdkVersion 7.0.0(26)）
```bash
./apps/harmony/scripts/build-rust-ohos.sh
# 使用 DevEco Studio (HarmonyOS 7 / API 26 SDK) 打开 apps/harmony 构建 HAP
```
