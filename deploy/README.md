# Self-Hosted Orbit Relay Guide (Zero Shared-Server Cost)

Orbit follows a strict **Bring-Your-Own-Relay (BYOR)** model so open-source maintainers never pay for public relay bandwidth, and users keep 100% control over their routing latency.

## Step 1: 1-Command Relay Setup on Your Cloud VM / AWS Devbox (`< 6MB` RAM)

Install Orbit and start the background Relay locked exclusively to your host public keys (printed by `orbit doctor` on your workstation / Mac mini):

```bash
curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh

orbit relay up --port 7878 --allow-pubkey <YOUR_HOST_PUBKEY>
```

- **Persistent Background Service**: Automatically configures `systemd --user` (with `loginctl enable-linger` so it survives SSH logout), `launchd` on macOS, or detached PID supervisor.
- **Zero-Knowledge**: All traffic is encrypted end-to-end with `Noise_NKpsk0_25519_ChaChaPoly_SHA256` (`orbit-relay-e2ee/2`). The relay only forwards opaque ciphertext records.
- **Zero Abuse**: Any connection targeting a public key outside `--allow-pubkey` is rejected immediately with `HTTP 403 Forbidden` before upgrading WebSocket frames.
- **Add More Machines Anytime**: Run `orbit relay allow <NEW_HOST_PUBKEY>` on the relay server.

> **Firewall / AWS Security Group Note**: Ensure inbound TCP port `7878` (or whichever `--port` you choose) is open in your cloud provider's Security Group.

## Step 2: 1-Command Attach on Each Host Machine (Workstation / Mac mini)

```bash
orbit use-relay <YOUR_RELAY_SERVER_IP>:7878
```

`orbit use-relay` automatically:
1. Normalizes the WebSocket URL and runs a 4-second live preflight check against your Relay,
2. Persists `relay_url` to `~/.orbit/config.json` and starts/restarts the background `orbit-daemon` service (existing `orbit shim` agent sessions stay alive uninterrupted),
3. Prints the fresh `orbit://pair/...` Pairing Code containing both LAN direct IPs and the E2EE Relay route — ready to paste into **Orbit App** (macOS / iOS / Android)!

## Direct LAN / Tailscale Mode (No Relay Needed)

If your machine shares a LAN or Tailscale mesh with your Mac/phone:

```bash
orbit daemon up
orbit pair
```
