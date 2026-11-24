import { useEffect, useMemo, useRef } from "react";
import {
  Check,
  Copy,
  Download,
  FilePenLine,
  FileSearch,
  FoldVertical,
  FolderTree,
  GitCompareArrows,
  GitFork,
  Globe,
  Layers,
  ListTodo,
  MessageCircleQuestion,
  Send,
  Settings,
  Shield,
  ShieldCheck,
  ShieldOff,
  Sparkles,
  SquarePen,
  SquareTerminal,
  Target,
  Terminal,
  Users,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import type { SessionMode, SlashCommandView } from "@/lib/orbit-ipc";
import {
  SLASH_GROUP_LABEL_KEY,
  filterSlashCommands,
  isSlashCommandSessionRequired,
  resolveSlashCommandGroup,
  resolveSlashDescription,
  type SlashCommandGroupId,
} from "@/lib/slash-commands";
import { ShortcutKbd } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

export {
  filterSlashCommands,
  isSlashCommandSessionRequired,
  resolveSlashCommandGroup,
  resolveSlashDescription,
  type SlashCommandGroupId,
};

export const SLASH_MENU_LISTBOX_ID = "orbit-composer-slash-menu";

export function slashOptionId(index: number): string {
  return `orbit-slash-opt-${index}`;
}

export function resolveSlashCommandIcon(id: string, iconKey = ""): LucideIcon {
  if (iconKey === "skill" || id.startsWith("skill:")) {
    return Sparkles;
  }
  switch (id || iconKey) {
    case "fork":
      return GitFork;
    case "btw":
      return MessageCircleQuestion;
    case "plan":
      return ListTodo;
    case "goal":
      return Target;
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
    case "export":
      return Download;
    case "copy":
      return Copy;
    case "fast":
    case "zap":
      return Zap;
    case "orbit":
      return Layers;
    case "orbit-handoff":
      return Send;
    case "orbit-committee":
      return Users;
    case "orbit-advisor":
      return Sparkles;
    case "supervised":
      return Shield;
    case "auto":
      return ShieldCheck;
    case "edits":
    case "file_pen":
      return FilePenLine;
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

interface SlashCommandMenuProps {
  items: SlashCommandView[];
  query: string;
  activeIndex: number;
  activeMode?: SessionMode;
  fastModeActive?: boolean;
  onHoverIndex: (index: number) => void;
  onSelect: (item: SlashCommandView) => void;
}

export function SlashCommandMenu({
  items,
  query,
  activeIndex,
  activeMode,
  fastModeActive = false,
  onHoverIndex,
  onSelect,
}: SlashCommandMenuProps) {
  const { t } = useI18n();
  const listRef = useRef<HTMLDivElement | null>(null);
  const mod = modSymbol();
  const isUnfiltered = query.trim().length === 0;

  useEffect(() => {
    const container = listRef.current;
    if (!container) return;
    const activeEl = container.querySelector<HTMLElement>(`[data-slash-idx="${activeIndex}"]`);
    activeEl?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  const groupStartIndices = useMemo(() => {
    const map = new Map<number, SlashCommandGroupId>();
    if (!isUnfiltered) return map;
    let prev: SlashCommandGroupId | null = null;
    for (let i = 0; i < items.length; i += 1) {
      const g = resolveSlashCommandGroup(items[i]);
      if (g !== prev) {
        map.set(i, g);
        prev = g;
      }
    }
    return map;
  }, [items, isUnfiltered]);

  if (items.length === 0) return null;

  const isModeSelected = (id: string): boolean => {
    if (!activeMode || typeof activeMode !== "string") return false;
    if (id === "supervised") return activeMode === "default" || activeMode === "plan";
    if (id === "auto") return activeMode === "auto";
    if (id === "edits") return activeMode === "accept_edits";
    if (id === "full") return activeMode === "full_access";
    return false;
  };

  return (
    <div
      id={SLASH_MENU_LISTBOX_ID}
      role="listbox"
      aria-label={t("composer.actions")}
      className="text-popover-foreground floating-surface orbit-inline-menu-up border-border-default absolute right-0 bottom-full left-0 z-40 mb-2 overflow-hidden rounded-[14px] border"
    >
      <ScrollArea viewportRef={listRef} viewportClassName="max-h-72 p-1.5">
        {items.map((cmd, idx) => {
          const Icon = resolveSlashCommandIcon(cmd.id, cmd.icon_key);
          const active = idx === activeIndex;
          const groupHeader = groupStartIndices.get(idx);
          const shortcut = cmd.shortcut_key ? `${mod}${cmd.shortcut_key}` : null;
          const localizedDesc = resolveSlashDescription(cmd, t);
          const modeActive = isModeSelected(cmd.id);
          const isFastCmd = cmd.id === "fast";

          return (
            <div key={cmd.id}>
              {groupHeader && (
                <div
                  className={cn(
                    "text-ink-faint px-2.5 pt-1 pb-1 text-[11px] font-medium tracking-[0.04em] uppercase select-none",
                    idx > 0 && "border-border-subtle mt-1 border-t pt-2",
                  )}
                >
                  {t(SLASH_GROUP_LABEL_KEY[groupHeader])}
                </div>
              )}
              <button
                id={slashOptionId(idx)}
                role="option"
                aria-selected={active}
                type="button"
                data-slash-idx={idx}
                onMouseEnter={() => onHoverIndex(idx)}
                onMouseDown={(e) => {
                  // Keep focus inside composer textarea
                  e.preventDefault();
                  onSelect(cmd);
                }}
                className={cn(
                  "flex w-full cursor-pointer items-center justify-between gap-3 rounded-[9px] px-2.5 py-1.5 text-left transition-colors duration-110",
                  active
                    ? "bg-accent text-ink-primary"
                    : "text-ink-secondary hover:bg-accent/60 hover:text-ink-primary",
                )}
              >
                <div className="flex min-w-0 items-center gap-2.5">
                  <Icon
                    className={cn(
                      "size-4 shrink-0 stroke-[1.75]",
                      active || modeActive ? "text-ink-primary" : "text-ink-muted",
                    )}
                  />
                  <div className="flex min-w-0 items-baseline gap-2">
                    <span className="text-ink-primary shrink-0 font-mono text-[13px] font-medium">
                      {cmd.command}
                    </span>
                    <span className="text-ink-muted truncate text-[11px]">
                      {localizedDesc}
                    </span>
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  {modeActive && (
                    <span className="text-ink-primary inline-flex items-center gap-1 text-[11px] font-medium">
                      <Check className="size-3.5 stroke-[2]" />
                    </span>
                  )}
                  {isFastCmd && fastModeActive && (
                    <span className="bg-signal-info-bg text-signal-info rounded px-1.5 py-0.5 text-[11px] font-medium">
                      {t("composer.slashStateOn")}
                    </span>
                  )}
                  {shortcut ? (
                    <ShortcutKbd keys={shortcut} className="shrink-0" />
                  ) : cmd.icon_key === "skill" ? (
                    <span className="bg-surface-recessed text-ink-muted shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium">
                      {t("composer.slashBadgeSkill")}
                    </span>
                  ) : null}
                </div>
              </button>
            </div>
          );
        })}
      </ScrollArea>
    </div>
  );
}
