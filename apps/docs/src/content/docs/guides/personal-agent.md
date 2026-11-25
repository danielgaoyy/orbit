---
title: Personal Agent & IM Dispatch
description: Orchestrate Claude Code, Codex, Antigravity, and ACP subagents across every host through one persistent Personal Agent connected to Slack, Discord, and Feishu / Lark.
---

Orbit centers multi-host engineering around **Personal Router (`AgentRole::PersonalRouter`)** — one persistent **Personal Agent** that coordinates work across your entire fleet.

Instead of opening separate SSH shells and copying context by hand between Claude Code on your Mac, Codex on a Linux build server, and Antigravity on a cloud GPU, you talk to **one persistent coordinator**. It sees every online machine, chains specialist agents in parallel or in sequence, arms conditional wakeup timers, and interrupts you only for high-signal human decisions.

## Global fleet election & cross-host context

- **Any online host can lead**: Elect your home Mac Studio, an always-on Mac mini, or a cloud Devbox as the single global Personal Agent. Your choice is persisted in `primary_router_selection` and synced across clients via encrypted Cloud Sync preferences.
- **Live `<orbit-fleet-state>` and `<orbit-fleet-delta>`**: `PersonalRouterManager` continuously tracks online hosts, discovered projects, active Mission Workspaces, worker statuses, and pending permission checkpoints, injecting compact incremental deltas rather than re-sending static state.
- **Zero-SSH remote context inspection**: Before dispatching work to a remote machine, the Personal Agent calls `orbit_control(action="list_projects" | "inspect_project_context")` to fetch `AGENTS.md`, `README.md`, directory trees, and current Git branches directly over Orbit's encrypted mesh — no manual SSH or file copying required.
- **Legacy memory continuity**: When `share_host_hermes_memory` is enabled (default `true`), existing `~/.hermes/memories/` long-term context interoperates seamlessly without requiring Python or Node.js inside `orbit`.

## Parallel Mission Workspaces & non-interrupting delivery

When a complex task spans multiple modules or hosts, the Personal Agent coordinates workers using Orbit's parallel-first invariants:

1. **Isolated Git worktrees**: Workers fork with `orbit_agent(action="fork", worktree=true)` carrying `orbit.worktree_branch` and `orbit.worktree_source_cwd` labels so multiple agents edit cleanly in parallel without colliding on uncommitted files.
2. **CAS-protected shared `BOARD.md`**: Peers coordinate decisions via section-scoped Compare-And-Swap (`expected_revision`) updates so concurrent agents never overwrite each other's findings.
3. **Non-interrupting async delivery (`PromptOrigin`)**: Human steer inputs (`PromptOrigin::Human`) can interrupt a running turn immediately, whereas asynchronous subagent completion reports (`PromptOrigin::Agent`) always queue safely in FIFO order (`Queued -> Dispatched -> Settled`) without cancelling the coordinator's active reasoning turn.
4. **Idle conditional wakeup timers**: Sessions holding armed background wakeup timers (`has_armed_timers: true`) remain calm and immediately talkable in the UI — never blocked behind a fake loading spinner.
5. **Historical `@handle` aliases**: Renaming an agent handle atomically records the old handle in `labels["orbit.handle_aliases"]` inside the same SQLite transaction so pending peer messages and `sender:<handle>` conditional timers never break.

## 3-Tier Guardian escalation

To keep autonomous fleets safe without drowning you in approval fatigue, Orbit enforces a strict three-tier approval ladder:

| Tier | Risk Level | Handling Path |
| --- | --- | --- |
| **Tier 1** | Low (read-only tools, workspace tests, allowlisted commands) | Automatically approved by `GuardianManager` with an audit trail. |
| **Tier 2** | Medium (scoped writes, package installs, branch operations) | Pre-reviewed by the Personal Agent / LLM Guardian classifier with workspace context. |
| **Tier 3** | High / Critical (credential access, destructive ops, production deploys, secret rotation) | **Protocol hard-blocked from auto-approval.** Always escalated to a human in Orbit's native DecisionDock and mirrored to your IM interactive card. |

## Zero-public-webhook IM long connections

Orbit embeds pure-Rust outbound WebSocket runners (`crates/orbit-daemon/src/im_longconn.rs`) so your Personal Agent stays reachable from mobile IM apps **behind home or corporate NATs without exposing a single public port or webhook URL**:

- **Slack Socket Mode (WSS)**: Outbound `POST /api/apps.connections.open` (`xapp-...`) with `events_api` and `interactive` envelope dispatch plus instant `envelope_id` ACK.
- **Discord Gateway v10 (WSS)**: Outbound `GET /api/v10/gateway/bot` with `HELLO` / jittered `HEARTBEAT` watchdog, `IDENTIFY` / `RESUME`, and fail-closed `ed25519-dalek` signature verification when HTTP webhooks are enabled.
- **Feishu / Lark WebSocket Long-Connection**: Outbound `POST /callback/ws/endpoint` with native binary `pbbp2.Frame` protobuf codec, chunk reassembly, `< 3s` immediate ACK, event deduplication, and bidirectional interactive cards for **Tier-3 approvals** (`ImRoutingMode::PersonalRouter`).

All connection URLs and query credentials (such as Slack `ticket` or Feishu `access_key` parameters) are scrubbed automatically before logging or persistence.
