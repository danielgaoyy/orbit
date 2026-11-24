# Orbit Desktop Design System — Orbital Quiet Precision

This document is the canonical design-system specification for `apps/desktop`. All UI modifications in `apps/` must adhere to these tokens, layout contracts, and component primitives alongside `.agents/skills/interface-design/SKILL.md`.

---

## 1. Visual Direction & Philosophy

- **Aesthetic**: *Orbital Quiet Precision* — calm, monochrome-first engineering console inspired by precision instruments.
- **Color Restraint**: Surfaces, borders, typography, and primary interactive controls remain strictly monochrome along a single subtle cool-slate neutral axis (**OKLCH hue ~258°**). Chromatic hue is reserved exclusively for:
  1. **Semantic Signals** (`success`, `warning`, `danger`, `info`),
  2. **Provider Marks** (Claude, Codex, Gemini, etc.), and
  3. **Subagent animal portraits**, which identify the same specialist across the timeline, crew and approval dock.
- **One Concept, ≤ 2 Representations**: Every core domain concept has at most 1–2 canonical views with zero simultaneous duplication on screen (e.g., Subagents Crew lives in the Overview HUD when open and relays to `SubagentCrewMiniDock` in the TopBar only when Overview is collapsed or narrow).

---

## 2. Token Architecture (`src/index.css`)

### 2.1 Layered Neutral Surfaces (Same-Hue OKLCH 258°)

| Token | Light (`Alabaster Paper`) | Dark (`Obsidian Graphite`) | Usage |
| --- | --- | --- | --- |
| `--surface-recessed` | `#F5F6F8` | `#121318` | Code wells, harmonized terminal backdrop, diff panes, segmented tracks, settings left rail |
| `--surface-base` | `#FCFCFE` | `#17181E` | Primary app canvas, sidebar, main conversation stage, side-panel body |
| `--surface-raised` | `#FFFFFF` | `#20222B` | Interactive cards, Composer shell, Overview HUD cards, SettingCard, DecisionDock |
| `--surface-overlay` | `#FFFFFF` | `#272A35` | Popovers, dropdowns, modals, tooltips, toast viewport |

### 2.2 Ink Hierarchy

| Token | Light | Dark | Usage |
| --- | --- | --- | --- |
| `--ink-primary` | `#14161D` | `#F1F2F7` | Primary prose, titles, KPI figures |
| `--ink-secondary` | `#4A5060` | `#B2B6C7` | Descriptions, body metadata, secondary nav items |
| `--ink-muted` | `#6E7487` | `#83889C` | Timestamps, secondary badges, sub-labels |
| `--ink-faint` | `#969CAE` | `#5F6477` | Section eyebrows, keyboard shortcut hints, empty ticks |

### 2.3 Borders & Primary Actions

- `--border-subtle`: Low-contrast `1px` hairlines dividing panels and card rows (`rgba(20, 22, 29, 0.07)` light / `rgba(241, 242, 247, 0.075)` dark).
- `--border-default`: Resting interactive controls and framed containers.
- `--border-strong`: Focused inputs, selected option cards, active outlines.
- `--action-primary-bg` / `--action-primary-fg` / `--action-primary-hover`: High-contrast monochrome primary actions (`#181A22` / `#FFFFFF` in light; `#F1F2F7` / `#13141A` in dark).

### 2.4 Semantic Signals (WCAG-AA Verified)

| Signal | Light Text | Dark Text | Usage |
| --- | --- | --- | --- |
| `success` | `#0F7B55` | `#3ED598` | Connected direct, completed tasks, diff additions, cache hit rate |
| `warning` | `#B45309` | `#F5B749` | Pending approval, awaiting input, quota ≥ 70%, reconnecting |
| `danger` | `#C8283B` | `#F87171` | Errors, destructive actions, diff deletions, quota ≥ 85% |
| `info` | `#2563EB` | `#6FA8FF` | Relay E2EE transport, active timers, output token composition |

---

## 3. Typography, Spacing & Depth

