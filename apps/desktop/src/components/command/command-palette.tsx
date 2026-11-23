import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import {
  Archive,
  Clock,
  FileCode2,
  FolderOpen,
  FolderPlus,
  FolderTree,
  GitCompareArrows,
  Globe,
  LayoutList,
  MessageCircleQuestion,
  MessageSquareText,
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
import { type ConversationSearchHit, orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface PaletteItem {
  id: string;
  section: string;
  title: string;
  subtitle?: string;
  snippet?: string;
  snippetRole?: string;
  metaBadge?: string;
  relativeTime?: string;
  shortcut?: string;
  icon: ReactNode;
  accent?: "amber" | "default";
  threadKey?: string;
  run: () => void;
}

const FTS_DEBOUNCE_MS = 80;
const FTS_MIN_QUERY_LEN = 2;
const FTS_RESULT_LIMIT = 20;
const CJK_CHAR_RE = /[\u3040-\u30FF\u3400-\u4DBF\u4E00-\u9FFF\uAC00-\uD7AF\uF900-\uFAFF]/;

function meetsFtsQueryThreshold(trimmed: string): boolean {
  return trimmed.length >= FTS_MIN_QUERY_LEN || (trimmed.length >= 1 && CJK_CHAR_RE.test(trimmed));
}

const SHORT_DATE_SAME_YEAR =
  typeof Intl !== "undefined"
    ? new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" })
    : null;
const SHORT_DATE_WITH_YEAR =
  typeof Intl !== "undefined"
    ? new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", year: "2-digit" })
    : null;

function formatCompactRelativeTime(iso: string | null | undefined): string | undefined {
  if (!iso) return undefined;
  const ts = Date.parse(iso);
  if (Number.isNaN(ts)) return undefined;
  const now = Date.now();
  const diffSec = Math.max(0, Math.floor((now - ts) / 1000));
  if (diffSec < 60) return "just now";
  const mins = Math.floor(diffSec / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  try {
    const d = new Date(ts);
    const sameYear = d.getFullYear() === new Date(now).getFullYear();
    const fmt = sameYear ? SHORT_DATE_SAME_YEAR : SHORT_DATE_WITH_YEAR;
    return fmt ? fmt.format(d) : `${days}d ago`;
  } catch {
    return `${days}d ago`;
  }
}

function renderHighlightedSnippet(snippet: string): ReactNode {
  const parts: ReactNode[] = [];
  const regex = /<mark>([\s\S]*?)<\/mark>/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  let idx = 0;

  while ((match = regex.exec(snippet)) !== null) {
    if (match.index > lastIndex) {
      parts.push(snippet.slice(lastIndex, match.index));
    }
    parts.push(
      <mark
        key={idx}
        className="text-foreground rounded-[3px] bg-foreground/[0.08] px-0.5 font-medium dark:bg-foreground/[0.14]"
      >
        {match[1]}
      </mark>,
    );
    idx += 1;
    lastIndex = match.index + match[0].length;
  }

  if (lastIndex < snippet.length) {
    parts.push(snippet.slice(lastIndex));
  }
  return parts.length > 0 ? parts : snippet;
}

function formatRolePrefix(role?: string): string | null {
  if (role === "user") return "You: ";
  if (role === "tool") return "Tool: ";
  if (role === "agent") return "Agent: ";
  return null;
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
  const [isComposing, setIsComposing] = useState(false);
  const [messageResult, setMessageResult] = useState<{
    query: string;
    hits: ConversationSearchHit[];
  }>({ query: "", hits: [] });
  const [isSearchingMessages, setIsSearchingMessages] = useState(false);

  const inputRef = useRef<HTMLInputElement | null>(null);
  const itemRefs = useRef<Map<number, HTMLButtonElement>>(new Map());
  const searchReqIdRef = useRef(0);
  const mod = modSymbol();
  const isMultiHost = fleet.hosts.length > 1;

  useEffect(() => {
    const id = setTimeout(() => inputRef.current?.focus(), 10);
    return () => clearTimeout(id);
  }, []);

  // Debounced (80ms) SQLite FTS5 cross-conversation search (2+ chars, or 1+ CJK char)
  useEffect(() => {
    if (isComposing) return;
    const trimmed = query.trim();
    const reqId = searchReqIdRef.current + 1;
    searchReqIdRef.current = reqId;

    if (!meetsFtsQueryThreshold(trimmed)) {
      setMessageResult({ query: "", hits: [] });
      setIsSearchingMessages(false);
      return;
    }

    let cancelled = false;
    setIsSearchingMessages(true);
    const timer = window.setTimeout(() => {
      void orbitIpc
        .searchMessages(trimmed, FTS_RESULT_LIMIT)
        .then((hits) => {
          if (cancelled || searchReqIdRef.current !== reqId) return;
          setMessageResult({ query: trimmed, hits });
          setIsSearchingMessages(false);
        })
        .catch(() => {
          if (cancelled || searchReqIdRef.current !== reqId) return;
          setMessageResult({ query: trimmed, hits: [] });
          setIsSearchingMessages(false);
        });
    }, FTS_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query, isComposing]);

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
            threadKey: tKey,
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
        id: "act-project-instructions",
        section: "Actions",
        title: "Project Instructions",
        subtitle: "Inspect & edit CLAUDE.md, AGENTS.md, local overrides, and @imports",
        icon: <FileCode2 className="size-3.5" />,
        run: () => orbitActions.openProjectInstructions(),
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
          relativeTime: formatCompactRelativeTime(ag.updated_at_rfc3339),
          threadKey: tKey,
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
        id: "settings-archived",
        section: "Settings & Fleet",
        title: "Open Archived Conversations",
        subtitle: "Search, restore, or permanently delete archived threads",
        icon: <Archive className="size-3.5" />,
        run: () => orbitActions.openSettings("archived"),
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

  const filtered = useMemo<PaletteItem[]>(() => {
    const trimmed = query.trim();
    const q = trimmed.toLowerCase();
    if (!q) return allItems;

    const activeMessageHits = messageResult.query === trimmed ? messageResult.hits : [];
    const hitByThread = new Map<string, ConversationSearchHit>();
    for (const hit of activeMessageHits) {
      const key = threadScopeKey(hit.host_id, hit.agent_id);
      if (!hitByThread.has(key)) {
        hitByThread.set(key, hit);
      }
    }

    const matchedLocal = allItems.filter(
      (it) =>
        it.title.toLowerCase().includes(q) ||
        (it.subtitle?.toLowerCase().includes(q) ?? false) ||
        it.section.toLowerCase().includes(q),
    );

    // Enrich already-listed session hits with matched FTS snippet & jump-to-seq,
    // and track deduplicated thread keys so they don't repeat under Messages.
    const seenThreads = new Set<string>();
    const enrichedLocal = matchedLocal.map((item) => {
      if (!item.threadKey) return item;
      seenThreads.add(item.threadKey);
      const hit = hitByThread.get(item.threadKey);
      if (!hit) return item;
      const metaBadge = buildMetaBadge(hit, isMultiHost);
      return {
        ...item,
        snippet: hit.snippet,
        snippetRole: hit.role,
        metaBadge,
        relativeTime: formatCompactRelativeTime(hit.updated_at_rfc3339) ?? item.relativeTime,
        run: () => orbitActions.selectAgent(hit.host_id, hit.agent_id, hit.seq),
      };
    });

    const extraMessageItems: PaletteItem[] = [];
    for (const hit of activeMessageHits) {
      const tKey = threadScopeKey(hit.host_id, hit.agent_id);
      if (seenThreads.has(tKey)) continue;
      seenThreads.add(tKey);
      const customTitle = threadCustomTitles[tKey];
      extraMessageItems.push({
        id: `msg-${hit.host_id}-${hit.agent_id}-${hit.seq}`,
        section: "Messages",
        title: customTitle || hit.title || "Conversation",
        snippet: hit.snippet,
        snippetRole: hit.role,
        metaBadge: buildMetaBadge(hit, isMultiHost),
        relativeTime: formatCompactRelativeTime(hit.updated_at_rfc3339),
        threadKey: tKey,
        icon:
          hit.role === "tool" ? (
            <SquareTerminal className="size-3.5 opacity-70" />
          ) : (
            <MessageSquareText className="size-3.5 opacity-70" />
          ),
        run: () => orbitActions.selectAgent(hit.host_id, hit.agent_id, hit.seq),
      });
    }

    if (extraMessageItems.length === 0) {
      return enrichedLocal;
    }

    // Place Messages cleanly below Commands/Conversations and ahead of Settings & Fleet
    const settingsIdx = enrichedLocal.findIndex(
      (it) => it.section === "Settings & Fleet" || it.section === "Fleet & Preferences",
    );
    if (settingsIdx === -1) {
      return [...enrichedLocal, ...extraMessageItems];
    }
    return [
      ...enrichedLocal.slice(0, settingsIdx),
      ...extraMessageItems,
      ...enrichedLocal.slice(settingsIdx),
    ];
  }, [allItems, query, messageResult, threadCustomTitles, isMultiHost]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  useEffect(() => {
    setActiveIndex((prev) => {
      if (filtered.length === 0) return 0;
      return prev >= filtered.length ? filtered.length - 1 : prev;
    });
  }, [filtered.length]);

  const executeItem = (item: PaletteItem | undefined) => {
    if (!item) return;
    orbitActions.setCommandPaletteOpen(false);
    item.run();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (ime.shouldIgnoreKeyDown(e)) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((prev) => {
        const next = filtered.length > 0 ? (prev + 1) % filtered.length : 0;
        itemRefs.current.get(next)?.scrollIntoView({ block: "nearest" });
        return next;
      });
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((prev) => {
        const next = filtered.length > 0 ? (prev - 1 + filtered.length) % filtered.length : 0;
        itemRefs.current.get(next)?.scrollIntoView({ block: "nearest" });
        return next;
      });
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
          onCompositionStart={() => {
            setIsComposing(true);
            ime.compositionProps.onCompositionStart();
          }}
          onCompositionEnd={() => {
            setIsComposing(false);
            ime.compositionProps.onCompositionEnd();
          }}
          onKeyDown={handleKeyDown}
          placeholder="Search messages, sessions, or commands..."
          className="text-foreground placeholder:text-muted-foreground w-full bg-transparent text-[14px] outline-none"
        />
        <Kbd>Esc</Kbd>
      </div>

      <ScrollArea viewportClassName="max-h-96 p-1.5">
        {filtered.length === 0 ? (
          <div className="text-muted-foreground py-8 text-center text-[13px]">
            {isSearchingMessages
              ? "Searching messages…"
              : "No matching messages, commands, or sessions."}
          </div>
        ) : (
          filtered.map((item, idx) => {
            const prevSection = idx > 0 ? filtered[idx - 1].section : null;
            const showSection = item.section !== prevSection;
            const selected = idx === activeIndex;
            const rolePrefix = formatRolePrefix(item.snippetRole);

            return (
              <div key={item.id}>
                {showSection && (
                  <div className="text-muted-foreground px-3 pt-2.5 pb-1 text-[12px] font-medium">
                    {item.section}
                  </div>
                )}
                <button
                  ref={(el) => {
                    if (el) {
                      itemRefs.current.set(idx, el);
                    } else {
                      itemRefs.current.delete(idx);
                    }
                  }}
                  type="button"
                  onMouseEnter={() => setActiveIndex(idx)}
                  onClick={() => executeItem(item)}
                  className={cn(
                    "flex w-full cursor-pointer items-center justify-between gap-3 rounded-[9px] px-3 py-2 text-left text-[13.5px] transition-colors",
                    selected ? "bg-accent text-foreground" : "text-foreground/85",
                  )}
                >
                  <div className="flex min-w-0 flex-1 items-center gap-2.5">
                    <span className="text-muted-foreground shrink-0 [&_svg]:h-[15px] [&_svg]:w-[15px]">
                      {item.icon}
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="flex min-w-0 items-center gap-2">
                        <span
                          className={cn(
                            "truncate font-medium",
                            item.accent === "amber" && "text-amber-600 dark:text-amber-400",
                          )}
                        >
                          {item.title}
                        </span>
                        {item.metaBadge && (
                          <span className="text-muted-foreground/80 bg-foreground/[0.045] dark:bg-foreground/[0.07] shrink-0 truncate max-w-[160px] rounded px-1.5 py-0.5 text-[11px] leading-none font-normal">
                            {item.metaBadge}
                          </span>
                        )}
                      </div>
                      {item.snippet ? (
                        <div className="text-muted-foreground mt-0.5 truncate text-[12px]">
                          {rolePrefix && (
                            <span className="text-muted-foreground/70">{rolePrefix}</span>
                          )}
                          {renderHighlightedSnippet(item.snippet)}
                        </div>
                      ) : (
                        item.subtitle && (
                          <div className="text-muted-foreground truncate text-[12px]">
                            {item.subtitle}
                          </div>
                        )
                      )}
                    </div>
                  </div>
                  {item.shortcut ? (
                    <ShortcutKbd keys={item.shortcut} className="shrink-0" />
                  ) : (
                    item.relativeTime && (
                      <span className="text-muted-foreground/70 shrink-0 text-[11px] tabular-nums">
                        {item.relativeTime}
                      </span>
                    )
                  )}
                </button>
              </div>
            );
          })
        )}
      </ScrollArea>
    </>
  );
}

function buildMetaBadge(hit: ConversationSearchHit, isMultiHost: boolean): string | undefined {
  const parts: string[] = [];
  if (hit.project_name) parts.push(hit.project_name);
  if (isMultiHost && hit.host_name) parts.push(hit.host_name);
  return parts.length > 0 ? parts.join(" · ") : undefined;
}
