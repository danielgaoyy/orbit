# Self-Hosted Orbit Relay Guide (Zero Shared-Server Cost)

Orbit follows a strict **Bring-Your-Own-Relay (BYOR)** model so open-source maintainers never pay for public relay bandwidth, and users keep 100% control over their routing latency.

## Step 1: 1-Command Relay Setup on Your Cloud VM / Intranet Host (`< 6MB` RAM)

### Option A — Zero-Inbound-Port Cloudflare Tunnel Relay (Recommended for Firewalled / Intranet Hosts)

When your server blocks all inbound public TCP ports (`22` / `7878` reachable only on internal VPC/Tailscale), expose `127.0.0.1:7878` over an outbound-only Cloudflare Tunnel:

```bash
curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh

# Instant Quick Tunnel (`*.trycloudflare.com` with persistent shared fleet_token)
orbit relay tunnel --port 7878 --allow-pubkey <YOUR_HOST_PUBKEY>

# Or permanent custom hostname via Cloudflare Zero Trust Named Tunnel (`0600` EnvironmentFile)
orbit relay tunnel --port 7878 --hostname relay.yourdomain.com --tunnel-token-stdin
```

- **Auto Hairpin Loopback Fallback**: If a daemon running on the relay host cannot dial the external tunnel/SSH address due to firewall/intranet restrictions, it automatically falls back to `127.0.0.1:<port>` (when authorized by matching `fleet_token` or `--allow-pubkey`) so remote clients still traverse the tunnel seamlessly.

### Option B — Public TCP Port Relay (`orbit relay up`)

Install Orbit and start the background Relay locked exclusively to your host public keys (printed by `orbit doctor` on your workstation / Mac mini):

```bash
curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh

orbit relay up --port 7878 --allow-pubkey <YOUR_HOST_PUBKEY>
```

- **Persistent Background Service**: Automatically configures `systemd --user` (with `loginctl enable-linger` so it survives SSH logout), `launchd` on macOS, or detached PID supervisor.
- **Zero-Knowledge**: All traffic is encrypted end-to-end with `Noise_NKpsk0_25519_ChaChaPoly_SHA256` (`orbit-relay-e2ee/2`). The relay only forwards opaque ciphertext records.
- **Zero Abuse**: Any connection targeting a public key outside `--allow-pubkey` (or lacking `?ft=`) is rejected immediately with `HTTP 403 Forbidden` before upgrading WebSocket frames.
- **Add More Machines Anytime**: Run `orbit relay allow <NEW_HOST_PUBKEY>` on the relay server.

> **Firewall / AWS Security Group Note**: Option B requires inbound TCP port `7878` (or whichever `--port` you choose) open in your cloud firewall. Use Option A (`orbit relay tunnel`) if inbound public ports are disabled.

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
