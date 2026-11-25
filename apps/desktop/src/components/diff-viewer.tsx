import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  FileCode2,
  Loader2,
  MessageSquarePlus,
  MessageSquareText,
  Minus,
  Plus,
  RotateCcw,
  Search,
  ShieldAlert,
  Trash2,
  UnfoldVertical,
  Wrench,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Textarea } from "@/components/ui/textarea";
import { resolveTokenColor } from "@/lib/code-highlight";
import { shouldPrefetchOnPointerDown } from "@/lib/file-prefetch";
import {
  extractRangeSnippetFromDiffFile,
  formatLineRangeTag,
  formatPathLineRange,
  isLineWithinSelection,
  normalizeLineRange,
  resolveDraftLineBounds,
  type ReviewCommentDraft,
} from "@/lib/git-collab";
import { extractFileHunks, type IndexedDiffHunk } from "@/lib/git-patch";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import {
  orbitIpc,
  type DiffFileView,
  type DiffLineView,
  type HighlightedTokenView,
  type InlineDiffSpan,
  type PrPatrolCategory,
  type PrPatrolFinding,
  type PrPatrolSeverity,
} from "@/lib/orbit-ipc";
import {
  formatPatrolLineRange,
  indexPatrolFindings,
  normalizePatrolPath,
} from "@/lib/pr-patrol";

function patrolCategoryI18nKey(category: PrPatrolCategory):
  | "review.patrolCategorySecurity"
  | "review.patrolCategoryCorrectness"
  | "review.patrolCategoryConcurrency"
  | "review.patrolCategoryPerformance"
  | "review.patrolCategoryMaintainability" {
  switch (category) {
    case "security":
      return "review.patrolCategorySecurity";
    case "correctness":
      return "review.patrolCategoryCorrectness";
    case "concurrency":
      return "review.patrolCategoryConcurrency";
    case "performance":
      return "review.patrolCategoryPerformance";
    default:
      return "review.patrolCategoryMaintainability";
  }
}
import { type DiffLayoutMode, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export type { ReviewCommentDraft };

interface ActiveCommentTarget {
  fileKey: string;
  path: string;
  side: "old" | "new";
  anchorLine: number | null;
  startLine: number | null;
  endLine: number | null;
  lineNumber: number | null;
  code: string;
  anchorKey: string;
}

interface SplitRow {
  kind: "header" | "pair";
  headerText?: string;
  headerLineIdx?: number;
  left?: DiffLineView;
  right?: DiffLineView;
}

type DiffVirtualRow =
  | {
      kind: "file_header";
      key: string;
      file: DiffFileView;
      fileKey: string;
      collapsed: boolean;
      viewed: boolean;
      commentCount: number;
      patrolCount: number;
      patrolHighestSeverity: PrPatrolSeverity | null;
    }
  | {
      kind: "hunk_header";
      key: string;
      file: DiffFileView;
      content: string;
      hunk: IndexedDiffHunk | null;
      hunkKey: string;
    }
  | {
      kind: "unified_line";
      key: string;
      file: DiffFileView;
      fileKey: string;
      line: DiffLineView;
      lineIdx: number;
      isLastInFile: boolean;
      patrolAnchorKey: string | null;
      patrolFindings: readonly PrPatrolFinding[] | null;
    }
  | {
      kind: "split_pair";
      key: string;
      file: DiffFileView;
      fileKey: string;
      left?: DiffLineView;
      right?: DiffLineView;
      rowIdx: number;
      isLastInFile: boolean;
      patrolAnchorKey: string | null;
      patrolFindings: readonly PrPatrolFinding[] | null;
    }
  | {
      kind: "patrol_finding_list";
      key: string;
      anchorKey: string;
      findings: readonly PrPatrolFinding[];
    }
  | {
      kind: "comment_list";
      key: string;
      comments: ReviewCommentDraft[];
    }
  | {
      kind: "comment_composer";
      key: string;
      target: ActiveCommentTarget;
    }
  | {
      kind: "omission";
      key: string;
      notice: string;
    }
  | {
      kind: "context_gap";
      key: string;
      gapKey: string;
      filePath: string;
      hiddenCount: number;
      startNew: number;
      endNew: number;
      startOld: number;
    }
  | {
      kind: "file_spacer";
      key: string;
    };

interface CachedFileContext {
  lines: string[];
  highlighted: Map<number, HighlightedTokenView[]>;
}

interface GapExpansionState {
  shownCount: number;
  showAll: boolean;
}

const CONTEXT_EXPAND_CHUNK_SIZE = 20;

function parseHunkHeaderStart(content: string): { oldStart: number; newStart: number } | null {
  const m = /^@@\s+-(\d+)(?:,\d+)?\s+\+(\d+)(?:,\d+)?\s+@@/.exec(content.trim());
  if (!m) return null;
  const oldStart = Number.parseInt(m[1], 10);
  const newStart = Number.parseInt(m[2], 10);
  if (!Number.isFinite(oldStart) || !Number.isFinite(newStart)) return null;
  return { oldStart, newStart };
}

function buildExpandedContextLines(
  cached: CachedFileContext | undefined,
  startNew: number,
  endNew: number,
  oldOffset: number,
): DiffLineView[] {
  if (!cached || endNew < startNew) return [];
  const out: DiffLineView[] = [];
  for (let ln = startNew; ln <= endNew; ln += 1) {
    const raw = cached.lines[ln - 1];
    if (raw === undefined) break;
    const oldLn = ln + oldOffset;
    out.push({
      kind: "context",
      old_line: oldLn > 0 ? oldLn : null,
      new_line: ln,
      content: raw,
      tokens: cached.highlighted.get(ln) ?? [],
      inline_changes: [],
    });
  }
  return out;
}

function patrolSeverityVariant(severity: PrPatrolSeverity): "destructive" | "warning" | "default" {
  if (severity === "P0") return "destructive";
  if (severity === "P1") return "warning";
  return "default";
}

function patrolLineBorderClass(severity: PrPatrolSeverity): string {
  if (severity === "P0") return "border-l-2 border-l-signal-danger/80";
  if (severity === "P1") return "border-l-2 border-l-signal-warning/80";
  return "border-l-2 border-l-signal-info/75";
}

function patrolInlinePillClass(severity: PrPatrolSeverity): string {
  if (severity === "P0") {
    return "border-signal-danger-border bg-signal-danger-bg text-signal-danger";
  }
  if (severity === "P1") {
    return "border-signal-warning-border bg-signal-warning-bg text-signal-warning";
  }
  return "border-signal-info-border bg-signal-info-bg text-signal-info";
}

function buildSplitRows(lines: DiffLineView[]): SplitRow[] {
  const rows: SplitRow[] = [];
  let i = 0;

  while (i < lines.length) {
    const cur = lines[i];
    if (cur.kind === "meta") {
      i += 1;
      continue;
    }
    if (cur.kind === "hunk_header") {
      rows.push({ kind: "header", headerText: cur.content, headerLineIdx: i });
      i += 1;
      continue;
    }
    if (cur.kind === "context") {
      rows.push({ kind: "pair", left: cur, right: cur });
      i += 1;
      continue;
    }

    const removedBatch: DiffLineView[] = [];
    const addedBatch: DiffLineView[] = [];
    while (i < lines.length && lines[i].kind === "removed") {
      removedBatch.push(lines[i]);
      i += 1;
    }
    while (i < lines.length && lines[i].kind === "added") {
      addedBatch.push(lines[i]);
      i += 1;
    }
    const maxLen = Math.max(removedBatch.length, addedBatch.length);
    for (let k = 0; k < maxLen; k += 1) {
      rows.push({
        kind: "pair",
        left: removedBatch[k],
        right: addedBatch[k],
      });
    }
  }

  return rows;
}

function renderTokensWithInlineSpans(
  content: string,
  tokens: HighlightedTokenView[] | undefined,
  inlineChanges: InlineDiffSpan[] | undefined,
  lineKind: DiffLineView["kind"],
  isDark: boolean,
): ReactNode {
  const emphClass =
    lineKind === "added"
      ? "rounded-[2px] bg-emerald-500/30 dark:bg-emerald-400/28"
      : lineKind === "removed"
        ? "rounded-[2px] bg-rose-500/30 dark:bg-rose-400/28"
        : "";

  const hasTokens = tokens && tokens.length > 0;
  const hasChanges = inlineChanges && inlineChanges.length > 0 && emphClass.length > 0;

  if (!hasTokens && !hasChanges) {
    return content;
  }

  if (!hasTokens && hasChanges) {
    const parts: ReactNode[] = [];
    let cursor = 0;
    for (let i = 0; i < inlineChanges.length; i += 1) {
      const span = inlineChanges[i];
      const s = Math.max(cursor, Math.min(span.start, content.length));
      const e = Math.max(s, Math.min(span.end, content.length));
      if (s > cursor) {
        parts.push(<span key={`p-${i}`}>{content.slice(cursor, s)}</span>);
      }
      if (e > s) {
        parts.push(
          <span key={`c-${i}`} className={emphClass}>
            {content.slice(s, e)}
          </span>,
        );
      }
      cursor = e;
    }
    if (cursor < content.length) {
      parts.push(<span key="tail">{content.slice(cursor)}</span>);
    }
    return parts;
  }

  if (hasTokens && !hasChanges) {
    return tokens.map((tok, idx) => (
      <span
        key={idx}
        style={{
          color: resolveTokenColor(tok, isDark),
          fontWeight: tok.bold ? 600 : undefined,
          fontStyle: tok.italic ? "italic" : undefined,
        }}
      >
        {tok.text}
      </span>
    ));
  }

  const nodes: ReactNode[] = [];
  let offset = 0;
  let changeIdx = 0;
  const spans = inlineChanges!;

  for (let tIdx = 0; tIdx < tokens!.length; tIdx += 1) {
    const tok = tokens![tIdx];
    const tokStart = offset;
    const tokEnd = offset + tok.text.length;
    offset = tokEnd;

    const style: React.CSSProperties = {
      color: resolveTokenColor(tok, isDark),
      fontWeight: tok.bold ? 600 : undefined,
      fontStyle: tok.italic ? "italic" : undefined,
    };

    while (changeIdx < spans.length && spans[changeIdx].end <= tokStart) {
      changeIdx += 1;
    }

    if (changeIdx >= spans.length || spans[changeIdx].start >= tokEnd) {
      nodes.push(
        <span key={tIdx} style={style}>
          {tok.text}
        </span>,
      );
      continue;
    }

    let subPos = tokStart;
    let c = changeIdx;
    while (subPos < tokEnd) {
      while (c < spans.length && spans[c].end <= subPos) c += 1;
      if (c >= spans.length || spans[c].start >= tokEnd) {
        nodes.push(
          <span key={`${tIdx}-${subPos}`} style={style}>
            {tok.text.slice(subPos - tokStart)}
          </span>,
        );
        break;
      }
      if (spans[c].start > subPos) {
        const segEnd = Math.min(spans[c].start, tokEnd);
        nodes.push(
          <span key={`${tIdx}-${subPos}`} style={style}>
            {tok.text.slice(subPos - tokStart, segEnd - tokStart)}
          </span>,
        );
        subPos = segEnd;
      }
      const hiEnd = Math.min(spans[c].end, tokEnd);
      if (hiEnd > subPos) {
        nodes.push(
          <span key={`${tIdx}-h-${subPos}`} style={style} className={emphClass}>
            {tok.text.slice(subPos - tokStart, hiEnd - tokStart)}
          </span>,
        );
        subPos = hiEnd;
      }
    }
  }

  return nodes;
}

const InlineCommentComposer = memo(function InlineCommentComposer({
  target,
  onCancel,
  onSave,
}: {
  target: ActiveCommentTarget;
  onCancel: () => void;
  onSave: (comment: string) => void;
}) {
  const { t } = useI18n();
  const [text, setText] = useState("");
  const commentIme = useImeCompositionGuard();
  const submit = () => {
    const trimmed = text.trim();
    if (!trimmed) return;
    onSave(trimmed);
    setText("");
  };

  const isMultiLine =
    target.startLine != null &&
    target.endLine != null &&
    target.startLine !== target.endLine;
  const locationLabel = formatPathLineRange(
    target.path,
    target.startLine ?? target.lineNumber,
    target.endLine ?? target.lineNumber,
    target.side,
  );

  return (
    <div className="border-border bg-elevated/95 border-x border-b px-3 py-2 font-sans">
      <div className="text-muted-foreground mb-1 flex items-center justify-between gap-2 text-[11px]">
        <div className="flex min-w-0 items-center gap-2">
          <span
            title={locationLabel}
            className={cn(
              "text-foreground/90 min-w-0 truncate font-mono font-medium",
              !isMultiLine && target.lineNumber != null && "max-w-[65%] shrink-0",
            )}
          >
            {locationLabel}
          </span>
          {!isMultiLine && target.lineNumber != null && (
            <span className="text-muted-foreground/75 hidden min-w-0 truncate text-[10.5px] sm:inline">
              {t("review.multiLineHint")}
            </span>
          )}
        </div>
        <button
          type="button"
          onClick={onCancel}
          className="hover:text-foreground inline-flex size-4 shrink-0 cursor-pointer items-center justify-center rounded"
        >
          <X className="size-3" />
        </button>
      </div>
      <Textarea
        autoFocus
        rows={2}
        value={text}
        onChange={(e) => setText(e.target.value)}
        {...commentIme.compositionProps}
        onKeyDown={(e) => {
          if (commentIme.shouldIgnoreKeyDown(e)) return;
          if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey || !e.shiftKey)) {
            e.preventDefault();
            submit();
          }
        }}
        placeholder={t("review.inlineCommentPlaceholder")}
        className="min-h-[54px] resize-none rounded-[6px] px-2 py-1.5 text-[12px] leading-relaxed"
      />
      <div className="mt-1.5 flex items-center justify-end gap-1.5">
        <Button type="button" variant="ghost" size="xs" onClick={onCancel}>
          {t("common.cancel")}
        </Button>
        <Button type="button" size="xs" disabled={!text.trim()} onClick={submit}>
          {t("review.addComment")}
        </Button>
      </div>
    </div>
  );
});

