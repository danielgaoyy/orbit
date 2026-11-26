import { useEffect, useRef } from "react";
import {
  hasOpenSettingsBlockingOverlay,
  isFocusInsideCodeEditor,
  isFocusInsideFileViewerPane,
  isFocusInsideSidebar,
  isFocusInsideSidePanel,
  resolveGlobalFindAction,
  shouldCloseSettingsOnEscape,
  SIDE_PANEL_SELECTOR,
} from "@/lib/file-viewer-shortcuts";
import {
  isAgentForegroundRunning,
  isMacDesktop,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  type ShortcutActionId,
  getActionShortcutDisplay,
  matchesActionChord,
} from "@/lib/shortcut-registry";
import {
  getFocusedDecisionTarget,
  isAlwaysDecision,
  isDenyDecision,
  stepFocusedDecisionQueue,
} from "@/lib/timeline-coalesce";
import { t as translateGlobal } from "@/lib/i18n";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { selectPersonalAgentRef } from "@/state/selectors/personal-agent-selectors";

export function modSymbol(): string {
  return isMacDesktop() ? "⌘" : "Ctrl+";
}

export { getActionShortcutDisplay };

export function useActionShortcutDisplay(actionId: ShortcutActionId): string {
  const customKeybindings = useOrbitSelector((s) => s.customKeybindings);
  return getActionShortcutDisplay(actionId, customKeybindings);
}

export function isConfirmDialogOpen(): boolean {
  return (
    typeof document !== "undefined" &&
    Boolean(document.querySelector('[data-slot="confirm-dialog"], [role="alertdialog"]'))
  );
}

function isShortcutRecorderActive(): boolean {
  return (
    typeof document !== "undefined" &&
    Boolean(document.querySelector('[data-orbit-shortcut-recorder="true"]'))
  );
}

