---
title: CLI, Architecture & Telemetry
description: Complete orbit CLI subcommand reference, 13-crate pure-Rust workspace architecture, Client Bridge Protocol v2, pricing/auto-compaction, and OpenMetrics/OTLP observability.
---

## `orbit` CLI subcommand reference

All commands live inside the single static `orbit` binary (`crates/orbit-cli`).

### Host, Daemon & Fleet Mesh

| Command | Purpose |
| --- | --- |
| `orbit doctor [--bundle PATH]` | Inspect host identity keys, kernel sandbox enforcement, relay status, and detected AI CLIs; `--bundle` exports a redacted diagnostic JSON bundle (even on a stopped host). |
| `orbit daemon up \| status \| restart \| down` | Manage the host-local persistent daemon (`127.0.0.1:6868` by default; supports `--service` for Windows SCM). |
| `orbit relay up \| allow \| status \| down` | Run and manage a self-hosted `< 6 MB` Zero-Knowledge E2EE Relay (`--allow-pubkey` allowlisting). |
| `orbit use-relay [HOST:PORT \| SSH_ALIAS]` | Attach to a relay, auto-provision over `~/.ssh/config`, or `--clear` to revert to direct LAN/Tailscale. |
| `orbit pair [--rotate-secret]` | Print a multi-host `orbit://pair/...` URI and terminal QR code; `--rotate-secret` rotates credentials and disconnects active E2EE sessions. |
| `orbit login \| sync \| logout` | Zero-server encrypted GitHub Secret Gist Cloud Sync (`--mesh` wires `connected_hosts` across your fleet). |
| `orbit upgrade [--all] [--force]` | SHA-256-verified binary self-upgrade and hot-restart (`--all` upgrades connected remote fleet hosts first). |

### Orchestration, Worktrees & Automation

| Command | Purpose |
| --- | --- |
| `orbit run -p <provider> [--worktree] "<prompt>"` | Launch and stream (or `--no-wait`) an agent session across `claude`, `codex`, `antigravity`, `pi`, or `--provider acp --acp-agent <id>`. |
| `orbit ls \| inspect \| logs \| send \| wait \| stop` | List, inspect, stream history, send follow-ups, wait for completion, or interrupt running agents. |
| `orbit fork <agent_id> [--worktree] [--at-seq N]` | Branch an existing conversation in place or into a fresh isolated Git worktree. |
| `orbit workspace / script / terminal` | Manage Mission Workspaces, worktrees, `orbit.json` dev services, and detached `portable-pty` terminals. |
| `orbit schedule / heartbeat` | Create and manage IANA-timezone cron schedules and periodic agent heartbeats. |
| `orbit audit verify \| export` | Verify SHA-256 + Ed25519 host security history or export ascending JSONL logs. |
| `orbit migrate [--from <ide>] [--apply]` | Preview or apply project/user MCP & rules migration from Claude Code, Codex, Antigravity, Cursor, or VS Code. |
| `orbit quotas` | Inspect live 5-hour / weekly usage quotas across Antigravity, Codex, and Claude Code. |

## Rust Workspace Topology (13 Crates + 5 Clients)

```text
orbit/
├── crates/
│   ├── orbit-protocol/        # Wire protocol, state enums, Zstd frame compression
│   ├── orbit-crypto/          # Noise_NKpsk0_25519_ChaChaPoly_SHA256 E2EE & pairing
│   ├── orbit-store/           # Embedded SQLite WAL (daemon, client replica, FTS5 memory, audit chain)
│   ├── orbit-shim/            # Detached setsid process shell (Unix Socket + portable-pty + egress proxy)
│   ├── orbit-providers/       # Native (Claude, Codex, Antigravity, Pi) + 34+ ACP registry drivers
│   ├── orbit-daemon/          # Host daemon (Personal Router, IM longconn, Sandbox, Guardian, LSP, Git)
│   ├── orbit-relay/           # < 6 MB zero-knowledge blind E2EE relay
│   ├── orbit-relay-transport/ # Shared Relay & ssh+relay tunnel transport
│   ├── orbit-client-core/     # Shared MVU ViewModel, connection pool, Markdown/Highlight/Diff/VT100/Diagram
│   ├── orbit-ffi-uniffi/      # UniFFI v0.32 bindings (iOS SwiftUI & Android Jetpack Compose)
│   ├── orbit-uniffi-bindgen/  # Standalone UniFFI CLI binding generator
│   ├── orbit-ffi-ohos/        # ohos-rs N-API bindings (HarmonyOS 7+ ArkUI V2)
│   └── orbit-cli/             # Single static multi-role binary
└── apps/
    ├── desktop/               # Tauri v2 + React 19 + Base UI + CodeMirror 6 + xterm.js WebGL
    ├── web/ (WASM PWA)        # Browser & installable PWA powered by orbit-client-core WASM
    ├── ios/                   # Swift + SwiftUI + Live Activity
    ├── android/               # Kotlin 2.1 + Jetpack Compose Material 3
    ├── harmony/               # Stage Model + ArkTS + ArkUI V2
    └── docs/                  # Astro Starlight bilingual showcase & documentation site
```

## Process Isolation Invariant (`orbit shim`)

`orbit daemon` never spawns `claude`, `codex`, `agy`, `pi`, ACP agents, or user PTYs as direct child processes. Every stateful workload runs inside a session-leader (`setsid`) `orbit shim` speaking over `$ORBIT_HOME/shims/<shim-id>.sock`:

- Upgrading or restarting `orbit daemon` re-attaches seamlessly to active shims using persisted sequence cursors.
- Under workspace-wide `unsafe_code = "deny"`, Unix process-group termination validates `pgid > 1` and passes `--` before negative PGIDs (`/bin/kill -s TERM -- -<pgid>`), preventing `getopt_long` misparsing on modern Linux (`procps-ng`).

## Cost Budgets, Auto-Compaction & Telemetry

- **Pricing & UTC Budgets**: Built-in API token pricing estimates can be overridden per model or `provider/model` in `$ORBIT_HOME/model-pricing.json`. Configure `daily_budget_usd` and `monthly_budget_usd` in `config.json` for UTC-period budget alerts.
- **Auto-Compaction**: Set `auto_compact_threshold_pct` (`50`–`95`) in `config.json` to compact long contexts automatically, or call `CompactAgentContext` on any idle agent.
- **OpenMetrics Scraping**: `GET /metrics` (restricted strictly to loopback peer + validated `Origin` + active `pairing_secret` Bearer token) exposes aggregate operational counters.
- **OTLP Log Export & Structured Logs**: Set `OTEL_EXPORTER_OTLP_ENDPOINT` or `telemetry_endpoint` in `config.json` to emit batched static-metadata logs to `<base>/v1/logs`, and set `ORBIT_LOG_FORMAT=json` for structured JSON host logs.
