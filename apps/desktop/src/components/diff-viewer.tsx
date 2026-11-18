import { memo, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, MessageSquarePlus } from "lucide-react";
import type { DiffFileView, DiffLineView } from "@/lib/orbit-ipc";
import type { DiffLayoutMode } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

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

function SingleFileDiffCard({
  file,
  layout,
  onQuoteLine,
}: {
  file: DiffFileView;
  layout: DiffLayoutMode;
  onQuoteLine?: (path: string, lineNumber: number | null, code: string) => void;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const splitRows = useMemo(
    () => (layout === "split" ? buildSplitRows(file.lines) : []),
    [file.lines, layout],
  );

  return (
    <div className="border-border overflow-hidden rounded-lg border bg-zinc-50/80 text-zinc-800 dark:bg-[#0B0F17] dark:text-zinc-200">
      <button
        type="button"
        onClick={() => setCollapsed((v) => !v)}
        className="border-border flex w-full cursor-pointer items-center justify-between gap-2 border-b bg-black/[0.02] px-3 py-1.5 text-left transition-colors hover:bg-black/[0.045] dark:bg-white/[0.025] dark:hover:bg-white/[0.045]"
      >
        <div className="flex min-w-0 items-center gap-1.5">
          {collapsed ? (
            <ChevronRight className="text-muted-foreground size-3.5 shrink-0" />
          ) : (
            <ChevronDown className="text-muted-foreground size-3.5 shrink-0" />
          )}
          <span className="text-foreground truncate font-mono text-[11.5px] font-medium">
            {file.path}
          </span>
        </div>
        <div className="tabular-nums flex shrink-0 items-center gap-2 font-mono text-[11px]">
          {file.additions > 0 && (
            <span className="text-emerald-600 dark:text-emerald-400">+{file.additions}</span>
          )}
          {file.deletions > 0 && (
            <span className="text-rose-600 dark:text-rose-400">-{file.deletions}</span>
          )}
        </div>
      </button>

      {!collapsed &&
        (layout === "unified" ? (
          <div className="selectable-text overflow-x-auto font-mono text-[11.5px] leading-[1.5]">
            {file.lines.map((line, lIdx) => {
              if (line.kind === "meta") return null;

              const rowClass =
                line.kind === "added"
                  ? "bg-emerald-500/[0.13] text-emerald-950 dark:bg-emerald-500/[0.11] dark:text-emerald-100"
                  : line.kind === "removed"
                    ? "bg-rose-500/[0.12] text-rose-950 dark:bg-rose-500/[0.11] dark:text-rose-100"
                    : line.kind === "hunk_header"
                      ? "bg-sky-500/[0.09] text-sky-800 dark:bg-sky-500/[0.08] dark:text-sky-300/90 py-0.5"
                      : "text-zinc-700 dark:text-zinc-300/90";

              const prefix =
                line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " ";
              const targetLn = line.new_line ?? line.old_line;

              return (
                <div key={lIdx} className={cn("group flex min-w-fit items-center px-2", rowClass)}>
                  <span className="tabular-nums w-8 shrink-0 pr-1.5 text-right text-[10.5px] text-zinc-400 select-none dark:text-zinc-600">
                    {line.old_line ?? ""}
                  </span>
                  <span className="tabular-nums w-8 shrink-0 pr-2 text-right text-[10.5px] text-zinc-400 select-none dark:text-zinc-600">
                    {line.new_line ?? ""}
                  </span>
                  {onQuoteLine && line.kind !== "hunk_header" ? (
                    <button
                      type="button"
                      title="Quote line into Composer"
                      onClick={() => onQuoteLine(file.path, targetLn, line.content)}
                      className="mr-1 inline-flex size-4 shrink-0 cursor-pointer items-center justify-center rounded opacity-0 transition-opacity group-hover:opacity-100 hover:bg-black/10 dark:hover:bg-white/15"
                    >
                      <MessageSquarePlus className="text-primary size-2.5" />
                    </button>
                  ) : (
                    <span className="w-3 shrink-0" />
                  )}
                  <span className="w-3.5 shrink-0 opacity-60 select-none">
                    {line.kind === "hunk_header" ? "" : prefix}
                  </span>
                  <span className="whitespace-pre">{line.content}</span>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="selectable-text overflow-x-auto font-mono text-[11px] leading-[1.5]">
            {splitRows.map((row, rIdx) => {
              if (row.kind === "header") {
                return (
                  <div
                    key={rIdx}
                    className="bg-sky-500/[0.09] px-3 py-0.5 text-[10.5px] text-sky-800 dark:bg-sky-500/[0.08] dark:text-sky-300/90"
                  >
                    {row.headerText}
                  </div>
                );
              }
              const left = row.left;
              const right = row.right;
              return (
                <div
                  key={rIdx}
                  className="divide-border grid min-w-[620px] grid-cols-2 divide-x"
                >
                  {/* Old side */}
                  <div
                    className={cn(
                      "group flex items-center overflow-hidden px-1.5",
                      left?.kind === "removed"
                        ? "bg-rose-500/[0.12] text-rose-950 dark:bg-rose-500/[0.11] dark:text-rose-100"
                        : left
                          ? "text-zinc-600 dark:text-zinc-400"
                          : "bg-black/[0.015] dark:bg-white/[0.01]",
                    )}
                  >
                    <span className="tabular-nums w-7 shrink-0 pr-1.5 text-right text-[10px] text-zinc-400 select-none dark:text-zinc-600">
                      {left?.old_line ?? ""}
                    </span>
                    <span className="w-3 shrink-0 opacity-60 select-none">
                      {left?.kind === "removed" ? "-" : " "}
                    </span>
                    <span className="truncate whitespace-pre">{left?.content ?? ""}</span>
                  </div>

                  {/* New side */}
                  <div
                    className={cn(
                      "group flex items-center overflow-hidden px-1.5",
                      right?.kind === "added"
                        ? "bg-emerald-500/[0.13] text-emerald-950 dark:bg-emerald-500/[0.11] dark:text-emerald-100"
                        : right
                          ? "text-zinc-700 dark:text-zinc-300"
                          : "bg-black/[0.015] dark:bg-white/[0.01]",
                    )}
                  >
                    <span className="tabular-nums w-7 shrink-0 pr-1.5 text-right text-[10px] text-zinc-400 select-none dark:text-zinc-600">
                      {right?.new_line ?? ""}
                    </span>
                    {onQuoteLine && right ? (
                      <button
                        type="button"
                        title="Quote line into Composer"
                        onClick={() => onQuoteLine(file.path, right.new_line, right.content)}
                        className="mr-0.5 inline-flex size-3.5 shrink-0 cursor-pointer items-center justify-center rounded opacity-0 transition-opacity group-hover:opacity-100 hover:bg-black/10 dark:hover:bg-white/15"
                      >
                        <MessageSquarePlus className="text-primary size-2.5" />
                      </button>
                    ) : (
                      <span className="w-3 shrink-0 opacity-60 select-none">
                        {right?.kind === "added" ? "+" : " "}
                      </span>
                    )}
                    <span className="truncate whitespace-pre">{right?.content ?? ""}</span>
                  </div>
                </div>
              );
            })}
          </div>
        ))}
    </div>
  );
}

export const DiffFilesViewer = memo(function DiffFilesViewer({
  files,
  layout = "unified",
  onQuoteLine,
}: {
  files: DiffFileView[];
  layout?: DiffLayoutMode;
  onQuoteLine?: (path: string, lineNumber: number | null, code: string) => void;
}) {
  if (files.length === 0) {
    return (
      <div className="text-muted-foreground flex h-36 items-center justify-center rounded-lg border border-dashed text-xs">
        Working tree clean — no changes to review.
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {files.map((file, idx) => (
        <SingleFileDiffCard
          key={`${file.path}-${idx}`}
          file={file}
          layout={layout}
          onQuoteLine={onQuoteLine}
        />
      ))}
    </div>
  );
});
