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
  ChevronRight,
  Copy,
  Crosshair,
  FileCode2,
  FileDiff,
  FileImage,
  FileText,
  Film,
  Folder,
  GitBranch,
  GitCompareArrows,
  GitFork,
  GitPullRequest,
  Globe,
  Music,
  Quote,
  Sparkles,
  SquareTerminal,
  Undo2,
  type LucideIcon,
} from "lucide-react";
import { DiffFilesViewer } from "@/components/diff-viewer";
import { InlineDirectiveToken } from "@/components/composer/composer-directive-token";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { resolveSlashCommandIcon } from "@/components/composer/slash-menu";
import { MarkdownBlocksView, useClipboardCopy } from "@/components/markdown-blocks";
import { ImageLightbox, type ImageLightboxItem } from "@/components/viewers/image-lightbox";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { openOrbitLink, prefetchOrbitLink } from "@/lib/file-links";
import {
  cancelHoverPrefetch,
  scheduleHoverPrefetch,
  shouldPrefetchOnPointerDown,
} from "@/lib/file-prefetch";
import { useI18n } from "@/lib/i18n";
import { isImeKeyEvent } from "@/lib/ime";
import { detectRichMediaKind } from "@/lib/media-utils";
import {
  orbitIpc,
  type AgentCardView,
  type DiffFileView,
  type PromptBadge,
  type RewindPreviewView,
  type TimelineItemView,
  type TurnFileDiffSummaryView,
} from "@/lib/orbit-ipc";
import {
  getCachedPromptImage,
  loadPromptImage,
  lookupRecentSentTurnImage,
  type PromptImageEntry,
} from "@/lib/prompt-image-cache";
import { stripWorkspacePrefix } from "@/lib/timeline-coalesce";
import { normalizeDirectiveLabel } from "@/lib/slash-commands";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import {
  type PromptImagePreviewTarget,
  type RewindRequestTarget,
  USER_PROMPT_PREVIEW_MAX_PX,
  type ViewportAnchorHoldFn,
  extractUserPromptPresentation,
  formatFullTimestampTooltip,
  formatRelativeTimeLabel,
  mayUserPromptOverflow,
  observeSharedBubbleResize,
  useSharedRelativeNow,
} from "./stage-helpers";

