# Orbit 系统架构规范 (Architecture v0.2)

## 1. 设计哲学与核心不变量

1. **多机舰队优先（Multi-Host Fleet First）**：
   客户端不假定“只有一个后台”。每台物理/云端主机（如 `Devbox`、`Linux Spark`、`Home Mac mini`）拥有独立的 `HostId` 与静态 X25519 公钥。`orbit-client-core` 并发维护多台主机的连接池与独立单调序列游标（`(HostId, CursorSeq)`）。
2. **共享纯 Rust 无头大脑 + 分端极致原生体验（Shared Rust Core + Tauri Desktop & Native Mobile UIs）**：
   - **客户端核心层（`orbit-client-core`）**：100% 纯 Rust 实现 MVU 状态树（`OrbitAppCore`）、Happy-Eyeballs 网络竞速、Noise NK 端到端加密、SQLite WAL 本地缓存、流式 CommonMark/GFM AST 解析（`pulldown-cmark`）、纯 Rust 零 C 依赖语法着色（`syntect`）、Git Diff 模型（`similar`）及 VT100 终端屏幕网格（`vt100`）。
   - **桌面端（macOS / Linux / Windows）**：统一采用 **Tauri v2 (`2.12`)** + **React 19 + TypeScript + Tailwind CSS v4 + Base UI + `@xterm/xterm` 6.0 WebGL**（位于 `apps/desktop`），直接进程内链接 `orbit-client-core` 并通过 `tauri::ipc::Channel` 实时推送事件与原始终端二进制流。
   - **移动端第一方原生 UI**：
     - **iOS (`16.4+`，与 Paseo 兼容基线对齐)**：原生 **Swift + SwiftUI**（`apps/ios`），经由 Mozilla **UniFFI (`0.32`)** 生成的 `OrbitCoreFFI.xcframework` 直连 Rust 内核。
     - **Android (`minSdk 29` / `targetSdk 36`，与 Paseo 兼容基线对齐)**：原生 **Kotlin 2.1 + Jetpack Compose Material 3**（`apps/android`），经由 **UniFFI** 生成的强类型 JNI/JNA 绑定直连 Rust 内核。
     - **HarmonyOS (`7.0.0(26)` / API 26，仅支持鸿蒙 7 以上)**：原生 Stage 模型 + **ArkTS + ArkUI 状态管理 V2 (`@ComponentV2` / `@ObservedV2` / `@Trace` / `Repeat().virtualScroll()`)**（`apps/harmony`），经由 **`ohos-rs` (`napi-ohos`)** 编译的 `liborbit_ohos.so` 直连 Rust 内核。
3. **守护进程与工作负载解耦（Zero-Downtime Daemon Reload）**：
   `orbit daemon` 绝不直接作为 `agy`、`codex`、`claude` 或用户 `PTY` 的直接父进程。一切有状态子进程必须通过脱离会话组（`setsid`）的 `orbit shim` 托管，并通过 `$ORBIT_HOME/shims/<shim-id>.sock` (Unix Domain Socket) 与 Daemon 通信。
4. **私有零知识中继（Zero-Knowledge Private Relay）**：
   `orbit relay` 仅转发经过 `Noise_NKpsk0_25519_ChaChaPoly_SHA256`（序言 `orbit-relay-e2ee/2`）端到端加密的密文块，并支持 `--allow-pubkey` 公钥白名单硬拦截。

---

## 2. 全栈仓库拓扑

```text
orbit/
├── crates/
│   ├── orbit-protocol/       # 线协议、状态机枚举与 Zstd 帧压缩
│   ├── orbit-crypto/         # Noise_NKpsk0_25519_ChaChaPoly_SHA256 E2EE 与配对凭证
│   ├── orbit-store/          # 嵌入式 SQLite WAL 持久化（Daemon & 客户端本地副本）
│   ├── orbit-shim/           # 热升级不死进程壳 (Unix Domain Socket + portable-pty)
│   ├── orbit-providers/      # Antigravity (agy) / Codex / Claude Code / Pi 与 34+ ACP 驱动
│   ├── orbit-daemon/         # 宿主机后台守护进程
│   ├── orbit-relay/          # < 5MB 零知识端到端加密中继
│   ├── orbit-relay-transport/# 共享 Relay 与 ssh+relay 隧道客户端传输层
│   ├── orbit-client-core/    # 全端共享 MVU 状态机 + 连接池 + Markdown/Highlight/Diff/VT100 引擎
│   ├── orbit-ffi-uniffi/     # UniFFI v0.32 绑定导出（服务 iOS SwiftUI & Android Compose）
│   ├── orbit-uniffi-bindgen/ # 轻量独立 UniFFI CLI 绑定生成器（避免移动构建重编宿主 Core）
│   ├── orbit-ffi-ohos/       # ohos-rs (napi-ohos) 绑定导出（服务 HarmonyOS 7+ ArkUI V2）
│   └── orbit-cli/            # 多合一单静态二进制 CLI (daemon / shim / relay / client)
└── apps/
    ├── desktop/              # 桌面端 (macOS / Linux / Windows): Tauri v2 + React 19 + Base UI + xterm.js
    ├── ios/                  # iOS 16.4+ 原生移动端: Swift + SwiftUI + OrbitCoreFFI.xcframework
    ├── android/              # Android 10+ (API 29..36) 原生移动端: Kotlin 2.1 + Jetpack Compose Material 3
    └── harmony/              # HarmonyOS 7+ (API 26) 原生移动端: Stage 模型 + ArkTS + ArkUI V2
```

> 更多分领域权威规范与跨端桥接细节，见全仓文档总索引 [`docs/INDEX.md`](docs/INDEX.md)。
