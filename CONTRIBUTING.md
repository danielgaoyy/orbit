# Contributing to Orbit

Thank you for your interest in contributing to Orbit. This guide covers the repository structure, architectural invariants, local development workflows, and pull request expectations.

By participating in this project, you agree to abide by the [Code of Conduct](CODE_OF_CONDUCT.md). To report a security issue, please follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

## Repository Layout

```text
orbit/
├── crates/
│   ├── orbit-protocol/         # Wire protocol, state machines, Zstd framing, ORBIT_GITHUB_REPO config
│   ├── orbit-crypto/           # Noise_NKpsk0 E2EE, Relay PoP, pairing credentials, SHA-256, Cloud Vault
│   ├── orbit-store/            # Embedded SQLite WAL persistence (daemon & client offline cache)
│   ├── orbit-shim/             # Detached hot-upgrade process host (Unix socket + portable-pty)
│   ├── orbit-providers/        # Native drivers + Universal ACP registry (34+ agents) & readiness probes
│   ├── orbit-daemon/           # Host background daemon (HTTP/WS, orchestration, cron, git, LSP)
│   ├── orbit-relay/            # Zero-knowledge E2EE relay with --allow-pubkey admission control
│   ├── orbit-relay-transport/  # Shared relay & ssh+relay tunnel client transport
│   ├── orbit-client-core/      # Shared pure-Rust client MVU core, routing, Markdown/syntect/diff/VT100
│   ├── orbit-ffi-uniffi/       # UniFFI bindings for native iOS (SwiftUI) and Android (Compose)
│   ├── orbit-ffi-ohos/         # ohos-rs (N-API) bindings for HarmonyOS 7+ (ArkUI V2)
│   └── orbit-cli/              # Single static binary bundling daemon, shim, relay, and CLI
├── apps/
│   ├── desktop/                # Tauri v2 + React 19 + TypeScript + Tailwind CSS v4 + Base UI + xterm.js
│   ├── ios/                    # Native SwiftUI client (iOS 16.4+)
│   ├── android/                # Native Kotlin 2.1 + Jetpack Compose Material 3 client (API 29-36)
│   └── harmony/                # Native ArkTS + ArkUI V2 client (HarmonyOS 7.0.0 / API 26+)
├── deploy/                     # Installer scripts, service units, signing & relay deployment guides
└── docs/                       # Architecture and sync specifications
```

## Architecture Redlines

All contributions must preserve the core invariants defined in [AGENTS.md](AGENTS.md):

1. **Single Static Binary**: `orbit` (`crates/orbit-cli`) bundles `daemon`, `shim`, `relay`, `pair`, `doctor`, and client subcommands into one executable with zero Node.js or Python runtime dependencies.
2. **Shim Process Isolation**: Stateful coding-agent sessions and interactive PTYs must run under detached `orbit shim` processes communicating over `$ORBIT_HOME/shims/*.sock`. Upgrading or restarting `orbit daemon` must never kill running agents or terminals.
3. **Multi-Host Fleet First**: Protocol types, cursors, connection pools, and UI state in `orbit-client-core` must treat multiple simultaneously connected hosts as first-class. Do not introduce single-host singleton assumptions.
4. **Zero-Knowledge Security Boundary**: `orbit-relay` must never handle plaintext payloads. Remote traffic stays end-to-end encrypted (`Noise_NKpsk0_25519_ChaChaPoly_SHA256`) with `--allow-pubkey` admission control; local daemon access defaults to loopback with `pairing_secret` and `Origin` checks.
5. **No `unsafe` and No Production `.unwrap()`**: The workspace enforces `unsafe_code = "deny"` and `clippy::unwrap_used = "warn"`. Propagate errors explicitly using `thiserror` or `anyhow` in production code.

## Fork & Organization Configuration (`ORBIT_GITHUB_REPO`)

All GitHub release, self-update, and remote installer URLs resolve through a single centralized helper: `orbit_protocol::configured_github_repo()` (default: `danielgaoyy/orbit`).

- **Rust builds / runtime**: Set `ORBIT_GITHUB_REPO=owner/repo` at compile time or runtime.
- **Installer scripts**: Set `ORBIT_GITHUB_REPO=owner/repo` when running `deploy/install.sh` or `deploy/install-desktop-linux.sh`.

Values are strictly validated against `^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$` (rejecting `.` segments, `..` traversal, whitespace, and shell/URL metacharacters).

## Local Development

### Prerequisites

- **Rust**: Stable toolchain with `rustfmt` and `clippy` (Edition 2024).
- **Desktop (`apps/desktop`)**: Node.js 22+ and npm; platform Tauri v2 prerequisites (Xcode Command Line Tools on macOS, WebKit2GTK 4.1 headers on Linux, MSVC/WebView2 on Windows).
- **iOS (`apps/ios`)**: macOS with Xcode and `xcodegen`.
- **Android (`apps/android`)**: JDK 21, Gradle 8.13+, and Android NDK (`ANDROID_NDK_HOME`).
- **HarmonyOS (`apps/harmony`)**: DevEco Studio / HarmonyOS 7 (API 26) SDK.

### Common Verification Commands

```bash
# 1. Rust formatting, Clippy (warnings denied), and workspace tests
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# 2. Local CLI self-check
cargo run -p orbit-cli -- doctor

# 3. Desktop frontend unit tests, production build, and headless runtime smoke test
cd apps/desktop
npm ci
node --experimental-strip-types --test "src/**/*.test.ts"
npm run build
npm run test:smoke
cd ../..

# 4. Tauri Desktop Rust crate formatting, Clippy, and tests
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml

# 5. Installer shell script syntax verification (Bash & POSIX sh)
bash -n deploy/install.sh deploy/install-desktop-linux.sh
sh -n deploy/install.sh deploy/install-desktop-linux.sh
```

### Mobile Client Builds

```bash
# iOS (macOS)
./apps/ios/scripts/build-rust-xcframework.sh
cd apps/ios && xcodegen generate

# Android
./apps/android/scripts/build-rust-android.sh
cd apps/android && gradle assembleDebug --no-daemon

# HarmonyOS 7+
./apps/harmony/scripts/build-rust-ohos.sh
```

## Pull Request Guidelines

- **PR-First policy (zero direct push to `main`)**: Every code, doc, CI, and release bump change must go through a Pull Request to `main` with all required CI checks passing; never bypass branch protection. Release tags (`v*`) are only cut from already-merged `main` commits per [`docs/engineering/git-pr-release-workflow.md`](docs/engineering/git-pr-release-workflow.md).
- **Pre-read & sync documentation**: Read the matching authoritative doc/Skill in [`docs/INDEX.md`](docs/INDEX.md) before editing, and keep docs + `docs/INDEX.md` updated in the same PR when architecture, contracts, design rules, or CI jobs change.
- **Keep PRs focused**: Prefer small, self-contained changes over sweeping multi-concern diffs.
- **Include tests**: Bug fixes should add a regression test; new protocol, crypto, security, or UI logic should include unit tests.
- **Follow commit conventions**: Use clear, scoped prefixes such as `feat(daemon): ...`, `fix(desktop): ...`, `security(relay): ...`, or `chore(ci): ...`.
- **Keep CI green**: Run the relevant local checks above before requesting review. All GitHub Actions gates in `.github/workflows/ci.yml` must pass.
- **UI copy & localization**: Keep default Desktop UI copy in restrained English (`apps/desktop/src/lib/i18n/locales/en.ts`) and maintain full key and placeholder parity across `zh-CN.ts` and `ja.ts` (enforced by `i18n.test.ts`).
