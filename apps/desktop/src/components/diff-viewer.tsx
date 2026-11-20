import { memo, useCallback, useMemo, useRef, useState, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Check,
  ChevronDown,
  ChevronRight,
  FileCode2,
  MessageSquarePlus,
  MessageSquareText,
  Trash2,
  X,
} from "lucide-react";
import { adaptTokenColor } from "@/lib/code-highlight";
import type {
  DiffFileView,
  DiffLineView,
  HighlightedTokenView,
  InlineDiffSpan,
} from "@/lib/orbit-ipc";
import { type DiffLayoutMode, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export interface ReviewCommentDraft {
  id: string;
  path: string;
  side: "old" | "new";
  lineNumber: number | null;
  code: string;
  comment: string;
}

interface ActiveCommentTarget {
  fileKey: string;
  path: string;
  side: "old" | "new";
  lineNumber: number | null;
  code: string;
  anchorKey: string;
}

interface SplitRow {
  kind: "header" | "pair";
  headerText?: string;
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
    }
  | {
      kind: "hunk_header";
      key: string;
      content: string;
    }
  | {
      kind: "unified_line";
      key: string;
      file: DiffFileView;
      fileKey: string;
      line: DiffLineView;
      lineIdx: number;
      isLastInFile: boolean;
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
      kind: "file_spacer";
      key: string;
    };

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
      rows.push({ kind: "header", headerText: cur.content });
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
          color: adaptTokenColor(tok.fg_hex, isDark),
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
      color: adaptTokenColor(tok.fg_hex, isDark),
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
  const [text, setText] = useState("");
  const submit = () => {
    const trimmed = text.trim();
    if (!trimmed) return;
    onSave(trimmed);
    setText("");
  };

  return (
    <div className="border-border bg-elevated/95 border-x border-b px-3 py-2 font-sans">
      <div className="text-muted-foreground mb-1 flex items-center justify-between text-[11px]">
        <span className="truncate font-mono">
          {target.path}
          {target.lineNumber != null ? `:${target.lineNumber}` : ""} ({target.side})
        </span>
        <button
          type="button"
          onClick={onCancel}
          className="hover:text-foreground inline-flex h-4 w-4 cursor-pointer items-center justify-center rounded"
        >
          <X className="h-3 w-3" />
        </button>
      </div>
      <textarea
        autoFocus
        rows={2}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.nativeEvent.isComposing) return;
          if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey || !e.shiftKey)) {
            e.preventDefault();
            submit();
          }
        }}
        placeholder="Add a review comment for the agent… (Enter to save draft)"
        className="border-border bg-background text-foreground placeholder:text-muted-foreground w-full resize-none rounded-md border px-2 py-1.5 text-[12px] leading-relaxed outline-none focus:border-foreground/30"
      />
      <div className="mt-1.5 flex items-center justify-end gap-1.5">
        <button
          type="button"
          onClick={onCancel}
          className="text-muted-foreground hover:text-foreground cursor-pointer rounded-md px-2 py-0.5 text-[11.5px]"
        >
          Cancel
        </button>
        <button
          type="button"
          disabled={!text.trim()}
          onClick={submit}
          className="bg-foreground text-background cursor-pointer rounded-md px-2.5 py-0.5 text-[11.5px] font-medium transition-opacity hover:opacity-90 disabled:cursor-default disabled:opacity-40"
        >
          Add comment
        </button>
      </div>
    </div>
  );
});

function estimateRowHeight(row: DiffVirtualRow): number {
  switch (row.kind) {
    case "file_header":
      return 38;
    case "hunk_header":
      return 24;
    case "unified_line":
    case "split_pair":
      return 20;
    case "comment_list":
      return 42 + Math.max(0, row.comments.length - 1) * 28;
    case "comment_composer":
      return 124;
    case "omission":
      return 30;
    case "file_spacer":
      return 12;
  }
}

