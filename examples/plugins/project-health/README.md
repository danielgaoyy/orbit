# Project Health Inspector

Install with `orbit plugin install examples/plugins/project-health --enable`. In a registered Orbit workspace run `orbit plugin invoke project-health check_health --cwd "$PWD"`. The operation returns one `nonempty` flag for each of `Cargo.toml`, `package.json` and `README.md`. The `stage_card` slot presents those same checks. Missing and empty files are ordinary results. Permission and size failures surface as call errors.

`health.wat` performs the three capability-gated reads. `guest.rs` builds the three labels and eight possible reports into the module. Rebuild the checked-in `plugin.wasm` from the repository root with `cargo run -p orbit-daemon --example build_plugins` on a native Rust toolchain. Licensed under the repository's MIT or Apache-2.0 license.
