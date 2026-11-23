import {
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  hasVisibleActiveTerminal,
} from "../store-helpers";
import type { OrbitStoreContext } from "../store-runtime";

export function createTerminalSlice(ctx: OrbitStoreContext) {
  const {
    detachedTerminalIds,
    getState,
    setState,
    terminalChannels,
  } = ctx;

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
      void orbitIpc.startTerminal(hostId, cwd, 110, 28);
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
        const nextActive =
          s.activeTerminalTabKey === tabKey
            ? (remaining.find((t) => t.hostId === s.selectedHostId)?.key ?? remaining[0]?.key ?? null)
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
