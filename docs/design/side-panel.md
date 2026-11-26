# Desktop Per-Session Right Side Panel Architecture

Authoritative contract for the right-hand `WorkspaceSidePanel` in `apps/desktop` (`src/state/side-panel-scope.ts`, `src/state/slices/{fleet,workspace,terminal}-slice.ts`, `src/components/side-panel/`, `src/lib/workspace-layout.ts`, `src/components/layout/workspace-motion.tsx`).

---

## 1. Per-Session Scope Model

Side-panel open state, tabs, active tab, dual-split layout, user dismissal, recently closed tab stack, and unseen-activity badges are strictly **per-session**, keyed by `SidePanelScopeKey`:

- Active thread: `` `${hostId}::${rootAgentId}` `` (child subagents always resolve up `parent_agent_id` / `parent_host_id` links to their root parent).
- Draft composer: `` `draft::${hostId}` ``.
- Global (not per-session): `sidePanelWidth` (stored in `orbit.desktop.prefs.v3`).

Live fields (`sidePanelOpen`, `sidePanelTabs`, `activeSidePanelTabId`, `sidePanelSplit`, `sidePanelUserDismissed`, `sidePanelClosedTabs`, `sidePanelUnseenTabIds`, `sidePanelCollapsedByResize`) represent the currently selected scope; inactive scopes live in `sidePanelByScope: Record<SidePanelScopeKey, SidePanelSnapshot>` (LRU capped at `100` entries; empty never-opened scopes are omitted).

---

## 2. Scope Transitions (`switchSidePanelScope`)

Every store path that mutates `selectedHostId`, `selectedAgentId`, or `isDraftNewSession` calls `switchSidePanelScope` (or `promoteDraftSidePanelScope`) inside the same `setState` batch:

| Action / Event | Behavior |
| --- | --- |
| `selectAgent` (including child -> root redirect) | Saves outgoing live snapshot (unless empty or archived), loads & validates incoming root scope, reconciles left-sidebar auto-yield. |
| `startNewThreadDraft` / `markThreadUnread` | Saves outgoing snapshot, switches to `draft::hostId` (starts closed when fresh), clears `selectedDiffPath`, and rebinds existing draft workspace-level tabs to the target draft cwd. |
| Draft project change (`selectProjectWorkspace` / `setSelectedWorkspaceCwd` / `deleteProject` while drafting) | Same scope (or dropped if no projects remain); `rebindDraftSidePanelCwd` moves draft workspace-level tabs (Files, Review, Terminal, Browser, Side Question, Artifacts) to the new cwd. Single-file tabs stay pinned. |
| Draft promotion (`reloadHostDetail` `newestCreated`) | Moves live `draft::hostId` tabs, closed stack, and side-question state directly onto `hostId::newAgentId` (rebinding draft workspace-level tabs to `newCwd`, e.g., a new Worktree cwd, while keeping pinned file tabs untouched) and deletes `draft::hostId`. |
| Optimistic agent replacement (`replaced_optimistic_agents`) | `remapOptimisticSidePanelScopes` migrates `opt_*` -> `ag_*` across `sidePanelByScope`, `sidePanelScopeKey`, and tab metadata. |
| `selectHost` / `selectProjectWorkspace` / `reloadFleet*` | Swaps to resolved target scope; hydrates persisted snapshot lazily on cold startup. |
| `archiveThread` / `deleteThreadPermanently` / `deleteArchivedThreadsBatch` / `deleteProject` | Drops archived/deleted snapshots via `dropSidePanelScopes`, prunes referencing `subagent`/`peer` tabs from live & cached scopes, fixes split, restores auto-yielded sidebar. |
| `archiveSubagent` / `archiveFinishedSubagents` | `removeSubagentTabsFromScopes` prunes tabs from both live state and cached parent snapshots, normalizes split, and closes/restores sidebar if last tab removed. |

Cross-scope transients (`focusNonce`, `terminalFocused`, `overviewPopoverOpen`, `pendingUnsavedFileAction`) are never carried across scopes. Global `fileEditorDrafts` are preserved across session switches (keyed by `hostId::cwd::path`). Underlying detached terminal PTYs (`terminalTabs`) are never killed on scope switch; `TerminalTab` remains mounted-hidden so WebGL xterm instances retain scrollback.

---

## 3. Tab Scope Binding Invariants