export const DiffFilesViewer = memo(function DiffFilesViewer({
  files,
  layout = "unified",
  wrapLines = false,
  virtualized = false,
  emptyMessage = "Working tree clean — no changes to review.",
  collapsedPaths,
  viewedPaths,
  comments,
  onToggleCollapse,
  onToggleViewed,
  onQuoteLine,
  onAddComment,
  onDeleteComment,
  onOpenFile,
}: {
  files: DiffFileView[];
  layout?: DiffLayoutMode;
  wrapLines?: boolean;
  virtualized?: boolean;
  emptyMessage?: string;
  collapsedPaths?: ReadonlySet<string>;
  viewedPaths?: ReadonlySet<string>;
  comments?: readonly ReviewCommentDraft[];
  onToggleCollapse?: (path: string) => void;
  onToggleViewed?: (path: string) => void;
  onQuoteLine?: (path: string, lineNumber: number | null, code: string) => void;
  onAddComment?: (draft: Omit<ReviewCommentDraft, "id">) => void;
  onDeleteComment?: (id: string) => void;
  onOpenFile?: (path: string, focusLine?: number | null) => void;
}) {
  const isDark = useOrbitSelector((s) => s.resolvedTheme === "dark");
  const [localCollapsed, setLocalCollapsed] = useState<Set<string>>(() => new Set());
  const [activeComment, setActiveComment] = useState<ActiveCommentTarget | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const effectiveCollapsed = collapsedPaths ?? localCollapsed;

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

  const commentsByFileAndAnchor = useMemo(() => {
    const byFile = new Map<string, Map<string, ReviewCommentDraft[]>>();
    const countByFile = new Map<string, number>();
    for (const c of comments ?? []) {
      countByFile.set(c.path, (countByFile.get(c.path) ?? 0) + 1);
      let anchorMap = byFile.get(c.path);
      if (!anchorMap) {
        anchorMap = new Map();
        byFile.set(c.path, anchorMap);
      }
      const ak = `${c.side}:${c.lineNumber ?? "na"}`;
      const list = anchorMap.get(ak);
      if (list) list.push(c);
      else anchorMap.set(ak, [c]);
    }
    return { byFile, countByFile };
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

      out.push({
        kind: "file_header",
        key: `fh:${fileKey}`,
        file,
        fileKey,
        collapsed,
        viewed,
        commentCount,
      });

      if (!collapsed) {
        const tail = file.lines[file.lines.length - 1];
        const omissionNotice =
          tail && tail.kind === "meta" && tail.content.startsWith("…") ? tail.content : null;

        if (layout === "unified") {
          const visibleLines: { line: DiffLineView; idx: number }[] = [];
          for (let lIdx = 0; lIdx < file.lines.length; lIdx += 1) {
            if (file.lines[lIdx].kind !== "meta") {
              visibleLines.push({ line: file.lines[lIdx], idx: lIdx });
            }
          }
          for (let vIdx = 0; vIdx < visibleLines.length; vIdx += 1) {
            const { line, idx } = visibleLines[vIdx];
            const isLast = vIdx === visibleLines.length - 1 && !omissionNotice;
            if (line.kind === "hunk_header") {
              out.push({ kind: "hunk_header", key: `uh:${fileKey}:${idx}`, content: line.content });
            } else {
              out.push({
                kind: "unified_line",
                key: `ul:${fileKey}:${idx}`,
                file,
                fileKey,
                line,
                lineIdx: idx,
                isLastInFile: isLast,
              });
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
          for (let rIdx = 0; rIdx < splitRows.length; rIdx += 1) {
            const sr = splitRows[rIdx];
            const isLast = rIdx === splitRows.length - 1 && !omissionNotice;
            if (sr.kind === "header") {
              out.push({
                kind: "hunk_header",
                key: `sh:${fileKey}:${rIdx}`,
                content: sr.headerText ?? "",
              });
            } else {
              out.push({
                kind: "split_pair",
                key: `sp:${fileKey}:${rIdx}`,
                file,
                fileKey,
                left: sr.left,
                right: sr.right,
                rowIdx: rIdx,
                isLastInFile: isLast,
              });
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
  }, [files, layout, effectiveCollapsed, viewedPaths, commentsByFileAndAnchor, activeComment]);

  const virtualizer = useVirtualizer({
    count: virtualized ? flatRows.length : 0,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) => estimateRowHeight(flatRows[index]),
    overscan: 16,
    getItemKey: (index) => flatRows[index]?.key ?? index,
  });

  const codeWrapClass = wrapLines ? "whitespace-pre-wrap break-all" : "whitespace-pre";

  const renderRow = (row: DiffVirtualRow): ReactNode => {
    switch (row.kind) {
      case "file_spacer":
        return <div className="h-3" />;

      case "file_header":
        return (
          <div
            className={cn(
              "border-border group/diffhdr flex h-[38px] w-full items-center justify-between gap-2 border bg-zinc-100/90 px-3 transition-colors hover:bg-zinc-200/60 dark:bg-[#101622] dark:hover:bg-[#141c2b]",
              row.collapsed ? "rounded-lg" : "rounded-t-lg",
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
            </button>
            <div className="tabular-nums flex shrink-0 items-center gap-2 font-mono text-[12px]">
              {row.file.additions > 0 && (
                <span className="text-emerald-600 dark:text-emerald-400">
                  +{row.file.additions}
                </span>
              )}
              {row.file.deletions > 0 && (
                <span className="text-rose-600 dark:text-rose-400">-{row.file.deletions}</span>
              )}
              {onToggleViewed && (
                <button
                  type="button"
                  title={row.viewed ? "Mark as unviewed" : "Mark as viewed"}
                  onClick={() => onToggleViewed(row.file.path)}
                  className={cn(
                    "inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-1.5 font-sans text-[11px] transition-colors",
                    row.viewed
                      ? "border-emerald-500/35 bg-emerald-500/12 text-emerald-700 dark:text-emerald-400"
                      : "border-border text-muted-foreground hover:text-foreground hover:bg-black/[0.05] dark:hover:bg-white/[0.06]",
                  )}
                >
                  <Check className="h-[11px] w-[11px]" />
                  <span>Viewed</span>
                </button>
              )}
              {onOpenFile && (
                <button
                  type="button"
                  title="Open full file"
                  onClick={() => onOpenFile(row.file.path)}
                  className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md opacity-70 transition-[color,background-color,opacity] hover:bg-black/[0.06] hover:opacity-100 dark:hover:bg-white/[0.08]"
                >
                  <FileCode2 className="h-[13.5px] w-[13.5px] stroke-[1.75]" />
                </button>
              )}
            </div>
          </div>
        );

      case "hunk_header":
        return (
          <div className="border-border border-x bg-sky-500/[0.09] px-3 py-0.5 font-mono text-[11.5px] leading-[1.6] text-sky-800 select-none dark:bg-sky-500/[0.08] dark:text-sky-300/90">
            {row.content}
          </div>
        );

      case "unified_line": {
        const { line, file, fileKey, lineIdx, isLastInFile } = row;
        const rowClass =
          line.kind === "added"
            ? "bg-emerald-500/[0.11] dark:bg-emerald-500/[0.10]"
            : line.kind === "removed"
              ? "bg-rose-500/[0.10] dark:bg-rose-500/[0.10]"
              : "bg-zinc-50/80 text-zinc-700 dark:bg-[#0B0F17] dark:text-zinc-300/90";
        const prefix = line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " ";
        const side: "old" | "new" = line.kind === "removed" ? "old" : "new";
        const targetLn = side === "old" ? line.old_line : (line.new_line ?? line.old_line);
        const anchorKey = `${side}:${targetLn ?? `idx_${lineIdx}`}`;

        return (
          <div
            className={cn(
              "border-border group selectable-text flex w-full items-start border-x px-2 font-mono text-[12px] leading-[20px]",
              isLastInFile && "rounded-b-lg border-b",
              rowClass,
            )}
          >
            {onOpenFile && line.old_line != null ? (
              <button
                type="button"
                title={`Open ${file.path} around line ${line.old_line}`}
                onClick={() => onOpenFile(file.path, line.old_line)}
                className="tabular-nums hover:text-foreground w-8 shrink-0 cursor-pointer pr-1.5 text-right text-[11.5px] text-zinc-400 select-none hover:underline dark:text-zinc-600"
              >
                {line.old_line}
              </button>
            ) : (
              <span className="tabular-nums w-8 shrink-0 pr-1.5 text-right text-[11.5px] text-zinc-400 select-none dark:text-zinc-600">
                {line.old_line ?? ""}
              </span>
            )}
            {onOpenFile && line.new_line != null ? (
              <button
                type="button"
                title={`Open ${file.path} at line ${line.new_line}`}
                onClick={() => onOpenFile(file.path, line.new_line)}
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
                    title="Add review comment on this line"
                    onClick={() =>
                      setActiveComment({
                        fileKey,
                        path: file.path,
                        side,
                        lineNumber: targetLn,
                        code: line.content,
                        anchorKey,
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
          </div>
        );
      }

      case "split_pair": {
        const { left, right, file, fileKey, rowIdx, isLastInFile } = row;
        const leftAk = `old:${left?.old_line ?? `r_${rowIdx}`}`;
        const rightAk = `new:${right?.new_line ?? `r_${rowIdx}`}`;

        return (
          <div
            className={cn(
              "border-border divide-border selectable-text grid w-full grid-cols-2 divide-x border-x bg-zinc-50/80 font-mono text-[12px] leading-[20px] dark:bg-[#0B0F17]",
              isLastInFile && "overflow-hidden rounded-b-lg border-b",
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
              )}
            >
              {onOpenFile && left?.old_line != null ? (
                <button
                  type="button"
                  title={`Open ${file.path} around line ${left.old_line}`}
                  onClick={() => onOpenFile(file.path, left.old_line)}
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
                      title="Comment on old line"
                      onClick={() =>
                        setActiveComment({
                          fileKey,
                          path: file.path,
                          side: "old",
                          lineNumber: left.old_line,
                          code: left.content,
                          anchorKey: leftAk,
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
                "group flex min-w-0 items-start px-1.5",
                right?.kind === "added"
                  ? "bg-emerald-500/[0.11] dark:bg-emerald-500/[0.10]"
                  : right
                    ? "text-zinc-700 dark:text-zinc-300"
                    : "bg-black/[0.015] dark:bg-white/[0.01]",
              )}
            >
              {onOpenFile && right?.new_line != null ? (
                <button
                  type="button"
                  title={`Open ${file.path} at line ${right.new_line}`}
                  onClick={() => onOpenFile(file.path, right.new_line)}
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
                      title="Comment on new line"
                      onClick={() =>
                        setActiveComment({
                          fileKey,
                          path: file.path,
                          side: "new",
                          lineNumber: right.new_line,
                          code: right.content,
                          anchorKey: rightAk,
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
            </div>
          </div>
        );
      }

      case "comment_list":
        return (
          <div className="border-border bg-elevated/80 space-y-1 border-x border-b px-3 py-1.5 font-sans text-[12px]">
            {row.comments.map((c) => (
              <div key={c.id} className="flex items-start justify-between gap-2">
                <div className="text-foreground/90 min-w-0 flex-1 whitespace-pre-wrap break-words">
                  <span className="text-sky-600 dark:text-sky-400 mr-1.5 text-[10.5px] font-medium uppercase">
                    Draft ({c.side})
                  </span>
                  {c.comment}
                </div>
                {onDeleteComment && (
                  <button
                    type="button"
                    title="Remove comment"
                    onClick={() => onDeleteComment(c.id)}
                    className="text-muted-foreground hover:text-rose-500 inline-flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded"
                  >
                    <Trash2 className="h-3 w-3" />
                  </button>
                )}
              </div>
            ))}
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
                lineNumber: row.target.lineNumber,
                code: row.target.code,
                comment,
              });
              setActiveComment(null);
            }}
          />
        );

      case "omission":
        return (
          <div className="border-border text-muted-foreground rounded-b-lg border bg-zinc-50/80 px-3 py-1.5 text-center font-mono text-[11.5px] select-none dark:bg-[#0B0F17]">
            {row.notice}
          </div>
        );
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
      <div className="min-w-0">
        {flatRows.map((row) => (
          <div key={row.key}>{renderRow(row)}</div>
        ))}
      </div>
    );
  }

  const virtualItems = virtualizer.getVirtualItems();

  return (
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
            wrapLines || row.kind === "comment_list" || row.kind === "comment_composer";
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
              {renderRow(row)}
            </div>
          );
        })}
      </div>
    </div>
  );
});