function estimateRowHeight(row: DiffVirtualRow): number {
  switch (row.kind) {
    case "file_header":
      return 38;
    case "hunk_header":
      return 28;
    case "unified_line":
    case "split_pair":
      return 20;
    case "patrol_finding_list":
      return 68 + Math.max(0, row.findings.length - 1) * 56;
    case "comment_list":
      return 42 + Math.max(0, row.comments.length - 1) * 28;
    case "comment_composer":
      return 124;
    case "omission":
      return 30;
    case "context_gap":
      return 26;
    case "file_spacer":
      return 12;
  }
}

export const DiffFilesViewer = memo(function DiffFilesViewer({
  files,
  hostId = null,
  cwd = null,
  enableLocalFind,
  layout = "unified",
  wrapLines = false,
  virtualized = false,
  emptyMessage = "Working tree clean — no changes to review.",
  collapsedPaths,
  viewedPaths,
  comments,
  gitWriteMode = null,
  busyPaths,
  busyHunkKeys,
  untrackedPaths,
  onToggleCollapse,
  onToggleViewed,
  onQuoteLine,
  onAddComment,
  onDeleteComment,
  onOpenFile,
  onPrefetchFile,
  onStageFile,
  onUnstageFile,
  onDiscardFile,
  onStageHunk,
  onUnstageHunk,
  onDiscardHunk,
  patrolFindings,
  patrolCwd,
  jumpTarget,
  onFixPatrolFinding,
}: {
  files: DiffFileView[];
  hostId?: string | null;
  cwd?: string | null;
  enableLocalFind?: boolean;
  layout?: DiffLayoutMode;
  wrapLines?: boolean;
  virtualized?: boolean;
  emptyMessage?: string;
  collapsedPaths?: ReadonlySet<string>;
  viewedPaths?: ReadonlySet<string>;
  comments?: readonly ReviewCommentDraft[];
  gitWriteMode?: "unstaged" | "staged" | null;
  busyPaths?: ReadonlySet<string>;
  busyHunkKeys?: ReadonlySet<string>;
  untrackedPaths?: ReadonlySet<string>;
  onToggleCollapse?: (path: string) => void;
  onToggleViewed?: (path: string) => void;
  onQuoteLine?: (path: string, lineNumber: number | null, code: string) => void;
  onAddComment?: (draft: Omit<ReviewCommentDraft, "id">) => void;
  onDeleteComment?: (id: string) => void;
  onOpenFile?: (path: string, focusLine?: number | null) => void;
  onPrefetchFile?: (path: string) => void;
  onStageFile?: (path: string) => void;
  onUnstageFile?: (path: string) => void;
  onDiscardFile?: (path: string) => void;
  onStageHunk?: (path: string, hunk: IndexedDiffHunk) => void;
  onUnstageHunk?: (path: string, hunk: IndexedDiffHunk) => void;
  onDiscardHunk?: (path: string, hunk: IndexedDiffHunk) => void;
  patrolFindings?: readonly PrPatrolFinding[];
  patrolCwd?: string;
  jumpTarget?: { path: string; line: number; nonce: number } | null;
  onFixPatrolFinding?: (finding: PrPatrolFinding) => void;
}) {
  const { t } = useI18n();
  const isDark = useOrbitSelector((s) => s.resolvedTheme === "dark");
  const [localCollapsed, setLocalCollapsed] = useState<Set<string>>(() => new Set());
  const [activeComment, setActiveComment] = useState<ActiveCommentTarget | null>(null);
  const [fileContextCache, setFileContextCache] = useState<Record<string, CachedFileContext>>({});
  const [expandedGaps, setExpandedGaps] = useState<Record<string, GapExpansionState>>({});
  const [loadingGapKey, setLoadingGapKey] = useState<string | null>(null);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [findActiveIndex, setFindActiveIndex] = useState(0);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const findInputRef = useRef<HTMLInputElement | null>(null);
  const [expandedPatrolKeys, setExpandedPatrolKeys] = useState<Set<string>>(() => new Set());
  const [highlightedRowKey, setHighlightedRowKey] = useState<string | null>(null);
  const highlightTimerRef = useRef<number | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const fileContextCacheRef = useRef(fileContextCache);
  fileContextCacheRef.current = fileContextCache;
  const findIme = useImeCompositionGuard();

  const localFindEnabled = enableLocalFind ?? virtualized;
  const canExpandHiddenContext = Boolean(hostId && cwd);

  // Reset cached context files when host/workspace changes
  useEffect(() => {
    setFileContextCache({});
    setExpandedGaps({});
    setLoadingGapKey(null);
  }, [hostId, cwd]);

  const handleExpandContextGap = useCallback(
    async (filePath: string, gapKey: string, mode: "chunk" | "all") => {
      if (!hostId || !cwd) return;
      if (!fileContextCacheRef.current[filePath]) {
        setLoadingGapKey(gapKey);
        try {
          const wf = await orbitIpc.readWorkspaceFile(hostId, cwd, filePath);
          const rawLines = wf.content.split(/\r?\n/);
          const highlighted = new Map<number, HighlightedTokenView[]>();
          let hlLines = wf.highlighted_lines;
          if ((!hlLines || hlLines.length === 0) && wf.content.length > 0) {
            try {
              hlLines = await orbitIpc.highlightWorkspaceCode(wf.language || "text", wf.content);
            } catch {
              hlLines = [];
            }
          }
          for (const hl of hlLines ?? []) {
            highlighted.set(hl.line_number, hl.tokens);
          }
          setFileContextCache((prev) => ({
            ...prev,
            [filePath]: { lines: rawLines, highlighted },
          }));
        } catch {
          return;
        } finally {
          setLoadingGapKey((cur) => (cur === gapKey ? null : cur));
        }
      }
      setExpandedGaps((prev) => {
        const cur = prev[gapKey] ?? { shownCount: 0, showAll: false };
        return {
          ...prev,
          [gapKey]:
            mode === "all"
              ? { shownCount: cur.shownCount, showAll: true }
              : { shownCount: cur.shownCount + CONTEXT_EXPAND_CHUNK_SIZE, showAll: cur.showAll },
        };
      });
    },
    [hostId, cwd],
  );

  const effectiveCollapsed = collapsedPaths ?? localCollapsed;

  const patrolIndex = useMemo(
    () => indexPatrolFindings(patrolFindings ?? [], patrolCwd),
    [patrolFindings, patrolCwd],
  );

  useEffect(() => {
    setExpandedPatrolKeys(new Set());
  }, [patrolFindings]);

  useEffect(() => {
    return () => {
      if (highlightTimerRef.current !== null) {
        window.clearTimeout(highlightTimerRef.current);
      }
    };
  }, []);

  const togglePatrolAnchor = useCallback((anchorKey: string) => {
    setExpandedPatrolKeys((prev) => {
      const next = new Set(prev);
      if (next.has(anchorKey)) next.delete(anchorKey);
      else next.add(anchorKey);
      return next;
    });
  }, []);

  const handleToggleCollapse = useCallback(
    (path: string) => {
      if (onToggleCollapse) {
        onToggleCollapse(path);
      } else {
        setLocalCollapsed((prev) => {
          const next = new Set(prev);
          if (next.has(path)) next.delete(path);
          else next.add(path);
          return next;
        });
      }
    },
    [onToggleCollapse],
  );

  const handleSelectCommentLine = useCallback(
    (params: {
      file: DiffFileView;
      fileKey: string;
      side: "old" | "new";
      lineNumber: number | null;
      code: string;
      anchorKey: string;
      shiftKey: boolean;
    }) => {
      const { file, fileKey, side, lineNumber, code, anchorKey, shiftKey } = params;
      setActiveComment((prev) => {
        if (
          shiftKey &&
          prev &&
          prev.fileKey === fileKey &&
          prev.side === side &&
          prev.anchorLine != null &&
          lineNumber != null
        ) {
          const { startLine, endLine } = normalizeLineRange(prev.anchorLine, lineNumber);
          const snippet =
            extractRangeSnippetFromDiffFile(file, side, startLine, endLine) || code;
          return {
            fileKey,
            path: file.path,
            side,
            anchorLine: prev.anchorLine,
            startLine,
            endLine,
            lineNumber: endLine,
            code: snippet,
            anchorKey: `${side}:${endLine}`,
          };
        }
        return {
          fileKey,
          path: file.path,
          side,
          anchorLine: lineNumber,
          startLine: lineNumber,
          endLine: lineNumber,
          lineNumber,
          code,
          anchorKey,
        };
      });
    },
    [],
  );

  const commentsByFileAndAnchor = useMemo(() => {
    const byFile = new Map<string, Map<string, ReviewCommentDraft[]>>();
    const rangesByFile = new Map<
      string,
      Array<{ side: "old" | "new"; startLine: number; endLine: number }>
    >();
    const countByFile = new Map<string, number>();
    for (const c of comments ?? []) {
      countByFile.set(c.path, (countByFile.get(c.path) ?? 0) + 1);
      let anchorMap = byFile.get(c.path);
      if (!anchorMap) {
        anchorMap = new Map();
        byFile.set(c.path, anchorMap);
      }
      const bounds = resolveDraftLineBounds(c);
      const anchorLine = bounds.endLine ?? c.lineNumber;
      const ak = `${c.side}:${anchorLine ?? "na"}`;
      const list = anchorMap.get(ak);
      if (list) list.push(c);
      else anchorMap.set(ak, [c]);

      if (bounds.startLine != null && bounds.endLine != null) {
        let rangeList = rangesByFile.get(c.path);
        if (!rangeList) {
          rangeList = [];
          rangesByFile.set(c.path, rangeList);
        }
        rangeList.push({
          side: c.side,
          startLine: bounds.startLine,
          endLine: bounds.endLine,
        });
      }
    }
    return { byFile, rangesByFile, countByFile };
  }, [comments]);

  // Flatten all files, headers, hunks, lines, and comments into a single O(1)-virtualized row list
  const flatRows = useMemo<DiffVirtualRow[]>(() => {
    const out: DiffVirtualRow[] = [];
    for (let fIdx = 0; fIdx < files.length; fIdx += 1) {
      const file = files[fIdx];
      const fileKey = `${file.path}::${fIdx}`;
      const collapsed = effectiveCollapsed.has(file.path);
      const viewed = viewedPaths?.has(file.path) ?? false;
      const commentCount = commentsByFileAndAnchor.countByFile.get(file.path) ?? 0;
      const anchorMap = commentsByFileAndAnchor.byFile.get(file.path);
      const normFilePath = normalizePatrolPath(file.path, patrolCwd);
      const filePatrol = patrolIndex.byFile.get(normFilePath) ?? null;

      out.push({
        kind: "file_header",
        key: `fh:${fileKey}`,
        file,
        fileKey,
        collapsed,
        viewed,
        commentCount,
        patrolCount: filePatrol?.count ?? 0,
        patrolHighestSeverity: filePatrol?.highestSeverity ?? null,
      });

      if (!collapsed) {
        const tail = file.lines[file.lines.length - 1];
        const omissionNotice =
          tail && tail.kind === "meta" && tail.content.startsWith("…") ? tail.content : null;
        let hunksByLineIdx: Map<number, IndexedDiffHunk> | null = null;
        if (gitWriteMode) {
          hunksByLineIdx = new Map();
          for (const h of extractFileHunks(file)) {
            hunksByLineIdx.set(h.headerLineIndex, h);
          }
        }

        const canExpandFile =
          canExpandHiddenContext && file.lines.some((l) => l.new_line != null);
        const cachedFile = canExpandFile ? fileContextCache[file.path] : undefined;

        const emitGapBeforeHunk = (
          prevNewEnd: number,
          hunkHeaderContent: string,
          isSplit: boolean,
          hunkIdxTag: string,
        ) => {
          if (!canExpandFile) return;
          const bounds = parseHunkHeaderStart(hunkHeaderContent);
          if (!bounds) return;
          const gapStartNew = prevNewEnd + 1;
          const gapEndNew = bounds.newStart - 1;
          if (gapEndNew < gapStartNew) return;
          const totalGap = gapEndNew - gapStartNew + 1;
          const oldOffset = bounds.oldStart - bounds.newStart;
          const gapKey = `${file.path}::${gapStartNew}-${gapEndNew}`;
          const exp = expandedGaps[gapKey];
          const shownCount = exp
            ? exp.showAll
              ? totalGap
              : Math.min(totalGap, exp.shownCount)
            : 0;

          if (shownCount > 0 && cachedFile) {
            const expandedLines = buildExpandedContextLines(
              cachedFile,
              gapStartNew,
              gapStartNew + shownCount - 1,
              oldOffset,
            );
            for (let eIdx = 0; eIdx < expandedLines.length; eIdx += 1) {
              const ctxLine = expandedLines[eIdx];
              if (isSplit) {
                out.push({
                  kind: "split_pair",
                  key: `sg:${fileKey}:${hunkIdxTag}:${ctxLine.new_line}`,
                  file,
                  fileKey,
                  left: ctxLine,
                  right: ctxLine,
                  rowIdx: -100000 - eIdx,
                  isLastInFile: false,
                  patrolAnchorKey: null,
                  patrolFindings: null,
                });
              } else {
                out.push({
                  kind: "unified_line",
                  key: `ug:${fileKey}:${hunkIdxTag}:${ctxLine.new_line}`,
                  file,
                  fileKey,
                  line: ctxLine,
                  lineIdx: -100000 - eIdx,
                  isLastInFile: false,
                  patrolAnchorKey: null,
                  patrolFindings: null,
                });
              }
            }
          }

          const remaining = totalGap - shownCount;
          if (remaining > 0) {
            out.push({
              kind: "context_gap",
              key: `cg:${fileKey}:${gapKey}`,
              gapKey,
              filePath: file.path,
              hiddenCount: remaining,
              startNew: gapStartNew + shownCount,
              endNew: gapEndNew,
              startOld: Math.max(1, gapStartNew + shownCount + oldOffset),
            });
          }
        };

        if (layout === "unified") {
          const visibleLines: { line: DiffLineView; idx: number }[] = [];
          for (let lIdx = 0; lIdx < file.lines.length; lIdx += 1) {
            if (file.lines[lIdx].kind !== "meta") {
              visibleLines.push({ line: file.lines[lIdx], idx: lIdx });
            }
          }
          let prevNewEnd = 0;
          for (let vIdx = 0; vIdx < visibleLines.length; vIdx += 1) {
            const { line, idx } = visibleLines[vIdx];
            const isLast = vIdx === visibleLines.length - 1 && !omissionNotice;
            if (line.kind === "hunk_header") {
              emitGapBeforeHunk(prevNewEnd, line.content, false, String(idx));
              const hunk = hunksByLineIdx?.get(idx) ?? null;
              out.push({
                kind: "hunk_header",
                key: `uh:${fileKey}:${idx}`,
                file,
                content: line.content,
                hunk,
                hunkKey: `${file.path}::${idx}`,
              });
            } else {
              if (line.new_line != null && line.new_line > prevNewEnd) {
                prevNewEnd = line.new_line;
              }
              const newLn = line.kind === "removed" ? null : (line.new_line ?? line.old_line);
              const linePatrol =
                newLn != null && filePatrol ? (filePatrol.byLine.get(newLn) ?? null) : null;
              const patrolAnchorKey =
                linePatrol && newLn != null ? `${normFilePath}:${newLn}` : null;
              out.push({
                kind: "unified_line",
                key: `ul:${fileKey}:${idx}`,
                file,
                fileKey,
                line,
                lineIdx: idx,
                isLastInFile: isLast,
                patrolAnchorKey,
                patrolFindings: linePatrol,
              });
              if (patrolAnchorKey && linePatrol && expandedPatrolKeys.has(patrolAnchorKey)) {
                out.push({
                  kind: "patrol_finding_list",
                  key: `pf:${fileKey}:${patrolAnchorKey}:${idx}`,
                  anchorKey: patrolAnchorKey,
                  findings: linePatrol,
                });
              }
              const side: "old" | "new" = line.kind === "removed" ? "old" : "new";
              const targetLn = side === "old" ? line.old_line : (line.new_line ?? line.old_line);
              const ak = `${side}:${targetLn ?? `idx_${idx}`}`;
              const existingComments = anchorMap?.get(`${side}:${targetLn ?? "na"}`);
              if (existingComments && existingComments.length > 0 && targetLn != null) {
                out.push({
                  kind: "comment_list",
                  key: `cl:${fileKey}:${ak}`,
                  comments: existingComments,
                });
              }
              if (
                activeComment &&
                activeComment.fileKey === fileKey &&
                activeComment.anchorKey === ak
              ) {
                out.push({
                  kind: "comment_composer",
                  key: `cc:${fileKey}:${ak}`,
                  target: activeComment,
                });
              }
            }
          }
        } else {
          const splitRows = buildSplitRows(file.lines);
          let prevNewEnd = 0;
          for (let rIdx = 0; rIdx < splitRows.length; rIdx += 1) {
            const sr = splitRows[rIdx];
            const isLast = rIdx === splitRows.length - 1 && !omissionNotice;
            if (sr.kind === "header") {
              emitGapBeforeHunk(prevNewEnd, sr.headerText ?? "", true, String(rIdx));
              const headerIdx = sr.headerLineIdx ?? -1;
              const hunk = hunksByLineIdx?.get(headerIdx) ?? null;
              out.push({
                kind: "hunk_header",
                key: `sh:${fileKey}:${rIdx}`,
                file,
                content: sr.headerText ?? "",
                hunk,
                hunkKey: `${file.path}::${headerIdx}`,
              });
            } else {
              if (sr.right?.new_line != null && sr.right.new_line > prevNewEnd) {
                prevNewEnd = sr.right.new_line;
              }
              const rightLn = sr.right?.new_line ?? null;
              const linePatrol =
                rightLn != null && filePatrol ? (filePatrol.byLine.get(rightLn) ?? null) : null;
              const patrolAnchorKey =
                linePatrol && rightLn != null ? `${normFilePath}:${rightLn}` : null;
              out.push({
                kind: "split_pair",
                key: `sp:${fileKey}:${rIdx}`,
                file,
                fileKey,
                left: sr.left,
                right: sr.right,
                rowIdx: rIdx,
                isLastInFile: isLast,
                patrolAnchorKey,
                patrolFindings: linePatrol,
              });
              if (patrolAnchorKey && linePatrol && expandedPatrolKeys.has(patrolAnchorKey)) {
                out.push({
                  kind: "patrol_finding_list",
                  key: `spf:${fileKey}:${patrolAnchorKey}:${rIdx}`,
                  anchorKey: patrolAnchorKey,
                  findings: linePatrol,
                });
              }
              const leftAk = `old:${sr.left?.old_line ?? `r_${rIdx}`}`;
              const rightAk = `new:${sr.right?.new_line ?? `r_${rIdx}`}`;
              const rowComments = [
                ...(sr.left?.old_line != null
                  ? (anchorMap?.get(`old:${sr.left.old_line}`) ?? [])
                  : []),
                ...(sr.right?.new_line != null
                  ? (anchorMap?.get(`new:${sr.right.new_line}`) ?? [])
                  : []),
              ];
              if (rowComments.length > 0) {
                out.push({
                  kind: "comment_list",
                  key: `scl:${fileKey}:${rIdx}`,
                  comments: rowComments,
                });
              }
              if (
                activeComment &&
                activeComment.fileKey === fileKey &&
                (activeComment.anchorKey === leftAk || activeComment.anchorKey === rightAk)
              ) {
                out.push({
                  kind: "comment_composer",
                  key: `scc:${fileKey}:${rIdx}`,
                  target: activeComment,
                });
              }
            }
          }
        }

        if (omissionNotice) {
          out.push({ kind: "omission", key: `om:${fileKey}`, notice: omissionNotice });
        }
      }

      if (fIdx < files.length - 1) {
        out.push({ kind: "file_spacer", key: `sp:${fileKey}` });
      }
    }
    return out;
  }, [
    files,
    layout,
    effectiveCollapsed,
    viewedPaths,
    commentsByFileAndAnchor,
    activeComment,
    gitWriteMode,
    canExpandHiddenContext,
    fileContextCache,
    expandedGaps,
    patrolIndex,
    patrolCwd,
    expandedPatrolKeys,
  ]);

  const virtualizer = useVirtualizer({
    count: virtualized ? flatRows.length : 0,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) => estimateRowHeight(flatRows[index]),
    overscan: 16,
    getItemKey: (index) => flatRows[index]?.key ?? index,
  });

  // Diff-local Cmd+F match calculation
  const normalizedFindQuery = findOpen ? findQuery.trim().toLowerCase() : "";
  const findMatches = useMemo<number[]>(() => {
    if (!normalizedFindQuery) return [];
    const hits: number[] = [];
    for (let i = 0; i < flatRows.length; i += 1) {
      const row = flatRows[i];
      if (row.kind === "unified_line") {
        if (row.line.content.toLowerCase().includes(normalizedFindQuery)) hits.push(i);
      } else if (row.kind === "split_pair") {
        const leftHit = row.left?.content.toLowerCase().includes(normalizedFindQuery) ?? false;
        const rightHit = row.right?.content.toLowerCase().includes(normalizedFindQuery) ?? false;
        if (leftHit || rightHit) hits.push(i);
      } else if (row.kind === "file_header") {
        if (row.file.path.toLowerCase().includes(normalizedFindQuery)) hits.push(i);
      }
    }
    return hits;
  }, [flatRows, normalizedFindQuery]);

  const findMatchRowSet = useMemo(() => new Set(findMatches), [findMatches]);
  const clampedFindIndex =
    findMatches.length > 0 ? Math.min(findActiveIndex, findMatches.length - 1) : 0;
  const activeFindRowIndex = findMatches.length > 0 ? findMatches[clampedFindIndex] : null;

  useEffect(() => {
    setFindActiveIndex(0);
  }, [normalizedFindQuery]);

  useEffect(() => {
    if (!findOpen || activeFindRowIndex === null) return;
    if (virtualized) {
      virtualizer.scrollToIndex(activeFindRowIndex, { align: "center" });
    }
  }, [findOpen, activeFindRowIndex, virtualized, virtualizer]);

  const stepDiffFind = useCallback(
    (delta: 1 | -1) => {
      if (findMatches.length === 0) return;
      setFindActiveIndex((prev) => {
        const len = findMatches.length;
        return ((prev + delta) % len + len) % len;
      });
    },
    [findMatches.length],
  );

  const openDiffFind = useCallback(() => {
    setFindOpen(true);
    window.requestAnimationFrame(() => {
      const el = findInputRef.current;
      if (!el) return;
      el.focus({ preventScroll: true });
      el.select();
    });
  }, []);

  const closeDiffFind = useCallback(() => {
    setFindOpen(false);
    containerRef.current?.focus({ preventScroll: true });
  }, []);

  // Capture Cmd+F / Ctrl+F when focus lives inside the Diff viewer or its enclosing Review tab
  useEffect(() => {
    if (!localFindEnabled) return;
    const onKeyDown = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod || e.shiftKey || e.altKey || e.key.toLowerCase() !== "f") return;
      const root = containerRef.current;
      if (!root || root.offsetParent === null) return;
      const active = document.activeElement;
      const reviewScope = root.closest("[data-orbit-review-tab='true']") ?? root;
      const targetEl = e.target instanceof Element ? e.target : null;
      const insideScope =
        (active instanceof Node && reviewScope.contains(active)) ||
        (targetEl !== null && reviewScope.contains(targetEl));
      if (!insideScope) return;
      e.preventDefault();
      e.stopPropagation();
      openDiffFind();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [localFindEnabled, openDiffFind]);

  // Jump smoothly to a Patrol finding's (file, line) row inside the virtualized diff stream
  useEffect(() => {
    if (!jumpTarget) return;
    const targetNorm = normalizePatrolPath(jumpTarget.path, patrolCwd);
    const anchorKey = `${targetNorm}:${jumpTarget.line}`;

    let matchIndex = -1;
    let fileHeaderFallback = -1;
    for (let i = 0; i < flatRows.length; i += 1) {
      const r = flatRows[i];
      if (
        r.kind === "file_header" &&
        fileHeaderFallback === -1 &&
        normalizePatrolPath(r.file.path, patrolCwd) === targetNorm
      ) {
        fileHeaderFallback = i;
      } else if (
        r.kind === "unified_line" &&
        normalizePatrolPath(r.file.path, patrolCwd) === targetNorm &&
        (r.line.kind !== "removed" && (r.line.new_line ?? r.line.old_line) === jumpTarget.line)
      ) {
        matchIndex = i;
        break;
      } else if (
        r.kind === "split_pair" &&
        normalizePatrolPath(r.file.path, patrolCwd) === targetNorm &&
        r.right?.new_line === jumpTarget.line
      ) {
        matchIndex = i;
        break;
      }
    }

    const scrollIdx = matchIndex !== -1 ? matchIndex : fileHeaderFallback;
    if (scrollIdx === -1) return;

    const matchedRow = flatRows[scrollIdx];
    if (
      (matchedRow.kind === "unified_line" || matchedRow.kind === "split_pair") &&
      matchedRow.patrolAnchorKey
    ) {
      setExpandedPatrolKeys((prev) => {
        if (prev.has(anchorKey)) return prev;
        const next = new Set(prev);
        next.add(anchorKey);
        return next;
      });
    }

    setHighlightedRowKey(matchedRow.key);
    if (highlightTimerRef.current !== null) {
      window.clearTimeout(highlightTimerRef.current);
    }
    highlightTimerRef.current = window.setTimeout(() => {
      highlightTimerRef.current = null;
      setHighlightedRowKey(null);
    }, 1800);

    if (virtualized) {
      window.requestAnimationFrame(() => {
        virtualizer.scrollToIndex(scrollIdx, { align: "center" });
      });
    }
    // Trigger strictly on new jumpTarget nonces
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jumpTarget?.nonce]);

  const codeWrapClass = wrapLines ? "whitespace-pre-wrap break-all" : "whitespace-pre";

  const renderRow = (row: DiffVirtualRow, flatIndex: number): ReactNode => {
    const isFindMatch = findOpen && findMatchRowSet.has(flatIndex);
    const isActiveFindMatch = findOpen && activeFindRowIndex === flatIndex;
    switch (row.kind) {
      case "file_spacer":
        return <div className="h-3" />;

      case "file_header": {
        const fileBusy = Boolean(busyPaths?.has(row.file.path));
        return (
          <div
            className={cn(
              "border-border-subtle group/diffhdr bg-surface-raised hover:bg-accent/70 flex h-[38px] w-full items-center justify-between gap-2 border px-3 transition-colors",
              row.collapsed ? "rounded-lg" : "rounded-t-lg",
              isActiveFindMatch
                ? "ring-1 ring-inset ring-amber-500/70 bg-amber-400/18 dark:bg-amber-400/16"
                : isFindMatch && "bg-amber-400/10 dark:bg-amber-400/10",
            )}
          >
            <button
              type="button"
              onClick={() => handleToggleCollapse(row.file.path)}
              className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
            >
              {row.collapsed ? (
                <ChevronRight className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
              ) : (
                <ChevronDown className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
              )}
              <span
                className={cn(
                  "truncate font-mono text-[12.5px] font-medium",
                  row.viewed ? "text-muted-foreground line-through" : "text-foreground",
                )}
              >
                {row.file.path}
              </span>
              {row.commentCount > 0 && (
                <span className="bg-sky-500/15 text-sky-700 dark:text-sky-300 shrink-0 rounded-full px-1.5 py-0.25 font-sans text-[10.5px] font-medium">
                  {row.commentCount}
                </span>
              )}
              {row.patrolCount > 0 && row.patrolHighestSeverity && (
                <Badge
                  variant={patrolSeverityVariant(row.patrolHighestSeverity)}
                  className="tabular-nums h-[17px] shrink-0 px-1.5 font-mono text-[10px]"
                >
                  <ShieldAlert className="size-[10px]" />
                  <span>
                    {row.patrolHighestSeverity} · {row.patrolCount}
                  </span>
                </Badge>
              )}
            </button>
            <div className="tabular-nums flex shrink-0 items-center gap-1.5 font-mono text-[12px]">
              {row.file.additions > 0 && (
                <span className="text-emerald-600 dark:text-emerald-400">
                  +{row.file.additions}
                </span>
              )}
              {row.file.deletions > 0 && (
                <span className="text-rose-600 dark:text-rose-400">-{row.file.deletions}</span>
              )}
              {gitWriteMode === "unstaged" && (onStageFile || onDiscardFile) && (
                <div className="flex items-center gap-1 font-sans">
                  {onStageFile && (
                    <button
                      type="button"
                      disabled={fileBusy}
                      title={t("review.stageFileTitle", { path: row.file.path })}
                      onClick={() => onStageFile(row.file.path)}
                      className="border-border text-muted-foreground hover:border-emerald-500/35 hover:bg-emerald-500/[0.10] hover:text-emerald-700 dark:hover:text-emerald-300 inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-1.5 text-[11px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                    >
                      {fileBusy ? (
                        <Loader2 className="h-[11px] w-[11px] animate-spin" />
                      ) : (
                        <Plus className="h-[11px] w-[11px]" />
                      )}
                      <span>{t("review.stage")}</span>
                    </button>
                  )}
                  {onDiscardFile && (
                    <button
                      type="button"
                      disabled={fileBusy}
                      title={t("review.discardFileTitle", { path: row.file.path })}
                      onClick={() => onDiscardFile(row.file.path)}
                      className="border-border text-muted-foreground hover:border-rose-500/35 hover:bg-rose-500/[0.10] hover:text-rose-600 dark:hover:text-rose-300 inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-1.5 text-[11px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                    >
                      <RotateCcw className="h-[10.5px] w-[10.5px]" />
                      <span>{t("review.discard")}</span>
                    </button>
                  )}
                </div>
              )}
              {gitWriteMode === "staged" && onUnstageFile && (
                <button
                  type="button"
                  disabled={fileBusy}
                  title={t("review.unstageFileTitle", { path: row.file.path })}
                  onClick={() => onUnstageFile(row.file.path)}
                  className="border-border text-muted-foreground hover:bg-black/[0.06] hover:text-foreground dark:hover:bg-white/[0.08] inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-1.5 font-sans text-[11px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                >
                  {fileBusy ? (
                    <Loader2 className="h-[11px] w-[11px] animate-spin" />
                  ) : (
                    <Minus className="h-[11px] w-[11px]" />
                  )}
                  <span>{t("review.unstage")}</span>
                </button>
              )}
              {onToggleViewed && (
                <button
                  type="button"
                  title={row.viewed ? t("review.markUnviewed") : t("review.markViewed")}
                  onClick={() => onToggleViewed(row.file.path)}
                  className={cn(
                    "inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-1.5 font-sans text-[11px] transition-colors",
                    row.viewed
                      ? "border-emerald-500/35 bg-emerald-500/12 text-emerald-700 dark:text-emerald-400"
                      : "border-border text-muted-foreground hover:text-foreground hover:bg-black/[0.05] dark:hover:bg-white/[0.06]",
                  )}
                >
                  <Check className="h-[11px] w-[11px]" />
                  <span>{t("review.viewed")}</span>
                </button>
              )}
              {onOpenFile && (
                <button
                  type="button"
                  title={t("review.openFullFile")}
                  onPointerDown={(e) => {
                    if (shouldPrefetchOnPointerDown(e)) {
                      onPrefetchFile?.(row.file.path);
                    }
                  }}
                  onClick={() => onOpenFile(row.file.path)}
                  className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md opacity-70 transition-[color,background-color,opacity] hover:bg-black/[0.06] hover:opacity-100 dark:hover:bg-white/[0.08]"
                >
                  <FileCode2 className="h-[13.5px] w-[13.5px] stroke-[1.75]" />
                </button>
              )}
            </div>
          </div>
        );
      }

      case "hunk_header": {
        const isUntracked = Boolean(untrackedPaths?.has(row.file.path));
        const hunkBusy = Boolean(busyHunkKeys?.has(row.hunkKey) || busyPaths?.has(row.file.path));
        const hunk = row.hunk;
        const canActOnHunk = Boolean(gitWriteMode && hunk && !isUntracked);
        return (
          <div className="border-border group/hunk flex h-[28px] items-center justify-between gap-2 border-x bg-sky-500/[0.08] px-3 font-mono text-[11.5px] text-sky-800 select-none dark:bg-sky-500/[0.08] dark:text-sky-300/90">
            <span className="min-w-0 flex-1 truncate">{row.content}</span>
            {canActOnHunk && hunk && (
              <div className="flex shrink-0 items-center gap-1 font-sans">
                {hunk.truncated ? (
                  <span
                    title={t("review.hunkTruncatedHint")}
                    className="text-muted-foreground px-1 text-[10.5px] opacity-70"
                  >
                    …
                  </span>
                ) : gitWriteMode === "unstaged" ? (
                  <>
                    {onStageHunk && (
                      <button
                        type="button"
                        disabled={hunkBusy}
                        onClick={() => onStageHunk(row.file.path, hunk)}
                        className="text-sky-800/80 hover:bg-emerald-500/[0.15] hover:text-emerald-700 dark:text-sky-200/80 dark:hover:bg-emerald-400/[0.16] dark:hover:text-emerald-300 inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 text-[10.5px] font-medium opacity-80 transition-[color,background-color,opacity] group-hover/hunk:opacity-100 focus-visible:opacity-100 disabled:pointer-events-none disabled:opacity-45"
                      >
                        {hunkBusy ? (
                          <Loader2 className="h-[10.5px] w-[10.5px] animate-spin" />
                        ) : (
                          <Plus className="h-[10.5px] w-[10.5px]" />
                        )}
                        <span>{t("review.stageHunk")}</span>
                      </button>
                    )}
                    {onDiscardHunk && (
                      <button
                        type="button"
                        disabled={hunkBusy}
                        onClick={() => onDiscardHunk(row.file.path, hunk)}
                        className="text-sky-800/75 hover:bg-rose-500/[0.15] hover:text-rose-600 dark:text-sky-200/75 dark:hover:bg-rose-400/[0.16] dark:hover:text-rose-300 inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 text-[10.5px] font-medium opacity-80 transition-[color,background-color,opacity] group-hover/hunk:opacity-100 focus-visible:opacity-100 disabled:pointer-events-none disabled:opacity-45"
                      >
                        <RotateCcw className="h-[10px] w-[10px]" />
                        <span>{t("review.discardHunk")}</span>
                      </button>
                    )}
                  </>
                ) : (
                  onUnstageHunk && (
                    <button
                      type="button"
                      disabled={hunkBusy}
                      onClick={() => onUnstageHunk(row.file.path, hunk)}
                      className="text-sky-800/80 hover:bg-black/[0.08] hover:text-foreground dark:text-sky-200/80 dark:hover:bg-white/[0.10] dark:hover:text-foreground inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 text-[10.5px] font-medium opacity-80 transition-[color,background-color,opacity] group-hover/hunk:opacity-100 focus-visible:opacity-100 disabled:pointer-events-none disabled:opacity-45"
                    >
                      {hunkBusy ? (
                        <Loader2 className="h-[10.5px] w-[10.5px] animate-spin" />
                      ) : (
                        <Minus className="h-[10.5px] w-[10.5px]" />
                      )}
                      <span>{t("review.unstageHunk")}</span>
                    </button>
                  )
                )}
              </div>
            )}
          </div>
        );
      }

      case "unified_line": {
        const { line, file, fileKey, lineIdx, isLastInFile, patrolAnchorKey, patrolFindings: linePatrol } =
          row;
        const rowClass =
          line.kind === "added"
            ? "bg-emerald-500/[0.11] dark:bg-emerald-500/[0.10]"
            : line.kind === "removed"
              ? "bg-rose-500/[0.10] dark:bg-rose-500/[0.10]"
              : "bg-surface-recessed text-ink-secondary";
        const prefix = line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " ";
        const side: "old" | "new" = line.kind === "removed" ? "old" : "new";
        const targetLn = side === "old" ? line.old_line : (line.new_line ?? line.old_line);
        const anchorKey = `${side}:${targetLn ?? `idx_${lineIdx}`}`;
        const inActiveSelection =
          activeComment?.fileKey === fileKey &&
          isLineWithinSelection(targetLn, side, activeComment);
        const inSavedRange =
          !inActiveSelection &&
          (commentsByFileAndAnchor.rangesByFile
            .get(file.path)
            ?.some((r) => isLineWithinSelection(targetLn, side, r)) ??
            false);
        const topPatrol = linePatrol?.[0] ?? null;
        const isPatrolExpanded = Boolean(
          patrolAnchorKey && expandedPatrolKeys.has(patrolAnchorKey),
        );
        const isJumpHighlighted = highlightedRowKey === row.key;

        return (
          <div
            className={cn(
              "border-border group selectable-text flex w-full items-start border-x px-2 font-mono text-[12px] leading-[20px] transition-colors",
              isLastInFile && !isPatrolExpanded && "rounded-b-lg border-b",
              rowClass,
              !inActiveSelection &&
                !inSavedRange &&
                topPatrol &&
                cn(patrolLineBorderClass(topPatrol.severity), "pl-[6px]"),
              isJumpHighlighted && "bg-signal-info-bg",
              inActiveSelection &&
                "border-l-2 border-l-sky-500/80 bg-sky-500/[0.08] pl-[6px] dark:border-l-sky-400/80 dark:bg-sky-400/[0.09]",
              inSavedRange &&
                "border-l-2 border-l-sky-500/45 pl-[6px] dark:border-l-sky-400/45",
              isActiveFindMatch
                ? "bg-amber-400/28 ring-1 ring-inset ring-amber-500/65 dark:bg-amber-400/24"
                : isFindMatch && "bg-amber-400/14 dark:bg-amber-400/14",
            )}
          >
            {(onOpenFile || onAddComment) && line.old_line != null ? (
              <button
                type="button"
                title={
                  onAddComment
                    ? t("review.gutterRangeOrOpenTitle", {
                        path: file.path,
                        line: line.old_line,
                      })
                    : `Open ${file.path} around line ${line.old_line}`
                }
                onPointerDown={(e) => {
                  if (!e.shiftKey && shouldPrefetchOnPointerDown(e)) onPrefetchFile?.(file.path);
                }}
                onClick={(e) => {
                  if (e.shiftKey && onAddComment) {
                    e.preventDefault();
                    handleSelectCommentLine({
                      file,
                      fileKey,
                      side,
                      lineNumber: targetLn,
                      code: line.content,
                      anchorKey,
                      shiftKey: true,
                    });
                    return;
                  }
                  onOpenFile?.(file.path, line.old_line);
                }}
                className="tabular-nums hover:text-foreground w-8 shrink-0 cursor-pointer pr-1.5 text-right text-[11.5px] text-zinc-400 select-none hover:underline dark:text-zinc-600"
              >
                {line.old_line}
              </button>
            ) : (
              <span className="tabular-nums w-8 shrink-0 pr-1.5 text-right text-[11.5px] text-zinc-400 select-none dark:text-zinc-600">
                {line.old_line ?? ""}
              </span>
            )}
            {(onOpenFile || onAddComment) && line.new_line != null ? (
              <button
                type="button"
                title={
                  onAddComment
                    ? t("review.gutterRangeOrOpenTitle", {
                        path: file.path,
                        line: line.new_line,
                      })
                    : `Open ${file.path} at line ${line.new_line}`
                }
                onPointerDown={(e) => {
                  if (!e.shiftKey && shouldPrefetchOnPointerDown(e)) onPrefetchFile?.(file.path);
                }}
                onClick={(e) => {
                  if (e.shiftKey && onAddComment) {
                    e.preventDefault();
                    handleSelectCommentLine({
                      file,
                      fileKey,
                      side,
                      lineNumber: targetLn,
                      code: line.content,
                      anchorKey,
                      shiftKey: true,
                    });
                    return;
                  }
                  onOpenFile?.(file.path, line.new_line);
                }}
                className="tabular-nums hover:text-foreground w-8 shrink-0 cursor-pointer pr-2 text-right text-[11.5px] text-zinc-400 select-none hover:underline dark:text-zinc-600"
              >
                {line.new_line}
              </button>
            ) : (
              <span className="tabular-nums w-8 shrink-0 pr-2 text-right text-[11.5px] text-zinc-400 select-none dark:text-zinc-600">
                {line.new_line ?? ""}
              </span>
            )}
            {onAddComment || onQuoteLine ? (
              <div className="mr-1 flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                {onAddComment && (
                  <button
                    type="button"
                    title={t("review.commentLineTitle")}
                    onClick={(e) =>
                      handleSelectCommentLine({
                        file,
                        fileKey,
                        side,
                        lineNumber: targetLn,
                        code: line.content,
                        anchorKey,
                        shiftKey: e.shiftKey,
                      })
                    }
                    className="inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded hover:bg-black/10 dark:hover:bg-white/15"
                  >
                    <MessageSquareText className="text-foreground h-[12px] w-[12px]" />
                  </button>
                )}
                {onQuoteLine && (
                  <button
                    type="button"
                    title="Quick-quote line into Composer"
                    onClick={() => onQuoteLine(file.path, targetLn, line.content)}
                    className="inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded hover:bg-black/10 dark:hover:bg-white/15"
                  >
                    <MessageSquarePlus className="text-foreground h-[12px] w-[12px]" />
                  </button>
                )}
              </div>
            ) : (
              <span className="w-3 shrink-0 select-none" />
            )}
            <span className="w-3.5 shrink-0 opacity-60 select-none">{prefix}</span>
            <span
              className={cn(
                "min-w-0 flex-1",
                !wrapLines && "overflow-x-auto [scrollbar-width:none]",
                codeWrapClass,
              )}
            >
              {renderTokensWithInlineSpans(
                line.content,
                line.tokens,
                line.inline_changes,
                line.kind,
                isDark,
              )}
            </span>
            {topPatrol && patrolAnchorKey && linePatrol && (
              <button
                type="button"
                title={topPatrol.title}
                onClick={() => togglePatrolAnchor(patrolAnchorKey)}
                className={cn(
                  "mt-[1.5px] ml-2 inline-flex h-[17px] shrink-0 cursor-pointer items-center gap-1 rounded-[5px] border px-1.5 font-sans text-[10px] font-medium transition-opacity hover:opacity-85",
                  patrolInlinePillClass(topPatrol.severity),
                )}
              >
                <ShieldAlert className="size-[10px] shrink-0" />
                <span className="font-mono">{topPatrol.severity}</span>
                {linePatrol.length > 1 && (
                  <span className="tabular-nums opacity-80">+{linePatrol.length - 1}</span>
                )}
              </button>
            )}
          </div>
        );
      }

      case "split_pair": {
        const { left, right, file, fileKey, rowIdx, isLastInFile, patrolAnchorKey, patrolFindings: linePatrol } =
          row;
        const topPatrol = linePatrol?.[0] ?? null;
        const isPatrolExpanded = Boolean(
          patrolAnchorKey && expandedPatrolKeys.has(patrolAnchorKey),
        );
        const isJumpHighlighted = highlightedRowKey === row.key;
        const leftAk = `old:${left?.old_line ?? `r_${rowIdx}`}`;
        const rightAk = `new:${right?.new_line ?? `r_${rowIdx}`}`;
        const savedRanges = commentsByFileAndAnchor.rangesByFile.get(file.path);
        const leftSelected =
          Boolean(left) &&
          activeComment?.fileKey === fileKey &&
          isLineWithinSelection(left?.old_line, "old", activeComment);
        const leftSaved =
          !leftSelected &&
          Boolean(left) &&
          (savedRanges?.some((r) => isLineWithinSelection(left?.old_line, "old", r)) ?? false);
        const rightSelected =
          Boolean(right) &&
          activeComment?.fileKey === fileKey &&
          isLineWithinSelection(right?.new_line, "new", activeComment);
        const rightSaved =
          !rightSelected &&
          Boolean(right) &&
          (savedRanges?.some((r) => isLineWithinSelection(right?.new_line, "new", r)) ?? false);

        return (
          <div
            className={cn(
              "border-border-subtle divide-border-subtle selectable-text bg-surface-recessed grid w-full grid-cols-2 divide-x border-x font-mono text-[12px] leading-[20px]",
              isLastInFile && !isPatrolExpanded && "overflow-hidden rounded-b-lg border-b",
              isActiveFindMatch
                ? "bg-amber-400/24 ring-1 ring-inset ring-amber-500/65 dark:bg-amber-400/20"
                : isFindMatch && "bg-amber-400/12 dark:bg-amber-400/12",
            )}
          >
            {/* Old side */}
            <div
              className={cn(
                "group flex min-w-0 items-start px-1.5",
                left?.kind === "removed"
                  ? "bg-rose-500/[0.10] dark:bg-rose-500/[0.10]"
                  : left
                    ? "text-zinc-600 dark:text-zinc-400"
                    : "bg-black/[0.015] dark:bg-white/[0.01]",
                leftSelected &&
                  "border-l-2 border-l-sky-500/80 bg-sky-500/[0.08] pl-[4px] dark:border-l-sky-400/80 dark:bg-sky-400/[0.09]",
                leftSaved && "border-l-2 border-l-sky-500/45 pl-[4px] dark:border-l-sky-400/45",
              )}
            >
              {(onOpenFile || onAddComment) && left?.old_line != null ? (
                <button
                  type="button"
                  title={
                    onAddComment
                      ? t("review.gutterRangeOrOpenTitle", {
                          path: file.path,
                          line: left.old_line,
                        })
                      : `Open ${file.path} around line ${left.old_line}`
                  }
                  onPointerDown={(e) => {
                    if (!e.shiftKey && shouldPrefetchOnPointerDown(e)) onPrefetchFile?.(file.path);
                  }}
                  onClick={(e) => {
                    if (e.shiftKey && onAddComment) {
                      e.preventDefault();
                      handleSelectCommentLine({
                        file,
                        fileKey,
                        side: "old",
                        lineNumber: left.old_line,
                        code: left.content,
                        anchorKey: leftAk,
                        shiftKey: true,
                      });
                      return;
                    }
                    onOpenFile?.(file.path, left.old_line);
                  }}
                  className="tabular-nums hover:text-foreground w-8 shrink-0 cursor-pointer pr-1.5 text-right text-[11.5px] text-zinc-400 select-none hover:underline dark:text-zinc-600"
                >
                  {left.old_line}
                </button>
              ) : (
                <span className="tabular-nums w-8 shrink-0 pr-1.5 text-right text-[11.5px] text-zinc-400 select-none dark:text-zinc-600">
                  {left?.old_line ?? ""}
                </span>
              )}
              {left && (onAddComment || onQuoteLine) && (
                <div className="mr-0.5 flex shrink-0 items-center opacity-0 transition-opacity group-hover:opacity-100">
                  {onAddComment && (
                    <button
                      type="button"
                      title={t("review.commentLineTitle")}
                      onClick={(e) =>
                        handleSelectCommentLine({
                          file,
                          fileKey,
                          side: "old",
                          lineNumber: left.old_line,
                          code: left.content,
                          anchorKey: leftAk,
                          shiftKey: e.shiftKey,
                        })
                      }
                      className="inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded hover:bg-black/10 dark:hover:bg-white/15"
                    >
                      <MessageSquareText className="text-foreground h-[11.5px] w-[11.5px]" />
                    </button>
                  )}
                  {onQuoteLine && (
                    <button
                      type="button"
                      title="Quote old line into Composer"
                      onClick={() => onQuoteLine(file.path, left.old_line, left.content)}
                      className="inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded hover:bg-black/10 dark:hover:bg-white/15"
                    >
                      <MessageSquarePlus className="text-foreground h-[11.5px] w-[11.5px]" />
                    </button>
                  )}
                </div>
              )}
              <span className="w-3 shrink-0 opacity-60 select-none">
                {left?.kind === "removed" ? "-" : " "}
              </span>
              <span
                className={cn(
                  "min-w-0 flex-1",
                  !wrapLines && "overflow-x-auto [scrollbar-width:none]",
                  codeWrapClass,
                )}
              >
                {left
                  ? renderTokensWithInlineSpans(
                      left.content,
                      left.tokens,
                      left.inline_changes,
                      left.kind,
                      isDark,
                    )
                  : ""}
              </span>
            </div>

            {/* New side */}
            <div
              className={cn(
                "group flex min-w-0 items-start px-1.5 transition-colors",
                right?.kind === "added"
                  ? "bg-emerald-500/[0.11] dark:bg-emerald-500/[0.10]"
                  : right
                    ? "text-zinc-700 dark:text-zinc-300"
                    : "bg-black/[0.015] dark:bg-white/[0.01]",
                !rightSelected &&
                  !rightSaved &&
                  topPatrol &&
                  cn(patrolLineBorderClass(topPatrol.severity), "pl-[4px]"),
                isJumpHighlighted && "bg-signal-info-bg",
                rightSelected &&
                  "border-l-2 border-l-sky-500/80 bg-sky-500/[0.08] pl-[4px] dark:border-l-sky-400/80 dark:bg-sky-400/[0.09]",
                rightSaved &&
                  "border-l-2 border-l-sky-500/45 pl-[4px] dark:border-l-sky-400/45",
              )}
            >
              {(onOpenFile || onAddComment) && right?.new_line != null ? (
                <button
                  type="button"
                  title={
                    onAddComment
                      ? t("review.gutterRangeOrOpenTitle", {
                          path: file.path,
                          line: right.new_line,
                        })
                      : `Open ${file.path} at line ${right.new_line}`
                  }
                  onPointerDown={(e) => {
                    if (!e.shiftKey && shouldPrefetchOnPointerDown(e)) onPrefetchFile?.(file.path);
                  }}
                  onClick={(e) => {
                    if (e.shiftKey && onAddComment) {
                      e.preventDefault();
                      handleSelectCommentLine({
                        file,
                        fileKey,
                        side: "new",
                        lineNumber: right.new_line,
                        code: right.content,
                        anchorKey: rightAk,
                        shiftKey: true,
                      });
                      return;
                    }
                    onOpenFile?.(file.path, right.new_line);
                  }}
                  className="tabular-nums hover:text-foreground w-8 shrink-0 cursor-pointer pr-1.5 text-right text-[11.5px] text-zinc-400 select-none hover:underline dark:text-zinc-600"
                >
                  {right.new_line}
                </button>
              ) : (
                <span className="tabular-nums w-8 shrink-0 pr-1.5 text-right text-[11.5px] text-zinc-400 select-none dark:text-zinc-600">
                  {right?.new_line ?? ""}
                </span>
              )}
              {right && (onAddComment || onQuoteLine) && (
                <div className="mr-0.5 flex shrink-0 items-center opacity-0 transition-opacity group-hover:opacity-100">
                  {onAddComment && (
                    <button
                      type="button"
                      title={t("review.commentLineTitle")}
                      onClick={(e) =>
                        handleSelectCommentLine({
                          file,
                          fileKey,
                          side: "new",
                          lineNumber: right.new_line,
                          code: right.content,
                          anchorKey: rightAk,
                          shiftKey: e.shiftKey,
                        })
                      }
                      className="inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded hover:bg-black/10 dark:hover:bg-white/15"
                    >
                      <MessageSquareText className="text-foreground h-[11.5px] w-[11.5px]" />
                    </button>
                  )}
                  {onQuoteLine && (
                    <button
                      type="button"
                      title="Quote line into Composer"
                      onClick={() => onQuoteLine(file.path, right.new_line, right.content)}
                      className="inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded hover:bg-black/10 dark:hover:bg-white/15"
                    >
                      <MessageSquarePlus className="text-foreground h-[11.5px] w-[11.5px]" />
                    </button>
                  )}
                </div>
              )}
              <span className="w-3 shrink-0 opacity-60 select-none">
                {right?.kind === "added" ? "+" : " "}
              </span>
              <span
                className={cn(
                  "min-w-0 flex-1",
                  !wrapLines && "overflow-x-auto [scrollbar-width:none]",
                  codeWrapClass,
                )}
              >
                {right
                  ? renderTokensWithInlineSpans(
                      right.content,
                      right.tokens,
                      right.inline_changes,
                      right.kind,
                      isDark,
                    )
                  : ""}
              </span>
              {topPatrol && patrolAnchorKey && linePatrol && (
                <button
                  type="button"
                  title={topPatrol.title}
                  onClick={() => togglePatrolAnchor(patrolAnchorKey)}
                  className={cn(
                    "mt-[1.5px] ml-1.5 inline-flex h-[17px] shrink-0 cursor-pointer items-center gap-1 rounded-[5px] border px-1.5 font-sans text-[10px] font-medium transition-opacity hover:opacity-85",
                    patrolInlinePillClass(topPatrol.severity),
                  )}
                >
                  <ShieldAlert className="size-[10px] shrink-0" />
                  <span className="font-mono">{topPatrol.severity}</span>
                  {linePatrol.length > 1 && (
                    <span className="tabular-nums opacity-80">+{linePatrol.length - 1}</span>
                  )}
                </button>
              )}
            </div>
          </div>
        );
      }

      case "patrol_finding_list":
        return (
          <div className="border-border bg-surface-raised/95 flex flex-col gap-1.5 border-x border-b px-3 py-2 font-sans">
            {row.findings.map((finding) => (
              <div
                key={finding.id}
                className="border-border-subtle bg-surface-base flex flex-col gap-1 rounded-md border px-2.5 py-2"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 items-center gap-1.5">
                    <Badge
                      variant={patrolSeverityVariant(finding.severity)}
                      className="h-[17px] px-1.5 font-mono text-[10px]"
                    >
                      {finding.severity}
                    </Badge>
                    <span className="text-ink-tertiary text-[10.5px] font-medium">
                      {t(patrolCategoryI18nKey(finding.category))}
                    </span>
                    <span className="text-ink-faint tabular-nums font-mono text-[10.5px]">
                      {formatPatrolLineRange(finding.start_line, finding.end_line)}
                    </span>
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    {onFixPatrolFinding && (
                      <button
                        type="button"
                        onClick={() => onFixPatrolFinding(finding)}
                        className="border-border-default bg-surface-raised text-ink-primary hover:bg-accent inline-flex h-[20px] cursor-pointer items-center gap-1 rounded-[5px] border px-1.5 text-[10.5px] font-medium transition-colors"
                      >
                        <Wrench className="size-[10px]" />
                        <span>{t("review.patrolFixWithAgent")}</span>
                      </button>
                    )}
                    <button
                      type="button"
                      aria-label={t("common.close")}
                      onClick={() => togglePatrolAnchor(row.anchorKey)}
                      className="text-ink-tertiary hover:bg-accent hover:text-ink-primary inline-flex size-[20px] cursor-pointer items-center justify-center rounded"
                    >
                      <X className="size-[11px]" />
                    </button>
                  </div>
                </div>
                <div className="text-ink-primary text-[12px] font-medium leading-snug">
                  {finding.title}
                </div>
                {finding.summary.trim() && (
                  <p className="text-ink-secondary text-[11.5px] leading-snug">
                    {finding.summary}
                  </p>
                )}
                {finding.suggested_fix.trim() && (
                  <p className="text-ink-tertiary text-[11px] leading-snug">
                    <span className="text-ink-secondary font-medium">
                      {t("review.patrolSuggestedFixLabel")}{" "}
                    </span>
                    {finding.suggested_fix}
                  </p>
                )}
              </div>
            ))}
          </div>
        );

      case "comment_list":
        return (
          <div className="border-border bg-elevated/80 flex flex-col gap-1 border-x border-b px-3 py-1.5 font-sans text-[12px]">
            {row.comments.map((c) => {
              const bounds = resolveDraftLineBounds(c);
              const rangeLabel = formatLineRangeTag(bounds.startLine, bounds.endLine, c.side);
              return (
                <div key={c.id} className="flex items-start justify-between gap-2">
                  <div className="text-foreground/90 min-w-0 flex-1 whitespace-pre-wrap break-words">
                    <span className="text-sky-600 dark:text-sky-400 mr-1.5 font-mono text-[10.5px] font-medium">
                      {rangeLabel}
                    </span>
                    {c.comment}
                  </div>
                  {onDeleteComment && (
                    <button
                      type="button"
                      title={t("review.removeComment")}
                      onClick={() => onDeleteComment(c.id)}
                      className="text-muted-foreground hover:text-rose-500 inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded"
                    >
                      <Trash2 className="size-3" />
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        );

      case "comment_composer":
        return (
          <InlineCommentComposer
            target={row.target}
            onCancel={() => setActiveComment(null)}
            onSave={(comment) => {
              onAddComment?.({
                path: row.target.path,
                side: row.target.side,
                lineNumber: row.target.endLine ?? row.target.lineNumber,
                startLine: row.target.startLine,
                endLine: row.target.endLine,
                code: row.target.code,
                comment,
              });
              setActiveComment(null);
            }}
          />
        );

      case "omission":
        return (
          <div className="border-border-subtle text-ink-muted bg-surface-recessed rounded-b-lg border px-3 py-1.5 text-center font-mono text-[11.5px] select-none">
            {row.notice}
          </div>
        );

      case "context_gap": {
        const isLoading = loadingGapKey === row.gapKey;
        const canExpandChunk = row.hiddenCount > CONTEXT_EXPAND_CHUNK_SIZE;
        return (
          <div className="border-border-subtle bg-surface-base/90 text-ink-tertiary flex h-[26px] items-center justify-between gap-2 border-x border-y border-y-border-subtle/50 px-3 text-[11px] select-none">
            <span className="inline-flex items-center gap-1.5 tabular-nums">
              {isLoading ? (
                <Loader2 className="size-3 animate-spin text-ink-muted" />
              ) : (
                <UnfoldVertical className="size-3 text-ink-muted" />
              )}
              <span>{t("review.hiddenContextLines", { count: row.hiddenCount })}</span>
            </span>
            <div className="flex items-center gap-1.5">
              {canExpandChunk && (
                <button
                  type="button"
                  disabled={isLoading}
                  onClick={() => void handleExpandContextGap(row.filePath, row.gapKey, "chunk")}
                  className="hover:bg-accent hover:text-ink-primary inline-flex h-[20px] cursor-pointer items-center rounded-[5px] px-1.5 text-[11px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                >
                  {t("review.expandContextLines", { count: CONTEXT_EXPAND_CHUNK_SIZE })}
                </button>
              )}
              <button
                type="button"
                disabled={isLoading}
                onClick={() => void handleExpandContextGap(row.filePath, row.gapKey, "all")}
                className="hover:bg-accent hover:text-ink-primary inline-flex h-[20px] cursor-pointer items-center rounded-[5px] px-1.5 text-[11px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
              >
                {canExpandChunk
                  ? t("review.showAllHiddenContext")
                  : t("review.expandContextLines", { count: row.hiddenCount })}
              </button>
            </div>
          </div>
        );
      }
    }
  };

  if (files.length === 0) {
    return (
      <div className={cn(virtualized && "p-3")}>
        <div className="text-muted-foreground flex h-36 items-center justify-center rounded-xl border border-dashed text-[13px]">
          {emptyMessage}
        </div>
      </div>
    );
  }

  if (!virtualized) {
    return (
      <div ref={containerRef} className="min-w-0">
        {flatRows.map((row, idx) => (
          <div key={row.key}>{renderRow(row, idx)}</div>
        ))}
      </div>
    );
  }

  const virtualItems = virtualizer.getVirtualItems();
  const hasFindQuery = findQuery.trim().length > 0;
  const canStepFind = findMatches.length > 0;

  return (
    <div
      ref={containerRef}
      tabIndex={-1}
      data-orbit-diff-viewer="true"
      className="relative flex min-h-0 flex-1 flex-col overflow-hidden outline-none"
    >
      {localFindEnabled && findOpen && (
        <div
          role="search"
          aria-label={t("review.findInDiffPlaceholder")}
          className="floating-surface orbit-find-bar-enter absolute top-2.5 right-4 z-30 flex h-[36px] w-[312px] max-w-[calc(100%-24px)] items-center gap-1.5 rounded-xl border px-2.5"
        >
          <Search className="text-ink-tertiary size-[14px] shrink-0" aria-hidden="true" />
          <input
            ref={findInputRef}
            type="text"
            value={findQuery}
            onChange={(e) => setFindQuery(e.target.value)}
            {...findIme.compositionProps}
            onKeyDown={(e) => {
              if (findIme.shouldIgnoreKeyDown(e)) return;
              if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                closeDiffFind();
                return;
              }
              if (e.key === "Enter" || e.key === "ArrowDown") {
                e.preventDefault();
                stepDiffFind(e.key === "Enter" && e.shiftKey ? -1 : 1);
                return;
              }
              if (e.key === "ArrowUp") {
                e.preventDefault();
                stepDiffFind(-1);
              }
            }}
            placeholder={t("review.findInDiffPlaceholder")}
            aria-label={t("review.findInDiffPlaceholder")}
            spellCheck={false}
            autoCorrect="off"
            className="text-ink-primary placeholder:text-ink-muted min-w-0 flex-1 bg-transparent text-[13px] leading-[1.45] outline-none"
          />
          {hasFindQuery && (
            <span
              aria-live="polite"
              className={cn(
                "shrink-0 font-mono text-[11px] leading-[1.35] tabular-nums select-none",
                !canStepFind ? "text-signal-danger" : "text-ink-tertiary",
              )}
            >
              {canStepFind ? `${clampedFindIndex + 1} / ${findMatches.length}` : "0 / 0"}
            </span>
          )}
          <span aria-hidden="true" className="bg-border-subtle mx-0.5 h-[14px] w-px shrink-0" />
          <div className="flex shrink-0 items-center gap-0.5">
            <IconButton
              size="xs"
              tooltip={t("stage.findPrevious")}
              shortcut="⇧↵"
              disabled={!canStepFind}
              onClick={() => stepDiffFind(-1)}
            >
              <ChevronUp />
            </IconButton>
            <IconButton
              size="xs"
              tooltip={t("stage.findNext")}
              shortcut="↵"
              disabled={!canStepFind}
              onClick={() => stepDiffFind(1)}
            >
              <ChevronDown />
            </IconButton>
            <IconButton
              size="xs"
              tooltip={t("common.close")}
              shortcut="Esc"
              onClick={closeDiffFind}
            >
              <X />
            </IconButton>
          </div>
        </div>
      )}

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto p-3">
        <div
          style={{
            height: `${virtualizer.getTotalSize()}px`,
            width: "100%",
            position: "relative",
          }}
        >
          {virtualItems.map((vItem) => {
            const row = flatRows[vItem.index];
            if (!row) return null;
            const needsDynamicMeasure =
              wrapLines ||
              row.kind === "patrol_finding_list" ||
              row.kind === "comment_list" ||
              row.kind === "comment_composer";
            return (
              <div
                key={vItem.key}
                data-index={vItem.index}
                ref={needsDynamicMeasure ? virtualizer.measureElement : undefined}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  transform: `translateY(${vItem.start}px)`,
                }}
              >
                {renderRow(row, vItem.index)}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
});
