## Summary

<!-- Briefly describe what this PR changes and why. Link related issues (e.g. Fixes #123). -->

## Areas Affected

- [ ] Rust workspace (`crates/*`)
- [ ] Desktop frontend (`apps/desktop/src`)
- [ ] Desktop Tauri Rust backend (`apps/desktop/src-tauri`)
- [ ] Mobile client (`apps/ios`, `apps/android`, or `apps/harmony`)
- [ ] Deploy scripts / CI / Documentation

## Architecture & Safety Checklist

- [ ] Respects `AGENTS.md` redlines: single static binary, shim process isolation, multi-host fleet compatibility, and zero-knowledge E2EE boundaries.
- [ ] No `unsafe` code (`unsafe_code = "deny"`) and no new `.unwrap()` calls in production Rust paths.
- [ ] No secrets or credentials passed via process `argv` or logged in plaintext.
- [ ] If Desktop UI copy changed, kept `en`, `zh-CN`, and `ja` i18n catalogs in sync.

## Verification

<!-- Check all verifications run locally and summarize any platform-specific notes. -->

- [ ] `cargo fmt --all -- --check` and `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (or targeted crate tests for scoped changes)
- [ ] Desktop: `node --experimental-strip-types --test "src/**/*.test.ts"`, `npm run build`, `npm run test:smoke`
- [ ] Tauri Rust: `cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings` & `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`
- [ ] Installer scripts: `bash -n deploy/install.sh deploy/install-desktop-linux.sh && sh -n deploy/install.sh deploy/install-desktop-linux.sh`
