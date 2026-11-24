import { useEffect, useRef } from "react";
import {
  hasOpenSettingsBlockingOverlay,
  isFocusInsideCodeEditor,
  isFocusInsideFileViewerPane,
  isFocusInsideSidePanel,
  shouldCloseSettingsOnEscape,
  SIDE_PANEL_SELECTOR,
} from "@/lib/file-viewer-shortcuts";
import {
  isAgentForegroundRunning,
  isMacDesktop,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  getFocusedDecisionTarget,
  isAlwaysDecision,
  isDenyDecision,
  stepFocusedDecisionQueue,
} from "@/lib/timeline-coalesce";
import { t as translateGlobal } from "@/lib/i18n";
import { orbitActions } from "@/state/orbit-store";

export function modSymbol(): string {
  return isMacDesktop() ? "⌘" : "Ctrl+";
}

export function isConfirmDialogOpen(): boolean {
  return (
    typeof document !== "undefined" &&
    Boolean(document.querySelector('[data-slot="confirm-dialog"], [role="alertdialog"]'))
  );
}

export function useGlobalErgonomicKeybindings(): void {
  const lastEscTimeRef = useRef<number>(0);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const snap = orbitActions.getSnapshot();
      const mod = isMacDesktop() ? e.metaKey : e.ctrlKey;
      const keyLower = e.key.toLowerCase();

      // Guard: confirmation alertdialogs own keyboard focus (Enter / Esc / Tab) and must block all global chords
      if (isConfirmDialogOpen()) {
        return;
      }

      // 1. Command Palette (`⌘K` / `Ctrl+K`) — yield to terminal clear/kill-line and inline CodeMirror AI Edit
      if (mod && !e.shiftKey && !e.altKey && keyLower === "k" && !snap.terminalFocused) {
        if (!snap.commandPaletteOpen && isFocusInsideCodeEditor(e.target, document.activeElement)) {
          return;
        }
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.setCommandPaletteOpen(!snap.commandPaletteOpen);
        return;
      }

      // 1a. Filter Sidebar Conversations (`⌘F` / `Ctrl+F`) — yield to terminal search, Side Panel / File Viewer, & modal overlays
      if (mod && !e.shiftKey && !e.altKey && keyLower === "f" && !snap.terminalFocused) {
        const sidePanelEl = document.querySelector<HTMLElement>(SIDE_PANEL_SELECTOR);
        const focusInsideSidePanelOrFileViewer =
          isFocusInsideSidePanel(e.target, document.activeElement) ||
          isFocusInsideFileViewerPane(sidePanelEl, e.target, document.activeElement);
        if (focusInsideSidePanelOrFileViewer) {
          return;
        }
        const isOverlayOpen =
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
          Boolean(
            document.querySelector(
              '[data-slot="dialog-content"], [role="dialog"], [role="alertdialog"]',
            ),
          );
        if (!isOverlayOpen) {
          e.preventDefault();
          if (e.repeat) return;
          orbitActions.focusSidebarFilter();
          return;
        }
      }

      // 1b. Settings Center (`⌘,` / `Ctrl+,`) — native standard preferences shortcut
      if (mod && !e.shiftKey && !e.altKey && e.key === ",") {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleSettings();
        return;
      }

      // 1c. Escape inside Settings View returns instantaneously to Workspace —
      //     unless an inner dialog, popover, menu, or listbox overlay is open.
      if (e.key === "Escape" && snap.activeView === "settings") {
        const hasBlockingOverlay =
          snap.pairDialogOpen ||
          snap.relayDialogOpen ||
          snap.remoteFolderPickerHostId !== null ||
          snap.mediaPreviewTarget !== null ||
          hasOpenSettingsBlockingOverlay();
        if (
          shouldCloseSettingsOnEscape({
            key: e.key,
            defaultPrevented: e.defaultPrevented,
            activeView: snap.activeView,
            commandPaletteOpen: snap.commandPaletteOpen,
            targetTagName: (e.target as HTMLElement | null)?.tagName,
            hasBlockingOverlay,
          })
        ) {
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

      // 6. Toggle Diff Review Tab (`⌘D` / `Ctrl+D`) — yield to CodeMirror multi-cursor `selectNextOccurrence`
      if (mod && !e.shiftKey && !e.altKey && keyLower === "d") {
        if (isFocusInsideCodeEditor(e.target, document.activeElement)) {
          return;
        }
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

      // 9. Cycle Session Mode (`Shift+Tab`) — yield to native reverse tabbing inside secondary form inputs
      if (e.shiftKey && e.key === "Tab" && !mod && !e.altKey) {
        const activeEl = document.activeElement as HTMLElement | null;
        const activeTag = activeEl?.tagName;
        const isMainComposerInput = activeEl?.dataset?.orbitComposerInput === "true";
        const inSecondaryEditable =
          !isMainComposerInput &&
          (activeTag === "INPUT" ||
            activeTag === "TEXTAREA" ||
            activeTag === "SELECT" ||
            Boolean(activeEl?.isContentEditable));
        if (
          !inSecondaryEditable &&
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
      //     and Queue Stepper (`⌘[` / `⌘]`) across both parent AND child subagent permissions.
      if (mod && !e.altKey && !e.repeat && snap.selectedHostId && snap.selectedAgentId && !snap.isDraftNewSession) {
        if (!e.shiftKey && (e.key === "[" || e.key === "]")) {
          const delta: -1 | 1 = e.key === "[" ? -1 : 1;
          if (stepFocusedDecisionQueue(delta)) {
            e.preventDefault();
            return;
          }
        }

        const focused = getFocusedDecisionTarget();
        if (focused?.kind !== "question") {
          let targetHostId: string | null = null;
          let targetAgentId: string | null = null;
          let targetRequestId: string | null = null;
          let rawDecisions: readonly string[] = [];
          let resolveHandler: ((decision: string) => void) | undefined;

          if (focused?.kind === "permission") {
            // Strictly honor the exact visible DecisionDock card (parent OR managed child subagent)
            targetHostId = focused.hostId;
            targetAgentId = focused.agentId;
            targetRequestId = focused.requestId;
            rawDecisions = focused.allowedDecisions;
            resolveHandler = focused.onResolve;
          } else {
            // Fallback when dock has not mounted/registered yet: first unresolved parent permission
            const stage = snap.stages[`${snap.selectedHostId}::${snap.selectedAgentId}`];
            const firstPerm = stage?.pending_permissions.find(
              (it): it is Extract<(typeof stage.pending_permissions)[number], { item_type: "permission_request" }> =>
                it.item_type === "permission_request" && !it.resolved_decision,
            );
            if (firstPerm) {
              targetHostId = snap.selectedHostId;
              targetAgentId = snap.selectedAgentId;
              targetRequestId = firstPerm.request_id;
              rawDecisions = firstPerm.allowed_decisions;
            }
          }

          if (targetHostId && targetAgentId && targetRequestId) {
            const decisions =
              rawDecisions.length > 0 ? rawDecisions : ["allow", "allow_always", "deny"];
            const allowChoice =
              decisions.find((d) => !isDenyDecision(d) && !isAlwaysDecision(d)) ?? null;
            const alwaysChoice = decisions.find((d) => isAlwaysDecision(d)) ?? null;
            const denyChoice = decisions.find((d) => isDenyDecision(d)) ?? null;

            const dispatchDecision = (choice: string) => {
              if (resolveHandler) {
                resolveHandler(choice);
                return;
              }
              void orbitIpc
                .resolvePermission(targetHostId!, targetAgentId!, targetRequestId!, choice)
                .catch((err) => {
                  orbitActions.showToast(
                    translateGlobal("approval.permissionFailed", { error: String(err) }),
                  );
                });
            };

            if (!e.shiftKey && keyLower === "y" && allowChoice) {
              e.preventDefault();
              dispatchDecision(allowChoice);
              return;
            }
            if (e.shiftKey && keyLower === "y" && alwaysChoice) {
              e.preventDefault();
              dispatchDecision(alwaysChoice);
              return;
            }
            if (!e.shiftKey && e.key === "Backspace" && denyChoice) {
              const activeEl = document.activeElement as HTMLElement | null;
              const activeTag = activeEl?.tagName;
              const isNonEmptyInput =
                ((activeTag === "INPUT" || activeTag === "TEXTAREA") &&
                  (activeEl as HTMLInputElement | HTMLTextAreaElement).value.length > 0) ||
                (Boolean(activeEl?.isContentEditable) &&
                  (activeEl?.textContent?.length ?? 0) > 0);
              if (!isNonEmptyInput) {
                e.preventDefault();
                dispatchDecision(denyChoice);
                return;
              }
            }
          }
        }
      }

      // 11. Double-Escape to Interrupt Active Agent Turn safely
      if (
        e.key === "Escape" &&
        !e.defaultPrevented &&
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
              orbitActions.showToast(translateGlobal("composer.toastInterruptedTurn"));
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
