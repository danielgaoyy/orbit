import { useEffect, useMemo, useRef } from "react";
import {
  AlertCircle,
  ArrowUpCircle,
  Check,
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

type PresetPermissionMode = "default" | "accept_edits" | "full_access";

const PREFIX_OPTIONS: SettingSelectOption<string>[] = [
  { value: "auto", label: "Auto", description: "Infer from thread topic" },
  { value: "orbit/", label: "orbit/", mono: true },
  { value: "feat/", label: "feat/", mono: true },
  { value: "fix/", label: "fix/", mono: true },
  { value: "chore/", label: "chore/", mono: true },
];

const PERMISSION_OPTIONS: SettingSelectOption<PresetPermissionMode>[] = [
  {
    value: "default",
    label: "Always Ask",
    description: "Confirm file edits and shell commands",
  },
  {
    value: "accept_edits",
    label: "Accept Edits",
    description: "Auto-approve edits, ask for commands",
  },
  {
    value: "full_access",
    label: "Full Access",
    description: "Run edits and commands without prompting",
  },
];

function orderHostsLocalFirst(hosts: HostCardView[]): HostCardView[] {
  const local: HostCardView[] = [];
  const remote: HostCardView[] = [];
  for (const h of hosts) {
    if (isLocalHostCard(h)) local.push(h);
    else remote.push(h);
  }
  return [...local, ...remote];
}

function hostTransportBadge(host: HostCardView): string {
  if (isLocalHostCard(host)) return "本机服务";
  if (host.ssh_alias) return `SSH · ${host.ssh_alias}`;
  if (host.connection_state === "online_relay") return "Relay";
  if (host.connection_state === "online_direct") return "直连";
  return "离线";
}

export function GeneralSection() {
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

  const appVersion = normalizeSemver(storeAppVersion) ?? "0.8.1";
  const autoCheckedRef = useRef(false);

  // Gently check for the latest release once when opening General settings
  useEffect(() => {
    if (autoCheckedRef.current || releaseUpdateInfo || checkingUpdate || isBrowserOffline) return;
    autoCheckedRef.current = true;
    void orbitActions.checkFleetUpdates({ silent: true });
  }, [releaseUpdateInfo, checkingUpdate, isBrowserOffline]);

  const activePermissionMode: PresetPermissionMode =
    draftMode === "accept_edits" || draftMode === "full_access" ? draftMode : "default";

  const activePrefix = PREFIX_OPTIONS.some((o) => o.value === worktreeBranchPrefix)
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
        return `发现 ${fleetSummary.totalPendingUpdates} 项可更新至 v${targetVersion}（含本机与已联通主机），运行中的会话不受影响。`;
      }
      return `本机桌面端与全部 ${fleetSummary.reachableHosts} 台在线主机均已是最新 v${targetVersion ?? appVersion}${fleetSummary.offlineHosts > 0 ? `（${fleetSummary.offlineHosts} 台离线）` : ""}。`;
    }
    return `统一检查并一键更新本机桌面端及全部 ${orderedHosts.length} 台已联通机器的 Orbit 版本。`;
  }, [
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
        title="General"
        description="Composer behavior, permissions, Git worktrees, and editor handoff."
      />

      {/* Workspace */}
      <div className="space-y-2">
        <SettingGroupLabel>Workspace</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title="Send with Enter"
            description={
              enterToSend
                ? "Press Enter to send, Shift+Enter for a new line."
                : `Press ${mod}Enter to send, Enter for a new line.`
            }
          >
            <ToggleSwitch
              checked={enterToSend}
              onChange={(v) => orbitActions.setEnterToSend(v)}
            />
          </SettingRow>

          <SettingRow
            title="Default access permissions"
            description="Initial approval level for new conversations."
          >
            <SettingSelect
              value={activePermissionMode}
              options={PERMISSION_OPTIONS}
              onChange={(mode) => orbitActions.setDraftConfig({ mode })}
              menuClassName="w-[260px]"
            />
          </SettingRow>

          <SettingRow
            title="Collapse thinking and tool calls"
            description="Fold all thinking and tool calls into a single live summary row; click anytime to expand all."
          >
            <ToggleSwitch
              checked={coalesceExploration}
              onChange={(v) => orbitActions.setWorktreePrefs({ coalesceExploration: v })}
            />
          </SettingRow>

          <SettingRow
            title="External editor"
            description={`Default editor opened with ${mod}O.`}
          >
            <SettingSelect
              value={effectivePreferred}
              options={editorOptions}
              onChange={(kind) => orbitActions.setPreferredEditor(kind)}
              placeholder="No editors found"
              disabled={editorOptions.length === 0}
            />
          </SettingRow>
        </SettingCard>
      </div>

      {/* Git & Worktrees */}
      <div className="space-y-2">
        <SettingGroupLabel>Git & Worktrees</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title="Isolate threads in worktrees"
            description="Start new conversations in an isolated Git worktree by default."
          >
            <ToggleSwitch
              checked={draftUseWorktree}
              onChange={(v) => orbitActions.setDraftConfig({ useWorktree: v })}
            />
          </SettingRow>

          <div className={cn(!draftUseWorktree && "opacity-55 transition-opacity")}>
            <SettingRow
              title="Branch prefix"
              description="Prefix for auto-generated worktree branches."
            >
              <SettingSelect
                value={activePrefix}
                options={PREFIX_OPTIONS}
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
        <SettingGroupLabel>Updates & Fleet Sync</SettingGroupLabel>
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
                      ? `最新 v${targetVersion}`
                      : `全部最新 v${targetVersion}`}
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
                    <span>重试失败主机</span>
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
                      <span>继续更新桌面端</span>
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
                    <span>{checkingUpdate ? "检查中…" : "检查更新"}</span>
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
                          ? "更新中…"
                          : fleetSummary.totalPendingUpdates > 0
                            ? `一键更新全部 (${fleetSummary.totalPendingUpdates})`
                            : "一键更新全部"}
                      </span>
                    </button>
                  ) : (
                    <button
                      type="button"
                      disabled={isFleetBusy || checkingUpdate || isBrowserOffline}
                      onClick={() =>
                        void orbitActions.upgradeAllConnectedHosts({ force: true })
                      }
                      title="强制将全部在线机器重新同步到最新版本（并重启服务）"
                      className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
                    >
                      {isFleetBusy ? (
                        <Loader2 className="h-[13px] w-[13px] animate-spin" />
                      ) : (
                        <ArrowUpCircle className="h-[13px] w-[13px]" />
                      )}
                      <span>{isFleetBusy ? "同步中…" : "全部重新同步"}</span>
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
                      本机客户端
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
                    <span>安装中</span>
                  </span>
                ) : fleetSummary.desktopOutdated && targetVersion ? (
                  <span className="rounded bg-sky-500/12 px-1.5 py-0.5 font-mono text-[11px] font-medium text-sky-600 dark:text-sky-400">
                    → v{targetVersion}
                  </span>
                ) : releaseUpdateInfo ? (
                  <span className="inline-flex items-center gap-1 text-[11.5px] text-emerald-600 dark:text-emerald-400">
                    <Check className="h-[12.5px] w-[12.5px]" />
                    <span>最新</span>
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
                      {normDaemon ? `v${normDaemon}` : "未知版本"}
                    </span>

                    {isUpgrading ? (
                      <span className="inline-flex items-center gap-1 text-[11.5px] text-sky-600 dark:text-sky-400">
                        <Loader2 className="h-[12px] w-[12px] animate-spin" />
                        <span>更新中…</span>
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
                        <span>重试</span>
                      </button>
                    ) : !reachable || isSkipped ? (
                      <span className="text-muted-foreground/80 rounded bg-muted px-1.5 py-0.5 text-[11px]">
                        离线跳过
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
                          <span>{isDone ? "已更新" : "最新"}</span>
                        </span>
                        <button
                          type="button"
                          disabled={isFleetBusy}
                          title={`强制更新 / 重启 ${host.display_name}`}
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
    </div>
  );
}
