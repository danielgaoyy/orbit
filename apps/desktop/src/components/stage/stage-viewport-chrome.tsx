import { memo, useEffect, useState, useSyncExternalStore, type CSSProperties } from "react";
import { ArrowDown, RotateCw } from "lucide-react";
import { OmniComposer } from "@/components/composer/omni-composer";
import { ChatOutlineRail, type OutlinePromptItem } from "@/components/stage/stage-outline-rail";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { orbitActions } from "@/state/orbit-store";
import { type StageScrollUiStore } from "./stage-helpers";

export const StageHydratingFallback = memo(function StageHydratingFallback({
  bootstrapError,
}: {
  bootstrapError: string | null;
}) {
  const { t } = useI18n();
  const [showIndicator, setShowIndicator] = useState(false);
  const [showSlowRetry, setShowSlowRetry] = useState(false);
  const [retrying, setRetrying] = useState(false);

  useEffect(() => {
    const showTimer = window.setTimeout(() => setShowIndicator(true), 350);
    const slowTimer = window.setTimeout(() => setShowSlowRetry(true), 5000);
    return () => {
      window.clearTimeout(showTimer);
      window.clearTimeout(slowTimer);
    };
  }, []);

  if (!bootstrapError && !showIndicator) {
    return <div className="flex-1" />;
  }

  const needsRetry = Boolean(bootstrapError) || showSlowRetry;

  return (
    <div className="orbit-stage-enter text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2.5 px-6 text-[13px] select-none">
      <div className="flex items-center gap-2">
        {!bootstrapError && (
          <OrbitSpinner size={13} className="text-muted-foreground/75 shrink-0" />
        )}
        <span className="text-muted-foreground/85">
          {bootstrapError
            ? t("stage.loadWorkspaceError")
            : showSlowRetry
              ? t("stage.connectionSlow")
              : t("stage.connecting")}
        </span>
      </div>
      {needsRetry && (
        <button
          type="button"
          disabled={retrying}
          onClick={() => {
            setRetrying(true);
            setShowSlowRetry(false);
            void orbitActions.retryBootstrap().finally(() => {
              setRetrying(false);
            });
          }}
          className="border-border/80 text-foreground/80 hover:bg-accent hover:text-foreground inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[13px] font-medium transition-colors disabled:opacity-50"
        >
          <RotateCw className={cn("size-3", retrying && "animate-spin")} />
          <span>{t(retrying ? "stage.retrying" : "common.retry")}</span>
        </button>
      )}
    </div>
  );
});

export const StageOutlineRailBridge = memo(function StageOutlineRailBridge({
  scrollUiStore,
  prompts,
  jumpingToSeq,
  onJumpToSeq,
  visible,
  offsetPx,
  offsetTransition,
  isAnimating,
}: {
  scrollUiStore: StageScrollUiStore;
  prompts: readonly OutlinePromptItem[];
  jumpingToSeq: number | null;
  onJumpToSeq: (seq: number) => void;
  visible: boolean;
  offsetPx: number;
  offsetTransition: string;
  isAnimating: boolean;
}) {
  const activePromptSeq = useSyncExternalStore(
    scrollUiStore.subscribe,
    () => scrollUiStore.getSnapshot().activePromptSeq,
    () => scrollUiStore.getSnapshot().activePromptSeq,
  );
  return (
    <ChatOutlineRail
      prompts={prompts}
      activeSeq={activePromptSeq ?? prompts[prompts.length - 1]?.seq ?? null}
      jumpingToSeq={jumpingToSeq}
      onJumpToSeq={onJumpToSeq}
      visible={visible}
      offsetPx={offsetPx}
      offsetTransition={offsetTransition}
      isAnimating={isAnimating}
    />
  );
});

