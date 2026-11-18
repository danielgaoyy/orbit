import type { DiffFileView } from "@/lib/orbit-ipc";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

export function DiffFilesViewer({ files }: { files: DiffFileView[] }) {
  if (files.length === 0) {
    return (
      <div className="text-muted-foreground flex h-44 items-center justify-center rounded-lg border border-dashed text-xs">
        Working tree clean or no diff selected.
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {files.map((file, idx) => (
        <div
          key={`${file.path}-${idx}`}
          className="overflow-hidden rounded-xl border border-white/10 bg-[#0A0E17]"
        >
          <div className="flex items-center justify-between border-b border-white/8 bg-white/[0.03] px-3.5 py-2">
            <span className="font-mono text-xs font-medium text-zinc-200">{file.path}</span>
            <div className="flex items-center gap-1.5">
              <Badge variant="success" className="font-mono">
                +{file.additions}
              </Badge>
              <Badge variant="destructive" className="font-mono">
                -{file.deletions}
              </Badge>
            </div>
          </div>
          <div className="overflow-x-auto font-mono text-[12px] leading-5">
            {file.lines.map((line, lIdx) => {
              const rowClass =
                line.kind === "added"
                  ? "bg-emerald-500/12 text-emerald-200"
                  : line.kind === "removed"
                    ? "bg-rose-500/12 text-rose-200"
                    : line.kind === "hunk_header"
                      ? "bg-sky-500/10 text-sky-300"
                      : line.kind === "meta"
                        ? "text-zinc-500"
                        : "text-zinc-300";
              const prefix =
                line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " ";

              return (
                <div key={lIdx} className={cn("flex min-w-fit px-2", rowClass)}>
                  <span className="w-10 shrink-0 pr-2 text-right text-zinc-600 select-none">
                    {line.old_line ?? ""}
                  </span>
                  <span className="w-10 shrink-0 pr-3 text-right text-zinc-600 select-none">
                    {line.new_line ?? ""}
                  </span>
                  <span className="w-4 shrink-0 opacity-70 select-none">
                    {line.kind === "hunk_header" || line.kind === "meta" ? "" : prefix}
                  </span>
                  <span className="whitespace-pre">{line.content}</span>
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}