- **Typefaces**: Inter / SF Pro for UI prose (`13px` controls, `14.5px` / `1.62` stage prose); JetBrains Mono / SF Mono for code, diffs, paths, shortcuts, and all numeric telemetry (`font-mono tabular-nums`).
- **Spacing Grid**: Strict `4px` base grid (`4 / 8 / 12 / 16 / 20 / 24 / 32`).
- **Radii Scale**: Standard shadcn/ui ladder (`--radius: 0.5rem`): `rounded-sm` (`4px`), `rounded-md` (`6px` chips/icon buttons), `rounded-lg` (`8px` inputs/buttons/nav rows/segmented tracks), `rounded-xl` (`12px` cards/HUD panels/modals), `rounded-2xl` (`16px` Composer & Settings modal), `rounded-full` (`9999px` circular avatars & status dots). Never override `--radius-lg` above `8px` so `32px` form controls never collapse into pill ovals.
- **Stage Reading Column**: Centered `max-w-[740px]` across conversation turns, `DecisionDock`, and `OmniComposer`.
- **Desktop Shell Proportions**: Left Sidebar `280px` default / double-click reset (`240px–420px` drag range) · Main Stage `740px` reading column · Overview HUD `288px` · Right Side Panel `480px` default (`320px–1400px`).

---

## 4. Global Reusable Primitives (`src/components/ui/`)

**shadcn/ui Primitive-First Policy**: Follow `.agents/skills/shadcn/SKILL.md`. Before writing UI, inspect `apps/desktop/src/components/ui/` first, reuse and compose official `shadcn/ui` primitives (`@base-ui/react`) with Orbit design tokens, and add missing standard components via `shadcn` instead of hand-rolling raw controls:

- **shadcn/ui Core Primitives**:
  - `<Kbd>`, `<KbdGroup>`, `<ShortcutKbd>` (`ui/kbd.tsx`): All keyboard shortcut displays across sidebar, search inputs, buttons (`variant="inverted"`), tooltips, shortcuts sheet, and command palette.
  - `<Switch>` (`ui/switch.tsx`): Powered by `@base-ui/react/switch` (wrapped by `ToggleSwitch`).
  - `<Progress>` (`ui/progress.tsx`): Powered by `@base-ui/react/progress`, supports semantic indicator styling and optional `trackOverlay` threshold ticks.
  - `<FieldGroup>`, `<Field>`, `<FieldLabel>`, `<FieldDescription>`, `<FieldError>`, `<FieldSet>` (`ui/field.tsx`), `<Input>` (`ui/input.tsx`), `<Textarea>` (`ui/textarea.tsx`) & `<Label>` (`ui/label.tsx`): Canonical form layout & controls aligned to `--surface-base` / `--border-subtle` / `--border-strong` (`8px` radius).
  - `<Button>` (`ui/button.tsx`), `<Badge>` (`ui/badge.tsx`), `<Card>` (`ui/card.tsx`), `<Checkbox>` (`ui/checkbox.tsx`), `<Separator>` (`ui/separator.tsx`), `<Collapsible>` (`ui/collapsible.tsx`), `<ScrollArea>` (`ui/scroll-area.tsx`).
  - `<Dialog>` & `<Tabs>` (`ui/dialog-tabs.tsx`), `<Popover>`, `<DropdownMenu>` (with `<DropdownMenuGroup>` & `variant="destructive"`), `<SimpleTooltip>` (`ui/overlays.tsx`).
- **Orbit Domain Primitives**:
  - `<IconButton>` (`ui/icon-button.tsx`): Unified `xs` (`22px`) / `sm` (`26px`) / `md` (`28px`) icon actions with integrated `SimpleTooltip`, optional `shortcut`, and semantic `indicatorTone` dot.
  - `<StatusIndicator>` & `<HostStatusIndicator>` (`ui/status-indicator.tsx`): Single canonical status language replacing scattered `Loader2` spinners and hand-coded status dots (`running` uses `<OrbitSpinner>`, `online_relay` uses `ShieldCheck` info tint).
  - `<ListRow>` (`ui/list-row.tsx`): Uniform selectable row pattern for Overview HUD, sidebar lists, and dropdowns.
  - `<ContextBanner>` (`ui/context-banner.tsx`): Unified notice primitive in `variant="strip"` (full-width viewport notice) and `variant="inline"` (turn/stage notice).
  - `<DiffStat>` (`ui/diff-stat.tsx`): Monospace tabular `+N / -M` diff summary.
  - `<ToastViewport>` (`ui/toast-viewport.tsx`): Floating `bottom-5 right-5` notification stack.
  - `<SubagentAvatar>` (`ui/subagent-avatar.tsx`) + `lib/subagent-avatar.ts`: Layered SVG animal portraits with activity and approval states.

