import { memo, useCallback, useEffect, useRef, useState } from "react";
import {
  Check,
  Copy,
  ExternalLink,
  Loader2,
  Play,
  RefreshCw,
  Server,
  Square,
} from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { useI18n } from "@/lib/i18n";
import { type HostDetailView, type WorkspaceScriptInfo, orbitIpc } from "@/lib/orbit-ipc";
import { normalizeProjectPath } from "@/lib/projects";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_SCRIPTS: readonly WorkspaceScriptInfo[] = Object.freeze([]);
const FETCH_DEDUPE_MS = 3_000;
const lastScriptsFetchByScope = new Map<string, number>();

export function resolveActiveWorkspaceId(
  detail: HostDetailView | null | undefined,
  selectedAgentId: string | null,
  isDraftNewSession: boolean,
  selectedWorkspaceCwd: string | null | undefined,
): string | null {
  if (!detail) return null;
  const activeAgent =
    !isDraftNewSession && selectedAgentId
      ? detail.agents.find((a) => a.agent_id === selectedAgentId) ?? null
      : null;
  if (activeAgent?.workspace_id) {
    return activeAgent.workspace_id;
  }

  const rawCwd = activeAgent?.cwd || selectedWorkspaceCwd || "";
  const normCwd = normalizeProjectPath(rawCwd);
  if (!normCwd || normCwd === "~") return null;

  const exactMatch = detail.workspaces.find(
    (w) => !w.archived_at && normalizeProjectPath(w.cwd) === normCwd,
  );
  if (exactMatch) return exactMatch.workspace_id;

  const sourceMatch = detail.workspaces.find(
    (w) => !w.archived_at && w.source_cwd && normalizeProjectPath(w.source_cwd) === normCwd,
  );
  return sourceMatch?.workspace_id ?? null;
}

export function resolveScriptUrl(script: WorkspaceScriptInfo): string | null {
  if (script.proxy_url && script.proxy_url.trim().length > 0) {
    return script.proxy_url.trim();
  }
  if (script.port && script.port > 0) {
    return `http://localhost:${script.port}`;
  }
  return null;
}