Every `SidePanelTab` stores explicit `hostId`, `agentId` (`string | null`, root parent or `null` for draft), `parentHostId` (for subagents and cross-host `peer` tabs such as Personal Agent workers), and `cwd` where relevant at creation time. Tab renderers (`ArtifactsTab`, `SubagentTab`, `ReviewTab`, `SideQuestionTab`, `FilesExplorerTab`, `FileViewerTab`, `TerminalTab`, `BrowserTab`, `WorkspaceSidePanel`) read only their `tab` fields and never fall back to `s.selectedHostId`, `s.selectedAgentId`, or `s.selectedWorkspaceCwd`.

Every `PanelErrorBoundary` and scope-isolated tab container (`FilesExplorerTab`, `BrowserTab`, `FileViewerTab`) inside `WorkspaceSidePanel` includes `sidePanelScopeKey` in its key/`resetKey` so state or errors trapped in Session A never poison Session B.

---

## 4. Single Open Entrypoint & Auto-Open Policy

All `open*Tab` actions route through `applyOpenSidePanelTab(s, tab, { source, activate })`, where `source` defaults to `"user"` and programmatic / background triggers pass `source: "auto"`:

| Trigger `source` | Panel State | Behavior |
| --- | --- | --- |
| `"user"` | Any | Add/focus tab, open panel, clear `userDismissed`, `collapsedByResize`, and `unseenTabIds`, apply dual-sidebar yield if tight. |
| `"auto"` | Open | Add tab; do not steal focus from another active tab unless `activeSidePanelTabId === null` (Launchpad) and `activate === true`. |
| `"auto"` | Closed, `!userDismissed`, non-compact | Open panel and activate tab. |
| `"auto"` | Closed, `userDismissed` (or compact viewport) | Add tab in background, keep panel closed, append tab ID to `sidePanelUnseenTabIds` (rendered as `--signal-info` dot on top-right side-panel toggle; cleared on open). |

Explicit closes (top-right toggle button, `⌘\`, closing the last tab, or toggling off Review / Terminal / Side Question) set `sidePanelUserDismissed = true` for the **current scope only**.

---

## 5. Width Clamping, Compact Resize & Motion Polish

- **Effective width clamp** (`clampEffectiveSidePanelWidth` & `--orbit-right-w` in `src/lib/workspace-layout.ts`):
  `effectiveWidth = max(320, min(storedWidth, viewportWidth - leftSidebarWidth - 360))`.
  The main conversation stage always retains at least `360px` and the panel at least `320px`, without mutating stored `sidePanelWidth`.
- **Compact viewport collapse**: Entering compact (`< 800px`, `COMPACT_VIEWPORT_COLLAPSE_PX`) collapses an open side panel with `sidePanelCollapsedByResize = true` (leaving `userDismissed` untouched). Leaving compact (`>= 824px`, `COMPACT_VIEWPORT_EXPAND_PX`) reopens the panel when `sidePanelCollapsedByResize && !sidePanelUserDismissed`.
- **Zero transition flash on scope switch**: `switchSidePanelScope` increments `sidePanelScopeSwitchNonce`; `WorkspaceMotionProvider` snaps directly to `phase = "idle"` when a toggle signature change coincides with a scope switch.

---

## 6. Persistence (`orbit.desktop.sidePanel.v1`)

Snapshots persist to `localStorage` under `orbit.desktop.sidePanel.v1`:

```json
{
  "version": 1,
  "byScope": {
    "local::ag_123": {
      "open": true,
      "tabs": [],
      "activeTabId": null,
      "split": null,
      "userDismissed": false,
      "closedTabs": [],
      "unseenTabIds": [],
      "lastUsedAt": 1760000000000
    }
  }
}
```

- Transient tab fields (`focusNonce`, `focusLine`, `focusEndLine`) are stripped on save.
- Writes are debounced (`220ms`) and flushed synchronously on `beforeunload` and Vite HMR dispose.
- Corrupt JSON, wrong `version`, or invalid entries fall back cleanly to `{}`.

---

## 7. Shortcuts & Reopen Closed Tab

- `⌘\` (`toggle_side_panel`): Toggle right side panel for the current session.
- `⌘T` (`new_side_tab`): Open Side Panel Launchpad (new tab).
- `⌘⇧T` (`reopen_closed_side_tab`): Reopen the most recently closed valid tab in the current session (`sidePanelClosedTabs`, max 10 LIFO; dirty-guard cancelled closes are not recorded; archived or deleted `peer`, `subagent`, and `board` targets are skipped and pruned). Also exposed in Command Palette (`act-reopen-closed-side-tab`).
