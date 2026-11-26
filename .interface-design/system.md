# Orbit Desktop Design System — Orbital Quiet Precision

Canonical visual, typography, motion, and spatial token reference for `apps/desktop`. Universal UX and motion principles live in [`docs/design/README.md`](../docs/design/README.md).

---

## 1. Visual Direction

- **Aesthetic**: *Orbital Quiet Precision* — calm, monochrome-first engineering console.
- **Color Restraint**: Surfaces, borders, text, and primary actions stay strictly monochrome on a cool-slate neutral axis (**OKLCH hue ~258°**). Color is reserved solely for:
  1. **Semantic Signals** (`success`, `warning`, `danger`, `info`),
  2. **Provider Marks** (Claude, Codex, Gemini, etc.), and
  3. **Subagent Animal Portraits** (stable specialist identity across timeline, crew, and approval dock).
- **Zero Duplication**: Every domain concept has at most 1–2 representations and never duplicates simultaneously on screen.

---

## 2. Tokens (`src/index.css`)

### 2.1 Neutral Surfaces (OKLCH 258°)
| Token | Light (`Alabaster Paper`) | Dark (`Obsidian Graphite`) | Usage |
| --- | --- | --- | --- |
| `--surface-recessed` | `#F5F6F8` | `#121318` | Code wells, terminal backdrop, diff panes, segmented tracks |
| `--surface-base` | `#FCFCFE` | `#17181E` | App canvas, sidebar, main conversation stage, side-panel body, Settings rail and canvas |
| `--surface-raised` | `#FFFFFF` | `#20222B` | Cards, Composer shell, Queue strip, Overview HUD, DecisionDock |
| `--surface-overlay` | `#FFFFFF` | `#272A35` | Popovers, dropdowns, modals, tooltips, toasts |

### 2.2 Ink & Borders
| Token | Light | Dark | Usage |
| --- | --- | --- | --- |
| `--ink-primary` | `#14161D` | `#F1F2F7` | Primary prose, active titles, KPI figures |
| `--ink-secondary` | `#4A5060` | `#B2B6C7` | Descriptions, body metadata, secondary items |
| `--ink-tertiary` / `--ink-muted` | `#6E7487` | `#83889C` | Timestamps, counters, secondary badges |
| `--ink-faint` | `#969CAE` | `#5F6477` | Eyebrows, keyboard shortcuts, empty ticks |

- **Hairlines**: `--border-subtle` (dividers/rows), `--border-default` (controls/cards), `--border-strong` (focus/active).
- **Primary Action**: `--action-primary-bg` / `--action-primary-fg` / `--action-primary-hover` (high-contrast monochrome).
- **Signals (WCAG AA)**: `success` (`#0F7B55` / `#3ED598`), `warning` (`#B45309` / `#F5B749`), `danger` (`#C8283B` / `#F87171`), `info` (`#2563EB` / `#6FA8FF`).

---

## 3. Typography, Spacing & Motion Tokens

### 3.1 3-Tier All-Integer Typography Scale (Stage & Primary UI)
Desktop sets `html { font-size: 13px }`; always use explicit `px` classes rather than uncalibrated `text-xs`/`text-sm`. Differentiate emphasis within a tier via ink tokens and font weight (`font-medium` vs `font-normal`), never extra half-pixel sizes.

| Tier | Size / Leading | Role |
| --- | --- | --- |
| **T1 · Reading** | `15px` / `1.65` | Conversation prose (assistant Markdown, user prompt bubble), Composer input & inline directive |
| **T2 · Interface** | `13px` / `1.45` | Controls, rows, Trace steps, Subagent/Peer rows, Todo plan, DecisionDock, Overview rows, code blocks (`20px` leading) |
| **T3 · Meta** | `11px` / `1.35` | Timestamps, section eyebrows, `DiffStat`, `Kbd`, counters (`done/total`), directory hints, line numbers, footers |

