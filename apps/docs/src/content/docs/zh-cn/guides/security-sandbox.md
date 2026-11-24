---
title: 安全边界、内核沙箱与审计链
description: 覆盖 Noise_NKpsk0 端到端加密、macOS Seatbelt / Linux bubblewrap 内核沙箱、SSRF 防护出站代理、细粒度权限规则与 Ed25519 防篡改审计链的纵深防御体系。
---

Orbit 在开发者的真实工作站与服务器上编排编码代理、Shell 终端与 Git 写入操作。安全边界被固化为**编译期与协议层不变量**，而非事后补丁。

## 密码学与进程安全红线

- **全仓强制 `unsafe_code = "deny"`**：Rust Workspace 全局禁用 `unsafe` 代码，生产路径显式传递错误。
- **默认回环监听 + 反 CSWSH**：`orbit daemon` 默认绑定 `127.0.0.1:6868`，强制校验持久化 `pairing_secret` 与严格 `Origin` 头，阻断本地浏览器跨站 WebSocket 劫持。
- **零 `argv` 凭据泄露**：配对密钥、MCP Token 与 `fleet_token` 严禁通过进程命令行参数传递，统一经 stdin 管道或 `0600` 权限文件注入。
- **密钥轮换主动断连**：执行 `orbit pair --rotate-secret` 会立即轮换 `pairing_secret` 并主动踢掉既有 E2EE 会话。
- **供应链与环境变量防护**：`deploy/install.sh`、`orbit upgrade` 与桌面更新强制 HTTPS、GitHub 域名白名单、防降级与 `SHA256SUMS.txt` 校验；自定义 ACP 环境变量自动拦截 `LD_PRELOAD`、`DYLD_*`、`NODE_OPTIONS`、`ORBIT_*` 等注入变量。

## OS 内核级工具沙箱（`sandbox_policy`）

在 `$ORBIT_HOME/config.json` 中配置 `sandbox_policy`（或通过已认证的 `GetSandboxConfig` / `UpdateSandboxConfig` RPC 下发），即可对新建的 Agent 进程、PTY、ACP 终端、Lifecycle Hook、Goal 命令及本地 MCP stdio 服务施加隔离：

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

### 隔离模式与操作系统后端

- **三种模式**：`off`（保持传统执行）、`workspace_write`（可读工作区、仅允许写入工作区与独立临时目录）与 `strict_read_only`（严格只读）。
- **强制凭据保护**：启用任意沙箱模式后，默认屏蔽对 SSH（`~/.ssh`）、AWS（`~/.aws`）、GnuPG（`~/.gnupg`）、Kubernetes（`~/.kube`）及 Orbit 宿主密钥（`$ORBIT_HOME`）的读取。
- **平台实现**：
  - **macOS**：原生 Seatbelt（`sandbox-exec`）内核策略。
  - **Linux**：基于 `bubblewrap`（`bwrap`）的 User Namespace 与 Network Namespace 隔离。
  - **Windows**：剥离凭据环境变量并绑定工作区私有临时目录（协作式文件系统与网络约束）。
- 在任意主机上运行 `orbit doctor` 即可查看当前生效的沙箱后端与内核强制状态。

## 防 SSRF 会话级出站代理（Egress Proxy）

沙箱默认关闭外网访问；开启 `network_enabled: true` 后：

- 所有出站 HTTP 与 `CONNECT` 请求必须认证至由对应 `orbit shim` 托管的**会话私有代理**（随子进程同生共死）。
- **`denied_domains` 永远优先于 `allowed_domains`**（支持精确域名与 `*.example.com` 通配符）。
- **DNS 解析结果钉选防 SSRF**：代理由自身解析 DNS，拦截所有私网、回环、链路本地及非公网 IP，并直接拨号通过校验的目标 IP，彻底根除 DNS Rebinding 攻击。
- 在 Linux 隔离网络命名空间内，回环代理监听经私有 Unix Domain Socket 转发。

## 细粒度权限规则、Guardian 与生命周期钩子

- **持久化权限规则**：按项目路径、Provider、工具类型与命令前缀配置细粒度 `allow` / `deny` 规则（`list_permission_rules` / `upsert_permission_rule`），并在 DecisionDock 支持安全批量审批。
- **三级 Guardian**：Tier-1 低危自动放行、Tier-2 Personal Agent / LLM 预审、Tier-3 协议硬阻断人工决策。
- **Lifecycle Hooks**：支持 `PreToolUse`、`PostToolUse` 与 `Notification` 钩子，并实施显式项目级信任开关（`set_project_hooks_trust`）。

## Ed25519 防篡改安全审计链

宿主的所有安全事件（策略变更、沙箱拦截、权限审批、E2EE 密钥轮换）均写入 SQLite 的 `security_audit_chain` 表：

- 每条记录通过 **SHA-256 哈希链**串联前一条摘要，并使用宿主身份派生的域隔离密钥进行 **Ed25519 签名**。
- 随时校验序列连续性、载荷完整性、签名与高水位标记，或导出留存：

```bash
# 校验本机安全审计链与 Ed25519 签名完整性
orbit audit verify

# 导出升序 JSONL 副本用于独立归档或接入 SIEM
orbit audit export --output security-events.jsonl
```

独立留存的导出副本即使在遭遇整库回滚攻击时也能提供可验证的密码学证据。
