import { memo, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, FileCode2, MessageSquarePlus } from "lucide-react";
import { ScrollArea } from "@/components/ui/scroll-area";
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

const SingleFileDiffCard = memo(function SingleFileDiffCard({
  file,
  layout,
  onQuoteLine,
  onOpenFile,
}: {
  file: DiffFileView;
  layout: DiffLayoutMode;
  onQuoteLine?: (path: string, lineNumber: number | null, code: string) => void;
  onOpenFile?: (path: string) => void;
}) {
  const [collapsed, setCollapsed] = useState(false);

  const omissionNotice = useMemo(
    () => file.lines.find((l) => l.kind === "meta" && l.content.startsWith("…"))?.content ?? null,
    [file.lines],
  );

  const splitRows = useMemo(
    () => (layout === "split" ? buildSplitRows(file.lines) : []),
    [file.lines, layout],
  );

  return (
    <div
      style={{ contain: "content" }}
      className="border-border overflow-hidden rounded-lg border bg-zinc-50/80 text-zinc-800 dark:bg-[#0B0F17] dark:text-zinc-200"
    >
      <div className="border-border group/diffhdr flex w-full items-center justify-between gap-2 border-b bg-black/[0.02] px-3 py-2 transition-colors hover:bg-black/[0.045] dark:bg-white/[0.025] dark:hover:bg-white/[0.045]">
        <button
          type="button"
          onClick={() => setCollapsed((v) => !v)}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
        >
          {collapsed ? (
            <ChevronRight className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
          ) : (
            <ChevronDown className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
          )}
          <span className="text-foreground truncate font-mono text-[12.5px] font-medium">
            {file.path}
          </span>
        </button>
        <div className="tabular-nums flex shrink-0 items-center gap-2 font-mono text-[12px]">
          {file.additions > 0 && (
            <span className="text-emerald-600 dark:text-emerald-400">+{file.additions}</span>
          )}
          {file.deletions > 0 && (
            <span className="text-rose-600 dark:text-rose-400">-{file.deletions}</span>
          )}
          {onOpenFile && (
            <button
              type="button"
              title="Open full file"
              onClick={() => onOpenFile(file.path)}
              className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md opacity-70 transition-all hover:bg-black/[0.06] hover:opacity-100 dark:hover:bg-white/[0.08]"
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
                  ? "bg-emerald-500/[0.13] text-emerald-950 dark:bg-emerald-500/[0.11] dark:text-emerald-100"
                  : line.kind === "removed"
                    ? "bg-rose-500/[0.12] text-rose-950 dark:bg-rose-500/[0.11] dark:text-rose-100"
                    : line.kind === "hunk_header"
                      ? "bg-sky-500/[0.09] text-sky-800 dark:bg-sky-500/[0.08] dark:text-sky-300/90 py-0.5 select-none"
                      : "text-zinc-700 dark:text-zinc-300/90";

              const prefix =
                line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " ";
              const targetLn = line.new_line ?? line.old_line;

              return (
                <div key={lIdx} className={cn("group flex min-w-fit items-center px-2", rowClass)}>
                  <span className="tabular-nums w-8 shrink-0 pr-1.5 text-right text-[11.5px] text-zinc-400 select-none dark:text-zinc-600">
                    {line.old_line ?? ""}
                  </span>
                  <span className="tabular-nums w-8 shrink-0 pr-2 text-right text-[11.5px] text-zinc-400 select-none dark:text-zinc-600">
                    {line.new_line ?? ""}
                  </span>
                  {onQuoteLine && line.kind !== "hunk_header" ? (
                    <button
                      type="button"
                      title="Quote line into Composer"
                      onClick={() => onQuoteLine(file.path, targetLn, line.content)}
                      className="mr-1 inline-flex h-[18px] w-[18px] shrink-0 cursor-pointer items-center justify-center rounded opacity-0 transition-opacity group-hover:opacity-100 hover:bg-black/10 dark:hover:bg-white/15"
                    >
                      <MessageSquarePlus className="text-foreground h-[12px] w-[12px]" />
                    </button>
                  ) : (
                    <span className="w-3 shrink-0 select-none" />
                  )}
                  <span className="w-3.5 shrink-0 opacity-60 select-none">
                    {line.kind === "hunk_header" ? "" : prefix}
                  </span>
                  <span className="whitespace-pre">{line.content}</span>
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
                    <span className="tabular-nums w-8 shrink-0 pr-1.5 text-right text-[11.5px] text-zinc-400 select-none dark:text-zinc-600">
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
                    <span className="tabular-nums w-8 shrink-0 pr-1.5 text-right text-[11.5px] text-zinc-400 select-none dark:text-zinc-600">
                      {right?.new_line ?? ""}
                    </span>
                    {onQuoteLine && right ? (
                      <button
                        type="button"
                        title="Quote line into Composer"
                        onClick={() => onQuoteLine(file.path, right.new_line, right.content)}
                        className="mr-0.5 inline-flex h-[18px] w-[18px] shrink-0 cursor-pointer items-center justify-center rounded opacity-0 transition-opacity group-hover:opacity-100 hover:bg-black/10 dark:hover:bg-white/15"
                      >
                        <MessageSquarePlus className="text-foreground h-[12px] w-[12px]" />
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
  onQuoteLine,
  onOpenFile,
}: {
  files: DiffFileView[];
  layout?: DiffLayoutMode;
  onQuoteLine?: (path: string, lineNumber: number | null, code: string) => void;
  onOpenFile?: (path: string) => void;
}) {
  if (files.length === 0) {
    return (
      <div className="text-muted-foreground flex h-36 items-center justify-center rounded-xl border border-dashed text-[13px]">
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
          onOpenFile={onOpenFile}
        />
      ))}
    </div>
  );
});