function resolvePromptBadgeIcon(badge: PromptBadge): LucideIcon {
  switch (badge.kind) {
    case "slash":
      return resolveSlashCommandIcon(normalizeDirectiveLabel(badge.label));
    case "skill":
      return Sparkles;
    case "command":
      return SquareTerminal;
    case "folder":
      return Folder;
    case "pr":
      return GitPullRequest;
    case "web":
      return Globe;
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
    case "element":
      return Crosshair;
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
  allImageBadges,
  cleanText,
  imageIndex,
  hostId,
  agentId,
  cwd,
  onPreviewImage,
}: {
  badge: PromptBadge;
  allImageBadges: readonly PromptBadge[];
  cleanText: string;
  imageIndex: number;
  hostId: string;
  agentId: string;
  cwd: string;
  onPreviewImage: (img: PromptImagePreviewTarget) => void;
}) {
  const { t } = useI18n();
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
        className="border-border-subtle bg-surface-recessed size-[56px] shrink-0 animate-pulse rounded-[12px] border"
      />
    );
  }

  if (thumbState.status === "ready" && thumbState.url) {
    const imgUrl = thumbState.url;
    const imgPath = thumbState.resolvedPath ?? effectivePath;
    return (
      <button
        type="button"
        title={t("stage.preview", { name: badge.label })}
        onClick={() => {
          const gallery: ImageLightboxItem[] = [];
          let galleryIndex = 0;
          for (let i = 0; i < allImageBadges.length; i += 1) {
            const b = allImageBadges[i];
            if (i === imageIndex) {
              galleryIndex = gallery.length;
              gallery.push({
                url: imgUrl,
                label: badge.label,
                path: imgPath,
                hostId,
                cwd,
              });
              continue;
            }
            const siblingRecent = !b.path
              ? lookupRecentSentTurnImage(hostId, agentId || null, cleanText, i, b.label)
              : null;
            const siblingPath = b.path?.trim() || siblingRecent?.path?.trim() || null;
            const cachedSibling =
              siblingPath && hostId ? getCachedPromptImage(hostId, siblingPath) : null;
            const siblingUrl =
              cachedSibling?.status === "ready"
                ? cachedSibling.url
                : siblingRecent?.previewUrl ?? null;
            if (siblingUrl) {
              gallery.push({
                url: siblingUrl,
                label: b.label,
                path: cachedSibling?.status === "ready" ? cachedSibling.resolvedPath : siblingPath,
                hostId,
                cwd,
              });
            }
          }
          onPreviewImage({
            url: imgUrl,
            label: badge.label,
            path: imgPath,
            hostId,
            cwd,
            gallery: gallery.length > 1 ? gallery : undefined,
            galleryIndex,
          });
        }}
        className="group/img border-border-subtle bg-surface-recessed relative size-[56px] shrink-0 cursor-pointer overflow-hidden rounded-[12px] border shadow-sm transition-[border-color,box-shadow] duration-150 hover:border-border-strong"
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
      className="text-ink-secondary inline-flex items-center gap-1.5 rounded-xl border border-border-subtle bg-surface-recessed px-2.5 py-1 text-[13px] leading-[18px]"
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
  const items = useMemo<readonly ImageLightboxItem[]>(() => {
    if (!target) return [];
    if (target.gallery && target.gallery.length > 0) {
      return target.gallery;
    }
    return [
      {
        url: target.url,
        label: target.label,
        path: target.path,
        hostId: target.hostId,
        cwd: target.cwd,
      },
    ];
  }, [target]);

  const handleOpenInSidePanel = useCallback(
    (item: ImageLightboxItem) => {
      const p = item.path;
      const h = item.hostId;
      const c = item.cwd;
      onClose();
      if (p && h) {
        orbitActions.openFileInSidePanel(p, { hostId: h, cwd: c });
      }
    },
    [onClose],
  );

  return (
    <ImageLightbox
      open={target !== null}
      items={items}
      initialIndex={target?.galleryIndex ?? 0}
      onClose={onClose}
      onOpenInSidePanel={handleOpenInSidePanel}
    />
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
  const [entering, setEntering] = useState(false);
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

  useEffect(() => {
    if (!anchorPos || !entering) return;
    let raf2 = 0;
    const raf1 = window.requestAnimationFrame(() => {
      raf2 = window.requestAnimationFrame(() => {
        setEntering(false);
      });
    });
    return () => {
      window.cancelAnimationFrame(raf1);
      if (raf2) window.cancelAnimationFrame(raf2);
    };
  }, [anchorPos, entering]);

  const scheduleShow = useCallback(
    (el: HTMLElement | null) => {
      clearTimer();
      if (!el || !label) return;
      timerRef.current = window.setTimeout(() => {
        timerRef.current = null;
        if (!el.isConnected) return;
        const rect = el.getBoundingClientRect();
        const prefersReducedMotion =
          typeof window !== "undefined" &&
          window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
        setShiftX(0);
        setEntering(!prefersReducedMotion);
        setAnchorPos({ x: rect.left + rect.width / 2, y: rect.top - 6 });
      }, delay);
    },
    [clearTimer, delay, label],
  );

  const hide = useCallback(() => {
    clearTimer();
    setAnchorPos(null);
    setEntering(false);
    setShiftX(0);
  }, [clearTimer]);

  useEffect(() => {
    if (!anchorPos) return;
    const options = { capture: true, passive: true };
    window.addEventListener("scroll", hide, options);
    window.addEventListener("wheel", hide, options);
    window.addEventListener("resize", hide, options);
    return () => {
      window.removeEventListener("scroll", hide, options);
      window.removeEventListener("wheel", hide, options);
      window.removeEventListener("resize", hide, options);
    };
  }, [anchorPos, hide]);

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
              data-side="top"
              data-starting-style={entering ? "" : undefined}
              style={{ transformOrigin: "bottom center" }}
              className={cn(
                "orbit-tooltip-content bg-surface-overlay text-ink-primary rounded-full border border-border-subtle px-2.5 py-0.5 text-[11px] leading-normal font-normal whitespace-nowrap shadow-md select-none",
                tabularNums && "tabular-nums px-3 py-1",
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

/** Shared pill-styled Copy icon button (`13px` icon in `22×22px` hit target, balanced with `15px` prose). */
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
  const { t } = useI18n();
  const actionLabel = copied ? t("common.copied") : label;
  return (
    <HoverPillTooltip
      label={actionLabel}
      delay={240}
      renderTrigger={(handlers) => (
        <button
          type="button"
          aria-label={actionLabel}
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
              "absolute size-[13px] stroke-[2] text-signal-success transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)]",
              copied ? "scale-100 rotate-0 opacity-100" : "scale-50 rotate-12 opacity-0",
            )}
          />
        </button>
      )}
    />
  );
});

export function StageCopyButton({ text, className }: { text: string; className?: string }) {
  const { t } = useI18n();
  const { copied, copy } = useClipboardCopy(text);
  return (
    <CopyActionButton label={t("common.copy")} copied={copied} onClick={copy} className={className} />
  );
}

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
  const { t } = useI18n();
  const [menuOpen, setMenuOpen] = useState(false);

  if (!canUseWorktree) {
    return (
      <HoverPillTooltip
        label={t("stage.forkFromHere")}
        delay={240}
        renderTrigger={(handlers) => (
          <button
            type="button"
            aria-label={t("stage.forkFromHere")}
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
        label={menuOpen ? "" : t("stage.forkMenu")}
        delay={240}
        renderTrigger={(handlers) => (
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label={t("stage.forkFromHere")}
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
              <span>{t("sidebar.forkHere")}</span>
            </span>
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => onFork(true)}>
            <span className="flex items-center gap-2 whitespace-nowrap">
              <GitBranch className="text-ink-muted size-[13px] stroke-[1.8]" />
              <span>{t("sidebar.forkInNewWorktree")}</span>
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
  const { locale } = useI18n();
  const nowMs = useSharedRelativeNow(timestampMs);
  const relativeText = formatRelativeTimeLabel(timestampMs, nowMs, locale);
  const fullText = formatFullTimestampTooltip(timestampMs, locale);

  return (
    <HoverPillTooltip
      label={fullText}
      delay={140}
      tabularNums
      renderTrigger={(handlers) => (
        <span
          {...handlers}
          className="text-muted-foreground hover:text-ink-secondary tabular-nums inline-flex cursor-default items-center rounded px-1 py-0.5 text-[11px] leading-none transition-colors select-none"
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
  const { t, locale } = useI18n();
  const imagesRef = useRef<HTMLDivElement | null>(null);
  const actionsRef = useRef<HTMLDivElement | null>(null);
  const bubbleRef = useRef<HTMLDivElement | null>(null);
  const bodyRef = useRef<HTMLDivElement | null>(null);

  const { cleanText, badges, sanitizedLegacy } = useMemo(
    () => extractUserPromptPresentation(item, locale),
    [item, locale],
  );

  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    if (!mayUserPromptOverflow(cleanText, badges.length)) {
      setOverflowing((prev) => (prev ? false : prev));
      return;
    }
    const measure = () => {
      const nextOverflow = el.scrollHeight > USER_PROMPT_PREVIEW_MAX_PX + 10;
      setOverflowing((prev) => (prev === nextOverflow ? prev : nextOverflow));
    };
    measure();
    return observeSharedBubbleResize(el, measure);
  }, [cleanText, badges.length]);

  useEffect(() => {
    if (!expanded) return;
    const handleClickOutside = (event: MouseEvent) => {
      if (!(event.target instanceof Element)) return;
      if (bubbleRef.current?.contains(event.target) || imagesRef.current?.contains(event.target) ||
          actionsRef.current?.contains(event.target)) return;
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
      holdViewport(bubbleRef.current, () => setExpanded(false), "top", { intent: "collapse" });
    };
    document.addEventListener("click", handleClickOutside);
    return () => {
      document.removeEventListener("click", handleClickOutside);
    };
  }, [expanded, holdViewport]);

  const copyPayload = cleanText || badges.map((b) => b.label).join(" ");

  const handleRewindClick = useCallback(() => {
    onRequestRewind?.(item.seq, cleanText, badges);
  }, [onRequestRewind, item.seq, cleanText, badges]);

  const handleBubbleClick = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!overflowing || expanded) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("a, button")) return;
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed && sel.toString().trim().length > 0) return;
    holdViewport(bubbleRef.current, () => setExpanded(true), "top", { intent: "expand" });
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
  const slashDirectiveLabel = slashBadge ? normalizeDirectiveLabel(slashBadge.label) : "";
  const hasBubbleContent =
    attachmentBadges.length > 0 || slashDirectiveLabel.length > 0 || cleanText.length > 0;
  const promptTimestampMs =
    typeof item.timestamp_ms === "number" &&
    Number.isFinite(item.timestamp_ms) &&
    item.timestamp_ms > 0
      ? item.timestamp_ms
      : null;

  return (
    <div
      className={cn(
        "group/user flex flex-col items-end",
        animateEntrance && "orbit-bubble-enter",
      )}
    >
      {imageBadges.length > 0 && (
        <div
          ref={imagesRef}
          className={cn(
            "flex max-w-[80%] flex-wrap justify-end gap-1.5 select-none",
            hasBubbleContent && "mb-1.5",
          )}
        >
          {imageBadges.map((badge, idx) => (
            <PromptImageThumbnail
              key={`uimg-${badge.path ?? badge.label}-${idx}`}
              badge={badge}
              allImageBadges={imageBadges}
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
          data-orbit-disclosure-root=""
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
                    badge.kind === "command" ||
                    badge.kind === "folder" ||
                    badge.kind === "pr" ||
                    badge.kind === "web" ||
                    mediaKind !== null;
                  const fileTarget = badge.path?.trim() || badge.label;
                  const isClickableMediaBadge = mediaKind !== null && fileTarget.trim().length > 0;
                  const isClickableFileBadge =
                    (badge.kind === "file" || badge.kind === "quote") &&
                    fileTarget.trim().length > 0;
                  if (isClickableMediaBadge && mediaKind) {
                    return (
                      <button
                        key={`${badge.kind}-${badge.label}-${idx}`}
                        type="button"
                        title={t("stage.preview", { name: badge.label })}
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
                        className="text-ink-secondary hover:text-foreground inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-border-subtle bg-surface-recessed px-2 py-0.5 text-[13px] leading-[18px] transition-colors hover:border-border-strong hover:bg-accent"
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
                        title={t("stage.openTargetInPanel", { target: badge.label })}
                        onMouseEnter={() =>
                          scheduleHoverPrefetch(() =>
                            prefetchOrbitLink(fileTarget, { hostId, cwd, intent: "hover" }),
                          )
                        }
                        onMouseLeave={cancelHoverPrefetch}
                        onPointerDown={(e) => {
                          if (shouldPrefetchOnPointerDown(e)) {
                            cancelHoverPrefetch();
                            prefetchOrbitLink(fileTarget, { hostId, cwd, intent: "press" });
                          }
                        }}
                        onClick={(e) => {
                          e.stopPropagation();
                          cancelHoverPrefetch();
                          openOrbitLink(fileTarget, { hostId, cwd });
                        }}
                        className="text-ink-secondary hover:text-foreground inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-border-subtle bg-surface-recessed px-2 py-0.5 text-[13px] leading-[18px] transition-colors hover:border-border-strong hover:bg-accent"
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
                      className="text-ink-secondary inline-flex items-center gap-1.5 rounded-lg border border-border-subtle bg-surface-recessed px-2 py-0.5 text-[13px] leading-[18px]"
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

            {slashDirectiveLabel ? (
              <p className="selectable-text text-foreground text-[15px] leading-[1.65] whitespace-pre-wrap">
                <InlineDirectiveToken
                  directive={slashDirectiveLabel}
                  size="bubble"
                  showTitle
                  className={cleanText.length > 0 ? "mr-[8px]" : undefined}
                />
                {cleanText}
              </p>
            ) : (
              cleanText.length > 0 &&
              (sanitizedLegacy ? (
                <p className="selectable-text text-foreground text-[15px] leading-[1.65] whitespace-pre-wrap">
                  {cleanText}
                </p>
              ) : (
                <MarkdownBlocksView blocks={item.blocks} bubble />
              ))
            )}
          </div>

          {overflowing && (
            <div
              aria-hidden="true"
              className={cn(
                "pointer-events-none absolute inset-x-0 bottom-0 h-14 rounded-b-[19px] transition-opacity duration-150 motion-reduce:transition-none",
                expanded ? "opacity-0" : "opacity-100",
              )}
              style={{
                background:
                  "linear-gradient(to top, var(--surface-raised) 14%, color-mix(in oklab, var(--surface-raised) 82%, transparent) 52%, transparent 100%)",
              }}
            />
          )}
        </div>
      )}

      <div ref={actionsRef} className="mt-1 flex h-[20px] shrink-0 items-center justify-end gap-1 pr-2 whitespace-nowrap opacity-0 transition-opacity duration-150 group-focus-within/user:opacity-100 group-hover/user:opacity-100 [@media(hover:none)]:opacity-100 motion-reduce:transition-none">
        {promptTimestampMs !== null && (
          <RelativeTimestampPill timestampMs={promptTimestampMs} />
        )}
        <StageCopyButton text={copyPayload} />
        {onRequestRewind && item.seq > 0 && (
          <UndoActionButton label={t("stage.undoToHere")} onClick={handleRewindClick} />
        )}
      </div>
    </div>
  );
});

/** Turn-end summary linking the changed files to Diff Review. */
export const TurnChangesBar = memo(function TurnChangesBar({
  diffs,
  cwd,
}: {
  diffs: TurnFileDiffSummaryView[];
  cwd: string;
}) {
  const { t } = useI18n();
  if (diffs.length === 0) return null;
  const totalAdd = diffs.reduce((acc, d) => acc + d.additions, 0);
  const totalDel = diffs.reduce((acc, d) => acc + d.deletions, 0);
  const primaryPath = diffs[0]?.path ?? null;

  const label =
    diffs.length === 1 && primaryPath
      ? t("stage.editedPath", { path: stripWorkspacePrefix(primaryPath, cwd) })
      : t(diffs.length === 1 ? "stage.fileChanged" : "stage.filesChanged", { count: diffs.length });

  return (
    <div>
      <button
        type="button"
        onClick={() => orbitActions.openInspectorForFile(primaryPath, "session_turns")}
        className="border-border-subtle bg-surface-raised/85 hover:bg-accent/70 hover:border-border-default inline-flex h-[32px] max-w-full cursor-pointer items-center gap-2 rounded-lg border px-2.5 text-[13px] leading-[18px] transition-colors"
      >
        <FileCode2 className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
        <span className="text-ink-primary truncate font-medium">{label}</span>
        <DiffStat additions={totalAdd} deletions={totalDel} />
        <span className="text-ink-tertiary inline-flex shrink-0 items-center gap-0.5 text-[11px]">
          {t("overview.review")}
          <ChevronRight className="size-3" />
        </span>
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
  const { t } = useI18n();
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
  const [forkPrefix, forkSuffix] = t("stage.forkedFrom").split("{title}");
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
            {forkPrefix}<span className="text-ink-primary font-medium">{parentTitle}</span>{forkSuffix}
          </span>
        </button>
      ) : (
        <span className="text-ink-muted inline-flex max-w-full items-center gap-1.5 py-0.5 text-[11px] leading-tight">
          <GitFork className="size-3 shrink-0 stroke-[1.75] opacity-60" />
          <span className="truncate">{t("stage.forkedFrom", { title: parentTitle })}</span>
        </span>
      )}
    </div>
  );
});

interface RewindSelectableFile {
  path: string;
  displayPath: string;
  basename: string;
  status: string;
  additions: number;
  deletions: number;
  hasExternalConflict: boolean;
  diffFile: DiffFileView | null;
}

export const RewindTurnDialog = memo(function RewindTurnDialog({
  hostId,
  agentId,
  target,
  isAgentRunning,
  submitting,
  onOpenChange,
  onConfirm,
}: {
  hostId?: string | null;
  agentId?: string | null;
  target: RewindRequestTarget | null;
  isAgentRunning: boolean;
  submitting: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (restoreFiles: boolean, restorePaths?: string[] | null) => void;
}) {
  const { t } = useI18n();
  const [restoreFiles, setRestoreFiles] = useState(true);
  const [preview, setPreview] = useState<RewindPreviewView | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(() => new Set());
  const [inspectedPath, setInspectedPath] = useState<string | null>(null);

  // Fetch file-level reverse diff preview & external conflict report from daemon on open
  useEffect(() => {
    if (!target) {
      setPreview(null);
      setPreviewLoading(false);
      setSelectedPaths(new Set());
      setInspectedPath(null);
      return;
    }
    const fallbackPaths = target.editedFiles.map((f) => f.displayPath);
    setRestoreFiles(fallbackPaths.length > 0);
    setSelectedPaths(new Set(fallbackPaths));
    setInspectedPath(null);

    if (!hostId || !agentId) return;
    let cancelled = false;
    setPreviewLoading(true);
    setPreview(null);

    void orbitIpc
      .previewRewindAgent(hostId, agentId, target.seq)
      .then((res) => {
        if (cancelled) return;
        setPreview(res);
        if (res.files.length > 0) {
          setRestoreFiles(true);
          setSelectedPaths(new Set(res.files.map((f) => f.path)));
        }
      })
      .catch(() => {
        // Older daemon or unsupported checkpoint — keep local editedFiles fallback cleanly
      })
      .finally(() => {
        if (!cancelled) setPreviewLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [hostId, agentId, target]);

  const files = useMemo<RewindSelectableFile[]>(() => {
    if (preview && preview.files.length > 0) {
      return preview.files.map((f) => {
        const norm = f.path.replace(/\\/g, "/");
        const basename = norm.split("/").pop() || norm;
        return {
          path: f.path,
          displayPath: f.path,
          basename,
          status: f.action,
          additions: f.additions,
          deletions: f.deletions,
          hasExternalConflict: f.has_external_conflict,
          diffFile: f.diff_file ?? null,
        };
      });
    }
    return (target?.editedFiles ?? []).map((f) => ({
      path: f.displayPath,
      displayPath: f.displayPath,
      basename: f.basename,
      status: "restore",
      additions: f.additions,
      deletions: f.deletions,
      hasExternalConflict: false,
      diffFile: null,
    }));
  }, [preview, target]);

  const hasFiles = files.length > 0;
  const externalConflicts = preview?.external_conflicts ?? [];
  const selectedCount = useMemo(
    () => files.reduce((acc, f) => acc + (selectedPaths.has(f.path) ? 1 : 0), 0),
    [files, selectedPaths],
  );
  const inspectedFile = useMemo(
    () => (inspectedPath ? files.find((f) => f.path === inspectedPath) ?? null : null),
    [files, inspectedPath],
  );

  const handleToggleMasterRestore = useCallback(
    (checked: boolean) => {
      setRestoreFiles(checked);
      if (checked && selectedPaths.size === 0 && files.length > 0) {
        setSelectedPaths(new Set(files.map((f) => f.path)));
      }
    },
    [files, selectedPaths.size],
  );

  const handleToggleFilePath = useCallback((path: string, checked: boolean) => {
    setSelectedPaths((prev) => {
      const next = new Set(prev);
      if (checked) next.add(path);
      else next.delete(path);
      setRestoreFiles(next.size > 0);
      return next;
    });
  }, []);

  const triggerConfirm = useCallback(() => {
    if (!hasFiles || !restoreFiles || selectedCount === 0) {
      onConfirm(false, null);
      return;
    }
    const restorePaths =
      selectedCount < files.length
        ? files.filter((f) => selectedPaths.has(f.path)).map((f) => f.path)
        : null;
    onConfirm(true, restorePaths);
  }, [files, hasFiles, onConfirm, restoreFiles, selectedCount, selectedPaths]);

  useEffect(() => {
    if (!target || submitting) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (isImeKeyEvent(e)) return;
      if (e.key !== "Enter" || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      if (document.activeElement?.closest('button, input, [role="checkbox"]')) return;
      e.preventDefault();
      triggerConfirm();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [target, submitting, triggerConfirm]);

  const basenameCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const f of files) {
      counts.set(f.basename, (counts.get(f.basename) ?? 0) + 1);
    }
    return counts;
  }, [files]);

  return (
    <Dialog open={target !== null} onOpenChange={(open) => !submitting && onOpenChange(open)}>
      <DialogContent
        className={cn(
          "gap-3.5 p-5 transition-[max-width] duration-150",
          inspectedFile?.diffFile ? "max-w-[740px]" : "max-w-[500px]",
        )}
      >
        <DialogHeader className="gap-1.5 pr-6">
          <DialogTitle className="text-[15px] font-semibold">{t("stage.confirmUndo")}</DialogTitle>
          <DialogDescription className="text-[13px] leading-snug">
            {target && target.affectedTurnsCount > 1
              ? t("stage.undoTurns", { count: target.affectedTurnsCount })
              : t("stage.undoTurn")}
            {hasFiles && <> {t("stage.undoFiles")}</>}
          </DialogDescription>
        </DialogHeader>

        {previewLoading && !hasFiles && (
          <div className="text-ink-tertiary flex items-center gap-2 py-2 text-[12px]">
            <OrbitSpinner size={13} />
            <span>{t("stage.rewindPreviewLoading")}</span>
          </div>
        )}

        {externalConflicts.length > 0 && restoreFiles && (
          <div className="flex items-start gap-2 rounded-lg border border-signal-warning-border bg-signal-warning-bg px-3 py-2 text-[12px] leading-[1.45] text-signal-warning">
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
            <span>
              {t("stage.rewindConflictWarning", { count: externalConflicts.length })}
            </span>
          </div>
        )}

        {hasFiles && (
          <div className="flex min-w-0 flex-col gap-2.5">
            <div className="flex items-center justify-between gap-2">
              <label className="inline-flex cursor-pointer items-center gap-2.5 select-none">
                <Checkbox
                  checked={restoreFiles && selectedCount > 0}
                  onCheckedChange={handleToggleMasterRestore}
                  disabled={submitting}
                  aria-label={t("stage.revertFiles")}
                />
                <span className="text-foreground text-[13px] font-medium">
                  {t("stage.revertFiles")}
                </span>
              </label>
              <div className="flex items-center gap-2 text-[11px] text-ink-tertiary tabular-nums">
                {previewLoading && <OrbitSpinner size={11} />}
                <span>
                  {t("stage.rewindSelectedCount", {
                    selected: restoreFiles ? selectedCount : 0,
                    total: files.length,
                  })}
                </span>
              </div>
            </div>

            <div className="relative -mx-5 min-w-0">
              <div
                className={cn(
                  "orbit-native-scroll max-h-[216px] flex flex-col gap-1 overflow-y-auto overscroll-contain px-5 transition-opacity duration-150",
                  !restoreFiles && "opacity-45",
                )}
              >
                {files.map((file) => {
                  const FileIcon = resolveSemanticFileIcon(file.displayPath);
                  const showRelPath = (basenameCounts.get(file.basename) ?? 0) > 1;
                  const checked = restoreFiles && selectedPaths.has(file.path);
                  const isInspected = inspectedPath === file.path;
                  return (
                    <div
                      key={file.path}
                      className={cn(
                        "flex min-w-0 items-center gap-2 rounded-[6px] px-1.5 py-1 text-[13px] transition-colors",
                        isInspected ? "bg-accent/80" : "hover:bg-accent/45",
                      )}
                    >
                      <Checkbox
                        checked={checked}
                        disabled={submitting}
                        aria-label={file.displayPath}
                        onCheckedChange={(val) => handleToggleFilePath(file.path, val)}
                      />
                      <button
                        type="button"
                        disabled={!file.diffFile}
                        title={
                          file.diffFile
                            ? t("stage.rewindInspectDiffHint", { path: file.displayPath })
                            : file.displayPath
                        }
                        onClick={() =>
                          setInspectedPath((cur) => (cur === file.path ? null : file.path))
                        }
                        className={cn(
                          "flex min-w-0 flex-1 items-center gap-2 text-left",
                          file.diffFile ? "cursor-pointer" : "cursor-default",
                        )}
                      >
                        <FileIcon className="text-muted-foreground/85 size-3.5 shrink-0 stroke-[1.75]" />
                        <span
                          className={cn(
                            "text-foreground/90 min-w-0 flex-1 truncate",
                            !checked && "line-through opacity-60",
                          )}
                        >
                          {showRelPath ? file.displayPath : file.basename}
                        </span>
                        {file.hasExternalConflict && (
                          <span
                            title={t("stage.rewindFileConflictBadge")}
                            className="bg-signal-warning-bg text-signal-warning border-signal-warning-border inline-flex shrink-0 items-center gap-0.5 rounded-[5px] border px-1.5 py-0.25 text-[10.5px] font-medium"
                          >
                            <AlertTriangle className="size-2.5" />
                            <span>{t("stage.rewindConflictShort")}</span>
                          </span>
                        )}
                        {(file.additions > 0 || file.deletions > 0) && (
                          <DiffStat additions={file.additions} deletions={file.deletions} />
                        )}
                        {file.diffFile && (
                          <ChevronRight
                            className={cn(
                              "size-3 shrink-0 text-ink-muted transition-transform",
                              isInspected && "rotate-90 text-ink-primary",
                            )}
                          />
                        )}
                      </button>
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

            {inspectedFile?.diffFile && (
              <div className="border-border-subtle bg-surface-base max-h-[260px] overflow-auto rounded-lg border p-2">
                <DiffFilesViewer
                  files={[inspectedFile.diffFile]}
                  layout="unified"
                  virtualized={false}
                />
              </div>
            )}
          </div>
        )}

        {isAgentRunning && (
          <div className="flex items-center gap-2 rounded-lg border border-signal-warning-border bg-signal-warning-bg px-3 py-2 text-[13px] leading-[1.45] text-signal-warning">
            <AlertTriangle className="size-3.5 shrink-0" />
            <span>{t("stage.stopActiveTurn")}</span>
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
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={submitting}
            onClick={triggerConfirm}
            className="gap-1.5"
          >
            {submitting && <OrbitSpinner size={13} />}
            <span>{t("common.confirm")}</span>
            {!submitting && <span className="text-[11px] opacity-75">↵</span>}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
});

/**
 * Completed Turn Footer (`[Copy · Fork · Collapse · 2m ago]`):
 * Hidden while a turn is active since live elapsed duration lives inline in `TurnActivityDisclosure`.
 */
export const TurnFooter = memo(function TurnFooter({
  completedAtMs,
  replyText,
  forkUpToSeq,
  showFork,
  isLastTurn,
  hasChangesBar = false,
  canForkWorktree = false,
  onRequestFork,
}: {
  completedAtMs: number | null;
  replyText: string | null;
  forkUpToSeq: number | null;
  showFork: boolean;
  isLastTurn?: boolean;
  hasChangesBar?: boolean;
  canForkWorktree?: boolean;
  onRequestFork?: (upToSeq: number | null, worktree: boolean) => void;
}) {
  const handleFork = useCallback(
    (worktree: boolean) => {
      onRequestFork?.(isLastTurn ? null : forkUpToSeq, worktree);
    },
    [onRequestFork, isLastTurn, forkUpToSeq],
  );

  return (
    <div className={cn(
      "flex items-center gap-1",
      isLastTurn
        ? cn(hasChangesBar ? "mt-[10px]" : "mt-[8px]", "h-[22px]")
        : cn(
            hasChangesBar ? "mt-[10px]" : "mt-[6px]",
            'h-[20px] opacity-0 transition-opacity duration-150 group-hover/turn:opacity-100 group-focus-within/turn:opacity-100 has-[[data-fork-menu-open="true"]]:opacity-100 [@media(hover:none)]:opacity-100 motion-reduce:transition-none [&_button]:h-[20px]',
          ),
      // The non-virtualized subagent panel renders turn sections as direct siblings.
      isLastTurn === undefined && "group-last/turn:opacity-100",
    )}>
      {replyText && (
        <StageCopyButton text={replyText} className="-ml-1" />
      )}

      {showFork && onRequestFork && (
        <ForkActionButton
          canUseWorktree={canForkWorktree}
          align="start"
          onFork={handleFork}
          className={replyText ? undefined : "-ml-1"}
        />
      )}

      {completedAtMs !== null && (
        <RelativeTimestampPill timestampMs={completedAtMs} />
      )}
    </div>
  );
});
