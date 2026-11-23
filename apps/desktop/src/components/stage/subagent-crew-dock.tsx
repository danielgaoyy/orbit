import { memo } from "react";
import { Archive, ArrowUpRight, ShieldAlert, Square } from "lucide-react";
import { IconButton } from "@/components/ui/icon-button";
import { SimpleTooltip } from "@/components/ui/overlays";
import { SubagentAvatar } from "@/components/ui/subagent-avatar";
import { orbitIpc } from "@/lib/orbit-ipc";
import { isSubagentActiveStatus } from "@/lib/subagents";
import type { SessionSubagentWithAvatar } from "@/state/selectors/session-selectors";
import { orbitActions } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function openSubagent(
  parentAgentId: string,
  sa: SessionSubagentWithAvatar,
) {
  orbitActions.openSubagentTab({
    callId: sa.callId,
    role: sa.title,
    promptPreview: sa.promptPreview,
    childAgentId: sa.childAgentId,
    childHostId: sa.childHostId || undefined,
    providerSubagentId: sa.providerSubagentId,
    parentAgentId,
  });
}

function statusSubtitle(sa: SessionSubagentWithAvatar): string {
  const base =
    sa.status === "awaiting_permission"
      ? "Approval required"
      : sa.status === "running"
        ? "Working…"
        : sa.status === "failed"
          ? "Failed"
          : sa.status === "canceled"
            ? "Stopped"
            : "Finished";
  return sa.remoteHostName ? `${base} · ${sa.remoteHostName}` : base;
}

export function archiveSettledSubagentsList(
  hostId: string,
  parentAgentId: string,
  settled: readonly SessionSubagentWithAvatar[],
): void {
  const managed = settled.filter((item) => item.isManaged && item.childAgentId);
  for (const item of managed) {
    if (item.childAgentId) {
      orbitActions.archiveThread(item.childHostId, item.childAgentId);
    }
  }
  const nativeItems = settled.filter((item) => !(item.isManaged && item.childAgentId));
  if (nativeItems.length > 0) {
    orbitActions.archiveFinishedSubagents(
      hostId,
      parentAgentId,
      nativeItems.map((item) => ({
        id: item.callId,
        providerSubagentId: item.providerSubagentId,
        childAgentId: item.childAgentId,
      })),
    );
  }
}

/**
 * Form 1A — Living Subagent Crew list rendered inside the Antigravity-style Overview HUD.
 * Every subagent shows its collision-free OneWorks 3D character: bouncing while working,
 * tilted with an amber halo when awaiting approval, and resting quietly once completed.
 */
