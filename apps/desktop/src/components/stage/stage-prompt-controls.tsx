import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactElement,
} from "react";
import { createPortal } from "react-dom";
import {
  AlertTriangle,
  Check,
  Copy,
  FileCode2,
  FileDiff,
  FileImage,
  FileSearch,
  FileSymlink,
  FileText,
  Film,
  FoldVertical,
  GitBranch,
  GitCompareArrows,
  GitFork,
  Layers,
  ListTodo,
  Music,
  Quote,
  Send,
  Sparkles,
  SquareTerminal,
  Undo2,
  Users,
  type LucideIcon,
} from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { Button } from "@/components/ui/button";
import { DiffStat } from "@/components/ui/diff-stat";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { openOrbitLink } from "@/lib/file-links";
import { isImeKeyEvent } from "@/lib/ime";
import { detectRichMediaKind } from "@/lib/media-utils";
import type {
  AgentCardView,
  PromptBadge,
  TimelineItemView,
  TurnFileDiffSummaryView,
} from "@/lib/orbit-ipc";
import {
  getCachedPromptImage,
  loadPromptImage,
  lookupRecentSentTurnImage,
  type PromptImageEntry,
} from "@/lib/prompt-image-cache";
import { stripWorkspacePrefix } from "@/lib/timeline-coalesce";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import {
  type PromptImagePreviewTarget,
  type RewindRequestTarget,
  type TurnTimingRecord,
  USER_PROMPT_PREVIEW_MAX_PX,
  type ViewportAnchorHoldFn,
  extractUserPromptPresentation,
  formatFullTimestampTooltip,
  formatRelativeTimeLabel,
  useSharedRelativeNow,
} from "./stage-helpers";

function resolvePromptBadgeIcon(badge: PromptBadge): LucideIcon {
  switch (badge.kind) {
    case "slash":
      if (badge.label === "/plan") return ListTodo;
      if (badge.label === "/review") return FileSearch;
      if (badge.label === "/compact") return FoldVertical;
      if (badge.label === "/orbit") return Layers;
      if (badge.label === "/orbit-handoff") return Send;
      if (badge.label === "/orbit-committee") return Users;
      if (badge.label === "/orbit-advisor") return Sparkles;
      return Sparkles;
    case "skill":
      return Sparkles;
    case "file":
      return resolveSemanticFileIcon(badge.label);
    case "git_diff":
      return /staged/i.test(badge.label) ? GitCompareArrows : FileDiff;
    case "terminal":
      return SquareTerminal;
    case "image":
      return FileImage;
    case "pdf":
      return FileText;
    case "video":
      return Film;
    case "audio":
      return Music;
    case "quote":
      return Quote;
    case "paste": {
      const cleanName = badge.label.split("·")[0]?.trim() ?? "";
      return cleanName.includes(".") ? resolveSemanticFileIcon(cleanName) : FileText;
    }
    default:
      return FileText;
  }
}

