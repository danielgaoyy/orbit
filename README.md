# Orbit

**Orbit** 是一个用纯 Rust 编写的超轻量、极低延迟、原生支持多主机舰队（Multi-Host Fleet）的本地 AI Coding Agent 编排与移动远程控制面（iOS & Android）。

无论你的 AI 编程会话运行在云端 **Devbox**、**Linux 工作站**，还是家里的 **Mac mini** 上，你都可以通过 **Flutter 120Hz 原生客户端**（内嵌 Rust `liborbit_ffi` 原生加密与 Zstd 压缩内核）或 `orbit` CLI 随时随地以毫秒级响应查看流式进展、审批工具权限、回答澄清提问、审查 Git Diff、操控原生终端以及监控各账号实时额度。

---

## 核心特性（Phase 1 完备交付）

1. **单静态二进制四合一（`9.1 MB` Single Binary Everything）**：
   同一个 `orbit` 可执行文件同时包含 `daemon`（守护进程）、`shim`（热升级不死宿主壳）、`relay`（零知识端到端加密中继）和 `cli`（命令行客户端）。Linux (`x86_64` / `aarch64`) 与 macOS (`Apple Silicon`) 零 Node.js / Python 依赖即拷即用。
2. **三大王牌 CLI 原生纯 Rust 驱动**：
   - **Antigravity (`agy`)**：原生对接 `agy acp --stdio` + `transcript_full.jsonl` 字节偏移增量流（支持双向权限、`ask_question` & `elicitation/create` 多选问答、跨回合后台自治唤醒防死锁、`/usage` + `/credits` 实时额度探测）。
   - **Codex (`codex`)**：原生对接 `codex app-server` 双向 JSON-RPC 2.0 协议（内置 SQLite 启动并发锁、会话创建与恢复、中途插话 `turn/steer`、权限与问答审批、`~/.codex/auth.json` Wham 实时额度与 Credits 探测）。
   - **Claude Code (`claude`)**：纯 Rust 实现双向 `stream-json` Control Protocol（脱离 Node SDK 包袱，完整支持 `can_use_tool` 权限审批、`AskUserQuestion`、中途 `interrupt`、以及 Linux `.credentials.json` / macOS Keychain 实时额度探测）。
3. **Daemon 热升级零中断（Detached Shim Architecture）**：
   所有 CLI 子进程与 `portable-pty` 伪终端均由独立的 `orbit shim` 守护进程组托管并通过 Unix Domain Socket (`$ORBIT_HOME/shims/*.sock`) 接入 `orbit daemon`。随时重启或升级 Daemon 二进制，正在运行的会话与 PTY 终端不断流、不丢失。
4. **专为跨国/弱网与开源零成本设计的混合网络（BYOR + Multi-Host）**：
   - **Happy-Eyeballs 多路径竞速**：局域网 / Tailscale 直连优先，外网自动无缝切换至私有中继，配合 Zstd 实时帧压缩，跨国传输流量降低 75%+。
   - **私有零知识白名单中继**：`orbit relay --allow-pubkey <KEY>` 采用 `Noise_NK_25519_ChaChaPoly_SHA256` 端到端加密与公钥白名单硬拦截，陌生请求在 HTTP Upgrade 阶段直接 `403` 丢弃，绝不消耗你的服务器内存与昂贵流量。

---

## 快速上手（多机部署指南）

### 1. 一键安装单二进制（Linux x64/ARM64 & macOS Apple Silicon）

```bash
curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh

# 一键体检本机身份公钥、后台守护状态与已安装的 agy / codex / claude
orbit doctor

# 一键并发查询三大 CLI 实时 5h / 周配额与剩余 Credits
orbit quotas
```

### 2. 在你的云开发机 / VPS 上一键拉起私有零知识中继（外网远程穿透）

```bash
# 自动写入 ~/.orbit/relay.json 并注册为后台常驻服务（仅放行你的机器公钥，陌生人 403 零消耗）
orbit relay up --port 7878 --allow-pubkey <你的工作站公钥>

# 后续新增机器（如家里的 Mac mini）时，随时追加授权并热重载：
orbit relay allow <你的MacMini公钥>
```

### 3. 在工作站 / Mac mini 上一键挂载中继并生成配对码

```bash
# 自动执行 WebSocket 连通性与白名单预检、持久化配置、重启后台 Daemon，并直接输出最新配对码！
orbit use-relay <你的云服务器IP>:7878

# 若仅在局域网 / Tailscale 内直连使用：
orbit daemon up
orbit pair
```

### 4. 命令行直接驱动三大 CLI

```bash
orbit run --provider agy "Refactor this module"
orbit run --provider codex "Run cargo test and fix warnings"
orbit run --provider claude "Explain ARCHITECTURE.md"
orbit ls
```

---

## 手机客户端（iOS & Android）构建

- **Android APK**：参见 [`.github/workflows/release.yml`](.github/workflows/release.yml) 中的 `build-android-apk`（通过 `cargo-ndk` 编译 `liborbit_ffi.so` 后执行 `flutter build apk --release --target-platform android-arm64`）。
- **iOS IPA**：无需本地配置 Xcode，推送到 GitHub 后 `build-ios-ipa` 工作流会自动在 `macos-latest` 构建机上交叉编译 `aarch64-apple-ios` 静态库并打包产出 `Orbit-iOS-unsigned.ipa`（支持 AltStore / SideStore / Sideloadly 或 Apple 开发者证书签名安装）。
