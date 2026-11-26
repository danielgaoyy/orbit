import { memo, useCallback, useEffect, useRef, useState } from "react";
import { Minus, Plus } from "lucide-react";
import { useI18n } from "@/lib/i18n";
import { noteExternalZoomEvent } from "@/lib/keybindings";
import {
  type UiZoomChangedPayload,
  type UiZoomState,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  ZOOM_DEFAULT_PERCENT,
  ZOOM_MAX_PERCENT,
  ZOOM_MIN_PERCENT,
} from "@/lib/typography";
import { cn } from "@/lib/utils";
import { orbitActions } from "@/state/orbit-store";

const ZOOM_HUD_DURATION_MS = 1600;
const ZOOM_HUD_EXIT_MS = 175;

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    Boolean(window.matchMedia?.("(prefers-reduced-motion: reduce)").matches)
  );
}

export const ZoomHud = memo(function ZoomHud({
  state,
  visible,
  exiting,
  onHoverChange,
  onFocusChange,
  onStep,
  onActualSize,
}: {
  state: UiZoomState;
  visible: boolean;
  exiting: boolean;
  onHoverChange: (hovered: boolean) => void;
  onFocusChange: (focused: boolean) => void;
  onStep: (delta: -1 | 1) => void;
  onActualSize: () => void;
}) {
  const { t } = useI18n();
  if (!visible) return null;

  const atMin = state.zoom_percent <= (state.min_percent || ZOOM_MIN_PERCENT);
  const atMax = state.zoom_percent >= (state.max_percent || ZOOM_MAX_PERCENT);
  const isDefault = state.zoom_percent === ZOOM_DEFAULT_PERCENT;

  return (
    <div
      role="status"
      aria-live="polite"
      aria-label={t("zoomHud.label")}
      data-slot="zoom-hud"
      style={{ top: "calc(var(--orbit-titlebar-h) + 8px)" }}
      onMouseEnter={() => onHoverChange(true)}
      onMouseLeave={() => onHoverChange(false)}
      onFocus={() => onFocusChange(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
          onFocusChange(false);
        }
      }}
      className={cn(
        "floating-surface border-border-default bg-surface-overlay text-ink-primary fixed right-[12px] z-50 flex items-center gap-1 rounded-xl border px-1.5 py-1 text-[13px] select-none",
        exiting ? "orbit-zoom-hud-exit" : "orbit-zoom-hud-enter",
      )}
    >
      <button
        type="button"
        disabled={atMin}
        aria-label={t("settings.zoomOut")}
        onClick={() => onStep(-1)}
        className="text-ink-secondary hover:bg-accent hover:text-ink-primary focus-visible:ring-ring/50 inline-flex size-[24px] cursor-pointer items-center justify-center rounded-lg transition-colors focus-visible:ring-1 disabled:pointer-events-none disabled:opacity-35"
      >
        <Minus className="size-[13px] stroke-[2]" />
      </button>

      <span className="min-w-[44px] px-1 text-center font-medium tabular-nums">
        {state.zoom_percent}%
      </span>

      <button
        type="button"
        disabled={atMax}
        aria-label={t("settings.zoomIn")}
        onClick={() => onStep(1)}
        className="text-ink-secondary hover:bg-accent hover:text-ink-primary focus-visible:ring-ring/50 inline-flex size-[24px] cursor-pointer items-center justify-center rounded-lg transition-colors focus-visible:ring-1 disabled:pointer-events-none disabled:opacity-35"
      >
        <Plus className="size-[13px] stroke-[2]" />
      </button>

      {!isDefault && (
        <>
          <span aria-hidden="true" className="text-ink-faint px-0.5">
            ·
          </span>
          <button
            type="button"
            onClick={onActualSize}
            className="text-ink-secondary hover:bg-accent hover:text-ink-primary focus-visible:ring-ring/50 inline-flex h-[24px] cursor-pointer items-center rounded-lg px-2 text-[12px] font-medium whitespace-nowrap transition-colors focus-visible:ring-1"
          >
            {t("settings.actualSize")}
          </button>
        </>
      )}
    </div>
  );
});

