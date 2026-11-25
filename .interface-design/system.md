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

### 3.2 Layout & Radii
- **Spacing Grid**: `4px` base (`4 / 8 / 12 / 16 / 20 / 24 / 32`).
- **Radii (`--radius: 8px`, independent of root font size)**: `rounded-sm` (`4px`), `rounded-md` (`6px` chips/icon buttons), `rounded-lg` (`8px` controls/rows), `rounded-xl` (`12px` cards/HUD), `rounded-2xl` (`16px` docked cards), `rounded-[20px]` (Composer & user bubbles), `rounded-full` (avatars/dots).
- **Shell Geometry**: Left Sidebar `280px` (`240–420px`) · Stage Reading Column `max-w-[740px]` · Overview HUD `288px` · Right Side Panel `480px` (`320–1400px`).

### 3.3 Canonical Motion Tokens (`src/index.css`)
- **Curves**: `--ease-out-expo` (snappy entrances & tactile press), `--ease-out-quart` (opacity fades & subtle reveals), `--ease-panel` (structural pane slides & disclosures), `--ease-exit` (fast, crisp dismissals).
- **Durations**: `--dur-fast` (`110ms`, press/micro highlight) · `--dur-ui` (`150ms`, controls, toggles, hover fades, stage cross-dissolve) · `--dur-overlay` (`190ms`, modals & docks) · `--dur-panel` (`240ms`, structural panes; `--duration-panel-in: 260ms` / `--duration-panel-out: 225ms`) · `--dur-exit` (`130ms`, crisp exits). Tooltips use `0ms` warm handoff via `data-instant`.
- **Rules**: Compositor-only (`transform`, `opacity`, GPU `scaleX` progress bars); atomic Light ↔ Dark switching via `html.orbit-theme-switching`; pause ambient animations when window is hidden (`html.orbit-window-hidden`); honor `@media (prefers-reduced-motion: reduce)` across CSS, JS timers, and streaming text reveal (`.orbit-spinner-reduced` uses a calm `2.4s` opacity pulse).

---

## 4. Reusable Primitives (`src/components/ui/`)

Always compose official `shadcn/ui` (`base-nova` / `@base-ui/react`) and Orbit primitives before hand-rolling controls:

- **shadcn/ui Core**: `Button`, `Badge`, `Card`, `Checkbox`, `Collapsible`, `Dialog`, `DropdownMenu`, `Empty`, `Field` (`FieldGroup`/`FieldLabel`/`FieldDescription`/`FieldError`), `Input`, `Item`, `Kbd`/`ShortcutKbd`, `Label`, `Marker`, `Popover`, `Progress`, `ScrollArea`, `Separator`, `SimpleTooltip`, `Switch`, `Textarea`, `Trace` (`TraceCollapsible`/`TraceLine`/`TraceIcon`/`TraceTitle`/`TraceDescription`).
- **Orbit Domain**: `IconButton`, `OrbitSpinner`, `StatusIndicator` / `HostStatusIndicator`, `ListRow`, `ContextBanner`, `DiffStat`, `ToastViewport`, `SubagentAvatar`.

---

## 5. Subagent Animal Portraits (`lib/subagent-avatar.ts`)

- 24 fixed animals in `SUBAGENT_AVATAR_ROSTER`; `assignSubagentCharacters()` deterministically assigns unique slots per sibling set.
- 64-unit inline SVG at `18 / 20 / 24 / 28px`; centerline-spanning contours fuse into continuous closed paths (`fuseCenterlineSymmetric`) to eliminate `x = 32` transform seams.
- Only visible active portraits animate (working hop `1160ms`, awaiting tilt `1560ms` + amber ring, blink `3400ms`); terminal, offscreen, hidden-window, and reduced-motion states render upright.

---

## 6. Key Spatial & Interaction Contracts

1. **Stage Reading Column & Scroll Mechanics** (`stream-stage.tsx`, `conversation-viewport.tsx`, `stage-outline-rail.tsx`):
   - Resting bottom padding is `24px`; active foreground streaming adds a `32px` cushion absorbed by `TurnFooter` at turn-end.
   - Any upward human scroll detaches sticky follow immediately; idle `ResizeObserver` never snaps while dock/shelf height changes compensate scroll cleanly. `holdViewport` pins disclosure triggers and reveals clamped previews for tall disclosures.
   - `UserPromptBubble` and `ConversationUserBubble` collapse on clicking anywhere outside the bubble card.
   - Full-history Outline Rail projects all `SessionInsightsView.outline_prompts`, shows up to 17 ticks before folding earlier prompts, and auto-paginates when jumping to unloaded history.
