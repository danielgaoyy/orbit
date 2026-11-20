---
name: orbit-committee
description: Fan out a hard engineering problem to two independent agents from different model families for read-only analysis, synthesize the best plan, implement it, and run an independent review.
---

# Orbit Committee Planning & Execution

Use multiple distinct model families to eliminate blind spots on high-stakes architecture, debugging, or refactoring problems.

## Steps

1. Call `list_profiles` and `list_providers`. Select **two** read-only analysis/planning configurations, strongly preferring **different provider families** (e.g. Claude Code + Codex, or Antigravity + Codex).
2. Launch two parallel read-only workers via `create_agent` in the current workspace (with `mode: "plan"` or explicit instruction **not to edit files**). Give both the exact problem statement and require concrete root-cause analysis, trade-offs, risks, and step-by-step plan.
3. End your turn and wait for Orbit's automatic finish notifications from both workers.
4. Synthesize both analyses into a unified, superior plan — explicitly calling out where the two agents agreed, disagreed, and how you resolved conflicts.
5. If implementation was requested, either implement directly or delegate into an isolated worktree via `create_workspace(isolation="worktree")` + `create_agent`, then trigger an independent reviewer agent on the final diff before reporting back.
