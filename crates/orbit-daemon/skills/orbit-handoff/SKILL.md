---
name: orbit-handoff
description: Transfer the current engineering task and structured context to a specialist Orbit agent (optionally in its own isolated git worktree).
---

# Orbit Task Handoff

Transfer the active task cleanly to another agent with complete context so it can execute without rediscovering everything.

## Steps

1. Inspect profiles via `list_profiles` (and `list_hosts` if the user requested a remote machine). Pick the profile matching the user's request or the task specialty.
2. If the receiving agent will modify files independently of the current checkout, create an isolated worktree workspace via `create_workspace(isolation="worktree")`.
3. Compose a tight, actionable handoff briefing containing:
   - **Goal & Acceptance Criteria**: exact expected outcome and verification commands.
   - **Relevant Files & Symbols**: concrete paths and line numbers already investigated.
   - **Progress & Decisions**: what is already done and key design choices made.
   - **Constraints & Pitfalls**: edge cases, invariants, or files not to touch.
4. Launch the receiving agent with `create_agent` (passing the briefing as `initial_prompt`).
5. Report the spawned agent ID, chosen provider/model/profile, and workspace path back to the user. Wait for Orbit's automatic finish notification if supervision is requested.
