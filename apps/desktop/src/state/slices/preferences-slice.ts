import {
  type OrbitLocale,
  resolveLocale,
  setLocale as setI18nLocale,
} from "@/lib/i18n";
import {
  type CloudSyncStatusView,
  type ExternalEditorKind,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  type ShortcutActionId,
  SHORTCUT_ACTION_MAP,
  isOsReservedChord,
  normalizeChord,
  sanitizeCustomKeybindings,
} from "@/lib/shortcut-registry";
import {
  applyDocumentTheme,
  formatCloudSyncToast,
  loadPrefs,
  markOnboardingCompleted,
  resolveSettingsSectionTarget,
  resolveTheme,
} from "../store-helpers";
import type { OrbitStoreContext } from "../store-runtime";
import type {
  InstructionsTarget,
  SettingsSection,
  SettingsSectionTarget,
  TerminalCursorStyle,
  TerminalThemeMode,
  ThemeMode,
  ToastAction,
} from "../store-types";

export function createPreferencesSlice(ctx: OrbitStoreContext) {
  const {
    getState,
    notifyToast,
    setState,
  } = ctx;

  let cloudDeviceFlowPollToken = 0;

  async function pushCloudPrefPatch(patch: Record<string, unknown>): Promise<void> {
    if (getState().cloudSyncStatus?.state !== "ready") return;
    await orbitIpc.cloudSyncUpdatePreferences(patch);
  }

  return {


    /**
     * Re-read persisted appearance & terminal typography preferences from localStorage
     * so secondary floating windows stay live-synced when Settings change in the main window.
     */
    syncAppearanceFromStorage(): void {
      const fresh = loadPrefs();
      const nextLocale = fresh.locale ? resolveLocale(fresh.locale) : getState().locale;
      if (nextLocale !== getState().locale) {
        setI18nLocale(nextLocale);
      }
      const nextTheme: ThemeMode = fresh.theme ?? getState().theme;
      const nextResolved = resolveTheme(nextTheme);
      applyDocumentTheme(nextResolved);
      const nextFontSize = fresh.terminalFontSize ?? getState().terminalFontSize;
      const nextCursor = fresh.terminalCursorStyle ?? getState().terminalCursorStyle;
      const nextScrollback = fresh.terminalScrollback ?? getState().terminalScrollback;
      const nextTerminalThemeMode = fresh.terminalThemeMode ?? getState().terminalThemeMode;
      if (
        nextLocale === getState().locale &&
        nextTheme === getState().theme &&
        nextResolved === getState().resolvedTheme &&
        nextFontSize === getState().terminalFontSize &&
        nextCursor === getState().terminalCursorStyle &&
        nextScrollback === getState().terminalScrollback &&
        nextTerminalThemeMode === getState().terminalThemeMode
      ) {
        return;
      }
      setState((s) => ({
        ...s,
        locale: nextLocale,
        theme: nextTheme,
        resolvedTheme: nextResolved,
        terminalFontSize: nextFontSize,
        terminalCursorStyle: nextCursor,
        terminalScrollback: nextScrollback,
        terminalThemeMode: nextTerminalThemeMode,
      }));
    },


    openSettings(section?: SettingsSectionTarget): void {
      void orbitIpc.listSshHosts().then((sshList) => {
        setState((s) => ({ ...s, sshHosts: sshList }));
      });
      setState((s) => ({
        ...s,
        activeView: "settings",
        settingsSection: resolveSettingsSectionTarget(s, section),
        commandPaletteOpen: false,
      }));
    },


    openProjectInstructions(target?: InstructionsTarget | null): void {
      setState((s) => ({
        ...s,
        activeView: "settings",
        settingsSection: "instructions",
        instructionsTarget: target ?? null,
        commandPaletteOpen: false,
        overviewPopoverOpen: false,
      }));
    },


    setInstructionsTarget(target: InstructionsTarget | null): void {
      setState((s) => ({ ...s, instructionsTarget: target }));
    },


    closeSettings(): void {
      setState((s) => ({ ...s, activeView: "workspace", instructionsTarget: null }));
    },


    toggleSettings(section?: SettingsSectionTarget): void {
      const resolved = section ? resolveSettingsSectionTarget(getState(), section) : null;
      if (getState().activeView === "settings" && (!resolved || getState().settingsSection === resolved)) {
        ctx.actions.closeSettings();
      } else {
        ctx.actions.openSettings(section);
      }
    },


    setSettingsSection(section: SettingsSection): void {
      setState((s) => ({ ...s, settingsSection: section }));
    },


    setCommandPaletteOpen(open: boolean): void {
      setState((s) => ({ ...s, commandPaletteOpen: open }));
    },


    openOnboardingGuide(): void {
      setState((s) => ({
        ...s,
        activeView: "workspace",
        commandPaletteOpen: false,
        onboardingOpen: true,
      }));
    },


    closeOnboardingGuide(markCompleted = true): void {
      if (markCompleted) {
        markOnboardingCompleted();
      }
      setState((s) => (s.onboardingOpen ? { ...s, onboardingOpen: false } : s));
    },


    setPairDialogOpen(open: boolean): void {
      if (open) {
        ctx.actions.openSettings("add_host");
      } else {
        setState((s) => ({ ...s, pairDialogOpen: false }));
      }
    },


    async openRelayDialog(): Promise<void> {
      ctx.actions.openSettings("hosts");
    },


    setRelayDialogOpen(open: boolean): void {
      setState((s) => ({ ...s, relayDialogOpen: open }));
    },


    setLocale(locale: OrbitLocale): void {
      const resolved = resolveLocale(locale);
      setI18nLocale(resolved);
      setState((s) => ({ ...s, locale: resolved }), true);
      void pushCloudPrefPatch({ locale: resolved });
    },


    setTheme(theme: ThemeMode): void {
      const resolved = resolveTheme(theme);
      applyDocumentTheme(resolved);
      setState((s) => ({ ...s, theme, resolvedTheme: resolved }), true);
      void pushCloudPrefPatch({ theme });
    },


    cycleTheme(): void {
      const order: ThemeMode[] = ["system", "light", "dark"];
      const next = order[(order.indexOf(getState().theme) + 1) % order.length];
      ctx.actions.setTheme(next);
    },


    setEnterToSend(enabled: boolean): void {
      setState((s) => ({ ...s, enterToSend: enabled }), true);
      void pushCloudPrefPatch({ enterToSend: enabled });
    },


    setCustomKeybinding(actionId: ShortcutActionId, chord: string | null): void {
      const def = SHORTCUT_ACTION_MAP[actionId];
      if (!def) return;
      let nextBindings: Record<string, string> = {};
      setState((s) => {
        const next = { ...s.customKeybindings };
        if (!chord) {
          delete next[actionId];
        } else {
          const norm = normalizeChord(chord);
          if (!norm || isOsReservedChord(norm)) return s;
          if (norm === def.defaultChord) {
            delete next[actionId];
          } else {
            next[actionId] = norm;
          }
        }
        nextBindings = next;
        return { ...s, customKeybindings: next };
      }, true);
      void pushCloudPrefPatch({ customKeybindings: nextBindings });
    },


    setCustomKeybindings(next: Record<string, string>): void {
      const sanitized = sanitizeCustomKeybindings(next);
      setState((s) => ({ ...s, customKeybindings: sanitized }), true);
      void pushCloudPrefPatch({ customKeybindings: sanitized });
    },


    resetCustomKeybinding(actionId: ShortcutActionId): void {
      let nextBindings: Record<string, string> = {};
      setState((s) => {
        if (!(actionId in s.customKeybindings)) {
          nextBindings = s.customKeybindings;
          return s;
        }
        const next = { ...s.customKeybindings };
        delete next[actionId];
        nextBindings = next;
        return { ...s, customKeybindings: next };
      }, true);
      void pushCloudPrefPatch({ customKeybindings: nextBindings });
    },


    resetAllCustomKeybindings(): void {
      setState((s) => ({ ...s, customKeybindings: {} }), true);
      void pushCloudPrefPatch({ customKeybindings: {} });
    },


    setNotificationPrefs(patch: {
      notifyOnApproval?: boolean;
      notifyOnTurnComplete?: boolean;
      notifyOnAgentError?: boolean;
      notifyOnGoalFinished?: boolean;
      notifySoundEnabled?: boolean;
      notifyQuietHoursEnabled?: boolean;
      notifyQuietStartHour?: number;
      notifyQuietEndHour?: number;
    }): void {
      setState(
        (s) => ({
          ...s,
          notifyOnApproval: patch.notifyOnApproval ?? s.notifyOnApproval,
          notifyOnTurnComplete: patch.notifyOnTurnComplete ?? s.notifyOnTurnComplete,
          notifyOnAgentError: patch.notifyOnAgentError ?? s.notifyOnAgentError,
          notifyOnGoalFinished: patch.notifyOnGoalFinished ?? s.notifyOnGoalFinished,
          notifySoundEnabled: patch.notifySoundEnabled ?? s.notifySoundEnabled,
          notifyQuietHoursEnabled: patch.notifyQuietHoursEnabled ?? s.notifyQuietHoursEnabled,
          notifyQuietStartHour:
            patch.notifyQuietStartHour !== undefined
              ? Math.min(23, Math.max(0, Math.round(patch.notifyQuietStartHour)))
              : s.notifyQuietStartHour,
          notifyQuietEndHour:
            patch.notifyQuietEndHour !== undefined
              ? Math.min(23, Math.max(0, Math.round(patch.notifyQuietEndHour)))
              : s.notifyQuietEndHour,
        }),
        true,
      );
      void pushCloudPrefPatch(patch);
    },


    setWorktreePrefs(patch: {
      worktreeBranchPrefix?: string;
      coalesceExploration?: boolean;
    }): void {
      setState(
        (s) => ({
          ...s,
          worktreeBranchPrefix: patch.worktreeBranchPrefix ?? s.worktreeBranchPrefix,
          coalesceExploration: patch.coalesceExploration ?? s.coalesceExploration,
        }),
        true,
      );
      void pushCloudPrefPatch(patch);
    },


    setPreferredEditor(editor: ExternalEditorKind | null): void {
      setState((s) => ({ ...s, preferredEditor: editor }), true);
    },


    setTerminalPrefs(patch: {
      fontSize?: number;
      cursorStyle?: TerminalCursorStyle;
      scrollback?: number;
      themeMode?: TerminalThemeMode;
    }): void {
      setState(
        (s) => ({
          ...s,
          terminalFontSize:
            patch.fontSize !== undefined
              ? Math.min(18, Math.max(10, patch.fontSize))
              : s.terminalFontSize,
          terminalCursorStyle: patch.cursorStyle ?? s.terminalCursorStyle,
          terminalScrollback:
            patch.scrollback !== undefined
              ? Math.min(50000, Math.max(500, patch.scrollback))
              : s.terminalScrollback,
          terminalThemeMode: patch.themeMode ?? s.terminalThemeMode,
        }),
        true,
      );
    },


    showToast(msg: string, action?: ToastAction | null): void {
      notifyToast(msg, null, action ?? null);
    },


    // --- Zero-Server E2EE Cloud Sync Actions ---

    async refreshCloudSyncStatus(): Promise<CloudSyncStatusView | null> {
      try {
        const status = await orbitIpc.getCloudSyncStatus();
        setState((s) => ({ ...s, cloudSyncStatus: status }));
        return status;
      } catch {
        return null;
      }
    },


    async hydrateSyncedPreferencesFromCore(): Promise<void> {
      try {
        const prefs = await orbitIpc.cloudSyncGetPreferences();
        if (!prefs || Object.keys(prefs).length === 0) return;
        let nextLocale: OrbitLocale | null = null;
        if (prefs.locale === "en" || prefs.locale === "zh-CN" || prefs.locale === "ja") {
          nextLocale = prefs.locale;
        }
        if (nextLocale) {
          setI18nLocale(nextLocale);
        }
        let nextTheme: ThemeMode | null = null;
        if (prefs.theme === "system" || prefs.theme === "light" || prefs.theme === "dark") {
          nextTheme = prefs.theme;
        }
        if (nextTheme) {
          applyDocumentTheme(resolveTheme(nextTheme));
        }
        setState(
          (s) => ({
            ...s,
            locale: nextLocale ?? s.locale,
            theme: nextTheme ?? s.theme,
            resolvedTheme: nextTheme ? resolveTheme(nextTheme) : s.resolvedTheme,
            enterToSend: typeof prefs.enterToSend === "boolean" ? prefs.enterToSend : s.enterToSend,
            notifyOnApproval:
              typeof prefs.notifyOnApproval === "boolean"
                ? prefs.notifyOnApproval
                : s.notifyOnApproval,
            notifyOnTurnComplete:
              typeof prefs.notifyOnTurnComplete === "boolean"
                ? prefs.notifyOnTurnComplete
                : s.notifyOnTurnComplete,
            notifyOnAgentError:
              typeof prefs.notifyOnAgentError === "boolean"
                ? prefs.notifyOnAgentError
                : s.notifyOnAgentError,
            notifyOnGoalFinished:
              typeof prefs.notifyOnGoalFinished === "boolean"
                ? prefs.notifyOnGoalFinished
                : s.notifyOnGoalFinished,
            notifySoundEnabled:
              typeof prefs.notifySoundEnabled === "boolean"
                ? prefs.notifySoundEnabled
                : s.notifySoundEnabled,
            notifyQuietHoursEnabled:
              typeof prefs.notifyQuietHoursEnabled === "boolean"
                ? prefs.notifyQuietHoursEnabled
                : s.notifyQuietHoursEnabled,
            notifyQuietStartHour:
              typeof prefs.notifyQuietStartHour === "number"
                ? Math.min(23, Math.max(0, Math.round(prefs.notifyQuietStartHour)))
                : s.notifyQuietStartHour,
            notifyQuietEndHour:
              typeof prefs.notifyQuietEndHour === "number"
                ? Math.min(23, Math.max(0, Math.round(prefs.notifyQuietEndHour)))
                : s.notifyQuietEndHour,
            coalesceExploration:
              typeof prefs.coalesceExploration === "boolean"
                ? prefs.coalesceExploration
                : s.coalesceExploration,
            worktreeBranchPrefix:
              typeof prefs.worktreeBranchPrefix === "string"
                ? prefs.worktreeBranchPrefix
                : s.worktreeBranchPrefix,
            draftUseWorktree:
              typeof prefs.draftUseWorktree === "boolean"
                ? prefs.draftUseWorktree
                : s.draftUseWorktree,
            draftMode:
              prefs.draftMode === "default" ||
              prefs.draftMode === "auto" ||
              prefs.draftMode === "plan" ||
              prefs.draftMode === "accept_edits" ||
              prefs.draftMode === "full_access"
                ? prefs.draftMode
                : s.draftMode,
            customKeybindings:
              prefs.customKeybindings && typeof prefs.customKeybindings === "object"
                ? sanitizeCustomKeybindings(prefs.customKeybindings)
                : s.customKeybindings,
          }),
          true,
        );
      } catch {
        // Ignore preference hydration errors
      }
    },


    async startGithubCloudLogin(clientIdOverride?: string | null): Promise<void> {
      cloudDeviceFlowPollToken += 1;
      const pollToken = cloudDeviceFlowPollToken;
      setState((s) => ({ ...s, cloudSyncBusy: true, cloudSyncError: null }));
      try {
        const started = await orbitIpc.cloudSyncStartGithubDeviceFlow(clientIdOverride ?? null);
        if (pollToken !== cloudDeviceFlowPollToken) return;
        setState((s) => ({
          ...s,
          cloudSyncBusy: false,
          cloudSyncDeviceFlow: started,
          cloudSyncError: null,
        }));

        // Poll GitHub Device Flow until authorized, cancelled, or expired
        let intervalMs = Math.max(2000, started.interval_secs * 1000);
        const deadline = Date.now() + started.expires_in_secs * 1000;
        while (pollToken === cloudDeviceFlowPollToken && Date.now() < deadline) {
          await new Promise((r) => window.setTimeout(r, intervalMs));
          if (pollToken !== cloudDeviceFlowPollToken) return;
          try {
            const polled = await orbitIpc.cloudSyncPollGithubDeviceFlow(
              started.device_code,
              clientIdOverride ?? null,
            );
            if (pollToken !== cloudDeviceFlowPollToken) return;
            if (polled.status === "pending") {
              continue;
            }
            if (polled.status === "slow_down") {
              intervalMs = Math.max(intervalMs + 5000, (polled.interval_secs ?? 10) * 1000);
              continue;
            }
            if (polled.status === "authorized") {
              const status = polled.sync_status ?? (await orbitIpc.getCloudSyncStatus());
              setState((s) => ({
                ...s,
                cloudSyncDeviceFlow: null,
                cloudSyncStatus: status,
                cloudSyncError: null,
              }));
              notifyToast(
                status.account_login
                  ? `Signed in to GitHub as @${status.account_login}`
                  : "Signed in to GitHub",
              );
              return;
            }
            setState((s) => ({
              ...s,
              cloudSyncDeviceFlow: null,
              cloudSyncError: polled.error ?? "GitHub authorization did not complete.",
            }));
            return;
          } catch (err) {
            if (pollToken !== cloudDeviceFlowPollToken) return;
            setState((s) => ({
              ...s,
              cloudSyncDeviceFlow: null,
              cloudSyncError: String(err),
            }));
            return;
          }
        }
        if (pollToken === cloudDeviceFlowPollToken) {
          setState((s) => ({
            ...s,
            cloudSyncDeviceFlow: null,
            cloudSyncError: "GitHub verification code expired — please start again.",
          }));
        }
      } catch (err) {
        if (pollToken !== cloudDeviceFlowPollToken) return;
        setState((s) => ({
          ...s,
          cloudSyncBusy: false,
          cloudSyncDeviceFlow: null,
          cloudSyncError: String(err),
        }));
      }
    },


    cancelGithubCloudLogin(): void {
      cloudDeviceFlowPollToken += 1;
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncDeviceFlow: null,
        cloudSyncError: null,
      }));
    },


    async loginCloudSyncWithToken(token: string): Promise<boolean> {
      setState((s) => ({ ...s, cloudSyncBusy: true, cloudSyncError: null }));
      try {
        const status = await orbitIpc.cloudSyncLoginWithToken(token.trim());
        setState((s) => ({
          ...s,
          cloudSyncBusy: false,
          cloudSyncStatus: status,
          cloudSyncError: null,
          cloudSyncDeviceFlow: null,
        }));
        notifyToast(
          status.account_login
            ? `Signed in to GitHub as @${status.account_login}`
            : "Signed in to GitHub",
        );
        return true;
      } catch (err) {
        setState((s) => ({
          ...s,
          cloudSyncBusy: false,
          cloudSyncError: String(err),
        }));
        return false;
      }
    },


    async unlockCloudSyncWithPassphrase(passphrase: string): Promise<boolean> {
      setState((s) => ({ ...s, cloudSyncBusy: true, cloudSyncError: null }));
      try {
        // Seed current local syncable prefs before first unlock so a brand-new vault captures them
        await orbitIpc.cloudSyncUpdatePreferences({
          locale: getState().locale,
          theme: getState().theme,
          enterToSend: getState().enterToSend,
          notifyOnApproval: getState().notifyOnApproval,
          notifyOnTurnComplete: getState().notifyOnTurnComplete,
          notifyOnAgentError: getState().notifyOnAgentError,
          notifyOnGoalFinished: getState().notifyOnGoalFinished,
          notifySoundEnabled: getState().notifySoundEnabled,
          notifyQuietHoursEnabled: getState().notifyQuietHoursEnabled,
          notifyQuietStartHour: getState().notifyQuietStartHour,
          notifyQuietEndHour: getState().notifyQuietEndHour,
          coalesceExploration: getState().coalesceExploration,
          worktreeBranchPrefix: getState().worktreeBranchPrefix,
          draftUseWorktree: getState().draftUseWorktree,
          draftMode: getState().draftMode,
        });
        const result = await orbitIpc.cloudSyncUnlockWithPassphrase(passphrase);
        const status = await orbitIpc.getCloudSyncStatus();
        setState((s) => ({
          ...s,
          cloudSyncBusy: false,
          cloudSyncStatus: status,
          cloudSyncLastResult: result,
          cloudSyncError: null,
        }));
        await ctx.actions.reloadFleetAndHosts();
        await ctx.actions.hydrateSyncedPreferencesFromCore();
        notifyToast(formatCloudSyncToast(result));
        return true;
      } catch (err) {
        const status = await orbitIpc.getCloudSyncStatus().catch(() => getState().cloudSyncStatus);
        setState((s) => ({
          ...s,
          cloudSyncBusy: false,
          cloudSyncStatus: status,
          cloudSyncError: String(err),
        }));
        return false;
      }
    },


    async syncCloudNow(): Promise<boolean> {
      setState((s) => ({ ...s, cloudSyncBusy: true, cloudSyncError: null }));
      try {
        const result = await orbitIpc.cloudSyncNow();
        const status = await orbitIpc.getCloudSyncStatus();
        setState((s) => ({
          ...s,
          cloudSyncBusy: false,
          cloudSyncStatus: status,
          cloudSyncLastResult: result,
          cloudSyncError: null,
        }));
        await ctx.actions.reloadFleetAndHosts();
        await ctx.actions.hydrateSyncedPreferencesFromCore();
        notifyToast(formatCloudSyncToast(result));
        return true;
      } catch (err) {
        const status = await orbitIpc.getCloudSyncStatus().catch(() => getState().cloudSyncStatus);
        setState((s) => ({
          ...s,
          cloudSyncBusy: false,
          cloudSyncStatus: status,
          cloudSyncError: String(err),
        }));
        return false;
      }
    },


    async setCloudAutoSync(enabled: boolean): Promise<void> {
      try {
        const status = await orbitIpc.cloudSyncSetAutoSync(enabled);
        setState((s) => ({ ...s, cloudSyncStatus: status }));
      } catch (err) {
        notifyToast(`Failed to update auto-sync: ${String(err)}`);
      }
    },


    async setCloudGithubClientId(clientId: string | null): Promise<void> {
      try {
        const status = await orbitIpc.cloudSyncSetGithubClientId(clientId);
        setState((s) => ({ ...s, cloudSyncStatus: status, cloudSyncError: null }));
      } catch (err) {
        setState((s) => ({ ...s, cloudSyncError: String(err) }));
      }
    },


    async logoutCloudSync(clearLocalHosts = false): Promise<void> {
      cloudDeviceFlowPollToken += 1;
      setState((s) => ({ ...s, cloudSyncBusy: true, cloudSyncError: null }));
      try {
        const status = await orbitIpc.cloudSyncLogout(clearLocalHosts);
        setState((s) => ({
          ...s,
          cloudSyncBusy: false,
          cloudSyncStatus: status,
          cloudSyncDeviceFlow: null,
          cloudSyncLastResult: null,
          cloudSyncError: null,
        }));
        if (clearLocalHosts) {
          await ctx.actions.reloadFleetAndHosts();
        }
        notifyToast("Signed out of Cloud Sync");
      } catch (err) {
        setState((s) => ({
          ...s,
          cloudSyncBusy: false,
          cloudSyncError: String(err),
        }));
      }
    },
  };
}
