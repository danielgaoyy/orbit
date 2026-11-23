import { Suspense, lazy, memo, useEffect, useMemo, useRef, useState } from "react";
import { Check, X } from "lucide-react";
import type { ResolvedSubagentStatus } from "@/lib/subagents";
import {
  type SubagentCharacterSpec,
  resolveSubagentCharacter,
} from "@/lib/subagent-avatar";
import { SUBAGENT_AVATAR_PORTRAITS } from "@/components/ui/subagent-avatar-portraits";
import { useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const LazyLiveSubagentAvatar = lazy(() => import("@/components/ui/subagent-avatar-live"));

export type SubagentAvatarSize = "xs" | "sm" | "md" | "lg";

const SIZE_PX: Record<SubagentAvatarSize, number> = {
  xs: 18,
  sm: 20,
  md: 24,
  lg: 28,
};

function useAvatarMotionAllowed(): boolean {
  const [allowed, setAllowed] = useState(() => {
    if (typeof window === "undefined" || typeof document === "undefined") return false;
    const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
    return !reduced && document.visibilityState === "visible";
  });

  useEffect(() => {
    if (typeof window === "undefined" || typeof document === "undefined") return;
    const mq = window.matchMedia?.("(prefers-reduced-motion: reduce)") ?? null;
    const sync = () => {
      const reduced = mq?.matches ?? false;
      setAllowed(!reduced && document.visibilityState === "visible");
    };
    sync();
    document.addEventListener("visibilitychange", sync);
    mq?.addEventListener?.("change", sync);
    return () => {
      document.removeEventListener("visibilitychange", sync);
      mq?.removeEventListener?.("change", sync);
    };
  }, []);

  return allowed;
}

export interface SubagentAvatarProps {
  /** Pre-assigned collision-free character spec (falls back to deterministic `seedKey`) */
  character?: SubagentCharacterSpec | null;
  /** Stable identity key used when `character` is not pre-assigned */
  seedKey?: string;
  /** Lifecycle status controlling whether the 3D character animates or rests */
  status?: ResolvedSubagentStatus;
  size?: SubagentAvatarSize;
  /** Show tiny status corner badge (check on completed, x on failed/canceled) */
  showStatusBadge?: boolean;
  className?: string;
}

/**
 * Living OneWorks 3D Subagent portrait:
 * - Resting (`completed` / `failed` / `canceled`) or off-screen portraits render an instant
 *   pre-baked 128x128 transparent portrait (<0.05ms, zero Marching Tetrahedra CPU compilation,
 *   zero inline SVG polygon DOM bloat) with live theme-matched circular backdrops.
 * - Active (`running` / `awaiting_permission`) portraits paint frame 0 instantaneously with the
 *   identical static portrait and seamlessly upgrade on the next animation frame (while in view)
 *   to the lazily-loaded live animated OneWorks 3D `<Avatar>`.
 * - Automatically pauses & tears down live animation on completion, window hide, or
 *   `prefers-reduced-motion: reduce`.
 */
export const SubagentAvatar = memo(function SubagentAvatar({
  character,
  seedKey,
  status = "completed",
  size = "md",
  showStatusBadge = false,
  className,
}: SubagentAvatarProps) {
  const resolvedTheme = useOrbitSelector((s) => s.resolvedTheme, "preferences");
  const motionAllowed = useAvatarMotionAllowed();
  const rootRef = useRef<HTMLSpanElement | null>(null);

  const spec = useMemo(
    () => character ?? resolveSubagentCharacter(seedKey ?? "subagent"),
    [character, seedKey],
  );

  const staticPortraitUrl = SUBAGENT_AVATAR_PORTRAITS[spec.id] ?? null;
  const isWorking = status === "running";
  const isAwaiting = status === "awaiting_permission";
  const wantsLiveAnimation = motionAllowed && (isWorking || isAwaiting);

  // Defer live 3D mount by 1 animation frame and gate on viewport visibility so thread switches
  // paint frame 0 at 60fps and off-screen running subagents never burn CPU.
  const [liveMounted, setLiveMounted] = useState(false);

  useEffect(() => {
    if (!wantsLiveAnimation) {
      setLiveMounted(false);
      return;
    }

    let cancelled = false;
    let rafId: number | null = null;
    const scheduleMount = () => {
      if (cancelled || rafId !== null) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = null;
        if (!cancelled) setLiveMounted(true);
      });
    };

    const el = rootRef.current;
    if (el && typeof IntersectionObserver !== "undefined") {
      const observer = new IntersectionObserver(
        (entries) => {
          const visible = entries.some((entry) => entry.isIntersecting);
          if (visible) {
            scheduleMount();
          } else {
            if (rafId !== null) {
              window.cancelAnimationFrame(rafId);
              rafId = null;
            }
            setLiveMounted(false);
          }
        },
        { rootMargin: "64px" },
      );
      observer.observe(el);
      return () => {
        cancelled = true;
        if (rafId !== null) window.cancelAnimationFrame(rafId);
        observer.disconnect();
      };
    }

    scheduleMount();
    return () => {
      cancelled = true;
      if (rafId !== null) window.cancelAnimationFrame(rafId);
    };
  }, [wantsLiveAnimation]);

  const px = SIZE_PX[size];
  const badgeSize = size === "lg" ? "size-[11px]" : "size-[10px]";
  const backdropColor = resolvedTheme === "dark" ? spec.darkBg : spec.lightBg;

  const staticPortraitEl = staticPortraitUrl ? (
    <span
      style={{ backgroundColor: backdropColor }}
      className="block size-full rounded-full"
    >
      <img
        src={staticPortraitUrl}
        alt=""
        aria-hidden="true"
        draggable={false}
        decoding="async"
        className="pointer-events-none block size-full select-none object-cover"
      />
    </span>
  ) : null;

  const shouldRenderLive = (wantsLiveAnimation && liveMounted) || !staticPortraitUrl;

  return (
    <span
      ref={rootRef}
      data-slot="subagent-avatar"
      data-status={status}
      data-character={spec.id}
      style={{ width: px, height: px }}
      className={cn("relative inline-flex shrink-0 items-center justify-center select-none", className)}
    >
      <span
        data-attention={isAwaiting ? "true" : undefined}
        className={cn(
          "orbit-avatar-frame orbit-avatar-noninteractive block size-full transition-shadow duration-200",
          status === "canceled" && "opacity-75 grayscale-[35%]",
        )}
      >
        {shouldRenderLive ? (
          <Suspense fallback={staticPortraitEl}>
            <LazyLiveSubagentAvatar
              spec={spec}
              resolvedTheme={resolvedTheme}
              status={status}
              shouldAnimate={wantsLiveAnimation}
            />
          </Suspense>
        ) : (
          staticPortraitEl
        )}
      </span>

      {showStatusBadge && status === "completed" && (
        <span
          aria-hidden="true"
          className={cn(
            "bg-signal-success ring-surface-base absolute -right-[2px] -bottom-[2px] flex items-center justify-center rounded-full text-white ring-[1.5px]",
            badgeSize,
          )}
        >
          <Check className="size-[7px] stroke-[3]" />
        </span>
      )}

      {showStatusBadge && status === "failed" && (
        <span
          aria-hidden="true"
          className={cn(
            "bg-signal-danger ring-surface-base absolute -right-[2px] -bottom-[2px] flex items-center justify-center rounded-full text-white ring-[1.5px]",
            badgeSize,
          )}
        >
          <X className="size-[7px] stroke-[3]" />
        </span>
      )}
    </span>
  );
});