const PromptImageThumbnail = memo(function PromptImageThumbnail({
  badge,
  cleanText,
  imageIndex,
  hostId,
  agentId,
  cwd,
  onPreviewImage,
}: {
  badge: PromptBadge;
  cleanText: string;
  imageIndex: number;
  hostId: string;
  agentId: string;
  cwd: string;
  onPreviewImage: (img: PromptImagePreviewTarget) => void;
}) {
  const recent = useMemo(
    () =>
      !badge.path
        ? lookupRecentSentTurnImage(hostId, agentId || null, cleanText, imageIndex, badge.label)
        : null,
    [badge.path, badge.label, cleanText, imageIndex, hostId, agentId],
  );
  const effectivePath = badge.path?.trim() || recent?.path?.trim() || null;

  const resolveInitialState = (): {
    status: "loading" | "ready" | "fallback";
    url: string | null;
    resolvedPath: string | null;
  } => {
    if (effectivePath && hostId) {
      const cached = getCachedPromptImage(hostId, effectivePath);
      if (cached?.status === "ready") {
        return { status: "ready", url: cached.url, resolvedPath: cached.resolvedPath };
      }
      if (cached?.status === "error") {
        return recent?.previewUrl
          ? { status: "ready", url: recent.previewUrl, resolvedPath: effectivePath }
          : { status: "fallback", url: null, resolvedPath: effectivePath };
      }
      return recent?.previewUrl
        ? { status: "ready", url: recent.previewUrl, resolvedPath: effectivePath }
        : { status: "loading", url: null, resolvedPath: effectivePath };
    }
    if (recent?.previewUrl) {
      return { status: "ready", url: recent.previewUrl, resolvedPath: effectivePath };
    }
    return { status: "fallback", url: null, resolvedPath: null };
  };

  const [thumbState, setThumbState] = useState(resolveInitialState);

  useEffect(() => {
    const initial = resolveInitialState();
    setThumbState(initial);
    if (initial.status !== "loading" || !effectivePath || !hostId) {
      return;
    }
    let cancelled = false;
    void loadPromptImage(hostId, cwd || "~", effectivePath).then((entry: PromptImageEntry) => {
      if (cancelled) return;
      if (entry.status === "ready") {
        setThumbState({ status: "ready", url: entry.url, resolvedPath: entry.resolvedPath });
      } else {
        setThumbState({ status: "fallback", url: null, resolvedPath: effectivePath });
      }
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostId, agentId, cwd, badge.path, badge.label, effectivePath]);

  if (thumbState.status === "loading") {
    return (
      <div
        title={badge.label}
        className="border-border bg-muted/55 size-[56px] shrink-0 animate-pulse rounded-[12px] border"
      />
    );
  }

  if (thumbState.status === "ready" && thumbState.url) {
    const imgUrl = thumbState.url;
    const imgPath = thumbState.resolvedPath ?? effectivePath;
    return (
      <button
        type="button"
        title={`Preview ${badge.label}`}
        onClick={() =>
          onPreviewImage({
            url: imgUrl,
            label: badge.label,
            path: imgPath,
            hostId,
            cwd,
          })
        }
        className="group/img border-border bg-muted/40 relative size-[56px] shrink-0 cursor-pointer overflow-hidden rounded-[12px] border shadow-[0_1px_3px_rgba(0,0,0,0.06)] transition-[border-color,box-shadow] duration-150 hover:border-foreground/25 dark:shadow-[0_1px_6px_rgba(0,0,0,0.26)]"
      >
        <img
          src={imgUrl}
          alt={badge.label}
          decoding="async"
          draggable={false}
          onError={() =>
            setThumbState({ status: "fallback", url: null, resolvedPath: imgPath })
          }
          className="size-full object-cover transition-transform duration-200 ease-[var(--ease-out-expo)] group-hover/img:scale-[1.03]"
        />
      </button>
    );
  }

  return (
    <span
      title={badge.label}
      className="text-foreground/85 inline-flex items-center gap-1.5 rounded-xl border border-black/[0.075] bg-black/[0.03] px-2.5 py-1 text-[12px] dark:border-white/[0.10] dark:bg-white/[0.055]"
    >
      <FileImage className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
      <span className="max-w-[180px] truncate font-medium">{badge.label}</span>
    </span>
  );
});

export const PromptImageLightbox = memo(function PromptImageLightbox({
  target,
  onClose,
}: {
  target: PromptImagePreviewTarget | null;
  onClose: () => void;
}) {
  return (
    <Dialog open={target !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="w-fit max-w-[92vw] gap-3 p-4 sm:p-5">
        <DialogHeader className="gap-1 pr-8">
          <DialogTitle className="truncate font-mono text-[13.5px] font-medium">
            {target?.label ?? ""}
          </DialogTitle>
          {target?.path && (
            <DialogDescription className="truncate font-mono text-[11.5px]">
              {target.path}
            </DialogDescription>
          )}
        </DialogHeader>

        {target && (
          <div className="flex flex-col items-end gap-3">
            <div className="border-border bg-muted/30 flex max-h-[76vh] max-w-[86vw] items-center justify-center overflow-hidden rounded-xl border p-1.5">
              <img
                src={target.url}
                alt={target.label}
                className="max-h-[72vh] max-w-[84vw] rounded-lg object-contain"
              />
            </div>

            {target.path && target.hostId && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => {
                  const p = target.path;
                  const h = target.hostId;
                  const c = target.cwd;
                  onClose();
                  if (p) {
                    orbitActions.openFileInSidePanel(p, { hostId: h, cwd: c });
                  }
                }}
                className="gap-1.5 text-[12px]"
              >
                <FileSymlink className="size-3.5" />
                Open in Side Panel
              </Button>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
});

/**
 * Zero-rest-overhead hover pill tooltip:
 * Avoids mounting hundreds of persistent tooltip context/portal providers per conversation turn.
 * Reads trigger geometry only after hover/focus delay elapses and mounts a single lightweight portal.
 */
export const HoverPillTooltip = memo(function HoverPillTooltip({
  label,
  delay = 220,
  tabularNums = false,
  renderTrigger,
}: {
  label: string;
  delay?: number;
  tabularNums?: boolean;
  renderTrigger: (handlers: {
    onMouseEnter: (e: React.MouseEvent<HTMLElement>) => void;
    onMouseLeave: () => void;
    onFocus: (e: React.FocusEvent<HTMLElement>) => void;
    onBlur: () => void;
  }) => ReactElement;
}) {
  const [anchorPos, setAnchorPos] = useState<{ x: number; y: number } | null>(null);
  const [shiftX, setShiftX] = useState(0);
  const timerRef = useRef<number | null>(null);
  const tooltipRef = useRef<HTMLDivElement | null>(null);

  const clearTimer = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  useEffect(() => clearTimer, [clearTimer]);

  useLayoutEffect(() => {
    if (!anchorPos || !tooltipRef.current) {
      setShiftX(0);
      return;
    }
    const half = tooltipRef.current.getBoundingClientRect().width / 2;
    const pad = 8;
    const vw = window.innerWidth;
    let nextShift = 0;
    if (anchorPos.x + half > vw - pad) {
      nextShift = vw - pad - (anchorPos.x + half);
    } else if (anchorPos.x - half < pad) {
      nextShift = pad - (anchorPos.x - half);
    }
    setShiftX(nextShift);
  }, [anchorPos, label]);

  const scheduleShow = useCallback(
    (el: HTMLElement | null) => {
      clearTimer();
      if (!el || !label) return;
      timerRef.current = window.setTimeout(() => {
        timerRef.current = null;
        if (!el.isConnected) return;
        const rect = el.getBoundingClientRect();
        setShiftX(0);
        setAnchorPos({ x: rect.left + rect.width / 2, y: rect.top - 6 });
      }, delay);
    },
    [clearTimer, delay, label],
  );

  const hide = useCallback(() => {
    clearTimer();
    setAnchorPos(null);
    setShiftX(0);
  }, [clearTimer]);

  return (
    <>
      {renderTrigger({
        onMouseEnter: (e) => scheduleShow(e.currentTarget),
        onMouseLeave: hide,
        onFocus: (e) => scheduleShow(e.currentTarget),
        onBlur: hide,
      })}
      {anchorPos &&
        label &&
        createPortal(
          <div
            ref={tooltipRef}
            role="tooltip"
            style={{
              left: `${anchorPos.x + shiftX}px`,
              top: `${anchorPos.y}px`,
            }}
            className="pointer-events-none fixed z-50 -translate-x-1/2 -translate-y-full whitespace-nowrap"
          >
            <div
              className={cn(
                "bg-elevated text-foreground rounded-full border border-black/[0.08] px-2.5 py-0.5 text-[11.5px] leading-normal font-normal whitespace-nowrap shadow-[0_4px_14px_rgba(0,0,0,0.08)] select-none dark:border-white/[0.11] dark:shadow-[0_4px_16px_rgba(0,0,0,0.36)]",
                tabularNums && "tabular-nums px-3 py-1 text-[12px]",
              )}
            >
              {label}
            </div>
          </div>,
          document.body,
        )}
    </>
  );
});

/** Shared pill-styled Copy icon button (`13px` icon in `22×22px` hit target, balanced with `14.5px` prose). */
export const CopyActionButton = memo(function CopyActionButton({
  copied,
  label,
  onClick,
  className,
}: {
  copied: boolean;
  label: string;
  onClick: () => void;
  className?: string;
}) {
  return (
    <HoverPillTooltip
      label={copied ? "Copied" : label}
      delay={240}
      renderTrigger={(handlers) => (
        <button
          type="button"
          aria-label={copied ? "Copied" : label}
          onClick={onClick}
          {...handlers}
          className={cn(
            "text-muted-foreground hover:text-foreground hover:bg-accent/65 orbit-press relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md",
            className,
          )}
        >
          <Copy
            className={cn(
              "size-[13px] stroke-[1.8] transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)]",
              copied ? "scale-50 -rotate-12 opacity-0" : "scale-100 rotate-0 opacity-100",
            )}
          />
          <Check
            className={cn(
              "absolute size-[13px] stroke-[2] text-emerald-600 transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)] dark:text-emerald-400",
              copied ? "scale-100 rotate-0 opacity-100" : "scale-50 rotate-12 opacity-0",
            )}
          />
        </button>
      )}
    />
  );
});

/** Semantic Undo / Rewind icon button matching `CopyActionButton` (`13px` icon in `22×22px` hit target). */
export const UndoActionButton = memo(function UndoActionButton({
  label,
  onClick,
  className,
}: {
  label: string;
  onClick: () => void;
  className?: string;
}) {
  return (
    <HoverPillTooltip
      label={label}
      delay={240}
      renderTrigger={(handlers) => (
        <button
          type="button"
          aria-label={label}
          onClick={onClick}
          {...handlers}
          className={cn(
            "text-muted-foreground hover:text-foreground hover:bg-accent/65 orbit-press relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md",
            className,
          )}
        >
          <Undo2 className="size-[13px] stroke-[1.8]" />
        </button>
      )}
    />
  );
});

/**
 * Restrained Fork icon button matching `CopyActionButton` / `UndoActionButton` (`13px` icon in `22×22px` hit target).
 * Directly forks in place for non-git directories; opens a tiny 2-option popover when worktree branching is available.
 */
export const ForkActionButton = memo(function ForkActionButton({
  canUseWorktree,
  onFork,
  align = "start",
  className,
}: {
  canUseWorktree: boolean;
  onFork: (worktree: boolean) => void;
  align?: "start" | "end";
  className?: string;
}) {
  const [menuOpen, setMenuOpen] = useState(false);

  if (!canUseWorktree) {
    return (
      <HoverPillTooltip
        label="Fork from here"
        delay={240}
        renderTrigger={(handlers) => (
          <button
            type="button"
            aria-label="Fork from here"
            onClick={() => onFork(false)}
            {...handlers}
            className={cn(
              "text-muted-foreground hover:text-foreground hover:bg-accent/65 orbit-press relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md",
              className,
            )}
          >
            <GitFork className="size-[13px] stroke-[1.8]" />
          </button>
        )}
      />
    );
  }

  return (
    <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
      <HoverPillTooltip
        label={menuOpen ? "" : "Fork…"}
        delay={240}
        renderTrigger={(handlers) => (
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label="Fork from here"
              data-fork-menu-open={menuOpen ? "true" : undefined}
              {...handlers}
              className={cn(
                "text-muted-foreground hover:text-foreground hover:bg-accent/65 orbit-press relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md",
                menuOpen && "bg-accent/65 text-foreground",
                className,
              )}
            >
              <GitFork className="size-[13px] stroke-[1.8]" />
            </button>
          </DropdownMenuTrigger>
        )}
      />
      <DropdownMenuContent align={align} sideOffset={6} className="min-w-[168px]">
        <DropdownMenuGroup>
          <DropdownMenuItem onSelect={() => onFork(false)}>
            <span className="flex items-center gap-2 whitespace-nowrap">
              <GitFork className="text-ink-muted size-[13px] stroke-[1.8]" />
              <span>Fork here</span>
            </span>
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => onFork(true)}>
            <span className="flex items-center gap-2 whitespace-nowrap">
              <GitBranch className="text-ink-muted size-[13px] stroke-[1.8]" />
              <span>Fork in new worktree</span>
            </span>
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
});

/** Auto-refreshing relative timestamp (`just now` -> `2m ago`) with full-date floating pill tooltip on hover. */
export const RelativeTimestampPill = memo(function RelativeTimestampPill({
  timestampMs,
}: {
  timestampMs: number;
}) {
  const nowMs = useSharedRelativeNow(timestampMs);
  const relativeText = formatRelativeTimeLabel(timestampMs, nowMs);
  const fullText = formatFullTimestampTooltip(timestampMs);

  return (
    <HoverPillTooltip
      label={fullText}
      delay={140}
      tabularNums
      renderTrigger={(handlers) => (
        <span
          {...handlers}
          className="text-muted-foreground hover:text-foreground/85 tabular-nums inline-flex cursor-default items-center rounded px-1 py-0.5 text-[12px] leading-none transition-colors select-none"
        >
          {relativeText}
        </span>
      )}
    />
  );
});

/**
 * Orbit Native User Prompt Bubble:
 * - Crisp cool-neutral Orbit surface aligned flush right.
 * - Renders image attachments as right-aligned rounded thumbnails cleanly above the text bubble.
 * - Renders semantic context & slash workflow tokens (`/plan`, `/review`, `@file`, `Diff`)
 *   without exposing hidden wire scaffolding.
 * - Long messages preview smoothly with a bottom fade gradient; click anywhere on the bubble to
 *   expand, and click outside while expanded to collapse back.
 * - Quiet bottom-right hover toolbar (`timestamp` + `Copy` + `Undo to this point`) with zero layout shift.
 */
export const UserPromptBubble = memo(function UserPromptBubble({
  item,
  hostId,
  agentId,
  cwd,
  holdViewport,
  animateEntrance = false,
  onRequestRewind,
  onPreviewImage,
}: {
  item: Extract<TimelineItemView, { item_type: "user_prompt" }>;
  hostId: string;
  agentId: string;
  cwd?: string;
  holdViewport: ViewportAnchorHoldFn;
  animateEntrance?: boolean;
  onRequestRewind?: (seq: number, promptText: string, badges: readonly PromptBadge[]) => void;
  onPreviewImage: (img: PromptImagePreviewTarget) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [overflowing, setOverflowing] = useState(false);
  const [copied, setCopied] = useState(false);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const bubbleRef = useRef<HTMLDivElement | null>(null);
  const bodyRef = useRef<HTMLDivElement | null>(null);

  const { cleanText, badges, sanitizedLegacy } = useMemo(
    () => extractUserPromptPresentation(item),
    [item],
  );

  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    const measure = () => {
      const nextOverflow = el.scrollHeight > USER_PROMPT_PREVIEW_MAX_PX + 10;
      setOverflowing((prev) => (prev === nextOverflow ? prev : nextOverflow));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    let rafId = 0;
    const ro = new ResizeObserver(() => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        measure();
      });
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [cleanText, badges.length]);

  useEffect(() => {
    if (!expanded) return;
    const handleClickOutside = (event: MouseEvent) => {
      if (!(event.target instanceof Element)) return;
      if (containerRef.current?.contains(event.target)) return;
      if (event.target.closest('[data-slot="dialog-content"], .orbit-dialog-overlay')) {
        return;
      }
      const sel = window.getSelection();
      if (
        sel &&
        !sel.isCollapsed &&
        sel.toString().trim().length > 0 &&
        bubbleRef.current &&
        sel.anchorNode &&
        bubbleRef.current.contains(sel.anchorNode)
      ) {
        return;
      }
      holdViewport(bubbleRef.current, () => setExpanded(false), "top");
    };
    document.addEventListener("click", handleClickOutside);
    return () => {
      document.removeEventListener("click", handleClickOutside);
    };
  }, [expanded, holdViewport]);

  const copyPayload = cleanText || badges.map((b) => b.label).join(" ");

  const handleCopy = useCallback(() => {
    void navigator.clipboard.writeText(copyPayload).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    });
  }, [copyPayload]);

  const handleRewindClick = useCallback(() => {
    onRequestRewind?.(item.seq, cleanText, badges);
  }, [onRequestRewind, item.seq, cleanText, badges]);

  const handleBubbleClick = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!overflowing || expanded) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("a, button")) return;
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed && sel.toString().trim().length > 0) return;
    holdViewport(bubbleRef.current, () => setExpanded(true), "top");
  };

  const imageBadges = useMemo(
    () => badges.filter((b) => b.kind === "image"),
    [badges],
  );
  const slashBadge = useMemo(
    () => badges.find((b) => b.kind === "slash") ?? null,
    [badges],
  );
  const attachmentBadges = useMemo(
    () => badges.filter((b) => b.kind !== "slash" && b.kind !== "image"),
    [badges],
  );
  const hasBubbleContent =
    attachmentBadges.length > 0 || Boolean(slashBadge) || cleanText.length > 0;
  const SlashIcon = slashBadge ? resolvePromptBadgeIcon(slashBadge) : null;
  const slashLabel = slashBadge ? slashBadge.label.replace(/^\//, "") : "";
  const promptTimestampMs =
    typeof item.timestamp_ms === "number" &&
    Number.isFinite(item.timestamp_ms) &&
    item.timestamp_ms > 0
      ? item.timestamp_ms
      : null;

  return (
    <div
      ref={containerRef}
      className={cn(
        "group/user flex flex-col items-end",
        animateEntrance && "orbit-bubble-enter",
      )}
    >
      {imageBadges.length > 0 && (
        <div
          className={cn(
            "flex max-w-[80%] flex-wrap justify-end gap-1.5 select-none",
            hasBubbleContent && "mb-1.5",
          )}
        >
          {imageBadges.map((badge, idx) => (
            <PromptImageThumbnail
              key={`uimg-${badge.path ?? badge.label}-${idx}`}
              badge={badge}
              cleanText={cleanText}
              imageIndex={idx}
              hostId={hostId}
              agentId={agentId}
              cwd={cwd || "~"}
              onPreviewImage={onPreviewImage}
            />
          ))}
        </div>
      )}

      {hasBubbleContent && (
        <div
          ref={bubbleRef}
          onClick={handleBubbleClick}
          className={cn(
            "prompt-bubble-surface text-foreground relative min-w-0 max-w-[80%] overflow-hidden rounded-[20px] px-4 py-2.5",
            overflowing && !expanded && "cursor-pointer",
          )}
        >
          <div
            ref={bodyRef}
            style={
              !expanded && overflowing
                ? { maxHeight: `${USER_PROMPT_PREVIEW_MAX_PX}px`, overflow: "hidden" }
                : undefined
            }
            className="relative space-y-1.5"
          >
            {attachmentBadges.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5 select-none">
                {attachmentBadges.map((badge, idx) => {
                  const BadgeIcon = resolvePromptBadgeIcon(badge);
                  const mediaKind =
                    badge.kind === "pdf" || badge.kind === "audio" || badge.kind === "video"
                      ? badge.kind
                      : badge.kind === "file"
                        ? detectRichMediaKind(badge.path || badge.label)
                        : null;
                  const isMono =
                    badge.kind === "file" ||
                    badge.kind === "quote" ||
                    badge.kind === "skill" ||
                    mediaKind !== null;
                  const fileTarget = badge.path?.trim() || badge.label;
                  const isClickableMediaBadge = mediaKind !== null && fileTarget.trim().length > 0;
                  const isClickableFileBadge =
                    badge.kind !== "skill" && isMono && fileTarget.trim().length > 0;
                  if (isClickableMediaBadge && mediaKind) {
                    return (
                      <button
                        key={`${badge.kind}-${badge.label}-${idx}`}
                        type="button"
                        title={`Preview ${badge.label}`}
                        onClick={(e) => {
                          e.stopPropagation();
                          orbitActions.openMediaPreview({
                            kind: mediaKind,
                            filename: badge.label,
                            path: fileTarget,
                            hostId,
                            cwd: cwd || "~",
                          });
                        }}
                        className="text-foreground/85 hover:text-foreground inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-black/[0.075] bg-black/[0.03] px-2 py-0.5 text-[12px] transition-colors hover:border-black/[0.14] hover:bg-black/[0.055] dark:border-white/[0.10] dark:bg-white/[0.055] dark:hover:border-white/[0.18] dark:hover:bg-white/[0.095]"
                      >
                        <BadgeIcon className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
                        <span className="font-mono leading-normal font-medium hover:underline">
                          {badge.label}
                        </span>
                      </button>
                    );
                  }
                  if (isClickableFileBadge) {
                    return (
                      <button
                        key={`${badge.kind}-${badge.label}-${idx}`}
                        type="button"
                        title={`Open ${badge.label} in Side Panel`}
                        onClick={(e) => {
                          e.stopPropagation();
                          openOrbitLink(fileTarget, { hostId, cwd });
                        }}
                        className="text-foreground/85 hover:text-foreground inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-black/[0.075] bg-black/[0.03] px-2 py-0.5 text-[12px] transition-colors hover:border-black/[0.14] hover:bg-black/[0.055] dark:border-white/[0.10] dark:bg-white/[0.055] dark:hover:border-white/[0.18] dark:hover:bg-white/[0.095]"
                      >
                        <BadgeIcon className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
                        <span className="font-mono leading-normal font-medium hover:underline">
                          {badge.label}
                        </span>
                      </button>
                    );
                  }
                  return (
                    <span
                      key={`${badge.kind}-${badge.label}-${idx}`}
                      className="text-foreground/85 inline-flex items-center gap-1.5 rounded-lg border border-black/[0.075] bg-black/[0.03] px-2 py-0.5 text-[12px] dark:border-white/[0.10] dark:bg-white/[0.055]"
                    >
                      <BadgeIcon className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
                      <span className={cn("leading-normal font-medium", isMono && "font-mono")}>
                        {badge.label}
                      </span>
                    </span>
                  );
                })}
              </div>
            )}

            {slashBadge && SlashIcon ? (
              <p className="selectable-text text-foreground text-[14.5px] leading-[1.68] whitespace-pre-wrap">
                <span
                  className={cn(
                    "mr-1.5 inline-flex items-center gap-1 rounded-md border px-1.5 py-[1px] align-baseline font-mono text-[12px] leading-[18px] font-medium select-none",
                    slashBadge.label === "/plan"
                      ? "bg-signal-info-bg border-signal-info-border text-signal-info"
                      : "bg-surface-recessed border-border-subtle text-ink-primary",
                  )}
                >
                  <SlashIcon className="size-[12.5px] shrink-0 stroke-[1.8]" />
                  <span>/{slashLabel}</span>
                </span>
                {cleanText}
              </p>
            ) : (
              cleanText.length > 0 &&
              (sanitizedLegacy ? (
                <p className="selectable-text text-foreground text-[14.5px] leading-[1.68] whitespace-pre-wrap">
                  {cleanText}
                </p>
              ) : (
                <MarkdownBlocksView blocks={item.blocks} bubble />
              ))
            )}
          </div>

          {!expanded && overflowing && (
            <div
              aria-hidden="true"
              className="pointer-events-none absolute inset-x-0 bottom-0 h-14 rounded-b-[19px]"
              style={{
                background:
                  "linear-gradient(to top, var(--surface-elevated) 14%, color-mix(in oklab, var(--surface-elevated) 82%, transparent) 52%, transparent 100%)",
              }}
            />
          )}
        </div>
      )}

      <div className="mt-3 flex h-[22px] shrink-0 items-center justify-end gap-1 pr-1 whitespace-nowrap opacity-0 transition-opacity duration-150 group-focus-within/user:opacity-100 group-hover/user:opacity-100">
        {promptTimestampMs !== null && (
          <RelativeTimestampPill timestampMs={promptTimestampMs} />
        )}
        <CopyActionButton copied={copied} label="Copy" onClick={handleCopy} />
        {onRequestRewind && item.seq > 0 && (
          <UndoActionButton label="Undo to this point" onClick={handleRewindClick} />
        )}
      </div>
    </div>
  );
});

