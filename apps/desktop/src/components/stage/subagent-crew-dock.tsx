import { memo } from "react";
import { Button } from "@/components/ui/button";
import { SimpleTooltip } from "@/components/ui/overlays";
import { SubagentAvatar } from "@/components/ui/subagent-avatar";
import { useI18n } from "@/lib/i18n";
import { isSubagentActiveStatus, type ResolvedSubagentItem } from "@/lib/subagents";
import type { SessionSubagentWithAvatar } from "@/state/selectors/session-selectors";
import { getOrbitState, orbitActions } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export function openSubagent(
  parentAgentId: string | undefined,
  sa: Pick<ResolvedSubagentItem, "callId" | "title" | "promptPreview" | "childAgentId" | "childHostId" | "providerSubagentId">,
) {
  // A managed peer can appear in the transcript before the fleet publishes its card.
  // Keep the call-scoped tab usable until the card supplies its host.
  const managedId = sa.childAgentId?.startsWith("ag_") ? sa.childAgentId : null;
  const hosts = getOrbitState().hostDetails;
  const knownHost = managedId && hosts[sa.childHostId]?.agents.some((a) => a.agent_id === managedId)
    ? sa.childHostId
    : managedId && Object.values(hosts).find((detail) =>
        detail.agents.some((a) => a.agent_id === managedId),
      )?.host.host_id;
  orbitActions.openSubagentTab({
    callId: sa.callId,
    role: sa.title,
    promptPreview: sa.promptPreview,
    childAgentId: managedId && !knownHost ? null : sa.childAgentId,
    childHostId: knownHost || sa.childHostId || undefined,
    providerSubagentId: sa.providerSubagentId,
    parentAgentId,
  });
}

export function SubagentPanelAction({
  name, inspect = false, alwaysVisible = false, onClick,
}: {
  name: string;
  inspect?: boolean;
  alwaysVisible?: boolean;
  onClick: () => void;
}) {
  const { t } = useI18n();
  const label = t(inspect ? "approval.inspectSubagent" : "sidebar.remoteOpenAction");
  return (
    <Button
      type="button"
      variant="ghost"
      size="xs"
      aria-label={inspect ? `${label} · ${name}` : t("overview.openSubagent", { name })}
      title={t("stage.openTargetInPanel", { target: name })}
      onClick={onClick}
      className={cn(
        "h-[22px] rounded-[5px] px-1.5 text-[11px] text-ink-secondary",
        !alwaysVisible && "opacity-0 group-hover/msgrow:opacity-100 group-focus-within/msgrow:opacity-100 [@media(hover:none)]:opacity-100",
      )}
    >
      {label}
    </Button>
  );
}

type TranslateFn = ReturnType<typeof useI18n>["t"];

function statusSubtitle(sa: SessionSubagentWithAvatar, t: TranslateFn): string {
  const base =
    sa.status === "awaiting_permission"
      ? t("overview.subagentApprovalRequired")
      : sa.status === "running"
        ? t("overview.subagentWorking")
        : sa.status === "failed"
          ? t("overview.subagentFailed")
          : sa.status === "canceled"
            ? t("overview.subagentStopped")
            : t("overview.subagentFinished");
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
 * Form 1A — Living Subagent Crew list rendered inside the Overview HUD.
 * Every subagent shows its assigned animal avatar: bouncing while working,
 * tilted with an amber halo when awaiting approval, and resting quietly once completed.
 */
export const SubagentCrewList = memo(function SubagentCrewList({
  agentId,
  subagents,
}: {
  hostId: string;
  agentId: string;
  subagents: readonly SessionSubagentWithAvatar[];
}) {
  const { t } = useI18n();
  if (subagents.length === 0) return null;

  return (
    <div className="flex flex-col gap-0.5 pb-1">
      {subagents.map((sa) => {
        const isActive = isSubagentActiveStatus(sa.status);
        const isAwaiting = sa.status === "awaiting_permission";
        const depth = Math.max(1, sa.depth ?? 1);
        const indentLevels = Math.min(depth - 1, 5);
        const parentCrumb =
          depth > 1 && sa.lineage && sa.lineage.length >= 2
            ? sa.lineage[sa.lineage.length - 2]?.title ?? null
            : null;
        const lineageText =
          sa.lineage && sa.lineage.length > 1
            ? sa.lineage.map((step) => step.title).join(" → ")
            : null;
        const subtitleBase = statusSubtitle(sa, t);
        const subtitle = parentCrumb ? `${subtitleBase} · ${parentCrumb}` : subtitleBase;
        const tooltip = [lineageText, sa.promptPreview || sa.title].filter(Boolean).join("\n");

        return (
          <div
            key={sa.key}
            style={indentLevels > 0 ? { paddingLeft: `${indentLevels * 14}px` } : undefined}
            className="min-w-0"
          >
            <button
              type="button"
              title={tooltip}
              onClick={() => openSubagent(sa.parentAgentId || agentId, sa)}
              className={cn(
                "flex cursor-pointer text-left min-h-[42px] w-full items-center gap-2.5 rounded-[8px] px-2 py-1.5 transition-colors",
                indentLevels > 0 && "border-l border-border-subtle/80 rounded-l-[4px]",
                isAwaiting
                  ? "bg-signal-warning-bg/75 hover:bg-signal-warning-bg"
                  : "hover:bg-accent/70",
              )}
            >
              <SubagentAvatar
                character={sa.character}
                status={sa.status}
                size={indentLevels > 0 ? "sm" : "md"}
                showStatusBadge={!isActive}
              />
              <span className="min-w-0 flex-1">
                <span className="text-ink-primary block truncate text-[13px] leading-[18px] font-medium">
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
                  {subtitle}
                </span>
              </span>
            </button>
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
  const { t } = useI18n();
  if (subagents.length === 0) return null;

  const shown = subagents.slice(0, 3);
  const overflow = Math.max(0, subagents.length - shown.length);

  return (
    <div
      data-slot="subagent-crew-mini-dock"
      className="flex shrink-0 items-center"
    >
      <div className="flex items-center -space-x-1.5">
        {shown.map((sa) => {
          const subtitle = statusSubtitle(sa, t);
          return (
            <SimpleTooltip
              key={sa.key}
              label={`${sa.title} · ${subtitle}`}
              side="bottom"
            >
              <button
                type="button"
                aria-label={`${sa.title} (${subtitle})`}
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
          );
        })}
      </div>

      {overflow > 0 && (
        <SimpleTooltip
          label={t("overview.moreSubagents", { count: overflow })}
          side="bottom"
        >
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
