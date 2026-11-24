# GitHub PR & Issue Board

Install with `orbit plugin install examples/plugins/pr-dashboard --enable`. Run `orbit plugin invoke pr-dashboard list_open` to retrieve up to five recent open items from the public Orbit repository. GitHub's issues endpoint includes pull requests. This package uses the public API without credentials and receives the same rate limits as other anonymous clients.

The `side_panel_tab` slot initially renders a badge, an empty table and a Refresh button. Send a second render with `action_id` set to `refresh` to fetch and populate the table. `board.wat` reads top-level item titles and classifies items with a `pull_request` member. It preserves JSON string escaping. `guest.rs` defines the endpoint and UI literals. The sole network capability is `api.github.com`.

Rebuild `plugin.wasm` with `cargo run -p orbit-daemon --example build_plugins`. The offline contract test supplies two representative API objects to this same bytecode. Licensed under the repository's MIT or Apache-2.0 license.
