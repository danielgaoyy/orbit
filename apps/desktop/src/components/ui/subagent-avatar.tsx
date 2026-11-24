import { type CSSProperties, type ReactNode, memo, useCallback } from "react";
import { Check, X } from "lucide-react";
import type { ResolvedSubagentStatus } from "@/lib/subagents";
import { type AvatarPath, type SubagentCharacterSpec, resolveSubagentCharacter } from "@/lib/subagent-avatar";
import { cn } from "@/lib/utils";

export type SubagentAvatarSize = "xs" | "sm" | "md" | "lg";

const SIZE_PX = { xs: 18, sm: 20, md: 24, lg: 28 };

type Motion = "working" | "awaiting";
type MotionState = { motion: Motion; visible: boolean };
const active = new Map<HTMLSpanElement, MotionState>();
let observer: IntersectionObserver | undefined;
let reducedMotion: MediaQueryList | undefined;

function syncMotion(el: HTMLSpanElement, state: MotionState) {
  if (state.visible && !reducedMotion?.matches && document.visibilityState === "visible") {
    el.dataset.animate = state.motion;
  } else {
    delete el.dataset.animate;
  }
}

function syncAll() {
  active.forEach((state, el) => syncMotion(el, state));
}

// Active avatars share one observer and one pair of document/preference listeners.
function observeMotion(el: HTMLSpanElement, motion: Motion) {
  if (typeof IntersectionObserver === "undefined") return;
  if (!observer) {
    reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    reducedMotion.addEventListener("change", syncAll);
    document.addEventListener("visibilitychange", syncAll);
    observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        const el = entry.target as HTMLSpanElement;
        const state = active.get(el);
        if (state) {
          state.visible = entry.intersectionRatio > 0;
          syncMotion(el, state);
        }
      }
    }, { threshold: Number.EPSILON });
  }
  active.set(el, { motion, visible: false });
  observer.observe(el);
  return () => {
    active.delete(el);
    observer?.unobserve(el);
    delete el.dataset.animate;
    if (active.size === 0) {
      observer?.disconnect();
      observer = undefined;
      reducedMotion?.removeEventListener("change", syncAll);
      document.removeEventListener("visibilitychange", syncAll);
    }
  };
}

function layer(name: string, paths: readonly AvatarPath[], children?: ReactNode) {
  return (
    <g className={`orbit-av-${name}`}>
      {paths.map(({ d, fill }, index) => <path key={index} d={d} fill={fill} />)}
      {children}
    </g>
  );
}

const Portrait = memo(function Portrait({ character }: { character: SubagentCharacterSpec }) {
  return (
    <svg viewBox="0 0 64 64" aria-hidden="true" className="orbit-avatar-frame block size-full">
      <g className="orbit-av-rig">
        {layer("back", character.back)}
        {layer("head", character.head)}
        {layer("face", character.face, layer("eyes", character.eyes))}
      </g>
    </svg>
  );
});

export interface SubagentAvatarProps {
  /** Pre-assigned collision-free character spec (falls back to deterministic `seedKey`) */
  character?: SubagentCharacterSpec | null;
  seedKey?: string;
  status?: ResolvedSubagentStatus;
  size?: SubagentAvatarSize;
  showStatusBadge?: boolean;
  className?: string;
}

export const SubagentAvatar = memo(function SubagentAvatar({
  character, seedKey, status = "completed", size = "md", showStatusBadge = false, className,
}: SubagentAvatarProps) {
  const spec = character ?? resolveSubagentCharacter(seedKey ?? "subagent");
  const awaiting = status === "awaiting_permission";
  const motion = awaiting ? "awaiting" : status === "running" ? "working" : undefined;
  const ref = useCallback((el: HTMLSpanElement | null) => {
    if (el && motion) return observeMotion(el, motion);
  }, [motion]);
  const px = SIZE_PX[size];
  const Badge = status === "completed" ? Check : X;

  return (
    <span
      ref={ref}
      data-slot="subagent-avatar"
      data-status={status}
      data-character={spec.id}
      data-attention={awaiting ? "true" : undefined}
      style={{ width: px, height: px, "--av-light": spec.lightBg, "--av-dark": spec.darkBg } as CSSProperties}
      className={cn("relative inline-flex shrink-0 items-center justify-center select-none", className)}
    >
      <Portrait character={spec} />
      {showStatusBadge && !motion && (
        <Badge
          aria-hidden="true"
          size={size === "lg" ? 11 : 10}
          strokeWidth={3}
          style={{ right: -2, bottom: -2, padding: 1.5 }}
          className={cn(
            "ring-surface-base absolute rounded-full text-white ring-[1.5px]",
            status === "completed" ? "bg-signal-success" : status === "failed" ? "bg-signal-danger" : "bg-ink-muted",
          )}
        />
      )}
    </span>
  );
});
