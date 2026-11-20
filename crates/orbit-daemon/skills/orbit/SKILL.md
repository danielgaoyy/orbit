---
name: orbit
description: Orchestrate multi-agent workflows, isolated git worktrees, terminals, schedules, heartbeats, and cross-host delegation using Orbit MCP tools or the orbit CLI.
---

# Orbit Orchestration Reference

Use Orbit's built-in MCP tools (or the `orbit` CLI) to coordinate multiple AI coding agents across providers, isolated worktrees, and connected hosts.

## Core Principles

1. **Discover before delegating:**
   - Call `list_profiles` first and read each profile's `when_to_use` notes to pick the best specialist for planning, implementation, UI work, or independent review.
   - If no profile matches, call `list_providers` and `list_models` to choose an available provider/model, and tell the user which fallback you chose.
   - For multi-machine work, call `list_hosts` to discover reachable remote hosts (`host_id`).
2. **Isolate parallel edits with Worktrees:**
   - When multiple workers edit files concurrently, call `create_workspace` with `isolation: "worktree"` (prefer `base_ref: "origin/main"` or the repository's default remote branch) for each independent task, then pass that `workspace_id` into `create_agent`.
   - Read-only research or review workers can share the caller's workspace (omit `workspace_id`).
3. **Never busy-wait or sleep-poll:**
   - `create_agent` and `send_agent_prompt` are asynchronous and automatically register a durable finish notification (`notify_on_finish: true` by default).
   - After delegating, stop and let Orbit wake you up automatically when workers finish, fail, or hit a permission checkpoint.
4. **Handle permission checkpoints cleanly:**
   - When a worker requires tool approval, Orbit sends you a `<permission-request>` notification with `agent_id`, `host_id`, and `request_id`.
   - Review the requested tool/command: if safe and aligned with the delegated task, call `respond_to_permission`; if high-risk or ambiguous, escalate to the human user.
5. **Keep long tasks moving with Heartbeats & Schedules:**
   - Use `create_heartbeat` to periodically wake yourself (e.g., every `10m`) while monitoring builds/migrations, and call `delete_heartbeat` once done.
   - Use `create_schedule` for recurring fresh-agent jobs (daily triage, maintenance checks).
