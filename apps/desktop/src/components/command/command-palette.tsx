import { useEffect, useMemo, useRef, useState } from "react";
import {
  Clock,
  FolderOpen,
  FolderPlus,
  FolderTree,
  GitCompareArrows,
  Globe,
  LayoutList,
  MessageCircleQuestion,
  Monitor,
  Moon,
  PanelLeft,
  PanelRight,
  Plus,
  Radio,
  RefreshCw,
  Search,
  Server,
  Settings2,
  ShieldAlert,
  Sparkles,
  SquareTerminal,
  Sun,
} from "lucide-react";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog-tabs";
import { Kbd, ShortcutKbd } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import { orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface PaletteItem {
  id: string;
  section: string;
  title: string;
  subtitle?: string;
  shortcut?: string;
  icon: React.ReactNode;
  accent?: "amber" | "default";
  run: () => void;
}

export function CommandPalette() {
  const open = useOrbitSelector((s) => s.commandPaletteOpen);
  return (
    <Dialog open={open} onOpenChange={(v) => orbitActions.setCommandPaletteOpen(v)}>
      <DialogContent className="max-w-xl gap-0 overflow-hidden p-0 [&>button]:hidden">
        <CommandPaletteDialog />
      </DialogContent>
    </Dialog>
  );
}

function CommandPaletteDialog() {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const externalEditors = useOrbitSelector((s) => s.externalEditors);
  const theme = useOrbitSelector((s) => s.theme);
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const threadCustomTitles = useOrbitSelector((s) => s.threadCustomTitles);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const ime = useImeCompositionGuard();

  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const mod = modSymbol();

  useEffect(() => {
    const id = setTimeout(() => inputRef.current?.focus(), 10);
    return () => clearTimeout(id);
  }, []);

  const allItems = useMemo<PaletteItem[]>(() => {
    const list: PaletteItem[] = [];

    // 1. Attention / Blocked sessions first
    for (const h of fleet.hosts) {
      const detail = hostDetails[h.host_id];
      if (!detail) continue;
      for (const ag of detail.agents) {
        const tKey = threadScopeKey(h.host_id, ag.agent_id);
        if (ag.is_awaiting_permission && !archivedThreads[tKey]) {
          list.push({
            id: `attn-${h.host_id}-${ag.agent_id}`,
            section: "Needs Your Approval",
            title: threadCustomTitles[tKey] || ag.title,
            subtitle: `${h.display_name} · ${ag.cwd}`,
            shortcut: `${mod}⇧A`,
            accent: "amber",
            icon: <ShieldAlert className="size-3.5 text-amber-400" />,
            run: () => orbitActions.selectAgent(h.host_id, ag.agent_id),
          });
        }
      }
    }

    // 2. Core Workspace & Layout Actions
    list.push(
      {
        id: "act-new-thread",
        section: "Actions",
        title: "New Conversation",
        subtitle: "Open launchpad and focus Composer",
        shortcut: `${mod}N`,
        icon: <Plus className="size-3.5" />,
        run: () => orbitActions.startNewThreadDraft(),
      },
      {
        id: "act-add-project",
        section: "Actions",
        title: "Add Project Folder…",
        subtitle: "Choose a repository directory from your machine or remote host",
        icon: <FolderPlus className="size-3.5" />,
        run: () => {
          orbitActions.setCommandPaletteOpen(false);
          void orbitActions.promptAddProject();
        },
      },
      {
        id: "act-automations",
        section: "Actions",
        title: "Open Automations",
        subtitle: "Repeatable engineering workflows and checks",
        icon: <Clock className="size-3.5" />,
        run: () => orbitActions.openAutomations(),
      },
      {
        id: "act-cycle-mode",
        section: "Actions",
        title: "Cycle Access Permissions",
        subtitle: "Always Ask ⇄ Accept Edits ⇄ Full Access",
        shortcut: "⇧Tab",
        icon: <Sparkles className="size-3.5" />,
        run: () => orbitActions.cycleActiveMode(),
      },
      {
        id: "act-toggle-overview",
        section: "Actions",
        title: "Toggle Stage Overview Panel",
        shortcut: `${mod}⌥O`,
        icon: <LayoutList className="size-3.5" />,
        run: () => orbitActions.toggleOverviewPanel(),
      },
      {
        id: "act-toggle-side-panel",
        section: "Actions",
        title: "Toggle Right Side Panel",
        shortcut: `${mod}\\`,
        icon: <PanelRight className="size-3.5" />,
        run: () => orbitActions.toggleSidePanel(),
      },
      {
        id: "act-quick-open-file",
        section: "Actions",
        title: "Open File in Side Panel…",
        shortcut: `${mod}P`,
        icon: <Search className="size-3.5" />,
        run: () => {
          orbitActions.setCommandPaletteOpen(false);
          orbitActions.setQuickOpenFilesOpen(true);
        },
      },
      {
        id: "act-files-explorer",
        section: "Actions",
        title: "Open Workspace Files Explorer",
        icon: <FolderTree className="size-3.5" />,
        run: () => orbitActions.openFilesExplorerTab(),
      },
      {
        id: "act-toggle-diff",
        section: "Actions",
        title: "Toggle Diff Review Tab",
        shortcut: `${mod}D`,
        icon: <GitCompareArrows className="size-3.5" />,
        run: () => orbitActions.toggleReviewTab(),
      },
      {
        id: "act-toggle-term",
        section: "Actions",
        title: "Toggle Persistent PTY Terminal Tab",
        shortcut: `${mod}J`,
        icon: <SquareTerminal className="size-3.5" />,
        run: () => orbitActions.toggleTerminalTab(),
      },
      {
        id: "act-open-browser",
        section: "Actions",
        title: "Open Browser Preview Tab",
        icon: <Globe className="size-3.5" />,
        run: () => orbitActions.openBrowserTab(),
      },
      {
        id: "act-side-question",
        section: "Actions",
        title: "Ask Side Question (/btw)",
        shortcut: `${mod};`,
        icon: <MessageCircleQuestion className="size-3.5" />,
        run: () => orbitActions.openSideQuestionTab(),
      },
      {
        id: "act-toggle-sidebar",
        section: "Actions",
        title: "Toggle Fleet Sidebar",
        shortcut: `${mod}B`,
        icon: <PanelLeft className="size-3.5" />,
        run: () => orbitActions.toggleSidebar(),
      },
    );

    // 3. External Editor Handoff
    for (const ed of externalEditors) {
      list.push({
        id: `editor-${ed.kind}`,
        section: "Open Workspace In",
        title: `Open in ${ed.label}`,
        shortcut: ed.kind === "cursor" ? `${mod}O` : undefined,
        icon: <FolderOpen className="size-3.5" />,
        run: () => void orbitActions.openWorkspaceInEditor(ed.kind),
      });
    }

    // 4. All Agent Threads Across Multi-Host Fleet (Active first, Archived in separate section)
    for (const h of fleet.hosts) {
      const detail = hostDetails[h.host_id];
      if (!detail) continue;
      for (const ag of detail.agents) {
        const tKey = threadScopeKey(h.host_id, ag.agent_id);
        const isArchived = Boolean(archivedThreads[tKey] || ag.archived_at_rfc3339);
        const effectiveTitle = threadCustomTitles[tKey] || ag.title;
        list.push({
          id: `thread-${h.host_id}-${ag.agent_id}`,
          section: isArchived ? "Archived Sessions" : "Sessions",
          title: effectiveTitle,
          subtitle: `${h.display_name} · ${ag.provider_label} · ${ag.cwd}`,
          icon: <Server className="size-3.5 opacity-70" />,
          run: () => orbitActions.selectAgent(h.host_id, ag.agent_id),
        });
      }
    }

    // 5. Settings & Fleet
    list.push(
      {
        id: "settings-open",
        section: "Settings & Fleet",
        title: "Open Settings",
        subtitle: "General, Hosts, Providers, Worktrees, Editor & Terminal, Keybindings",
        shortcut: `${mod},`,
        icon: <Settings2 className="size-3.5" />,
        run: () => orbitActions.openSettings("general"),
      },
      {
        id: "fleet-pair",
        section: "Settings & Fleet",
        title: "Settings: Hosts & Zero-Knowledge Relay",
        subtitle: "Pair new Orbit hosts or deploy SSH Noise_NK Relay",
        icon: <Radio className="size-3.5" />,
        run: () => orbitActions.openSettings("hosts"),
      },
      {
        id: "fleet-quotas",
        section: "Settings & Fleet",
        title: "Settings: Providers & Live Quotas",
        subtitle: "Configure default CLI engine and inspect 5h/weekly windows",
        icon: <Sparkles className="size-3.5" />,
        run: () => orbitActions.openSettings("providers"),
      },
      {
        id: "pref-theme-system",
        section: "Settings & Fleet",
        title: "Appearance: Follow System",
        subtitle: theme === "system" ? "Currently active" : "Auto-sync with macOS / OS appearance",
        icon: <Monitor className="size-3.5" />,
        run: () => orbitActions.setTheme("system"),
      },
      {
        id: "pref-theme-light",
        section: "Settings & Fleet",
        title: "Appearance: Light Mode",
        icon: <Sun className="size-3.5" />,
        run: () => orbitActions.setTheme("light"),
      },
      {
        id: "pref-theme-dark",
        section: "Settings & Fleet",
        title: "Appearance: Dark Mode",
        icon: <Moon className="size-3.5" />,
        run: () => orbitActions.setTheme("dark"),
      },
      {
        id: "pref-enter",
        section: "Settings & Fleet",
        title: enterToSend
          ? `Require ${mod}Enter to Send Prompts`
          : "Use Enter to Send Prompts",
        icon: <Sparkles className="size-3.5" />,
        run: () => orbitActions.setEnterToSend(!enterToSend),
      },
    );

    if (selectedHostId) {
      list.push({
        id: "fleet-refresh",
        section: "Fleet & Preferences",
        title: "Reconnect & Refresh Current Host",
        icon: <RefreshCw className="size-3.5" />,
        run: () => {
          void orbitIpc.connectHost(selectedHostId);
          void orbitIpc.refreshHost(selectedHostId);
          void orbitIpc.refreshQuotas(selectedHostId);
        },
      });
    }

    return list;
  }, [
    fleet.hosts,
    hostDetails,
    externalEditors,
    theme,
    enterToSend,
    selectedHostId,
    threadCustomTitles,
    archivedThreads,
    mod,
  ]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return allItems;
    return allItems.filter(
      (it) =>
        it.title.toLowerCase().includes(q) ||
        (it.subtitle?.toLowerCase().includes(q) ?? false) ||
        it.section.toLowerCase().includes(q),
    );
  }, [allItems, query]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  const executeItem = (item: PaletteItem | undefined) => {
    if (!item) return;
    orbitActions.setCommandPaletteOpen(false);
    item.run();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (ime.shouldIgnoreKeyDown(e)) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((prev) => (filtered.length > 0 ? (prev + 1) % filtered.length : 0));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((prev) =>
        filtered.length > 0 ? (prev - 1 + filtered.length) % filtered.length : 0,
      );
    } else if (e.key === "Enter") {
      e.preventDefault();
      executeItem(filtered[activeIndex]);
    }
  };

  return (
    <>
      <DialogTitle className="sr-only">Orbit Command Palette</DialogTitle>
      <div className="border-border flex h-[46px] items-center gap-2.5 border-b px-4">
          <Search className="text-muted-foreground h-[16px] w-[16px] shrink-0" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            {...ime.compositionProps}
            onKeyDown={handleKeyDown}
            placeholder="Jump to session, run action, or switch host..."
            className="text-foreground placeholder:text-muted-foreground w-full bg-transparent text-[14px] outline-none"
          />
          <Kbd>Esc</Kbd>
        </div>

        <ScrollArea viewportClassName="max-h-96 p-1.5">
          {filtered.length === 0 ? (
            <div className="text-muted-foreground py-8 text-center text-[13px]">
              No matching commands or sessions.
            </div>
          ) : (
            filtered.map((item, idx) => {
              const prevSection = idx > 0 ? filtered[idx - 1].section : null;
              const showSection = item.section !== prevSection;
              const selected = idx === activeIndex;

              return (
                <div key={item.id}>
                  {showSection && (
                    <div className="text-muted-foreground px-3 pt-2.5 pb-1 text-[12px] font-medium">
                      {item.section}
                    </div>
                  )}
                  <button
                    type="button"
                    onMouseEnter={() => setActiveIndex(idx)}
                    onClick={() => executeItem(item)}
                    className={cn(
                      "flex w-full cursor-pointer items-center justify-between gap-3 rounded-[9px] px-3 py-2 text-left text-[13.5px] transition-colors",
                      selected ? "bg-accent text-foreground" : "text-foreground/85",
                    )}
                  >
                    <div className="flex min-w-0 items-center gap-2.5">
                      <span className="text-muted-foreground shrink-0 [&_svg]:h-[15px] [&_svg]:w-[15px]">
                        {item.icon}
                      </span>
                      <div className="min-w-0">
                        <div
                          className={cn(
                            "truncate font-medium",
                            item.accent === "amber" && "text-amber-600 dark:text-amber-400",
                          )}
                        >
                          {item.title}
                        </div>
                        {item.subtitle && (
                          <div className="text-muted-foreground truncate text-[12px]">
                            {item.subtitle}
                          </div>
                        )}
                      </div>
                    </div>
                    {item.shortcut && <ShortcutKbd keys={item.shortcut} className="shrink-0" />}
                  </button>
                </div>
              );
            })
          )}
        </ScrollArea>
    </>
  );
}
