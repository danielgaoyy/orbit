---
title: Cross-Provider Memory & WASM Plugins
description: Share Git-friendly Markdown + FTS5 + RRF project memory across Claude Code, Codex, Antigravity, and ACP agents, and extend Orbit safely with sandboxed wasmi WASM plugins.
---

AI coding knowledge should belong to your **project and team**, not stay trapped inside one vendor's opaque cloud cache. Orbit combines transparent Markdown-first project memory with a capability-scoped WebAssembly plugin runtime.

## Cross-Provider Project Memory

Orbit scopes project memory per canonical workspace directory on each host, storing every record as a human-editable Markdown file:

```text
<project-root>/
└── .orbit/
    └── memory/
        ├── noise-rekey-invariant.md
        └── worktree-handoff-rules.md
```

Each file contains structured YAML frontmatter (`id`, `category`, `status`, `confidence`, `bound_paths`, `tags`, `source`, timestamps) and plain Markdown prose you can inspect, edit, or commit alongside your code.

### Hybrid offline retrieval (FTS5 + RRF)

- **Zero cloud requirement**: Orbit indexes memories locally in SQLite combining **FTS5 BM25 lexical ranking** and a built-in **256-dimension feature-hash cosine similarity** (covering English words, code identifiers, and CJK character n-grams), fused with **Reciprocal Rank Fusion (`k = 60`)**.
- **Automatic deduplication & staleness tracking**: New memories exceeding `0.92` cosine similarity merge cleanly with existing records. A debounced `notify` file watcher tracks edits, renames, and deletions—automatically flagging any confirmed memory as `needs_revalidation` (and writing that status back to Markdown) whenever one of its `bound_paths` source files changes.
- **Confirmed-only injection**: AI-created entries start as `hypothesis` and are injected into a provider's first turn (`<orbit_project_memory>` within a strict character/token budget) only after explicit confirmation.
- **Legacy continuity**: Existing `~/.hermes/memories/*.md` entries are automatically imported as read-only hypotheses scoped by `workspace_root`.

### Optional local or remote embeddings

To augment the built-in offline vectors with an OpenAI-compatible embedding model (such as local Ollama `nomic-embed-text`), configure `memory_embedding` in `$ORBIT_HOME/config.json` and restart the daemon:

```json
{
  "memory_embedding": {
    "provider": "open_ai_compatible",
    "endpoint_url": "http://127.0.0.1:11434/v1/embeddings",
    "model": "nomic-embed-text",
    "auto_inject_budget_tokens": 1200
  }
}
```

Requests enforce a strict 3-second timeout and fall back cleanly to built-in local vectors whenever the endpoint is unavailable.

### The `orbit_memory` Core Tool

Every provider running on an Orbit host—whether Claude Code, Codex, Antigravity, Pi, or any of the 34+ ACP agents—shares the exact same `orbit_memory` Core Pack tool with four actions:

- `recall` (supports `include_unconfirmed: true` to inspect pending hypotheses)
- `remember` (creates a new scoped `hypothesis` record)
- `confirm` (promotes a verified memory for automatic first-turn injection)
- `forget` (removes obsolete records from disk and index)

Paired clients also manage project memories directly over `ListProjectMemories`, `UpsertProjectMemory`, `ConfirmProjectMemory`, `DeleteProjectMemory`, and `RecallProjectMemories`.

## Sandboxed `wasmi` WASM Plugin SDK

Traditional IDE extensions execute arbitrary native code with full access to your user account. Orbit runs plugins inside an embedded pure-Rust **`wasmi` WebAssembly sandbox**:

- **Zero native privilege**: Plugins execute deterministically inside memory- and fuel-bounded WASM instances—never spawning arbitrary subprocesses or reading outside declared workspace capabilities.
- **Cross-provider `orbit_plugin` tool surface**: Plugins register typed tools and lifecycle extensions exposed consistently to every connected agent provider.
- **Declarative Cross-Platform UI**: Instead of shipping separate React, SwiftUI, Compose, and ArkTS bundles, plugins emit declarative UI trees (metrics, tables, action buttons, forms) that render natively across Desktop, Web WASM, iOS, Android, and HarmonyOS.
- **SEP-1865 MCP Apps Bridge**: Orbit also supports MCP servers exposing interactive `ui://` resources (`ReadMcpResource`) and server-bound tool calls (`CallArtifactMcpTool`) directly inside the desktop Artifacts tab.
