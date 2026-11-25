---
title: Security, Sandbox & Audit Chain
description: Defense-in-depth engineering security across Noise_NKpsk0 E2EE, macOS Seatbelt and Linux bubblewrap kernel sandboxes, SSRF-pinned egress proxy, permission rules, and Ed25519 signed audit chains.
---

Orbit executes AI coding agents, shell PTYs, and Git mutations across developer machines. Security boundaries are enforced as **compile-time and protocol-level invariants**, not optional afterthoughts.

## Cryptographic & process invariants

- **Workspace-wide `unsafe_code = "deny"`**: Every Rust crate in the workspace forbids `unsafe` code and handles errors explicitly.
- **Loopback-by-default daemon**: `orbit daemon` binds to `127.0.0.1:6868` by default, authenticating WebSocket upgrades with a persisted per-host `pairing_secret` and strict `Origin` header validation to prevent Cross-Site WebSocket Hijacking (CSWSH).
- **Zero `argv` secret leakage**: Pairing secrets, MCP bearer tokens, and fleet tokens are never passed via process command-line arguments; they are delivered over stdin/pipes or stored in private `0600` files under `$ORBIT_HOME`.
- **Active teardown on secret rotation**: Running `orbit pair --rotate-secret` rotates the persisted `pairing_secret` and actively disconnects existing E2EE sessions.
- **Verified supply chain**: `deploy/install.sh`, `orbit upgrade`, and desktop updates enforce HTTPS, GitHub host allowlists, downgrade rejection, and mandatory SHA-256 verification against `SHA256SUMS.txt`.
- **Dynamic linker protection**: Custom ACP agent environment overrides strip dangerous runtime-injection variables (`LD_PRELOAD`, `DYLD_*`, `NODE_OPTIONS`, `ORBIT_*`).

## OS Kernel Tool Sandbox (`sandbox_policy`)

Configure `sandbox_policy` in `$ORBIT_HOME/config.json` (or via authenticated `GetSandboxConfig` / `UpdateSandboxConfig` RPCs) to confine new agent processes, PTYs, ACP terminals, command hooks, goal commands, and local MCP stdio servers:

```json
{
  "sandbox_policy": {
    "mode": "workspace_write",
    "network_enabled": true,
    "allowed_domains": ["crates.io", "*.github.com", "api.anthropic.com"],
    "denied_domains": ["metadata.google.internal"]
  }
}
```

### Supported isolation modes & OS backends

- **Modes**: `off` (legacy unconstrained execution), `workspace_write` (read workspace + write strictly inside workspace & isolated temp dirs), and `strict_read_only`.
- **Mandatory credential protection**: Every enabled policy automatically blocks read access to SSH keys (`~/.ssh`), AWS credentials (`~/.aws`), GnuPG (`~/.gnupg`), Kubernetes configs (`~/.kube`), and Orbit host secrets (`$ORBIT_HOME`).
- **Platform enforcement**:
  - **macOS**: Native Seatbelt (`sandbox-exec`) kernel profiles.
  - **Linux**: `bubblewrap` (`bwrap`) user and network namespaces with isolated loopback forwarding.
  - **Windows**: Strips credential environment variables and binds workspace-local temp directories (cooperative filesystem/network policy).
- Run `orbit doctor` on any host to inspect the active sandbox backend and kernel enforcement status.

## SSRF-safe session Egress Proxy

When sandboxed network access is disabled (the default), agents cannot dial outbound sockets. When enabled:

- All outbound HTTP and `CONNECT` traffic authenticates to a **session-local egress proxy** owned by the detached `orbit shim` for the child process lifetime.
- **Deny rules always take precedence** over `allowed_domains` (supporting exact hosts and `*.example.com` wildcards).
- **DNS answer pinning against SSRF**: The proxy resolves DNS itself, rejects any private, loopback, link-local, or non-public IP address, and connects directly to the validated IP — eliminating DNS rebinding attacks.
- On Linux inside isolated network namespaces, the loopback listener is bridged strictly through a private Unix domain socket.

## Permission Rules, Guardian & Lifecycle Hooks

Combine deterministic policies with contextual pre-review:

- **Persistent Permission Rules**: Scope `allow` / `deny` rules by workspace, provider, tool kind, and command prefix (`list_permission_rules` / `upsert_permission_rule`) plus safe batch approvals in DecisionDock.
- **3-Tier Guardian**: Tier-1 low-risk auto-allow, Tier-2 Personal Agent / LLM classifier pre-review, and protocol-hard-blocked Tier-3 human-only approvals.
- **Lifecycle Hooks**: Wire `PreToolUse`, `PostToolUse`, and `Notification` hooks with explicit per-project trust gating (`set_project_hooks_trust`).

## Tamper-evident Ed25519 Audit Chain

Every host security event (sandbox violations, permission decisions, E2EE rekeys, policy updates) is recorded in SQLite's `security_audit_chain`:

- Each entry links cryptographically to the previous entry's **SHA-256 digest** and is signed with **Ed25519** using a domain-separated key derived from the host identity.
- Verify chain continuity, payloads, signatures, and the sequence high-water mark at any time:

```bash
# Verify local host security history integrity
orbit audit verify

# Export ascending JSONL pages for independent archival / SIEM retention
orbit audit export --output security-events.jsonl
```

Retaining periodic exports externally provides cryptographic proof even against complete database rollback attempts.
