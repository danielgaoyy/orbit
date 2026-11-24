import type {
  PendingPlanApprovalView,
  PlanApprovalAction,
  SessionMode,
} from "../../lib/orbit-ipc.ts";

export type PlanReviewTabMode = "preview" | "edit";

export interface PlanReviewDraft {
  readonly markdown: string;
  readonly feedback: string;
  readonly tabMode: PlanReviewTabMode;
  readonly feedbackOpen: boolean;
  readonly expanded: boolean;
}

export interface PlanApprovalSubmitPayload {
  readonly action: PlanApprovalAction;
  readonly editedPlanMarkdown: string | null;
  readonly feedback: string | null;
  readonly targetMode: SessionMode | null;
}

export function normalizePlanMarkdown(raw: string): string {
  return raw.replace(/\r\n/g, "\n");
}

export function isPlanDraftModified(originalMarkdown: string, draftMarkdown: string): boolean {
  return normalizePlanMarkdown(originalMarkdown).trim() !== normalizePlanMarkdown(draftMarkdown).trim();
}

export function createInitialPlanDraft(originalMarkdown: string): PlanReviewDraft {
  return {
    markdown: originalMarkdown,
    feedback: "",
    tabMode: "preview",
    feedbackOpen: false,
    expanded: false,
  };
}

export function resetPlanDraftMarkdown(
  draft: PlanReviewDraft,
  originalMarkdown: string,
): PlanReviewDraft {
  if (draft.markdown === originalMarkdown) return draft;
  return {
    ...draft,
    markdown: originalMarkdown,
  };
}

export function canSubmitPlanChanges(
  originalMarkdown: string,
  draft: Pick<PlanReviewDraft, "markdown" | "feedback">,
): boolean {
  return draft.feedback.trim().length > 0 || isPlanDraftModified(originalMarkdown, draft.markdown);
}

export function buildPlanApprovalSubmitPayload(
  action: PlanApprovalAction,
  originalMarkdown: string,
  draft: Pick<PlanReviewDraft, "markdown" | "feedback">,
): PlanApprovalSubmitPayload {
  if (action === "dismiss") {
    return {
      action,
      editedPlanMarkdown: null,
      feedback: null,
      targetMode: null,
    };
  }
  const modified = isPlanDraftModified(originalMarkdown, draft.markdown);
  const trimmedFeedback = draft.feedback.trim();
  return {
    action,
    editedPlanMarkdown: modified ? draft.markdown : null,
    feedback: trimmedFeedback.length > 0 ? trimmedFeedback : null,
    targetMode: action === "approve_and_build" ? "default" : null,
  };
}

const GENERIC_PLAN_TITLES = new Set([
  "exitplanmode",
  "exit_plan_mode",
  "plan approval",
  "review implementation plan",
  "approve tool execution",
]);

export function resolvePlanReviewHeaderTitle(
  plan: Pick<PendingPlanApprovalView, "plan_title" | "title">,
  fallbackTitle: string,
): string {
  const candidates = [plan.plan_title, plan.title];
  for (const raw of candidates) {
    const trimmed = (raw ?? "").trim();
    if (!trimmed) continue;
    const lower = trimmed.toLowerCase();
    if (GENERIC_PLAN_TITLES.has(lower) || lower.startsWith("claude wants to use exitplanmode")) {
      continue;
    }
    return trimmed;
  }
  return fallbackTitle;
}

export function shouldTriggerPlanModEnter(event: {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  isComposing?: boolean;
}): boolean {
  if (event.isComposing) return false;
  return (
    event.key === "Enter" &&
    (event.metaKey || event.ctrlKey) &&
    !event.shiftKey &&
    !event.altKey
  );
}

export function planDraftScopeKey(hostId: string, agentId: string, requestId: string): string {
  return JSON.stringify([hostId, agentId, requestId]);
}

export interface PlanReviewDraftCache {
  get(scopeKey: string): PlanReviewDraft | undefined;
  save(scopeKey: string, draft: PlanReviewDraft): void;
  delete(scopeKey: string): void;
  size(): number;
}

/**
 * Bounded LRU cache preserving inline Markdown edits and feedback drafts across
 * re-renders, tab switches, and dismiss actions so user edits are never dropped.
 */
export function createPlanReviewDraftCache(maxEntries = 32): PlanReviewDraftCache {
  const map = new Map<string, PlanReviewDraft>();
  return {
    get(scopeKey) {
      const value = map.get(scopeKey);
      if (value !== undefined) {
        map.delete(scopeKey);
        map.set(scopeKey, value);
      }
      return value;
    },
    save(scopeKey, draft) {
      map.delete(scopeKey);
      map.set(scopeKey, draft);
      if (map.size > maxEntries) {
        const oldest = map.keys().next().value;
        if (oldest !== undefined) map.delete(oldest);
      }
    },
    delete(scopeKey) {
      map.delete(scopeKey);
    },
    size() {
      return map.size;
    },
  };
}
