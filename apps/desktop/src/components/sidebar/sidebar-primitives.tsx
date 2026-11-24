import { memo, useSyncExternalStore } from "react";
import { AlarmClock, FolderPlus, GitBranch } from "lucide-react";
import {
  type AgentCardView,
  type HostCardView,
  isAgentTimerWaiting,
} from "@/lib/orbit-ipc";
import { useI18n } from "@/lib/i18n";
import { isLocalHostCard } from "@/lib/projects";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { HostStatusIndicator } from "@/components/ui/status-indicator";
import { orbitActions } from "@/state/orbit-store";

export function formatCompactRelativeTime(rfc3339: string, nowMs: number): string {
  const ts = Date.parse(rfc3339);
  if (Number.isNaN(ts)) return "";
  const diffSec = Math.max(0, Math.floor((nowMs - ts) / 1000));
  if (diffSec < 45) return "now";
  const mins = Math.floor(diffSec / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  const weeks = Math.floor(days / 7);
  if (days < 30) return `${weeks}w`;
  const months = Math.floor(days / 30);
  if (days < 365) return `${Math.max(1, months)}mo`;
  const years = Math.max(1, Math.floor(days / 365));
  return `${years}y`;
}

// Shared singleton 30s relative clock so individual timestamp spans update without
// triggering full-sidebar React reconciliation every 30 seconds.
let sharedClockNowMs = Date.now();
const clockListeners = new Set<() => void>();
let clockIntervalId: number | null = null;
let clockVisibilityAttached = false;

function tickRelativeClock(): void {
  if (typeof document !== "undefined" && document.hidden) return;
  sharedClockNowMs = Date.now();
  for (const listener of clockListeners) {
    listener();
  }
}

function handleVisibilityChange(): void {
  if (typeof document !== "undefined" && !document.hidden) {
    tickRelativeClock();
  }
}

function subscribeRelativeClock(listener: () => void): () => void {
  clockListeners.add(listener);
  if (clockIntervalId === null && typeof window !== "undefined") {
    sharedClockNowMs = Date.now();
    clockIntervalId = window.setInterval(tickRelativeClock, 30_000);
    if (!clockVisibilityAttached && typeof document !== "undefined") {
      document.addEventListener("visibilitychange", handleVisibilityChange);
      clockVisibilityAttached = true;
    }
  }
  return () => {
    clockListeners.delete(listener);
    if (clockListeners.size === 0 && clockIntervalId !== null && typeof window !== "undefined") {
      window.clearInterval(clockIntervalId);
      clockIntervalId = null;
      if (clockVisibilityAttached && typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", handleVisibilityChange);
        clockVisibilityAttached = false;
      }
    }
  };
}

function getRelativeClockSnapshot(): number {
  return sharedClockNowMs;
}

const noopSubscribe = () => () => {};

export function useSidebarRelativeClock(enabled: boolean): number {
  return useSyncExternalStore(
    enabled ? subscribeRelativeClock : noopSubscribe,
    getRelativeClockSnapshot,
    getRelativeClockSnapshot,
  );
}

export const SidebarRelativeTime = memo(function SidebarRelativeTime({
  rfc3339,
  className,
}: {
  rfc3339: string;
  className?: string;
}) {
  const nowMs = useSidebarRelativeClock(true);
  return <span className={className}>{formatCompactRelativeTime(rfc3339, nowMs)}</span>;
});

/**
 * Unified left `16×16px` lifecycle status rail (strictly aligned with Codex Desktop / Cursor / Claude Code):
 * 1. Blocked / awaiting approval -> amber 6px signal dot
 * 2. Actively running (main agent or subagents) -> monochrome `OrbitSpinner`
 * 3. Error -> red 6px signal dot
 * 4. Completed & unread -> blue 6px signal dot
 * 5. Idle & read inside isolated Git worktree -> faint `GitBranch` glyph
 * 6. Normal idle & read (including background dev servers / armed timers, which live in Overview) -> empty
 */
export const ConversationStatusGlyph = memo(function ConversationStatusGlyph({
  agent,
  isForegroundRunning,
  hasRunningSubagents,
  isWorktree,
  isUnread,
}: {
  agent: AgentCardView;
  isForegroundRunning: boolean;
  hasRunningSubagents: boolean;
  isWorktree: boolean;
  isUnread: boolean;
}) {
  const { t } = useI18n();
  if (agent.is_awaiting_permission) {
    return (
      <span
        title={t("sidebar.waitingForApproval")}
        className="bg-signal-warning size-[6px] shrink-0 rounded-full"
      />
    );
  }
  if (isForegroundRunning || hasRunningSubagents) {
    return (
      <span
        title={isForegroundRunning ? t("sidebar.running") : t("sidebar.subagentRunning")}
        className="text-ink-secondary inline-flex shrink-0 items-center justify-center"
      >
        <OrbitSpinner size={13} />
      </span>
    );
  }
  if (agent.status_label === "Error") {
    return (
      <span
        title={t("sidebar.error")}
        className="bg-signal-danger size-[6px] shrink-0 rounded-full"
      />
    );
  }
  if (isUnread) {
    return <span className="bg-signal-info size-[6px] shrink-0 rounded-full" />;
  }
  if (isAgentTimerWaiting(agent)) {
    return (
      <span
        title={t("sidebar.timerArmed")}
        className="inline-flex shrink-0 items-center justify-center"
      >
        <AlarmClock
          aria-label={t("sidebar.timerArmed")}
          className="text-ink-faint size-[11.5px] shrink-0 stroke-[1.7]"
        />
      </span>
    );
  }
  if (isWorktree) {
    return (
      <span
        title={agent.worktree_branch || undefined}
        className="inline-flex shrink-0 items-center justify-center"
      >
        <GitBranch className="text-ink-faint size-[12px] shrink-0 stroke-[1.75]" />
      </span>
    );
  }
  return null;
});

export const AddProjectButton = memo(function AddProjectButton({
  hosts,
}: {
  hosts: HostCardView[];
}) {
  const { t } = useI18n();

  if (hosts.length === 0) {
    return (
      <IconButton
        size="xs"
        tooltip={t("sidebar.connectHost")}
        onClick={() => orbitActions.openSettings("hosts")}
      >
        <FolderPlus className="size-[14px] stroke-[1.75]" />
      </IconButton>
    );
  }

  if (hosts.length === 1) {
    return (
      <IconButton
        size="xs"
        tooltip={t("sidebar.addProject")}
        onClick={() => void orbitActions.promptAddProject(hosts[0].host_id)}
      >
        <FolderPlus className="size-[14px] stroke-[1.75]" />
      </IconButton>
    );
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <IconButton size="xs" tooltip={t("sidebar.addProject")}>
          <FolderPlus className="size-[14px] stroke-[1.75]" />
        </IconButton>
      </DropdownMenuTrigger>

      <DropdownMenuContent align="start" sideOffset={6} className="min-w-[208px] max-w-[260px]">
        <DropdownMenuGroup>
          <DropdownMenuLabel>{t("sidebar.addProjectFrom")}</DropdownMenuLabel>
          {hosts.map((h) => {
            const isLocal = isLocalHostCard(h);
            return (
              <DropdownMenuItem
                key={h.host_id}
                onSelect={() => void orbitActions.promptAddProject(h.host_id)}
              >
                <span className="flex min-w-0 items-center gap-2">
                  <HostStatusIndicator state={h.connection_state} />
                  <span className="truncate">{h.display_name}</span>
                </span>
                <span className="text-ink-tertiary shrink-0 text-[11px]">
                  {isLocal ? t("sidebar.hostLocal") : h.ssh_alias ? "SSH" : t("sidebar.hostRemote")}
                </span>
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
});
