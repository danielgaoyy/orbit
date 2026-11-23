import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import {
  Activity,
  Archive,
  AtSign,
  Blocks,
  BookOpen,
  Clock,
  Code2,
  Compass,
  Download,
  FileCode2,
  FolderOpen,
  FolderPlus,
  FolderTree,
  GitCompareArrows,
  GitFork,
  Globe,
  Hash,
  LayoutList,
  MessageCircleQuestion,
  MessageSquare,
  MessageSquareText,
  Monitor,
  Moon,
  PanelLeft,
  PanelRight,
  Play,
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
  Square,
  SquareTerminal,
  Sun,
  Terminal,
  Webhook,
  X,
  Zap,
} from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { useWorkspaceScripts } from "@/components/side-panel/scripts-panel";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog-tabs";
import { Kbd, ShortcutKbd } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  fuzzyMatch,
  getPaletteRecencyBonus,
  loadPaletteRecents,
  recordPaletteRecent,
  splitHighlightSegments,
} from "@/lib/fuzzy-match";
import { useImeCompositionGuard } from "@/lib/ime";
import { type MessageKey, type TranslationParams, useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import { type ConversationSearchHit, orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type PaletteMode = "all" | "commands" | "threads" | "files";
type PaletteItemKind = "command" | "thread" | "file";

interface PaletteItem {
  id: string;
  recentId?: string;
  kind: PaletteItemKind;
  sectionKey: MessageKey;
  section: string;
  title: string;
  subtitle?: string;
  keywords?: string;
  snippet?: string;
  snippetRole?: string;
  metaBadge?: string;
  gitStatus?: string;
  relativeTime?: string;
  shortcut?: string;
  icon: ReactNode;
  accent?: "amber" | "default";
  threadKey?: string;
  titleIndices?: number[];
  subtitleIndices?: number[];
  run: () => void;
}

const EMPTY_FILES: readonly string[] = Object.freeze([]);
const FTS_DEBOUNCE_MS = 80;
const FTS_MIN_QUERY_LEN = 2;
const FTS_RESULT_LIMIT = 20;
const MAX_RECENT_SECTION_ITEMS = 5;
const MAX_FILE_RESULTS = 60;
const CJK_CHAR_RE = /[\u3040-\u30FF\u3400-\u4DBF\u4E00-\u9FFF\uAC00-\uD7AF\uF900-\uFAFF]/;

function meetsFtsQueryThreshold(trimmed: string): boolean {
  return trimmed.length >= FTS_MIN_QUERY_LEN || (trimmed.length >= 1 && CJK_CHAR_RE.test(trimmed));
}

function parsePaletteQuery(raw: string): { mode: PaletteMode; term: string } {
  const leading = raw.trimStart();
  if (leading.startsWith(">")) {
    return { mode: "commands", term: leading.slice(1).trim() };
  }
  if (leading.startsWith("@")) {
    return { mode: "threads", term: leading.slice(1).trim() };
  }
  if (leading.startsWith("#")) {
    return { mode: "files", term: leading.slice(1).trim() };
  }
  return { mode: "all", term: raw.trim() };
}

function parseFileTermAndLine(raw: string): { pathQuery: string; line: number | null } {
  const trimmed = raw.trim();
  const match = trimmed.match(/^(.*?):(\d+)(?::\d+)?$/);
  if (match && match[1]) {
    const line = Number.parseInt(match[2], 10);
    return { pathQuery: match[1].trim(), line: Number.isNaN(line) ? null : line };
  }
  return { pathQuery: trimmed, line: null };
}

function splitFilePath(filePath: string): { name: string; dir: string } {
  const slashIdx = filePath.lastIndexOf("/");
  if (slashIdx < 0) return { name: filePath, dir: "" };
  return {
    name: filePath.slice(slashIdx + 1) || filePath,
    dir: filePath.slice(0, slashIdx),
  };
}

function FuzzyHighlightedText({
  text,
  indices,
  markClassName,
}: {
  text: string;
  indices?: readonly number[];
  markClassName?: string;
}) {
  if (!indices || indices.length === 0) return text;
  const segments = splitHighlightSegments(text, indices);
  return (
    <>
      {segments.map((seg, i) =>
        seg.matched ? (
          <mark
            key={i}
            className={cn(
              "text-foreground rounded-[2.5px] bg-foreground/[0.08] font-semibold dark:bg-foreground/[0.15]",
              markClassName,
            )}
          >
            {seg.text}
          </mark>
        ) : (
          <span key={i}>{seg.text}</span>
        ),
      )}
    </>
  );
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
  const selectedWorkspaceCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
  const externalEditors = useOrbitSelector((s) => s.externalEditors);
  const theme = useOrbitSelector((s) => s.theme);
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const threadCustomTitles = useOrbitSelector((s) => s.threadCustomTitles);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const ime = useImeCompositionGuard();

  const activeCwd = useOrbitSelector((s) => {
    if (!s.selectedHostId) return s.selectedWorkspaceCwd ?? "";
    if (!s.isDraftNewSession && s.selectedAgentId) {
      const ag = s.hostDetails[s.selectedHostId]?.agents.find(
        (a) => a.agent_id === s.selectedAgentId,
      );
      if (ag?.cwd) return ag.cwd;
    }
    return s.selectedWorkspaceCwd ?? "";
  });

  const filesCacheKey = selectedHostId && activeCwd ? `${selectedHostId}::${activeCwd}` : "";
  const workspaceFiles = useOrbitSelector((s) =>
    filesCacheKey ? (s.workspaceFilesCache[filesCacheKey] ?? EMPTY_FILES) : EMPTY_FILES,
  );
  const gitStatusFiles = useOrbitSelector((s) =>
    filesCacheKey ? (s.gitReviews[filesCacheKey]?.status?.files ?? null) : null,
  );

  const { workspaceId: activeWorkspaceId, scripts: workspaceScripts } =
    useWorkspaceScripts(selectedHostId);

  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const [isComposing, setIsComposing] = useState(false);
  const [recents, setRecents] = useState<string[]>(() => loadPaletteRecents());
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

  const { mode, term } = useMemo(() => parsePaletteQuery(query), [query]);
  const parsedFile = useMemo(
    () => (mode === "files" ? parseFileTermAndLine(term) : { pathQuery: "", line: null }),
    [mode, term],
  );

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

  // Ensure workspace files index is populated as soon as `#` file jump mode activates
  useEffect(() => {
    if (mode !== "files" || !selectedHostId) return;
    const cwd = activeCwd || selectedWorkspaceCwd;
    if (cwd) {
      void orbitActions.ensureWorkspaceFiles(selectedHostId, cwd);
    }
  }, [mode, selectedHostId, activeCwd, selectedWorkspaceCwd]);

  // Debounced (80ms) SQLite FTS5 cross-conversation search in `all` & `@` modes
  useEffect(() => {
    if (isComposing) return;
    const reqId = searchReqIdRef.current + 1;
    searchReqIdRef.current = reqId;

    if ((mode !== "all" && mode !== "threads") || !meetsFtsQueryThreshold(term)) {
      setMessageResult({ query: "", hits: [] });
      setIsSearchingMessages(false);
      return;
    }

    let cancelled = false;
    setIsSearchingMessages(true);
    const timer = window.setTimeout(() => {
      void orbitIpc
        .searchMessages(term, FTS_RESULT_LIMIT)
        .then((hits) => {
          if (cancelled || searchReqIdRef.current !== reqId) return;
          setMessageResult({ query: term, hits });
          setIsSearchingMessages(false);
        })
        .catch(() => {
          if (cancelled || searchReqIdRef.current !== reqId) return;
          setMessageResult({ query: "", hits: [] });
          setIsSearchingMessages(false);
        });
    }, FTS_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [mode, term, isComposing]);

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
            recentId: `thread-${h.host_id}-${ag.agent_id}`,
            kind: "thread",
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
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("common.newConversation"),
        subtitle: t("palette.newConversationSub"),
        keywords: "new chat thread conversation",
        shortcut: `${mod}N`,
        icon: <Plus className="size-3.5" />,
        run: () => orbitActions.startNewThreadDraft(),
      },
      {
        id: "act-add-project",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.addProjectFolder"),
        subtitle: t("palette.addProjectFolderSub"),
        keywords: "add folder repo repository project",
        icon: <FolderPlus className="size-3.5" />,
        run: () => {
          orbitActions.setCommandPaletteOpen(false);
          void orbitActions.promptAddProject();
        },
      },
      {
        id: "act-automations",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.openAutomations"),
        subtitle: t("palette.openAutomationsSub"),
        keywords: "cron schedule workflow automation",
        icon: <Clock className="size-3.5" />,
        run: () => orbitActions.openAutomations(),
      },
      {
        id: "act-cycle-mode",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.cyclePermissions"),
        subtitle: t("palette.cyclePermissionsSub"),
        keywords: "permission mode auto approve full access",
        shortcut: "⇧Tab",
        icon: <Sparkles className="size-3.5" />,
        run: () => orbitActions.cycleActiveMode(),
      },
      {
        id: "act-toggle-fast",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleFastMode"),
        subtitle: t("palette.toggleFastModeSub"),
        keywords: "fast speed latency",
        icon: <Zap className="size-3.5" />,
        run: () => orbitActions.toggleActiveFastMode(),
      },
      ...(selectedHostId && selectedAgentId && !isDraftNewSession
        ? [
            {
              id: "act-fork-thread",
              kind: "command" as const,
              sectionKey: "palette.sectionActions" as const,
              section: actionsSection,
              title: t("palette.forkCurrent"),
              subtitle: t("palette.forkCurrentSub"),
              keywords: "fork branch clone thread",
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
              kind: "command" as const,
              sectionKey: "palette.sectionActions" as const,
              section: actionsSection,
              title: t("palette.exportConversation"),
              subtitle: t("palette.exportConversationSub"),
              keywords: "export markdown html json save transcript",
              icon: <Download className="size-3.5" />,
              run: () => orbitActions.openExportDialog(selectedHostId, selectedAgentId),
            },
            {
              id: "act-copy-code-block",
              kind: "command" as const,
              sectionKey: "palette.sectionActions" as const,
              section: actionsSection,
              title: t("palette.copyCodeBlock"),
              subtitle: t("palette.copyCodeBlockSub"),
              keywords: "copy code snippet block",
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
              kind: "command" as const,
              sectionKey: "palette.sectionActions" as const,
              section: actionsSection,
              title: t("palette.openArtifacts"),
              subtitle: t("palette.openArtifactsSub"),
              keywords: "artifacts preview html diagram mcp",
              icon: <Blocks className="size-3.5" />,
              run: () => orbitActions.openArtifactsTab({ hostId: selectedHostId }),
            },
          ]
        : []),
      {
        id: "act-toggle-overview",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleOverview"),
        keywords: "overview summary outline",
        shortcut: `${mod}⌥O`,
        icon: <LayoutList className="size-3.5" />,
        run: () => orbitActions.toggleOverviewPanel(),
      },
      {
        id: "act-toggle-side-panel",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleSidePanel"),
        keywords: "right side panel drawer",
        shortcut: `${mod}\\`,
        icon: <PanelRight className="size-3.5" />,
        run: () => orbitActions.toggleSidePanel(),
      },
      {
        id: "act-quick-open-file",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.openFileInSidePanel"),
        keywords: "quick open file jump",
        shortcut: `${mod}P`,
        icon: <Search className="size-3.5" />,
        run: () => {
          orbitActions.setCommandPaletteOpen(false);
          orbitActions.setQuickOpenFilesOpen(true);
        },
      },
      {
        id: "act-files-explorer",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.openFilesExplorer"),
        keywords: "files tree explorer sidebar",
        icon: <FolderTree className="size-3.5" />,
        run: () => orbitActions.openFilesExplorerTab(),
      },
      {
        id: "act-project-instructions",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.projectInstructions"),
        subtitle: t("palette.projectInstructionsSub"),
        keywords: "agents.md claude.md instructions rules",
        icon: <FileCode2 className="size-3.5" />,
        run: () => orbitActions.openProjectInstructions(),
      },
      {
        id: "act-toggle-diff",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleDiffReview"),
        keywords: "git diff review changes",
        shortcut: `${mod}D`,
        icon: <GitCompareArrows className="size-3.5" />,
        run: () => orbitActions.toggleReviewTab(),
      },
      {
        id: "act-toggle-term",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleTerminal"),
        keywords: "terminal pty shell console",
        shortcut: `${mod}J`,
        icon: <SquareTerminal className="size-3.5" />,
        run: () => orbitActions.toggleTerminalTab(),
      },
      {
        id: "act-open-browser",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.openBrowserPreview"),
        keywords: "browser web localhost preview",
        icon: <Globe className="size-3.5" />,
        run: () => orbitActions.openBrowserTab(),
      },
      {
        id: "act-side-question",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.askSideQuestion"),
        keywords: "btw side question ask",
        shortcut: `${mod};`,
        icon: <MessageCircleQuestion className="size-3.5" />,
        run: () => orbitActions.openSideQuestionTab(),
      },
      {
        id: "act-toggle-sidebar",
        kind: "command",
        sectionKey: "palette.sectionActions",
        section: actionsSection,
        title: t("palette.toggleSidebar"),
        keywords: "left sidebar toggle collapse",
        shortcut: `${mod}B`,
        icon: <PanelLeft className="size-3.5" />,
        run: () => orbitActions.toggleSidebar(),
      },
    );

    // 2b. Workspace Services & Scripts (only present when workspace defines orbit.json/paseo.json)
    if (selectedHostId && activeWorkspaceId && workspaceScripts.length > 0) {
      const scriptsSection = t("palette.sectionScripts");
      for (const script of workspaceScripts) {
        const isRunning = script.status === "running";
        list.push({
          id: `script-${activeWorkspaceId}-${script.name}`,
          kind: "command",
          sectionKey: "palette.sectionScripts",
          section: scriptsSection,
          title: isRunning
            ? t("palette.stopScript", { name: script.name })
            : t("palette.startScript", { name: script.name }),
          subtitle: script.proxy_url || script.command,
          keywords: `service script ${script.name} ${script.kind} localhost`,
          icon: isRunning ? (
            <Square className="size-3.5 text-emerald-500" />
          ) : (
            <Play className="size-3.5" />
          ),
          run: () => {
            const action = isRunning
              ? orbitIpc.stopWorkspaceScript(selectedHostId, activeWorkspaceId, script.name)
              : orbitIpc.startWorkspaceScript(selectedHostId, activeWorkspaceId, script.name);
            void action
              .then(() => orbitIpc.listWorkspaceScripts(selectedHostId, activeWorkspaceId))
              .catch(() => orbitActions.showToast(t("common.actionFailed")));
          },
        });
      }
    }

    // 3. External Editor Handoff
    for (const ed of externalEditors) {
      list.push({
        id: `editor-${ed.kind}`,
        kind: "command",
        sectionKey: "palette.sectionOpenIn",
        section: openInSection,
        title: t("palette.openInEditor", { editor: ed.label }),
        keywords: `open editor ${ed.label} ${ed.kind}`,
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
          kind: "thread",
          sectionKey: isArchived ? "palette.sectionArchived" : "palette.sectionSessions",
          section: isArchived ? t("palette.sectionArchived") : t("palette.sectionSessions"),
          title: effectiveTitle,
          subtitle: `${h.display_name} · ${ag.provider_label} · ${ag.cwd}`,
          keywords: ag.handle || undefined,
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
        kind: "command",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.openSettings"),
        subtitle: t("palette.openSettingsSub"),
        keywords: "preferences settings config",
        shortcut: `${mod},`,
        icon: <Settings2 className="size-3.5" />,
        run: () => orbitActions.openSettings("general"),
      },
      {
        id: "settings-setup-guide",
        kind: "command",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.setupGuide"),
        subtitle: t("palette.setupGuideSub"),
        keywords: "onboarding setup qr guide",
        icon: <Compass className="size-3.5" />,
        run: () => orbitActions.openOnboardingGuide(),
      },
      {
        id: "settings-archived",
        kind: "command",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.openArchived"),
        subtitle: t("palette.openArchivedSub"),
        keywords: "archived restore deleted threads",
        icon: <Archive className="size-3.5" />,
        run: () => orbitActions.openSettings("archived"),
      },
      {
        id: "fleet-pair",
        kind: "command",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.settingsHosts"),
        subtitle: t("palette.settingsHostsSub"),
        keywords: "hosts pair ssh remote relay",
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
              kind: "command" as const,
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsModels"),
              subtitle: t("palette.settingsModelsSub"),
              keywords: "models providers claude codex acp",
              icon: <Plug className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:agents`),
            },
            {
              id: "settings-usage",
              kind: "command" as const,
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsUsage"),
              subtitle: t("palette.settingsUsageSub"),
              keywords: "usage tokens quotas spend",
              icon: <Activity className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:usage`),
            },
            {
              id: "settings-rules",
              kind: "command" as const,
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsRules"),
              subtitle: t("palette.settingsRulesSub"),
              keywords: "rules skills instructions",
              icon: <BookOpen className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:rules`),
            },
            {
              id: "settings-mcp",
              kind: "command" as const,
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsMcp"),
              subtitle: t("palette.settingsMcpSub"),
              keywords: "mcp hooks lsp language servers",
              icon: <Webhook className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:mcp`),
            },
            {
              id: "settings-guardian",
              kind: "command" as const,
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsGuardian"),
              subtitle: t("palette.settingsGuardianSub"),
              keywords: "guardian approval risk policy",
              icon: <ShieldCheck className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:guardian`),
            },
            {
              id: "settings-im",
              kind: "command" as const,
              sectionKey: "palette.sectionSettings" as const,
              section: settingsSection,
              title: t("palette.settingsIm"),
              subtitle: t("palette.settingsImSub"),
              keywords: "im channels lark slack telegram discord",
              icon: <MessageSquare className="size-3.5" />,
              run: () => orbitActions.openSettings(`host:${effectiveSettingsHostId}:im_channels`),
            },
          ]
        : []),
      {
        id: "pref-theme-system",
        kind: "command",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.themeFollowSystem"),
        subtitle:
          theme === "system" ? t("palette.themeCurrentlyActive") : t("palette.themeFollowSystemSub"),
        keywords: "theme appearance system auto",
        icon: <Monitor className="size-3.5" />,
        run: () => orbitActions.setTheme("system"),
      },
      {
        id: "pref-theme-light",
        kind: "command",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.themeLight"),
        keywords: "theme appearance light mode",
        icon: <Sun className="size-3.5" />,
        run: () => orbitActions.setTheme("light"),
      },
      {
        id: "pref-theme-dark",
        kind: "command",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: t("palette.themeDark"),
        keywords: "theme appearance dark mode",
        icon: <Moon className="size-3.5" />,
        run: () => orbitActions.setTheme("dark"),
      },
      {
        id: "pref-enter",
        kind: "command",
        sectionKey: "palette.sectionSettings",
        section: settingsSection,
        title: enterToSend
          ? t("palette.requireModEnter", { mod })
          : t("palette.useEnterToSend"),
        keywords: "enter send prompt shortcut",
        icon: <Sparkles className="size-3.5" />,
        run: () => orbitActions.setEnterToSend(!enterToSend),
      },
    );

    if (selectedHostId) {
      list.push({
        id: "fleet-refresh",
        kind: "command",
        sectionKey: "palette.sectionFleet",
        section: fleetSection,
        title: t("palette.reconnectCurrentHost"),
        keywords: "reconnect refresh sync host",
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
    activeWorkspaceId,
    workspaceScripts,
    mod,
    t,
  ]);

  const filtered = useMemo<PaletteItem[]>(() => {
    // Prefix mode `#`: Workspace files quick-open jump
    if (mode === "files") {
      const filesSection = t("palette.sectionFiles");
      const targetHostId = selectedHostId;
      const targetCwd = activeCwd || selectedWorkspaceCwd;
      const pathQuery = parsedFile.pathQuery;
      const focusLine = parsedFile.line;

      const makeFileItem = (
        filePath: string,
        titleIndices?: number[],
        subtitleIndices?: number[],
      ): PaletteItem => {
        const FileIcon = resolveSemanticFileIcon(filePath);
        const { name, dir } = splitFilePath(filePath);
        const gitStatus = gitStatusMap.get(filePath);
        return {
          id: `file:${filePath}`,
          recentId: `file:${filePath}`,
          kind: "file",
          sectionKey: "palette.sectionFiles",
          section: filesSection,
          title: name,
          subtitle: dir || undefined,
          gitStatus,
          titleIndices,
          subtitleIndices,
          icon: <FileIcon className="size-3.5 opacity-80" />,
          run: () => {
            if (!targetHostId) return;
            orbitActions.openFileInSidePanel(filePath, {
              hostId: targetHostId,
              cwd: targetCwd,
              focusLine,
            });
          },
        };
      };

      if (!pathQuery) {
        // Prioritize recent files, then Git-modified files, then remaining workspace files
        const recentRank = new Map<string, number>();
        recents.forEach((id, idx) => {
          if (id.startsWith("file:")) recentRank.set(id.slice(5), idx);
        });
        const sorted = [...workspaceFiles].sort((a, b) => {
          const ra = recentRank.get(a);
          const rb = recentRank.get(b);
          if (ra !== undefined && rb !== undefined) return ra - rb;
          if (ra !== undefined) return -1;
          if (rb !== undefined) return 1;
          const ga = gitStatusMap.has(a) ? 0 : 1;
          const gb = gitStatusMap.has(b) ? 0 : 1;
          if (ga !== gb) return ga - gb;
          return 0;
        });
        return sorted.slice(0, MAX_FILE_RESULTS).map((p) => makeFileItem(p));
      }

      const scoredFiles: Array<{
        path: string;
        score: number;
        titleIndices?: number[];
        subtitleIndices?: number[];
      }> = [];

      for (const filePath of workspaceFiles) {
        const { name, dir } = splitFilePath(filePath);
        const baseMatch = fuzzyMatch(pathQuery, name);
        const fullMatch = fuzzyMatch(pathQuery, filePath);
        if (!baseMatch && !fullMatch) continue;

        let score = 0;
        let titleIndices: number[] | undefined;
        let subtitleIndices: number[] | undefined;

        if (baseMatch && (!fullMatch || baseMatch.score + 70 >= fullMatch.score)) {
          score = baseMatch.score + 70;
          titleIndices = baseMatch.indices;
        } else if (fullMatch) {
          score = fullMatch.score;
          if (dir) {
            const nameOffset = dir.length + 1;
            const dIdx: number[] = [];
            const nIdx: number[] = [];
            for (const idx of fullMatch.indices) {
              if (idx < dir.length) dIdx.push(idx);
              else if (idx >= nameOffset) nIdx.push(idx - nameOffset);
            }
            if (nIdx.length > 0) titleIndices = nIdx;
            if (dIdx.length > 0) subtitleIndices = dIdx;
          } else {
            titleIndices = fullMatch.indices;
          }
        }

        const gitBoost = gitStatusMap.has(filePath) ? 20 : 0;
        const recencyBoost = getPaletteRecencyBonus(`file:${filePath}`, recents, 22);
        scoredFiles.push({
          path: filePath,
          score: score + gitBoost + recencyBoost,
          titleIndices,
          subtitleIndices,
        });
      }

      scoredFiles.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path));
      return scoredFiles
        .slice(0, MAX_FILE_RESULTS)
        .map((entry) => makeFileItem(entry.path, entry.titleIndices, entry.subtitleIndices));
    }

    // Scope local items according to prefix mode (`>` commands only, `@` threads only, `all`)
    const scopedItems =
      mode === "commands"
        ? allItems.filter((it) => it.kind === "command")
        : mode === "threads"
          ? allItems.filter((it) => it.kind === "thread")
          : allItems;

    // Empty search term: preserve clean grouped sections, surfacing LRU Recents cleanly at top
    if (!term) {
      if (recents.length === 0) return scopedItems;

      const byRecentKey = new Map<string, PaletteItem>();
      for (const item of scopedItems) {
        const key = item.recentId ?? item.id;
        // Prefer Needs Approval or primary entry without overwriting an already-seen attention item
        if (!byRecentKey.has(key)) {
          byRecentKey.set(key, item);
        }
      }

      const approvalItems = scopedItems.filter(
        (it) => it.sectionKey === "palette.sectionApproval",
      );
      const approvalKeys = new Set(approvalItems.map((it) => it.recentId ?? it.id));

      const recentSectionItems: PaletteItem[] = [];
      const recentUsedIds = new Set<string>();
      for (const recId of recents) {
        if (approvalKeys.has(recId)) continue;
        const found = byRecentKey.get(recId);
        if (!found) continue;
        recentUsedIds.add(found.id);
        recentSectionItems.push({
          ...found,
          sectionKey: "palette.sectionRecent",
          section: t("palette.sectionRecent"),
        });
        if (recentSectionItems.length >= MAX_RECENT_SECTION_ITEMS) break;
      }

      if (recentSectionItems.length === 0) return scopedItems;

      const rest = scopedItems.filter(
        (it) =>
          it.sectionKey !== "palette.sectionApproval" && !recentUsedIds.has(it.id),
      );
      return [...approvalItems, ...recentSectionItems, ...rest];
    }

    // Non-empty query: score every candidate with fuzzyMatch + subtle LRU recency boost
    const activeMessageHits =
      (mode === "all" || mode === "threads") && messageResult.query === term
        ? messageResult.hits
        : [];
    const hitByThread = new Map<string, ConversationSearchHit>();
    for (const hit of activeMessageHits) {
      const key = threadScopeKey(hit.host_id, hit.agent_id);
      if (!hitByThread.has(key)) {
        hitByThread.set(key, hit);
      }
    }

    const scoredLocal: Array<{ item: PaletteItem; score: number; order: number }> = [];
    scopedItems.forEach((item, order) => {
      const titleMatch = fuzzyMatch(term, item.title);
      const subtitleMatch = item.subtitle ? fuzzyMatch(term, item.subtitle) : null;
      const sectionMatch = fuzzyMatch(term, item.section);
      const keywordMatch = item.keywords ? fuzzyMatch(term, item.keywords) : null;

      if (!titleMatch && !subtitleMatch && !sectionMatch && !keywordMatch) {
        return;
      }

      let bestScore = -1_000_000;
      if (titleMatch) {
        bestScore = Math.max(bestScore, Math.round(titleMatch.score * 1.15) + 45);
      }
      if (subtitleMatch) {
        bestScore = Math.max(bestScore, subtitleMatch.score);
      }
      if (keywordMatch) {
        bestScore = Math.max(bestScore, Math.round(keywordMatch.score * 0.85));
      }
      if (sectionMatch) {
        bestScore = Math.max(bestScore, Math.round(sectionMatch.score * 0.55));
      }
      if (titleMatch && subtitleMatch) {
        bestScore += 18;
      }

      const recencyBoost = getPaletteRecencyBonus(item.recentId ?? item.id, recents);
      scoredLocal.push({
        item: {
          ...item,
          titleIndices: titleMatch?.indices,
          subtitleIndices: subtitleMatch?.indices,
        },
        score: bestScore + recencyBoost,
        order,
      });
    });

    scoredLocal.sort((a, b) => b.score - a.score || a.order - b.order);

    // Enrich already-listed session hits with matched FTS snippet & jump-to-seq,
    // and deduplicate thread keys so they don't repeat under Messages.
    const seenThreads = new Set<string>();
    const enrichedLocal = scoredLocal.map(({ item }) => {
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
      const title = customTitle || hit.title || t("common.conversation");
      const titleMatch = fuzzyMatch(term, title);
      extraMessageItems.push({
        id: `msg-${hit.host_id}-${hit.agent_id}-${hit.seq}`,
        recentId: `thread-${hit.host_id}-${hit.agent_id}`,
        kind: "thread",
        sectionKey: "palette.sectionMessages",
        section: t("palette.sectionMessages"),
        title,
        titleIndices: titleMatch?.indices,
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

    return [...enrichedLocal, ...extraMessageItems];
  }, [
    mode,
    term,
    parsedFile.pathQuery,
    parsedFile.line,
    workspaceFiles,
    gitStatusMap,
    selectedHostId,
    activeCwd,
    selectedWorkspaceCwd,
    allItems,
    recents,
    messageResult,
    threadCustomTitles,
    isMultiHost,
    t,
  ]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  useEffect(() => {
    setActiveIndex((prev) => {
      if (filtered.length === 0) return 0;
      return prev >= filtered.length ? filtered.length - 1 : prev;
    });
  }, [filtered.length]);

  const applyModePrefix = (targetMode: Exclude<PaletteMode, "all">) => {
    const prefixMap: Record<Exclude<PaletteMode, "all">, string> = {
      commands: ">",
      threads: "@",
      files: "#",
    };
    if (mode === targetMode) {
      setQuery(term);
    } else {
      const nextPrefix = prefixMap[targetMode];
      setQuery(term ? `${nextPrefix}${term}` : nextPrefix);
    }
    window.requestAnimationFrame(() => {
      const el = inputRef.current;
      if (el) {
        el.focus();
        const len = el.value.length;
        el.setSelectionRange(len, len);
      }
    });
  };

  const clearModePrefix = () => {
    setQuery(term);
    window.requestAnimationFrame(() => inputRef.current?.focus());
  };

  const executeItem = (item: PaletteItem | undefined) => {
    if (!item) return;
    const nextRecents = recordPaletteRecent(item.recentId ?? item.id);
    setRecents(nextRecents);
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

  const activePlaceholder =
    mode === "commands"
      ? t("palette.placeholderCommands")
      : mode === "threads"
        ? t("palette.placeholderThreads")
        : mode === "files"
          ? t("palette.placeholderFiles")
          : t("palette.placeholder");

  const emptyMessage =
    mode === "files"
      ? workspaceFiles.length === 0
        ? t("palette.indexingFiles")
        : t("palette.noMatchingFiles")
      : mode === "commands"
        ? t("palette.noMatchingCommands")
        : mode === "threads"
          ? isSearchingMessages
            ? t("palette.searchingMessages")
            : t("palette.noMatchingThreads")
          : isSearchingMessages
            ? t("palette.searchingMessages")
            : t("palette.noMatches");

  const showGroupedHeaders = !term || mode === "files";

  return (
    <>
      <DialogTitle className="sr-only">{t("palette.title")}</DialogTitle>
      <div className="border-border flex h-[46px] items-center gap-2 border-b px-4">
        <Search className="text-muted-foreground h-[15.5px] w-[15.5px] shrink-0" />
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
          placeholder={activePlaceholder}
          className="text-foreground placeholder:text-muted-foreground min-w-0 flex-1 bg-transparent text-[14px] outline-none"
        />

        {mode !== "all" && (
          <button
            type="button"
            onClick={clearModePrefix}
            title={t("palette.clearMode")}
            className="text-foreground/80 bg-foreground/[0.06] hover:bg-foreground/[0.1] dark:bg-foreground/[0.1] dark:hover:bg-foreground/[0.16] inline-flex h-[22px] shrink-0 cursor-pointer items-center gap-1 rounded-[6px] px-1.5 text-[11px] font-medium transition-colors"
          >
            {mode === "commands" ? (
              <Terminal className="h-[11px] w-[11px] opacity-75" />
            ) : mode === "threads" ? (
              <AtSign className="h-[11px] w-[11px] opacity-75" />
            ) : (
              <Hash className="h-[11px] w-[11px] opacity-75" />
            )}
            <span>
              {mode === "commands"
                ? t("palette.modeCommands")
                : mode === "threads"
                  ? t("palette.modeThreads")
                  : t("palette.modeFiles")}
            </span>
            <X className="h-[10px] w-[10px] opacity-60" />
          </button>
        )}

        {mode === "files" && parsedFile.line != null && (
          <span className="bg-primary/12 text-primary shrink-0 rounded px-1.5 py-0.5 font-mono text-[11px] font-medium">
            L{parsedFile.line}
          </span>
        )}

        <Kbd>Esc</Kbd>
      </div>

      <ScrollArea viewportClassName="max-h-96 p-1.5">
        {filtered.length === 0 ? (
          <div className="text-muted-foreground py-8 text-center text-[13px]">{emptyMessage}</div>
        ) : (
          filtered.map((item, idx) => {
            const prevItem = idx > 0 ? filtered[idx - 1] : null;
            const isMessageItem = item.sectionKey === "palette.sectionMessages";
            const prevIsMessage = prevItem?.sectionKey === "palette.sectionMessages";
            const showSection = showGroupedHeaders
              ? item.section !== prevItem?.section
              : isMessageItem && !prevIsMessage;
            const selected = idx === activeIndex;
            const rolePrefix = formatRolePrefix(item.snippetRole, t);

            return (
              <div key={`${item.sectionKey}:${item.id}`}>
                {showSection && (
                  <div className="text-muted-foreground px-3 pt-2.5 pb-1 text-[11.5px] font-medium">
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
                          <FuzzyHighlightedText text={item.title} indices={item.titleIndices} />
                        </span>
                        {item.metaBadge && (
                          <span className="text-muted-foreground/80 bg-foreground/[0.045] dark:bg-foreground/[0.07] max-w-[160px] shrink-0 truncate rounded px-1.5 py-0.5 text-[11px] leading-none font-normal">
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
                          <div
                            className={cn(
                              "text-muted-foreground truncate text-[12px]",
                              item.kind === "file" && "font-mono text-[11.5px]",
                            )}
                          >
                            <FuzzyHighlightedText
                              text={item.subtitle}
                              indices={item.subtitleIndices}
                              markClassName="font-medium"
                            />
                          </div>
                        )
                      )}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    {!showGroupedHeaders && !isMessageItem && !item.shortcut && !item.relativeTime && !item.gitStatus && (
                      <span className="text-muted-foreground/55 text-[11px]">{item.section}</span>
                    )}
                    {item.gitStatus && (
                      <span
                        className={cn(
                          "font-mono text-[10.5px] font-medium",
                          item.gitStatus === "A" || item.gitStatus === "??"
                            ? "text-emerald-600 dark:text-emerald-400"
                            : item.gitStatus === "D"
                              ? "text-rose-600 dark:text-rose-400"
                              : "text-amber-600 dark:text-amber-400",
                        )}
                      >
                        {item.gitStatus}
                      </span>
                    )}
                    {item.shortcut ? (
                      <ShortcutKbd keys={item.shortcut} className="shrink-0" />
                    ) : (
                      item.relativeTime && (
                        <span className="text-muted-foreground/70 shrink-0 text-[11px] tabular-nums">
                          {item.relativeTime}
                        </span>
                      )
                    )}
                  </div>
                </button>
              </div>
            );
          })
        )}
      </ScrollArea>

      {/* Restrained Raycast/Linear-grade Footer with Prefix Modes & Keyboard Hints */}
      <div className="border-border/80 flex h-[32px] items-center justify-between gap-2 border-t px-3 text-[11px] select-none">
        <div className="flex items-center gap-1">
          {(
            [
              { key: "commands", prefix: ">", label: t("palette.modeCommands") },
              { key: "threads", prefix: "@", label: t("palette.modeThreads") },
              { key: "files", prefix: "#", label: t("palette.modeFiles") },
            ] as const
          ).map((pill) => {
            const active = mode === pill.key;
            return (
              <button
                key={pill.key}
                type="button"
                onClick={() => applyModePrefix(pill.key)}
                className={cn(
                  "inline-flex h-[20px] cursor-pointer items-center gap-1 rounded-[5px] px-1.5 transition-colors",
                  active
                    ? "bg-foreground/[0.08] text-foreground font-medium dark:bg-foreground/[0.14]"
                    : "text-muted-foreground/75 hover:text-foreground hover:bg-foreground/[0.045] dark:hover:bg-foreground/[0.07]",
                )}
              >
                <span className="font-mono text-[10.5px] opacity-80">{pill.prefix}</span>
                <span>{pill.label}</span>
              </button>
            );
          })}
        </div>

        <div className="text-ink-muted hidden items-center gap-2.5 sm:flex">
          <span className="inline-flex items-center gap-1">
            <Kbd size="xs">↑↓</Kbd>
            <span>{t("palette.hintNavigate")}</span>
          </span>
          <span className="inline-flex items-center gap-1">
            <Kbd size="xs">↵</Kbd>
            <span>{t("palette.hintOpen")}</span>
          </span>
        </div>
      </div>
    </>
  );
}

function buildMetaBadge(hit: ConversationSearchHit, isMultiHost: boolean): string | undefined {
  const parts: string[] = [];
  if (hit.project_name) parts.push(hit.project_name);
  if (isMultiHost && hit.host_name) parts.push(hit.host_name);
  return parts.length > 0 ? parts.join(" · ") : undefined;
}
