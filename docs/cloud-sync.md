# Orbit Zero-Server E2EE Cloud Sync (GitHub Secret Gist)

Orbit provides multi-device login and automatic configuration sync **without any Orbit-hosted backend server, user database, or infrastructure cost** using the **BYOS (Bring Your Own Storage)** model:

- **Identity & Auth**: GitHub OAuth 2.0 Device Authorization Grant (RFC 8628, public `client_id`, no `client_secret`) or a GitHub Personal Access Token with only the **`gist`** scope.
- **Storage**: Two private user-owned **GitHub Secret Gists**:
  1. `orbit-cloud-vault.v1.enc.json` (`orbit-cloud-vault/v1`) — Zstd-compressed, AES-256-GCM encrypted fleet, projects, preferences, and relay topology.
  2. `orbit-cloud-vault-key.v1.json` (`orbit-auto-unlock-key-v1`) — **Dual-Gist Instant Unlock** key envelope (bound to your GitHub account, enabled by default so new sign-ins unlock and sync immediately with **zero passphrase friction**). Users who prefer passphrase-only isolation across devices can enable **Require manual passphrase on new devices** (which deletes `orbit-cloud-vault-key.v1.json` and requires Argon2id passphrase entry on fresh logins).
- **1-Scan Device Link QR (`orbit://sync-login?v=1&p=...`) & Token Reuse**: Already-signed-in Desktop / CLI / mobile instances can export a self-contained QR/URI payload carrying the active GitHub session + master vault key + fleet Relay URL. Scanning it on iOS or running `orbit login --device-link-stdin` on a CLI host signs in, unlocks, and hydrates the full fleet in **one step without minting an extra GitHub OAuth token** (avoiding GitHub's 10-token-per-OAuth-App eviction limit). `orbit login` also automatically reuses an existing valid GitHub token unless `--reauth` is passed.
- **Zero-Knowledge Ciphertext**: All paired hosts, pinned projects, project order, user preferences, and account-level fleet network topology are Zstd-compressed and encrypted client-side with **AES-256-GCM** (and **Argon2id 19 MiB, t=2, p=1** when using a Sync Passphrase) before upload.
- **Conflict Resolution**: Deterministic per-entity Last-Write-Wins (LWW) with soft-delete tombstones across hosts, projects, manual order, and preferences. Machine-local loopback endpoints (`127.0.0.1` / `localhost` / `[::1]`) are automatically stripped before upload, while shareable LAN/Tailscale endpoints advertised via `HostInfo.direct_endpoints` merge automatically.
- **Zero-Config Fleet Network Fabric**:
  - `CloudVaultPayload.fleet_network` synchronizes an account-wide primary relay (`primary_relay_url` with dynamic `?ft=` fleet admission token, plus `relay_urls` and `primary_relay_host_id`) via LWW timestamp (`updated_at_ms`), while per-host `SyncedHostEntry.bridge_host_ids` unions Peer-Bridge reachability metadata across devices (proactively recorded whenever peers join the online mesh).
  - When Cloud Sync has SSH-paired hosts and no relay is configured yet, desktop auto-provisions a private `orbit relay` over SSH using `--fleet-token-stdin` (no secrets in `argv`, stored `0600`), or users can deploy a **Zero-Port Cloudflare Tunnel Relay** (`orbit relay tunnel` / Desktop UI) with zero inbound firewall ports.
  - Legacy allowlist-only `ssh+relay://` URLs without `?ft=` are never sprayed onto unauthorized peers and are automatically upgraded across online hosts once an authenticated fleet Relay (`?ft=` / Cloudflare Tunnel) is configured.
  - **Peer-Bridged Multi-Hop E2EE Tunneling**: LAN-isolated hosts unreachable from the internet can be reached transparently through any online Peer-Bridge Daemon via `/ws/e2ee` and `PeerBridgeOpen` / `PeerBridgeData` frames (staggered 600 ms race against direct/relay paths); the bridge daemon forwards strictly opaque `Noise_NKpsk0` ciphertext.
  - **Instant Cross-Device Convergence**: Cloud Sync uses conditional HTTP polling (`If-None-Match` ETag against GitHub Gist) plus account-wide `NotifyFleetVaultChanged` / `FleetVaultChanged` daemon broadcasts whenever another device pushes an update.

---

## Official & Custom GitHub OAuth App Configuration

Official Orbit builds ship with Orbit's built-in public GitHub OAuth App Client ID (`Ov23lit3VKcNBq6uNUfp`, RFC 8628 Device Flow enabled, zero client secret), so **Sign in with GitHub** works out of the box on CLI, Desktop, iOS, Android, and HarmonyOS.

If you fork or self-host a custom Orbit build and want your own OAuth App branding:

1. Open [GitHub Developer Settings → OAuth Apps](https://github.com/settings/developers) and click **New OAuth App**.
2. Check **Enable Device Flow** and save.
3. Copy the public **Client ID** (`Ov23li...`) and either:
   - Set the GitHub Actions Repository Variable `ORBIT_GITHUB_CLIENT_ID` (picked up automatically by CI and Release workflows), **or**
   - Pass `ORBIT_GITHUB_CLIENT_ID` at runtime / override it in `crates/orbit-client-core/src/cloud_sync.rs`.

Users can also sign in at any time using a `gist`-scoped GitHub Personal Access Token.

---

## Quick Start

### 1. Register each host (Linux / macOS server or workstation)
```bash
orbit login
# Optional: also wire all synced hosts into this daemon's cross-host mesh
orbit sync --mesh
```

### 2. Sync on Desktop / iOS / Android
Open **Settings → Cloud Sync**, sign in with the same GitHub account, and enter your **Sync Passphrase** (or generated **Recovery Key**) once. All paired hosts, pinned projects, manual order, and preferences appear automatically — no pairing codes required.
