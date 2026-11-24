---
title: 多主机舰队与零知识中继
description: 通过 Happy-Eyeballs 智能路由、自托管零知识 Relay、Peer-Bridge 多跳隧道与加密 GitHub Gist 云同步，将 Mac、Linux 与云端 GPU 融为统一舰队。
---

Orbit 从第一行协议设计起即坚持**多主机一等公民（Multi-Host Fleet First）**。每台物理工作站、Mac mini、Linux 构建机或云端 GPU 实例都拥有独立的 `HostId` 与静态 X25519 公钥；`orbit-client-core` 并发维护多机连接池与相互独立的 `(HostId, CursorSeq)` 单调序列游标。

## Happy-Eyeballs 智能竞速路由

当客户端连接任意已配对主机时，`orbit-client-core` 自动在可用链路间竞速并无缝切换至最低延迟通道：

1. **本地回环 / 局域网直连（`127.0.0.1:6868` 或 LAN IP）**：亚毫秒级直连 WebSocket。
2. **Tailscale / WireGuard / SSH 隧道（`ssh+relay`）**：加密私网直连，SSH 通道强制执行 Host Key 指纹校验。
3. **自托管零知识中继（BYOR）**：当你离开办公室或移动端切到 5G 蜂窝网时，自动无感切至 E2EE Relay。

## 自托管 `< 6 MB` 零知识中继（BYOR）

`orbit relay` 与守护进程编译在同一个静态二进制中。在任意最小规格云主机或跳板机上执行一条命令即可上线你的私有中继：

```bash
# 仅允许你自己的 Daemon 公钥接入
orbit relay up --port 7878 --allow-pubkey <HOST_1_PUBKEY> --allow-pubkey <HOST_2_PUBKEY>
```

在每台工作站或 Devbox 上挂载：

```bash
# 支持直接填地址，或传 ~/.ssh/config 别名一键通过 SSH 部署零端口中继
orbit use-relay relay.example.com:7878
```

### 零知识密码学保障

- **端到端 Noise 双向加密**：穿透中继的客户端到守护进程流量采用 `Noise_NKpsk0_25519_ChaChaPoly_SHA256`（域隔离序言 `orbit-relay-e2ee/2`），32 字节 PSK 派生自宿主的 `pairing_secret`，并严格校验 X25519 贡献性（拒绝低阶曲线点）。
- **控制信道防顶号（PoP）**：守护进程通过 1-RTT `Noise_NK` Proof-of-Possession（`orbit-relay-pop/1`）鉴权控制通道，并以 HMAC-SHA256 挂载数据隧道，杜绝伪造注册。
- **可选 `fleet_token` 准入**：在 WebSocket 升级前执行恒定时间 Bearer Token 校验（`--fleet-token-stdin`，以 `0600` 持久化并在日志与 UI 中自动脱敏）。
- **盲转发**：中继仅透传经过 Zstd 压缩的不透明密文帧，永远无法解密你的提示词、代码 Diff、终端流或凭据。

## Peer-Bridge 多跳 E2EE 隧道

对于身处企业隔离内网、既无法直连也无法直出 Relay 的构建机或实验室 GPU，Orbit 提供 **Peer-Bridge 多跳穿透**：

- 可达的跳板机 Daemon 暴露受 `Origin` 校验与并发信号量限流的 `/ws/e2ee` 端点。
- 作为中继桥（`PeerBridgeOpen` / `PeerBridgeData`）时，中间节点**仅**拨号其 `connected_hosts` 表中明确登记的白名单目标，彻底阻断 SSRF。
- 客户端直接与最内层的目标 Daemon 完成 `Noise_NKpsk0` 双向握手；中间跳板机只转发密文字节流，不持有目标主机的 `pairing_secret` 或任何明文。

## 零知识 GitHub Gist 云同步与移动端非局域网重连

无需搭建中心数据库，即可在桌面端与多台移动端之间同步主机目录、中继路由与 Personal Agent 偏好：

```bash
orbit login --mesh    # GitHub Device Flow 登录 + 设置本地 Sync Passphrase
orbit sync --mesh     # 拉取合并加密 Vault，并自动打通本机 connected_hosts Mesh
```

- **纯本地端到端加密**：客户端在上传私有 GitHub Secret Gist 之前，先使用 **Argon2id + AES-256-GCM**（`orbit-cloud-vault/v1`）加密完整 Vault。
- **全舰队中继配置传播**：通过 `WireMessage::ConfigureHostRelay` 与 `apply_relay_to_fleet` 同步各主机的中继地址并写入 Vault，让脱离局域网的手机端无需回到电脑旁即可恢复全舰队连接。
- **自适应心跳与鉴权自愈**：20s / 65s 心跳探测窗口配合非阻塞切网钩子（`on_network_changed`），在 Wi-Fi/5G 切换及 `401` 密钥轮换时平滑恢复。
