import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import {
  ArrowUpRight,
  FileText,
  Maximize2,
  MessageSquareText,
  Minimize2,
  Play,
  RotateCcw,
  ScrollText,
  X,
} from "lucide-react";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ShortcutKbd } from "@/components/ui/kbd";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SimpleTooltip } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Textarea } from "@/components/ui/textarea";
import { useI18n } from "@/lib/i18n";
import { isImeKeyEvent, useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import {
  type PendingPlanApprovalView,
  type PlanApprovalAction,
  type RichBlockView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { stripWorkspacePrefix } from "@/lib/timeline-coalesce";
import { cn } from "@/lib/utils";
import { orbitActions } from "@/state/orbit-store";
import {
  buildPlanApprovalSubmitPayload,
  canSubmitPlanChanges,
  createInitialPlanDraft,
  createPlanReviewDraftCache,
  isPlanDraftModified,
  planDraftScopeKey,
  resetPlanDraftMarkdown,
  resolvePlanReviewHeaderTitle,
  shouldTriggerPlanModEnter,
  type PlanReviewDraft,
} from "./plan-review-card-model";

const sharedPlanDraftCache = createPlanReviewDraftCache(32);

export interface PlanReviewCardProps {
  hostId: string;
  agentId: string;
  cwd: string;
  plan: PendingPlanApprovalView;
  compact?: boolean;
}

export const PlanReviewCard = memo(function PlanReviewCard({
  hostId,
  agentId,
  cwd,
  plan,
  compact = false,
}: PlanReviewCardProps) {
  const scopeKey = planDraftScopeKey(hostId, agentId, plan.request_id);
  return (
    <PlanReviewCardInner
      key={scopeKey}
      scopeKey={scopeKey}
      hostId={hostId}
      agentId={agentId}
      cwd={cwd}
      plan={plan}
      compact={compact}
    />
  );
});

function PlanReviewCardInner({
  scopeKey,
  hostId,
  agentId,
  cwd,
  plan,
  compact,
}: PlanReviewCardProps & { scopeKey: string }) {
  const { t } = useI18n();
  const mod = modSymbol();
  const ime = useImeCompositionGuard();

  const [draft, setDraft] = useState<PlanReviewDraft>(
    () => sharedPlanDraftCache.get(scopeKey) ?? createInitialPlanDraft(plan.plan_markdown),
  );
  const [focusedWithin, setFocusedWithin] = useState(false);
  const [submittingAction, setSubmittingAction] = useState<PlanApprovalAction | null>(null);
  const [exiting, setExiting] = useState(false);
  const [editedPreviewBlocks, setEditedPreviewBlocks] = useState<RichBlockView[] | null>(null);

  const submittingRef = useRef<PlanApprovalAction | null>(null);
  submittingRef.current = submittingAction;
  const editTextareaRef = useRef<HTMLTextAreaElement | null>(null);
  const feedbackTextareaRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    sharedPlanDraftCache.save(scopeKey, draft);
  }, [scopeKey, draft]);

  const isModified = useMemo(
    () => isPlanDraftModified(plan.plan_markdown, draft.markdown),
    [plan.plan_markdown, draft.markdown],
  );

  // When unmodified, reuse Rust's pre-parsed `plan.blocks` directly (zero flicker).
  // When modified and viewing Preview, re-parse edited Markdown via Rust core.
  useEffect(() => {
    if (!isModified) {
      setEditedPreviewBlocks(null);
      return;
    }
    if (draft.tabMode !== "preview") return;
    let cancelled = false;
    void orbitIpc
      .parseMarkdownPreview(draft.markdown, false)
      .then((blocks) => {
        if (!cancelled) setEditedPreviewBlocks(blocks);
      })
      .catch(() => {
        if (!cancelled) setEditedPreviewBlocks(null);
      });
    return () => {
      cancelled = true;
    };
  }, [isModified, draft.tabMode, draft.markdown]);

  // Autosize the Edit textarea cleanly within bounded viewport heights.
  useLayoutEffect(() => {
    if (draft.tabMode !== "edit") return;
    const el = editTextareaRef.current;
    if (!el) return;
    const maxPx = draft.expanded
      ? Math.max(320, Math.round(window.innerHeight * 0.55))
      : compact
        ? 220
        : 300;
    el.style.height = "auto";
    el.style.height = `${Math.min(Math.max(el.scrollHeight, 140), maxPx)}px`;
  }, [draft.tabMode, draft.markdown, draft.expanded, compact]);

  // Auto-focus the feedback textarea when the user opens the Request Changes drawer.
  useEffect(() => {
    if (!draft.feedbackOpen) return;
    const id = window.requestAnimationFrame(() => {
      feedbackTextareaRef.current?.focus();
    });
    return () => window.cancelAnimationFrame(id);
  }, [draft.feedbackOpen]);

  const headerTitle = useMemo(
    () => resolvePlanReviewHeaderTitle(plan, t("planReview.defaultTitle")),
    [plan, t],
  );
  const relativePlanPath = useMemo(() => {
    const raw = plan.plan_file_path?.trim();
    if (!raw) return null;
    return stripWorkspacePrefix(raw, cwd) || raw;
  }, [plan.plan_file_path, cwd]);

  const allowedPrompts = plan.allowed_prompts ?? [];
  const previewBlocks = isModified ? editedPreviewBlocks : plan.blocks;
  const hasAnyPlanText = draft.markdown.trim().length > 0;
  const canSendChanges = canSubmitPlanChanges(plan.plan_markdown, draft);

  const handleSubmit = useCallback(
    async (action: PlanApprovalAction) => {
      if (submittingRef.current) return;
      if (action === "request_changes" && !canSubmitPlanChanges(plan.plan_markdown, draft)) {
        setDraft((prev) => ({ ...prev, feedbackOpen: true }));
        return;
      }

      const payload = buildPlanApprovalSubmitPayload(action, plan.plan_markdown, draft);
      submittingRef.current = action;
      setSubmittingAction(action);
      try {
        await orbitIpc.resolvePlanApproval(hostId, agentId, plan.request_id, payload.action, {
          editedPlanMarkdown: payload.editedPlanMarkdown,
          feedback: payload.feedback,
          targetMode: payload.targetMode,
        });
        setExiting(true);
        // On approve_and_build / request_changes, the user's edits have been sent to the agent.
        // On dismiss, keep the saved draft intact so user edits are never lost.
        if (action !== "dismiss") {
          sharedPlanDraftCache.delete(scopeKey);
        }
      } catch (err) {
        submittingRef.current = null;
        setSubmittingAction(null);
        setExiting(false);
        const detail = err instanceof Error ? err.message.trim() : String(err).trim();
        orbitActions.showToast(
          t("planReview.submitFailed", { error: detail || t("common.actionFailed") }),
        );
      }
    },
    [agentId, draft, hostId, plan.plan_markdown, plan.request_id, scopeKey, t],
  );

  const handleCardKeyDown = useCallback(
    (e: ReactKeyboardEvent<HTMLDivElement>) => {
      if (ime.shouldIgnoreKeyDown(e) || isImeKeyEvent(e)) return;
      if (e.key === "Escape" && draft.feedbackOpen) {
        e.preventDefault();
        e.stopPropagation();
        setDraft((prev) => ({ ...prev, feedbackOpen: false }));
        return;
      }
      if (!shouldTriggerPlanModEnter(e)) return;
      e.preventDefault();
      e.stopPropagation();
      const insideFeedback =
        feedbackTextareaRef.current !== null &&
        feedbackTextareaRef.current === document.activeElement;
      if (insideFeedback) {
        if (canSubmitPlanChanges(plan.plan_markdown, draft)) {
          void handleSubmit("request_changes");
        }
        return;
      }
      void handleSubmit("approve_and_build");
    },
    [draft, handleSubmit, ime, plan.plan_markdown],
  );

  const handleOpenPlanFile = useCallback(() => {
    const filePath = plan.plan_file_path?.trim();
    if (!filePath) return;
    orbitActions.openFileInSidePanel(filePath, { hostId, cwd });
  }, [cwd, hostId, plan.plan_file_path]);

  const busy = submittingAction !== null;

  return (
    <div
      data-slot="plan-review-card"
      className={cn(
        "orbit-dock-enter w-full pb-2 transition-[opacity,transform] duration-[var(--dur-overlay)] ease-[var(--ease-exit)] motion-reduce:transition-none",
        compact ? "px-3" : "px-[24px]",
        exiting && "pointer-events-none translate-y-1 opacity-0",
      )}
      onFocusCapture={() => setFocusedWithin(true)}
      onBlurCapture={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
          setFocusedWithin(false);
        }
      }}
      onKeyDown={handleCardKeyDown}
    >
      <div className="composer-queue-surface mx-auto w-full max-w-[var(--orbit-stage-max-w,740px)] overflow-hidden rounded-2xl">
        {/* Header Bar */}
        <div className="border-border-subtle flex flex-wrap items-center justify-between gap-2 border-b px-4 py-2.5">
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <span className="bg-signal-warning size-1.5 shrink-0 rounded-full" aria-hidden="true" />
            <ScrollText
              className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]"
              aria-hidden="true"
            />
            <span className="text-ink-tertiary shrink-0 text-[11px] font-medium tracking-[0.02em] uppercase">
              {t("planReview.eyebrow")}
            </span>
            <span className="text-ink-faint shrink-0" aria-hidden="true">
              ·
            </span>
            <span className="text-ink-primary min-w-0 truncate text-[13px] font-medium">
              {headerTitle}
            </span>
            {isModified && (
              <Badge variant="warning" className="shrink-0">
                {t("planReview.modified")}
              </Badge>
            )}
          </div>

          <div className="flex shrink-0 items-center gap-1.5">
            {relativePlanPath && !compact && (
              <SimpleTooltip label={t("planReview.openFile")} side="top">
                <button
                  type="button"
                  onClick={handleOpenPlanFile}
                  className="bg-surface-recessed border-border-subtle text-ink-tertiary hover:text-ink-primary hover:border-border-default inline-flex h-[24px] max-w-[180px] cursor-pointer items-center gap-1 rounded-[6px] border px-2 font-mono text-[11px] transition-colors"
                >
                  <FileText className="size-3 shrink-0" aria-hidden="true" />
                  <span className="truncate">{relativePlanPath}</span>
                  <ArrowUpRight className="size-2.5 shrink-0 opacity-70" aria-hidden="true" />
                </button>
              </SimpleTooltip>
            )}

            {/* Preview / Edit Segmented Control */}
            <div
              role="tablist"
              aria-label={t("planReview.eyebrow")}
              className="bg-surface-recessed border-border-subtle inline-flex items-center rounded-[8px] border p-0.5"
            >
              <button
                type="button"
                role="tab"
                aria-selected={draft.tabMode === "preview"}
                disabled={busy}
                onClick={() => setDraft((prev) => ({ ...prev, tabMode: "preview" }))}
                className={cn(
                  "h-[22px] cursor-pointer rounded-[6px] px-2 text-[11px] font-medium transition-colors",
                  draft.tabMode === "preview"
                    ? "bg-surface-raised text-ink-primary shadow-2xs"
                    : "text-ink-tertiary hover:text-ink-primary",
                )}
              >
                {t("planReview.previewTab")}
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={draft.tabMode === "edit"}
                disabled={busy}
                onClick={() => setDraft((prev) => ({ ...prev, tabMode: "edit" }))}
                className={cn(
                  "h-[22px] cursor-pointer rounded-[6px] px-2 text-[11px] font-medium transition-colors",
                  draft.tabMode === "edit"
                    ? "bg-surface-raised text-ink-primary shadow-2xs"
                    : "text-ink-tertiary hover:text-ink-primary",
                )}
              >
                {t("planReview.editTab")}
              </button>
            </div>

            {isModified && (
              <SimpleTooltip label={t("planReview.resetTooltip")} side="top">
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  disabled={busy}
                  onClick={() =>
                    setDraft((prev) => resetPlanDraftMarkdown(prev, plan.plan_markdown))
                  }
                  className="text-ink-tertiary hover:text-ink-primary h-[24px] px-2 text-[11px]"
                >
                  <RotateCcw data-icon="inline-start" className="size-3" />
                  {t("planReview.reset")}
                </Button>
              </SimpleTooltip>
            )}

            <SimpleTooltip
              label={draft.expanded ? t("planReview.collapse") : t("planReview.expand")}
              side="top"
            >
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                disabled={busy}
                aria-label={draft.expanded ? t("planReview.collapse") : t("planReview.expand")}
                onClick={() => setDraft((prev) => ({ ...prev, expanded: !prev.expanded }))}
                className="text-ink-tertiary hover:text-ink-primary size-[24px] rounded-[6px]"
              >
                {draft.expanded ? (
                  <Minimize2 className="size-3" />
                ) : (
                  <Maximize2 className="size-3" />
                )}
              </Button>
            </SimpleTooltip>

            <SimpleTooltip label={t("planReview.dismissTooltip")} side="top">
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                disabled={busy}
                aria-label={t("planReview.dismiss")}
                onClick={() => void handleSubmit("dismiss")}
                className="text-ink-tertiary hover:text-ink-primary size-[24px] rounded-[6px]"
              >
                <X className="size-3.5" />
              </Button>
            </SimpleTooltip>
          </div>
        </div>

        {/* Optional provider review summary (kept separate from editable plan Markdown) */}
        {plan.summary &&
          plan.summary.trim().length > 0 &&
          plan.summary.trim() !== plan.plan_markdown.trim() && (
            <div className="border-border-subtle/60 bg-surface-recessed/35 text-ink-secondary border-b px-4 py-2 text-[12px] leading-[1.45]">
              {plan.summary.trim()}
            </div>
          )}

        {/* Plan Content: Preview or Edit */}
        {draft.tabMode === "preview" ? (
          <ScrollArea
            viewportClassName={cn(
              "selectable-text overscroll-contain px-4 py-3",
              draft.expanded
                ? "max-h-[55vh]"
                : compact
                  ? "max-h-[220px]"
                  : "max-h-[300px]",
            )}
          >
            {!hasAnyPlanText ? (
              <p className="text-ink-tertiary text-[13px] leading-[1.5]">
                {t("planReview.emptyPlan")}
              </p>
            ) : previewBlocks && previewBlocks.length > 0 ? (
              <MarkdownBlocksView blocks={previewBlocks} compact />
            ) : (
              <pre className="text-ink-primary m-0 font-sans text-[13px] leading-[1.55] whitespace-pre-wrap break-words">
                {draft.markdown}
              </pre>
            )}
          </ScrollArea>
        ) : (
          <div className="px-4 py-3">
            <Textarea
              ref={editTextareaRef}
              value={draft.markdown}
              disabled={busy}
              {...ime.compositionProps}
              onChange={(e) => setDraft((prev) => ({ ...prev, markdown: e.target.value }))}
              placeholder={t("planReview.editPlaceholder")}
              aria-label={t("planReview.editTab")}
              className="bg-surface-recessed/70 border-border-subtle text-ink-primary min-h-[140px] resize-none rounded-xl font-mono text-[13px] leading-[1.55]"
            />
          </div>
        )}

        {/* Optional Requested Tool Permissions Summary */}
        {allowedPrompts.length > 0 && (
          <div className="border-border-subtle/70 bg-surface-recessed/45 text-ink-tertiary flex items-center gap-2 border-t px-4 py-1.5 text-[11px]">
            <span className="shrink-0 font-medium">
              {t("planReview.allowedPrompts", { count: allowedPrompts.length })}
            </span>
            <span className="text-ink-faint" aria-hidden="true">
              ·
            </span>
            <span className="min-w-0 truncate font-mono">{allowedPrompts.join(" · ")}</span>
          </div>
        )}

        {/* Collapsible Inline Feedback Drawer for "Request Changes" */}
        {draft.feedbackOpen && (
          <div className="border-border-subtle bg-surface-recessed/45 flex flex-col gap-2 border-t px-4 py-2.5">
            <Textarea
              ref={feedbackTextareaRef}
              value={draft.feedback}
              disabled={busy}
              rows={2}
              {...ime.compositionProps}
              onChange={(e) => setDraft((prev) => ({ ...prev, feedback: e.target.value }))}
              placeholder={t("planReview.feedbackPlaceholder")}
              aria-label={t("planReview.requestChanges")}
              className="bg-surface-base border-border-default min-h-[58px] resize-none rounded-lg text-[13px] leading-[1.45]"
            />
            <div className="flex items-center justify-between gap-2">
              <span className="text-ink-tertiary text-[11px] leading-[1.35]">
                {t("planReview.feedbackHint")}
              </span>
              <div className="flex shrink-0 items-center gap-1.5">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={busy}
                  onClick={() => setDraft((prev) => ({ ...prev, feedbackOpen: false }))}
                  className="text-[13px]"
                >
                  {t("common.cancel")}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={busy || !canSendChanges}
                  onClick={() => void handleSubmit("request_changes")}
                  className="text-[13px]"
                >
                  {submittingAction === "request_changes" && (
                    <OrbitSpinner size={12} className="shrink-0" />
                  )}
                  <span>
                    {submittingAction === "request_changes"
                      ? t("planReview.sendingChanges")
                      : t("planReview.sendChanges")}
                  </span>
                </Button>
              </div>
            </div>
          </div>
        )}

        {/* Footer Action Bar */}
        <div className="border-border-subtle flex items-center justify-between gap-2 border-t px-4 py-2.5">
          <div className="flex items-center gap-1.5">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={busy}
              onClick={() => void handleSubmit("dismiss")}
              className="text-ink-tertiary hover:text-ink-primary text-[13px]"
            >
              {t("planReview.dismiss")}
            </Button>
            <Button
              type="button"
              variant={draft.feedbackOpen ? "secondary" : "outline"}
              size="sm"
              disabled={busy}
              aria-expanded={draft.feedbackOpen}
              onClick={() =>
                setDraft((prev) => ({ ...prev, feedbackOpen: !prev.feedbackOpen }))
              }
              className="text-[13px]"
            >
              <MessageSquareText data-icon="inline-start" className="size-3.5" />
              <span>{t("planReview.requestChanges")}</span>
            </Button>
          </div>

          <Button
            type="button"
            variant="default"
            size="sm"
            disabled={busy}
            onClick={() => void handleSubmit("approve_and_build")}
            className="gap-1.5 rounded-lg px-3.5 text-[13px] font-medium"
          >
            {submittingAction === "approve_and_build" ? (
              <OrbitSpinner size={12} className="shrink-0" />
            ) : (
              <Play data-icon="inline-start" className="size-3 fill-current" />
            )}
            <span>
              {submittingAction === "approve_and_build"
                ? t("planReview.approving")
                : t("planReview.approveAndBuild")}
            </span>
            {focusedWithin && submittingAction === null && (
              <ShortcutKbd keys={`${mod}↵`} size="xs" variant="inverted" className="ml-0.5" />
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