function formatCompactScriptUrl(rawUrl: string): string {
  try {
    const parsed = new URL(rawUrl);
    return parsed.host + (parsed.pathname !== "/" ? parsed.pathname : "");
  } catch {
    return rawUrl.replace(/^https?:\/\//i, "");
  }
}

export function useWorkspaceScripts(hostId: string | null) {
  const workspaceId = useOrbitSelector((s) => {
    if (!hostId) return null;
    const detail = s.hostDetails[hostId];
    return resolveActiveWorkspaceId(
      detail,
      s.selectedAgentId,
      s.isDraftNewSession,
      s.selectedWorkspaceCwd,
    );
  });

  const isHostOnline = useOrbitSelector((s) => {
    if (!hostId) return false;
    const host = s.fleet.hosts.find((h) => h.host_id === hostId);
    return host?.connection_state === "online_direct" || host?.connection_state === "online_relay";
  });

  const scripts = useOrbitSelector((s) => {
    if (!hostId || !workspaceId) return EMPTY_SCRIPTS;
    return s.hostDetails[hostId]?.workspace_scripts?.[workspaceId] ?? EMPTY_SCRIPTS;
  });

  const refresh = useCallback(
    (force = false) => {
      if (!hostId || !workspaceId) return;
      const scopeKey = `${hostId}::${workspaceId}`;
      const now = Date.now();
      const last = lastScriptsFetchByScope.get(scopeKey) ?? 0;
      if (!force && now - last < FETCH_DEDUPE_MS) return;
      lastScriptsFetchByScope.set(scopeKey, now);
      void orbitIpc.listWorkspaceScripts(hostId, workspaceId).catch(() => undefined);
    },
    [hostId, workspaceId],
  );

  useEffect(() => {
    if (!hostId || !workspaceId || !isHostOnline) return;
    refresh(false);
  }, [hostId, workspaceId, isHostOnline, refresh]);

  return { workspaceId, scripts, refresh };
}

interface WorkspaceScriptRowProps {
  hostId: string;
  workspaceId: string;
  script: WorkspaceScriptInfo;
  compact?: boolean;
}

const WorkspaceScriptRow = memo(function WorkspaceScriptRow({
  hostId,
  workspaceId,
  script,
  compact = false,
}: WorkspaceScriptRowProps) {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const copiedTimerRef = useRef<number | null>(null);
  const busyTimerRef = useRef<number | null>(null);

  useEffect(() => {
    // Clear transient busy spinner as soon as daemon reports updated status
    setBusy(false);
    if (busyTimerRef.current !== null) {
      window.clearTimeout(busyTimerRef.current);
      busyTimerRef.current = null;
    }
  }, [script.status, script.proxy_url, script.port]);

  useEffect(() => {
    return () => {
      if (copiedTimerRef.current !== null) window.clearTimeout(copiedTimerRef.current);
      if (busyTimerRef.current !== null) window.clearTimeout(busyTimerRef.current);
    };
  }, []);

  const isRunning = script.status === "running";
  const isFailed = script.status === "failed";
  const url = resolveScriptUrl(script);

  const handleToggle = async () => {
    if (busy) return;
    setBusy(true);
    busyTimerRef.current = window.setTimeout(() => {
      setBusy(false);
    }, 6_000);
    try {
      if (isRunning) {
        await orbitIpc.stopWorkspaceScript(hostId, workspaceId, script.name);
      } else {
        await orbitIpc.startWorkspaceScript(hostId, workspaceId, script.name);
      }
      // Request fresh snapshot right after start/stop to guarantee sync
      void orbitIpc.listWorkspaceScripts(hostId, workspaceId).catch(() => undefined);
    } catch {
      setBusy(false);
      orbitActions.showToast(t("common.actionFailed"));
    }
  };

  const handleOpenUrl = () => {
    if (!url) return;
    void orbitIpc.openExternalBrowserUrl(url).catch(() => {
      orbitActions.showToast(t("common.actionFailed"));
    });
  };

  const handleCopyUrl = () => {
    if (!url) return;
    void navigator.clipboard
      .writeText(url)
      .then(() => {
        setCopied(true);
        if (copiedTimerRef.current !== null) window.clearTimeout(copiedTimerRef.current);
        copiedTimerRef.current = window.setTimeout(() => setCopied(false), 1_500);
        orbitActions.showToast(t("sidePanel.scriptCopiedUrl", { url: formatCompactScriptUrl(url) }));
      })
      .catch(() => {
        orbitActions.showToast(t("common.actionFailed"));
      });
  };

  const statusLabel = isRunning
    ? t("sidePanel.scriptRunning")
    : isFailed
      ? t("sidePanel.scriptFailed")
      : t("sidePanel.scriptStopped");

  return (
    <div
      className={cn(
        "group/script flex flex-col rounded-[8px] transition-colors hover:bg-black/[0.035] dark:hover:bg-white/[0.045]",
        compact ? "px-2 py-1.5" : "px-2.5 py-1.5",
      )}
    >
      <div className="flex min-w-0 items-center justify-between gap-2">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <span
            title={statusLabel}
            aria-label={statusLabel}
            className={cn(
              "h-[6.5px] w-[6.5px] shrink-0 rounded-full",
              isRunning
                ? "bg-emerald-500"
                : isFailed
                  ? "bg-rose-500"
                  : "bg-muted-foreground/35",
            )}
          />
          <span className="text-foreground/90 truncate text-[12.5px] font-medium">
            {script.name}
          </span>
          {script.port != null && script.port > 0 && (
            <span className="text-muted-foreground/65 shrink-0 font-mono text-[11px] tabular-nums">
              :{script.port}
            </span>
          )}
        </div>

        <button
          type="button"
          disabled={busy}
          onClick={() => void handleToggle()}
          className={cn(
            "inline-flex h-[24px] shrink-0 cursor-pointer items-center gap-1 rounded-[6px] px-2 text-[11.5px] font-medium transition-colors disabled:opacity-50",
            isRunning
              ? "text-muted-foreground hover:text-foreground bg-black/[0.045] hover:bg-black/[0.08] dark:bg-white/[0.07] dark:hover:bg-white/[0.12]"
              : "text-foreground/82 hover:text-foreground bg-black/[0.04] hover:bg-black/[0.075] dark:bg-white/[0.06] dark:hover:bg-white/[0.11]",
          )}
        >
          {busy ? (
            <Loader2 className="h-[11px] w-[11px] animate-spin" />
          ) : isRunning ? (
            <Square className="h-[10px] w-[10px] fill-current" />
          ) : (
            <Play className="h-[10.5px] w-[10.5px] fill-current" />
          )}
          <span>{isRunning ? t("sidePanel.scriptStop") : t("sidePanel.scriptStart")}</span>
        </button>
      </div>

      {/* Live *.localhost URL with Open + Copy, or restrained command preview when stopped */}
      {url ? (
        <div className="mt-1 flex min-w-0 items-center justify-between gap-1.5 pl-[14.5px]">
          <button
            type="button"
            onClick={handleOpenUrl}
            title={t("sidePanel.scriptOpenUrl", { url })}
            className="text-muted-foreground/80 hover:text-foreground group/url inline-flex min-w-0 flex-1 cursor-pointer items-center gap-1 text-left font-mono text-[11px] transition-colors"
          >
            <span className="truncate underline decoration-transparent underline-offset-2 transition-colors group-hover/url:decoration-current">
              {formatCompactScriptUrl(url)}
            </span>
            <ExternalLink className="h-[10.5px] w-[10.5px] shrink-0 opacity-60 transition-opacity group-hover/url:opacity-100" />
          </button>

          <SimpleTooltip label={copied ? t("common.copied") : t("sidePanel.scriptCopyUrl")}>
            <button
              type="button"
              aria-label={t("sidePanel.scriptCopyUrl")}
              onClick={handleCopyUrl}
              className="text-muted-foreground/65 hover:text-foreground inline-flex h-[20px] w-[20px] shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
            >
              {copied ? (
                <Check className="h-[11px] w-[11px] text-emerald-500" />
              ) : (
                <Copy className="h-[11px] w-[11px]" />
              )}
            </button>
          </SimpleTooltip>
        </div>
      ) : script.command ? (
        <div
          title={script.command}
          className="text-muted-foreground/55 mt-0.5 truncate pl-[14.5px] font-mono text-[11px]"
        >
          {script.command}
        </div>
      ) : null}

      {isFailed && script.last_error && (
        <div
          title={script.last_error}
          className="mt-0.5 truncate pl-[14.5px] text-[11px] text-rose-500/85 dark:text-rose-400/85"
        >
          {script.last_error}
        </div>
      )}
    </div>
  );
});

/**
 * Restrained inline section for `SidePanelLaunchpad`.
 * Renders ZERO DOM (0px) when the active workspace has no `orbit.json` / `paseo.json` scripts.
 */
export const WorkspaceScriptsSection = memo(function WorkspaceScriptsSection({
  hostId,
}: {
  hostId: string | null;
}) {
  const { t } = useI18n();
  const { workspaceId, scripts, refresh } = useWorkspaceScripts(hostId);

  if (!hostId || !workspaceId || scripts.length === 0) return null;

  const runningCount = scripts.filter((s) => s.status === "running").length;

  return (
    <section>
      <div className="mb-1.5 flex items-center justify-between gap-2 px-2.5 text-[11.5px] font-medium">
        <div className="flex items-center gap-1.5">
          <span className="text-muted-foreground/75">{t("sidePanel.scriptsTitle")}</span>
          <span className="text-muted-foreground/55 tabular-nums">
            {runningCount > 0 ? `${runningCount}/${scripts.length}` : scripts.length}
          </span>
        </div>
        <SimpleTooltip label={t("sidePanel.scriptRefresh")}>
          <button
            type="button"
            aria-label={t("sidePanel.scriptRefresh")}
            onClick={() => refresh(true)}
            className="text-muted-foreground/55 hover:text-foreground inline-flex h-[20px] w-[20px] cursor-pointer items-center justify-center rounded-[5px] transition-colors hover:bg-black/[0.05] dark:hover:bg-white/[0.07]"
          >
            <RefreshCw className="h-[11.5px] w-[11.5px]" />
          </button>
        </SimpleTooltip>
      </div>
      <div className="flex flex-col gap-0.5">
        {scripts.map((script) => (
          <WorkspaceScriptRow
            key={script.name}
            hostId={hostId}
            workspaceId={workspaceId}
            script={script}
          />
        ))}
      </div>
    </section>
  );
});

/**
 * Subtle header Services indicator popover for `WorkspaceSidePanel`.
 * Invisible (0px) when the active workspace has no configured scripts.
 */
export const WorkspaceScriptsPopover = memo(function WorkspaceScriptsPopover({
  hostId,
}: {
  hostId: string | null;
}) {
  const { t } = useI18n();
  const { workspaceId, scripts, refresh } = useWorkspaceScripts(hostId);

  if (!hostId || !workspaceId || scripts.length === 0) return null;

  const runningCount = scripts.filter((s) => s.status === "running").length;

  return (
    <Popover onOpenChange={(open) => { if (open) refresh(true); }}>
      <SimpleTooltip
        label={
          runningCount > 0
            ? `${t("sidePanel.scriptsTitle")} · ${t("sidePanel.servicesRunningCount", { running: runningCount })}`
            : t("sidePanel.scriptsTitle")
        }
      >
        <PopoverTrigger asChild>
          <button
            type="button"
            aria-label={t("sidePanel.scriptsTitle")}
            className={cn(
              "inline-flex h-[26px] shrink-0 cursor-pointer items-center gap-1.5 rounded-[7px] px-2 text-[11.5px] transition-colors",
              runningCount > 0
                ? "text-foreground/85 bg-black/[0.04] hover:bg-black/[0.07] dark:bg-white/[0.06] dark:hover:bg-white/[0.1]"
                : "text-muted-foreground hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
            )}
          >
            {runningCount > 0 ? (
              <span className="h-[6px] w-[6px] shrink-0 rounded-full bg-emerald-500" />
            ) : (
              <Server className="h-[12.5px] w-[12.5px] shrink-0 stroke-[1.75]" />
            )}
            <span className="tabular-nums font-medium">
              {runningCount > 0 ? `${runningCount}/${scripts.length}` : scripts.length}
            </span>
          </button>
        </PopoverTrigger>
      </SimpleTooltip>

      <PopoverContent align="end" sideOffset={6} className="w-[310px] p-2">
        <div className="mb-1.5 flex items-center justify-between px-2 pt-0.5">
          <span className="text-muted-foreground text-[11.5px] font-medium">
            {t("sidePanel.scriptsTitle")}
          </span>
          <SimpleTooltip label={t("sidePanel.scriptRefresh")}>
            <button
              type="button"
              aria-label={t("sidePanel.scriptRefresh")}
              onClick={() => refresh(true)}
              className="text-muted-foreground/65 hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[5px] transition-colors hover:bg-black/[0.05] dark:hover:bg-white/[0.08]"
            >
              <RefreshCw className="h-[12px] w-[12px]" />
            </button>
          </SimpleTooltip>
        </div>
        <div className="flex max-h-[280px] flex-col gap-0.5 overflow-y-auto">
          {scripts.map((script) => (
            <WorkspaceScriptRow
              key={script.name}
              hostId={hostId}
              workspaceId={workspaceId}
              script={script}
              compact
            />
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
});
