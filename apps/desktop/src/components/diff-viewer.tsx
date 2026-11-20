import { memo, useMemo, useState, type ReactNode } from "react";
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
import { ScrollArea } from "@/components/ui/scroll-area";
import { adaptTokenColor } from "@/lib/code-highlight";
import type { DiffFileView, DiffLineView, HighlightedTokenView, InlineDiffSpan } from "@/lib/orbit-ipc";
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

interface SplitRow {
  kind: "header" | "pair";
  headerText?: string;
  left?: DiffLineView;
  right?: DiffLineView;
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

  // Merge syntax tokens with intra-line changed word spans
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

interface ActiveCommentTarget {
  side: "old" | "new";
  lineNumber: number | null;
  code: string;
  anchorKey: string;
}

const InlineCommentComposer = memo(function InlineCommentComposer({
  path,
  target,
  onCancel,
  onSave,
}: {
  path: string;
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
    <div className="border-border bg-elevated/95 mx-2 my-1.5 rounded-lg border p-2 font-sans shadow-xs">
      <div className="text-muted-foreground mb-1 flex items-center justify-between text-[11px]">
        <span className="truncate font-mono">
          {path}
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

const SingleFileDiffCard = memo(function SingleFileDiffCard({
  file,
  layout,
  wrapLines,
  isDark,
  collapsed: controlledCollapsed,
  viewed,
  fileComments,
  onToggleCollapse,
  onToggleViewed,
  onQuoteLine,
  onAddComment,
  onDeleteComment,
  onOpenFile,
}: {
  file: DiffFileView;
  layout: DiffLayoutMode;
  wrapLines: boolean;
  isDark: boolean;
  collapsed?: boolean;
  viewed?: boolean;
  fileComments?: readonly ReviewCommentDraft[];
  onToggleCollapse?: (path: string) => void;
  onToggleViewed?: (path: string) => void;
  onQuoteLine?: (path: string, lineNumber: number | null, code: string) => void;
  onAddComment?: (draft: Omit<ReviewCommentDraft, "id">) => void;
  onDeleteComment?: (id: string) => void;
  onOpenFile?: (path: string, focusLine?: number | null) => void;
}) {
  const [localCollapsed, setLocalCollapsed] = useState(false);
  const [activeComment, setActiveComment] = useState<ActiveCommentTarget | null>(null);
  const collapsed = controlledCollapsed ?? localCollapsed;

  const handleToggleCollapse = () => {
    if (onToggleCollapse) onToggleCollapse(file.path);
    else setLocalCollapsed((v) => !v);
  };

  const omissionNotice = useMemo(() => {
    const tail = file.lines[file.lines.length - 1];
    if (tail && tail.kind === "meta" && tail.content.startsWith("…")) {
      return tail.content;
    }
    return null;
  }, [file.lines]);

  const splitRows = useMemo(
    () => (!collapsed && layout === "split" ? buildSplitRows(file.lines) : []),
    [collapsed, file.lines, layout],
  );

  const commentsByAnchor = useMemo(() => {
    if (!fileComments || fileComments.length === 0) return null;
    const map = new Map<string, ReviewCommentDraft[]>();
    for (const c of fileComments) {
      const k = `${c.side}:${c.lineNumber ?? "na"}`;
      const list = map.get(k);
      if (list) list.push(c);
      else map.set(k, [c]);
    }
    return map;
  }, [fileComments]);

  const codeWrapClass = wrapLines ? "whitespace-pre-wrap break-all" : "whitespace-pre";

  return (
    <div
      style={{ contain: "content" }}
      className="border-border overflow-hidden rounded-lg border bg-zinc-50/80 text-zinc-800 dark:bg-[#0B0F17] dark:text-zinc-200"
    >
      <div className="border-border group/diffhdr flex w-full items-center justify-between gap-2 border-b bg-black/[0.02] px-3 py-2 transition-colors hover:bg-black/[0.045] dark:bg-white/[0.025] dark:hover:bg-white/[0.045]">
        <button
          type="button"
          onClick={handleToggleCollapse}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
        >
          {collapsed ? (
            <ChevronRight className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
          ) : (
            <ChevronDown className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
          )}
          <span
            className={cn(
              "truncate font-mono text-[12.5px] font-medium",
              viewed ? "text-muted-foreground line-through" : "text-foreground",
            )}
          >
            {file.path}
          </span>
          {fileComments && fileComments.length > 0 && (
            <span className="bg-sky-500/15 text-sky-700 dark:text-sky-300 shrink-0 rounded-full px-1.5 py-0.25 font-sans text-[10.5px] font-medium">
              {fileComments.length}
            </span>
          )}
        </button>
        <div className="tabular-nums flex shrink-0 items-center gap-2 font-mono text-[12px]">
          {file.additions > 0 && (
            <span className="text-emerald-600 dark:text-emerald-400">+{file.additions}</span>
          )}
          {file.deletions > 0 && (
            <span className="text-rose-600 dark:text-rose-400">-{file.deletions}</span>
          )}
          {onToggleViewed && (
            <button
              type="button"
              title={viewed ? "Mark as unviewed" : "Mark as viewed"}
              onClick={() => onToggleViewed(file.path)}
              className={cn(
                "inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-1.5 font-sans text-[11px] transition-colors",
                viewed
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
              onClick={() => onOpenFile(file.path)}
              className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md opacity-70 transition-[color,background-color,opacity] hover:bg-black/[0.06] hover:opacity-100 dark:hover:bg-white/[0.08]"
            >
              <FileCode2 className="h-[13.5px] w-[13.5px] stroke-[1.75]" />
            </button>
          )}
        </div>
      </div>

      {!collapsed &&
        (layout === "unified" ? (
          <ScrollArea
            orientation="horizontal"
            viewportClassName="selectable-text font-mono text-[12px] leading-[1.55]"
          >
            {file.lines.map((line, lIdx) => {
              if (line.kind === "meta") return null;

              const rowClass =
                line.kind === "added"
                  ? "bg-emerald-500/[0.11] dark:bg-emerald-500/[0.10]"
                  : line.kind === "removed"
                    ? "bg-rose-500/[0.10] dark:bg-rose-500/[0.10]"
                    : line.kind === "hunk_header"
                      ? "bg-sky-500/[0.09] text-sky-800 dark:bg-sky-500/[0.08] dark:text-sky-300/90 py-0.5 select-none"
                      : "text-zinc-700 dark:text-zinc-300/90";

              const prefix =
                line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " ";
              const side: "old" | "new" = line.kind === "removed" ? "old" : "new";
              const targetLn = side === "old" ? line.old_line : (line.new_line ?? line.old_line);
              const anchorKey = `${side}:${targetLn ?? "na"}`;
              const lineComments = commentsByAnchor?.get(anchorKey);
              const isCommenting = activeComment?.anchorKey === anchorKey && activeComment.code === line.content;

              return (
                <div key={lIdx}>
                  <div
                    className={cn(
                      "group flex items-start px-2",
                      wrapLines ? "w-full" : "min-w-fit",
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
                    {line.kind !== "hunk_header" && (onAddComment || onQuoteLine) ? (
                      <div className="mr-1 flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                        {onAddComment && (
                          <button
                            type="button"
                            title="Add review comment on this line"
                            onClick={() =>
                              setActiveComment({
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
                    <span className="w-3.5 shrink-0 opacity-60 select-none">
                      {line.kind === "hunk_header" ? "" : prefix}
                    </span>
                    <span className={cn("min-w-0 flex-1", codeWrapClass)}>
                      {line.kind === "hunk_header"
                        ? line.content
                        : renderTokensWithInlineSpans(
                            line.content,
                            line.tokens,
                            line.inline_changes,
                            line.kind,
                            isDark,
                          )}
                    </span>
                  </div>

                  {lineComments && lineComments.length > 0 && (
                    <div className="border-border/60 bg-elevated/70 mx-2 my-1 space-y-1 rounded-lg border px-2.5 py-1.5 font-sans text-[12px]">
                      {lineComments.map((c) => (
                        <div key={c.id} className="flex items-start justify-between gap-2">
                          <div className="text-foreground/90 min-w-0 flex-1 whitespace-pre-wrap break-words">
                            <span className="text-sky-600 dark:text-sky-400 mr-1.5 text-[10.5px] font-medium uppercase">
                              Draft
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
                  )}

                  {isCommenting && onAddComment && activeComment && (
                    <InlineCommentComposer
                      path={file.path}
                      target={activeComment}
                      onCancel={() => setActiveComment(null)}
                      onSave={(comment) => {
                        onAddComment({
                          path: file.path,
                          side: activeComment.side,
                          lineNumber: activeComment.lineNumber,
                          code: activeComment.code,
                          comment,
                        });
                        setActiveComment(null);
                      }}
                    />
                  )}
                </div>
              );
            })}
          </ScrollArea>
        ) : (
          <ScrollArea
            orientation="horizontal"
            viewportClassName="selectable-text font-mono text-[12px] leading-[1.55]"
          >
            {splitRows.map((row, rIdx) => {
              if (row.kind === "header") {
                return (
                  <div
                    key={rIdx}
                    className="bg-sky-500/[0.09] px-3 py-0.5 text-[11.5px] text-sky-800 select-none dark:bg-sky-500/[0.08] dark:text-sky-300/90"
                  >
                    {row.headerText}
                  </div>
                );
              }
              const left = row.left;
              const right = row.right;
              const leftAnchor = `old:${left?.old_line ?? "na"}`;
              const rightAnchor = `new:${right?.new_line ?? "na"}`;
              const rowComments = [
                ...(left ? (commentsByAnchor?.get(leftAnchor) ?? []) : []),
                ...(right ? (commentsByAnchor?.get(rightAnchor) ?? []) : []),
              ];
              const isCommentingRow =
                (left && activeComment?.anchorKey === leftAnchor && activeComment.code === left.content) ||
                (right && activeComment?.anchorKey === rightAnchor && activeComment.code === right.content);

              return (
                <div key={rIdx}>
                  <div
                    className={cn(
                      "divide-border grid grid-cols-2 divide-x",
                      wrapLines ? "w-full" : "min-w-[680px]",
                    )}
                  >
                    {/* Old side */}
                    <div
                      className={cn(
                        "group flex items-start px-1.5",
                        wrapLines ? "min-w-0" : "overflow-x-auto",
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
                      {left && (onAddComment || onQuoteLine) ? (
                        <div className="mr-0.5 flex shrink-0 items-center opacity-0 transition-opacity group-hover:opacity-100">
                          {onAddComment && (
                            <button
                              type="button"
                              title="Comment on removed/old line"
                              onClick={() =>
                                setActiveComment({
                                  side: "old",
                                  lineNumber: left.old_line,
                                  code: left.content,
                                  anchorKey: leftAnchor,
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
                      ) : null}
                      <span className="w-3 shrink-0 opacity-60 select-none">
                        {left?.kind === "removed" ? "-" : " "}
                      </span>
                      <span className={cn("min-w-0 flex-1", codeWrapClass)}>
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
                        "group flex items-start px-1.5",
                        wrapLines ? "min-w-0" : "overflow-x-auto",
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
                      {right && (onAddComment || onQuoteLine) ? (
                        <div className="mr-0.5 flex shrink-0 items-center opacity-0 transition-opacity group-hover:opacity-100">
                          {onAddComment && (
                            <button
                              type="button"
                              title="Comment on new line"
                              onClick={() =>
                                setActiveComment({
                                  side: "new",
                                  lineNumber: right.new_line,
                                  code: right.content,
                                  anchorKey: rightAnchor,
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
                      ) : null}
                      <span className="w-3 shrink-0 opacity-60 select-none">
                        {right?.kind === "added" ? "+" : " "}
                      </span>
                      <span className={cn("min-w-0 flex-1", codeWrapClass)}>
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

                  {rowComments.length > 0 && (
                    <div className="border-border/60 bg-elevated/70 mx-2 my-1 space-y-1 rounded-lg border px-2.5 py-1.5 font-sans text-[12px]">
                      {rowComments.map((c) => (
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
                  )}

                  {isCommentingRow && onAddComment && activeComment && (
                    <InlineCommentComposer
                      path={file.path}
                      target={activeComment}
                      onCancel={() => setActiveComment(null)}
                      onSave={(comment) => {
                        onAddComment({
                          path: file.path,
                          side: activeComment.side,
                          lineNumber: activeComment.lineNumber,
                          code: activeComment.code,
                          comment,
                        });
                        setActiveComment(null);
                      }}
                    />
                  )}
                </div>
              );
            })}
          </ScrollArea>
        ))}

      {!collapsed && omissionNotice && (
        <div className="border-border text-muted-foreground border-t px-3 py-1.5 text-center font-mono text-[11.5px] select-none">
          {omissionNotice}
        </div>
      )}
    </div>
  );
});

export const DiffFilesViewer = memo(function DiffFilesViewer({
  files,
  layout = "unified",
  wrapLines = false,
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

  const commentsByFile = useMemo(() => {
    if (!comments || comments.length === 0) return null;
    const map = new Map<string, ReviewCommentDraft[]>();
    for (const c of comments) {
      const list = map.get(c.path);
      if (list) list.push(c);
      else map.set(c.path, [c]);
    }
    return map;
  }, [comments]);

  if (files.length === 0) {
    return (
      <div className="text-muted-foreground flex h-36 items-center justify-center rounded-xl border border-dashed text-[13px]">
        {emptyMessage}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {files.map((file) => (
        <SingleFileDiffCard
          key={file.path}
          file={file}
          layout={layout}
          wrapLines={wrapLines}
          isDark={isDark}
          collapsed={collapsedPaths?.has(file.path)}
          viewed={viewedPaths?.has(file.path)}
          fileComments={commentsByFile?.get(file.path)}
          onToggleCollapse={onToggleCollapse}
          onToggleViewed={onToggleViewed}
          onQuoteLine={onQuoteLine}
          onAddComment={onAddComment}
          onDeleteComment={onDeleteComment}
          onOpenFile={onOpenFile}
        />
      ))}
    </div>
  );
});
