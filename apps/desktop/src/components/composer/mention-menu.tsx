import { useEffect, useRef } from "react";
import {
  AtSign,
  BookOpen,
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
  Film,
  GitCompareArrows,
  Music,
  SquareTerminal,
  Users,
  type LucideIcon,
} from "lucide-react";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { MentionCandidateView } from "@/lib/orbit-ipc";
import { symbolKindGlyph } from "@/lib/symbol-mentions";
import { cn } from "@/lib/utils";

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
    case "pdf":
      return FileText;
    case "mp4":
    case "mov":
    case "m4v":
    case "webm":
    case "mkv":
      return Film;
    case "mp3":
    case "wav":
    case "m4a":
    case "aac":
    case "ogg":
    case "flac":
    case "opus":
    case "weba":
      return Music;
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

interface MentionMenuProps {
  items: MentionCandidateView[];
  activeIndex: number;
  onHoverIndex: (index: number) => void;
  onSelect: (item: MentionCandidateView) => void;
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

  const firstSymbolIdx = items.findIndex((it) => it.kind === "code_symbol");

  return (
    <div className="bg-popover text-popover-foreground floating-surface orbit-inline-menu-up absolute right-0 bottom-full left-0 z-40 mb-2 overflow-hidden rounded-[14px] border border-black/[0.09] dark:border-white/[0.10]">
      <ScrollArea viewportRef={listRef} viewportClassName="max-h-64 p-1.5">
        {items.map((item, idx) => {
          const active = idx === activeIndex;
          const iconClass = cn(
            "size-4 shrink-0 stroke-[1.75]",
            active ? "text-foreground" : "text-muted-foreground",
          );

          if (item.kind === "code_symbol") {
            const meta = symbolKindGlyph(item.symbol_kind ?? "symbol");
            const showHeader = idx === firstSymbolIdx;
            const locLabel =
              item.file_path && item.start_line
                ? `${item.file_path}:L${item.start_line}`
                : item.subtitle;
            return (
              <div key={item.key}>
                {showHeader && (
                  <div
                    className={cn(
                      "text-muted-foreground/75 px-2.5 pt-1 pb-1 text-[10.5px] font-medium tracking-[0.04em] uppercase select-none",
                      idx > 0 && "mt-1 border-t border-black/[0.06] pt-2 dark:border-white/[0.07]",
                    )}
                  >
                    Symbols
                  </div>
                )}
                <button
                  type="button"
                  data-mention-idx={idx}
                  title={item.signature || `${item.title} — ${locLabel}`}
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
                  <div className="flex min-w-0 items-center gap-2">
                    <span
                      aria-label={meta.label}
                      className={cn(
                        "inline-flex size-[18px] shrink-0 items-center justify-center rounded-[5px] font-mono text-[11px] font-semibold",
                        active
                          ? "bg-foreground/12 text-foreground"
                          : "bg-muted text-muted-foreground dark:bg-white/[0.08]",
                      )}
                    >
                      {meta.glyph}
                    </span>
                    <span className="shrink-0 font-mono text-[13px] font-medium">
                      {item.title}
                    </span>
                    {item.container && (
                      <span className="text-muted-foreground/80 shrink-0 font-mono text-[11px]">
                        {item.container}
                      </span>
                    )}
                  </div>
                  <span className="text-muted-foreground truncate font-mono text-[11.5px]">
                    {locLabel}
                  </span>
                </button>
              </div>
            );
          }

          if (item.kind === "file") {
            const path = item.file_path || item.title;
            const slashIdx = path.lastIndexOf("/");
            const basename = slashIdx >= 0 ? path.slice(slashIdx + 1) : path;
            const dirPath = slashIdx >= 0 ? path.slice(0, slashIdx) : "";
            const FileIcon = resolveSemanticFileIcon(basename);
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
                    {basename}
                  </span>
                  {dirPath && (
                    <span className="text-muted-foreground truncate font-mono text-[11.5px]">
                      {dirPath}
                    </span>
                  )}
                </div>
                {item.git_status && (
                  <span className="bg-muted text-muted-foreground shrink-0 rounded-[4px] px-1.5 py-0.5 font-mono text-[10.5px] font-medium dark:bg-white/[0.09]">
                    {item.git_status}
                  </span>
                )}
              </button>
            );
          }

          if (item.kind === "path_rule") {
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
                  <BookOpen className={iconClass} />
                  <span className="shrink-0 font-mono text-[13px] font-medium">
                    {item.title}
                  </span>
                  <span className="text-muted-foreground truncate text-[12px]">
                    {item.subtitle}
                  </span>
                </div>
                <span className="bg-muted text-muted-foreground shrink-0 rounded-[4px] px-1.5 py-0.5 text-[10.5px] font-medium dark:bg-white/[0.09]">
                  {item.git_status || "Rule"}
                </span>
              </button>
            );
          }

          if (item.kind === "workspace_peer") {
            const isBroadcast = item.token_label === "@all";
            const PeerIcon = isBroadcast ? Users : AtSign;
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
                  <PeerIcon className={iconClass} />
                  <span className="shrink-0 font-mono text-[13px] font-medium">
                    {item.title}
                  </span>
                  <span className="text-muted-foreground truncate text-[12px]">
                    {item.subtitle}
                  </span>
                </div>
                <span className="bg-muted text-muted-foreground shrink-0 rounded-[4px] px-1.5 py-0.5 text-[10.5px] font-medium dark:bg-white/[0.09]">
                  {isBroadcast ? "Broadcast" : "Peer"}
                </span>
              </button>
            );
          }

          const isStagedDiff = item.key.includes("staged");
          const SourceIcon =
            item.kind === "git_diff"
              ? isStagedDiff
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
                <span className="shrink-0 text-[13px] font-medium">{item.title}</span>
                <span className="text-muted-foreground truncate text-[12px]">
                  {item.subtitle}
                </span>
              </div>
            </button>
          );
        })}
      </ScrollArea>
    </div>
  );
}
