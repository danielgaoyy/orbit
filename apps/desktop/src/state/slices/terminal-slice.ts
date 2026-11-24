import {
  orbitIpc,
} from "@/lib/orbit-ipc";
import { t as translateGlobal } from "@/lib/i18n";
import {
  hasVisibleActiveTerminal,
} from "../store-helpers";
import type { OrbitStoreContext } from "../store-runtime";
import type { TerminalSplitDirection, TerminalSplitGroup } from "../store-types";

const MAX_SPLIT_PANES = 4;
const PENDING_SPLIT_TTL_MS = 15_000;
const PENDING_CMD_TTL_MS = 15_000;

interface PendingTerminalSplit {
  hostId: string;
  sourceTabKey: string;
  direction: TerminalSplitDirection;
  requestedAtMs: number;
}

interface PendingTerminalCommand {
  hostId: string;
  payload: string;
  execute: boolean;
  requestedAtMs: number;
}

function makeEqualRatios(count: number): number[] {
  if (count <= 0) return [];
  const each = 1 / count;
  return Array.from({ length: count }, () => each);
}

function pruneSplitsForClosedTab(
  splits: readonly TerminalSplitGroup[],
  closedKey: string,
): TerminalSplitGroup[] {
  const next: TerminalSplitGroup[] = [];
  for (const group of splits) {
    if (!group.paneKeys.includes(closedKey)) {
      next.push(group);
      continue;
    }
    const remainingKeys = group.paneKeys.filter((k) => k !== closedKey);
    if (remainingKeys.length >= 2) {
      next.push({
        ...group,
        paneKeys: remainingKeys,
        ratios: makeEqualRatios(remainingKeys.length),
      });
    }
  }
  return next;
}

