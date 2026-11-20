import { memo, useEffect, useState } from "react";
import { FolderPlus, GitBranch, ShieldCheck } from "lucide-react";
import {
  type AgentCardView,
  type HostCardView,
  type HostConnectionState,
  isAgentBackgroundWaiting,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
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

export function useSidebarRelativeClock(enabled: boolean): number {
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (!enabled) return;
    setNowMs(Date.now());
    const id = window.setInterval(() => {
      setNowMs(Date.now());
    }, 30_000);
    return () => window.clearInterval(id);
  }, [enabled]);
  return nowMs;
}

export const HostStateDot = memo(function HostStateDot({
  state,
}: {
  state: HostConnectionState;
}) {
  switch (state) {
    case "online_direct":
      return (
        <span
          className="h-[5.5px] w-[5.5px] shrink-0 rounded-full bg-emerald-500"
          title="Online"
        />
      );
    case "online_relay":
      return (
        <ShieldCheck
          className="h-[12px] w-[12px] shrink-0 text-indigo-400"
          aria-label="Noise E2EE Relay"
        />
      );
    case "connecting_direct":
    case "connecting_relay":
      return <OrbitSpinner size={12} className="text-amber-500" />;
    default:
      return (
        <span
          className="h-[5.5px] w-[5.5px] shrink-0 rounded-full bg-zinc-400/70"
          title="Offline"
        />
      );
  }
});

/**
 * Left `16×16px` semantic status slot (running spinner lives in the right time/action slot):
 * - Blocked / awaiting input -> `6.5px` amber dot
 * - Unread completed -> `6px` sky-blue dot
 * - Error -> `6px` rose dot
 * - Background task running -> `6px` calm emerald dot
 * - Idle & read inside isolated Git worktree -> `12.5px` muted branch glyph
 * - Normal local idle & read -> completely blank (zero gray bullet clutter)
 */
export const ConversationStatusGlyph = memo(function ConversationStatusGlyph({
  agent,
  isWorktree,
  isUnread,
}: {
  agent: AgentCardView;
  isWorktree: boolean;
  isUnread: boolean;
}) {
  if (agent.is_awaiting_permission) {
    return <span className="h-[6.5px] w-[6.5px] shrink-0 rounded-full bg-amber-500" />;
  }
  if (isUnread) {
    return <span className="h-[6px] w-[6px] shrink-0 rounded-full bg-sky-500" />;
  }
  if (agent.status_label === "Error") {
    return <span className="h-[6px] w-[6px] shrink-0 rounded-full bg-rose-500" />;
  }
  if (isAgentBackgroundWaiting(agent)) {
    return (
      <span
        title={agent.status_label || "Background task running"}
        className="h-[6px] w-[6px] shrink-0 rounded-full bg-emerald-500/85"
      />
    );
  }
  if (isWorktree) {
    return (
      <GitBranch className="text-muted-foreground/55 h-[12.5px] w-[12.5px] shrink-0 stroke-[1.75]" />
    );
  }
  return null;
});

export const AddProjectButton = memo(function AddProjectButton({
  hosts,
}: {
  hosts: HostCardView[];
}) {
  if (hosts.length === 0) {
    return (
      <SimpleTooltip label="Connect host">
        <button
          type="button"
          onClick={() => orbitActions.openSettings("hosts")}
          className="text-muted-foreground hover:text-foreground inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-[6px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
        >
          <FolderPlus className="h-[14px] w-[14px] stroke-[1.75]" />
        </button>
      </SimpleTooltip>
    );
  }

  if (hosts.length === 1) {
    return (
      <SimpleTooltip label="Add project">
        <button
          type="button"
          onClick={() => void orbitActions.promptAddProject(hosts[0].host_id)}
          className="text-muted-foreground hover:text-foreground inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-[6px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
        >
          <FolderPlus className="h-[14px] w-[14px] stroke-[1.75]" />
        </button>
      </SimpleTooltip>
    );
  }

  return (
    <DropdownMenu>
      <SimpleTooltip label="Add project">
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            className="text-muted-foreground hover:text-foreground inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-[6px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
          >
            <FolderPlus className="h-[14px] w-[14px] stroke-[1.75]" />
          </button>
        </DropdownMenuTrigger>
      </SimpleTooltip>

      <DropdownMenuContent align="start" sideOffset={6} className="w-[224px] p-1.5">
        <DropdownMenuLabel>Add Project From</DropdownMenuLabel>
        {hosts.map((h) => {
          const isLocal = isLocalHostCard(h);
          return (
            <DropdownMenuItem
              key={h.host_id}
              onSelect={() => void orbitActions.promptAddProject(h.host_id)}
            >
              <span className="flex min-w-0 items-center gap-2">
                <HostStateDot state={h.connection_state} />
                <span className="truncate">{h.display_name}</span>
              </span>
              <span className="text-muted-foreground shrink-0 text-[11.5px]">
                {isLocal ? "Local" : h.ssh_alias ? "SSH" : "Remote"}
              </span>
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
});
