---
title: 五分钟上手
description: 安装单一静态 Orbit 二进制、启动本地守护进程、配对多端设备，并激活你的全局 Personal Agent。
---

Orbit 以**单一静态 Rust 二进制**（`orbit`）发布，零 Node.js 与 Python 运行时依赖。`daemon`、`shim`、`relay`、`pair`、`doctor` 及完整的多机编排 CLI 全部内聚在同一可执行文件中。

## 1. 安装 `orbit` 二进制

在 **macOS（Apple Silicon）** 与 **Linux（`x86_64` / `aarch64`）** 上执行：

```bash
curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh
```

安装脚本强制使用 HTTPS、限制 GitHub Release 下载域名，并在落盘前对二进制执行强制 `SHA256SUMS.txt` 校验。Fork 或企业内网镜像可通过 `ORBIT_GITHUB_REPO=owner/repo` 覆盖发布源。

:::tip[平滑迁移现有 CLI / IDE 配置]
运行 `orbit migrate` 即可预览从 Claude Code、Codex、Antigravity、Cursor 或 VS Code 导入 MCP 服务与项目规则的清单，确认后添加 `--apply` 执行写入。
:::

## 2. 检查宿主环境并启动守护进程

```bash
orbit doctor        # 探测 Ed25519/X25519 密钥、内核沙箱后端与已安装的 AI CLI
orbit daemon up     # 安装并启动持久化回环守护进程（127.0.0.1:6868）
orbit pair          # 输出 orbit://pair/... 配对链接与终端可扫二维码
```

执行 `orbit daemon up` 后，Orbit 会自动保障以下机制：

- **默认回环监听**：仅绑定 `127.0.0.1:6868`，使用持久化 `pairing_secret` 与严格 `Origin` 头校验防御跨站 WebSocket 劫持（CSWSH）。
- **零中断 Shim 隔离**：所有有状态编码代理与交互式 PTY 均运行在 `setsid` 脱离会话组的 `orbit shim` 中，通过 `$ORBIT_HOME/shims/*.sock` 通信；后续运行 `orbit upgrade` 热重启 Daemon 时不会杀死正在干活的 Agent。
- **Windows 服务支持**：`orbit daemon up` 注册当前用户的交互式计划任务；`orbit daemon up --service` 安装具备重启恢复能力的 LocalService SCM 服务。

## 3. 连接桌面端、Web WASM 或移动端

1. 启动 **Orbit Desktop**（macOS / Linux / Windows），客户端将自动发现本机 `127.0.0.1:6868` 守护进程。
2. 在 **iOS**、**Android**、**HarmonyOS** 或 **Web WASM PWA** 上扫描 `orbit pair` 输出的二维码（或粘贴 `orbit://pair/...` URI）。
3. 在你的 Linux 构建机、家用 Mac mini 或云端 GPU 虚拟机上重复步骤 1–2，即可将它们纳入同一多机舰队。

## 4. 开启非局域网远程访问（可选）

当设备在同一局域网、Tailscale 或 SSH 隧道内时，Orbit 自动优先直连。若需在公司/家庭 NAT 后随时远程连回，可部署 `< 6 MB` 自托管零知识中继：

```bash
# 在任意轻量云主机或跳板机上启动盲中继
orbit relay up --port 7878 --allow-pubkey <YOUR_HOST_PUBKEY>

# 在每台工作站 / Devbox / Mac Studio 上挂载中继
orbit use-relay <YOUR_RELAY_HOST>:7878
```

或者使用零服务器加密 GitHub Secret Gist 云同步，在所有设备间自动同步主机目录与 Mesh 拓扑：

```bash
orbit login --mesh
```

## 5. 激活全局 Personal Agent

无需在多台机器的十几个会话之间疲于切换：

1. 在 Orbit Desktop 中打开 **设置 → 应用 → Personal Agent**。
2. 在线舰队中指定任意一台常驻机器（如家用 Mac Studio 或云端 Devbox）担任全局唯一的 **Personal Router（分工总控）**。
3. 选择主脑引擎（`Claude Code`、`Codex`、`Antigravity` 或任意 ACP 代理），并可按需启用**零公网 Webhook** 的飞书、Slack Socket Mode 或 Discord Gateway v10 长连接。

从此你只需对着唯一的 Personal Agent 下达目标，它会跨主机并行派发任务、挂起条件定时器，仅在出现 **Tier-3 高危决策** 时推送卡片请你拍板。

## 下一步阅读

- [Personal Agent 与全局调度](../../guides/personal-agent/) — 深入了解跨主机上下文聚合、Worktree 协作与 IM 长连接。
- [多主机舰队与零知识中继](../../guides/multi-host-fleet/) — 掌握 Happy-Eyeballs 竞速路由、Peer-Bridge 与云同步。
- [安全边界、内核沙箱与审计链](../../guides/security-sandbox/) — 配置 OS 内核沙箱、出站代理白名单与防篡改审计。
