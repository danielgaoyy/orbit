import { useEffect, useRef } from "react";
import {
  FileSearch,
  FoldVertical,
  GitCompareArrows,
  ListTodo,
  Settings,
  Shield,
  ShieldCheck,
  ShieldOff,
  SquarePen,
  SquareTerminal,
  type LucideIcon,
} from "lucide-react";
import { modSymbol } from "@/lib/keybindings";
import { ShortcutKbd } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

export type SlashCommandId =
  | "plan"
  | "auto"
  | "supervised"
  | "full"
  | "review"
  | "diff"
  | "compact"
  | "terminal"
  | "new"
  | "settings";

export interface SlashCommandDef {
  id: SlashCommandId;
  command: string;
  label: string;
  description: string;
  shortcut?: string;
  icon: LucideIcon;
  keywords: string[];
}

export function getSlashCommands(): SlashCommandDef[] {
  const mod = modSymbol();
  return [
    {
      id: "plan",
      command: "/plan",
      label: "Draft Implementation Plan",
      description: "Prompt the agent to design a step-by-step plan before editing",
      icon: ListTodo,
      keywords: ["plan", "design", "architect", "checklist"],
    },
    {
      id: "review",
      command: "/review",
      label: "Code Review Working Tree",
      description: "Attach uncommitted git diff and prepare an engineering review",
      icon: FileSearch,
      keywords: ["review", "audit", "critique", "git"],
    },
    {
      id: "diff",
      command: "/diff",
      label: "Toggle Diff Inspector",
      description: "Open or close the side-by-side Git & Turn Diff panel",
      shortcut: `${mod}D`,
      icon: GitCompareArrows,
      keywords: ["diff", "inspector", "changes"],
    },
    {
      id: "compact",
      command: "/compact",
      label: "Compact Context",
      description: "Ask active agent to summarize and compact conversation history",
      icon: FoldVertical,
      keywords: ["compact", "summarize", "context", "memory"],
    },
    {
      id: "supervised",
      command: "/ask",
      label: "Always Ask Permissions",
      description: "Require approval before file edits and shell commands",
      icon: Shield,
      keywords: ["ask", "supervised", "default", "safe", "permission"],
    },
    {
      id: "auto",
      command: "/edits",
      label: "Accept Edits Permissions",
      description: "Auto-approve file edits, ask before shell commands",
      icon: ShieldCheck,
      keywords: ["edits", "auto", "edit", "accept", "permission"],
    },
    {
      id: "full",
      command: "/full",
      label: "Full Access Permissions",
      description: "Edit files and run commands without approval prompts",
      icon: ShieldOff,
      keywords: ["full", "autonomous", "bypass", "unrestricted", "permission"],
    },
    {
      id: "terminal",
      command: "/terminal",
      label: "Toggle PTY Terminal",
      description: "Show or hide the persistent multi-slot terminal dock",
      shortcut: `${mod}J`,
      icon: SquareTerminal,
      keywords: ["terminal", "pty", "shell", "console"],
    },
    {
      id: "new",
      command: "/new",
      label: "New Agent Session",
      description: "Start a fresh isolated session in the stage",
      shortcut: `${mod}N`,
      icon: SquarePen,
      keywords: ["new", "thread", "session", "clear"],
    },
    {
      id: "settings",
      command: "/settings",
      label: "Settings",
      description: "Configure hosts, engines, worktrees, and keybindings",
      shortcut: `${mod},`,
      icon: Settings,
      keywords: ["settings", "preferences", "config"],
    },
  ];
}

export function filterSlashCommands(query: string): SlashCommandDef[] {
  const all = getSlashCommands();
  const q = query.trim().toLowerCase();
  if (!q) return all;
  return all.filter(
    (c) =>
      c.command.slice(1).toLowerCase().includes(q) ||
      c.id.includes(q) ||
      c.label.toLowerCase().includes(q) ||
      c.keywords.some((k) => k.includes(q)),
  );
}

interface SlashCommandMenuProps {
  items: SlashCommandDef[];
  activeIndex: number;
  onHoverIndex: (index: number) => void;
  onSelect: (item: SlashCommandDef) => void;
}

export function SlashCommandMenu({
  items,
  activeIndex,
  onHoverIndex,
  onSelect,
}: SlashCommandMenuProps) {
  const listRef = useRef<HTMLDivElement | null>(null);

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
          const Icon = cmd.icon;
          const active = idx === activeIndex;
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
              {cmd.shortcut && <ShortcutKbd keys={cmd.shortcut} className="shrink-0" />}
            </button>
          );
        })}
      </ScrollArea>
    </div>
  );
}