/** Turn-end aggregated diff summary pill (`N files changed · +A -D · Review ▸`). */
export const TurnChangesBar = memo(function TurnChangesBar({
  diffs,
  cwd,
}: {
  diffs: TurnFileDiffSummaryView[];
  cwd: string;
}) {
  if (diffs.length === 0) return null;
  const totalAdd = diffs.reduce((acc, d) => acc + d.additions, 0);
  const totalDel = diffs.reduce((acc, d) => acc + d.deletions, 0);
  const primaryPath = diffs[0]?.path ?? null;

  const label =
    diffs.length === 1 && primaryPath
      ? `Edited ${stripWorkspacePrefix(primaryPath, cwd)}`
      : `${diffs.length} files changed`;

  return (
    <div className="pt-1.5">
      <button
        type="button"
        onClick={() => orbitActions.openInspectorForFile(primaryPath, "session_turns")}
        className="border-border-subtle bg-surface-raised hover:bg-accent/70 hover:border-border-default inline-flex max-w-full cursor-pointer items-center gap-2 rounded-full border px-3 py-1 text-[12px] transition-colors"
      >
        <FileCode2 className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
        <span className="text-ink-primary truncate font-medium">{label}</span>
        <DiffStat additions={totalAdd} deletions={totalDel} />
        <span className="text-ink-tertiary shrink-0 text-[11px]">Review ▸</span>
      </button>
    </div>
  );
});

