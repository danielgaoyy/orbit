import { useEffect, useRef } from "react";
import {
  Database,
  File,
  FileCode2,
  FileCog,
  FileDiff,
  FileImage,
  FileJson2,
  FileLock2,
  FileTerminal,
  FileText,
  GitCompareArrows,
  SquareTerminal,
  type LucideIcon,
} from "lucide-react";
import type { GitFileEntry } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

export type MentionCandidate =
  | {
      key: string;
      kind: "git_diff";
      staged: boolean;
      label: string;
      subtitle: string;
    }
  | {
      key: string;
      kind: "terminal_output";
      label: string;
      subtitle: string;
    }
  | {
      key: string;
      kind: "file";
      path: string;
      basename: string;
      dirPath: string;
      gitStatus: string | null;
    };

const LOCKFILE_NAMES = new Set([
  "cargo.lock",
  "package-lock.json",
  "pnpm-lock.yaml",
  "yarn.lock",
  "bun.lock",
  "bun.lockb",
  "flake.lock",
  "poetry.lock",
  "uv.lock",
]);

const CODE_EXTENSIONS = new Set([
  "rs",
  "ts",
  "tsx",
  "js",
  "jsx",
  "mjs",
  "cjs",
  "py",
  "go",
  "swift",
  "kt",
  "kts",
  "java",
  "c",
  "h",
  "cc",
  "cpp",
  "hpp",
  "cs",
  "rb",
  "php",
  "lua",
  "zig",
  "nix",
  "proto",
  "graphql",
  "gql",
  "css",
  "scss",
  "less",
  "html",
  "vue",
  "svelte",
  "astro",
]);

/** Resolves a semantic monochrome Lucide icon from a filename or extension. */
export function resolveSemanticFileIcon(basename: string): LucideIcon {
  const lower = basename.toLowerCase();

  if (LOCKFILE_NAMES.has(lower) || lower.endsWith(".lock")) {
    return FileLock2;
  }
  if (
    lower === "dockerfile" ||
    lower.startsWith("dockerfile.") ||
    lower.startsWith(".env") ||
    lower.includes(".config.")
  ) {
    return FileCog;
  }
  if (lower === "makefile" || lower === "justfile") {
    return FileTerminal;
  }
  if (lower === "license" || lower.startsWith("readme") || lower.startsWith("changelog")) {
    return FileText;
  }

  const dotIdx = lower.lastIndexOf(".");
  const ext = dotIdx >= 0 ? lower.slice(dotIdx + 1) : "";

  switch (ext) {
    case "json":
    case "jsonc":
    case "jsonl":
      return FileJson2;
    case "toml":
    case "yaml":
    case "yml":
    case "ini":
    case "conf":
    case "plist":
      return FileCog;
    case "sh":
    case "zsh":
    case "bash":
    case "fish":
    case "ps1":
      return FileTerminal;
    case "md":
    case "mdx":
    case "txt":
    case "rst":
    case "adoc":
      return FileText;
    case "png":
    case "jpg":
    case "jpeg":
    case "webp":
    case "gif":
    case "svg":
    case "ico":
    case "icns":
    case "bmp":
      return FileImage;
    case "sql":
    case "sqlite":
    case "db":
    case "prisma":
      return Database;
    default:
      return CODE_EXTENSIONS.has(ext) ? FileCode2 : File;
  }
}

function fuzzyMatchScore(target: string, basename: string, queryLower: string): number {
  if (!queryLower) return 10;
  const baseLower = basename.toLowerCase();
  const fullLower = target.toLowerCase();

  if (baseLower === queryLower) return 120;
  if (baseLower.startsWith(queryLower)) return 100;
  if (baseLower.includes(queryLower)) return 82;
  if (fullLower.includes(queryLower)) return 64;

  // Subsequence match (e.g. "omc" -> "omni-composer.tsx")
  let qi = 0;
  for (let i = 0; i < fullLower.length && qi < queryLower.length; i += 1) {
    if (fullLower[i] === queryLower[qi]) {
      qi += 1;
    }
  }
  return qi === queryLower.length ? 40 : 0;
}

