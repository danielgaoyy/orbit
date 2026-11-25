---
title: CLI 命令、五端架构与可观测性
description: orbit 单二进制子命令速查、13 个 Rust Crate 与 5 端客户端仓库拓扑、Shim 热升级不变量，以及预算告警、自动压缩与 OpenMetrics / OTLP 可观测性。
---

## `orbit` CLI 核心子命令速查

以下全部命令均打包在单一静态可执行文件 `orbit`（`crates/orbit-cli`）中。

### 宿主、守护进程与多主机舰队

| 命令 | 说明 |
| --- | --- |
| `orbit doctor [--bundle PATH]` | 检查宿主密钥、内核沙箱状态、Relay 连接与已安装 AI CLI；`--bundle` 导出脱敏诊断包（守护进程停止时同样可用）。 |
| `orbit daemon up \| status \| restart \| down` | 管理本机常驻守护进程（默认 `127.0.0.1:6868`；Windows 支持 `--service` 安装 SCM 服务）。 |
| `orbit relay up \| allow \| status \| down` | 启动与管理 `< 6 MB` 自托管零知识中继（支持 `--allow-pubkey` 公钥白名单）。 |
| `orbit use-relay [HOST:PORT \| SSH_ALIAS]` | 挂载中继、基于 `~/.ssh/config` 自动部署零端口 SSH 中继，或 `--clear` 切回直连模式。 |
| `orbit pair [--rotate-secret]` | 输出多主机配对 `orbit://pair/...` URI 与终端二维码；`--rotate-secret` 轮换密钥并主动断开既有 E2EE 会话。 |
| `orbit login \| sync \| logout` | 零服务器加密 GitHub Secret Gist 云同步（`--mesh` 自动同步 `connected_hosts` 拓扑）。 |
| `orbit upgrade [--all] [--force]` | 经 SHA-256 校验的自升级与热重启（`--all` 优先滚动升级舰队内所有已连接远端主机）。 |

### 编排、工作树、定时与审计

| 命令 | 说明 |
| --- | --- |
| `orbit run -p <provider> [--worktree] "<prompt>"` | 创建并流式执行（或 `--no-wait` 后台启动）`claude`、`codex`、`antigravity`、`pi` 或 `--provider acp --acp-agent <id>` 会话。 |
| `orbit ls \| inspect \| logs \| send \| wait \| stop` | 查看列表、检查详情、拉取日志、发送续聊、阻塞等待结果或打断活跃轮次。 |
| `orbit fork <agent_id> [--worktree] [--at-seq N]` | 原地或向独立 Git Worktree 派生既有会话。 |
| `orbit workspace / script / terminal` | 管理 Mission Workspace、工作树、`orbit.json` 服务与热升级保活的 `portable-pty` 终端。 |
| `orbit schedule / heartbeat` | 管理支持 IANA 时区的 Cron 定时任务与周期性心跳巡检。 |
| `orbit audit verify \| export` | 校验 SHA-256 + Ed25519 宿主安全审计链或导出升序 JSONL。 |
| `orbit migrate [--from <ide>] [--apply]` | 预览或执行从 Claude Code、Codex、Antigravity、Cursor、VS Code 迁移 MCP 与规则。 |
| `orbit quotas` | 实时查看 Antigravity、Codex 与 Claude Code 的 5 小时 / 周用量配额。 |

## Rust 全栈拓扑（13 个 Crate + 5 套客户端）

```text
orbit/
├── crates/
│   ├── orbit-protocol/        # 线协议、状态机枚举与 Zstd 帧压缩
│   ├── orbit-crypto/          # Noise_NKpsk0 端到端加密与配对凭据
│   ├── orbit-store/           # SQLite WAL、FTS5 记忆与审计链
│   ├── orbit-shim/            # setsid 进程壳、PTY 与出站代理
│   ├── orbit-providers/       # 原生 CLI 与 34+ ACP 注册表驱动
│   ├── orbit-daemon/          # Router、IM 长连接、沙箱、LSP 与 Git
│   ├── orbit-relay/           # < 6 MB 零知识端到端加密盲中继
│   ├── orbit-relay-transport/ # Relay 与 ssh+relay 隧道传输层
│   ├── orbit-client-core/     # 五端共享 MVU 核心、Diff 与 VT100
│   ├── orbit-ffi-uniffi/      # UniFFI v0.32 绑定（iOS / Android）
│   ├── orbit-uniffi-bindgen/  # 独立轻量 UniFFI 绑定生成器
│   ├── orbit-ffi-ohos/        # ohos-rs N-API 绑定（HarmonyOS 7+）
│   └── orbit-cli/             # 多合一单静态二进制 CLI
└── apps/
    ├── desktop/               # Tauri v2、React 19、CodeMirror 6、xterm
    ├── web/ (WASM PWA)        # 浏览器与 PWA（Rust WASM 核心）
    ├── ios/                   # iOS 16.4+：Swift + SwiftUI + 灵动岛
    ├── android/               # Android：Kotlin 2.1 + Jetpack Compose
    ├── harmony/               # HarmonyOS 7+：Stage 模型 + ArkUI V2
    └── docs/                  # Astro Starlight 双语官网与文档中心
```

## 进程热升级隔离不变量（`orbit shim`）

`orbit daemon` 严禁将 `claude`、`codex`、`agy`、`pi`、ACP 代理或用户 PTY 作为直接子进程运行。所有有状态工作负载均托管在 `setsid` 独立会话组的 `orbit shim` 中，经 `$ORBIT_HOME/shims/<shim-id>.sock` 双向通信：

- 重启或热升级 `orbit daemon` 时，新进程依据持久化序列游标无中断重连所有活跃 Shim。
- 在全仓 `unsafe_code = "deny"` 约束下，Unix 进程组终止前强制校验 `pgid > 1` 并在负进程组 ID 前传入 `--`（`/bin/kill -s TERM -- -<pgid>`），杜绝现代 Linux `procps-ng` 参数误解析风险。

## 用量预算、自动压缩与可观测性

- **费率估算与 UTC 预算告警**：内置主流模型 API 估算表，并可在 `$ORBIT_HOME/model-pricing.json` 按模型名或 `provider/model` 覆盖企业协议价；在 `config.json` 设置 `daily_budget_usd` 与 `monthly_budget_usd` 即可接收 UTC 周期预算通知。
- **上下文自动压缩**：在 `config.json` 中设置 `auto_compact_threshold_pct`（`50`–`95`）可在上下文接近阈值时自动压缩，亦可通过 `CompactAgentContext` 对任意空闲会话按需触发。
- **OpenMetrics 指标抓取**：`GET /metrics` 严格限定回环来源 + 合法 `Origin` + 当前 `pairing_secret` Bearer Token，输出聚合运行计数。
- **OTLP 日志导出与结构化日志**：配置 `OTEL_EXPORTER_OTLP_ENDPOINT` 或 `config.json` 中的 `telemetry_endpoint` 即可向 `<base>/v1/logs` 批量发送静态元数据日志；设置 `ORBIT_LOG_FORMAT=json` 开启本机结构化 JSON 日志。