/** Super subtle, restrained 1-line provenance chip at the top of a forked thread. */
export const ForkedFromBanner = memo(function ForkedFromBanner({
  hostId,
  agent,
}: {
  hostId: string;
  agent: AgentCardView | null;
}) {
  const sourceId = agent?.forked_from_agent_id ?? null;
  const parentAgent = useOrbitSelector((s) =>
    sourceId
      ? (s.hostDetails[hostId]?.agents.find((a) => a.agent_id === sourceId) ?? null)
      : null,
  );
  const customTitle = useOrbitSelector((s) =>
    sourceId ? s.threadCustomTitles[`${hostId}::${sourceId}`] : undefined,
  );
  const isLocallyArchived = useOrbitSelector((s) =>
    sourceId ? Boolean(s.archivedThreads[`${hostId}::${sourceId}`]) : false,
  );

  if (!sourceId) return null;

  const parentTitle =
    customTitle?.trim() ||
    parentAgent?.title?.trim() ||
    (sourceId.length > 14 ? `${sourceId.slice(0, 12)}…` : sourceId);
  const canJump = Boolean(
    parentAgent && !parentAgent.archived_at_rfc3339 && !isLocallyArchived,
  );

  return (
    <div className="mx-auto flex w-full max-w-[var(--orbit-stage-max-w,740px)] items-center px-[10px] pt-2 pb-0.5">
      {canJump ? (
        <button
          type="button"
          onClick={() => orbitActions.selectAgent(hostId, sourceId)}
          className="text-ink-tertiary hover:text-ink-primary inline-flex max-w-full cursor-pointer items-center gap-1.5 rounded-[6px] py-0.5 text-[11px] leading-tight transition-colors"
        >
          <GitFork className="size-3 shrink-0 stroke-[1.75] opacity-70" />
          <span className="truncate">
            Forked from <span className="text-ink-primary font-medium">{parentTitle}</span>
          </span>
        </button>
      ) : (
        <span className="text-ink-muted inline-flex max-w-full items-center gap-1.5 py-0.5 text-[11px] leading-tight">
          <GitFork className="size-3 shrink-0 stroke-[1.75] opacity-60" />
          <span className="truncate">Forked from {parentTitle}</span>
        </span>
      )}
    </div>
  );
});