export function buildMentionCandidates(
  query: string,
  workspaceFiles: readonly string[],
  gitFiles: readonly GitFileEntry[],
): MentionCandidate[] {
  const q = query.trim().toLowerCase();
  const results: MentionCandidate[] = [];

  // 1. Live Context Sources
  const specialSources: Exclude<MentionCandidate, { kind: "file" }>[] = [
    {
      key: "special:working_diff",
      kind: "git_diff",
      staged: false,
      label: "Working Tree Diff",
      subtitle: "Uncommitted changes in workspace",
    },
    {
      key: "special:staged_diff",
      kind: "git_diff",
      staged: true,
      label: "Staged Diff",
      subtitle: "Git index staged changes",
    },
    {
      key: "special:terminal",
      kind: "terminal_output",
      label: "Terminal Output",
      subtitle: "Recent lines from active PTY terminal",
    },
  ];

  for (const sp of specialSources) {
    if (
      !q ||
      sp.label.toLowerCase().includes(q) ||
      sp.subtitle.toLowerCase().includes(q) ||
      (sp.kind === "git_diff" && "diff".includes(q)) ||
      (sp.kind === "terminal_output" && "pty".includes(q))
    ) {
      results.push(sp);
    }
  }

  // 2. Workspace & Modified Files
  const gitStatusByPath = new Map<string, string>();
  for (const gf of gitFiles) {
    gitStatusByPath.set(gf.path, gf.untracked ? "U" : gf.status || "M");
  }

  const allPaths = new Set<string>();
  for (const gf of gitFiles) {
    allPaths.add(gf.path);
  }
  for (const f of workspaceFiles) {
    allPaths.add(f);
  }

  const scoredFiles: { score: number; item: Extract<MentionCandidate, { kind: "file" }> }[] = [];
  for (const path of allPaths) {
    const slashIdx = path.lastIndexOf("/");
    const basename = slashIdx >= 0 ? path.slice(slashIdx + 1) : path;
    const dirPath = slashIdx >= 0 ? path.slice(0, slashIdx) : "";
    const baseScore = fuzzyMatchScore(path, basename, q);
    if (baseScore <= 0) continue;

    const gitStatus = gitStatusByPath.get(path) ?? null;
    const finalScore = baseScore + (gitStatus ? 18 : 0) - Math.min(12, path.length * 0.08);
    scoredFiles.push({
      score: finalScore,
      item: {
        key: `file:${path}`,
        kind: "file",
        path,
        basename,
        dirPath,
        gitStatus,
      },
    });
  }

  scoredFiles.sort((a, b) => b.score - a.score);
  const maxFiles = Math.max(4, 11 - results.length);
  for (let i = 0; i < Math.min(maxFiles, scoredFiles.length); i += 1) {
    results.push(scoredFiles[i].item);
  }

  return results;
}

interface MentionMenuProps {
  items: MentionCandidate[];
  activeIndex: number;
  onHoverIndex: (index: number) => void;
  onSelect: (item: MentionCandidate) => void;
}

export function MentionMenu({
  items,
  activeIndex,
  onHoverIndex,
  onSelect,
}: MentionMenuProps) {
  const listRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const container = listRef.current;
    if (!container) return;
    const activeEl = container.querySelector<HTMLElement>(`[data-mention-idx="${activeIndex}"]`);
    activeEl?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  if (items.length === 0) return null;

  return (
    <div className="bg-popover text-popover-foreground floating-surface absolute right-0 bottom-full left-0 z-40 mb-2 overflow-hidden rounded-[14px] border border-black/[0.09] dark:border-white/[0.10]">
      <div ref={listRef} className="max-h-64 overflow-y-auto p-1.5">
        {items.map((item, idx) => {
          const active = idx === activeIndex;
          const iconClass = cn(
            "size-4 shrink-0 stroke-[1.75]",
            active ? "text-foreground" : "text-muted-foreground",
          );

          if (item.kind === "file") {
            const FileIcon = resolveSemanticFileIcon(item.basename);
            return (
              <button
                key={item.key}
                type="button"
                data-mention-idx={idx}
                onMouseEnter={() => onHoverIndex(idx)}
                onMouseDown={(e) => {
                  e.preventDefault();
                  onSelect(item);
                }}
                className={cn(
                  "flex w-full cursor-pointer items-center justify-between gap-3 rounded-[9px] px-2.5 py-1.5 text-left transition-colors",
                  active
                    ? "bg-accent text-accent-foreground"
                    : "text-foreground/90 hover:bg-accent/60",
                )}
              >
                <div className="flex min-w-0 items-center gap-2.5">
                  <FileIcon className={iconClass} />
                  <span className="shrink-0 font-mono text-[13px] font-medium">
                    {item.basename}
                  </span>
                  {item.dirPath && (
                    <span className="text-muted-foreground truncate font-mono text-[11.5px]">
                      {item.dirPath}
                    </span>
                  )}
                </div>
                {item.gitStatus && (
                  <span className="bg-muted text-muted-foreground shrink-0 rounded-[4px] px-1.5 py-0.5 font-mono text-[10.5px] font-medium dark:bg-white/[0.09]">
                    {item.gitStatus}
                  </span>
                )}
              </button>
            );
          }

          const SourceIcon =
            item.kind === "git_diff"
              ? item.staged
                ? GitCompareArrows
                : FileDiff
              : SquareTerminal;

          return (
            <button
              key={item.key}
              type="button"
              data-mention-idx={idx}
              onMouseEnter={() => onHoverIndex(idx)}
              onMouseDown={(e) => {
                e.preventDefault();
                onSelect(item);
              }}
              className={cn(
                "flex w-full cursor-pointer items-center justify-between gap-3 rounded-[9px] px-2.5 py-1.5 text-left transition-colors",
                active
                  ? "bg-accent text-accent-foreground"
                  : "text-foreground/90 hover:bg-accent/60",
              )}
            >
              <div className="flex min-w-0 items-center gap-2.5">
                <SourceIcon className={iconClass} />
                <span className="shrink-0 text-[13px] font-medium">{item.label}</span>
                <span className="text-muted-foreground truncate text-[12px]">
                  {item.subtitle}
                </span>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