export function useGlobalErgonomicKeybindings(): void {
  const lastEscTimeRef = useRef<number>(0);

  useEffect(() => {
    const innerEscapes = new WeakSet<KeyboardEvent>();
    const captureOverlayEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && orbitActions.getSnapshot().activeView === "settings"
        && hasOpenSettingsBlockingOverlay()) innerEscapes.add(event);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      const snap = orbitActions.getSnapshot();
      const custom = snap.customKeybindings;
      const isMac = isMacDesktop();
      const mod = isMac ? e.metaKey : e.ctrlKey;
      const keyLower = e.key.toLowerCase();

      // A nested layer may have already dismissed itself during event propagation.
      if (e.defaultPrevented || e.isComposing) return;

      // Guard: shortcut recorder & confirmation alertdialogs own keyboard focus
      if (isShortcutRecorderActive() || isConfirmDialogOpen()) {
        return;
      }

      // 1. Command Palette (`⌘K` / custom) — yield to terminal clear/kill-line and inline CodeMirror AI Edit
      if (matchesActionChord(e, "command_palette", custom, isMac) && !snap.terminalFocused) {
        if (!snap.commandPaletteOpen && isFocusInsideCodeEditor(e.target, document.activeElement)) {
          return;
        }
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.setCommandPaletteOpen(!snap.commandPaletteOpen);
        return;
      }

      // 1a. Focus-aware Find (`⌘F` / custom) and Sidebar Filter (`⌘⇧F` / custom)
      const isCustomFind = Boolean(custom.find);
      const isCustomSidebarFilter = Boolean(custom.sidebar_filter);
      if (snap.activeView !== "settings" && isCustomSidebarFilter && matchesActionChord(e, "sidebar_filter", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.focusSidebarFilter();
        return;
      }
      if (
        (isCustomFind && matchesActionChord(e, "find", custom, isMac)) ||
        (!isCustomFind && mod && !e.altKey && keyLower === "f")
      ) {
        const sidePanelEl = document.querySelector<HTMLElement>(SIDE_PANEL_SELECTOR);
        const focusInsideSidePanelOrFileViewer =
          isFocusInsideSidePanel(e.target, document.activeElement) ||
          isFocusInsideFileViewerPane(sidePanelEl, e.target, document.activeElement);
        const focusInsideSidebar = isFocusInsideSidebar(e.target, document.activeElement);
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
        const hasOpenConversation = Boolean(
          snap.workspacePanel === "conversation" &&
            snap.selectedHostId &&
            snap.selectedAgentId &&
            !snap.isDraftNewSession,
        );
        const findAction = isCustomFind
          ? !snap.terminalFocused && !focusInsideSidePanelOrFileViewer && !isOverlayOpen && hasOpenConversation
            ? "stage_find"
            : null
          : resolveGlobalFindAction({
              mod,
              shiftKey: e.shiftKey,
              altKey: e.altKey,
              keyLower,
              terminalFocused: snap.terminalFocused,
              focusInsideSidePanel: focusInsideSidePanelOrFileViewer,
              focusInsideSidebar,
              isOverlayOpen,
              hasOpenConversation,
            });
        if (findAction === "stage_find") {
          e.preventDefault();
          if (e.repeat) return;
          orbitActions.openStageFind();
          return;
        }
        if (findAction === "sidebar_filter") {
          e.preventDefault();
          if (e.repeat) return;
          orbitActions.focusSidebarFilter();
          return;
        }
      }

      // 1b. Settings Center (`⌘,` / custom)
      if (matchesActionChord(e, "open_settings", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleSettings();
        return;
      }

      // 1c. Escape inside Settings View returns instantaneously to Workspace
      if (e.key === "Escape" && snap.activeView === "settings") {
        const hasBlockingOverlay =
          innerEscapes.has(e) ||
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
            hasBlockingOverlay,
          })
        ) {
          e.preventDefault();
          orbitActions.closeSettings();
          return;
        }
      }

      if (snap.activeView === "settings") return;

      // 2. Toggle Terminal Tab in Side Panel (`⌘J` / `Ctrl+\`` / custom) — works even inside terminal
      if (matchesActionChord(e, "toggle_terminal", custom, isMac)) {
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

      // 3. New Agent Thread (`⌘N` / custom)
      if (matchesActionChord(e, "new_conversation", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.startNewThreadDraft();
        return;
      }

      // 3b. Personal Agent Home (`⌘⇧O` / custom)
      if (matchesActionChord(e, "open_global_overview", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        const pa = selectPersonalAgentRef(snap);
        if (
          !pa &&
          snap.workspacePanel === "overview" &&
          snap.selectedHostId &&
          snap.selectedAgentId
        ) {
          orbitActions.selectAgent(snap.selectedHostId, snap.selectedAgentId);
        } else {
          orbitActions.openGlobalOverview();
        }
        return;
      }

      // 4. Jump to Next Attention / Approval Item (`⌘⇧A` / custom)
      if (matchesActionChord(e, "next_attention", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.jumpToNextAttentionItem();
        return;
      }

      // 5. Toggle Left Fleet Sidebar (`⌘B` / custom)
      if (matchesActionChord(e, "toggle_sidebar", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleSidebar();
        return;
      }

      // 5b. Toggle Unified Right Side Panel (`⌘\` / custom)
      if (matchesActionChord(e, "toggle_side_panel", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleSidePanel();
        return;
      }

      // 5b-2. Open Side Panel New Tab / Launchpad (`⌘T` / custom)
      if (matchesActionChord(e, "new_side_tab", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.openSidePanelNewTab();
        return;
      }

      // 5b-3. Reopen Closed Side Panel Tab (`⌘⇧T` / custom)
      if (matchesActionChord(e, "reopen_closed_side_tab", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.reopenClosedSidePanelTab();
        return;
      }

      // 5c. Toggle Main Stage Overview HUD (`⌘⌥O` / custom)
      if (matchesActionChord(e, "toggle_overview_hud", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleOverviewPanel();
        return;
      }

      // 6. Toggle Diff Review Tab (`⌘D` / custom) — yield to CodeMirror multi-cursor
      if (matchesActionChord(e, "toggle_diff", custom, isMac)) {
        if (isFocusInsideCodeEditor(e.target, document.activeElement)) {
          return;
        }
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleReviewTab();
        return;
      }

      // 6b. Quick Open Workspace File (`⌘P` / custom)
      if (matchesActionChord(e, "quick_open_file", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.setQuickOpenFilesOpen(true);
        return;
      }

      // 6c. Toggle Side Question (`/btw`) Tab (`⌘;` / custom)
      if (matchesActionChord(e, "toggle_side_question", custom, isMac)) {
        e.preventDefault();
        if (e.repeat) return;
        orbitActions.toggleSideQuestionTab();
        return;
      }

      // 7. Open in External Editor (`⌘O` / custom)
      if (matchesActionChord(e, "open_external_editor", custom, isMac)) {
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

      // 9. Cycle Session Mode (`Shift+Tab` / custom)
      if (matchesActionChord(e, "cycle_mode", custom, isMac)) {
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

      // 10. Quick Permission Approval Chords (`⌘Y`, `⌘⇧Y`, `⌘⌫`, `⌘[`, `⌘]` / custom)
      if (!e.repeat && snap.selectedHostId && snap.selectedAgentId && !snap.isDraftNewSession) {
        if (matchesActionChord(e, "approval_prev", custom, isMac)) {
          if (stepFocusedDecisionQueue(-1)) {
            e.preventDefault();
            return;
          }
        }
        if (matchesActionChord(e, "approval_next", custom, isMac)) {
          if (stepFocusedDecisionQueue(1)) {
            e.preventDefault();
            return;
          }
        }

        const wantAllow = matchesActionChord(e, "approve_allow", custom, isMac);
        const wantAlways = matchesActionChord(e, "approve_always", custom, isMac);
        const wantDeny = matchesActionChord(e, "approve_deny", custom, isMac);

        if (wantAllow || wantAlways || wantDeny) {
          const focused = getFocusedDecisionTarget();
          if (focused?.kind !== "question") {
            let targetHostId: string | null = null;
            let targetAgentId: string | null = null;
            let targetRequestId: string | null = null;
            let rawDecisions: readonly string[] = [];
            let resolveHandler: ((decision: string) => void) | undefined;

            if (focused?.kind === "permission") {
              targetHostId = focused.hostId;
              targetAgentId = focused.agentId;
              targetRequestId = focused.requestId;
              rawDecisions = focused.allowedDecisions;
              resolveHandler = focused.onResolve;
            } else {
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

              if (wantAllow && allowChoice) {
                e.preventDefault();
                dispatchDecision(allowChoice);
                return;
              }
              if (wantAlways && alwaysChoice) {
                e.preventDefault();
                dispatchDecision(alwaysChoice);
                return;
              }
              if (wantDeny && denyChoice) {
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

    window.addEventListener("keydown", captureOverlayEscape, true);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", captureOverlayEscape, true);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, []);
}
