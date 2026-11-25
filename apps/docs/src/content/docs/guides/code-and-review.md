---
title: Code, Git Rewind & Orbit Patrol
description: Edit workspace files with CodeMirror 6, LSP, and FIM ghost text, rewind agent file changes to any turn, resolve 3-way Git conflicts, and run Two-Stage Orbit Patrol PR reviews.
---

Orchestrating agents is only half of software engineering; **verifying, editing, rewinding, and landing changes cleanly** is where production quality is won. Orbit builds the full code and review loop directly beside the conversation stage.

## CodeMirror 6 + LSP + FIM Edit Prediction

Open any local or remote workspace file in the right Side Panel (`⌘P` / `Ctrl+P` Quick Open or clicking any path in the timeline):

- **Instant Rust-assisted preview**: Uses Rust 250-line prefix syntax highlighting, promise-deduplicated hover/pointerdown intent prefetch, and a stale-while-revalidate file cache so remote files feel instantaneous.
- **First-class Language Server Protocol (LSP)**: Query workspace symbols, hover types, jump to definition, inspect references, view inline diagnostics, trigger code actions, and execute safe cross-file renames over the remote host's language servers (`lsp_hover`, `lsp_goto_definition`, `lsp_references`, `lsp_rename`, `lsp_apply_workspace_edit`).
- **FIM (Fill-in-the-Middle) Ghost Text**: Low-latency inline edit predictions appear directly inside CodeMirror 6 as calm translucent ghost text — accept with `Tab` or dismiss with `Esc`, with zero popup clutter.

## Interactive Conversation & File Rewind

When an agent takes a wrong architectural turn, you never have to untangle partial edits manually:

- Invoke **Rewind** (`rewind_agent`) on any prior turn in the timeline.
- Orbit restores both the **conversation checkpoint** and the **exact workspace files modified after that turn** (`restored_files` count + restored draft prompt), letting you branch cleanly in a better direction.
- Want to explore two approaches simultaneously instead of discarding the current attempt? Use **Fork to Worktree** (`orbit fork --worktree`) to branch the conversation and repository state into an isolated Git worktree at any `seq`.

## Full Git Write Loop & Worktree Handoff

Press `⌘D` (`Ctrl+D`) to toggle the unified **Review** tab, supporting both **Working Tree** and **Session Turns** diff scopes:

- **Hunk-level precision**: Stage, unstage, or discard individual diff hunks or whole files; leave line-anchored review comments and send them back to the active agent in one click.
- **Stash, Rebase & 3-Way Conflict Resolver**: Manage Git stashes (`git_stash_*`), monitor or abort/continue rebases (`git_rebase_*`), and resolve merge/rebase conflicts visually block-by-block (`git_list_conflicts` / `git_resolve_conflict`) without dropping into raw conflict markers.
- **Bidirectional Worktree Handoff**: Safely transfer finished work between isolated agent worktrees and your main checkout with automatic backup refs and conflict-aware `dry_run` / `apply` / `move` modes (`worktree_handoff.rs`).
- **Commit, Push & PR Checks**: Generate deterministic or AI-assisted commit messages, push branches, open pull requests, and inspect live GitHub CI check status directly above the diff.

## Two-Stage Orbit Patrol PR Review

Before merging changes produced by multiple parallel agents, run **Orbit Patrol** for structured, high-signal verification:

1. **Stage 1 — Automated Sweep & Invariant Triage**: Scans changed hunks, touched files, LSP diagnostics, test results, and workspace rules (`AGENTS.md` / path rules) across the PR to surface potential regressions, security boundary violations, and missing parity updates.
2. **Stage 2 — Deep Verification & Actionable Review**: Re-checks candidate findings against surrounding call sites and runtime invariants to eliminate noisy false positives, posting concise, line-anchored fix recommendations or handing confirmed items straight to a specialist subagent to patch.
