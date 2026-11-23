# Orbit

**Orbit** is a pure-Rust, low-latency, multi-host orchestration engine and cross-platform native control plane for AI coding agents.

Run coding agents across cloud Devboxes, Linux workstations, and home Macs simultaneously, then monitor streaming turns, approve tool calls, answer clarifying questions, review Git diffs, manage worktrees, and operate detached remote PTYs from a single native interface:

- **Desktop (macOS / Linux / Windows)**: Tauri v2 + React 19 + TypeScript + Tailwind CSS v4 + Base UI + `@xterm/xterm` WebGL (`apps/desktop`) with guided onboarding, live provider readiness probing, pure-Rust QR code pairing, system tray integration, and built-in localization (`en`, `zh-CN`, `ja`).
- **iOS (`16.4+`)**: Native Swift + SwiftUI (`apps/ios`), linked to Rust via UniFFI.
- **Android (`API 29–36`)**: Native Kotlin 2.1 + Jetpack Compose Material 3 (`apps/android`), linked to Rust via UniFFI.
- **HarmonyOS (`7.0.0 / API 26+`)**: Native Stage model + ArkTS + ArkUI V2 (`apps/harmony`), linked to Rust via `ohos-rs` N-API.
- **CLI**: Single static `orbit` binary (`crates/orbit-cli`) with zero Node.js or Python runtime dependencies.

---

## Key Capabilities

### Single Binary & Zero-Downtime Shim Isolation
- One static `orbit` executable bundles `daemon`, `shim`, `relay`, `pair`, `doctor`, and full orchestration CLI commands.
- Stateful agent sessions and interactive PTYs run inside detached `orbit shim` processes over `$ORBIT_HOME/shims/*.sock`. Upgrading or restarting `orbit daemon` keeps active sessions running uninterrupted.

### Multi-Host Fleet & Shared Rust Client Core
- `orbit-client-core` manages concurrent multi-host connection pools, Happy-Eyeballs smart routing (direct LAN/SSH/Tailscale vs. E2EE Relay), SQLite WAL offline caches, streaming CommonMark/GFM AST parsing (`pulldown-cmark`), native light/dark syntax highlighting (`syntect`), structured Git Diff (`similar`), and VT100 terminal grids (`vt100`).
- Supports multi-agent **Mission Workspaces** with shared `BOARD.md` collaboration, isolated Git worktrees, bidirectional worktree handoff, cron schedules with IANA timezones, conditional timers, lifecycle hooks, and LSP diagnostics/symbols.

### 34+ ACP & First-Class Native Providers
- **Native protocol drivers**: Claude Code (`stream-json` control protocol), Codex (`app-server` JSON-RPC 2.0), Antigravity (`agy` ACP stdio + byte-offset transcript streaming), and Pi (`pi-rpc`).
- **First-class ACP stdio providers**: OpenCode, Gemini CLI, Cursor Agent, GitHub Copilot, Goose, Kimi CLI, Cline, Mistral Vibe, and Augment.
- **Universal ACP Registry**: Curated built-in catalog of 34 ACP-compatible coding agents aligned with `agentclientprotocol/registry`, plus live `initialize` handshake probing and custom agent management.

### Zero-Trust Security Architecture
- **Loopback-by-default daemon**: Binds to `127.0.0.1:6868` by default with persisted `pairing_secret` authentication and strict anti-CSWSH `Origin` validation.
- **Mutually authenticated zero-knowledge relay**: `orbit relay --allow-pubkey <PUBKEY>` enforces `Noise_NKpsk0_25519_ChaChaPoly_SHA256` end-to-end encryption (with contributory X25519 DH validation and active session disconnect on secret rotation), `Noise_NK` Proof-of-Possession on daemon control channels, HMAC-SHA256 tunnel attachment, and Zstd frame compression.
- **Verified delivery & zero-knowledge sync**: Mandatory SHA-256 verification against `SHA256SUMS.txt` on installs and self-upgrades, SSH host-key verification on `ssh+relay` tunnels, and optional client-side encrypted GitHub Gist Cloud Sync (`Argon2id` + `AES-256-GCM`). See [SECURITY.md](SECURITY.md) for details.

---

## Quick Start

### 1. Install the `orbit` CLI / Daemon / Relay (Linux & macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh
```

> Forks and organizations can override the release repository with `ORBIT_GITHUB_REPO=owner/repo` (for both installer scripts and compiled binaries).

### 2. Inspect Host Readiness & Start the Daemon

```bash
orbit doctor        # Inspect identity keys and detected coding-agent CLIs
orbit daemon up     # Install and start the persistent background daemon (127.0.0.1:6868)
orbit pair          # Print an orbit://pair/... URI (or scan the QR code in Orbit Desktop)
```

### 3. Optional: Self-Hosted Zero-Knowledge Relay (BYOR)

On a cloud VM or bastion host:

```bash
orbit relay up --port 7878 --allow-pubkey <YOUR_HOST_PUBKEY>
```

On each workstation / Devbox / Mac mini:

```bash
orbit use-relay <YOUR_RELAY_HOST>:7878
```

See [deploy/README.md](deploy/README.md) for full relay and firewall setup instructions.

---

## Building from Source

### Rust Workspace

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### Desktop App (macOS / Linux / Windows — Tauri v2)

```bash
cd apps/desktop
npm ci
node --experimental-strip-types --test "src/**/*.test.ts"
npm run tauri dev     # Launch desktop app (auto-discovers local 127.0.0.1:6868 daemon)
npm run tauri build   # Build native installer (.dmg / .deb / .msi / .exe)
```

### Mobile Clients

```bash
# iOS (16.4+, SwiftUI + UniFFI)
./apps/ios/scripts/build-rust-xcframework.sh
cd apps/ios && xcodegen generate && open OrbitMobile.xcodeproj

# Android (API 29-36, Jetpack Compose + UniFFI)
./apps/android/scripts/build-rust-android.sh
cd apps/android && gradle assembleDebug --no-daemon

# HarmonyOS 7+ (API 26+, ArkTS + ArkUI V2 + N-API)
./apps/harmony/scripts/build-rust-ohos.sh
```

---

## Documentation & Community

- [docs/INDEX.md](docs/INDEX.md) — Authoritative registry & task routing index for all Orbit specs, skills, and scoped `AGENTS.md`
- [ARCHITECTURE.md](ARCHITECTURE.md) — System design, process isolation, and crate topology
- [CONTRIBUTING.md](CONTRIBUTING.md) — Development setup, architecture redlines, and PR checklist
- [SECURITY.md](SECURITY.md) — Threat model, cryptographic design, and private vulnerability reporting
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) — Community standards (Contributor Covenant v2.1)
- [deploy/README.md](deploy/README.md) — Self-hosted Relay & service supervision guide

## License

Dual-licensed under [MIT](LICENSE) or Apache-2.0.