export function createTerminalSlice(ctx: OrbitStoreContext) {
  const {
    detachedTerminalIds,
    getState,
    notifyToast,
    setState,
    terminalChannels,
  } = ctx;

  let pendingSplit: PendingTerminalSplit | null = null;
  let pendingCommand: PendingTerminalCommand | null = null;

  return {


    openTerminalTab(opts?: { spawnIfEmpty?: boolean }): void {
      const tabId = "tab:terminal";
      setState((s) => {
        const exists = s.sidePanelTabs.some((t) => t.id === tabId);
        const nextTabs = exists
          ? s.sidePanelTabs
          : [...s.sidePanelTabs, { id: tabId, kind: "terminal" as const, title: "Terminal" }];
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: tabId,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });

      const snap = getState();
      if (opts?.spawnIfEmpty !== false && snap.selectedHostId) {
        const hostTabs = snap.terminalTabs.filter((t) => t.hostId === snap.selectedHostId);
        if (hostTabs.length === 0) {
          ctx.actions.spawnNewTerminalTab(snap.selectedHostId, snap.selectedWorkspaceCwd || "~");
        } else if (!hostTabs.some((t) => t.key === snap.activeTerminalTabKey)) {
          setState((s) => ({ ...s, activeTerminalTabKey: hostTabs[0].key }));
        }
      }
    },


    toggleTerminalTab(openOverride?: boolean): void {
      const tabId = "tab:terminal";
      const isCurrentlyActive = getState().sidePanelOpen && getState().activeSidePanelTabId === tabId;
      if (openOverride === false || (openOverride === undefined && isCurrentlyActive)) {
        ctx.actions.toggleSidePanel(false);
        return;
      }
      ctx.actions.openTerminalTab();
    },


    setTerminalFocused(focused: boolean): void {
      if (getState().terminalFocused === focused) return;
      setState((s) => ({ ...s, terminalFocused: focused }));
    },


    spawnNewTerminalTab(hostId: string, cwd: string): void {
      pendingSplit = null;
      void orbitIpc.startTerminal(hostId, cwd, 110, 28);
    },


    splitActiveTerminalPane(direction: TerminalSplitDirection): void {
      const snap = getState();
      const hostId = snap.selectedHostId;
      if (!hostId) return;
      const hostTabs = snap.terminalTabs.filter((t) => t.hostId === hostId);
      const activeTab =
        hostTabs.find((t) => t.key === snap.activeTerminalTabKey) ?? hostTabs[0] ?? null;
      if (!activeTab) {
        ctx.actions.openTerminalTab({ spawnIfEmpty: true });
        return;
      }
      const existingSplit = snap.terminalSplits.find(
        (g) => g.hostId === hostId && g.paneKeys.includes(activeTab.key),
      );
      if (existingSplit && existingSplit.paneKeys.length >= MAX_SPLIT_PANES) {
        notifyToast(translateGlobal("terminal.maxSplitPanes"));
        return;
      }
      pendingSplit = {
        hostId,
        sourceTabKey: activeTab.key,
        direction,
        requestedAtMs: Date.now(),
      };
      ctx.actions.openTerminalTab({ spawnIfEmpty: false });
      void orbitIpc.startTerminal(hostId, activeTab.cwd || snap.selectedWorkspaceCwd || "~", 90, 24);
    },


    setTerminalSplitRatios(splitId: string, ratios: number[]): void {
      setState((s) => {
        const idx = s.terminalSplits.findIndex((g) => g.id === splitId);
        if (idx < 0) return s;
        const target = s.terminalSplits[idx];
        if (ratios.length !== target.paneKeys.length) return s;
        const next = s.terminalSplits.slice();
        next[idx] = { ...target, ratios };
        return { ...s, terminalSplits: next };
      });
    },


    runCommandInTerminal(
      command: string,
      opts?: { execute?: boolean; hostId?: string; cwd?: string },
    ): void {
      const cleaned = command.replace(/\r\n/g, "\n").trim();
      if (!cleaned) return;
      const snap = getState();
      const hostId = opts?.hostId ?? snap.selectedHostId;
      if (!hostId) return;
      const execute = opts?.execute ?? true;
      // Single-line safe commands execute cleanly; multi-line or destructive snippets
      // use bracketed paste without trailing Enter so the user can review before running.
      const payload = execute ? `${cleaned}\r` : `\x1b[200~${cleaned}\x1b[201~`;

      const hostTabs = snap.terminalTabs.filter((t) => t.hostId === hostId);
      const activeTab =
        hostTabs.find((t) => t.key === snap.activeTerminalTabKey) ?? hostTabs[0] ?? null;

      ctx.actions.openTerminalTab({ spawnIfEmpty: false });

      if (activeTab?.terminalId) {
        setState((s) => ({ ...s, activeTerminalTabKey: activeTab.key }));
        void orbitIpc.writeTerminalInput(hostId, activeTab.terminalId, payload);
        notifyToast(
          translateGlobal(execute ? "terminal.commandSentToast" : "terminal.commandPastedToast"),
        );
        return;
      }

      pendingCommand = {
        hostId,
        payload,
        execute,
        requestedAtMs: Date.now(),
      };
      const targetCwd = opts?.cwd || snap.selectedWorkspaceCwd || "~";
      void orbitIpc.startTerminal(hostId, targetCwd, 110, 28);
    },


    onTerminalTabAttached(hostId: string, tabKey: string, terminalId: string): void {
      const now = Date.now();
      if (
        pendingSplit &&
        pendingSplit.hostId === hostId &&
        now - pendingSplit.requestedAtMs <= PENDING_SPLIT_TTL_MS &&
        pendingSplit.sourceTabKey !== tabKey
      ) {
        const req = pendingSplit;
        pendingSplit = null;
        setState((s) => {
          const sourceStillOpen = s.terminalTabs.some((t) => t.key === req.sourceTabKey);
          const newStillOpen = s.terminalTabs.some((t) => t.key === tabKey);
          if (!sourceStillOpen || !newStillOpen) return s;

          const existingIdx = s.terminalSplits.findIndex(
            (g) => g.hostId === hostId && g.paneKeys.includes(req.sourceTabKey),
          );
          if (existingIdx >= 0) {
            const existing = s.terminalSplits[existingIdx];
            if (existing.paneKeys.includes(tabKey) || existing.paneKeys.length >= MAX_SPLIT_PANES) {
              return s;
            }
            const sourcePos = existing.paneKeys.indexOf(req.sourceTabKey);
            const nextKeys = existing.paneKeys.slice();
            nextKeys.splice(sourcePos + 1, 0, tabKey);
            const nextSplits = s.terminalSplits.slice();
            nextSplits[existingIdx] = {
              ...existing,
              direction: req.direction,
              paneKeys: nextKeys,
              ratios: makeEqualRatios(nextKeys.length),
            };
            return { ...s, terminalSplits: nextSplits, activeTerminalTabKey: tabKey };
          }

          const created: TerminalSplitGroup = {
            id: `split_${now}_${Math.random().toString(36).slice(2, 6)}`,
            hostId,
            direction: req.direction,
            paneKeys: [req.sourceTabKey, tabKey],
            ratios: [0.5, 0.5],
          };
          return {
            ...s,
            terminalSplits: [...s.terminalSplits, created],
            activeTerminalTabKey: tabKey,
          };
        });
      }

      if (
        pendingCommand &&
        pendingCommand.hostId === hostId &&
        now - pendingCommand.requestedAtMs <= PENDING_CMD_TTL_MS &&
        terminalId
      ) {
        const cmd = pendingCommand;
        pendingCommand = null;
        // Brief delay lets the interactive shell finish initial prompt setup on first spawn
        window.setTimeout(() => {
          void orbitIpc.writeTerminalInput(hostId, terminalId, cmd.payload);
          notifyToast(
            translateGlobal(cmd.execute ? "terminal.commandSentToast" : "terminal.commandPastedToast"),
          );
        }, 280);
      }
    },


    async attachExistingTerminal(
      hostId: string,
      terminalId: string,
      cwd: string,
      title: string,
      preferredSlot?: number | null,
    ): Promise<void> {
      detachedTerminalIds.delete(`${hostId}::${terminalId}`);
      const existing = getState().terminalTabs.find(
        (t) => t.hostId === hostId && t.terminalId === terminalId,
      );
      if (existing) {
        setState((s) => ({ ...s, activeTerminalTabKey: existing.key }));
        ctx.actions.openTerminalTab({ spawnIfEmpty: false });
        return;
      }

      // Query Rust core for the authoritative slot bound to `terminalId` (reusing its existing slot
      // or allocating a free slot across BOTH main-window tabs and open floating detached windows!)
      // so reattaching one terminal can never steal a slot from another open detached window.
      const coreSlot =
        preferredSlot && preferredSlot >= 1
          ? preferredSlot
          : await orbitIpc.ensureTerminalSlot(hostId, terminalId).catch(() => null);

      // Re-check after async IPC in case a concurrent HostChanged event already inserted this tab
      const existingAfterResolve = getState().terminalTabs.find(
        (t) => t.hostId === hostId && t.terminalId === terminalId,
      );
      if (existingAfterResolve) {
        setState((s) => ({ ...s, activeTerminalTabKey: existingAfterResolve.key }));
        ctx.actions.openTerminalTab({ spawnIfEmpty: false });
        return;
      }

      let slot: number;
      let shouldDispatchLegacyAttach = false;
      if (coreSlot !== null && coreSlot >= 1) {
        slot = coreSlot;
      } else {
        const usedSlots = new Set(
          getState().terminalTabs.filter((t) => t.hostId === hostId).map((t) => t.slot),
        );
        slot = 1;
        while (usedSlots.has(slot) && slot < 240) {
          slot += 1;
        }
        shouldDispatchLegacyAttach = true;
      }

      const tabKey = `${hostId}::slot-${slot}`;
      setState((s) => {
        const withoutDupKey = s.terminalTabs.filter((t) => t.key !== tabKey);
        return {
          ...s,
          terminalTabs: [
            ...withoutDupKey,
            { key: tabKey, hostId, terminalId, slot, cwd, title },
          ],
          activeTerminalTabKey: tabKey,
        };
      });
      ctx.actions.openTerminalTab({ spawnIfEmpty: false });
      if (shouldDispatchLegacyAttach) {
        void orbitIpc.attachTerminal(hostId, terminalId, slot);
      }
    },


    async closeTerminalTab(
      tabKey: string,
      killRemoteShim: boolean,
      expectedTerminalId?: string | null,
    ): Promise<boolean> {
      const target = getState().terminalTabs.find((t) => t.key === tabKey);
      if (!target) return false;
      if (expectedTerminalId !== undefined && target.terminalId !== expectedTerminalId) {
        return false;
      }
      if (target.terminalId) {
        const detachedKey = `${target.hostId}::${target.terminalId}`;
        if (killRemoteShim) {
          await orbitIpc.killTerminal(target.hostId, target.terminalId);
          detachedTerminalIds.delete(detachedKey);
          const latest = getState().terminalTabs.find((t) => t.key === tabKey);
          if (!latest || latest.terminalId !== target.terminalId) {
            return false;
          }
        } else {
          detachedTerminalIds.add(detachedKey);
        }
      }
      terminalChannels.delete(`${target.hostId}::${target.slot}`);
      setState((s) => {
        const remaining = s.terminalTabs.filter((t) => t.key !== tabKey);
        const existingGroup = s.terminalSplits.find((g) => g.paneKeys.includes(tabKey));
        const siblingKey = existingGroup?.paneKeys.find((k) => k !== tabKey) ?? null;
        const nextSplits = pruneSplitsForClosedTab(s.terminalSplits, tabKey);
        const nextActive =
          s.activeTerminalTabKey === tabKey
            ? (siblingKey && remaining.some((t) => t.key === siblingKey)
                ? siblingKey
                : remaining.find((t) => t.hostId === s.selectedHostId)?.key ??
                  remaining[0]?.key ??
                  null)
            : s.activeTerminalTabKey;
        const nextTerminalFocused =
          s.terminalFocused &&
          s.activeTerminalTabKey !== tabKey &&
          hasVisibleActiveTerminal({
            sidePanelOpen: s.sidePanelOpen,
            sidePanelTabs: s.sidePanelTabs,
            activeSidePanelTabId: s.activeSidePanelTabId,
            terminalTabs: remaining,
            activeTerminalTabKey: nextActive,
          });
        return {
          ...s,
          terminalTabs: remaining,
          terminalSplits: nextSplits,
          activeTerminalTabKey: nextActive,
          terminalFocused: nextTerminalFocused,
        };
      });
      return true;
    },


    setActiveTerminalTab(tabKey: string): void {
      setState((s) => ({ ...s, activeTerminalTabKey: tabKey }));
    },


    subscribeTerminalSlot(
      hostId: string,
      slot: number,
      onBytes: (bytes: Uint8Array) => void,
    ): () => void {
      const chKey = `${hostId}::${slot}`;
      let ch = terminalChannels.get(chKey);
      if (!ch) {
        ch = { chunks: [], subs: new Set() };
        terminalChannels.set(chKey, ch);
      }
      // Replay buffered bytes when mounting
      for (const buffered of ch.chunks) {
        onBytes(buffered);
      }
      ch.subs.add(onBytes);
      return () => {
        ch?.subs.delete(onBytes);
      };
    },


    /**
     * Append a raw binary PTY chunk into the non-reactive tail ring buffer (used by
     * `captureActiveTerminalTail` for `@terminal` composer context) without causing store re-renders.
     */
    recordTerminalOutputChunk(hostId: string, slot: number, bytes: Uint8Array): void {
      if (bytes.byteLength === 0) return;
      const chKey = `${hostId}::${slot}`;
      let ch = terminalChannels.get(chKey);
      if (!ch) {
        ch = { chunks: [], subs: new Set() };
        terminalChannels.set(chKey, ch);
      }
      ch.chunks.push(bytes);
      if (ch.chunks.length > 96) {
        ch.chunks.splice(0, ch.chunks.length - 96);
      }
    },
  };
}