- **Monospace**: JetBrains Mono / SF Mono (`font-mono tabular-nums`) for code, diffs, paths, shortcuts, and numeric telemetry; inline code `0.88em`.
- **Document Heading Exceptions**: Markdown headings use integer steps (`H1 20px`, `H2 17px`, `H3–H6 15px`); empty-state hero heading `24px`.
- **Customizable Reading & Code Typography (`src/lib/typography.ts`, `src/index.css`)**:
  - Reading surfaces (Markdown body/compact/headings/tables, user prompt bubbles, Composer input/mirror/placeholder/directive, Side Question, subagent/peer prose) consume `--orbit-read-*` (`13–22px`, default `15px` / `1.65`), `--orbit-composer-*`, and `--orbit-font-content` (`match-ui` default → `var(--orbit-font-ui)`).
  - Code surfaces scale from `codeFontSize` (`11–18px`, default `13px`): Markdown code blocks and CodeMirror consume `--orbit-code-size` (default `13px`, `--orbit-code-block-leading: 1.6`), while Diff code lines and File Viewer code lines consume `--orbit-code-pane-size` (`round(12 × codeSize / 13)`, default `12px`) and `--orbit-code-line` (default `20px`). For font stacks, CodeMirror and xterm.js preserve their own JetBrains Mono–first default stacks (`--orbit-font-code` / `ORBIT_TERMINAL_DEFAULT_STACK`), whereas Markdown code blocks, Diff code lines, and File Viewer code lines consume `--orbit-font-code-surface` (default `var(--font-mono)`, overridden only when the user picks a non-default code font, which also prepends CJK monospace fallbacks).
  - Tailwind v4 arbitrary variable bindings MUST carry explicit CSS type hints (`text-(length:--orbit-*)`, `leading-(--orbit-*)`, `font-(family-name:--orbit-*)`, `max-h-(--orbit-*)`) so `tailwind-merge` never drops ink color utilities.
  - Interface chrome (sidebar, top bar, Settings, buttons, badges, gutter line numbers) stays fixed in `px` (never `rem`, never `html { font-size }` mutation, never CSS `zoom`) and scales solely via native **Window Zoom** (`80%–150%` in `10%` steps via Tauri `WebviewWindow::set_zoom`, synced across `main` and `orbit-terminal-*`, excluding `orbit-browser-*`). On macOS (`html.orbit-mac`), `--orbit-zoom` instantaneously compensates titlebar height (`max(calc(40px / var(--orbit-zoom)), 30px)`) and traffic-light horizontal offsets (`--orbit-traffic-x`, `--orbit-traffic-pad`, `--orbit-detached-pad`) with zero CSS transitions. Window zoom and text size are orthogonal and multiplicative (`screen size = text size × zoom`).

### 3.2 Layout & Radii
- **Spacing Grid**: `4px` base (`4 / 8 / 12 / 16 / 20 / 24 / 32`).
- **Radii (`--radius: 8px`, independent of root font size)**: `rounded-sm` (`4px`), `rounded-md` (`6px` chips/icon buttons), `rounded-lg` (`8px` controls/rows), `rounded-xl` (`12px` cards/HUD), `rounded-2xl` (`16px` docked cards), `rounded-[20px]` (Composer & user bubbles), `rounded-full` (avatars/dots).
- **Shell Geometry**: Left Sidebar `280px` (`240–420px`) · Stage Reading Column `max-w-[740px]` · Overview HUD `304px` · Right Side Panel `480px` (`320–1400px`).

### 3.3 Canonical Motion Tokens (`src/index.css`)
- **Curves**: `--ease-out-expo` (snappy entrances & tactile press), `--ease-out-quart` (opacity fades & subtle reveals), `--ease-panel` (structural pane slides & disclosures), `--ease-exit` (fast, crisp dismissals).
- **Durations**: `--dur-fast` (`110ms`, press/micro highlight) · `--dur-ui` (`150ms`, controls, toggles, hover fades, stage cross-dissolve) · `--dur-overlay` (`190ms`, modals & docks) · `--dur-panel` (`240ms`, structural panes; `--duration-panel-in: 260ms` / `--duration-panel-out: 225ms`) · `--dur-exit` (`130ms`, crisp exits). Tooltips use `0ms` warm handoff via `data-instant`.
- **Rules**: Compositor-only (`transform`, `opacity`, GPU `scaleX` progress bars); atomic Light ↔ Dark switching via `html.orbit-theme-switching`; pause ambient animations when window is hidden (`html.orbit-window-hidden`); honor `@media (prefers-reduced-motion: reduce)` across CSS, JS timers, and streaming text reveal (`.orbit-spinner-reduced` uses a calm `2.4s` opacity pulse).

---

## 4. Reusable Primitives (`src/components/ui/`)

Always compose official `shadcn/ui` (`base-nova` / `@base-ui/react`) and Orbit primitives before hand-rolling controls:

