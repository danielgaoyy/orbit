---
name: orbit-advisor
description: Get an independent read-only second opinion or diff review from another Orbit agent without modifying any files.
---

# Orbit Advisor (Independent Second Opinion)

Spawn a fresh, unbiased reviewer agent to critique a design, migration plan, or git diff without touching files.

## Steps

1. Call `list_profiles` to look for a Review / Planning / Advisor profile (prefer a different provider family from your own when available). If none exist, pick another available provider via `list_providers` + `list_models`.
2. Call `create_agent` in the current workspace with `mode: "plan"` and a focused review prompt instructing the advisor:
   - **Do not edit any files.**
   - Inspect the exact diff, design, or question.
   - Report concrete correctness bugs, concurrency/security edge cases, missing tests, and unnecessary complexity, with exact file paths and line numbers.
3. Wait for Orbit's automatic finish notification, critically evaluate the advisor's findings, fix valid issues, and present a concise summary to the user.
