import { useEffect, useRef } from "react";
import {
  FileSearch,
  FoldVertical,
  FolderTree,
  GitCompareArrows,
  Globe,
  ListTodo,
  MessageCircleQuestion,
  Settings,
  Shield,
  ShieldCheck,
  ShieldOff,
  SquarePen,
  SquareTerminal,
  Terminal,
  type LucideIcon,
} from "lucide-react";
import { modSymbol } from "@/lib/keybindings";
import type { SlashCommandView } from "@/lib/orbit-ipc";
import { ShortcutKbd } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

function resolveSlashCommandIcon(cmd: SlashCommandView): LucideIcon {
  switch (cmd.id || cmd.icon_key) {
    case "btw":
      return MessageCircleQuestion;
    case "plan":
      return ListTodo;
    case "review":
      return FileSearch;
    case "diff":
      return GitCompareArrows;
    case "files":
      return FolderTree;
    case "browser":
      return Globe;
    case "compact":
      return FoldVertical;
    case "supervised":
      return Shield;
    case "auto":
      return ShieldCheck;
    case "full":
      return ShieldOff;
    case "terminal":
      return SquareTerminal;
    case "new":
      return SquarePen;
    case "settings":
      return Settings;
    default:
      return Terminal;
  }
}

export function filterSlashCommands(
  commands: readonly SlashCommandView[],
  query: string,
): SlashCommandView[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...commands];
  return commands.filter(
    (c) =>
      c.command.slice(1).toLowerCase().includes(q) ||
      c.id.toLowerCase().includes(q) ||
      c.label.toLowerCase().includes(q) ||
      c.keywords.some((k) => k.toLowerCase().includes(q)),
  );
}

interface SlashCommandMenuProps {
  items: SlashCommandView[];
  activeIndex: number;
  onHoverIndex: (index: number) => void;
  onSelect: (item: SlashCommandView) => void;
}

export function SlashCommandMenu({
  items,
  activeIndex,
  onHoverIndex,
  onSelect,
}: SlashCommandMenuProps) {
  const listRef = useRef<HTMLDivElement | null>(null);
  const mod = modSymbol();

  useEffect(() => {
    const container = listRef.current;
    if (!container) return;
    const activeEl = container.querySelector<HTMLElement>(`[data-slash-idx="${activeIndex}"]`);
    activeEl?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  if (items.length === 0) return null;

  return (
    <div className="bg-popover text-popover-foreground floating-surface orbit-inline-menu-up absolute right-0 bottom-full left-0 z-40 mb-2 overflow-hidden rounded-[14px] border border-black/[0.09] dark:border-white/[0.10]">
      <ScrollArea viewportRef={listRef} viewportClassName="max-h-64 p-1.5">
        {items.map((cmd, idx) => {
          const Icon = resolveSlashCommandIcon(cmd);
          const active = idx === activeIndex;
          const shortcut = cmd.shortcut_key ? `${mod}${cmd.shortcut_key}` : null;
          return (
            <button
              key={cmd.id}
              type="button"
              data-slash-idx={idx}
              onMouseEnter={() => onHoverIndex(idx)}
              onMouseDown={(e) => {
                // Keep focus inside composer textarea
                e.preventDefault();
                onSelect(cmd);
              }}
              className={cn(
                "flex w-full cursor-pointer items-center justify-between gap-3 rounded-[9px] px-2.5 py-1.5 text-left transition-colors",
                active
                  ? "bg-accent text-accent-foreground"
                  : "text-foreground/90 hover:bg-accent/60",
              )}
            >
              <div className="flex min-w-0 items-center gap-2.5">
                <Icon
                  className={cn(
                    "size-4 shrink-0 stroke-[1.75]",
                    active ? "text-foreground" : "text-muted-foreground",
                  )}
                />
                <div className="flex min-w-0 items-baseline gap-2">
                  <span className="shrink-0 font-mono text-[13px] font-medium">
                    {cmd.command}
                  </span>
                  <span className="text-muted-foreground truncate text-[12px]">
                    {cmd.description}
                  </span>
                </div>
              </div>
              {shortcut && <ShortcutKbd keys={shortcut} className="shrink-0" />}
            </button>
          );
        })}
      </ScrollArea>
    </div>
  );
}
