import {
  t,
} from "@/lib/i18n";
import {
  isLocalHostCard,
} from "@/lib/projects";
import {
  type ReleaseUpdateInfo,
  compareSemver,
  normalizeSemver,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  resolveFleetTargetVersion,
} from "../store-helpers";
import type { OrbitStoreContext } from "../store-runtime";
import type {
  HostUpgradeOutcome,
} from "../store-types";

export function createUpdaterSlice(ctx: OrbitStoreContext) {
  const {
    getState,
    notifyToast,
    setState,
  } = ctx;

  return {


    async checkFleetUpdates(opts?: { silent?: boolean }): Promise<ReleaseUpdateInfo | null> {
      if (getState().checkingReleaseUpdate) {
        return getState().releaseUpdateInfo;
      }
      setState((s) => ({
        ...s,
        checkingReleaseUpdate: true,
        releaseCheckError: null,
      }));
      try {
        const [info] = await Promise.all([
          orbitIpc.checkAppUpdate(),
          ctx.actions.reloadFleetAndHosts(false).catch(() => {}),
        ]);
        setState((s) => ({
          ...s,
          releaseUpdateInfo: info,
          checkingReleaseUpdate: false,
          releaseCheckError: null,
        }));
        return info;
      } catch (err) {
        const detail =
          err instanceof Error ? err.message : typeof err === "string" ? err.trim() : "";
        const errMsg = detail
          ? t("store.checkUpdateFailed", { detail })
          : t("store.checkUpdateFallback");
        setState((s) => ({
          ...s,
          checkingReleaseUpdate: false,
          releaseCheckError: errMsg,
        }));
        if (!opts?.silent) {
          notifyToast(errMsg);
        }
        return null;
      }
    },


    async installDesktopAppUpdate(
      explicitRelease?: ReleaseUpdateInfo | null,
    ): Promise<boolean> {
      const release = explicitRelease ?? getState().releaseUpdateInfo;
      if (!release) return false;
      setState((s) => ({
        ...s,
        fleetUpdate: {
          ...s.fleetUpdate,
          phase: "upgrading_desktop",
          targetVersion: normalizeSemver(release.latest_version) ?? s.fleetUpdate.targetVersion,
          desktopStatus: t("store.downloadingDesktop", { version: release.latest_version }),
          desktopProgress: 0.02,
          desktopError: null,
          pendingDesktopUpdate: false,
          startedAt: s.fleetUpdate.startedAt ?? Date.now(),
        },
      }));
      try {
        await orbitIpc.performAppSelfUpdate(release.tag_name, (ev) => {
          const rawProgress = Number.isFinite(ev.progress) ? ev.progress : 0;
          const normalized = rawProgress > 1 ? Math.min(1, rawProgress / 100) : Math.max(0, rawProgress);
          const pct = Math.round(normalized * 100);
          const label = ev.status
            ? pct > 0 && pct < 100
              ? `${ev.status} (${pct}%)`
              : ev.status
            : t("store.updatingDesktopPct", { percent: pct });
          setState((s) => ({
            ...s,
            fleetUpdate: {
              ...s.fleetUpdate,
              phase: "upgrading_desktop",
              desktopStatus: label,
              desktopProgress: normalized,
              desktopError: null,
            },
          }));
        });
        return true;
      } catch (err) {
        const detail =
          err instanceof Error ? err.message : typeof err === "string" ? err.trim() : "";
        const errMsg = detail
          ? t("store.desktopUpdateFailed", { detail })
          : t("store.desktopUpdateFallback");
        setState((s) => ({
          ...s,
          fleetUpdate: {
            ...s.fleetUpdate,
            phase: "completed",
            desktopStatus: null,
            desktopProgress: null,
            desktopError: errMsg,
            summaryMessage: errMsg,
          },
        }));
        notifyToast(errMsg);
        return false;
      }
    },


    async upgradeHostDaemon(
      hostId: string,
      opts?: {
        force?: boolean;
        silent?: boolean;
        targetVersion?: string | null;
        skipReload?: boolean;
      },
    ): Promise<HostUpgradeOutcome> {
      const host = getState().fleet.hosts.find((h) => h.host_id === hostId);
      if (!host) return { ok: false, error: "Host not found" };
      if (getState().hostUpgradeStates[hostId]?.status === "upgrading") {
        return { ok: true, skipped: true };
      }

      const isLocal = isLocalHostCard(host);
      const isOnline =
        host.connection_state === "online_direct" || host.connection_state === "online_relay";
      const supportsWire = (host.daemon_protocol_version ?? 0) >= 4;
      const normApp = normalizeSemver(getState().appVersion);
      const fromVersion = normalizeSemver(host.daemon_version);
      const explicitTarget = normalizeSemver(opts?.targetVersion);
      const targetVersion =
        explicitTarget ?? resolveFleetTargetVersion(getState().appVersion, getState().releaseUpdateInfo);

      // Unreachable remote host without SSH fallback cannot be upgraded while offline.
      if (!isLocal && !isOnline && !host.ssh_alias) {
        const reason = t("store.hostOfflineSkipped");
        setState((s) => ({
          ...s,
          hostUpgradeStates: {
            ...s.hostUpgradeStates,
            [hostId]: {
              status: "skipped",
              mode: "wire",
              fromVersion,
              toVersion: fromVersion,
              error: reason,
              startedAt: Date.now(),
              finishedAt: Date.now(),
            },
          },
        }));
        return { ok: false, skipped: true, error: reason, version: fromVersion };
      }

      // Decide primary transport:
      // - Local host: use wire upgrade if online v4+ AND targeting a release newer than bundled Desktop;
      //   otherwise use 0ms bundled sidecar sync (`syncLocalDaemon`).
      // - Remote host: prefer wire on online v4+ (fast, works over Direct/Relay/ssh+relay with exact targetVersion),
      //   with automatic SSH fallback if `ssh_alias` is configured; use SSH directly when offline.
      const localNeedsRemoteDownload =
        isLocal &&
        isOnline &&
        supportsWire &&
        Boolean(targetVersion && normApp && compareSemver(targetVersion, normApp) > 0);
      const mode: "local" | "ssh" | "wire" = isLocal
        ? localNeedsRemoteDownload
          ? "wire"
          : "local"
        : isOnline && supportsWire
          ? "wire"
          : host.ssh_alias
            ? "ssh"
            : "wire";

      const startedAt = Date.now();
      setState((s) => ({
        ...s,
        hostUpgradeStates: {
          ...s.hostUpgradeStates,
          [hostId]: {
            status: "upgrading",
            mode,
            fromVersion,
            toVersion: targetVersion,
            startedAt,
          },
        },
      }));

      const waitForReconnectedVersion = async (): Promise<string | null> => {
        try {
          await orbitIpc.connectHost(host.host_id);
          await orbitIpc.refreshHost(host.host_id);
        } catch {
          // best-effort reconnect trigger
        }
        let latestVer = fromVersion;
        for (let attempt = 0; attempt < 8; attempt += 1) {
          try {
            const snap = await orbitIpc.getFleetOverview();
            const refreshed = snap.hosts.find((h) => h.host_id === host.host_id);
            const refreshedVer = normalizeSemver(refreshed?.daemon_version);
            if (refreshedVer) {
              latestVer = refreshedVer;
            }
            const onlineNow =
              refreshed?.connection_state === "online_direct" ||
              refreshed?.connection_state === "online_relay";
            if (
              onlineNow &&
              refreshedVer &&
              (!targetVersion || compareSemver(refreshedVer, targetVersion) >= 0)
            ) {
              break;
            }
          } catch {
            // ignore transient poll failure while daemon restarts
          }
          await new Promise((r) => window.setTimeout(r, 750));
        }
        return latestVer;
      };

      try {
        let resolvedVer: string | null = targetVersion ?? fromVersion;

        if (mode === "local") {
          const res = await orbitIpc.syncLocalDaemon(hostId, Boolean(opts?.force));
          if (res.status === "skipped_dev") {
            setState((s) => {
              const next = { ...s.hostUpgradeStates };
              delete next[hostId];
              return { ...s, hostUpgradeStates: next };
            });
            return { ok: true, skipped: true, version: fromVersion };
          }
          resolvedVer = normalizeSemver(res.daemon_version) ?? (await waitForReconnectedVersion());
          if (!opts?.skipReload) {
            await ctx.actions.reloadFleetAndHosts(false);
          }
          setState((s) => ({
            ...s,
            hostUpgradeStates: {
              ...s.hostUpgradeStates,
              [hostId]: {
                status: "done",
                mode,
                fromVersion,
                toVersion: resolvedVer,
                startedAt,
                finishedAt: Date.now(),
              },
            },
          }));
          if (!opts?.silent) {
            notifyToast(
              res.status === "updated"
                ? t("store.localServiceUpdated", {
                    version: resolvedVer
                      ? t("store.versionSuffix", { version: resolvedVer })
                      : "",
                  })
                : t("store.localServiceRestarted"),
            );
          }
          return { ok: true, version: resolvedVer };
        }

        if (mode === "wire") {
          try {
            await orbitIpc.upgradeHostDaemonOverWire(host.host_id, targetVersion ?? null);
          } catch (wireErr) {
            if (!isLocal && host.ssh_alias) {
              await orbitIpc.upgradeRemoteDaemon(
                host.host_id,
                host.ssh_alias,
                host.remote_daemon_port || undefined,
              );
            } else {
              throw wireErr;
            }
          }
        } else if (host.ssh_alias) {
          await orbitIpc.upgradeRemoteDaemon(
            host.host_id,
            host.ssh_alias,
            host.remote_daemon_port || undefined,
          );
        }

        resolvedVer = await waitForReconnectedVersion();
        if (!opts?.skipReload) {
          await ctx.actions.reloadFleetAndHosts(false);
        }
        setState((s) => ({
          ...s,
          hostUpgradeStates: {
            ...s.hostUpgradeStates,
            [hostId]: {
              status: "done",
              mode,
              fromVersion,
              toVersion: resolvedVer,
              startedAt,
              finishedAt: Date.now(),
            },
          },
        }));
        if (!opts?.silent) {
          notifyToast(
            resolvedVer
              ? t("store.hostUpdatedToVersion", {
                  host: host.display_name,
                  version: resolvedVer,
                })
              : t("store.hostUpdated", {
                  host: host.display_name,
                  version: "",
                }),
          );
        }
        return { ok: true, version: resolvedVer };
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setState((s) => ({
          ...s,
          hostUpgradeStates: {
            ...s.hostUpgradeStates,
            [hostId]: {
              status: "failed",
              mode,
              fromVersion,
              toVersion: targetVersion,
              error: msg,
              startedAt,
              finishedAt: Date.now(),
            },
          },
        }));
        if (!opts?.silent) {
          notifyToast(t("store.hostUpdateFailed", { host: host.display_name, detail: msg }));
        }
        return { ok: false, error: msg, version: fromVersion };
      }
    },


    async upgradeAllConnectedHosts(opts?: {
      force?: boolean;
      includeDesktopApp?: boolean;
      onlyHostIds?: string[];
    }): Promise<void> {
      if (
        getState().fleetUpdate.phase === "checking" ||
        getState().fleetUpdate.phase === "upgrading_hosts" ||
        getState().fleetUpdate.phase === "upgrading_desktop"
      ) {
        return;
      }

      const force = Boolean(opts?.force);
      const includeDesktopApp = opts?.includeDesktopApp ?? true;
      const onlyIdSet = opts?.onlyHostIds ? new Set(opts.onlyHostIds) : null;

      setState((s) => ({
        ...s,
        fleetUpdate: {
          phase: "checking",
          targetVersion: resolveFleetTargetVersion(s.appVersion, s.releaseUpdateInfo),
          desktopStatus: null,
          desktopProgress: null,
          desktopError: null,
          startedAt: Date.now(),
          summaryMessage: t("store.checkingFleetStatus"),
          pendingDesktopUpdate: false,
        },
      }));

      // Always resolve latest release before kicking off a fleet update so remote hosts
      // upgrade to the true latest GitHub Release (not stale local appVersion).
      let releaseInfo = getState().releaseUpdateInfo;
      if (!releaseInfo || force) {
        const fresh = await ctx.actions.checkFleetUpdates({ silent: true });
        if (fresh) {
          releaseInfo = fresh;
        }
      } else {
        await ctx.actions.reloadFleetAndHosts(false).catch(() => {});
      }

      const targetVersion = resolveFleetTargetVersion(getState().appVersion, releaseInfo);
      const desktopNeedsUpdate = Boolean(
        includeDesktopApp && releaseInfo?.has_newer_version,
      );

      const allHosts = getState().fleet.hosts;
      const candidateHosts = onlyIdSet
        ? allHosts.filter((h) => onlyIdSet.has(h.host_id))
        : allHosts;

      const hostsToUpgrade: typeof candidateHosts = [];
      let skippedOffline = 0;
      let alreadyUpToDate = 0;

      for (const host of candidateHosts) {
        const isLocal = isLocalHostCard(host);
        const isOnline =
          host.connection_state === "online_direct" ||
          host.connection_state === "online_relay";
        const normDaemon = normalizeSemver(host.daemon_version);

        // When Desktop itself will run `performAppSelfUpdate`, its updater script copies
        // the newly bundled `orbit` sidecar and hot-restarts the local daemon atomically,
        // so skip double-restarting the local daemon here unless explicitly retrying only local.
        if (isLocal && desktopNeedsUpdate && !onlyIdSet) {
          continue;
        }

        if (!isLocal && !isOnline && !host.ssh_alias) {
          skippedOffline += 1;
          setState((s) => ({
            ...s,
            hostUpgradeStates: {
              ...s.hostUpgradeStates,
              [host.host_id]: {
                status: "skipped",
                mode: "wire",
                fromVersion: normDaemon,
                toVersion: normDaemon,
                error: t("store.hostOfflineSkipped"),
                startedAt: Date.now(),
                finishedAt: Date.now(),
              },
            },
          }));
          continue;
        }

        const needsUpgrade =
          force ||
          !normDaemon ||
          (targetVersion !== null && compareSemver(normDaemon, targetVersion) < 0);

        if (needsUpgrade) {
          hostsToUpgrade.push(host);
        } else {
          alreadyUpToDate += 1;
          setState((s) => ({
            ...s,
            hostUpgradeStates: {
              ...s.hostUpgradeStates,
              [host.host_id]: {
                status: "done",
                mode: isLocal ? "local" : host.ssh_alias ? "ssh" : "wire",
                fromVersion: normDaemon,
                toVersion: normDaemon,
                startedAt: Date.now(),
                finishedAt: Date.now(),
              },
            },
          }));
        }
      }

      // Nothing to update anywhere
      if (hostsToUpgrade.length === 0 && !desktopNeedsUpdate) {
        const versionTag = targetVersion
          ? t("store.versionSuffix", { version: targetVersion })
          : "";
        const msg =
          skippedOffline > 0
            ? t("store.fleetOnlineUpToDateOfflineSkipped", {
                version: versionTag,
                count: skippedOffline,
              })
            : t("store.fleetAllUpToDate", { version: versionTag });
        setState((s) => ({
          ...s,
          fleetUpdate: {
            ...s.fleetUpdate,
            phase: "completed",
            targetVersion,
            summaryMessage: msg,
            pendingDesktopUpdate: false,
          },
        }));
        notifyToast(msg);
        return;
      }

      let succeeded = 0;
      let failed = 0;

      if (hostsToUpgrade.length > 0) {
        setState((s) => ({
          ...s,
          fleetUpdate: {
            ...s.fleetUpdate,
            phase: "upgrading_hosts",
            targetVersion,
            summaryMessage: t("store.fleetUpgradingHosts", {
              count: hostsToUpgrade.length,
              version: targetVersion
                ? t("store.versionSuffix", { version: targetVersion })
                : "",
            }),
            pendingDesktopUpdate: desktopNeedsUpdate,
          },
        }));

        const results = await Promise.allSettled(
          hostsToUpgrade.map((host) =>
            ctx.actions.upgradeHostDaemon(host.host_id, {
              force: true,
              silent: true,
              targetVersion,
              skipReload: true,
            }),
          ),
        );

        for (const r of results) {
          if (r.status === "fulfilled" && r.value.ok) {
            succeeded += 1;
          } else if (r.status === "fulfilled" && r.value.skipped) {
            skippedOffline += 1;
          } else {
            failed += 1;
          }
        }

        await ctx.actions.reloadFleetAndHosts(false).catch(() => {});
      }

      // Phase 2: Desktop self-update (exits & relaunches Desktop + syncs bundled local daemon)
      if (desktopNeedsUpdate && releaseInfo) {
        if (failed > 0) {
          const offlineSuffix =
            skippedOffline > 0
              ? t("store.fleetPausedOfflineSuffix", { count: skippedOffline })
              : "";
          const pausedMsg = t("store.fleetPausedWithFailures", {
            succeeded,
            failed,
            offline: offlineSuffix,
          });
          setState((s) => ({
            ...s,
            fleetUpdate: {
              ...s.fleetUpdate,
              phase: "paused_with_failures",
              targetVersion,
              summaryMessage: pausedMsg,
              pendingDesktopUpdate: true,
            },
          }));
          notifyToast(t("store.fleetPausedToast", { count: failed }));
          return;
        }

        await ctx.actions.installDesktopAppUpdate(releaseInfo);
        return;
      }

      const parts: string[] = [];
      if (succeeded > 0) parts.push(t("store.fleetPartSucceeded", { count: succeeded }));
      if (alreadyUpToDate > 0) parts.push(t("store.fleetPartCurrent", { count: alreadyUpToDate }));
      if (skippedOffline > 0) parts.push(t("store.fleetPartOffline", { count: skippedOffline }));
      if (failed > 0) parts.push(t("store.fleetPartFailed", { count: failed }));
      const summary =
        parts.length > 0
          ? t("store.fleetUpdateCompleteParts", {
              parts: parts.join(t("store.fleetPartsSeparator")),
              version: targetVersion
                ? t("store.versionSuffix", { version: targetVersion })
                : "",
            })
          : t("store.fleetUpdateComplete");

      setState((s) => ({
        ...s,
        fleetUpdate: {
          ...s.fleetUpdate,
          phase: "completed",
          targetVersion,
          summaryMessage: summary,
          pendingDesktopUpdate: false,
        },
      }));
      notifyToast(summary);
    },


    async retryFailedFleetHosts(): Promise<void> {
      const failedIds = Object.entries(getState().hostUpgradeStates)
        .filter(([, st]) => st.status === "failed")
        .map(([hostId]) => hostId);
      if (failedIds.length === 0) return;
      await ctx.actions.upgradeAllConnectedHosts({
        force: true,
        includeDesktopApp: getState().fleetUpdate.pendingDesktopUpdate,
        onlyHostIds: failedIds,
      });
    },
  };
}
