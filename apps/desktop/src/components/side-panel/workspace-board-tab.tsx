import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AlarmClock,
  AlertTriangle,
  ArrowUpRight,
  Check,
  CheckCircle2,
  ChevronRight,
  ClipboardList,
  Columns2,
  Edit3,
  Eye,
  FileCode2,
  FileEdit,
  FileText,
  GitBranch,
  History,
  Lightbulb,
  Loader2,
  MessageSquare,
  RefreshCw,
  Save,
  ShieldCheck,
  Target,
  Users,
} from "lucide-react";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import {
  type AgentCardView,
  type RichBlockView,
  type WorkspaceEventRecord,
  isAgentForegroundRunning,
  isAgentTimerWaiting,
  orbitIpc,
  semanticAgentTabLabel,
} from "@/lib/orbit-ipc";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Textarea } from "@/components/ui/textarea";
import { type SidePanelTab, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface ParsedBoardSection {
  key: string;
  rawHeading: string;
  displayTitle: string;
  kind: "decisions" | "tasks" | "findings" | "custom";
  markdown: string;
}

interface ParsedBoardDoc {
  extractedGoal: string | null;
  sections: ParsedBoardSection[];
}

const TEMPLATE_PLACEHOLDER_PATTERNS = [
  /^\(record shared apis/i,
  /^\(track peer ownership/i,
  /^\(record review findings/i,
  /^coordinate peers in this mission workspace\.?$/i,
];

function isBoilerplateLine(line: string): boolean {
  const cleaned = line.trim().replace(/^[-*•]\s+/, "").trim();
  if (!cleaned) return false;
  return TEMPLATE_PLACEHOLDER_PATTERNS.some((re) => re.test(cleaned));
}

function classifySectionHeading(heading: string): {
  kind: ParsedBoardSection["kind"];
  displayTitle: string;
} {
  const lower = heading.trim().toLowerCase();
  if (lower.includes("decision") || lower.includes("contract") || lower.includes("决策") || lower.includes("约定")) {
    return { kind: "decisions", displayTitle: "Decisions & Contracts" };
  }
  if (lower.includes("task") || lower.includes("todo") || lower.includes("任务") || lower.includes("分工")) {
    return { kind: "tasks", displayTitle: "Tasks" };
  }
  if (lower.includes("finding") || lower.includes("result") || lower.includes("progress") || lower.includes("结论") || lower.includes("进展")) {
    return { kind: "findings", displayTitle: "Findings & Progress" };
  }
  return { kind: "custom", displayTitle: heading.trim() };
}

function parseBoardMarkdown(raw: string): ParsedBoardDoc {
  const lines = raw.split(/\r?\n/);
  let extractedGoal: string | null = null;
  const rawSections: { heading: string; lines: string[] }[] = [];
  let current: { heading: string; lines: string[] } | null = null;

  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith("# ") && !trimmed.startsWith("## ")) {
      // Skip redundant H1 workspace title (already rendered in header)
      continue;
    }
    if (trimmed.startsWith("## ")) {
      if (current) rawSections.push(current);
      current = { heading: trimmed.slice(3).trim(), lines: [] };
      continue;
    }
    if (!current) {
      if (trimmed) {
        current = { heading: "Notes", lines: [line] };
      }
    } else {
      current.lines.push(line);
    }
  }
  if (current) rawSections.push(current);

  const sections: ParsedBoardSection[] = [];
  for (const sec of rawSections) {
    const keptLines = sec.lines.filter((l) => !isBoilerplateLine(l));
    const body = keptLines.join("\n").trim();
    if (!body) continue;

    const lowerHeading = sec.heading.toLowerCase();
    if (lowerHeading === "goal" || lowerHeading === "目标" || lowerHeading === "mission goal") {
      extractedGoal = body.replace(/^[-*•]\s+/, "").trim();
      continue;
    }

    const { kind, displayTitle } = classifySectionHeading(sec.heading);
    sections.push({
      key: `${sec.heading}-${sections.length}`,
      rawHeading: sec.heading,
      displayTitle,
      kind,
      markdown: body,
    });
  }

  return { extractedGoal, sections };
}

function SectionIcon({ kind }: { kind: ParsedBoardSection["kind"] }) {
  switch (kind) {
    case "decisions":
      return <ShieldCheck className="h-[13.5px] w-[13.5px] shrink-0 text-sky-500" />;
    case "tasks":
      return <ClipboardList className="h-[13.5px] w-[13.5px] shrink-0 text-violet-500" />;
    case "findings":
      return <Lightbulb className="h-[13.5px] w-[13.5px] shrink-0 text-emerald-500" />;
    default:
      return <FileText className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0" />;
  }
}

function shortFileLabel(rawPath: string, rootCwd?: string): string {
  const clean = rawPath.trim().replace(/^`|`$/g, "");
  if (!clean) return "";
  if (rootCwd) {
    const cleanRoot = rootCwd.replace(/\/+$/, "");
    if (clean.startsWith(`${cleanRoot}/`)) {
      const rel = clean.slice(cleanRoot.length + 1);
      const parts = rel.split("/");
      return parts.length > 3 ? `…/${parts.slice(-2).join("/")}` : rel;
    }
  }
  const parts = clean.split("/").filter(Boolean);
  if (parts.length <= 2) return parts.join("/") || clean;
  return `${parts[parts.length - 2]}/${parts[parts.length - 1]}`;
}

function stripRawMarkdownNoise(text: string): string {
  return text
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/[*#_~]+/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function formatRelativeEventTime(iso: string): string {
  try {
    const ts = Date.parse(iso);
    if (!Number.isFinite(ts)) return "";
    const diffSec = Math.max(0, Math.floor((Date.now() - ts) / 1000));
    if (diffSec < 60) return "just now";
    const mins = Math.floor(diffSec / 60);
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    return new Date(ts).toLocaleDateString(undefined, { month: "short", day: "numeric" });
  } catch {
    return "";
  }
}

interface HumanActivityItem {
  key: string;
  kind: WorkspaceEventRecord["kind"];
  actor: string;
  description: string;
  createdAt: string;
  count: number;
  filePath?: string | null;
}

function buildHumanActivities(
  events: WorkspaceEventRecord[],
  rootCwd?: string,
): HumanActivityItem[] {
  const out: HumanActivityItem[] = [];

  for (const ev of events) {
    const actor = ev.agent_handle?.trim() || "System";
    let description = "";

    switch (ev.kind) {
      case "conflict_warning": {
        const f = ev.file_path ? shortFileLabel(ev.file_path, rootCwd) : "a shared file";
        description = `concurrent edit detected on ${f}`;
        break;
      }
      case "board_updated": {
        const secMatch = /section `([^`]+)`/i.exec(ev.summary);
        description = secMatch
          ? `updated ${classifySectionHeading(secMatch[1]).displayTitle}`
          : "updated notes";
        break;
      }
      case "file_edited": {
        const f = ev.file_path ? shortFileLabel(ev.file_path, rootCwd) : "a file";
        description = `edited ${f}`;
        break;
      }
      case "peer_message": {
        const cleaned = stripRawMarkdownNoise(ev.summary).slice(0, 72);
        description = cleaned || "sent a message";
        break;
      }
      case "peer_joined": {
        description = "joined the group";
        break;
      }
      case "peer_finished": {
        const rawRest = ev.summary.replace(/^@\S+\s+finished:\s*/i, "").trim();
        const cleaned = stripRawMarkdownNoise(rawRest);
        if (!cleaned || /^finished turn$/i.test(cleaned)) {
          description = "finished turn";
        } else {
          description = `${cleaned.slice(0, 64)}${cleaned.length > 64 ? "…" : ""}`;
        }
        break;
      }
      default: {
        description = stripRawMarkdownNoise(ev.summary).slice(0, 72) || "updated status";
      }
    }

    // Coalesce consecutive identical edits/actions from the same actor
    const prev = out[out.length - 1];
    if (
      prev &&
      prev.kind === ev.kind &&
      prev.actor === actor &&
      prev.description === description
    ) {
      prev.count += 1;
      continue;
    }

    out.push({
      key: `${ev.seq}`,
      kind: ev.kind,
      actor,
      description,
      createdAt: ev.created_at,
      count: 1,
      filePath: ev.file_path,
    });
  }

  return out;
}

function EventIcon({ kind }: { kind: WorkspaceEventRecord["kind"] }) {
  switch (kind) {
    case "conflict_warning":
      return <AlertTriangle className="h-[13px] w-[13px] shrink-0 text-amber-500" />;
    case "board_updated":
      return <FileText className="h-[13px] w-[13px] shrink-0 text-sky-500" />;
    case "peer_message":
      return <MessageSquare className="h-[13px] w-[13px] shrink-0 text-violet-500" />;
    case "file_edited":
      return <FileEdit className="text-muted-foreground h-[13px] w-[13px] shrink-0" />;
    case "peer_finished":
      return <CheckCircle2 className="h-[13px] w-[13px] shrink-0 text-emerald-500" />;
    default:
      return <Users className="text-muted-foreground h-[13px] w-[13px] shrink-0" />;
  }
}

export interface BoardDraftSyncInput {
  prevScopeKey: string;
  nextScopeKey: string;
  prevServerContent: string;
  nextServerContent: string;
  currentDraft: string;
}

function normalizeBoardText(text: string): string {
  return text.replace(/\r\n/g, "\n");
}

export function reconcileBoardDraft(input: BoardDraftSyncInput): {
  nextDraft: string;
  shouldExitEdit: boolean;
} {
  if (input.prevScopeKey !== input.nextScopeKey) {
    return { nextDraft: input.nextServerContent, shouldExitEdit: true };
  }
  const prevNorm = normalizeBoardText(input.prevServerContent);
  const nextNorm = normalizeBoardText(input.nextServerContent);
  const curNorm = normalizeBoardText(input.currentDraft);
  if (prevNorm === nextNorm) {
    return { nextDraft: input.currentDraft, shouldExitEdit: false };
  }
  const wasDirty = curNorm !== prevNorm && curNorm !== nextNorm;
  return {
    nextDraft: wasDirty ? input.currentDraft : input.nextServerContent,
    shouldExitEdit: false,
  };
}

const EMPTY_BOARD_EVENTS: readonly WorkspaceEventRecord[] = [];
const EMPTY_BOARD_PEERS: readonly AgentCardView[] = [];

function sameBoardPeers(prev: readonly AgentCardView[], next: readonly AgentCardView[]): boolean {
  if (prev === next) return true;
  if (prev.length !== next.length) return false;
  for (let i = 0; i < prev.length; i += 1) {
    const a = prev[i];
    const b = next[i];
    if (
      a.agent_id !== b.agent_id ||
      a.title !== b.title ||
      a.handle !== b.handle ||
      a.provider !== b.provider ||
      a.model !== b.model ||
      a.status_label !== b.status_label ||
      a.is_running !== b.is_running ||
      a.is_awaiting_permission !== b.is_awaiting_permission ||
      a.has_background_tasks !== b.has_background_tasks ||
      a.has_armed_timers !== b.has_armed_timers ||
      a.worktree_branch !== b.worktree_branch ||
      a.updated_at_rfc3339 !== b.updated_at_rfc3339
    ) {
      return false;
    }
  }
  return true;
}

export const WorkspaceBoardTab = memo(function WorkspaceBoardTab({
  tab,
  visible = true,
}: {
  tab: SidePanelTab;
  visible?: boolean;
}) {
  const { t } = useI18n();
  const hostId = tab.hostId ?? "";
  const workspaceId = tab.workspaceId ?? "";
  const scopeKey = `${hostId}::${workspaceId}`;
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);

  const board = useOrbitSelector((s) =>
    hostId && workspaceId ? (s.hostDetails[hostId]?.workspace_boards?.[workspaceId] ?? null) : null,
  );
  const rawEvents = useOrbitSelector((s) =>
    visible && hostId && workspaceId
      ? (s.hostDetails[hostId]?.workspace_events?.[workspaceId] ?? EMPTY_BOARD_EVENTS)
      : EMPTY_BOARD_EVENTS,
  );
  const workspaceKey = useOrbitSelector((s) => {
    if (!hostId || !workspaceId) return "";
    const ws = s.hostDetails[hostId]?.workspaces.find((w) => w.workspace_id === workspaceId);
    if (!ws) return "";
    return `${ws.workspace_id}\0${ws.title ?? ""}\0${ws.mission_goal ?? ""}\0${ws.project_root ?? ""}\0${ws.source_cwd ?? ""}\0${ws.cwd ?? ""}`;
  });
  const workspace = useMemo(() => {
    if (!workspaceKey) return null;
    const [workspace_id, title, mission_goal, project_root, source_cwd, cwd] =
      workspaceKey.split("\0");
    return {
      workspace_id,
      title,
      mission_goal: mission_goal || null,
      project_root: project_root || null,
      source_cwd: source_cwd || null,
      cwd,
    };
  }, [workspaceKey]);

  const peersCacheRef = useRef<readonly AgentCardView[]>(EMPTY_BOARD_PEERS);
  const peers = useOrbitSelector((s) => {
    if (!visible || !hostId || !workspaceId) {
      peersCacheRef.current = EMPTY_BOARD_PEERS;
      return EMPTY_BOARD_PEERS;
    }
    const agents = s.hostDetails[hostId]?.agents;
    if (!agents || agents.length === 0) {
      peersCacheRef.current = EMPTY_BOARD_PEERS;
      return EMPTY_BOARD_PEERS;
    }
    const nextPeers = agents.filter(
      (a) =>
        a.workspace_id === workspaceId &&
        !a.archived_at_rfc3339 &&
        !a.background &&
        !a.parent_agent_id,
    );
    if (sameBoardPeers(peersCacheRef.current, nextPeers)) {
      return peersCacheRef.current;
    }
    peersCacheRef.current = nextPeers;
    return nextPeers;
  });

  const serverContent = board?.content ?? "";
  const events = useMemo(() => [...rawEvents].reverse(), [rawEvents]);

  const [editingBoard, setEditingBoard] = useState(false);
  const [boardDraft, setBoardDraft] = useState(serverContent);
  const [savingBoard, setSavingBoard] = useState(false);
  const lastSyncedBoardRef = useRef<{ scopeKey: string; serverContent: string }>({
    scopeKey,
    serverContent,
  });
  const [goalDraft, setGoalDraft] = useState(workspace?.mission_goal ?? "");
  const [editingGoal, setEditingGoal] = useState(false);
  const [titleDraft, setTitleDraft] = useState(workspace?.title ?? "");
  const [editingTitle, setEditingTitle] = useState(false);
  const titleIme = useImeCompositionGuard();
  const goalIme = useImeCompositionGuard();
  const [activityExpanded, setActivityExpanded] = useState(false);
  const [sectionBlocks, setSectionBlocks] = useState<Record<string, RichBlockView[]>>({});

  const isBoardDirty = boardDraft !== serverContent;
  const previewSource = isBoardDirty ? boardDraft : serverContent;

  const parsedDoc = useMemo(
    () => parseBoardMarkdown(previewSource),
    [previewSource],
  );

  const rootCwd = workspace?.project_root || workspace?.source_cwd || workspace?.cwd || "";

  const conflictEvents = useMemo(
    () => events.filter((ev) => ev.kind === "conflict_warning").slice(0, 5),
    [events],
  );

  const touchedFiles = useMemo(() => {
    const byPath = new Map<
      string,
      { path: string; label: string; lastActor: string; count: number; lastAt: string }
    >();
    for (const ev of events) {
      if (ev.kind !== "file_edited" || !ev.file_path) continue;
      const existing = byPath.get(ev.file_path);
      if (existing) {
        existing.count += 1;
      } else {
        byPath.set(ev.file_path, {
          path: ev.file_path,
          label: shortFileLabel(ev.file_path, rootCwd),
          lastActor: ev.agent_handle?.trim() || "Thread",
          count: 1,
          lastAt: ev.created_at,
        });
      }
    }
    return Array.from(byPath.values()).slice(0, 8);
  }, [events, rootCwd]);

  const humanActivities = useMemo(
    () => buildHumanActivities(events, rootCwd).slice(0, 24),
    [events, rootCwd],
  );

  useEffect(() => {
    if (!hostId || !workspaceId) return;
    void orbitActions.fetchWorkspaceBoardAndEvents(hostId, workspaceId);
  }, [hostId, workspaceId]);

  useEffect(() => {
    const prev = lastSyncedBoardRef.current;
    if (prev.scopeKey === scopeKey && prev.serverContent === serverContent) {
      return;
    }
    lastSyncedBoardRef.current = { scopeKey, serverContent };
    if (prev.scopeKey !== scopeKey) {
      setBoardDraft(serverContent);
      setEditingBoard(false);
      setEditingGoal(false);
      setEditingTitle(false);
      return;
    }
    setBoardDraft((currentDraft) =>
      reconcileBoardDraft({
        prevScopeKey: prev.scopeKey,
        nextScopeKey: scopeKey,
        prevServerContent: prev.serverContent,
        nextServerContent: serverContent,
        currentDraft,
      }).nextDraft,
    );
  }, [scopeKey, serverContent]);

  useEffect(() => {
    if (!editingGoal) {
      setGoalDraft(workspace?.mission_goal ?? parsedDoc.extractedGoal ?? "");
    }
  }, [workspace?.mission_goal, parsedDoc.extractedGoal, editingGoal]);

  useEffect(() => {
    if (!visible || editingBoard) return;
    if (parsedDoc.sections.length === 0) {
      setSectionBlocks({});
      return;
    }
    let cancelled = false;
    void Promise.all(
      parsedDoc.sections.map(async (sec) => {
        try {
          const blocks = await orbitIpc.parseMarkdownPreview(sec.markdown, false);
          return [sec.key, blocks] as const;
        } catch {
          return [sec.key, [] as RichBlockView[]] as const;
        }
      }),
    ).then((entries) => {
      if (!cancelled) {
        setSectionBlocks(Object.fromEntries(entries));
      }
    });
    return () => {
      cancelled = true;
    };
  }, [visible, editingBoard, parsedDoc.sections]);

  useEffect(() => {
    if (!editingTitle) {
      setTitleDraft(workspace?.title ?? "");
    }
  }, [workspace?.title, editingTitle]);

  const handleSaveBoard = async () => {
    if (!hostId || !workspaceId || savingBoard) return;
    const savedContent = boardDraft;
    setSavingBoard(true);
    try {
      await orbitActions.updateWorkspaceBoard(hostId, workspaceId, savedContent, null);
      setEditingBoard(false);
    } catch (err) {
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      orbitActions.showToast(
        detail ? t("store.failedUpdateGroup", { detail }) : t("common.actionFailed"),
      );
    } finally {
      setSavingBoard(false);
    }
  };

  const handleDiscardBoard = () => {
    setBoardDraft(serverContent);
    setEditingBoard(false);
  };

  const handleSaveTitle = async () => {
    const clean = titleDraft.trim();
    if (!hostId || !workspaceId || !clean) {
      setEditingTitle(false);
      return;
    }
    await orbitActions.renameWorkspace(hostId, workspaceId, clean);
    setEditingTitle(false);
  };

  const handleSaveGoal = async () => {
    if (!hostId || !workspaceId) return;
    await orbitActions.updateWorkspaceGoal(hostId, workspaceId, goalDraft.trim());
    setEditingGoal(false);
  };

  const runningPeersCount = peers.filter((p) => isAgentForegroundRunning(p)).length;
  const effectiveGoal = workspace?.mission_goal?.trim() || parsedDoc.extractedGoal || "";

  return (
    <div
      className={cn(
        "flex min-h-0 flex-1 flex-col overflow-hidden",
        !visible && "hidden",
      )}
    >
      {/* Header */}
      <div className="border-border/70 flex items-center justify-between gap-2 border-b px-4 py-2.5">
        <div className="min-w-0 flex-1">
          {editingTitle ? (
            <input
              autoFocus
              value={titleDraft}
              onChange={(e) => setTitleDraft(e.target.value)}
              onBlur={() => void handleSaveTitle()}
              {...titleIme.compositionProps}
              onKeyDown={(e) => {
                if (titleIme.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Enter") void handleSaveTitle();
                if (e.key === "Escape") {
                  setTitleDraft(workspace?.title ?? "");
                  setEditingTitle(false);
                }
              }}
              className="border-border bg-canvas text-foreground h-[24px] w-full max-w-64 rounded-md border px-2 text-[13.5px] font-semibold outline-none"
            />
          ) : (
            <button
              type="button"
              title="Click to rename group"
              onClick={() => setEditingTitle(true)}
              className="text-foreground hover:text-foreground/80 flex max-w-full cursor-pointer items-center gap-1.5 truncate text-left text-[14px] font-semibold transition-colors"
            >
              <span className="truncate">{workspace?.title || "Group Overview"}</span>
              <Edit3 className="text-muted-foreground/65 h-[11.5px] w-[11.5px] shrink-0" />
            </button>
          )}
          <div className="text-muted-foreground mt-0.5 flex items-center gap-2 text-[11.5px]">
            <span>{peers.length === 1 ? "1 thread" : `${peers.length} threads`}</span>
            {runningPeersCount > 0 && (
              <>
                <span>·</span>
                <span className="text-foreground/80">{runningPeersCount} running</span>
              </>
            )}
            {touchedFiles.length > 0 && (
              <>
                <span>·</span>
                <span>
                  {touchedFiles.length === 1 ? "1 file" : `${touchedFiles.length} files`}
                </span>
              </>
            )}
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            title="Refresh"
            onClick={() => void orbitActions.fetchWorkspaceBoardAndEvents(hostId, workspaceId)}
            className="text-muted-foreground hover:text-foreground inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] dark:hover:bg-white/[0.07]"
          >
            <RefreshCw className="h-[13px] w-[13px]" />
          </button>
          <button
            type="button"
            onClick={() => setEditingBoard((v) => !v)}
            className="border-border hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1.5 rounded-md border px-2 text-[12px]"
          >
            {editingBoard ? (
              <>
                <Eye className="h-[12px] w-[12px]" />
                <span>{t("sidePanel.preview")}</span>
              </>
            ) : (
              <>
                <Edit3 className="h-[12px] w-[12px]" />
                <span>Edit notes</span>
              </>
            )}
            {isBoardDirty && (
              <span
                title={t("sidePanel.unsavedChanges")}
                className="bg-signal-warning size-1.5 shrink-0 rounded-full"
              />
            )}
          </button>
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1" viewportClassName="p-4 space-y-4">
        {/* 1. Goal Card */}
        <section className="border-border/70 bg-elevated/45 space-y-1.5 rounded-xl border p-3.5">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground flex items-center gap-1.5 text-[11.5px] font-medium">
              <Target className="h-[13px] w-[13px] opacity-75" />
              <span>Goal</span>
            </span>
            {!editingGoal && (
              <button
                type="button"
                onClick={() => setEditingGoal(true)}
                className="text-muted-foreground hover:text-foreground cursor-pointer text-[11.5px]"
              >
                Edit
              </button>
            )}
          </div>

          {editingGoal ? (
            <div className="space-y-2">
              <Input
                autoFocus
                value={goalDraft}
                onChange={(e) => setGoalDraft(e.target.value)}
                {...goalIme.compositionProps}
                onKeyDown={(e) => {
                  if (goalIme.shouldIgnoreKeyDown(e)) return;
                  if (e.key === "Enter") void handleSaveGoal();
                  if (e.key === "Escape") setEditingGoal(false);
                }}
                placeholder="Describe the shared goal for this group…"
              />
              <div className="flex justify-end gap-1.5">
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  onClick={() => setEditingGoal(false)}
                >
                  Cancel
                </Button>
                <Button type="button" size="xs" onClick={() => void handleSaveGoal()}>
                  <Check className="h-[11px] w-[11px]" />
                  <span>Save</span>
                </Button>
              </div>
            </div>
          ) : (
            <p className="text-foreground/90 text-[13px] leading-relaxed">
              {effectiveGoal || "No goal set yet. Click Edit to share a goal across threads."}
            </p>
          )}
        </section>

        {/* 2. Conflict Alerts */}
        {conflictEvents.length > 0 && (
          <section className="space-y-1.5 rounded-xl border border-amber-500/30 bg-amber-500/[0.07] p-3">
            <div className="flex items-center gap-1.5 text-[12px] font-medium text-amber-700 dark:text-amber-300">
              <AlertTriangle className="h-[13.5px] w-[13.5px]" />
              <span>Conflicts ({conflictEvents.length})</span>
            </div>
            <div className="space-y-1">
              {conflictEvents.map((ev) => (
                <div
                  key={ev.seq}
                  className="flex items-center justify-between gap-2 text-[12px] text-amber-800/90 dark:text-amber-200/90"
                >
                  <span className="truncate">
                    {ev.agent_handle ? `${ev.agent_handle} · ` : ""}
                    {ev.file_path ? shortFileLabel(ev.file_path, rootCwd) : ev.summary}
                  </span>
                  <span className="shrink-0 text-[11px] opacity-75">
                    {formatRelativeEventTime(ev.created_at)}
                  </span>
                </div>
              ))}
            </div>
          </section>
        )}

        {/* 3. Threads in Group */}
        {peers.length > 0 && (
          <section className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground flex items-center gap-1.5 text-[11.5px] font-medium">
                <Users className="h-[12.5px] w-[12.5px]" />
                <span>Threads ({peers.length})</span>
              </span>
            </div>
            <div className="space-y-1.5">
              {peers.map((peer) => {
                const isFocused = peer.agent_id === selectedAgentId;
                const handle = semanticAgentTabLabel(peer);
                const peerForegroundRunning = isAgentForegroundRunning(peer);
                const peerTimerArmed = !peerForegroundRunning && isAgentTimerWaiting(peer);
                const statusText = peerForegroundRunning
                  ? "Running"
                  : peer.is_awaiting_permission
                    ? "Approval"
                    : "Idle";
                return (
                  <div
                    key={peer.agent_id}
                    className={cn(
                      "border-border/65 flex items-start justify-between gap-2 rounded-xl border px-3 py-2 text-[12px] transition-colors",
                      isFocused
                        ? "border-foreground/20 bg-foreground/[0.04]"
                        : "bg-elevated/35 hover:bg-elevated/65",
                    )}
                  >
                    <button
                      type="button"
                      onClick={() => orbitActions.selectAgent(hostId, peer.agent_id)}
                      className="flex min-w-0 flex-1 cursor-pointer items-start gap-2.5 text-left"
                    >
                      {peerForegroundRunning ? (
                        <Loader2 className="text-foreground/70 mt-0.5 h-[13px] w-[13px] shrink-0 animate-spin" />
                      ) : peerTimerArmed ? (
                        <AlarmClock className="text-ink-faint mt-0.5 h-[12.5px] w-[12.5px] shrink-0 stroke-[1.7]" />
                      ) : (
                        <ProviderIcon
                          provider={peer.provider}
                          className="mt-0.5 h-[13.5px] w-[13.5px] shrink-0 opacity-80"
                        />
                      )}
                      <div className="min-w-0 flex-1 space-y-0.5">
                        <div className="flex flex-wrap items-center gap-1.5">
                          <span className="text-foreground truncate font-medium">{handle}</span>
                          <span
                            className={cn(
                              "rounded-full px-1.5 py-[1px] text-[10px] font-medium",
                              peerForegroundRunning
                                ? "bg-foreground/[0.07] text-foreground/80"
                                : peer.is_awaiting_permission
                                  ? "bg-amber-500/15 text-amber-700 dark:text-amber-300"
                                  : "bg-black/[0.05] text-muted-foreground dark:bg-white/[0.08]",
                            )}
                          >
                            {statusText}
                          </span>
                          {peer.worktree_branch && (
                            <span className="bg-muted text-muted-foreground inline-flex items-center gap-0.5 rounded px-1.5 py-[1px] font-mono text-[10px]">
                              <GitBranch className="h-[9.5px] w-[9.5px]" />
                              {peer.worktree_branch}
                            </span>
                          )}
                        </div>
                        <div className="text-muted-foreground line-clamp-2 text-[11.5px] leading-snug">
                          {peer.task_summary || peer.title}
                        </div>
                      </div>
                    </button>
                    <button
                      type="button"
                      title="Open side-by-side"
                      onClick={() => orbitActions.openPeerSplitTab(hostId, peer.agent_id, handle)}
                      className="text-muted-foreground hover:text-foreground mt-0.5 inline-flex h-[22px] w-[22px] shrink-0 cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
                    >
                      <Columns2 className="h-[12px] w-[12px]" />
                    </button>
                  </div>
                );
              })}
            </div>
          </section>
        )}

        {/* 4. Shared Notes */}
        <section className="space-y-2">
          <div className="flex items-center justify-between gap-2">
            <span className="text-muted-foreground flex items-center gap-1.5 text-[11.5px] font-medium">
              <FileText className="h-[12.5px] w-[12.5px]" />
              <span>Shared notes</span>
              {isBoardDirty && (
                <span className="bg-signal-warning-bg border-signal-warning-border text-signal-warning inline-flex items-center gap-1 rounded-full border px-1.5 py-0.5 text-[10.5px] leading-none font-medium">
                  <span className="bg-signal-warning size-1.5 rounded-full" />
                  <span>{t("sidePanel.unsavedChanges")}</span>
                </span>
              )}
            </span>
            {board?.updated_by && !editingBoard && !isBoardDirty && parsedDoc.sections.length > 0 && (
              <span className="text-muted-foreground/70 text-[11px]">
                Updated by {board.updated_by}
              </span>
            )}
          </div>

          {!editingBoard && isBoardDirty && (
            <div className="border-signal-warning-border bg-signal-warning-bg flex items-center justify-between gap-2 rounded-xl border px-3 py-2">
              <span className="text-ink-primary flex min-w-0 items-center gap-1.5 text-[12px] font-medium">
                <span className="bg-signal-warning size-1.5 shrink-0 rounded-full" />
                <span className="truncate">{t("sidePanel.unsavedChanges")}</span>
              </span>
              <div className="flex shrink-0 items-center gap-1.5">
                <Button
                  type="button"
                  variant="outline"
                  size="xs"
                  disabled={savingBoard}
                  onClick={handleDiscardBoard}
                >
                  {t("review.discard")}
                </Button>
                <Button
                  type="button"
                  size="xs"
                  disabled={savingBoard}
                  onClick={() => void handleSaveBoard()}
                >
                  {savingBoard ? (
                    <Loader2 className="h-[11px] w-[11px] animate-spin" />
                  ) : (
                    <Save className="h-[11px] w-[11px]" />
                  )}
                  <span>{t("peerStrip.save")}</span>
                </Button>
              </div>
            </div>
          )}

          {editingBoard ? (
            <div className="space-y-2">
              <Textarea
                rows={12}
                value={boardDraft}
                onChange={(e) => setBoardDraft(e.target.value)}
                placeholder={"## Decisions & Contracts\n- ...\n\n## Tasks\n- ...\n\n## Findings\n- ..."}
                className="p-3 font-mono text-[12px] leading-relaxed"
              />
              <div className="flex justify-end gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={savingBoard}
                  onClick={handleDiscardBoard}
                >
                  {isBoardDirty ? t("review.discard") : t("common.cancel")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  disabled={savingBoard || !isBoardDirty}
                  onClick={() => void handleSaveBoard()}
                >
                  {savingBoard ? (
                    <Loader2 className="h-[12px] w-[12px] animate-spin" />
                  ) : (
                    <Save className="h-[12px] w-[12px]" />
                  )}
                  <span>{t("peerStrip.save")}</span>
                </Button>
              </div>
            </div>
          ) : parsedDoc.sections.length > 0 ? (
            <div className="space-y-2.5">
              {parsedDoc.sections.map((sec) => {
                const blocks = sectionBlocks[sec.key] ?? [];
                return (
                  <div
                    key={sec.key}
                    className="border-border/65 bg-elevated/35 space-y-2 rounded-xl border p-3.5 select-text"
                  >
                    <div className="flex items-center gap-1.5 text-[12px] font-semibold">
                      <SectionIcon kind={sec.kind} />
                      <span className="text-foreground/90">{sec.displayTitle}</span>
                    </div>
                    <div className="text-[12.5px] leading-relaxed">
                      {blocks.length > 0 ? (
                        <MarkdownBlocksView blocks={blocks} compact />
                      ) : (
                        <div className="text-foreground/85 whitespace-pre-wrap text-[12.5px] leading-relaxed">
                          {stripRawMarkdownNoise(sec.markdown)}
                        </div>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="border-border/65 text-muted-foreground rounded-xl border border-dashed px-4 py-5 text-center text-[12px] leading-relaxed">
              No shared notes yet. Click <strong>Edit notes</strong> to record decisions or tasks.
            </div>
          )}
        </section>

        {/* 5. Touched Files */}
        {touchedFiles.length > 0 && (
          <section className="space-y-1.5">
            <div className="text-muted-foreground flex items-center gap-1.5 text-[11.5px] font-medium">
              <FileCode2 className="h-[12.5px] w-[12.5px]" />
              <span>Touched files ({touchedFiles.length})</span>
            </div>
            <div className="border-border/60 bg-elevated/25 divide-border/50 divide-y rounded-xl border">
              {touchedFiles.map((file) => (
                <button
                  key={file.path}
                  type="button"
                  title={file.path}
                  onClick={() =>
                    orbitActions.openFileInSidePanel(file.path, {
                      hostId,
                      cwd: rootCwd || undefined,
                    })
                  }
                  className="hover:bg-accent/55 flex w-full cursor-pointer items-center justify-between gap-2 px-3 py-1.5 text-left text-[12px] transition-colors first:rounded-t-xl last:rounded-b-xl"
                >
                  <span className="flex min-w-0 flex-1 items-center gap-2">
                    <span className="text-foreground/88 truncate font-mono text-[11.5px]">
                      {file.label}
                    </span>
                    {file.count > 1 && (
                      <span className="text-muted-foreground/75 shrink-0 text-[10.5px] tabular-nums">
                        ×{file.count}
                      </span>
                    )}
                  </span>
                  <span className="text-muted-foreground/75 flex shrink-0 items-center gap-1.5 text-[11px]">
                    <span className="max-w-20 truncate">{file.lastActor}</span>
                    <ArrowUpRight className="h-[11.5px] w-[11.5px] opacity-65" />
                  </span>
                </button>
              ))}
            </div>
          </section>
        )}

        {/* 6. Collapsible Activity Feed */}
        {humanActivities.length > 0 && (
          <section className="border-border/60 border-t pt-2.5">
            <button
              type="button"
              onClick={() => setActivityExpanded((v) => !v)}
              className="text-muted-foreground hover:text-foreground flex w-full cursor-pointer items-center justify-between rounded-lg px-1 py-1 text-[11.5px] font-medium transition-colors"
            >
              <span className="flex items-center gap-1.5">
                <History className="h-[12.5px] w-[12.5px]" />
                <span>Activity ({humanActivities.length})</span>
              </span>
              <ChevronRight
                className={cn(
                  "h-[13px] w-[13px] transition-transform duration-150",
                  activityExpanded && "rotate-90",
                )}
              />
            </button>

            {activityExpanded && (
              <div className="mt-1.5 space-y-1">
                {humanActivities.map((item) => (
                  <div
                    key={item.key}
                    className={cn(
                      "flex items-start gap-2 rounded-lg px-2.5 py-1.5 text-[11.5px]",
                      item.kind === "conflict_warning"
                        ? "border border-amber-500/35 bg-amber-500/10 text-amber-800 dark:text-amber-200"
                        : "bg-elevated/30 text-foreground/82",
                    )}
                  >
                    <div className="mt-0.5">
                      <EventIcon kind={item.kind} />
                    </div>
                    <div className="min-w-0 flex-1 leading-snug">
                      <span className="text-foreground mr-1.5 font-medium">{item.actor}</span>
                      <span>{item.description}</span>
                      {item.count > 1 && (
                        <span className="text-muted-foreground ml-1 text-[10.5px] tabular-nums">
                          ×{item.count}
                        </span>
                      )}
                    </div>
                    <span className="text-muted-foreground/65 shrink-0 text-[10.5px]">
                      {formatRelativeEventTime(item.createdAt)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </section>
        )}
      </ScrollArea>
    </div>
  );
});