export const RewindTurnDialog = memo(function RewindTurnDialog({
  target,
  isAgentRunning,
  submitting,
  onOpenChange,
  onConfirm,
}: {
  target: RewindRequestTarget | null;
  isAgentRunning: boolean;
  submitting: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (restoreFiles: boolean) => void;
}) {
  const [restoreFiles, setRestoreFiles] = useState(true);
  const files = target?.editedFiles ?? [];
  const hasFiles = files.length > 0;

  useEffect(() => {
    if (target) {
      setRestoreFiles(target.editedFiles.length > 0);
    }
  }, [target]);

  useEffect(() => {
    if (!target || submitting) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (isImeKeyEvent(e)) return;
      if (e.key !== "Enter" || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      const activeTag = (document.activeElement as HTMLElement | null)?.tagName;
      if (activeTag === "BUTTON" || activeTag === "INPUT") return;
      e.preventDefault();
      onConfirm(hasFiles ? restoreFiles : false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [target, submitting, hasFiles, restoreFiles, onConfirm]);

  const basenameCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const f of files) {
      counts.set(f.basename, (counts.get(f.basename) ?? 0) + 1);
    }
    return counts;
  }, [files]);

  const effectiveRestore = hasFiles && restoreFiles;

  return (
    <Dialog open={target !== null} onOpenChange={(open) => !submitting && onOpenChange(open)}>
      <DialogContent className="max-w-[440px] gap-3.5 p-5">
        <DialogHeader className="gap-1.5 pr-6">
          <DialogTitle className="text-[15.5px] font-semibold">Confirm Undo</DialogTitle>
          <DialogDescription className="text-[13px] leading-snug">
            {hasFiles
              ? "Confirming this undo action will make the following changes:"
              : `Confirming this undo action will rewind ${
                  target && target.affectedTurnsCount > 1
                    ? `${target.affectedTurnsCount} turns`
                    : "this turn"
                } and restore the prompt to the composer.`}
          </DialogDescription>
        </DialogHeader>

        {hasFiles && (
          <div className="min-w-0 space-y-2.5">
            <label className="inline-flex cursor-pointer items-center gap-2.5 select-none">
              <button
                type="button"
                role="checkbox"
                aria-checked={restoreFiles}
                disabled={submitting}
                onClick={() => setRestoreFiles((v) => !v)}
                className={cn(
                  "flex size-4 shrink-0 cursor-pointer items-center justify-center rounded-[4px] border transition-colors disabled:opacity-60",
                  restoreFiles
                    ? "border-primary bg-primary text-primary-foreground"
                    : "border-muted-foreground/45 bg-background hover:border-foreground/50",
                )}
              >
                {restoreFiles && <Check className="size-3 stroke-[2.5]" />}
              </button>
              <span className="text-foreground text-[13.5px] font-medium">
                Revert file changes
              </span>
            </label>

            <div className="relative -mx-5 min-w-0">
              <div
                className={cn(
                  "orbit-native-scroll max-h-[216px] space-y-1.5 overflow-y-auto overscroll-contain px-5 transition-opacity duration-150",
                  !restoreFiles && "opacity-45",
                )}
              >
                {files.map((file) => {
                  const FileIcon = resolveSemanticFileIcon(file.displayPath);
                  const showRelPath = (basenameCounts.get(file.basename) ?? 0) > 1;
                  return (
                    <div
                      key={file.displayPath}
                      title={file.displayPath}
                      className="flex min-w-0 items-center gap-2 py-0.5 text-[13px]"
                    >
                      <FileIcon className="text-muted-foreground/85 size-3.5 shrink-0 stroke-[1.75]" />
                      <span
                        className={cn(
                          "text-foreground/90 min-w-0 truncate",
                          !restoreFiles && "line-through",
                        )}
                      >
                        {showRelPath ? file.displayPath : file.basename}
                      </span>
                      {(file.additions > 0 || file.deletions > 0) && (
                        <DiffStat additions={file.additions} deletions={file.deletions} />
                      )}
                    </div>
                  );
                })}
              </div>
              {files.length > 7 && (
                <div
                  className="pointer-events-none absolute inset-x-0 bottom-0 h-7"
                  style={{
                    background:
                      "linear-gradient(to top, var(--surface-elevated) 0%, transparent 100%)",
                  }}
                />
              )}
            </div>
          </div>
        )}

        {isAgentRunning && (
          <div className="flex items-center gap-2 rounded-lg border border-amber-500/25 bg-amber-500/[0.07] px-3 py-2 text-[12px] text-amber-700 dark:text-amber-300">
            <AlertTriangle className="size-3.5 shrink-0" />
            <span>Active turn will be stopped first.</span>
          </div>
        )}

        <div className="flex items-center justify-end gap-2 pt-1">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={submitting}
            onClick={() => onOpenChange(false)}
          >
            Cancel
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={submitting}
            onClick={() => onConfirm(effectiveRestore)}
            className="gap-1.5"
          >
            {submitting && <OrbitSpinner size={13} />}
            <span>Confirm</span>
            {!submitting && <span className="text-[11px] opacity-75">↵</span>}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
});

/**
 * Completed Turn Footer (`[Copy · Fork · 2m ago]`):
 * Hidden while a turn is active since live elapsed duration lives inline in `TurnActivityDisclosure`.
 */
export const TurnFooter = memo(function TurnFooter({
  isTurnActive,
  timing,
  completedAtMs,
  replyText,
  forkUpToSeq,
  isLastTurn = false,
  canForkWorktree = false,
  onRequestFork,
}: {
  isTurnActive: boolean;
  timing?: TurnTimingRecord;
  completedAtMs: number | null;
  replyText: string | null;
  forkUpToSeq?: number | null;
  isLastTurn?: boolean;
  canForkWorktree?: boolean;
  onRequestFork?: (upToSeq: number | null, worktree: boolean) => void;
}) {
  const [copied, setCopied] = useState(false);

  const resolvedCompletedAtMs = timing?.completedAtMs ?? completedAtMs;
  const showFork = Boolean(
    onRequestFork &&
      (isLastTurn || (forkUpToSeq !== null && forkUpToSeq !== undefined && forkUpToSeq > 0)),
  );

  const handleCopy = useCallback(() => {
    if (!replyText) return;
    void navigator.clipboard.writeText(replyText).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    });
  }, [replyText]);

  const handleFork = useCallback(
    (worktree: boolean) => {
      onRequestFork?.(isLastTurn ? null : (forkUpToSeq ?? null), worktree);
    },
    [onRequestFork, isLastTurn, forkUpToSeq],
  );

  const hasReply = Boolean(replyText && replyText.trim().length > 0);
  if (isTurnActive || !hasReply) {
    return null;
  }

  return (
    <div className="flex h-[22px] items-center gap-1">
      {replyText && (
        <CopyActionButton copied={copied} label="Copy" onClick={handleCopy} className="-ml-1" />
      )}

      {showFork && onRequestFork && (
        <ForkActionButton
          canUseWorktree={canForkWorktree}
          align="start"
          onFork={handleFork}
          className={replyText ? undefined : "-ml-1"}
        />
      )}

      {resolvedCompletedAtMs !== null && (
        <RelativeTimestampPill timestampMs={resolvedCompletedAtMs} />
      )}
    </div>
  );
});
