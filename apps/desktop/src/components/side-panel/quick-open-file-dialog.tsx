import { useEffect, useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog-tabs";
import { Kbd } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_FILES: readonly string[] = [];

function parseQueryAndLine(raw: string): { pathQuery: string; line: number | null } {
  const trimmed = raw.trim();
  const match = trimmed.match(/^(.*?):(\d+)(?::\d+)?$/);
  if (match && match[1]) {
    const line = Number.parseInt(match[2], 10);
    return { pathQuery: match[1].trim(), line: Number.isNaN(line) ? null : line };
  }
  return { pathQuery: trimmed, line: null };
}

function scorePathMatch(path: string, query: string): number {
  const lower = path.toLowerCase();
  const base = lower.split("/").pop() ?? lower;
  if (base === query) return 1000;
  if (base.startsWith(query)) return 800 - base.length;
  if (base.includes(query)) return 600 - base.length;
  if (lower.includes(query)) return 400 - lower.length;

  // Subsequence fuzzy match
  let qi = 0;
  for (let i = 0; i < lower.length && qi < query.length; i += 1) {
    if (lower[i] === query[qi]) qi += 1;
  }
  return qi === query.length ? 200 - lower.length : -1;
}

export function QuickOpenFileDialog() {
  const open = useOrbitSelector((s) => s.quickOpenFilesOpen);
  return (
    <Dialog open={open} onOpenChange={(v) => orbitActions.setQuickOpenFilesOpen(v)}>
      <DialogContent className="max-w-lg gap-0 overflow-hidden p-0 [&>button]:hidden">
        <QuickOpenFileInner />
      </DialogContent>
    </Dialog>
  );
}

function QuickOpenFileInner() {
  const hostId = useOrbitSelector((s) => s.selectedHostId);
  const cwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const cacheKey = hostId && cwd ? `${hostId}::${cwd}` : "";
  const files = useOrbitSelector((s) =>
    cacheKey ? (s.workspaceFilesCache[cacheKey] ?? EMPTY_FILES) : EMPTY_FILES,
  );
  const gitStatusFiles = useOrbitSelector((s) =>
    cacheKey ? (s.gitReviews[cacheKey]?.status?.files ?? null) : null,
  );

  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const itemRefs = useRef<Map<number, HTMLButtonElement>>(new Map());

  const gitStatusMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const f of gitStatusFiles ?? []) {
      map.set(f.path, f.status);
    }
    return map;
  }, [gitStatusFiles]);

  useEffect(() => {
    const id = setTimeout(() => inputRef.current?.focus(), 10);
    return () => clearTimeout(id);
  }, []);

  const parsed = useMemo(() => parseQueryAndLine(query), [query]);

  const matches = useMemo(() => {
    const q = parsed.pathQuery.toLowerCase();
    if (!q) {
      // Prioritize Git-modified files first when query is empty
      const modified = files.filter((p) => gitStatusMap.has(p));
      const rest = files.filter((p) => !gitStatusMap.has(p));
      return [...modified, ...rest].slice(0, 60);
    }
    const scored: { path: string; score: number }[] = [];
    for (const p of files) {
      const sc = scorePathMatch(p, q);
      if (sc > -1) {
        const gitBoost = gitStatusMap.has(p) ? 25 : 0;
        scored.push({ path: p, score: sc + gitBoost });
      }
    }
    scored.sort((a, b) => b.score - a.score);
    return scored.slice(0, 60).map((s) => s.path);
  }, [files, parsed.pathQuery, gitStatusMap]);

  const safeIdx = matches.length === 0 ? 0 : Math.min(activeIndex, matches.length - 1);

  useEffect(() => {
    itemRefs.current.get(safeIdx)?.scrollIntoView({ block: "nearest" });
  }, [safeIdx]);

  const openSelected = (filePath: string) => {
    if (!hostId) return;
    orbitActions.openFileInSidePanel(filePath, {
      hostId,
      cwd,
      focusLine: parsed.line,
    });
  };

  return (
    <div className="flex flex-col">
      <DialogTitle className="sr-only">Quick Open File</DialogTitle>
      <div className="border-border flex items-center gap-2.5 border-b px-3.5 py-2.5">
        <Search className="text-muted-foreground h-4 w-4 shrink-0" />
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActiveIndex(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setActiveIndex((prev) => (matches.length ? (prev + 1) % matches.length : 0));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActiveIndex((prev) =>
                matches.length ? (prev - 1 + matches.length) % matches.length : 0,
              );
            } else if (e.key === "Enter" && matches[safeIdx]) {
              e.preventDefault();
              openSelected(matches[safeIdx]);
            }
          }}
          placeholder="Open file in Side Panel… (supports path:line)"
          className="text-foreground placeholder:text-muted-foreground min-w-0 flex-1 bg-transparent text-[13px] outline-none"
        />
        {parsed.line != null && (
          <span className="bg-primary/12 text-primary rounded px-1.5 py-0.5 font-mono text-[11px]">
            L{parsed.line}
          </span>
        )}
        <Kbd>Esc</Kbd>
      </div>

      <ScrollArea viewportClassName="max-h-80 p-1.5">
        {matches.length === 0 ? (
          <div className="text-muted-foreground py-8 text-center text-[12.5px]">
            {files.length === 0
              ? "Indexing workspace files…"
              : `No files matching “${query}”`}
          </div>
        ) : (
          matches.map((filePath, idx) => {
            const Icon = resolveSemanticFileIcon(filePath);
            const parts = filePath.split("/");
            const name = parts.pop() || filePath;
            const dir = parts.join("/");
            const active = idx === safeIdx;
            const gitStatus = gitStatusMap.get(filePath);
            return (
              <button
                key={filePath}
                ref={(el) => {
                  if (el) itemRefs.current.set(idx, el);
                  else itemRefs.current.delete(idx);
                }}
                type="button"
                onMouseEnter={() => setActiveIndex(idx)}
                onClick={() => openSelected(filePath)}
                className={cn(
                  "flex w-full cursor-pointer items-center justify-between gap-3 rounded-lg px-2.5 py-1.5 text-left transition-colors",
                  active ? "bg-accent text-foreground" : "text-foreground/85 hover:bg-accent/60",
                )}
              >
                <div className="flex min-w-0 items-center gap-2">
                  <Icon className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                  <span className="shrink-0 text-[12.5px] font-medium">{name}</span>
                  {dir && (
                    <span className="text-muted-foreground truncate font-mono text-[11.5px]">
                      {dir}
                    </span>
                  )}
                </div>
                {gitStatus && (
                  <span
                    className={cn(
                      "shrink-0 font-mono text-[10.5px] font-medium",
                      gitStatus === "A" || gitStatus === "??"
                        ? "text-emerald-600 dark:text-emerald-400"
                        : gitStatus === "D"
                          ? "text-rose-600 dark:text-rose-400"
                          : "text-amber-600 dark:text-amber-400",
                    )}
                  >
                    {gitStatus}
                  </span>
                )}
              </button>
            );
          })
        )}
      </ScrollArea>
    </div>
  );
}
