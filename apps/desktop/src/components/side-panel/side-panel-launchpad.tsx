import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import {
  AlertCircle,
  Blocks,
  CheckCircle2,
  ChevronRight,
  Clock,
  CornerDownLeft,
  File,
  FileDiff,
  Globe,
  Loader2,
  MessageCircleQuestion,
  Search,
  SquareTerminal,
  X,
} from "lucide-react";

import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { useImeCompositionGuard } from "@/lib/ime";
import { resolveSessionSubagents } from "@/lib/subagents";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { orbitActions, sideQuestionScopeKey, useOrbitSelector } from "@/state/orbit-store";
import type { DiffFileView, SessionInsightsView } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

const EMPTY_INSIGHTS: SessionInsightsView = {
  touched_files: [],
  background_tasks: [],
  subagents: [],
  detected_local_urls: [],
};
const EMPTY_DIFFS: readonly DiffFileView[] = [];
const EMPTY_FILES: string[] = [];

const WEB_TLD_URL_RE =
  /^(?:[a-z0-9-]+\.)+(?:com|org|net|io|dev|ai|app|cn|co|edu|gov|me|xyz|gg|tv|so|sh|cloud|site)(?::\d+)?(?:\/.*)?$/i;
const LOCALHOST_URL_RE = /^localhost(?::\d+)?(?:\/.*)?$/i;
const IPV4_URL_RE = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?::\d+)?(?:\/.*)?$/;

function detectLaunchpadUrl(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed || /\s/.test(trimmed) || trimmed.startsWith("/") || trimmed.startsWith("?")) {
    return null;
  }

  if (/^https?:\/\//i.test(trimmed)) {
    try {
      const parsed = new URL(trimmed);
      if (parsed.protocol === "http:" || parsed.protocol === "https:") {
        return parsed.toString();
      }
    } catch {
      return null;
    }
    return null;
  }

  if (LOCALHOST_URL_RE.test(trimmed)) {
    try {
      return new URL(`http://${trimmed}`).toString();
    } catch {
      return null;
    }
  }

  const ipv4Match = IPV4_URL_RE.exec(trimmed);
  if (ipv4Match) {
    const validOctets = ipv4Match
      .slice(1, 5)
      .every((octet) => Number(octet) >= 0 && Number(octet) <= 255);
    if (!validOctets) return null;
    try {
      return new URL(`http://${trimmed}`).toString();
    } catch {
      return null;
    }
  }

  if (WEB_TLD_URL_RE.test(trimmed)) {
    try {
      return new URL(`https://${trimmed}`).toString();
    } catch {
      return null;
    }
  }

  return null;
}

function parseSideQuestionQuery(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const btwMatch = /^\/btw\s+(.+)$/i.exec(trimmed);
  if (btwMatch?.[1]?.trim()) {
    return btwMatch[1].trim();
  }
  const qMatch = /^\?\s*(.+)$/.exec(trimmed);
  if (qMatch?.[1]?.trim()) {
    return qMatch[1].trim();
  }
  return null;
}

function scoreLaunchpadPathMatch(path: string, query: string): number {
  const q = query.trim().toLowerCase();
  if (!q) return 1;
  const lower = path.toLowerCase();
  const base = (lower.split("/").pop() ?? lower).trim();
  if (base === q) return 1000;
  if (base.startsWith(q)) return 800 - (base.length - q.length);
  const baseIdx = base.indexOf(q);
  if (baseIdx >= 0) return 600 - baseIdx;
  const pathIdx = lower.indexOf(q);
  if (pathIdx >= 0) return 400 - pathIdx;

  let qi = 0;
  let score = 100;
  for (let i = 0; i < lower.length && qi < q.length; i += 1) {
    if (lower[i] === q[qi]) {
      qi += 1;
      score += 5;
    }
  }
  return qi === q.length ? score : -1;
}

function splitDisplayPath(filePath: string): { name: string; dir: string } {
  const normalized = filePath.replace(/\/+$/, "");
  const slashIdx = normalized.lastIndexOf("/");
  if (slashIdx < 0) return { name: normalized, dir: "" };
  return {
    name: normalized.slice(slashIdx + 1) || normalized,
    dir: normalized.slice(0, slashIdx),
  };
}