export const SubagentCrewList = memo(function SubagentCrewList({
  hostId,
  agentId,
  subagents,
}: {
  hostId: string;
  agentId: string;
  subagents: readonly SessionSubagentWithAvatar[];
}) {
  if (subagents.length === 0) return null;

  return (
    <div className="space-y-0.5 pb-1">
      {subagents.map((sa) => {
        const isActive = isSubagentActiveStatus(sa.status);
        const isAwaiting = sa.status === "awaiting_permission";
        const canManage = sa.isManaged && Boolean(sa.childAgentId);
        const canStop = canManage && sa.status === "running";
        const canArchive = !isActive;

        return (
          <div
            key={sa.key}
            className={cn(
              "group/crew flex min-h-[42px] w-full items-center gap-2 rounded-[8px] px-2 py-1.5 transition-colors",
              isAwaiting
                ? "bg-signal-warning-bg/75 hover:bg-signal-warning-bg"
                : "hover:bg-accent/70",
            )}
          >
            <button
              type="button"
              title={sa.promptPreview || sa.title}
              onClick={() => openSubagent(agentId, sa)}
              className="flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 text-left outline-none"
            >
              <SubagentAvatar
                character={sa.character}
                status={sa.status}
                size="md"
                showStatusBadge={!isActive}
              />
              <span className="min-w-0 flex-1">
                <span className="text-ink-primary block truncate text-[12px] leading-[16px] font-medium">
                  {sa.title}
                </span>
                <span
                  className={cn(
                    "block truncate text-[11px] leading-[15px]",
                    isAwaiting
                      ? "text-signal-warning font-medium"
                      : sa.status === "failed"
                        ? "text-signal-danger"
                        : "text-ink-tertiary",
                  )}
                >
                  {statusSubtitle(sa)}
                </span>
              </span>
            </button>

            <div className="flex shrink-0 items-center gap-0.5">
              {isAwaiting ? (
                <button
                  type="button"
                  onClick={() => openSubagent(agentId, sa)}
                  className="bg-signal-warning-bg text-signal-warning border-signal-warning-border hover:bg-signal-warning/20 inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-[6px] border px-2 text-[11px] font-medium transition-colors"
                >
                  <ShieldAlert className="size-3 shrink-0" />
                  <span>Review</span>
                </button>
              ) : (
                <div className="hidden items-center gap-0.5 group-focus-within/crew:flex group-hover/crew:flex">
                  {canStop && sa.childAgentId && (
                    <IconButton
                      size="xs"
                      tone="danger"
                      tooltip="Stop subagent"
                      onClick={() => {
                        void orbitIpc.interruptAgent(sa.childHostId, sa.childAgentId!).then(
                          () => orbitActions.showToast(`Stopped ${sa.title}`),
                          (err) => orbitActions.showToast(`Failed to stop: ${String(err)}`),
                        );
                      }}
                    >
                      <Square className="size-[9px] fill-current" />
                    </IconButton>
                  )}
                  {canArchive && (
                    <IconButton
                      size="xs"
                      tooltip="Archive subagent"
                      onClick={() => {
                        if (canManage && sa.childAgentId) {
                          orbitActions.archiveThread(sa.childHostId, sa.childAgentId);
                        } else {
                          orbitActions.archiveSubagent(hostId, agentId, {
                            id: sa.callId,
                            providerSubagentId: sa.providerSubagentId,
                            childAgentId: sa.childAgentId,
                            role: sa.title,
                          });
                        }
                      }}
                    >
                      <Archive className="size-3" />
                    </IconButton>
                  )}
                  <IconButton
                    size="xs"
                    tooltip={`Open ${sa.title}`}
                    onClick={() => openSubagent(agentId, sa)}
                  >
                    <ArrowUpRight className="size-3" />
                  </IconButton>
                </div>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
});

/**
 * Form 1B — TopBar Mini-Dock relay shown ONLY when Overview is collapsed or narrow,
 * guaranteeing zero visual duplication across TopBar and Overview at any viewport width.
 */
export const SubagentCrewMiniDock = memo(function SubagentCrewMiniDock({
  agentId,
  subagents,
}: {
  agentId: string;
  subagents: readonly SessionSubagentWithAvatar[];
}) {
  if (subagents.length === 0) return null;

  const shown = subagents.slice(0, 3);
  const overflow = Math.max(0, subagents.length - shown.length);

  return (
    <div
      data-slot="subagent-crew-mini-dock"
      className="flex shrink-0 items-center"
    >
      <div className="flex items-center -space-x-1.5">
        {shown.map((sa) => (
          <SimpleTooltip
            key={sa.key}
            label={`${sa.title} · ${statusSubtitle(sa)}`}
            side="bottom"
          >
            <button
              type="button"
              aria-label={`${sa.title} (${statusSubtitle(sa)})`}
              onClick={() => openSubagent(agentId, sa)}
              className="ring-surface-base relative inline-flex cursor-pointer rounded-full ring-2 transition-transform duration-150 hover:z-10 hover:scale-110 focus-visible:z-10 focus-visible:outline-none"
            >
              <SubagentAvatar
                character={sa.character}
                status={sa.status}
                size="sm"
              />
            </button>
          </SimpleTooltip>
        ))}
      </div>

      {overflow > 0 && (
        <SimpleTooltip label={`${overflow} more subagents`} side="bottom">
          <button
            type="button"
            onClick={() => orbitActions.toggleOverviewPanel()}
            className="bg-surface-recessed text-ink-secondary hover:bg-accent hover:text-ink-primary border-border-subtle ml-1 inline-flex h-[20px] min-w-[22px] cursor-pointer items-center justify-center rounded-full border px-1.5 font-mono text-[11px] font-medium tabular-nums transition-colors"
          >
            +{overflow}
          </button>
        </SimpleTooltip>
      )}
    </div>
  );
});
