# Security Policy

Orbit orchestrates local and remote AI coding agents, shell PTYs, and Git worktrees across developer machines. Security boundaries are treated as core architecture invariants rather than optional add-ons.

## Supported Versions

Security fixes are released for the current minor release line:

| Version | Supported |
| ------- | --------- |
| `0.9.x` | Yes       |
| `< 0.9` | No        |

## Security Architecture Summary

Orbit is designed around defense-in-depth and zero-knowledge infrastructure:

- **Loopback-by-Default Daemon & Anti-CSWSH Protection**: `orbit daemon` binds to `127.0.0.1:6868` by default. WebSocket connections require a persisted per-host `pairing_secret` and enforce strict `Origin` header validation to block cross-site WebSocket hijacking (CSWSH) from local browsers.
- **Zero-Knowledge Relay & Mutual E2EE (`Noise_NKpsk0`)**: Remote client-to-daemon traffic traversing `orbit relay` is encrypted end-to-end with `Noise_NKpsk0_25519_ChaChaPoly_SHA256` (domain-separated prologue `orbit-relay-e2ee/2`) using a 32-byte pre-shared key derived from the host's `pairing_secret` and contributory X25519 DH validation (rejecting low-order Curve25519 points). Rotating a host's `pairing_secret` actively disconnects existing E2EE sessions.
- **Relay Admission & Control Channel Hardening**: `orbit relay` enforces `--allow-pubkey` allowlisting before WebSocket upgrades, authenticates the daemon control channel via 1-RTT `Noise_NK` Proof-of-Possession (`orbit-relay-pop/1`), and authenticates `/v1/daemon/tunnel` attachment via HMAC-SHA256. The relay forwards only opaque ciphertext and cannot inspect prompts, diffs, or terminal streams.
- **Zero-Knowledge Cloud Sync Vault**: Optional GitHub Gist synchronization encrypts client state locally with `Argon2id` + `AES-256-GCM` (`orbit-cloud-vault/v1`) before upload.
- **Supply-Chain & Runtime Controls**:
  - CLI self-upgrades, desktop one-click updates, and `deploy/install*.sh` scripts enforce HTTPS and mandatory SHA-256 verification against `SHA256SUMS.txt`, downgrade rejection, and strict GitHub download-host allowlists.
  - SSH relay provisioning and tunneling enforce SSH host-key verification.
  - Secrets are never passed via process `argv`; sensitive material is passed over stdin/pipes or stored in private `0600`/`0700` files.
  - Custom ACP agent environment overrides block dynamic-linker and runtime-injection variables (`LD_PRELOAD`, `DYLD_*`, `NODE_OPTIONS`, `ORBIT_*`, etc.).
  - Rust crates enforce `unsafe_code = "deny"` workspace-wide.

## Reporting a Vulnerability

**Please do not report security vulnerabilities in public GitHub issues or pull requests.**

Report vulnerabilities privately through GitHub's **Private Vulnerability Reporting** flow:

1. Open the repository's **Security** tab.
2. Select **Advisories** -> **Report a vulnerability**.

If private vulnerability reporting is unavailable in your environment or fork, contact the repository maintainers directly via their GitHub profile contact before disclosing details publicly.

### What to Include

To help us reproduce and fix the issue quickly, please include:

- Affected component(s) (e.g., `orbit-crypto`, `orbit-daemon`, `orbit-relay`, Desktop, iOS, Android, HarmonyOS, installer scripts) and exact version (`orbit --version` or app version).
- Clear reproduction steps, minimal proof-of-concept, or protocol transcript (with personal keys/tokens redacted).
- Threat model and security impact (e.g., auth bypass, cross-origin access, E2EE downgrade, privilege escalation, RCE).
- Suggested remediation or patch, if available.

## Response & Disclosure Process

- **Acknowledgement**: Within **72 hours** of report receipt.
- **Triage & Severity Assessment**: Within **7 days**, including confirmation of scope and target remediation window.
- **Coordinated Disclosure**: We aim to ship fixes and publish an advisory within **30–90 days** depending on severity and complexity. Reporters will be credited in the advisory unless they prefer to remain anonymous.

## Out of Scope

- Vulnerabilities inside third-party AI provider CLIs themselves (please report those directly to the upstream provider).
- Attacks that require pre-existing arbitrary code execution as the same OS user account running `orbit daemon`.
- Denial-of-service against a self-hosted `orbit relay` caused by network-level flooding without a protocol bypass.
