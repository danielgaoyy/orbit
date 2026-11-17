# Self-Hosted Orbit Relay Guide (Zero Shared-Server Cost)

Orbit follows a strict **Bring-Your-Own-Relay (BYOR)** model so open-source maintainers never pay for public relay bandwidth, and users keep 100% control over their routing latency.

## Option A: Single-Binary Private Relay on Your Own Cloud VM / AWS Devbox (`< 5MB` RAM)

Copy the single `orbit` binary to your cloud machine and lock it strictly to your own hosts' public keys (printed by `orbit doctor` on your workstation / Devbox / Mac mini):

```bash
orbit relay \
  --listen 0.0.0.0:7878 \
  --allow-pubkey <YOUR_WORKSTATION_PUBKEY> \
  --allow-pubkey <YOUR_DEVBOX_PUBKEY> \
  --allow-pubkey <YOUR_MAC_MINI_PUBKEY>
```

- **Zero-Knowledge**: All traffic is encrypted end-to-end with `Noise_NK_25519_ChaChaPoly_SHA256` (`orbit-relay-e2ee/1`). The relay only forwards opaque ciphertext records.
- **Zero Abuse**: Any connection targeting a public key outside `--allow-pubkey` is rejected immediately with `HTTP 403 Forbidden` before upgrading WebSocket frames.

## Option B: Direct LAN / Tailscale / Public Devbox (No Relay Needed)

If your machine has a Tailscale IP, LAN IP, or public Devbox port, you do not even need a relay:

```bash
orbit daemon --listen 0.0.0.0:6868
orbit pair --endpoint ws://<TAILSCALE_OR_PUBLIC_IP>:6868/ws
```
