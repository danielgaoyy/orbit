---
title: Multi-Host Fleet & E2EE Relay
description: Connect Mac, Linux, and Cloud GPU daemons simultaneously with Happy-Eyeballs routing, self-hosted zero-knowledge Relay, Peer-Bridge multi-hop tunnels, and encrypted GitHub Gist sync.
---

Orbit treats **multi-host fleets as a first-class invariant**. Every physical workstation, Mac mini, Linux build server, or cloud GPU VM receives a unique `HostId` and static X25519 identity keypair. `orbit-client-core` maintains concurrent connection pools and independent monotonic `(HostId, CursorSeq)` cursors across every host simultaneously.

## Happy-Eyeballs smart routing

When any Orbit client connects to a paired host, `orbit-client-core` automatically races available transport routes and settles on the lowest-latency healthy link:

1. **Direct LAN / Loopback (`127.0.0.1:6868` or LAN IP)**: Sub-millisecond local or same-network WebSocket traffic.
2. **Tailscale / WireGuard / SSH Forward (`ssh+relay`)**: Direct encrypted private-network or SSH-tunneled links with enforced SSH host-key verification.
3. **Self-Hosted Zero-Knowledge Relay (BYOR)**: Automatic Off-LAN fallback when you leave the office or switch to cellular data on mobile.

## Self-hosted Zero-Knowledge Relay (`< 6 MB`)

Because `orbit relay` is bundled inside the same static `orbit` binary, deploying your own private relay takes one command on any minimal cloud VM or bastion host:

```bash
# Start the blind relay restricted strictly to your daemon public key(s)
orbit relay up --port 7878 --allow-pubkey <HOST_1_PUBKEY> --allow-pubkey <HOST_2_PUBKEY>
```

Attach each host in your fleet:

```bash
# Attach by hostname:port or auto-provision over an ~/.ssh/config alias
orbit use-relay relay.example.com:7878
```

### Why the Relay is truly zero-knowledge

- **End-to-end Noise encryption**: Client-to-daemon traffic traversing the relay is encrypted with `Noise_NKpsk0_25519_ChaChaPoly_SHA256` (domain-separated prologue `orbit-relay-e2ee/2`) with a 32-byte PSK derived from the host's `pairing_secret` and contributory X25519 DH validation.
- **Control channel Proof-of-Possession**: Daemons authenticate their control channel via 1-RTT `Noise_NK` PoP (`orbit-relay-pop/1`) and attach tunnels via HMAC-SHA256, preventing spoofed hijack registrations.
- **Optional `fleet_token` admission**: Gate WebSocket upgrades with constant-time Bearer token validation (`--fleet-token-stdin`, persisted `0600`, automatically redacted from URLs and logs).
- **Blind forwarding**: The relay routes only Zstd-compressed opaque ciphertext frames. It cannot read prompts, code diffs, terminal streams, or credentials.

## Peer-Bridge multi-hop E2EE tunneling

Some build servers or lab GPUs sit inside isolated corporate LANs with no direct inbound route and no outbound relay access. Orbit solves this via **Peer-Bridged Multi-Hop E2EE Tunneling**:

- Any reachable Orbit daemon exposes an `Origin`-validated, semaphore-bounded `/ws/e2ee` endpoint.
- When acting as a bridge (`PeerBridgeOpen` / `PeerBridgeData`), the intermediate host dials **only** targets explicitly allowlisted in its `connected_hosts` store (eliminating SSRF).
- The client performs a direct `Noise_NKpsk0` mutual handshake with the isolated target daemon; the intermediate bridge forwards strictly opaque encrypted bytes and never learns target `pairing_secret`s or plaintext.

## Zero-Knowledge Cloud Sync & Off-LAN mobile reconnect

Sync your fleet host directory, relay routes, project pins, and Personal Agent preferences across Desktop and Mobile without running a central SaaS database:

```bash
orbit login --mesh    # Sign in with GitHub Device Flow + set your local Sync Passphrase
orbit sync --mesh     # Pull/merge vault changes and wire connected_hosts across machines
```

- **Local client-side encryption**: Your vault (`orbit-cloud-vault/v1`) is encrypted locally using **Argon2id** key derivation + **AES-256-GCM** before upload to a private GitHub Secret Gist.
- **Fleet-wide relay propagation**: Updating a relay route via `WireMessage::ConfigureHostRelay` / `apply_relay_to_fleet` propagates the configuration across connected hosts and into the encrypted vault so Off-LAN mobile clients reconnect seamlessly without returning to your desk.
- **Adaptive heartbeats & auth recovery**: Clients maintain tuned 20s / 65s heartbeat windows with non-blocking network handoff (`on_network_changed`) and clean recovery on `401` secret rotation.