- **shadcn/ui Core**: `Button`, `Badge`, `Card`, `Checkbox`, `Collapsible`, `Dialog`, `DropdownMenu`, `Empty`, `Field` (`FieldGroup`/`FieldLabel`/`FieldDescription`/`FieldError`), `Input`, `Item`, `Kbd`/`ShortcutKbd`, `Label`, `Marker`, `Popover`, `Progress`, `ScrollArea`, `Separator`, `SimpleTooltip`, `Switch`, `Textarea`, `Trace` (`TraceCollapsible`/`TraceLine`/`TraceIcon`/`TraceTitle`/`TraceDescription`).
- **Orbit Domain**: `IconButton`, `OrbitSpinner`, `Segmented`, `StatusIndicator` / `HostStatusIndicator`, `ListRow`, `AgentListItem`, `StageRailHeader` / `StageRailScrollBody` / `StageRailSection` / `StageRailExpandFooter` / `StageRailEmptyState`, `ContextBanner`, `DiffStat`, `ToastViewport`, `SubagentAvatar`, `ResizeSash`, `DragShield`, `DragGhost`, `DropIndicatorLine`.

---

## 5. Subagent Animal Portraits (`lib/subagent-avatar.ts`)

- 24 fixed animals in `SUBAGENT_AVATAR_ROSTER`; `assignSubagentCharacters()` deterministically assigns unique slots per sibling set.
- 64-unit inline SVG at `18 / 20 / 24 / 28px`; centerline-spanning contours fuse into continuous closed paths (`fuseCenterlineSymmetric`) to eliminate `x = 32` transform seams.
- Only visible active portraits animate (working hop `1160ms`, awaiting tilt `1560ms` + amber ring, blink `3400ms`); terminal, offscreen, hidden-window, and reduced-motion states render upright.

---

## 6. Key Spatial & Interaction Contracts

1. **Stage Reading Column & Scroll Mechanics** (`stream-stage.tsx`, `conversation-viewport.tsx`, `stage-outline-rail.tsx`):
   - Resting bottom padding is `24px`; active foreground streaming adds a `32px` cushion absorbed by `TurnFooter` at turn-end.
   - Any upward human scroll detaches sticky follow immediately; idle `ResizeObserver` never snaps while dock/shelf height changes compensate scroll cleanly. `holdViewport` pins disclosure triggers and reveals clamped previews for historical/tall disclosures, while expanding the trailing latest step disclosure (or its inner steps) from the bottom tracks `scrollHeight - clientHeight` across the expand transition and preserves `stickToBottom = true` so subsequent tool calls keep auto-scrolling.
   - `UserPromptBubble` and `ConversationUserBubble` collapse on clicking anywhere outside the bubble card.
   - Image attachments across user prompt bubbles (`PromptImageLightbox`) and the composer tray (`ContextTray`) share a frameless Claude-style `ImageLightbox` (`components/viewers/image-lightbox.tsx`): full-viewport dark scrim (`bg-black/82 backdrop-blur-[6px]`) with the image floating directly at center (zero card chrome or title bar), click-scrim/`Esc` dismissal, click/wheel zoom + drag pan, subtle floating top-right circular actions, and `←`/`→` gallery controls only for multi-image sets.
   - Full-history Outline Rail projects all `SessionInsightsView.outline_prompts`, shows up to 17 ticks before folding earlier prompts, and auto-paginates when jumping to unloaded history.
