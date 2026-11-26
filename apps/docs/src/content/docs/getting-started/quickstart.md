---
title: Quickstart
description: Install the single static Orbit binary, bring up your first daemon, pair your devices, and activate your Personal Agent in five minutes.
---

Orbit ships as a **single static Rust binary** (`orbit`) with zero Node.js or Python runtime dependencies. Every role (`daemon`, `shim`, `relay`, `pair`, `doctor`, and the multi-host orchestration CLI) compiles into one portable executable.

## 1. Install the `orbit` binary

On **macOS (Apple Silicon)** and **Linux (`x86_64` / `aarch64`)**:

```bash
curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh
```

The installer enforces HTTPS, restricts downloads to GitHub release hosts, and verifies the binary's SHA-256 digest against `SHA256SUMS.txt` before installation. Forks and enterprise mirrors can override the source repository via `ORBIT_GITHUB_REPO=owner/repo`.

:::tip[Importing existing IDE / CLI setups]
Run `orbit migrate` to preview importing MCP servers and rules from Claude Code, Codex, Antigravity, Cursor, or VS Code, then pass `--apply` to commit the migration.
:::

## 2. Inspect host readiness & start the daemon

```bash
orbit doctor        # Verify host Ed25519/X25519 keys, kernel sandbox backend, and installed AI CLIs
orbit daemon up     # Install and start the persistent loopback daemon on 127.0.0.1:6868
orbit pair          # Print an orbit://pair/... URI and scannable terminal QR code
```

What happens automatically when `orbit daemon up` runs:

- **Loopback-by-default binding**: Listens strictly on `127.0.0.1:6868` with a persisted `pairing_secret` and anti-CSWSH `Origin` validation.
- **Zero-downtime shim supervision**: Stateful agents and PTYs are spawned inside detached `orbit shim` processes (`setsid`) communicating over `$ORBIT_HOME/shims/*.sock`. Future `orbit upgrade` restarts never terminate running agents.
- **Windows supervision**: On Windows, `orbit daemon up` registers an interactive-token user task; `orbit daemon up --service` installs a LocalService SCM service with restart recovery.

## 3. Connect Orbit Desktop, Web WASM, or Mobile

1. Launch **Orbit Desktop** (macOS / Linux / Windows) — it auto-discovers the local `127.0.0.1:6868` daemon immediately.
2. On **iOS**, **Android**, **HarmonyOS**, or **Web WASM PWA**, scan the QR code printed by `orbit pair` (or paste the `orbit://pair/...` URI).
3. Repeat step 1–2 on your Linux build box, home Mac mini, or cloud GPU VM to add them into your multi-host fleet.

## 4. Enable remote Off-LAN access (optional)

Orbit routes directly over LAN, Tailscale, or SSH tunnels whenever reachable. For seamless Off-LAN access behind home or corporate NATs, attach a self-hosted `< 6 MB` zero-knowledge Relay:

```bash
# On a small cloud VM or bastion host
orbit relay up --port 7878 --allow-pubkey <YOUR_HOST_PUBKEY>

# On each workstation / Devbox / Mac Studio
orbit use-relay <YOUR_RELAY_HOST>:7878
```

Or synchronize your encrypted host directory across all devices via zero-knowledge GitHub Secret Gist Cloud Sync:

```bash
orbit login --mesh
```

## 5. Activate your Personal Agent

Instead of manually juggling separate threads across machines:

1. Open **Settings → App → Personal Agent** in Orbit Desktop.
2. Elect any always-on host in your fleet (such as a home Mac Studio or cloud Devbox) as your global **Personal Agent** (`chief-of-staff`).
3. Choose your preferred coordinator provider (`Claude Code`, `Codex`, `Antigravity`, or any ACP agent) and optionally connect **Slack Socket Mode**, **Discord Gateway v10**, or **Feishu / Lark** with zero public webhooks.

Your Personal Agent maintains a live incremental view of every online machine, dispatches work across hosts into isolated Git worktrees, and pings you only when an irreversible **Tier-3 human decision** (such as a production push, deploy, or secret rotation) is required.

## Next steps

- [Personal Agent & IM Dispatch](../../guides/personal-agent/) — Configure cross-host orchestration and zero-webhook IM cards.
- [Multi-Host Fleet & E2EE Relay](../../guides/multi-host-fleet/) — Deep-dive on Happy-Eyeballs routing, Peer-Bridge, and Cloud Sync.
- [Security, Sandbox & Audit Chain](../../guides/security-sandbox/) — Harden OS kernel sandboxes, egress allowlists, and signed audit logs.
