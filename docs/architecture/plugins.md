# Orbit plugin SDK

An Orbit daemon owns its plugin catalog. Select the host with the CLI's existing `--server` option. Install a package from a directory on that host, or use a public HTTPS GitHub repository with the manifest at its root. An optional `#<40-hex-commit>` pins the Git revision. Otherwise Orbit resolves the default branch once and reads both files at that commit. The installer copies the manifest and module into a staging directory, then renames it into `$ORBIT_HOME/plugins/<id>/`. Installation defaults to disabled. Enablement persists across daemon restarts.

```sh
orbit plugin install ./examples/plugins/project-health --enable
orbit plugin list
orbit plugin invoke project-health check_health --cwd "$PWD" --input '{}'
orbit plugin disable project-health
orbit plugin enable project-health
orbit plugin remove project-health
```

## Manifest and entry points

The package root contains `orbit-plugin.json`. The portable Rust contract lives in `orbit_protocol::plugins`. IDs use up to 64 lowercase ASCII letters, digits, underscores and hyphens and start with a letter or digit. `wasm_entry` is a relative path to a binary module. Capabilities, named MCP operations and titled UI slots default to empty. The `hook_events` field reserves event names for future delivery. Runtime entry points in this version are explicit operation calls and UI renders.

The daemon advertises one Core MCP tool named `orbit_plugin` when any enabled plugin declares an operation and caller policy permits it. Use `{"action":"list"}` to discover names and descriptions, then `{"action":"call","plugin":"project-health","op":"check_health","input":{}}`. Operation names and per-plugin schemas stay out of `tools/list`. File access through MCP uses the caller agent's authorized workspace. A client without a workspace can call file-independent operations.

All plugin wire requests carry a UUID `request_id`. List, install, invoke and render return `plugins_list`, `plugin_installed`, `plugin_op_result` and `plugin_ui_rendered`, respectively. Enable, disable and uninstall return `ack`. Failures return a correlated `rpc_error`. Wire requests use `operation` for the guest operation name because `op` identifies the wire message. The CLI and the wire render request accept a registered host workspace through `cwd`. Omitting it grants no filesystem access.

## orbit_abi_v1

Each call receives a fresh wasm32 instance. Export linear `memory`, `orbit_alloc(i32) -> i32` and `orbit_invoke(i32, i32) -> i64`. Orbit allocates one request buffer, writes UTF-8 JSON, and passes its pointer and byte length to the guest. Return a packed pointer in the high 32 bits and byte length in the low 32 bits. The result must be valid UTF-8 JSON of at most 256 KiB. A trap becomes a call error. The entire instance is discarded afterward.

The first member of the compact request envelope is `kind`. The two forms are

```json
{"kind":"call","op":"check_health","input":{}}
{"kind":"render","slot":"stage_card","action_id":null,"payload":null}
```

| Import from `orbit` | Contract |
| --- | --- |
| `host_read_file(i32, i32) -> i64` | Read a literal relative path matched by a declared glob. The result contains raw file bytes. A missing authorized file returns -1. A denied path, symlink, special file or oversized file traps. |
| `host_http_get(i32, i32) -> i64` | Fetch raw bytes from exactly a declared domain over HTTPS port 443. The host checks every resolved address, pins DNS, disables proxies and redirects, enforces a five-second deadline and caps successful responses at 256 KiB. |
| `host_log(i32, i32, i32)` | Emit a structured diagnostic with the plugin ID, guest level and up to 4 KiB of UTF-8 text. |

Guests that import either read function export an i32 global `orbit_host_buffer` pointing to a reserved 256 KiB region. A successful read overwrites this region and returns the same packed pointer/length convention. Consume or copy the slice before the next host read. The host never calls back into guest code to allocate its response. No WASI, native imports or start functions are linked.

The default budget is 5 million fuel units and 8 MiB of linear memory, with hard maxima of 50 million and 32 MiB. Per-call limits include one memory, one table of up to 4096 elements, 64 host calls and six seconds elapsed. Four shared worker permits bound concurrent executions and installs. A timed-out worker retains its permit until it exits. Module binaries are capped at 1 MiB and the registry accepts up to 32 packages.

## Portable UI

Render requests identify a declared `side_panel_tab`, `stage_card` or `settings_panel`. The guest returns `{"root":{...}}` using the tagged nodes in `OrbitPluginUiNode`. The daemon rejects trees deeper than eight levels, trees with over 256 nodes, unknown fields or variants, invalid IDs, oversized text, and invalid table dimensions. A table has up to 16 columns and 128 equal-width rows. Clients render all strings as text. A Markdown renderer must disable HTML, scripts, navigation and remote resource loading. Buttons send another render to the same host, plugin and slot with the button's `action_id` and JSON payload. Form inputs hold text under their `field_id`. No executable client assets are part of this protocol.

See the packages in [examples/plugins](../../examples/plugins/) for manifests, host-native Rust generators, WAT and installable bytecode. Rebuild them with `cargo run -p orbit-daemon --example build_plugins`.
