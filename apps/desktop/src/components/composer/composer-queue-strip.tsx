import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  CornerDownRight,
  Folder,
  GitPullRequest,
  Globe,
  GripVertical,
  Image as ImageIcon,
  Sparkles,
  Terminal,
  X,
  type LucideIcon,
} from "lucide-react";
import { InlineDirectiveToken } from "@/components/composer/composer-directive-token";
import { DragShield } from "@/components/ui/drag-feedback";
import { SimpleTooltip } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useInlineSortable } from "@/lib/drag/use-inline-sortable";
import { useI18n } from "@/lib/i18n";
import type { PromptBadge, QueuedPromptView } from "@/lib/orbit-ipc";
import {
  getCachedPromptImage,
  loadPromptImage,
  lookupRecentSentTurnImage,
} from "@/lib/prompt-image-cache";
import {
  type QueuedPromptItem,
  composerScopeKey,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { selectIsSelectedAgentPersonalAgent } from "@/state/selectors/personal-agent-selectors";
import { cn } from "@/lib/utils";

const EMPTY_QUEUED: readonly QueuedPromptItem[] = [];
const EMPTY_SERVER_QUEUED: readonly QueuedPromptView[] = [];
const MAX_QUEUED_IMAGE_THUMBS = 3;

function resolveQueuedBadgeIcon(badge: PromptBadge): LucideIcon | null {
  if (badge.kind === "skill") return Sparkles;
  if (badge.kind === "command") return Terminal;
  if (badge.kind === "folder") return Folder;
  if (badge.kind === "pr") return GitPullRequest;
  if (badge.kind === "web") return Globe;
  return null;
}

/**
 * Compact `20×20` inline image thumbnail for queued follow-up rows.
 * Sized strictly inside the `28px` queue row so rows with and without images share identical height.
 */
const QueuedImageThumb = memo(function QueuedImageThumb({
  badge,
  displayText,
  imageIndex,
  hostId,
  agentId,
  cwd,
  overflowCount = 0,
  tooltip,
}: {
  badge: PromptBadge;
  displayText: string;
  imageIndex: number;
  hostId: string;
  agentId: string | null;
  cwd: string;
  overflowCount?: number;
  tooltip: string;
}) {
  const recent = useMemo(
    () =>
      !badge.path
        ? lookupRecentSentTurnImage(hostId, agentId, displayText, imageIndex, badge.label)
        : null,
    [badge.path, badge.label, displayText, imageIndex, hostId, agentId],
  );
  const effectivePath = badge.path?.trim() || recent?.path?.trim() || null;

  const resolveSyncUrl = (): string | null => {
    if (effectivePath && hostId) {
      const cached = getCachedPromptImage(hostId, effectivePath);
      if (cached?.status === "ready") return cached.url;
    }
    return recent?.previewUrl ?? null;
  };

  const [url, setUrl] = useState<string | null>(resolveSyncUrl);

  useEffect(() => {
    const sync = resolveSyncUrl();
    if (sync) {
      setUrl(sync);
      return;
    }
    if (!effectivePath || !hostId) {
      setUrl(null);
      return;
    }
    let cancelled = false;
    void loadPromptImage(hostId, cwd || "~", effectivePath).then((entry) => {
      if (cancelled) return;
      if (entry.status === "ready") {
        setUrl(entry.url);
      }
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostId, agentId, cwd, effectivePath, badge.label]);

  return (
    <span
      title={tooltip}
      className="bg-surface-recessed border-border-default relative inline-flex size-[20px] shrink-0 items-center justify-center overflow-hidden rounded-[5px] border"
    >
      {url ? (
        <img
          src={url}
          alt={badge.label}
          decoding="async"
          draggable={false}
          onError={() => setUrl(null)}
          className="size-full object-cover"
        />
      ) : (
        <ImageIcon className="text-ink-muted size-3 stroke-[1.7]" />
      )}
      {overflowCount > 0 && (
        <span className="tabular-nums absolute inset-0 flex items-center justify-center bg-black/55 font-mono text-[11px] leading-none font-semibold text-white">
          +{overflowCount}
        </span>
      )}
    </span>
  );
});

/**
 * Isolated Queued Follow-Up Strip docked above OmniComposer.
 * Never subscribes to `composerDrafts`, so typing triggers zero re-renders here.
 */
export const ComposerQueueStrip = memo(function ComposerQueueStrip() {
  const { t } = useI18n();
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isPersonalAgentSession = useOrbitSelector(selectIsSelectedAgentPersonalAgent);
  const localQueued = useOrbitSelector((s) => s.queuedPrompts[composerScopeKey(s)] ?? EMPTY_QUEUED);
  const hostSupportsServerQueue = useOrbitSelector((s) =>
    Boolean(s.selectedHostId && s.hostDetails[s.selectedHostId]?.host.supports_server_queue),
  );
  const serverQueued = useOrbitSelector((s) => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return EMPTY_SERVER_QUEUED;
    return (
      s.hostDetails[s.selectedHostId]?.agents.find((a) => a.agent_id === s.selectedAgentId)
        ?.queued_prompts ?? EMPTY_SERVER_QUEUED
    );
  });

  const queued = useMemo(() => {
    if (hostSupportsServerQueue) {
      return serverQueued.map((q) => ({
        id: q.queue_id,
        displayText: q.display_text,
        badges: q.badges ?? [],
      }));
    }
    return localQueued;
  }, [hostSupportsServerQueue, serverQueued, localQueued]);

  const scrollViewportRef = useRef<HTMLDivElement | null>(null);
  const queuedIds = useMemo(() => queued.map((q) => q.id), [queued]);

  const queueSortable = useInlineSortable({
    axis: "y",
    itemIds: queuedIds,
    scrollViewportRef,
    ignoreSelector: "button",
    onCommit: (fromIdx, toIdx) => {
      orbitActions.reorderQueuedPrompts(fromIdx, toIdx);
    },
  });

  if (!selectedHostId || queued.length === 0) return null;

  return (
    <>
      <div className="composer-queue-surface orbit-stage-enter overflow-hidden rounded-2xl text-[13px]">
        <ScrollArea
          className="w-full"
          viewportRef={scrollViewportRef}
          viewportClassName="max-h-[156px] overscroll-contain px-2 py-1.5"
          contentClassName="flex flex-col gap-0.5"
        >
          {queued.map((qItem, idx) => {
            const canDrag = queued.length > 1;
            const isDragged = queueSortable.sortState?.fromIndex === idx;
            const isSettling = isDragged && queueSortable.sortState?.phase === "settling";

            const imageBadges = qItem.badges.filter((b) => b.kind === "image");
            const otherBadges = qItem.badges.filter((b) => b.kind !== "image");
            const visibleImages = imageBadges.slice(0, MAX_QUEUED_IMAGE_THUMBS);
            const extraImageCount = Math.max(0, imageBadges.length - MAX_QUEUED_IMAGE_THUMBS);
            const visibleOtherBadges = otherBadges.slice(0, 2);
            const hiddenOtherBadgeCount = Math.max(
              0,
              otherBadges.length - visibleOtherBadges.length,
            );
            const trimmedDisplayText = qItem.displayText?.trim() ?? "";
            const hasDisplayText = trimmedDisplayText.length > 0;
            const imageOnlyLabel =
              !hasDisplayText && otherBadges.length === 0 && imageBadges.length > 0
                ? imageBadges.length === 1
                  ? t("composer.imageCountOne")
                  : t("composer.imageCountMany", { count: imageBadges.length })
                : "";

            return (
              <div
                key={qItem.id}
                ref={(el) => queueSortable.registerItem(qItem.id, el)}
                onPointerDown={canDrag ? (e) => queueSortable.handlePointerDown(e, idx) : undefined}
                onDragStart={(e) => e.preventDefault()}
                style={queueSortable.getNeighborStyle(idx)}
                className={cn(
                  "group/qrow flex h-[28px] min-w-0 items-center justify-between gap-2 overflow-hidden rounded-[8px] px-2 select-none [-webkit-user-drag:none]",
                  isDragged
                    ? cn("drag-lifted-surface relative z-10", !isSettling && "cursor-grabbing")
                    : cn(
                        queueSortable.isActive
                          ? "transition-transform duration-150 ease-[var(--ease-out-expo)] motion-reduce:transition-none"
                          : "hover:bg-accent/65 transition-colors",
                        canDrag && !queueSortable.isActive && "cursor-grab",
                      ),
                )}
              >
              <div className="flex min-w-0 flex-1 items-center gap-1.5 overflow-hidden">
                {canDrag ? (
                  <span className="relative inline-flex size-3.5 shrink-0 items-center justify-center">
                    <CornerDownRight
                      aria-hidden="true"
                      className={cn(
                        "text-ink-faint size-3.5 stroke-[1.8] transition-opacity duration-150",
                        isDragged ? "opacity-0" : "opacity-100 group-hover/qrow:opacity-0",
                      )}
                    />
                    <GripVertical
                      aria-hidden="true"
                      className={cn(
                        "text-ink-muted absolute inset-0 size-3.5 stroke-[1.8] transition-opacity duration-150",
                        isDragged ? "opacity-100" : "opacity-0 group-hover/qrow:opacity-100",
                      )}
                    />
                  </span>
                ) : (
                  <CornerDownRight className="text-ink-faint size-3.5 shrink-0 stroke-[1.8]" />
                )}
                {visibleImages.length > 0 && (
                  <div className="flex shrink-0 items-center gap-1">
                    {visibleImages.map((imgBadge, imgIdx) => {
                      const isLastWithOverflow =
                        imgIdx === visibleImages.length - 1 && extraImageCount > 0;
                      return (
                        <QueuedImageThumb
                          key={`qimg-${imgBadge.path ?? imgBadge.label}-${imgIdx}`}
                          badge={imgBadge}
                          displayText={trimmedDisplayText}
                          imageIndex={imgIdx}
                          hostId={selectedHostId}
                          agentId={selectedAgentId}
                          cwd={selectedCwd || "~"}
                          overflowCount={isLastWithOverflow ? extraImageCount : 0}
                          tooltip={
                            isLastWithOverflow
                              ? t("composer.imagesAttached", { count: imageBadges.length })
                              : imgBadge.label
                          }
                        />
                      );
                    })}
                  </div>
                )}
                {visibleOtherBadges.map((b, bIdx) => {
                  if (b.kind === "slash") {
                    return (
                      <InlineDirectiveToken
                        key={`${b.kind}-${bIdx}`}
                        directive={b.label}
                        size="compact"
                        showTitle
                        className="shrink-0"
                      />
                    );
                  }
                  const BadgeIcon = resolveQueuedBadgeIcon(b);
                  return (
                    <span
                      key={`${b.kind}-${bIdx}`}
                      title={b.label}
                      className={cn(
                        "bg-surface-recessed text-ink-secondary inline-flex min-w-0 shrink items-center gap-1 truncate rounded px-1.5 py-0.5 font-mono text-[11px]",
                        hasDisplayText ? "max-w-[118px]" : "max-w-[168px]",
                      )}
                    >
                      {BadgeIcon && <BadgeIcon className="text-ink-muted size-3 shrink-0 stroke-[1.75]" />}
                      <span className="truncate">{b.label}</span>
                    </span>
                  );
                })}
                {hiddenOtherBadgeCount > 0 && (
                  <span
                    title={otherBadges.map((b) => b.label).join(", ")}
                    className="bg-surface-recessed text-ink-muted shrink-0 rounded px-1.5 py-0.5 font-mono text-[11px]"
                  >
                    +{hiddenOtherBadgeCount}
                  </span>
                )}
                {hasDisplayText ? (
                  <span
                    title={qItem.displayText}
                    className="text-ink-primary min-w-[64px] flex-1 truncate"
                  >
                    {qItem.displayText}
                  </span>
                ) : imageOnlyLabel ? (
                  <span className="text-ink-secondary min-w-[48px] flex-1 truncate">
                    {imageOnlyLabel}
                  </span>
                ) : null}
              </div>

              <div
                className={cn(
                  "flex shrink-0 items-center gap-0.5 transition-opacity duration-150",
                  queueSortable.isActive && "pointer-events-none opacity-45",
                )}
              >
                {!isPersonalAgentSession && (
                  <SimpleTooltip label={t("composer.sendNowTooltip")} side="top">
                    <button
                      type="button"
                      onClick={() => void orbitActions.flushQueuedPromptImmediately(idx, qItem.id)}
                      className="text-ink-secondary hover:text-ink-primary hover:bg-accent inline-flex h-[22px] cursor-pointer items-center rounded-[6px] px-2 text-[11px] font-medium transition-colors"
                    >
                      {t("composer.steerAction")}
                    </button>
                  </SimpleTooltip>
                )}
                <SimpleTooltip label={t("composer.editQueuedTooltip")} side="top">
                  <button
                    type="button"
                    aria-label={t("composer.editQueuedAria")}
                    onClick={() => orbitActions.popQueuedToDraft(idx, qItem.id)}
                    className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex h-[22px] cursor-pointer items-center rounded-[6px] px-2 text-[11px] font-medium transition-colors"
                  >
                    {t("common.edit")}
                  </button>
                </SimpleTooltip>
                <SimpleTooltip label={t("composer.remove")} side="top">
                  <button
                    type="button"
                    aria-label={t("composer.removeQueuedAria")}
                    onClick={() => orbitActions.removeQueuedPrompt(idx, qItem.id)}
                    className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-[22px] cursor-pointer items-center justify-center rounded-[6px] transition-colors"
                  >
                    <X className="size-3.5 stroke-[1.8]" />
                  </button>
                </SimpleTooltip>
              </div>
              </div>
            );
          })}
        </ScrollArea>
      </div>
      <DragShield active={queueSortable.isDragging} cursor="grabbing" />
    </>
  );
});