2. **Steps, Footers & Parallel Viewport Parity** (`stage-step-group.tsx`, `stage-activity-wave.tsx`, `subagents-track.tsx`, `subagent-tab.tsx`, `agent-stage-pane.tsx`):
   - Folded steps compose `Trace`; active context compaction renders `CompactingStatusRow` (`TraceLine status="running"`, subtle pulsing `FoldVertical` icon, shimmer label, and live elapsed timer), then settles into `CompactionMilestoneRow` (gradient-faded centered hairline divider with `FoldVertical` icon, localized manual/auto label, and optional `tabular-nums` token metadata); wakeup milestones use `Marker`. Live step groups show `[dynamic active tool icon or subtle pulsing 13px Brain icon while thinking] [active label] [elapsed]` with pure spacing (no middle-dot separator) and no step counts, diff stats, stall/offline hints, queued counts, `Interrupt & continue` buttons, or thinking headlines; completed tool groups stay iconless (except a `13px` `AlertTriangle` when carrying unrecovered failures), omit per-group duration, and use count-free natural summaries (`Edited files, read files, ran commands`), whereas thought-only disclosures (`Thought about it` / `Thought for {duration}` / `已思考`) keep the restrained static `13px` `Brain` icon so settled thinking never jumps horizontally. Completed turns whose pre-final segments consist strictly of pure tool-bearing `step_group`s (no earlier `assistant_message`, `notice`, `compaction`, `wakeup`, visible `agent_message`, or promoted `timer`/`handoff` milestone) fold behind a single `Worked for {duration}` turn disclosure (falling back to the natural work summary when `< 1s` and expanding directly to `StepTimelineRail` without a nested second fold); multi-reply, wakeup, warning, compaction, and milestone turns preserve every important segment visibly in chronological order while folding their surrounding step groups quietly. Inner tool rows use `FilePenLine` (edit) and `GitBranch` (subagent); inner thinking rows align with sibling rows via a `13px` `Brain` icon (pulsing while active, static when settled) while omitting truncated body previews (`Thinking…` / `Thought for {duration}`). Expanded groups of `≤ 12` steps flow inline; groups of `> 12` steps show the latest 8 steps with a top `Show N earlier steps` reveal (staying folded until explicitly clicked, never auto-expanding earlier steps on settle or historical errors) and no bottom collapse button.
   - Historical `TurnFooter` reveals on hover/focus/touch; `StreamStage`, `SubagentTab`, and `AgentStagePane` share `TurnStageSection`, `getTurnFooter`, `getTurnTopGapPx`, and `StageActivityWave`.
   - Outgoing subagent dispatches (`invoke_subagent`, `create_agent`, `spawn_workspace_peer`, `detail.kind === "subagent"`) and outgoing inter-agent messages (`send_message`, `send_agent_prompt`, `message_peer`) stay folded inside regular step groups (aligned with Codex / Antigravity, using `GitBranch` / `MessageSquareText` inner icons and direct-open subagent actions) rather than splitting work blocks; only session-level state milestones (`timer`, `handoff_worktree`) promote outside the fold. On regular worker/peer timelines, incoming direct agent dispatches (`create_agent` `initial_prompt`, `spawn_workspace_peer`, `send_agent_prompt`, `message_peer`) reuse the global `UserPromptBubble` with a restrained localized `11px` source provenance label above the bubble (`From Personal Agent` / `来自 个人代理` / `From @handle`) that clicks through to the source session when `source_agent_id` is available. On **Personal Agent** timelines, background completion reports and worker callbacks never masquerade as user bubbles or leak raw `<agent-response>` XML: each report opens its own assistant turn with a quiet clickable `11px` source tag (`From {title} · Host`), the assistant's natural summary prose, and a default-collapsed detail disclosure (`Details` / `Open conversation`); the Personal Agent composer stays minimalist (retaining text input, attachments, and `<ComposerModelPicker />` while hiding `ContextOccupancyPill`, `ComposerModePicker`, `ComposerQueueStrip`, and red Stop squares, allowing direct follow-up appends alongside a subtle `Working…` indicator). Routine/queued `finish_notification` callbacks, resolved `permission_context` callbacks, and pure system/protocol noise (`<orbit-fleet-state>`, `<orbit-fleet-delta>`, control envelopes) stay completely silent across both main stage and subagent/peer timelines (rejoining surrounding step groups and omitting phantom footers), with zero inline `Queued · waiting` pills or `Interrupt & deliver` buttons; only live pending blocking approvals, failed finish alerts, and heartbeat wakeups render inline via `AgentMessageCard` with zero animal avatars—leaving rich animal portraits, live timers, and nested trees exclusively to the Overview HUD, TopBar Mini-Dock, and Approval Dock. Active background dependencies surface via `StageActivityWave` at the bottom of the message stream as a quiet text-only shimmer summary (no leading dot; clickable only while the Overview preview card is hidden; no concentric ripple rings or trailing Overview hint). While the foreground turn is actively streaming, non-blocking background tasks, running subagents, timers, and goals stay suppressed so the stream never stacks two live animations; blocking `awaiting_permission` subagents always surface immediately in warning tone. Child subagents live exclusively in the Right Side Panel (`selectAgent` routes child selections to parent thread + right panel).
3. **Composer Dock Stack** (`components/composer/`, `stage-approval-dock.tsx`):
   - Reading column stacks `DecisionDock` → `ComposerPlanShelf` → `ComposerQueueStrip` → `OmniComposer`.
   - `InlineDirectiveToken` (`composer-directive-token.tsx`) is frameless, colorless, baseline-locked (`items-baseline` + `self-center` icon), and shared across `OmniComposer`, `UserPromptBubble`, and `ComposerQueueStrip`.
   - `ComposerPlanShelf` is the sole desktop plan surface, sharing the exact `740px` (`px-[24px]`) reading-column width and `.composer-queue-surface rounded-2xl` (`--surface-raised`) card elevation of `ComposerQueueStrip` and `DecisionDock`. It auto-opens for new unfinished plans, yields to pending decisions, holds live completion briefly (`2.8s`, pausing on hover/expand) before auto-dismissing, and keeps settled history hidden on mount.
   - `OmniComposer` keeps a stationary `size-8.5` right action slot (Stop / Queue-Steer / Spinner).
