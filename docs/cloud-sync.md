# Orbit Zero-Server E2EE Cloud Sync (GitHub Secret Gist)

Orbit provides multi-device login and automatic configuration sync **without any Orbit-hosted backend server, user database, or infrastructure cost** using the **BYOS (Bring Your Own Storage)** model:

- **Identity & Auth**: GitHub OAuth 2.0 Device Authorization Grant (RFC 8628, public `client_id`, no `client_secret`) or a GitHub Personal Access Token with only the **`gist`** scope.
- **Storage**: A single user-owned **GitHub Secret Gist** (`orbit-cloud-vault.v1.enc.json`). Orbit never requests repository permissions (`repo` scope is never requested).
- **Zero-Knowledge Encryption**: All paired hosts, pinned projects, project order, and user preferences are Zstd-compressed and encrypted client-side with **Argon2id (19 MiB, t=2, p=1) + AES-256-GCM** (`orbit-cloud-vault/v1`) before upload. GitHub only ever stores opaque ciphertext.
- **Conflict Resolution**: Deterministic per-entity Last-Write-Wins (LWW) with soft-delete tombstones across hosts, projects, manual order, and preferences. Machine-local loopback endpoints (`127.0.0.1` / `localhost` / `[::1]`) are automatically stripped before upload.

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