---

## 5. Subagent animal portraits

- `SUBAGENT_AVATAR_ROSTER` defines 24 animals and their light and dark circular backdrops. The roster order is part of the identity contract. `assignSubagentCharacters()` hashes stable seeds and probes for a free slot for each of the first 24 siblings in sorted order.
- Each portrait uses a 64-unit inline SVG at 18, 20, 24 or 28px. Ears and crowns sit behind the head. Solid face markings and a translucent forehead highlight sit on the coat. The foreground carries the eyes, catchlights and nose or beak. Filled paths can be mirrored about the centerline.
- CSS transforms turn the face and ears in opposite directions. Working agents hop twice over 1160ms. Agents awaiting approval tilt over 1560ms and carry the amber warning ring. Both blink over a separate 3400ms cycle.
- Only visible active portraits animate. Terminal states, offscreen portraits, hidden documents and reduced-motion mode display the upright drawing. The optional corner badge shows a check on completion or a cross on failure or cancellation.

---

## 6. Signature Spatial & Interaction Contracts

1. **Session Overview HUD & Zero-Duplication Subagent Relay (`stage-overview-panel.tsx`, `stage-overview-sections.tsx`, `subagent-crew-dock.tsx`, `top-bar.tsx`)**:
   - `288px` right-side **Overview HUD** (`.overview-card-surface` / `--overview-surface: var(--surface-raised)` when docked; `.floating-surface` / `--overview-surface: var(--surface-overlay)` popover on compact/split stages) uses pure CSS Flexbox scrolling (`min-h-0 shrink overflow-y-auto`, pinned `1px` top hairline, zero JS ResizeObserver subtraction), isolated `DockedOverviewShell` / `PopoverOverviewShell` motion wrappers (zero body re-renders on unrelated sidebar animations), narrow single-host/single-project store subscriptions (immune to remote-host heartbeats & unrelated local threads), quiet header collapse button (`⌘⌥O`), active `ListRow` click-to-toggle + keyboard shortcut tooltips (`⌘D`, `⌘J`), and project-scoped `Terminal` counts.
   - All conditional situational sections follow a strict **Live Runtime → Deliverables → Secondary Context** priority order (`Subagents` → `Goal & Timers` → `Background Tasks` → `Pull Requests` → `Artifacts` [scoped to active session + shared workspace artifacts] → `Links` [localhost -> built-in Browser + prose-only external links] → `Side Questions` [scroll-to-entry on click] → `Modified Files` [strictly created/edited with single `Show N more / Show less`, falling back to `Touched Files` for read-only sessions]) unified on `<OverviewSection>` + `<OverviewExpandFooter>` with persisted collapse states, collapsed signal indicators, always-visible active `Stop` vs hover-only cleanup actions, `--overview-surface`-matched zero-shift hover action pills, leaf-isolated 1s live clocks (`lib/live-clock.ts`), Popover focus restoration on `Esc`, and full 3-locale i18n parity. `Artifacts` lives solely as its direct-access section (no duplicate bare Layer 1 row).
   - When Overview is closed or viewport is narrow (`< 980px`), `TopBar` reveals `<SubagentCrewMiniDock>` with overlapping living avatars and uses `LayoutList` with reason-accurate indicator tooltips (`approval` / `subagent` / `background_task` / `goal` / `timer`). Zero subagent strips above the Composer.
