import { useEffect, useMemo, useRef } from "react";
import {
  AlertCircle,
  ArrowUpCircle,
  Check,
  Compass,
  Download,
  Laptop,
  Loader2,
  RefreshCw,
  RotateCcw,
  Server,
} from "lucide-react";
import {
  HostStatusIndicator,
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import {
  type ExternalEditorKind,
  type HostCardView,
  compareSemver,
  normalizeSemver,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import {
  orbitActions,
  resolveFleetTargetVersion,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type PresetPermissionMode = "default" | "auto" | "accept_edits" | "full_access";

function orderHostsLocalFirst(hosts: HostCardView[]): HostCardView[] {
  const local: HostCardView[] = [];
  const remote: HostCardView[] = [];
  for (const h of hosts) {
    if (isLocalHostCard(h)) local.push(h);
    else remote.push(h);
  }
  return [...local, ...remote];
}

export function GeneralSection() {
  const { locale, t } = useI18n();
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const draftMode = useOrbitSelector((s) => s.draftMode);
  const draftUseWorktree = useOrbitSelector((s) => s.draftUseWorktree);
  const worktreeBranchPrefix = useOrbitSelector((s) => s.worktreeBranchPrefix);
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const externalEditors = useOrbitSelector((s) => s.externalEditors);
  const preferredEditor = useOrbitSelector((s) => s.preferredEditor);
  const storeAppVersion = useOrbitSelector((s) => s.appVersion);
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostUpgradeStates = useOrbitSelector((s) => s.hostUpgradeStates);
  const releaseUpdateInfo = useOrbitSelector((s) => s.releaseUpdateInfo);
  const checkingUpdate = useOrbitSelector((s) => s.checkingReleaseUpdate);
  const releaseCheckError = useOrbitSelector((s) => s.releaseCheckError);
  const fleetUpdate = useOrbitSelector((s) => s.fleetUpdate);
  const isBrowserOffline = useOrbitSelector((s) => s.isBrowserOffline);
  const mod = modSymbol();

  const prefixOptions = useMemo<SettingSelectOption<string>[]>(
    () => [
      { value: "auto", label: t("composer.modeAuto"), description: t("settings.branchPrefixAutoDesc") },
      { value: "orbit/", label: "orbit/", mono: true },
      { value: "feat/", label: "feat/", mono: true },
      { value: "fix/", label: "fix/", mono: true },
      { value: "chore/", label: "chore/", mono: true },
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [locale],
  );

  const permissionOptions = useMemo<SettingSelectOption<PresetPermissionMode>[]>(
    () => [
      {
        value: "default",
        label: t("composer.modeAlwaysAsk"),
        description: t("composer.modeAlwaysAskShortDesc"),
      },
      {
        value: "auto",
        label: t("composer.modeAuto"),
        description: t("composer.modeAutoShortDesc"),
      },
      {
        value: "accept_edits",
        label: t("composer.modeAcceptEdits"),
        description: t("composer.modeAcceptEditsShortDesc"),
      },
      {
        value: "full_access",
        label: t("composer.modeFullAccess"),
        description: t("composer.modeFullAccessShortDesc"),
      },
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [locale],
  );

  const hostTransportBadge = (host: HostCardView): string => {
    if (isLocalHostCard(host)) return t("common.localService");
    if (host.ssh_alias) return `SSH · ${host.ssh_alias}`;
    if (host.connection_state === "online_relay") return t("common.relay");
    if (host.connection_state === "online_direct") return t("common.direct");
    return t("common.offline");
  };

  const appVersion = normalizeSemver(storeAppVersion) ?? "0.9.9";
  const autoCheckedRef = useRef(false);

  // Gently check for the latest release once when opening General settings
  useEffect(() => {
    if (autoCheckedRef.current || releaseUpdateInfo || checkingUpdate || isBrowserOffline) return;
    autoCheckedRef.current = true;
    void orbitActions.checkFleetUpdates({ silent: true });
  }, [releaseUpdateInfo, checkingUpdate, isBrowserOffline]);

  const activePermissionMode: PresetPermissionMode =
    draftMode === "auto" || draftMode === "accept_edits" || draftMode === "full_access"
      ? draftMode
      : "default";

  const activePrefix = prefixOptions.some((o) => o.value === worktreeBranchPrefix)
    ? worktreeBranchPrefix
    : "auto";

  const installedEditors = useMemo(
    () => externalEditors.filter((ed) => ed.installed),
    [externalEditors],
  );

  const effectivePreferred: ExternalEditorKind | null =
    (preferredEditor && installedEditors.some((ed) => ed.kind === preferredEditor)
      ? preferredEditor
      : installedEditors[0]?.kind) ?? null;

  const editorOptions = useMemo<SettingSelectOption<ExternalEditorKind>[]>(
    () =>
      installedEditors.map((ed) => ({
        value: ed.kind,
        label: ed.label,
      })),
    [installedEditors],
  );

  const orderedHosts = useMemo(() => orderHostsLocalFirst(fleet.hosts), [fleet.hosts]);
  const targetVersion = useMemo(
    () => resolveFleetTargetVersion(appVersion, releaseUpdateInfo),
    [appVersion, releaseUpdateInfo],
  );

  const fleetSummary = useMemo(() => {
    const desktopOutdated = Boolean(releaseUpdateInfo?.has_newer_version);
    let reachableHosts = 0;
    let outdatedReachableHosts = 0;
    let offlineHosts = 0;
    let failedHosts = 0;

    for (const host of orderedHosts) {
      const isLocal = isLocalHostCard(host);
      const isOnline =
        host.connection_state === "online_direct" ||
        host.connection_state === "online_relay";
      const reachable = isLocal || isOnline || Boolean(host.ssh_alias);
      if (reachable) reachableHosts += 1;
      else offlineHosts += 1;

      const normDaemon = normalizeSemver(host.daemon_version);
      const outdated = Boolean(
        reachable &&
          targetVersion &&
          (!normDaemon || compareSemver(normDaemon, targetVersion) < 0),
      );
      if (outdated) outdatedReachableHosts += 1;
      if (hostUpgradeStates[host.host_id]?.status === "failed") failedHosts += 1;
    }

    // When Desktop updates, its bundled sidecar updates local daemon in the same restart,
    // so avoid double-counting local daemon alongside Desktop when computing pending items.
    const localOutdated = orderedHosts.some((h) => {
      if (!isLocalHostCard(h)) return false;
      const v = normalizeSemver(h.daemon_version);
      return Boolean(targetVersion && (!v || compareSemver(v, targetVersion) < 0));
    });
    const remoteOutdatedCount =
      desktopOutdated && localOutdated
        ? Math.max(0, outdatedReachableHosts - 1)
        : outdatedReachableHosts;
    const totalPendingUpdates = (desktopOutdated ? 1 : 0) + remoteOutdatedCount;

    return {
      desktopOutdated,
      reachableHosts,
      outdatedReachableHosts,
      offlineHosts,
      failedHosts,
      totalPendingUpdates,
    };
  }, [orderedHosts, releaseUpdateInfo, targetVersion, hostUpgradeStates]);

  const isFleetBusy =
    fleetUpdate.phase === "checking" ||
    fleetUpdate.phase === "upgrading_hosts" ||
    fleetUpdate.phase === "upgrading_desktop";

  const statusDescription = useMemo(() => {
    if (fleetUpdate.phase === "upgrading_desktop" && fleetUpdate.desktopStatus) {
      return fleetUpdate.desktopStatus;
    }
    if (isFleetBusy && fleetUpdate.summaryMessage) {
      return fleetUpdate.summaryMessage;
    }
    if (fleetUpdate.phase === "paused_with_failures" && fleetUpdate.summaryMessage) {
      return fleetUpdate.summaryMessage;
    }
    if (fleetUpdate.desktopError) {
      return fleetUpdate.desktopError;
    }
    if (releaseCheckError) {
      return releaseCheckError;
    }
    if (fleetUpdate.phase === "completed" && fleetUpdate.summaryMessage) {
      return fleetUpdate.summaryMessage;
    }
    if (releaseUpdateInfo) {
      if (fleetSummary.totalPendingUpdates > 0 && targetVersion) {
        return t("settings.updatesAvailableDesc", {
          count: fleetSummary.totalPendingUpdates,
          version: targetVersion,
        });
      }
      const ver = targetVersion ?? appVersion;
      return fleetSummary.offlineHosts > 0
        ? t("settings.allUpToDateWithOfflineDesc", {
            reachable: fleetSummary.reachableHosts,
            version: ver,
            offline: fleetSummary.offlineHosts,
          })
        : t("settings.allUpToDateDesc", {
            reachable: fleetSummary.reachableHosts,
            version: ver,
          });
    }
    return t("settings.updatesDefaultDesc", { count: orderedHosts.length });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    locale,
    fleetUpdate,
    isFleetBusy,
    releaseCheckError,
    releaseUpdateInfo,
    fleetSummary,
    targetVersion,
    appVersion,
    orderedHosts.length,
  ]);

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title={t("settings.generalTitle")}
        description={t("settings.generalDesc")}
      />

      {/* Workspace */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupWorkspace")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title={t("settings.sendWithEnter")}
            description={
              enterToSend
                ? t("settings.sendWithEnterOnDesc")
                : t("settings.sendWithEnterOffDesc", { mod })
            }
          >
            <ToggleSwitch
              checked={enterToSend}
              onChange={(v) => orbitActions.setEnterToSend(v)}
            />
          </SettingRow>

          <SettingRow
            title={t("settings.defaultPermissions")}
            description={t("settings.defaultPermissionsDesc")}
          >
            <SettingSelect
              value={activePermissionMode}
              options={permissionOptions}
              onChange={(mode) => orbitActions.setDraftConfig({ mode })}
              menuClassName="w-[260px]"
            />
          </SettingRow>

          <SettingRow
            title={t("settings.collapseExploration")}
            description={t("settings.collapseExplorationDesc")}
          >
            <ToggleSwitch
              checked={coalesceExploration}
              onChange={(v) => orbitActions.setWorktreePrefs({ coalesceExploration: v })}
            />
          </SettingRow>

          <SettingRow
            title={t("settings.externalEditor")}
            description={t("settings.externalEditorDesc", { mod })}
          >
            <SettingSelect
              value={effectivePreferred}
              options={editorOptions}
              onChange={(kind) => orbitActions.setPreferredEditor(kind)}
              placeholder={t("settings.noEditorsFound")}
              disabled={editorOptions.length === 0}
            />
          </SettingRow>
        </SettingCard>
      </div>

      {/* Git & Worktrees */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupGitWorktrees")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title={t("settings.isolateWorktrees")}
            description={t("settings.isolateWorktreesDesc")}
          >
            <ToggleSwitch
              checked={draftUseWorktree}
              onChange={(v) => orbitActions.setDraftConfig({ useWorktree: v })}
            />
          </SettingRow>

          <div className={cn(!draftUseWorktree && "opacity-55 transition-opacity")}>
            <SettingRow
              title={t("settings.branchPrefix")}
              description={t("settings.branchPrefixDesc")}
            >
              <SettingSelect
                value={activePrefix}
                options={prefixOptions}
                disabled={!draftUseWorktree}
                onChange={(pfx) => orbitActions.setWorktreePrefs({ worktreeBranchPrefix: pfx })}
                menuClassName="w-[200px]"
              />
            </SettingRow>
          </div>
        </SettingCard>
      </div>

      {/* Fleet Software Updates (Local Desktop + All Connected Hosts) */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupUpdatesFleet")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title={
              <span className="inline-flex flex-wrap items-center gap-2">
                <span>Orbit v{appVersion}</span>
                {releaseUpdateInfo && targetVersion && (
                  <span
                    className={cn(
                      "rounded-full px-2 py-0.5 font-mono text-[11px] font-medium",
                      fleetSummary.totalPendingUpdates > 0
                        ? "bg-sky-500/12 text-sky-600 dark:text-sky-400"
                        : "bg-emerald-500/12 text-emerald-600 dark:text-emerald-400",
                    )}
                  >
                    {fleetSummary.totalPendingUpdates > 0
                      ? t("settings.latestBadge", { version: targetVersion })
                      : t("settings.allCurrentBadge", { version: targetVersion })}
                  </span>
                )}
              </span>
            }
            description={statusDescription}
          >
            <div className="flex flex-wrap items-center justify-end gap-1.5">
              {fleetUpdate.phase === "paused_with_failures" ? (
                <>
                  <button
                    type="button"
                    onClick={() => void orbitActions.retryFailedFleetHosts()}
                    className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors"
                  >
                    <RotateCcw className="h-[13px] w-[13px]" />
                    <span>{t("settings.retryFailedHosts")}</span>
                  </button>
                  {fleetUpdate.pendingDesktopUpdate && releaseUpdateInfo && (
                    <button
                      type="button"
                      onClick={() =>
                        void orbitActions.installDesktopAppUpdate(releaseUpdateInfo)
                      }
                      className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12px] font-medium transition-opacity hover:opacity-90"
                    >
                      <Download className="h-[13px] w-[13px]" />
                      <span>{t("settings.continueDesktopUpdate")}</span>
                    </button>
                  )}
                </>
              ) : (
                <>
                  <button
                    type="button"
                    disabled={checkingUpdate || isFleetBusy || isBrowserOffline}
                    onClick={() => void orbitActions.checkFleetUpdates()}
                    className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
                  >
                    {checkingUpdate ? (
                      <Loader2 className="h-[13px] w-[13px] animate-spin" />
                    ) : (
                      <RefreshCw className="h-[13px] w-[13px]" />
                    )}
                    <span>
                      {checkingUpdate ? t("settings.checking") : t("settings.checkForUpdates")}
                    </span>
                  </button>

                  {fleetSummary.totalPendingUpdates > 0 || !releaseUpdateInfo ? (
                    <button
                      type="button"
                      disabled={isFleetBusy || checkingUpdate || isBrowserOffline}
                      onClick={() => void orbitActions.upgradeAllConnectedHosts()}
                      className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12px] font-medium transition-opacity hover:opacity-90 disabled:opacity-50"
                    >
                      {isFleetBusy ? (
                        <Loader2 className="h-[13.5px] w-[13.5px] animate-spin" />
                      ) : (
                        <ArrowUpCircle className="h-[13.5px] w-[13.5px]" />
                      )}
                      <span>
                        {isFleetBusy
                          ? t("common.updating")
                          : fleetSummary.totalPendingUpdates > 0
                            ? t("settings.updateAllCount", {
                                count: fleetSummary.totalPendingUpdates,
                              })
                            : t("settings.updateAll")}
                      </span>
                    </button>
                  ) : (
                    <button
                      type="button"
                      disabled={isFleetBusy || checkingUpdate || isBrowserOffline}
                      onClick={() =>
                        void orbitActions.upgradeAllConnectedHosts({ force: true })
                      }
                      title={t("settings.resyncAllTitle")}
                      className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
                    >
                      {isFleetBusy ? (
                        <Loader2 className="h-[13px] w-[13px] animate-spin" />
                      ) : (
                        <ArrowUpCircle className="h-[13px] w-[13px]" />
                      )}
                      <span>
                        {isFleetBusy ? t("common.syncing") : t("settings.resyncAll")}
                      </span>
                    </button>
                  )}
                </>
              )}
            </div>
          </SettingRow>

          {/* Compact Fleet Version Matrix: Desktop + Every Connected Machine */}
          <div className="divide-border/70 divide-y px-4 py-1">
            {/* Local Desktop App Row */}
            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="flex min-w-0 items-center gap-2.5">
                <Laptop className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
                <div className="min-w-0">
                  <div className="flex items-center gap-1.5">
                    <span className="text-foreground truncate text-[12.5px] font-medium">
                      Orbit Desktop
                    </span>
                    <span className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-[10.5px]">
                      {t("settings.localClient")}
                    </span>
                  </div>
                  {fleetUpdate.phase === "upgrading_desktop" &&
                    fleetUpdate.desktopProgress !== null && (
                      <div className="mt-1.5 flex items-center gap-2">
                        <div className="bg-muted h-1.5 w-36 overflow-hidden rounded-full">
                          <div
                            className="bg-sky-500 h-full rounded-full transition-all duration-200"
                            style={{
                              width: `${Math.max(4, Math.round(fleetUpdate.desktopProgress * 100))}%`,
                            }}
                          />
                        </div>
                        <span className="text-muted-foreground font-mono text-[11px] tabular-nums">
                          {Math.round(fleetUpdate.desktopProgress * 100)}%
                        </span>
                      </div>
                    )}
                </div>
              </div>

              <div className="flex shrink-0 items-center gap-2 text-[12px]">
                <span className="text-muted-foreground font-mono">v{appVersion}</span>
                {fleetUpdate.phase === "upgrading_desktop" ? (
                  <span className="inline-flex items-center gap-1 text-sky-600 dark:text-sky-400">
                    <Loader2 className="h-[12px] w-[12px] animate-spin" />
                    <span>{t("settings.installing")}</span>
                  </span>
                ) : fleetSummary.desktopOutdated && targetVersion ? (
                  <span className="rounded bg-sky-500/12 px-1.5 py-0.5 font-mono text-[11px] font-medium text-sky-600 dark:text-sky-400">
                    → v{targetVersion}
                  </span>
                ) : releaseUpdateInfo ? (
                  <span className="inline-flex items-center gap-1 text-[11.5px] text-emerald-600 dark:text-emerald-400">
                    <Check className="h-[12.5px] w-[12.5px]" />
                    <span>{t("common.upToDate")}</span>
                  </span>
                ) : null}
              </div>
            </div>

            {/* Connected Host Daemons (Local + Remote) */}
            {orderedHosts.map((host) => {
              const isLocal = isLocalHostCard(host);
              const isOnline =
                host.connection_state === "online_direct" ||
                host.connection_state === "online_relay";
              const reachable = isLocal || isOnline || Boolean(host.ssh_alias);
              const normDaemon = normalizeSemver(host.daemon_version);
              const upState = hostUpgradeStates[host.host_id] ?? null;
              const isUpgrading = upState?.status === "upgrading";
              const isFailed = upState?.status === "failed";
              const isSkipped = upState?.status === "skipped";
              const isDone = upState?.status === "done";
              const isOutdated = Boolean(
                targetVersion && (!normDaemon || compareSemver(normDaemon, targetVersion) < 0),
              );

              return (
                <div
                  key={host.host_id}
                  className="group/hostrow flex items-center justify-between gap-3 py-2.5"
                >
                  <div className="flex min-w-0 items-center gap-2.5">
                    <Server className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
                    <div className="min-w-0">
                      <div className="flex items-center gap-1.5">
                        <HostStatusIndicator state={host.connection_state} />
                        <span className="text-foreground truncate text-[12.5px] font-medium">
                          {host.display_name}
                        </span>
                        <span className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10.5px]">
                          {hostTransportBadge(host)}
                        </span>
                      </div>
                      {isFailed && upState?.error && (
                        <div className="mt-0.5 truncate text-[11px] text-rose-600 dark:text-rose-400">
                          {upState.error}
                        </div>
                      )}
                    </div>
                  </div>

                  <div className="flex shrink-0 items-center gap-2 text-[12px]">
                    <span className="text-muted-foreground font-mono">
                      {normDaemon ? `v${normDaemon}` : t("common.unknownVersion")}
                    </span>

                    {isUpgrading ? (
                      <span className="inline-flex items-center gap-1 text-[11.5px] text-sky-600 dark:text-sky-400">
                        <Loader2 className="h-[12px] w-[12px] animate-spin" />
                        <span>{t("common.updating")}</span>
                      </span>
                    ) : isFailed ? (
                      <button
                        type="button"
                        onClick={() =>
                          void orbitActions.upgradeHostDaemon(host.host_id, {
                            force: true,
                            targetVersion,
                          })
                        }
                        className="inline-flex cursor-pointer items-center gap-1 rounded-md bg-rose-500/12 px-2 py-0.5 text-[11.5px] font-medium text-rose-600 transition-colors hover:bg-rose-500/20 dark:text-rose-400"
                      >
                        <AlertCircle className="h-[12px] w-[12px]" />
                        <span>{t("common.retry")}</span>
                      </button>
                    ) : !reachable || isSkipped ? (
                      <span className="text-muted-foreground/80 rounded bg-muted px-1.5 py-0.5 text-[11px]">
                        {t("settings.offlineSkipped")}
                      </span>
                    ) : isOutdated && targetVersion ? (
                      <button
                        type="button"
                        disabled={isFleetBusy}
                        onClick={() =>
                          void orbitActions.upgradeHostDaemon(host.host_id, {
                            force: true,
                            targetVersion,
                          })
                        }
                        className="inline-flex cursor-pointer items-center gap-1 rounded-md bg-sky-500/12 px-2 py-0.5 font-mono text-[11px] font-medium text-sky-600 transition-colors hover:bg-sky-500/20 disabled:opacity-50 dark:text-sky-400"
                      >
                        <span>→ v{targetVersion}</span>
                      </button>
                    ) : (
                      <div className="flex items-center gap-1.5">
                        <span className="inline-flex items-center gap-1 text-[11.5px] text-emerald-600 dark:text-emerald-400">
                          <Check className="h-[12.5px] w-[12.5px]" />
                          <span>{isDone ? t("common.updated") : t("common.upToDate")}</span>
                        </span>
                        <button
                          type="button"
                          disabled={isFleetBusy}
                          title={t("settings.forceRestartHost", { host: host.display_name })}
                          onClick={() =>
                            void orbitActions.upgradeHostDaemon(host.host_id, {
                              force: true,
                              targetVersion,
                            })
                          }
                          className="text-muted-foreground hover:text-foreground hover:bg-accent hidden h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md transition-colors group-hover/hostrow:inline-flex disabled:opacity-40"
                        >
                          <RefreshCw className="h-[11.5px] w-[11.5px]" />
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        </SettingCard>
      </div>

      {/* Getting Started / Onboarding Guide */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupGettingStarted")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title={t("settings.setupGuide")}
            description={t("settings.setupGuideDesc")}
          >
            <button
              type="button"
              onClick={() => orbitActions.openOnboardingGuide()}
              className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors"
            >
              <Compass className="text-muted-foreground h-[13px] w-[13px]" />
              <span>{t("settings.openSetupGuide")}</span>
            </button>
          </SettingRow>
        </SettingCard>
      </div>
    </div>
  );
}