function formatCompactHostLabel(rawUrl: string): string {
  try {
    const parsed = new URL(rawUrl);
    return parsed.host;
  } catch {
    return rawUrl.replace(/^https?:\/\//i, "");
  }
}

interface LaunchpadPrimaryAction {
  id: "review" | "files" | "artifacts" | "terminal" | "browser" | "side_question";
  label: string;
  keywords: string[];
  icon: ReactNode;
  trailing?: ReactNode;
  onSelect: () => void;
}

type LaunchpadSearchEntry =
  | {
      key: string;
      kind: "side_question_ask";
      question: string;
      onSelect: () => void;
    }
  | {
      key: string;
      kind: "url";
      url: string;
      onSelect: () => void;
    }
  | {
      key: string;
      kind: "action";
      action: LaunchpadPrimaryAction;
    }
  | {
      key: string;
      kind: "file";
      filePath: string;
      onSelect: () => void;
    };

export function SidePanelLaunchpad({
  selectedHostId,
  isPanelOpen,
  isPanelSettled,
}: {
  selectedHostId: string | null;
  isPanelOpen: boolean;
  isPanelSettled: boolean;
}) {
  const [query, setQuery] = useState("");
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const [showAllTasks, setShowAllTasks] = useState(false);
  const [expandedTasks, setExpandedTasks] = useState<Record<string, boolean>>({});
  const inputRef = useRef<HTMLInputElement | null>(null);
  const ime = useImeCompositionGuard();

  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedWorkspaceCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const fleetHosts = useOrbitSelector((s) => s.fleet.hosts);
  const stages = useOrbitSelector((s) => s.stages);
  const gitReviews = useOrbitSelector((s) => s.gitReviews);
  const workspaceFilesCache = useOrbitSelector((s) => s.workspaceFilesCache);
  const terminalTabs = useOrbitSelector((s) => s.terminalTabs);
  const sideQuestionsByScope = useOrbitSelector((s) => s.sideQuestionsByScope);
  const focusNonce = useOrbitSelector((s) => s.sidePanelLaunchpadFocusNonce);

  const stageKey =
    selectedHostId && selectedAgentId && !isDraftNewSession
      ? `${selectedHostId}::${selectedAgentId}`
      : null;
  const stage = stageKey ? (stages[stageKey] ?? null) : null;

  const hostDetail = selectedHostId ? (hostDetails[selectedHostId] ?? null) : null;
  const currentAgent = useMemo(() => {
    if (isDraftNewSession || !selectedHostId || !selectedAgentId) return null;
    return (
      hostDetail?.agents.find((a) => a.agent_id === selectedAgentId) ??
      stage?.agent ??
      null
    );
  }, [hostDetail, isDraftNewSession, selectedAgentId, selectedHostId, stage?.agent]);

  const cwd = currentAgent?.cwd || selectedWorkspaceCwd || "~";
  const insights = stage?.insights ?? EMPTY_INSIGHTS;
  const backgroundTasks = insights.background_tasks;
  const resolvedSubagents = useMemo(
    () =>
      resolveSessionSubagents({
        hostId: selectedHostId,
        agentId: isDraftNewSession ? null : selectedAgentId,
        insightsSubagents: insights.subagents,
        hostDetails,
        fleetHosts,
      }),
    [selectedHostId, isDraftNewSession, selectedAgentId, insights.subagents, hostDetails, fleetHosts],
  );

  const sessionTurnDiffs = stage?.session_turn_diffs ?? EMPTY_DIFFS;
  const gitReview =
    selectedHostId && cwd && cwd !== "~" ? (gitReviews[`${selectedHostId}::${cwd}`] ?? null) : null;

  const reviewSummary = useMemo(() => {
    if (gitReview?.diff_files && gitReview.diff_files.length > 0) {
      const additions = gitReview.diff_files.reduce((acc, f) => acc + f.additions, 0);
      const deletions = gitReview.diff_files.reduce((acc, f) => acc + f.deletions, 0);
      return { additions, deletions, count: gitReview.diff_files.length };
    }
    if (gitReview?.status?.files && gitReview.status.files.length > 0) {
      return { additions: 0, deletions: 0, count: gitReview.status.files.length };
    }
    if (sessionTurnDiffs.length > 0) {
      const additions = sessionTurnDiffs.reduce((acc, f) => acc + f.additions, 0);
      const deletions = sessionTurnDiffs.reduce((acc, f) => acc + f.deletions, 0);
      return { additions, deletions, count: sessionTurnDiffs.length };
    }
    return { additions: 0, deletions: 0, count: 0 };
  }, [gitReview, sessionTurnDiffs]);

  const totalTerminalCount = useMemo(
    () => (selectedHostId ? terminalTabs.filter((t) => t.hostId === selectedHostId).length : 0),
    [selectedHostId, terminalTabs],
  );

  const sideQuestionCount = useMemo(() => {
    const scope = sideQuestionScopeKey({ selectedHostId, selectedAgentId, isDraftNewSession });
    return (sideQuestionsByScope[scope] ?? []).length;
  }, [isDraftNewSession, selectedAgentId, selectedHostId, sideQuestionsByScope]);

  const artifactCount = useMemo(() => {
    const all = hostDetail?.artifacts ?? [];
    if (all.length === 0) return 0;
    if (currentAgent?.workspace_id) {
      const wsCount = all.filter((a) => a.workspace_id === currentAgent.workspace_id).length;
      if (wsCount > 0) return wsCount;
    }
    return all.length;
  }, [hostDetail?.artifacts, currentAgent?.workspace_id]);

  const primaryLocalUrl = insights.detected_local_urls[0] ?? null;

  const primaryActions = useMemo<LaunchpadPrimaryAction[]>(
    () => [
      {
        id: "review",
        label: "Review",
        keywords: ["review", "diff", "git", "changes"],
        icon: <FileDiff className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />,
        trailing:
          reviewSummary.count > 0 ? (
            reviewSummary.additions > 0 || reviewSummary.deletions > 0 ? (
              <span className="font-mono text-[11.5px] tabular-nums">
                <span className="text-emerald-600 dark:text-emerald-400">
                  +{reviewSummary.additions}
                </span>{" "}
                <span className="text-red-500 dark:text-red-400">
                  -{reviewSummary.deletions}
                </span>
              </span>
            ) : (
              <span className="text-muted-foreground/75 text-[11.5px] tabular-nums">
                {reviewSummary.count} {reviewSummary.count === 1 ? "file" : "files"}
              </span>
            )
          ) : null,
        onSelect: () => orbitActions.openReviewTab(),
      },
      {
        id: "files",
        label: "Open File",
        keywords: ["open file", "files", "explorer", "tree", "folder"],
        icon: <File className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />,
        onSelect: () => orbitActions.openFilesExplorerTab(),
      },
      {
        id: "artifacts",
        label: "Artifacts",
        keywords: ["artifacts", "html", "mcp", "app", "preview", "diagram", "svg", "mermaid"],
        icon: <Blocks className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />,
        trailing:
          artifactCount > 0 ? (
            <Badge
              variant="secondary"
              className="text-muted-foreground h-4 min-w-[18px] justify-center rounded-full px-1.5 text-[10.5px] font-medium tabular-nums"
            >
              {artifactCount}
            </Badge>
          ) : null,
        onSelect: () =>
          orbitActions.openArtifactsTab({
            hostId: selectedHostId ?? undefined,
            workspaceId: currentAgent?.workspace_id,
          }),
      },
      {
        id: "terminal",
        label: "Terminal",
        keywords: ["terminal", "shell", "zsh", "bash", "console", "pty"],
        icon: (
          <SquareTerminal className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />
        ),
        trailing:
          totalTerminalCount > 0 ? (
            <Badge
              variant="secondary"
              className="text-muted-foreground h-4 min-w-[18px] justify-center rounded-full px-1.5 text-[10.5px] font-medium tabular-nums"
            >
              {totalTerminalCount}
            </Badge>
          ) : null,
        onSelect: () => orbitActions.openTerminalTab(),
      },
      {
        id: "browser",
        label: "Browser",
        keywords: ["browser", "web", "url", "preview", "http", "localhost"],
        icon: <Globe className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />,
        trailing: primaryLocalUrl ? (
          <Badge
            variant="secondary"
            onClick={(e) => {
              e.stopPropagation();
              orbitActions.openBrowserTab(primaryLocalUrl);
            }}
            className="text-muted-foreground hover:text-foreground h-4 max-w-[140px] gap-1 rounded-full px-1.5 text-[10.5px] font-normal transition-colors"
          >
            <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-emerald-500" />
            <span className="truncate">{formatCompactHostLabel(primaryLocalUrl)}</span>
          </Badge>
        ) : null,
        onSelect: () => orbitActions.openBrowserTab(),
      },
      {
        id: "side_question",
        label: "Side Question",
        keywords: ["side question", "btw", "ask", "question", "chat"],
        icon: (
          <MessageCircleQuestion className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />
        ),
        trailing:
          sideQuestionCount > 0 ? (
            <Badge
              variant="secondary"
              className="text-muted-foreground h-4 min-w-[18px] justify-center rounded-full px-1.5 text-[10.5px] font-medium tabular-nums"
            >
              {sideQuestionCount}
            </Badge>
          ) : null,
        onSelect: () => orbitActions.openSideQuestionTab(),
      },
    ],
    [
      artifactCount,
      currentAgent?.workspace_id,
      primaryLocalUrl,
      reviewSummary,
      selectedHostId,
      sideQuestionCount,
      totalTerminalCount,
    ],
  );

  const trimmedQuery = query.trim();
  const isSearching = trimmedQuery.length > 0;
  const sideQuestionText = useMemo(() => parseSideQuestionQuery(query), [query]);
  const detectedUrl = useMemo(() => detectLaunchpadUrl(query), [query]);

  const workspaceFiles =
    selectedHostId && cwd ? (workspaceFilesCache[`${selectedHostId}::${cwd}`] ?? EMPTY_FILES) : EMPTY_FILES;

  const matchedFiles = useMemo(() => {
    if (!isSearching) return EMPTY_FILES;
    return workspaceFiles
      .map((path) => ({ path, score: scoreLaunchpadPathMatch(path, trimmedQuery) }))
      .filter((entry) => entry.score >= 0)
      .sort((a, b) => b.score - a.score || a.path.localeCompare(b.path))
      .slice(0, 18)
      .map((entry) => entry.path);
  }, [isSearching, trimmedQuery, workspaceFiles]);

  const searchEntries = useMemo<LaunchpadSearchEntry[]>(() => {
    if (!isSearching) return [];
    const entries: LaunchpadSearchEntry[] = [];

    if (sideQuestionText) {
      entries.push({
        key: `side-q:${sideQuestionText}`,
        kind: "side_question_ask",
        question: sideQuestionText,
        onSelect: () => {
          orbitActions.openSideQuestionTab();
          void orbitActions.submitSideQuestion(sideQuestionText);
        },
      });
    }

    if (detectedUrl) {
      entries.push({
        key: `url:${detectedUrl}`,
        kind: "url",
        url: detectedUrl,
        onSelect: () => orbitActions.openBrowserTab(detectedUrl),
      });
    }

    const qLower = trimmedQuery.toLowerCase();
    for (const action of primaryActions) {
      const matchesAction =
        action.label.toLowerCase().includes(qLower) ||
        action.keywords.some((kw) => kw.includes(qLower) || qLower.includes(kw));
      if (matchesAction) {
        entries.push({
          key: `action:${action.id}`,
          kind: "action",
          action,
        });
      }
    }

    for (const filePath of matchedFiles) {
      entries.push({
        key: `file:${filePath}`,
        kind: "file",
        filePath,
        onSelect: () => orbitActions.openFileInSidePanel(filePath),
      });
    }

    return entries;
  }, [detectedUrl, isSearching, matchedFiles, primaryActions, sideQuestionText, trimmedQuery]);

  // Reset keyboard selection cleanly when switching between empty and query modes
  useEffect(() => {
    setHighlightedIndex(isSearching && searchEntries.length > 0 ? 0 : -1);
  }, [isSearching, trimmedQuery, searchEntries.length]);

  // Gentle auto-focus only after the panel slide animation has settled
  useEffect(() => {
    if (!isPanelOpen || !isPanelSettled) return;
    const timer = window.setTimeout(() => {
      inputRef.current?.focus({ preventScroll: true });
    }, 24);
    return () => window.clearTimeout(timer);
  }, [isPanelOpen, isPanelSettled, focusNonce]);

  const handleInputKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (ime.shouldIgnoreKeyDown(e)) return;
    if (isSearching) {
      if (e.key === "ArrowDown") {
        if (searchEntries.length === 0) return;
        e.preventDefault();
        setHighlightedIndex((prev) => (prev + 1) % searchEntries.length);
        return;
      }
      if (e.key === "ArrowUp") {
        if (searchEntries.length === 0) return;
        e.preventDefault();
        setHighlightedIndex((prev) => (prev <= 0 ? searchEntries.length - 1 : prev - 1));
        return;
      }
      if (e.key === "Enter") {
        const idx = highlightedIndex >= 0 && highlightedIndex < searchEntries.length ? highlightedIndex : 0;
        const entry = searchEntries[idx];
        if (!entry) return;
        e.preventDefault();
        if (entry.kind === "action") {
          entry.action.onSelect();
        } else {
          entry.onSelect();
        }
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setQuery("");
        return;
      }
      return;
    }

    if (e.key === "ArrowDown") {
      e.preventDefault();
      setHighlightedIndex((prev) => (prev < 0 ? 0 : (prev + 1) % primaryActions.length));
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setHighlightedIndex((prev) => (prev <= 0 ? primaryActions.length - 1 : prev - 1));
      return;
    }
    if (e.key === "Enter" && highlightedIndex >= 0 && highlightedIndex < primaryActions.length) {
      e.preventDefault();
      primaryActions[highlightedIndex].onSelect();
    }
  };

  const visibleBackgroundTasks = showAllTasks
    ? backgroundTasks
    : backgroundTasks.slice(0, 4);
  const recentFiles = insights.touched_files.slice(0, 5);

  return (
    <ScrollArea className="h-full w-full">
      <div className="mx-auto flex w-full max-w-[680px] flex-col px-5 pt-5 pb-8">
        {/* Top Search / URL Pill Input */}
        <div
          className={cn(
            "bg-background relative flex h-[38px] w-full items-center gap-2.5 rounded-full border border-black/[0.1] px-3.5 shadow-[0_1px_2px_rgba(0,0,0,0.03)] transition-colors",
            "focus-within:border-primary/50 focus-within:ring-primary/15 focus-within:ring-[3px]",
            "dark:border-white/[0.12] dark:bg-white/[0.03] dark:focus-within:border-primary/50 dark:focus-within:ring-primary/20",
          )}
        >
          {sideQuestionText ? (
            <MessageCircleQuestion className="text-primary h-[14.5px] w-[14.5px] shrink-0 stroke-[1.8]" />
          ) : detectedUrl ? (
            <Globe className="text-primary h-[14.5px] w-[14.5px] shrink-0 stroke-[1.8]" />
          ) : (
            <Search className="text-muted-foreground/65 h-[14.5px] w-[14.5px] shrink-0 stroke-[1.8]" />
          )}
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            {...ime.compositionProps}
            onKeyDown={handleInputKeyDown}
            placeholder="Search files or enter a URL"
            aria-label="Search files or enter a URL"
            className="text-foreground placeholder:text-muted-foreground/60 min-w-0 flex-1 bg-transparent text-[13px] select-text focus:outline-none"
          />
          {query.length > 0 && (
            <button
              type="button"
              aria-label="Clear search"
              onClick={() => {
                setQuery("");
                inputRef.current?.focus({ preventScroll: true });
              }}
              className="text-muted-foreground/65 hover:text-foreground inline-flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors hover:bg-black/[0.05] dark:hover:bg-white/[0.08]"
            >
              <X className="h-[12px] w-[12px]" />
            </button>
          )}
        </div>

        {/* Mode A: Live Query Results */}
        {isSearching ? (
          <div className="mt-4 flex flex-col gap-0.5">
            {searchEntries.length > 0 ? (
              searchEntries.map((entry, idx) => {
                const isSelected = idx === highlightedIndex;
                const rowClass = cn(
                  "flex h-[34px] w-full cursor-pointer items-center justify-between gap-2.5 rounded-[8px] px-2.5 text-left text-[13px] transition-colors",
                  isSelected
                    ? "bg-black/[0.055] text-foreground dark:bg-white/[0.08]"
                    : "text-foreground/85 hover:bg-black/[0.04] dark:hover:bg-white/[0.05]",
                );

                if (entry.kind === "side_question_ask") {
                  return (
                    <button
                      key={entry.key}
                      type="button"
                      onMouseEnter={() => setHighlightedIndex(idx)}
                      onClick={entry.onSelect}
                      className={rowClass}
                    >
                      <span className="flex min-w-0 items-center gap-2.5">
                        <MessageCircleQuestion className="text-primary h-[15px] w-[15px] shrink-0 stroke-[1.75]" />
                        <span className="truncate font-medium">
                          Ask Side Question: &ldquo;{entry.question}&rdquo;
                        </span>
                      </span>
                      <span className="text-muted-foreground/65 flex shrink-0 items-center gap-1 text-[11px]">
                        <span>Ask</span>
                        {isSelected && <CornerDownLeft className="h-[11px] w-[11px]" />}
                      </span>
                    </button>
                  );
                }

                if (entry.kind === "url") {
                  return (
                    <button
                      key={entry.key}
                      type="button"
                      onMouseEnter={() => setHighlightedIndex(idx)}
                      onClick={entry.onSelect}
                      className={rowClass}
                    >
                      <span className="flex min-w-0 items-center gap-2.5">
                        <Globe className="text-primary h-[15px] w-[15px] shrink-0 stroke-[1.75]" />
                        <span className="shrink-0 font-medium">Open in Browser</span>
                        <span className="text-muted-foreground/70 truncate text-[12px]">
                          {entry.url}
                        </span>
                      </span>
                      {isSelected && (
                        <CornerDownLeft className="text-muted-foreground/65 h-[12px] w-[12px] shrink-0" />
                      )}
                    </button>
                  );
                }

                if (entry.kind === "action") {
                  return (
                    <button
                      key={entry.key}
                      type="button"
                      onMouseEnter={() => setHighlightedIndex(idx)}
                      onClick={entry.action.onSelect}
                      className={rowClass}
                    >
                      <span className="flex min-w-0 items-center gap-2.5">
                        {entry.action.icon}
                        <span className="truncate">{entry.action.label}</span>
                      </span>
                      <span className="flex shrink-0 items-center gap-2">
                        {entry.action.trailing}
                        {isSelected && (
                          <CornerDownLeft className="text-muted-foreground/65 h-[12px] w-[12px]" />
                        )}
                      </span>
                    </button>
                  );
                }

                const FileIcon = resolveSemanticFileIcon(entry.filePath);
                const { name, dir } = splitDisplayPath(entry.filePath);
                return (
                  <button
                    key={entry.key}
                    type="button"
                    onMouseEnter={() => setHighlightedIndex(idx)}
                    onClick={entry.onSelect}
                    className={rowClass}
                  >
                    <span className="flex min-w-0 flex-1 items-center gap-2.5">
                      <FileIcon className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />
                      <span className="shrink-0 font-medium">{name}</span>
                      {dir && (
                        <span className="text-muted-foreground/65 truncate text-[12px]">{dir}</span>
                      )}
                    </span>
                    {isSelected && (
                      <CornerDownLeft className="text-muted-foreground/65 h-[12px] w-[12px] shrink-0" />
                    )}
                  </button>
                );
              })
            ) : (
              <div className="text-muted-foreground/75 px-2.5 py-6 text-center text-[12.5px]">
                No matching files, URLs, or actions for &ldquo;{trimmedQuery}&rdquo;
              </div>
            )}
          </div>
        ) : (
          /* Mode B: Default Launchpad Actions & Contextual Session Insights */
          <>
            <div className="mt-4 flex flex-col gap-0.5">
              {primaryActions.map((action, idx) => {
                const isSelected = idx === highlightedIndex;
                return (
                  <button
                    key={action.id}
                    type="button"
                    onMouseEnter={() => setHighlightedIndex(-1)}
                    onClick={action.onSelect}
                    className={cn(
                      "flex h-[34px] w-full cursor-pointer items-center justify-between gap-2.5 rounded-[8px] px-2.5 text-left text-[13px] transition-colors",
                      isSelected
                        ? "bg-black/[0.055] text-foreground dark:bg-white/[0.08]"
                        : "text-foreground/90 hover:bg-black/[0.04] dark:hover:bg-white/[0.05]",
                    )}
                  >
                    <span className="flex min-w-0 items-center gap-2.5">
                      {action.icon}
                      <span className="truncate">{action.label}</span>
                    </span>
                    {action.trailing && (
                      <span className="flex shrink-0 items-center gap-1.5">{action.trailing}</span>
                    )}
                  </button>
                );
              })}
            </div>

            {(resolvedSubagents.length > 0 ||
              backgroundTasks.length > 0 ||
              recentFiles.length > 0) && (
              <div className="mt-6 flex flex-col gap-5">
                {/* Subagents */}
                {resolvedSubagents.length > 0 && (
                  <section>
                    <div className="mb-1.5 flex items-center gap-1.5 px-2.5 text-[11.5px] font-medium">
                      <span className="text-muted-foreground/75">Subagents</span>
                      <span className="text-muted-foreground/55 tabular-nums">
                        {resolvedSubagents.length}
                      </span>
                    </div>
                    <div className="flex flex-col gap-0.5">
                      {resolvedSubagents.map((subagent) => {
                        const isAwaiting = subagent.status === "awaiting_permission";
                        const isRunning = subagent.status === "running";
                        const isFailed = subagent.status === "failed";
                        return (
                          <button
                            key={subagent.key}
                            type="button"
                            onClick={() => {
                              if (isAwaiting && subagent.isManaged && subagent.childAgentId) {
                                orbitActions.selectAgent(subagent.childHostId, subagent.childAgentId);
                                return;
                              }
                              orbitActions.openSubagentTab({
                                callId: subagent.callId,
                                role: subagent.title,
                                promptPreview: subagent.promptPreview,
                                childAgentId: subagent.childAgentId,
                                childHostId: subagent.childHostId,
                                providerSubagentId: subagent.providerSubagentId,
                                parentAgentId: selectedAgentId,
                              });
                            }}
                            className="group/item text-foreground/85 flex h-[32px] w-full cursor-pointer items-center justify-between gap-2 rounded-[7px] px-2.5 text-left text-[12.5px] transition-colors hover:bg-black/[0.04] dark:hover:bg-white/[0.05]"
                          >
                            <span className="flex min-w-0 items-center gap-2.5">
                              {isAwaiting ? (
                                <span className="size-[6.5px] shrink-0 rounded-full bg-amber-500 shadow-[0_0_0_2.5px_rgba(245,158,11,0.18)]" />
                              ) : isRunning ? (
                                <Loader2 className="text-muted-foreground h-[14px] w-[14px] shrink-0 animate-spin" />
                              ) : isFailed ? (
                                <AlertCircle className="h-[14px] w-[14px] shrink-0 text-red-500" />
                              ) : (
                                <CheckCircle2 className="h-[14px] w-[14px] shrink-0 text-emerald-600 dark:text-emerald-400" />
                              )}
                              <span className="truncate font-medium">{subagent.title}</span>
                            </span>
                            <span className="flex shrink-0 items-center gap-1.5">
                              {isAwaiting ? (
                                <span className="text-[11px] font-medium text-amber-600/90 dark:text-amber-400/90">
                                  Approval
                                </span>
                              ) : subagent.remoteHostName ? (
                                <span className="text-muted-foreground/60 max-w-[76px] truncate text-[11px]">
                                  {subagent.remoteHostName}
                                </span>
                              ) : null}
                              <ChevronRight className="text-muted-foreground/45 group-hover/item:text-muted-foreground h-[13px] w-[13px] shrink-0 transition-colors" />
                            </span>
                          </button>
                        );
                      })}
                    </div>
                  </section>
                )}

                {/* Background Tasks */}
                {backgroundTasks.length > 0 && (
                  <section>
                    <div className="mb-1.5 flex items-center gap-1.5 px-2.5 text-[11.5px] font-medium">
                      <span className="text-muted-foreground/75">Background Tasks</span>
                      <span className="text-muted-foreground/55 tabular-nums">
                        {backgroundTasks.length}
                      </span>
                    </div>
                    <div className="flex flex-col gap-0.5">
                      {visibleBackgroundTasks.map((task) => {
                        const isRunning = task.status === "running";
                        const isTimerLike =
                          task.kind === "wakeup" || /\btimer\b/i.test(task.title);
                        const hasDetails = Boolean(
                          task.command || task.output_preview || task.log_path,
                        );
                        const isExpanded = Boolean(expandedTasks[task.id]);
                        return (
                          <div key={task.id} className="flex flex-col rounded-[7px]">
                            <button
                              type="button"
                              title={task.command || task.title}
                              onClick={() => {
                                if (!hasDetails) return;
                                setExpandedTasks((prev) => ({
                                  ...prev,
                                  [task.id]: !prev[task.id],
                                }));
                              }}
                              className={cn(
                                "text-foreground/85 flex min-h-[32px] w-full items-center justify-between gap-2 rounded-[7px] px-2.5 py-1 text-left text-[12.5px] transition-colors hover:bg-black/[0.04] dark:hover:bg-white/[0.05]",
                                hasDetails ? "cursor-pointer" : "cursor-default",
                              )}
                            >
                              <span className="flex min-w-0 items-center gap-2.5">
                                {isRunning ? (
                                  <Loader2 className="text-muted-foreground h-[14px] w-[14px] shrink-0 animate-spin" />
                                ) : isTimerLike ? (
                                  <Clock className="text-muted-foreground/75 h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                                ) : (
                                  <SquareTerminal className="text-muted-foreground/75 h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                                )}
                                <span className="min-w-0 truncate">{task.title}</span>
                              </span>
                              {hasDetails && (
                                <ChevronRight
                                  className={cn(
                                    "text-muted-foreground/50 h-[13px] w-[13px] shrink-0 transition-transform",
                                    isExpanded && "rotate-90",
                                  )}
                                />
                              )}
                            </button>
                            {hasDetails && isExpanded && (
                              <div className="border-border/60 selectable-text mx-2.5 mb-1.5 space-y-1 rounded-md border bg-black/[0.03] px-2.5 py-1.5 font-mono text-[11px] dark:bg-white/[0.04]">
                                {task.command && (
                                  <div className="text-foreground/85 break-all whitespace-pre-wrap">
                                    $ {task.command}
                                  </div>
                                )}
                                {task.log_path && (
                                  <div className="text-muted-foreground break-all">
                                    Log: {task.log_path}
                                  </div>
                                )}
                                {task.output_preview && (
                                  <pre className="text-muted-foreground max-h-[120px] overflow-auto break-words whitespace-pre-wrap">
                                    {task.output_preview.slice(0, 600)}
                                  </pre>
                                )}
                              </div>
                            )}
                          </div>
                        );
                      })}
                      {backgroundTasks.length > 4 && (
                        <button
                          type="button"
                          onClick={() => setShowAllTasks((prev) => !prev)}
                          className="text-muted-foreground hover:text-foreground mt-0.5 w-fit cursor-pointer px-2.5 py-1 text-[12px] transition-colors"
                        >
                          {showAllTasks ? "Show less" : "See all"}
                        </button>
                      )}
                    </div>
                  </section>
                )}

                {/* Recent Files */}
                {recentFiles.length > 0 && (
                  <section>
                    <div className="mb-1.5 flex items-center gap-1.5 px-2.5 text-[11.5px] font-medium">
                      <span className="text-muted-foreground/75">Recent Files</span>
                      <span className="text-muted-foreground/55 tabular-nums">
                        {insights.touched_files.length}
                      </span>
                    </div>
                    <div className="flex flex-col gap-0.5">
                      {recentFiles.map((file) => {
                        const FileIcon = resolveSemanticFileIcon(file.path);
                        const { name, dir } = splitDisplayPath(file.path);
                        return (
                          <button
                            key={file.path}
                            type="button"
                            onClick={() => orbitActions.openFileInSidePanel(file.path)}
                            className="text-foreground/85 flex h-[32px] w-full cursor-pointer items-center justify-between gap-2 rounded-[7px] px-2.5 text-left text-[12.5px] transition-colors hover:bg-black/[0.04] dark:hover:bg-white/[0.05]"
                          >
                            <span className="flex min-w-0 items-center gap-2.5">
                              <FileIcon className="text-muted-foreground/80 h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                              <span className="shrink-0 font-medium">{name}</span>
                              {dir && (
                                <span className="text-muted-foreground/60 truncate text-[11.5px]">
                                  {dir}
                                </span>
                              )}
                            </span>
                          </button>
                        );
                      })}
                    </div>
                  </section>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </ScrollArea>
  );
}