2. **Unified DecisionDock (`stage-approval-dock.tsx`, `keybindings.ts`, `timeline-coalesce.ts`)**:
   - Powered by `selectPendingDecisions()` (`state/selectors/session-selectors.ts`), merging parent permissions, parent questions, and all managed child subagent permissions (with their assigned `<SubagentAvatar>`) into one `rounded-2xl` (`16px`) focal dock above the Composer (while Composer softly recedes via `data-dock-active="true"`), ordered cleanly by explicit `(sortGroup, orderKey)`.
   - `FocusedDecisionTarget` carries `{ hostId, agentId, requestId, allowedDecisions, onResolve }` so `⌘Y` / `⌘⇧Y` / `⌘⌫` (including empty focused textarea) and `⌘[` / `⌘]` queue stepping act accurately on BOTH parent and child subagent cards with `<OrbitSpinner>` submitting feedback and localized error handling.
   - `PermissionApprovalCard` applies semantic risk rims (`data-risk="danger" | "warning"`, keeping `low` risk neutral and hiding the Guardian risk banner for `low` risk), multi-line readable Guardian reasons with classifier tier (`AI policy` / `Rule policy` / `fail_closed`) and signal pills, deduplicated shell titles with non-workspace `cwd` display, `<DiffStat>` & create/edit labels for file edits, line ranges for file reads, `parseGenericToolName` formatting for MCP tools, concise subagent prompt previews, quiet borderless secondary `Always allow` button with session-scoped tooltip, and full 3-locale i18n; `StickyQuestionDockCard` preserves drafts across queue steps and supports `1–9` option shortcuts.
3. **OmniComposer & Extracted Submodules (`components/composer/`)**:
   - Modularized into `omni-composer.tsx` (contextual dynamic placeholder per active directive / foreground-running / follow-up state, `data-orbit-composer-input="true"` scoping for `⇧Tab`, localized mode & submit tooltips, borderless headerless `+` add-context dropdown, standalone immediate slash command fallback), `composer-queue-strip.tsx` (isolated zero-keystroke-re-render `.composer-queue-surface` without redundant header chrome or `Next` badge, drag-to-reorder rows, `Steer` / `Edit` / `×` actions, and semantic directive/skill icons), `composer-model-picker.tsx` (compact shadcn-scale `w-[226px]` / `rounded-[11px]` / `28px`-row `<Popover>` menu, auto-focused keyboard list, `90ms` hover-intent, Antigravity `Orchestrator` grouped cascading menu with Level-2 sub-models & Level-3 thinking flyout when advertised by CLI, single-line compact Level-2 thinking/fast flyout with `ArrowUp`/`ArrowDown`/`ArrowRight`/`ArrowLeft` navigation, single right-edge icon slot swapping selected `Check` to `ChevronRight` on hover / open flyout, `Zap` Fast indicator, profile metadata), `composer-mode-picker.tsx` (container-width `iconOnly`, wrappable localized descriptions, `SessionMode::Custom` support, uniform neutral iconography across all modes), `context-occupancy-pill.tsx` (separate hover vs click-pinned `Popover`, model-switch & throttled running refresh, recoverable retry state, provider-reported authoritative window & persisted single-call prompt telemetry, localized categories including `System & tools` with `%` & hover-linked bar highlighting, clean numeric readout without redundant window sub-label or educational banners), `context-tray.tsx` (skill chips, clickable image & pasted-text preview dialogs, `<OrbitSpinner>`), `composer-hero-header.tsx` (semantic tokens + `<SimpleTooltip>`), and `role="listbox"`/`aria-activedescendant` grouped/ranked `slash-menu.tsx` (filters out draft-ineligible commands & unsupported `/fast`, `/auto` + `/edits` parity, `/fast` `On` indicator only when active, localized search) & `mention-menu.tsx` (`@staged-diff` + `@terminal` full parity + localized built-in descriptions).
   - `composer-directive-token.tsx` places a muted 14px icon and directive name on the first 22px text baseline. The native textarea and sizing mirror use the measured token width plus 7px as extra left padding while empty (WKWebView caret) and first-line indent while typing. The placeholder shares the empty caret inset. The overlay clips at the content top and follows the textarea’s scroll position synchronously. Click focuses offset zero, Backspace there clears the token, and hover shows its localized description. Plan retains the `data-plan="true"` rim. Side questions and goals dispatch on submit before prompt composition or queueing.
   - Keeps a single stationary `34×34` far-right action slot (zero toolbar horizontal jitter: `■` Stop with `Esc Esc` when running & empty, `↑` Queue/Steer with `Enter` / `⌘↵` tooltip when running with draft text, `<OrbitSpinner>` while uploading/sending).
