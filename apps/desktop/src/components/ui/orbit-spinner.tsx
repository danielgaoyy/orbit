import { memo, useId, useLayoutEffect, useRef } from "react";
import {
  ORBIT_SPINNER_DURATION_MS,
  syncElementAmbientAnimations,
} from "@/lib/animation-sync";
import { cn } from "@/lib/utils";

/**
 * Refined orbital comet spinner:
 * - Whisper-light full orbital track + tapered gradient comet arc.
 * - Pure SVG + GPU `transform: rotate()` animation (zero layout thrash).
 * - Phase-locked to the global `document.timeline` clock so concurrent spinners across
 *   Sidebar, Overview, Stage, and Peer Strip rotate in calm, synchronized unison.
 */
export const OrbitSpinner = memo(function OrbitSpinner({
  size = 14,
  className,
}: {
  size?: number;
  className?: string;
}) {
  const gradId = useId();
  const svgRef = useRef<SVGSVGElement | null>(null);

  useLayoutEffect(() => {
    syncElementAmbientAnimations(svgRef.current, ORBIT_SPINNER_DURATION_MS);
  }, []);

  return (
    <svg
      ref={svgRef}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      aria-hidden="true"
      className={cn(
        "orbit-spinner-reduced shrink-0 animate-[spin_0.85s_linear_infinite] will-change-transform",
        className,
      )}
    >
      <defs>
        <linearGradient id={gradId} x1="8" y1="1.75" x2="14.25" y2="8" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="currentColor" stopOpacity="0.05" />
          <stop offset="55%" stopColor="currentColor" stopOpacity="0.55" />
          <stop offset="100%" stopColor="currentColor" stopOpacity="1" />
        </linearGradient>
      </defs>
      <circle
        cx="8"
        cy="8"
        r="6.1"
        stroke="currentColor"
        strokeOpacity="0.14"
        strokeWidth="1.6"
      />
      <path
        d="M8 1.9A6.1 6.1 0 0 1 14.1 8"
        stroke={`url(#${gradId})`}
        strokeWidth="1.75"
        strokeLinecap="round"
      />
    </svg>
  );
});