/**
 * Isolated host for native Desktop window zoom bootstrap, event sync, and ephemeral HUD.
 * Mounted once at the root of `MainDesktopApp` so HUD ticks never re-render the workspace.
 */
export const GlobalZoomHudHost = memo(function GlobalZoomHudHost() {
  const [hudState, setHudState] = useState<UiZoomState>({
    zoom_percent: ZOOM_DEFAULT_PERCENT,
    min_percent: ZOOM_MIN_PERCENT,
    max_percent: ZOOM_MAX_PERCENT,
    step_percent: 10,
  });
  const [visible, setVisible] = useState(false);
  const [exiting, setExiting] = useState(false);

  const hoveredRef = useRef(false);
  const focusedRef = useRef(false);
  const hideTimerRef = useRef<number | null>(null);
  const exitTimerRef = useRef<number | null>(null);

  const clearTimers = useCallback(() => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    if (exitTimerRef.current !== null) {
      window.clearTimeout(exitTimerRef.current);
      exitTimerRef.current = null;
    }
  }, []);

  const scheduleHide = useCallback(() => {
    clearTimers();
    if (hoveredRef.current || focusedRef.current) return;
    hideTimerRef.current = window.setTimeout(() => {
      hideTimerRef.current = null;
      if (hoveredRef.current || focusedRef.current) return;
      if (prefersReducedMotion()) {
        setVisible(false);
        setExiting(false);
        return;
      }
      setExiting(true);
      exitTimerRef.current = window.setTimeout(() => {
        exitTimerRef.current = null;
        setVisible(false);
        setExiting(false);
      }, ZOOM_HUD_EXIT_MS);
    }, ZOOM_HUD_DURATION_MS);
  }, [clearTimers]);

  const showZoomHud = useCallback(
    (payload: UiZoomState) => {
      setHudState({
        zoom_percent: payload.zoom_percent,
        min_percent: payload.min_percent,
        max_percent: payload.max_percent,
        step_percent: payload.step_percent,
      });
      setVisible(true);
      setExiting(false);
      scheduleHide();
    },
    [scheduleHide],
  );

  const handleHoverChange = useCallback(
    (hovered: boolean) => {
      hoveredRef.current = hovered;
      if (hovered) {
        clearTimers();
        setExiting(false);
      } else {
        scheduleHide();
      }
    },
    [clearTimers, scheduleHide],
  );

  const handleFocusChange = useCallback(
    (focused: boolean) => {
      focusedRef.current = focused;
      if (focused) {
        clearTimers();
        setExiting(false);
      } else {
        scheduleHide();
      }
    },
    [clearTimers, scheduleHide],
  );

  const handleStep = useCallback((delta: -1 | 1) => {
    void orbitIpc
      .stepUiZoom(delta, "shortcut")
      .then((next) => {
        orbitActions.applyUiZoomState(next);
      })
      .catch(() => undefined);
  }, []);

  const handleActualSize = useCallback(() => {
    void orbitIpc
      .setUiZoom(ZOOM_DEFAULT_PERCENT, "shortcut")
      .then((next) => {
        orbitActions.applyUiZoomState(next);
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;

    void orbitIpc
      .getUiZoom()
      .then((state) => {
        if (!active) return;
        orbitActions.applyUiZoomState(state);
      })
      .catch(() => undefined);

    void orbitIpc
      .onUiZoomChanged((payload: UiZoomChangedPayload) => {
        if (!active) return;
        noteExternalZoomEvent(payload.source);
        orbitActions.applyUiZoomState(payload);
        if (payload.source === "menu" || payload.source === "shortcut") {
          showZoomHud(payload);
        }
      })
      .then((fn) => {
        if (!active) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => undefined);

    return () => {
      active = false;
      unlisten?.();
      clearTimers();
    };
  }, [clearTimers, showZoomHud]);

  return (
    <ZoomHud
      state={hudState}
      visible={visible}
      exiting={exiting}
      onHoverChange={handleHoverChange}
      onFocusChange={handleFocusChange}
      onStep={handleStep}
      onActualSize={handleActualSize}
    />
  );
});
