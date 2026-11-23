import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Avatar, type AvatarHandle } from "@oneworks/avatar-react/renderer";
import { Check, X } from "lucide-react";
import type { ResolvedSubagentStatus } from "@/lib/subagents";
import {
  SUBAGENT_AWAITING_CLIP,
  SUBAGENT_WORKING_CLIP,
  type SubagentCharacterSpec,
  buildSubagentAvatarDefinition,
  resolveSubagentCharacter,
} from "@/lib/subagent-avatar";
import { useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

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
 * - Animates cheerfully while `running` (bouncy head bob + look-around)
 * - Tilts inquisitively with an amber halo when `awaiting_permission`
 * - Immediately stops & resets upright on `completed` / `failed` / `canceled` (zero background rAF)
 * - Automatically pauses on window hide or `prefers-reduced-motion: reduce`
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
  const handleRef = useRef<AvatarHandle | null>(null);

  const spec = useMemo(
    () => character ?? resolveSubagentCharacter(seedKey ?? "subagent"),
    [character, seedKey],
  );

  const definition = useMemo(
    () => buildSubagentAvatarDefinition(spec, resolvedTheme),
    [spec, resolvedTheme],
  );

  const isWorking = status === "running";
  const isAwaiting = status === "awaiting_permission";
  const shouldAnimate = motionAllowed && (isWorking || isAwaiting);
  const activeClip = !shouldAnimate
    ? null
    : isAwaiting
      ? SUBAGENT_AWAITING_CLIP
      : SUBAGENT_WORKING_CLIP;

  useEffect(() => {
    if (!shouldAnimate) {
      handleRef.current?.stop({ reset: true });
    }
  }, [shouldAnimate, status]);

  const px = SIZE_PX[size];
  const badgeSize = size === "lg" ? "size-[11px]" : "size-[10px]";

  return (
    <span
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
        <Avatar
          ref={handleRef}
          definition={definition}
          interactive={false}
          animation={activeClip}
          autoplay={shouldAnimate}
          theme={resolvedTheme}
          aria-hidden="true"
          className="block size-full"
        />
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