4. **Overview HUD & TopBar** (`stage-overview-panel.tsx`, `personal-agent-briefing-rail.tsx`, `stage-rail-primitives.tsx`, `subagent-crew-dock.tsx`, `top-bar.tsx`):
   - Both `StageOverviewPanel` and `PersonalAgentBriefingRail` share `StageRailShell` (docked `304px` `.overview-card-surface` or narrow popover `.floating-surface`), `StageRailHeader`, `StageRailScrollBody`, `StageRailSection` (`OverviewSection`, with persisted `overviewCollapsedSections`, collapsed live-status indicators, and hover-revealed actions), `StageRailExpandFooter`, `OverviewGoalRow`, and `ArmedTimerRow`. Session Overview orders **Live Runtime → Deliverables → Secondary Context**; Personal Agent Briefing orders **Needs you → In progress (strictly actively executing agents) → Waiting → Completed (settled/stopped/idle PA workers with hover `Archive done`) → Handled for you → Hosts (`hosts.length > 1` only)**.
   - `AgentListItem` (`components/ui/agent-list-item.tsx`) is the single canonical strict 2-line row/card primitive shared by Overview `SubagentCrewList`, `PersonalAgentBriefingRail` (`Needs you` / `In progress` / `Completed`), and `WorkspaceBoardTab` (`Threads`): left circular `SubagentAvatar` (with corner status badge on terminal states `completed`/`failed`/`canceled`, animated hop/tilt on active states, calm upright on `idle`), right strictly 2 lines — 13px single-line truncated title + 11px metadata line (`[ProviderIcon] Model Effort · Status`, with no dot separator between Model and Effort, `shrink-0` on icon, Effort, and semantic Status, `min-w-0 truncate` on Model, and graceful fallback to `[ProviderIcon] Agent · Status` when model is unknown or `depth >= 4`), plus optional inline/action slots (`titleTrailing`, `trailing`, `variant="card"` + `footer` for approval controls) and zero 3rd-line text. Hover tooltips surface full lineage, Agent, raw model ID, effort, status, worktree branch, and prompt/step preview.
   - When Overview/Briefing is closed or narrow (`< 996px`), `TopBar` uniformly renders `SubagentCrewMiniDock`, conversation actions (`...`), a `1px` hairline divider, and the standard `LayoutList` toggle with reason-accurate indicator tooltips across both regular sessions and Personal Agent (normalizing legacy `"Personal Router"` default titles to localized `Personal Agent` / `个人代理` / `パーソナルエージェント`).
5. **Sidebar & Side Panel** (`components/sidebar/`, `components/side-panel/`, `settings/`):
   - `Needs Attention` and `Pinned` share `SidebarConversationRow` and deduplicate below. Each row reserves a `16px` leading signal column (awaiting / error / unread / idle armed timer `<AlarmClock>` / worktree) and a single non-shifting trailing status/action cell (foreground work & running subagents spin; idle timer-waiting threads stay calm & talkable).
   - `SubagentTab` opens with a unified `32px` semantic context header (`data-slot="subagent-context-header"`) across Orbit-managed subagents, provider-native subagents, and worker previews opened from `PersonalAgentBriefingRail`: left shows `[ProviderIcon] Agent · Model Effort` (with no dot separator between Model and Effort) plus optional bounded worktree/remote-host chips (preceded by clickable ancestor crumbs only for nested trees `depth >= 2`, avoiding 1-level tab-title duplication); right pins the live semantic status indicator (plus a subtle `Focus session` action only for top-level non-child workers).
   - File preview (`file-viewer-tab.tsx`, `workspace-slice.ts`) uses Rust 250-line prefix highlighting, promise-deduplicated intent prefetch (`pointerdown` + `65ms` hover dwell), stale-while-revalidate cache, and a `120ms` loading grace delay.
