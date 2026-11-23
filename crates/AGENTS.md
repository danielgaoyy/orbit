# `crates/` 局部工程规范（Rust Workspace）

本文件管辖 `crates/*` 下全部 13 个 Rust crate 的共有工程要求。

1. **编码与安全红线**：
   - Workspace 已强制 `unsafe_code = "deny"` 与 `clippy::unwrap_used = "warn"`；生产代码严禁新增 `.unwrap()` / `.expect()` 裸崩或未校验敏感外泄，错误统一由 `thiserror` / `anyhow` 显式向上传递。
2. **按职责先读权威文档**：
   - **Daemon / Shim / Provider / 消息对齐 / 分页 / Relay**：先读 [`docs/architecture/daemon-shim-providers.md`](../docs/architecture/daemon-shim-providers.md)。
   - **Mission Workspace / `BOARD.md` / Worktree / 定时器 / Handle 别名**：先读 [`docs/architecture/mission-workspace-collab.md`](../docs/architecture/mission-workspace-collab.md)（特别注意改 `handle` 的所有写入口都必须原子保留 `orbit.handle_aliases`）。
   - **跨端 FFI / IPC / Client Core**：先读 [`docs/CLIENT_BRIDGE_ARCHITECTURE.md`](../docs/CLIENT_BRIDGE_ARCHITECTURE.md) 与 [`docs/cloud-sync.md`](../docs/cloud-sync.md)。
   - **密码学 / 认证 / 自更新 / Relay 准入**：先读 [`SECURITY.md`](../SECURITY.md)。
3. **协议与持久化向后兼容**：新增 `WireMessage` 枚举变体、字段或 SQLite 列需提供安全默认值 / 幂等迁移，确保新旧版本 Daemon 与多端客户端可渐进升级。
4. **本地验证链**（参考 [`docs/engineering/testing-and-ci.md`](../docs/engineering/testing-and-ci.md)）：
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   TMPDIR=/tmp cargo test -p orbit-cli --test e2e_pitfall_regressions -- --test-threads=4
   TMPDIR=/tmp cargo test --workspace -- --test-threads=4
   ```
