---
title: 五端原生客户端与离线语音
description: 一套纯 Rust orbit-client-core ViewModel 驱动 Desktop、Web WASM PWA、iOS（含 Live Activity）、Android 与 HarmonyOS 7+，并内置热升级保活 PTY 与端侧离线语音。
---

Orbit 拒绝在各端客户端重复编写状态机、协议解析器与 Diff 引擎。**所有领域逻辑、本地缓存与投影计算在纯 Rust（`crates/orbit-client-core` + `crates/orbit-store`）中只写一次**，五套极薄原生 UI 专注各自平台的人机交互与 120Hz 渲染质感。

## 一套 Rust ViewModel，五套原生界面

| 客户端 | 原生界面栈 | 与 `orbit-client-core` 的桥接 | 平台亮点 |
| --- | --- | --- | --- |
| **Desktop**（macOS / Linux / Windows） | Tauri v2 + React 19 + Tailwind v4 + Base UI + `@xterm/xterm` WebGL | 进程内 Tauri IPC + 强类型 `Channel` 流 | GPU 三栏平滑布局、CodeMirror 6 + LSP + FIM、系统托盘、纯 Rust 二维码配对、中/英/日三语 |
| **Web WASM PWA** | 现代浏览器 / 可安装 PWA | `orbit-client-core` 编译为 WebAssembly | 零安装打开即用，在浏览器 WASM 沙箱内直接完成 `Noise_NKpsk0` 端到端加密握手 |
| **iOS（`16.4+`）** | 原生 Swift + SwiftUI | Mozilla UniFFI（`OrbitCoreFFI.xcframework`） | 原生导航与触感反馈、**Live Activity / 灵动岛**实时呈现舰队与子代理进度 |
| **Android（`API 29–36`）** | Kotlin 2.1 + Jetpack Compose Material 3 | Mozilla UniFFI JNI/JNA 绑定 | 全面屏 Material 3、切网无阻塞重连、Tier-3 审批即时推送 |
| **HarmonyOS（`7.0+ / API 26`）** | Stage 模型 + ArkTS + ArkUI V2（`@ComponentV2` / `@Trace`） | `ohos-rs`（`napi-ohos`）`liborbit_ohos.so` | `Repeat().virtualScroll()` 虚拟滚动、原生 ArkWeb 图表渲染、完整 Contract v2 对齐 |

## 面向千轮大会话的响应式数据流

- **Pull-on-Notify 增量拉取**：高频流式 Token 与终端输出不会反复序列化全量快照。`OrbitClientCore` 在 SQLite WAL 中合批投影，仅推送轻量级 `CoreNotification` 变更通知，由各端通过 `agent_stage_delta` 增量拉取变更轮次。
- **尾部分页与全历史大纲导航**：`SubscribeTimeline` 首次仅下发尾部窗口并返回 `has_older_history`，上滚按需触发 `FetchTimelineBefore`；**全历史大纲轨**利用 SQLite `(agent_id, seq)` 索引直接投影全量 `outline_prompts`，跳转未加载历史时自动翻页定位。
- **纯 Rust 渲染管线**：流式 CommonMark/GFM AST（`pulldown-cmark`）、零 C 依赖浅/深色语法高亮（`syntect`）、结构化 Diff（`similar`）以及纯 Rust Mermaid（`mermaid-rs-renderer`）+ Graphviz DOT（`layout-rs`）防 XSS SVG 渲染全部下沉在 Rust Core。
- **跨端一致的线程偏好**：置顶与已读游标持久化在 `client_thread_prefs` 表中，确保 Worktree 未读聚合与唤醒分割线在五端表现完全一致。

## 热升级保活的原生 PTY 终端

Orbit 将远程交互式终端视为与 AI Agent 平等的一等公民：

- 由 `orbit shim` 通过 `portable-pty` 托管，在 `orbit-client-core` 内以 `vt100` 网格建模（`terminal_screen_slot`）。
- 桌面端将原始二进制帧直推 `@xterm/xterm` WebGL，支持 OSC 133 命令边界、搜索与独立浮动终端窗口。
- 移动端提供流畅终端网格与快捷功能键；由于 PTY 运行在独立的 `orbit shim` 中，重启 Daemon 或在 Wi-Fi/5G 间切换永不中断正在运行的编译或 `tmux` 会话。

## 端侧离线语音输入

无需把麦克风录音上传任何第三方语音云，即可在桌面端或移动端直接用语音驱动编码代理与 Personal Agent：

- **端侧语音活动检测（VAD）+ 本地 Whisper 转写**：全流程离线运行，将口语自然转化为清晰、带标点的工程指令。
- 支持按住说话（Push-to-Talk）与连续语音输入，直接填充当前 `OmniComposer` 或直达 **Personal Agent**，让你在离开键盘时也能发起跨主机派单、追问构建进度或回复澄清问题。
