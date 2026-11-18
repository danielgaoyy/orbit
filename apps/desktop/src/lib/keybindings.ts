import { useEffect, useRef } from "react";
import { isMacDesktop, orbitIpc } from "@/lib/orbit-ipc";
import { processTimelineForStage } from "@/lib/timeline-coalesce";
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
        orbitActions.setCommandPaletteOpen(!snap.commandPaletteOpen);
        return;
      }

      // 1b. Settings Center (`⌘,` / `Ctrl+,`) — native standard preferences shortcut
      if (mod && !e.shiftKey && !e.altKey && e.key === ",") {
        e.preventDefault();
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
        orbitActions.toggleTerminalDock();
        return;
      }

      // Guard: if xterm canvas is focused, do not hijack standard shell keys unless `Mod` is pressed on macOS
      if (snap.terminalFocused && !e.metaKey) {
        return;
      }

      // 3. New Agent Thread (`⌘N` / `Ctrl+N`)
      if (mod && !e.shiftKey && !e.altKey && keyLower === "n") {
        e.preventDefault();
        orbitActions.startNewThreadDraft();
        return;
      }

      // 4. Jump to Next Attention / Approval Item (`⌘⇧A` / `Ctrl+Shift+A`)
      if (mod && e.shiftKey && !e.altKey && keyLower === "a") {
        e.preventDefault();
        orbitActions.jumpToNextAttentionItem();
        return;
      }

      // 5. Toggle Left Fleet Sidebar (`⌘B` / `Ctrl+B`)
      if (mod && !e.shiftKey && !e.altKey && keyLower === "b") {
        e.preventDefault();
        orbitActions.toggleSidebar();
        return;
      }

      // 6. Toggle Right Diff Review Inspector (`⌘D` / `Ctrl+D`)
      if (mod && !e.shiftKey && !e.altKey && keyLower === "d") {
        e.preventDefault();
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

      // 10. Quick Permission Approval Chords (`⌘Y` Allow, `⌘⇧Y` Always Allow, `⌘Backspace` Deny)
      if (mod && snap.selectedHostId && snap.selectedAgentId && !snap.isDraftNewSession) {
        const tl = snap.timelines[`${snap.selectedHostId}::${snap.selectedAgentId}`];
        if (tl) {
          const { pendingPermissions } = processTimelineForStage(tl.items);
          const firstPerm = pendingPermissions[0];
          if (firstPerm) {
            if (!e.shiftKey && keyLower === "y") {
              e.preventDefault();
              void orbitIpc.resolvePermission(
                snap.selectedHostId,
                snap.selectedAgentId,
                firstPerm.request_id,
                "allow",
              );
              return;
            }
            if (e.shiftKey && keyLower === "y") {
              e.preventDefault();
              const alwaysChoice =
                firstPerm.allowed_decisions.find((d) => d.includes("always")) ?? "allow_always";
              void orbitIpc.resolvePermission(
                snap.selectedHostId,
                snap.selectedAgentId,
                firstPerm.request_id,
                alwaysChoice,
              );
              return;
            }
            if (e.key === "Backspace") {
              e.preventDefault();
              void orbitIpc.resolvePermission(
                snap.selectedHostId,
                snap.selectedAgentId,
                firstPerm.request_id,
                "deny",
              );
              return;
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
            if (ag?.is_running) {
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