2. **Steps, Footers & Parallel Viewport Parity** (`stage-step-group.tsx`, `subagents-track.tsx`, `subagent-tab.tsx`, `peer-split-tab.tsx`):
   - Folded steps compose `Trace`; compaction/wakeup milestones use `Marker`. Groups of ≥ 10 steps show a bottom collapse action. Stalled live runs or queued subagent deliveries surface inline status & `Interrupt & continue` / `Interrupt & deliver` controls.
   - Historical `TurnFooter` reveals on hover/focus/touch; `StreamStage`, `SubagentTab`, and `PeerSplitTab` share `TurnStageSection`, `getTurnFooter`, and `getTurnTopGapPx`.
   - `SubagentDispatchRow` and `PeerMessageRow` share a negative-bleed row shell aligned at `x = 0px` with prose. Child subagents live exclusively in the Right Side Panel (`selectAgent` routes child selections to parent thread + right panel).
3. **Composer Dock Stack** (`components/composer/`, `stage-approval-dock.tsx`):
   - Reading column stacks `DecisionDock` → `ComposerPlanShelf` → `ComposerQueueStrip` → `OmniComposer`.
   - `InlineDirectiveToken` (`composer-directive-token.tsx`) is frameless, colorless, baseline-locked (`items-baseline` + `self-center` icon), and shared across `OmniComposer`, `UserPromptBubble`, and `ComposerQueueStrip`.
   - `ComposerPlanShelf` is the sole desktop plan surface, sharing the exact `740px` (`px-[24px]`) reading-column width and `.composer-queue-surface rounded-2xl` (`--surface-raised`) card elevation of `ComposerQueueStrip` and `DecisionDock`. It auto-opens for new unfinished plans, yields to pending decisions, holds live completion briefly (`2.8s`, pausing on hover/expand) before auto-dismissing, and keeps settled history hidden on mount.
   - `OmniComposer` keeps a stationary `size-8.5` right action slot (Stop / Queue-Steer / Spinner).
4. **Overview HUD & TopBar** (`stage-overview-panel.tsx`, `top-bar.tsx`):
   - Docked `288px` card (`.overview-card-surface`) or compact popover (`.floating-surface`), ordered **Live Runtime → Deliverables → Secondary Context**.
   - When Overview is closed or narrow (`< 980px`), `TopBar` shows `SubagentCrewMiniDock` and reason-accurate indicator tooltips.
5. **Sidebar & Side Panel** (`components/sidebar/`, `components/side-panel/`, `settings/`):
   - `Needs Attention` and `Pinned` share `SidebarConversationRow` and deduplicate below. Each row reserves a `16px` leading signal column (awaiting / error / unread / idle armed timer `<AlarmClock>` / worktree) and a single non-shifting trailing status/action cell (foreground work & running subagents spin; idle timer-waiting threads stay calm & talkable).
   - File preview (`file-viewer-tab.tsx`, `workspace-slice.ts`) uses Rust 250-line prefix highlighting, promise-deduplicated intent prefetch (`pointerdown` + `65ms` hover dwell), stale-while-revalidate cache, and a `120ms` loading grace delay.
6. **Desktop Settings** (`components/settings/`):
   - Settings opens as a centered floating modal (`min(1180px, 92vw) × min(820px, 88vh)`, `rounded-[16px]`, `.floating-surface`) above the mounted, `inert` workspace. Close (`×`), clicking outside the backdrop, and `Escape` restore workspace focus, draft, and scroll position; inner drawers, menus, dialogs, and active search consume `Escape` first.
   - A `240px` rail shares the canvas surface with a `1px` subtle divider and fits all 16 destinations under Preferences, Fleet & Sync, Agents & Safety, and Context & Extensions without scrolling on laptop viewports. Preference columns measure up to `680px`, operational pages up to `880px`, and the split Instructions editor fills the available dialog canvas.
   - Host-scoped pages show a persistent host and transport selector in the `44px` top bar when the fleet includes a remote host or multiple hosts. Fleet details use a breadcrumb. Resource editors slide out as `480px` right drawers contained inside the Settings dialog frame. Page titles, subpage titles, controls and metadata use `20/17/13/11px`.
   - Settings search uses real row anchors. `@modified` matches client preferences against their defaults. Hovering or focusing a changed preference reveals a quiet reset action without persistent modified-row chrome. A jump opens the target disclosure, scrolls it into view and highlights it for `1.2s`. Modal and drawer transitions use opacity and transform, and respect reduced motion.
