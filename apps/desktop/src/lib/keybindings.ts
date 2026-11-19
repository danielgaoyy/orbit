import { useEffect, useRef } from "react";
import { isMacDesktop, orbitIpc } from "@/lib/orbit-ipc";
import {
  findFirstPendingPermission,
  getFocusedDecisionTarget,
  isAlwaysDecision,
  isDenyDecision,
} from "@/lib/timeline-coalesce";
import { orbitActions } from "@/state/orbit-store";

export function modSymbol(): string {
  return isMacDesktop() ? "⌘" : "Ctrl+";
}

export function useGlobalErgonomicKeybindings(): void {
  const lastEscTimeRef = useRef<number>(0);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const snap = orbitActions.getSnapshot();
      const mod = isMacDesktop() ? e.metaKey : e.ctrlKey;
      const keyLower = e.key.toLowerCase();

      // 1. Command Palette (`⌘K` / `Ctrl+K`) — always works everywhere
      if (mod && !e.shiftKey && !e.altKey && keyLower === "k") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.setCommandPaletteOpen(!snap.commandPaletteOpen);
        return;
      }

      // 1b. Settings Center (`⌘,` / `Ctrl+,`) — native standard preferences shortcut
      if (mod && !e.shiftKey && !e.altKey && e.key === ",") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleSettings();
        return;
      }

      // 1c. Escape inside Settings View returns instantaneously to Workspace
      if (e.key === "Escape" && snap.activeView === "settings" && !snap.commandPaletteOpen) {
        const tag = (e.target as HTMLElement | null)?.tagName;
        if (tag !== "INPUT" && tag !== "TEXTAREA") {
          e.preventDefault();
          orbitActions.closeSettings();
          return;
        }
      }

      // 2. Toggle Terminal Dock (`⌘J` / `Ctrl+J` or `Ctrl+\``) — works even inside terminal to toggle focus/visibility
      if (
        (mod && !e.shiftKey && !e.altKey && keyLower === "j") ||
        (e.ctrlKey && !e.shiftKey && !e.altKey && e.key === "`")
      ) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleTerminalDock();
        return;
      }

      // Guard: if xterm canvas is focused, do not hijack standard shell keys unless `Mod` is pressed on macOS
      if (snap.terminalFocused && !e.metaKey) {
        return;
      }

      // Guard: do not toggle workspace panels behind full-window Settings or modal dialogs
      const isModalOverlayOpen =
        snap.activeView === "settings" ||
        snap.commandPaletteOpen ||
        snap.pairDialogOpen ||
        snap.relayDialogOpen ||
        snap.remoteFolderPickerHostId !== null;

      if (isModalOverlayOpen) {
        return;
      }

      // 3. New Agent Thread (`⌘N` / `Ctrl+N`)
      if (mod && !e.shiftKey && !e.altKey && keyLower === "n") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.startNewThreadDraft();
        return;
      }

      // 4. Jump to Next Attention / Approval Item (`⌘⇧A` / `Ctrl+Shift+A`)
      if (mod && e.shiftKey && !e.altKey && keyLower === "a") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.jumpToNextAttentionItem();
        return;
      }

      // 5. Toggle Left Fleet Sidebar (`⌘B` / `Ctrl+B`)
      if (mod && !e.shiftKey && !e.altKey && keyLower === "b") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleSidebar();
        return;
      }

      // 6. Toggle Right Diff Review Inspector (`⌘D` / `Ctrl+D`)
      if (mod && !e.shiftKey && !e.altKey && keyLower === "d") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleInspector();
        return;
      }

      // 7. Open in External Editor (`⌘O` / `Ctrl+O`)
      if (mod && !e.shiftKey && !e.altKey && keyLower === "o") {
        e.preventDefault();
        void orbitActions.openWorkspaceInEditor();
        return;
      }

      // 8. Thread Jumping (`⌘1..9` / `Ctrl+1..9`)
      if (mod && !e.shiftKey && !e.altKey && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        const idx = Number.parseInt(e.key, 10) - 1;
        orbitActions.selectThreadByIndex(idx);
        return;
      }

      // 9. Cycle Session Mode (`Shift+Tab`)
      if (e.shiftKey && e.key === "Tab" && !mod && !e.altKey) {
        if (!snap.commandPaletteOpen && !snap.pairDialogOpen && !snap.relayDialogOpen) {
          e.preventDefault();
          orbitActions.cycleActiveMode();
          return;
        }
      }

      // 10. Quick Permission Approval Chords (`⌘Y` Allow, `⌘⇧Y` Always Allow, `⌘⌫` Deny)
      if (mod && !e.altKey && !e.repeat && snap.selectedHostId && snap.selectedAgentId && !snap.isDraftNewSession) {
        const focused = getFocusedDecisionTarget();
        if (focused?.kind !== "question") {
          const tl = snap.timelines[`${snap.selectedHostId}::${snap.selectedAgentId}`];
          if (tl) {
            const focusedPerm =
              focused?.kind === "permission"
                ? tl.items.find(
                    (it): it is Extract<(typeof tl.items)[number], { item_type: "permission_request" }> =>
                      it.item_type === "permission_request" &&
                      !it.resolved_decision &&
                      it.request_id === focused.requestId,
                  ) ?? null
                : null;
            const targetPerm = focusedPerm ?? findFirstPendingPermission(tl.items);
            if (targetPerm) {
              const decisions =
                targetPerm.allowed_decisions.length > 0
                  ? targetPerm.allowed_decisions
                  : ["allow", "allow_always", "deny"];
              const allowChoice =
                decisions.find((d) => !isDenyDecision(d) && !isAlwaysDecision(d)) ?? null;
              const alwaysChoice = decisions.find((d) => isAlwaysDecision(d)) ?? null;
              const denyChoice = decisions.find((d) => isDenyDecision(d)) ?? null;

              if (!e.shiftKey && keyLower === "y" && allowChoice) {
                e.preventDefault();
                void orbitIpc.resolvePermission(
                  snap.selectedHostId,
                  snap.selectedAgentId,
                  targetPerm.request_id,
                  allowChoice,
                );
                return;
              }
              if (e.shiftKey && keyLower === "y" && alwaysChoice) {
                e.preventDefault();
                void orbitIpc.resolvePermission(
                  snap.selectedHostId,
                  snap.selectedAgentId,
                  targetPerm.request_id,
                  alwaysChoice,
                );
                return;
              }
              if (!e.shiftKey && e.key === "Backspace" && denyChoice) {
                const activeEl = document.activeElement as HTMLElement | null;
                const activeTag = activeEl?.tagName;
                const isEditable =
                  activeTag === "INPUT" ||
                  activeTag === "TEXTAREA" ||
                  Boolean(activeEl?.isContentEditable);
                if (!isEditable) {
                  e.preventDefault();
                  void orbitIpc.resolvePermission(
                    snap.selectedHostId,
                    snap.selectedAgentId,
                    targetPerm.request_id,
                    denyChoice,
                  );
                  return;
                }
              }
            }
          }
        }
      }

      // 11. Double-Escape to Interrupt Active Agent Turn safely
      if (
        e.key === "Escape" &&
        !snap.commandPaletteOpen &&
        !snap.pairDialogOpen &&
        !snap.relayDialogOpen
      ) {
        const now = Date.now();
        if (now - lastEscTimeRef.current < 420) {
          lastEscTimeRef.current = 0;
          if (snap.selectedHostId && snap.selectedAgentId) {
            const detail = snap.hostDetails[snap.selectedHostId];
            const ag = detail?.agents.find((a) => a.agent_id === snap.selectedAgentId);
            if (ag?.is_running || ag?.is_awaiting_permission) {
              e.preventDefault();
              void orbitIpc.interruptAgent(snap.selectedHostId, snap.selectedAgentId);
              orbitActions.showToast("Interrupted active turn");
            }
          }
        } else {
          lastEscTimeRef.current = now;
        }
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