export const StageBottomViewportOverlay = memo(function StageBottomViewportOverlay({
  scrollUiStore,
  stagePushStyle,
  overviewReservePx,
  overviewReserveTransition,
  onJumpLatest,
  isGenerating,
}: {
  scrollUiStore: StageScrollUiStore;
  stagePushStyle: CSSProperties;
  overviewReservePx: number;
  overviewReserveTransition: string;
  onJumpLatest: () => void;
  isGenerating: boolean;
}) {
  const { t } = useI18n();
  const snap = useSyncExternalStore(
    scrollUiStore.subscribe,
    scrollUiStore.getSnapshot,
    scrollUiStore.getSnapshot,
  );
  const showFadeMask = snap.showJumpLatest || snap.isOccludingText;

  return (
    <div className="pointer-events-none absolute right-0 bottom-0 left-0 z-10">
      {/* Subtle 24px bottom feather above Composer — never obscures streaming lines */}
      <div
        style={{
          paddingRight: `${Math.max(20, overviewReservePx)}px`,
          transition: overviewReserveTransition,
        }}
        className="pointer-events-none absolute right-0 bottom-0 left-0"
      >
        <div
          className={cn(
            "from-surface-base via-surface-base/75 h-6 w-full bg-gradient-to-t to-transparent transition-opacity duration-200 ease-out",
            showFadeMask ? "opacity-100" : "opacity-0",
          )}
        />
      </div>

      {/* Centered Jump-to-Latest circular button — zero collision since Subagent capsule moved to Overview/TopBar */}
      <div
        style={{
          paddingRight: `${overviewReservePx}px`,
          transition: overviewReserveTransition,
        }}
        className="pointer-events-none relative"
      >
        <div style={stagePushStyle} className="pointer-events-none relative px-[24px]">
          <div className="pointer-events-none relative mx-auto flex w-full max-w-[var(--orbit-stage-max-w,740px)] items-end justify-center">
            <div
              className={cn(
                "pointer-events-none absolute bottom-3 left-1/2 -translate-x-1/2 transition-[opacity,scale,translate]",
                snap.showJumpLatest
                  ? "translate-y-0 scale-100 opacity-100 duration-[160ms] ease-[var(--ease-out-expo)]"
                  : "translate-y-1 scale-95 opacity-0 duration-[115ms] ease-[var(--ease-exit)]",
              )}
            >
              <button
                type="button"
                aria-label={t("stage.scrollToLatest")}
                aria-description={isGenerating ? t("sidebar.running") : undefined}
                aria-hidden={!snap.showJumpLatest}
                tabIndex={snap.showJumpLatest ? 0 : -1}
                onClick={onJumpLatest}
                className={cn(
                  "orbit-press border-border-default bg-surface-raised text-ink-tertiary hover:text-ink-primary hover:border-border-strong floating-surface relative inline-flex size-[32px] cursor-pointer items-center justify-center rounded-full border",
                  snap.showJumpLatest ? "pointer-events-auto" : "pointer-events-none",
                )}
              >
                {snap.showJumpLatest && isGenerating && (
                  <OrbitSpinner
                    size={30}
                    className="text-ink-muted pointer-events-none absolute opacity-75 motion-reduce:animate-none"
                  />
                )}
                <ArrowDown className="size-[16px] stroke-[1.9]" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
});

export const StageDockedComposer = memo(function StageDockedComposer({
  scrollUiStore,
  hasPendingDecisions,
  stageDraggingFiles,
}: {
  scrollUiStore: StageScrollUiStore;
  hasPendingDecisions: boolean;
  stageDraggingFiles: boolean;
}) {
  const isOccludingText = useSyncExternalStore(
    scrollUiStore.subscribe,
    () => scrollUiStore.getSnapshot().isOccludingText,
    () => scrollUiStore.getSnapshot().isOccludingText,
  );
  return (
    <OmniComposer
      variant="docked"
      occluding={isOccludingText}
      hasPendingDecisions={hasPendingDecisions}
      stageDraggingFiles={stageDraggingFiles}
      externalDropSurface
    />
  );
});
