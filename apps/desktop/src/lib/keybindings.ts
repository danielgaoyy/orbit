import { useEffect, useRef } from "react";
import {
  isAgentForegroundRunning,
  isMacDesktop,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
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

      // 1. Command Palette (`⌘K` / `Ctrl+K`) — yield to terminal clear/kill-line when terminal is focused
      if (mod && !e.shiftKey && !e.altKey && keyLower === "k" && !snap.terminalFocused) {
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

      // 2. Toggle Terminal Tab in Side Panel (`⌘J` / `Ctrl+J` or `Ctrl+\``) — works even inside terminal
      if (
        (mod && !e.shiftKey && !e.altKey && keyLower === "j") ||
        (e.ctrlKey && !e.shiftKey && !e.altKey && e.key === "`")
      ) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleTerminalTab();
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
        snap.quickOpenFilesOpen ||
        snap.pairDialogOpen ||
        snap.relayDialogOpen ||
        snap.remoteFolderPickerHostId !== null ||
        snap.exportDialogTarget !== null ||
        snap.copyCodeBlocksTarget !== null ||
        snap.handoffDialogTarget !== null ||
        snap.mediaPreviewTarget !== null ||
        Boolean(document.querySelector('[data-slot="dialog-content"]'));

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

      // 5b. Toggle Unified Right Side Panel (`⌘\` / `Ctrl+\`)
      if (mod && !e.shiftKey && !e.altKey && e.key === "\\") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleSidePanel();
        return;
      }

      // 5b-2. Open Side Panel New Tab / Launchpad (`⌘T` / `Ctrl+T`)
      if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "t") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.openSidePanelNewTab();
        return;
      }

      // 5c. Toggle Main Stage Overview Panel (`⌘⌥O` / `Ctrl+Alt+O`)
      if (mod && e.altKey && !e.shiftKey && (keyLower === "o" || e.code === "KeyO")) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleOverviewPanel();
        return;
      }

      // 6. Toggle Diff Review Tab (`⌘D` / `Ctrl+D`)
      if (mod && !e.shiftKey && !e.altKey && keyLower === "d") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleReviewTab();
        return;
      }

      // 6b. Quick Open Workspace File (`⌘P` / `Ctrl+P`)
      if (mod && !e.shiftKey && !e.altKey && keyLower === "p") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.setQuickOpenFilesOpen(true);
        return;
      }

      // 6c. Toggle Side Question (`/btw`) Tab (`⌘;` / `Ctrl+;`)
      if (mod && !e.shiftKey && !e.altKey && e.key === ";") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleSideQuestionTab();
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
        if (
          !snap.commandPaletteOpen &&
          !snap.pairDialogOpen &&
          !snap.relayDialogOpen &&
          !snap.exportDialogTarget &&
          !snap.copyCodeBlocksTarget &&
          !snap.handoffDialogTarget &&
          !snap.mediaPreviewTarget
        ) {
          e.preventDefault();
          orbitActions.cycleActiveMode();
          return;
        }
      }

      // 10. Quick Permission Approval Chords (`⌘Y` Allow, `⌘⇧Y` Always Allow, `⌘⌫` Deny)
      if (mod && !e.altKey && !e.repeat && snap.selectedHostId && snap.selectedAgentId && !snap.isDraftNewSession) {
        const focused = getFocusedDecisionTarget();
        if (focused?.kind !== "question") {
          const stage = snap.stages[`${snap.selectedHostId}::${snap.selectedAgentId}`];
          if (stage) {
            const pendingPerms = stage.pending_permissions.filter(
              (it): it is Extract<(typeof stage.pending_permissions)[number], { item_type: "permission_request" }> =>
                it.item_type === "permission_request" && !it.resolved_decision,
            );
            const focusedPerm =
              focused?.kind === "permission"
                ? pendingPerms.find((it) => it.request_id === focused.requestId) ?? null
                : null;
            const targetPerm = focusedPerm ?? pendingPerms[0] ?? null;
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
        !snap.relayDialogOpen &&
        !snap.exportDialogTarget &&
        !snap.copyCodeBlocksTarget &&
        !snap.handoffDialogTarget &&
        !snap.mediaPreviewTarget
      ) {
        const now = Date.now();
        if (now - lastEscTimeRef.current < 420) {
          lastEscTimeRef.current = 0;
          if (!snap.isDraftNewSession && snap.selectedHostId && snap.selectedAgentId) {
            const detail = snap.hostDetails[snap.selectedHostId];
            const ag = detail?.agents.find((a) => a.agent_id === snap.selectedAgentId);
            if (isAgentForegroundRunning(ag) || ag?.is_awaiting_permission) {
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