6. **Desktop Settings** (`components/settings/`):
   - Settings opens as a centered floating modal (`min(1180px, 92vw) × min(820px, 88vh)`, `rounded-[16px]`, `.floating-surface`) above the mounted, `inert` workspace. Close (`×`), clicking outside the backdrop, and `Escape` restore workspace focus, draft, and scroll position; inner drawers, menus, dialogs, and active search consume `Escape` first.
   - A `240px` rail shares the canvas surface with a `1px` subtle divider and fits all 16 single-noun destinations under Preferences (`General`, `Appearance`, `Workspace`, `Shortcuts`, `Notifications`, `Voice`), Fleet & Sync (`Hosts`, `Cloud Sync`), Agents & Safety (`Agents`, `Permissions`, `Usage`), and Context & Extensions (`Context`, `MCP`, `Hooks`, `Extensions`, `Channels`) without scrolling on laptop viewports. Preference columns measure up to `680px`, operational pages up to `880px`, and the split Instructions editor fills the available dialog canvas.
   - Host-scoped pages show a persistent host and transport selector in the `44px` top bar when the fleet includes a remote host or multiple hosts. Fleet details use a breadcrumb. Resource editors slide out as `480px` right drawers contained inside the Settings dialog frame. Page titles, subpage titles, controls and metadata use `20/17/13/11px`.
   - Settings search uses real row anchors. A jump opens the target disclosure, scrolls it into view and highlights it for `1.2s`. Modal and drawer transitions use opacity and transform, and respect reduced motion.
7. **Unified Drag, Resize & Drop Contract** (`lib/drag/`, `components/ui/resize-sash.tsx`, `components/ui/drag-feedback.tsx`):
   - **Shared Pointer Session (`usePointerDrag`, `useEdgeAutoScroll`)**: All drag surfaces use Pointer Events (`4px` sort/pan threshold, `2px` resize threshold with double-click pass-through), lock `body.cursor`, portal full-viewport `<DragShield />` & `<DragGhost />` to `document.body` (preventing transformed/contained panels and embedded iframes/WebGL from clipping or swallowing events), and cancel/rollback cleanly on `Escape`, `pointercancel`, `window.blur`, and unmount.
   - **Pane & Split Resizers (`ResizeSash`)**: Sidebar edge, Side Panel edge, Side Panel dual-split, and Terminal multi-pane splits share `6px` hit tracks with `1.5px` monochrome lines (`--border-strong` hover, state-driven `--ink-muted` dragging), rAF coalescing, double-click reset with localized hints, `Escape` rollback, and keyboard arrow nudging.
   - **Inline Sortables (`useInlineSortable`)**: Queue rows, Side Panel tabs, and Terminal sub-tabs share 1:1 zero-rerender DOM `translate3d` lift using the unified borderless, shadowless `.drag-lifted-surface` (`--surface-overlay` fill, shared with Sidebar Projects, Pinned rows, `<DragRowPreview />`, and `<DragGhost />`), live neighbor slide (`150ms --ease-out-expo`), and `140ms` slot settle (with `prefers-reduced-motion` instant commit) plus edge auto-scroll.
   - **Sidebar Tree & Pinned Drag (`useSidebarDrag`)**: Pinned threads and Projects lift their actual DOM nodes inline (`translate3d`) with `.drag-lifted-surface` while siblings displace live (`150ms --ease-out-expo`) and settle into their vacated slot (`140ms`, with optimistic project order sync and `maxLeadInPx = 56` for variable-height expanded projects). Conversations lift a portaled full-width `<DragRowPreview />` (leaving a clean empty slot placeholder) so rows never clip across `overflow: hidden` Curated Groups; same-scope siblings displace live with zero insertion line, while cross-group/cross-scope drops cleanly reveal `--signal-info-bg` / `--signal-info-border` container rings or a cross-scope `<DropIndicatorLine />` alongside live edge-scroll hit recalculation.
   - **External File Drop (`useFileDrop`)**: Tauri `dragDropEnabled: false` + `installGlobalFileDropGuard()` routes OS file drops across the entire Conversation Stage (`NewSessionLaunchpad` + active `StreamStage`) and `OmniComposer` with strict `Files` MIME filtering, cross-ancestor deduplication, and single non-duplicated drop feedback.
   - **Canvas Pan/Zoom & Scrubbing (`usePanZoom`)**: `ImageLightbox` and `DiagramLightbox` share `4px` drag thresholds, explicit `cursor-grab` / `cursor-grabbing` affordances, viewport-clamped pan bounds, and `--ease-out-expo` reset transitions; `MediaPlayer` seek and volume tracks share hover/scrub monochrome thumb feedback.
