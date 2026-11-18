import { useEffect, useMemo, useRef, useState } from "react";
import {
  ExternalLink,
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
  Sun,
  Terminal as TerminalIcon,
} from "lucide-react";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog-tabs";
import { Kbd, ShortcutKbd } from "@/components/ui/overlays";
import { modSymbol } from "@/lib/keybindings";
import { orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
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
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const externalEditors = useOrbitSelector((s) => s.externalEditors);
  const theme = useOrbitSelector((s) => s.theme);
  const enterToSend = useOrbitSelector((s) => s.enterToSend);

  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const mod = modSymbol();

  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIndex(0);
      setTimeout(() => inputRef.current?.focus(), 10);
    }
  }, [open]);

  const allItems = useMemo<PaletteItem[]>(() => {
    const list: PaletteItem[] = [];

    // 1. Attention / Blocked sessions first
    for (const h of fleet.hosts) {
      const detail = hostDetails[h.host_id];
      if (!detail) continue;
      for (const ag of detail.agents) {
        if (ag.is_awaiting_permission) {
          list.push({
            id: `attn-${h.host_id}-${ag.agent_id}`,
            section: "Needs Your Approval",
            title: ag.title,
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
        title: "New Agent Session",
        subtitle: "Open zero-modal launchpad and focus Composer",
        shortcut: `${mod}N`,
        icon: <Plus className="size-3.5" />,
        run: () => orbitActions.startNewThreadDraft(),
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
        id: "act-toggle-diff",
        section: "Actions",
        title: "Toggle Diff Review Inspector",
        shortcut: `${mod}D`,
        icon: <PanelRight className="size-3.5" />,
        run: () => orbitActions.toggleInspector(),
      },
      {
        id: "act-toggle-term",
        section: "Actions",
        title: "Toggle Persistent PTY Terminal",
        shortcut: `${mod}J`,
        icon: <TerminalIcon className="size-3.5" />,
        run: () => orbitActions.toggleTerminalDock(),
      },
      {
        id: "act-toggle-sidebar",
        section: "Actions",
        title: "Toggle Fleet Sidebar (Zen Mode)",
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
        icon: <ExternalLink className="size-3.5" />,
        run: () => void orbitActions.openWorkspaceInEditor(ed.kind),
      });
    }

    // 4. All Agent Threads Across Multi-Host Fleet
    for (const h of fleet.hosts) {
      const detail = hostDetails[h.host_id];
      if (!detail) continue;
      for (const ag of detail.agents) {
        list.push({
          id: `thread-${h.host_id}-${ag.agent_id}`,
          section: "Sessions",
          title: ag.title,
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
        section: "Fleet & Preferences",
        title: "Appearance: Light Mode",
        icon: <Sun className="size-3.5" />,
        run: () => orbitActions.setTheme("light"),
      },
      {
        id: "pref-theme-dark",
        section: "Fleet & Preferences",
        title: "Appearance: Dark Mode",
        icon: <Moon className="size-3.5" />,
        run: () => orbitActions.setTheme("dark"),
      },
      {
        id: "pref-enter",
        section: "Fleet & Preferences",
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
  }, [fleet.hosts, hostDetails, externalEditors, theme, enterToSend, selectedHostId, mod]);

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
    <Dialog open={open} onOpenChange={(v) => orbitActions.setCommandPaletteOpen(v)}>
      <DialogContent className="max-w-xl gap-0 overflow-hidden p-0 [&>button]:hidden">
        <DialogTitle className="sr-only">Orbit Command Palette</DialogTitle>
        <div className="border-border flex items-center gap-2.5 border-b px-3.5 py-2.5">
          <Search className="text-muted-foreground size-4 shrink-0" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Jump to session, run action, or switch host..."
            className="text-foreground placeholder:text-muted-foreground w-full bg-transparent text-sm outline-none"
          />
          <Kbd>Esc</Kbd>
        </div>

        <div className="max-h-88 overflow-y-auto p-1.5">
          {filtered.length === 0 ? (
            <div className="text-muted-foreground py-8 text-center text-xs">
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
                    <div className="text-muted-foreground px-2.5 pt-2 pb-1 text-[10.5px] font-medium">
                      {item.section}
                    </div>
                  )}
                  <button
                    type="button"
                    onMouseEnter={() => setActiveIndex(idx)}
                    onClick={() => executeItem(item)}
                    className={cn(
                      "flex w-full cursor-pointer items-center justify-between gap-3 rounded-md px-2.5 py-1.5 text-left text-xs transition-colors",
                      selected ? "bg-accent text-foreground" : "text-foreground/85",
                    )}
                  >
                    <div className="flex min-w-0 items-center gap-2.5">
                      <span className="text-muted-foreground shrink-0">{item.icon}</span>
                      <div className="min-w-0">
                        <div
                          className={cn(
                            "truncate font-medium",
                            item.accent === "amber" && "text-amber-400",
                          )}
                        >
                          {item.title}
                        </div>
                        {item.subtitle && (
                          <div className="text-muted-foreground truncate text-[10.5px]">
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
        </div>
      </DialogContent>
    </Dialog>
  );
}
