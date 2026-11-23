import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import {
  Activity,
  Archive,
  Blocks,
  BookOpen,
  Clock,
  Code2,
  Download,
  FileCode2,
  FolderOpen,
  FolderPlus,
  FolderTree,
  GitCompareArrows,
  GitFork,
  Globe,
  LayoutList,
  MessageCircleQuestion,
  MessageSquare,
  MessageSquareText,
  Monitor,
  Moon,
  PanelLeft,
  PanelRight,
  Plug,
  Plus,
  Radio,
  RefreshCw,
  Search,
  Server,
  Settings2,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  SquareTerminal,
  Sun,
  Webhook,
  Zap,
} from "lucide-react";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog-tabs";
import { Kbd, ShortcutKbd } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useImeCompositionGuard } from "@/lib/ime";
import { type MessageKey, type TranslationParams, useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import { type ConversationSearchHit, orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface PaletteItem {
  id: string;
  sectionKey: MessageKey;
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

function formatCompactRelativeTime(
  iso: string | null | undefined,
  t: (key: MessageKey, params?: TranslationParams) => string,
): string | undefined {
  if (!iso) return undefined;
  const ts = Date.parse(iso);
  if (Number.isNaN(ts)) return undefined;
  const now = Date.now();
  const diffSec = Math.max(0, Math.floor((now - ts) / 1000));
  if (diffSec < 60) return t("palette.justNow");
  const mins = Math.floor(diffSec / 60);
  if (mins < 60) return t("palette.minutesAgo", { n: mins });
  const hours = Math.floor(mins / 60);
  if (hours < 24) return t("palette.hoursAgo", { n: hours });
  const days = Math.floor(hours / 24);
  if (days < 30) return t("palette.daysAgo", { n: days });
  try {
    const d = new Date(ts);
    const sameYear = d.getFullYear() === new Date(now).getFullYear();
    const fmt = sameYear ? SHORT_DATE_SAME_YEAR : SHORT_DATE_WITH_YEAR;
    return fmt ? fmt.format(d) : t("palette.daysAgo", { n: days });
  } catch {
    return t("palette.daysAgo", { n: days });
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

function formatRolePrefix(
  role: string | undefined,
  t: (key: MessageKey) => string,
): string | null {
  if (role === "user") return t("palette.roleYou");
  if (role === "tool") return t("palette.roleTool");
  if (role === "agent") return t("palette.roleAgent");
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
  const { t } = useI18n();
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
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
            sectionKey: "palette.sectionApproval",
            section: t("palette.sectionApproval"),
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
    const actionsSection = t("palette.sectionActions");
    const openInSection = t("palette.sectionOpenIn");
    const settingsSection = t("palette.sectionSettings");
    const fleetSection = t("palette.sectionFleet");
    list.push(
      {
        id: "act-new-thread",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("common.newConversation"),
        subtitle: t("palette.newConversationSub"),
        shortcut: `${mod}N`,
        icon: <Plus className="size-3.5" />,
        run: () => orbitActions.startNewThreadDraft(),
      },
      {
        id: "act-add-project",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.addProjectFolder"),
        subtitle: t("palette.addProjectFolderSub"),
        icon: <FolderPlus className="size-3.5" />,
        run: () => {
          orbitActions.setCommandPaletteOpen(false);
          void orbitActions.promptAddProject();
        },
      },
      {
        id: "act-automations",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.openAutomations"),
        subtitle: t("palette.openAutomationsSub"),
        icon: <Clock className="size-3.5" />,
        run: () => orbitActions.openAutomations(),
      },
      {
        id: "act-cycle-mode",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.cyclePermissions"),
        subtitle: t("palette.cyclePermissionsSub"),
        shortcut: "⇧Tab",
        icon: <Sparkles className="size-3.5" />,
        run: () => orbitActions.cycleActiveMode(),
      },
      {
        id: "act-toggle-fast",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleFastMode"),
        subtitle: t("palette.toggleFastModeSub"),
        icon: <Zap className="size-3.5" />,
        run: () => orbitActions.toggleActiveFastMode(),
      },
      ...(selectedHostId && selectedAgentId && !isDraftNewSession
        ? [
            {
              id: "act-fork-thread",
              sectionKey: "palette.sectionActions" as const,
              section: actionsSection,
              title: t("palette.forkCurrent"),
              subtitle: t("palette.forkCurrentSub"),
              icon: <GitFork className="size-3.5" />,
              run: () => {
                void orbitActions.forkAgent({
                  hostId: selectedHostId,
                  sourceAgentId: selectedAgentId,
                });
              },
            },
            {
              id: "act-export-thread",
              sectionKey: "palette.sectionActions" as const,
              section: actionsSection,
              title: t("palette.exportConversation"),
              subtitle: t("palette.exportConversationSub"),
              icon: <Download className="size-3.5" />,
              run: () => orbitActions.openExportDialog(selectedHostId, selectedAgentId),
            },
            {
              id: "act-copy-code-block",
              sectionKey: "palette.sectionActions" as const,
              section: actionsSection,
              title: t("palette.copyCodeBlock"),
              subtitle: t("palette.copyCodeBlockSub"),
              icon: <Code2 className="size-3.5" />,
              run: () => {
                void orbitActions.openCopyCodeBlocksModal(selectedHostId, selectedAgentId);
              },
            },
          ]
        : []),
      ...(selectedHostId
        ? [
            {
              id: "act-open-artifacts",
              sectionKey: "palette.sectionActions" as const,
              section: actionsSection,
              title: t("palette.openArtifacts"),
              subtitle: t("palette.openArtifactsSub"),
              icon: <Blocks className="size-3.5" />,
              run: () => orbitActions.openArtifactsTab({ hostId: selectedHostId }),
            },
          ]
        : []),
      {
        id: "act-toggle-overview",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleOverview"),
        shortcut: `${mod}⌥O`,
        icon: <LayoutList className="size-3.5" />,
        run: () => orbitActions.toggleOverviewPanel(),
      },
      {
        id: "act-toggle-side-panel",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleSidePanel"),
        shortcut: `${mod}\\`,
        icon: <PanelRight className="size-3.5" />,
        run: () => orbitActions.toggleSidePanel(),
      },
      {
        id: "act-quick-open-file",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.openFileInSidePanel"),
        shortcut: `${mod}P`,
        icon: <Search className="size-3.5" />,
        run: () => {
          orbitActions.setCommandPaletteOpen(false);
          orbitActions.setQuickOpenFilesOpen(true);
        },
      },
      {
        id: "act-files-explorer",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.openFilesExplorer"),
        icon: <FolderTree className="size-3.5" />,
        run: () => orbitActions.openFilesExplorerTab(),
      },
      {
        id: "act-project-instructions",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.projectInstructions"),
        subtitle: t("palette.projectInstructionsSub"),
        icon: <FileCode2 className="size-3.5" />,
        run: () => orbitActions.openProjectInstructions(),
      },
      {
        id: "act-toggle-diff",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleDiffReview"),
        shortcut: `${mod}D`,
        icon: <GitCompareArrows className="size-3.5" />,
        run: () => orbitActions.toggleReviewTab(),
      },
      {
        id: "act-toggle-term",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleTerminal"),
        shortcut: `${mod}J`,
        icon: <SquareTerminal className="size-3.5" />,
        run: () => orbitActions.toggleTerminalTab(),
      },
      {
        id: "act-open-browser",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.openBrowserPreview"),
        icon: <Globe className="size-3.5" />,
        run: () => orbitActions.openBrowserTab(),
      },
      {
        id: "act-side-question",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.askSideQuestion"),
        shortcut: `${mod};`,
        icon: <MessageCircleQuestion className="size-3.5" />,
        run: () => orbitActions.openSideQuestionTab(),
      },
      {
        id: "act-toggle-sidebar",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleSidebar"),
        shortcut: `${mod}B`,
        icon: <PanelLeft className="size-3.5" />,
        run: () => orbitActions.toggleSidebar(),
      },
    );

    // 3. External Editor Handoff
    for (const ed of externalEditors) {
      list.push({
        id: `editor-${ed.kind}`,
        sectionKey: "palette.sectionOpenIn",
        section: openInSection,
        title: t("palette.openInEditor", { editor: ed.label }),
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
          sectionKey: isArchived ? "palette.sectionArchived" : "palette.sectionSessions",
          section: isArchived ? t("palette.sectionArchived") : t("palette.sectionSessions"),
          title: effectiveTitle,
          subtitle: `${h.display_name} · ${ag.provider_label} · ${ag.cwd}`,
          relativeTime: formatCompactRelativeTime(ag.updated_at_rfc3339, t),
          threadKey: tKey,
          icon: <Server className="size-3.5 opacity-70" />,
          run: () => orbitActions.selectAgent(h.host_id, ag.agent_id),
        });
      }
    }

    // 5. Settings & Fleet
    const effectiveSettingsHostId = selectedHostId ?? fleet.hosts[0]?.host_id ?? null;
    list.push(
      {
        id: "settings-open",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.openSettings"),
        subtitle: t("palette.openSettingsSub"),
        shortcut: `${mod},`,
        icon: <Settings2 className="size-3.5" />,
        run: () => orbitActions.openSettings("general"),
      },
      {
        id: "settings-archived",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.openArchived"),
        subtitle: t("palette.openArchivedSub"),
        icon: <Archive className="size-3.5" />,
        run: () => orbitActions.openSettings("archived"),
      },
      {
        id: "fleet-pair",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.settingsHosts"),
        subtitle: t("palette.settingsHostsSub"),
        icon: <Radio className="size-3.5" />,
        run: () =>
          orbitActions.openSettings(
            effectiveSettingsHostId ? `host:${effectiveSettingsHostId}:connection` : "add_host",
          ),
      },
      ...(effectiveSettingsHostId
        ? [
            {
              id: "fleet-quotas",
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsModels"),
              subtitle: t("palette.settingsModelsSub"),
              icon: <Plug className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:agents`),
            },
            {
              id: "settings-usage",
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsUsage"),
              subtitle: t("palette.settingsUsageSub"),
              icon: <Activity className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:usage`),
            },
            {
              id: "settings-rules",
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsRules"),
              subtitle: t("palette.settingsRulesSub"),
              icon: <BookOpen className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:rules`),
            },
            {
              id: "settings-mcp",
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsMcp"),
              subtitle: t("palette.settingsMcpSub"),
              icon: <Webhook className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:mcp`),
            },
            {
              id: "settings-guardian",
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsGuardian"),
              subtitle: t("palette.settingsGuardianSub"),
              icon: <ShieldCheck className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:guardian`),
            },
            {
              id: "settings-im",
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsIm"),
              subtitle: t("palette.settingsImSub"),
              icon: <MessageSquare className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:im_channels`),
            },
          ]
        : []),
      {
        id: "pref-theme-system",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.themeFollowSystem"),
        subtitle:
          theme === "system" ? t("palette.themeCurrentlyActive") : t("palette.themeFollowSystemSub"),
        icon: <Monitor className="size-3.5" />,
        run: () => orbitActions.setTheme("system"),
      },
      {
        id: "pref-theme-light",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.themeLight"),
        icon: <Sun className="size-3.5" />,
        run: () => orbitActions.setTheme("light"),
      },
      {
        id: "pref-theme-dark",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.themeDark"),
        icon: <Moon className="size-3.5" />,
        run: () => orbitActions.setTheme("dark"),
      },
      {
        id: "pref-enter",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: enterToSend
          ? t("palette.requireModEnter", { mod })
          : t("palette.useEnterToSend"),
        icon: <Sparkles className="size-3.5" />,
        run: () => orbitActions.setEnterToSend(!enterToSend),
      },
    );

    if (selectedHostId) {
      list.push({
        id: "fleet-refresh",
        sectionKey: "palette.sectionFleet",
        section: fleetSection,
        title: t("palette.reconnectCurrentHost"),
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
    selectedAgentId,
    isDraftNewSession,
    threadCustomTitles,
    archivedThreads,
    mod,
    t,
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
        relativeTime: formatCompactRelativeTime(hit.updated_at_rfc3339, t) ?? item.relativeTime,
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
        sectionKey: "palette.sectionMessages",
        section: t("palette.sectionMessages"),
        title: customTitle || hit.title || t("common.conversation"),
        snippet: hit.snippet,
        snippetRole: hit.role,
        metaBadge: buildMetaBadge(hit, isMultiHost),
        relativeTime: formatCompactRelativeTime(hit.updated_at_rfc3339, t),
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
      (it) =>
        it.sectionKey === "palette.sectionSettings" || it.sectionKey === "palette.sectionFleet",
    );
    if (settingsIdx === -1) {
      return [...enrichedLocal, ...extraMessageItems];
    }
    return [
      ...enrichedLocal.slice(0, settingsIdx),
      ...extraMessageItems,
      ...enrichedLocal.slice(settingsIdx),
    ];
  }, [allItems, query, messageResult, threadCustomTitles, isMultiHost, t]);

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
      <DialogTitle className="sr-only">{t("palette.title")}</DialogTitle>
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
          placeholder={t("palette.placeholder")}
          className="text-foreground placeholder:text-muted-foreground w-full bg-transparent text-[14px] outline-none"
        />
        <Kbd>Esc</Kbd>
      </div>

      <ScrollArea viewportClassName="max-h-96 p-1.5">
        {filtered.length === 0 ? (
          <div className="text-muted-foreground py-8 text-center text-[13px]">
            {isSearchingMessages
              ? t("palette.searchingMessages")
              : t("palette.noMatches")}
          </div>
        ) : (
          filtered.map((item, idx) => {
            const prevSection = idx > 0 ? filtered[idx - 1].section : null;
            const showSection = item.section !== prevSection;
            const selected = idx === activeIndex;
            const rolePrefix = formatRolePrefix(item.snippetRole, t);

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
