# AGENTS.md — Orbit 开发与架构演进准则

本仓库是 **Orbit** 的主源码仓库。协作开发时请严格遵守以下工程规范。

---

## 1. 核心架构红线（不可违背）

1. **单二进制多角色（Single Static Binary）**：
   `orbit` 二进制（`crates/orbit-cli`）必须保持零外部运行时依赖（无需 Node.js / Python），内聚集成 `daemon`、`shim`、`relay`、`pair`、`doctor` 等全部子命令，确保在 Linux (`x86_64`/`aarch64`) 与 macOS (`Apple Silicon`) 上单文件即拷即用。
2. **进程热升级隔离（Shim Isolation）**：
   任何有状态的 AI CLI 会话（`agy`、`codex`、`claude`）与用户交互式 `PTY` 必须由独立的 `orbit shim` 子进程托管并通过 `$ORBIT_HOME/shims/*.sock` 通信，严禁让 `orbit daemon` 重启时连带杀死正在运行的 Agent。
3. **多主机一等公民（Multi-Host Fleet）**：
   协议层与 `orbit-client-core` 中一切状态、游标缓存和连接池均需支持多台主机（如 Devbox + 工作站 + 家里 Mac mini）同时在线并存，严禁在客户端硬编码单主机单例假设。
4. **私有零知识安全边界（Zero-Knowledge & Allowlist）**：
   `orbit-relay` 不得接触任何明文业务载荷，所有远程流量经由 `orbit-crypto`（`Noise_NK_25519_ChaChaPoly_SHA256`）端到端加密，并支持 `--allow-pubkey` 公钥白名单强拦截。
5. **拒绝 `unwrap()` 裸奔**：
   Workspace 已开启 `unsafe_code = "deny"` 与 `clippy::unwrap_used = "warn"`。生产路径统一使用 `thiserror` / `anyhow` 显式传递错误。

---

## 2. UI 与界面改动约定

- 凡涉及 `apps/` 下任何界面设计、布局、样式或交互的改动，动手前**必须**先读取并遵循 `.agents/skills/interface-design/SKILL.md`（若仓库中已生成 `.interface-design/system.md`，需一并读取）。

---

## 3. 常用研发与验证命令

```bash
# 代码格式检查与 Clippy 静态检查
cargo fmt --all -- --check
cargo clippy --workspace --all-targets

# 运行全量单元测试与集成测试
cargo test --workspace

# 本地自检（探测本机身份密钥与已安装的 agy / codex / claude）
cargo run -p orbit-cli -- doctor
```
