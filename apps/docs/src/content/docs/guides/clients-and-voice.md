---
title: Five Native Clients & Offline Voice
description: One pure-Rust orbit-client-core ViewModel powering Desktop, Web WASM PWA, iOS (+ Live Activity), Android, and HarmonyOS 7+, plus detached raw PTYs and local offline voice input.
---

Orbit refuses to duplicate state machines, protocol parsers, or diff engines across client codebases. **All domain logic lives once in pure Rust (`crates/orbit-client-core` + `crates/orbit-store`)**, while five thin native interfaces focus strictly on platform ergonomics and 120 Hz rendering.

## One Rust ViewModel, five native interfaces

| Client | UI Stack | Bridge to `orbit-client-core` | Platform Highlights |
| --- | --- | --- | --- |
| **Desktop** (macOS / Linux / Windows) | Tauri v2 + React 19 + Tailwind v4 + Base UI + `@xterm/xterm` WebGL | In-process Tauri IPC + typed `Channel` streams | 3-pane GPU layout, CodeMirror 6 + LSP + FIM, system tray, QR pairing, `en`/`zh-CN`/`ja` i18n |
| **Web WASM PWA** | Browser / Installable PWA | `orbit-client-core` compiled to WebAssembly | Zero-install access from any browser or tablet with direct E2EE Noise handshake in WASM |
| **iOS (`16.4+`)** | Native Swift + SwiftUI | Mozilla UniFFI (`OrbitCoreFFI.xcframework`) | Native navigation sheets, **Live Activity / Dynamic Island** real-time fleet progress, haptics |
| **Android (`API 29–36`)** | Kotlin 2.1 + Jetpack Compose Material 3 | Mozilla UniFFI JNI/JNA bindings | Edge-to-edge Material 3, adaptive background reconnection, instant Tier-3 approval notifications |
| **HarmonyOS (`7.0+ / API 26`)** | Stage Model + ArkTS + ArkUI V2 (`@ComponentV2` / `@Trace`) | `ohos-rs` (`napi-ohos`) `liborbit_ohos.so` | Virtualized scrolling (`Repeat().virtualScroll()`), native ArkWeb diagrams, full Contract v2 parity |

## High-performance reactive dataflow

- **Pull-on-Notify deltas**: Instead of re-serializing giant JSON snapshots on every streamed token, `OrbitClientCore` coalesces bursts in SQLite WAL and emits lightweight `CoreNotification` invalidations (`stage_updated`, `fleet_updated`, `git_updated`, `terminal_updated`). Clients pull only changed turns via `agent_stage_delta`.
- **Tail-first pagination for thousand-turn sessions**: `SubscribeTimeline` delivers the tail window immediately with `has_older_history`, while `FetchTimelineBefore` loads earlier history on demand. The **Full-History Outline Rail** projects every prompt anchor (`outline_prompts`) from SQLite's `(agent_id, seq)` index without loading off-page payloads.
- **Native Rust Markdown, Highlighting & Diagrams**: `pulldown-cmark` GFM AST, zero-C `syntect` light/dark syntax highlighting, `similar` structured diffs, and pure-Rust Mermaid (`mermaid-rs-renderer`) + Graphviz DOT (`layout-rs`) SVG rendering with XSS sanitization all run inside Rust Core.
- **Unified SQLite thread preferences**: Pinned threads and read cursors (`seen_seq`) persist in `client_thread_prefs` so unread counts, worktree unread rollups, and wakeup dividers stay consistent everywhere.

## Detached Raw PTY terminals

Orbit treats remote shell access as a first-class sibling to AI agents:

- Managed by `orbit shim` via `portable-pty` and modeled in `orbit-client-core` with a `vt100` screen grid (`terminal_screen_slot`).
- Desktop streams raw binary frames directly into `@xterm/xterm` WebGL with OSC 133 command markers, search, and floating detached terminal windows.
- Mobile clients render crisp synchronized terminal buffers and quick-key bars. Because every PTY runs inside a detached `orbit shim`, restarting `orbit daemon` or switching from Wi-Fi to 5G never interrupts running builds or `tmux` sessions.

## Local Offline Voice Input

Speak naturally to your coding agents or Personal Agent from Desktop or mobile **without streaming microphone audio to a third-party speech cloud**:

- **On-device Voice Activity Detection (VAD) + local Whisper transcription** converts speech into clean, punctuated engineering prompts entirely offline.
- Push-to-talk or hands-free dictation feeds straight into the active `OmniComposer` or routes directly to your **Personal Agent** so you can trigger cross-host tasks, check build status, or answer clarifying questions while away from the keyboard.