4. **Working-State Reading Runway (`stream-stage.tsx`)**:
   - While an agent is actively running, a dynamic bottom cushion (`clamp(120px, 26vh, 220px)`) plus `56px` base clearance keeps live output centered comfortably above the Composer.
4. **Long Folded Step Groups (`stage-step-group.tsx`)**:
   - `>= 9` steps open into a bounded `340px` internal scroll container with a fixed `32px` top Label Bar and a `Maximize2` toggle for full-timeline expansion (where the Label Bar switches to `position: sticky; top: 0` with a bottom `Collapse N steps ↑` button).
5. **External Timeline Milestones (`turn-delegation-split.ts`)**:
   - Subagent dispatches, inter-agent messages (`AgentMessageCard` with sender `<SubagentAvatar>`), artifacts, compaction notices, timers (`schedule` / `orbit_timer`), and worktree handoffs (`handoff_worktree`) always render outside folded work steps.
6. **TopBar Right Icon Budget (`top-bar.tsx`)**:
   - Conditional `SubagentCrewMiniDock` (only when Overview is hidden/narrow) + `Users` (when peers exist) + `More` | `1px` divider | `Overview` + stationary `Side Panel`. Duplicate `Artifacts` button removed.
7. **Sidebar Hierarchy, Sorting & Deduplication (`fleet-sidebar.tsx`, `sidebar-project-section.tsx`, `sidebar-conversation-row.tsx`, `sidebar-model.ts`, `use-sidebar-drag.ts`)**:
   - `Needs Attention` and `Pinned` both render canonical `SidebarConversationRow` items with full menu/draft/tooltip parity and deduplicate cleanly from below (except during explicit search); sort mode stays cleanly tucked inside the `[⇅]` dropdown menu with zero persistent sort pills beside `Projects` (strictly 2 buttons: `[⇅]` + `[📁+]`, 大道至简). The on-demand thread filter trigger (`🔍` / `⌘F`, matching titles, group names, projects, handles, hosts) sits symmetrically on the right edge of the sidebar's `40px` titlebar drag strip opposite `[◫]` and expands cleanly at the top of the list (above `Needs Attention`, `Pinned`, and `Projects`) only when invoked.
   - Organizing progress stays visible and hover-readable in the project header's right slot (`<OrbitSpinner size={12} />` + shimmer localized `Organizing` with full phase detail on tooltip hover) so the thread list never jumps.
   - Each conversation row reserves a centered 16px leading column. It shows the first applicable signal in this order — awaiting approval (amber dot), error (red dot), unread (blue dot), isolated worktree (muted branch), or empty. Pinned rows use the same column under their section heading. Grouped and child rows keep the column at their own indentation depth so sibling titles line up in every state.
   - At rest the trailing column shows optional project context and one of a 13px monochrome `OrbitSpinner` (foreground work or running subagents), Draft, or shared-clock relative time, in priority order. Hover, action focus, and an open menu reveal Archive and More in the same grid cell. Both layers participate in sizing so the title keeps its width. Background processes and timers stay in Overview. Subagent activity uses the fleet topic.
   - Nested groups indent under their parent project (`pl-[20px]`), align their child vertical guide rail under the `Layers` icon (`ml-[27px] border-l border-border-subtle pl-2`), hide zero-count numbers & hide counts on hover, persist collapse state, create new groups inline without a modal dialog, and move `@handle` badges to row hover tooltips so full conversation titles stay readable.
8. **Settings IA & High-Craft Usage Dashboard (`settings/`)**:
   - Left rail (`236px`) organized into 3 domain groups: `APP`, `HOST` (with top Host Scope switcher when multi-host), and `WORKSPACE`.
   - `Usage & Quotas` promoted to a top-level section with `840px` wide layout, unified Hero Telemetry Card, 3-column inline telemetry strip, hover-dimmed SVG trend chart, `70%/85%` ticked quota gauges, and proportional share bars in By Model / By Project tables.
9. **Harmonized Terminal (`xterm-pane.tsx` & `terminal-tab.tsx`)**:
   - Uses `--surface-recessed` (`#121318` dark / `#F5F6F8` light) with OKLCH-balanced 16-color ANSI palettes by default, plus an optional `High contrast` palette toggle in Settings → Appearance → Terminal.
