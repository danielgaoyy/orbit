import { memo, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Check,
  Columns2,
  Edit3,
  Eye,
  FileEdit,
  FileText,
  GitBranch,
  Loader2,
  MessageSquare,
  RefreshCw,
  Save,
  Target,
  Users,
} from "lucide-react";
import {
  type RichBlockView,
  type WorkspaceEventRecord,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { ScrollArea } from "@/components/ui/scroll-area";
import { type SidePanelTab, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function formatEventTime(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    });
  } catch {
    return "";
  }
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
    default:
      return <Users className="text-muted-foreground h-[13px] w-[13px] shrink-0" />;
  }
}

export const WorkspaceBoardTab = memo(function WorkspaceBoardTab({
  tab,
}: {
  tab: SidePanelTab;
}) {
  const hostId = tab.hostId ?? "";
  const workspaceId = tab.workspaceId ?? "";
  const hostDetail = useOrbitSelector((s) => (hostId ? (s.hostDetails[hostId] ?? null) : null));
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);

  const workspace = useMemo(
    () => hostDetail?.workspaces.find((w) => w.workspace_id === workspaceId) ?? null,
    [hostDetail, workspaceId],
  );
  const board = hostDetail?.workspace_boards?.[workspaceId] ?? null;
  const events = useMemo(
    () => [...(hostDetail?.workspace_events?.[workspaceId] ?? [])].reverse(),
    [hostDetail, workspaceId],
  );
  const peers = useMemo(
    () =>
      (hostDetail?.agents ?? []).filter(
        (a) => a.workspace_id === workspaceId && !a.archived_at_rfc3339 && !a.background,
      ),
    [hostDetail, workspaceId],
  );

  const [editingBoard, setEditingBoard] = useState(false);
  const [boardDraft, setBoardDraft] = useState(board?.content ?? "");
  const [savingBoard, setSavingBoard] = useState(false);
  const [goalDraft, setGoalDraft] = useState(workspace?.mission_goal ?? "");
  const [editingGoal, setEditingGoal] = useState(false);
  const [previewBlocks, setPreviewBlocks] = useState<RichBlockView[]>([]);

  useEffect(() => {
    if (!hostId || !workspaceId) return;
    void orbitActions.fetchWorkspaceBoardAndEvents(hostId, workspaceId);
  }, [hostId, workspaceId]);

  useEffect(() => {
    if (!editingBoard) {
      setBoardDraft(board?.content ?? "");
    }
  }, [board?.content, editingBoard]);

  useEffect(() => {
    if (!editingGoal) {
      setGoalDraft(workspace?.mission_goal ?? "");
    }
  }, [workspace?.mission_goal, editingGoal]);

  useEffect(() => {
    const content = board?.content ?? "";
    if (!content.trim()) {
      setPreviewBlocks([]);
      return;
    }
    let cancelled = false;
    void orbitIpc
      .parseMarkdownPreview(content, false)
      .then((blocks) => {
        if (!cancelled) setPreviewBlocks(blocks);
      })
      .catch(() => {
        if (!cancelled) setPreviewBlocks([]);
      });
    return () => {
      cancelled = true;
    };
  }, [board?.content]);

  const handleSaveBoard = async () => {
    if (!hostId || !workspaceId || savingBoard) return;
    setSavingBoard(true);
    try {
      await orbitActions.updateWorkspaceBoard(hostId, workspaceId, boardDraft, null);
      setEditingBoard(false);
    } finally {
      setSavingBoard(false);
    }
  };

  const handleSaveGoal = async () => {
    if (!hostId || !workspaceId) return;
    await orbitActions.updateWorkspaceGoal(hostId, workspaceId, goalDraft.trim());
    setEditingGoal(false);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Header */}
      <div className="border-border/70 flex items-center justify-between gap-2 border-b px-3.5 py-2.5">
        <div className="min-w-0">
          <div className="text-foreground truncate text-[13.5px] font-semibold">
            {workspace?.title || "Mission Workspace"}
          </div>
          <div className="text-muted-foreground flex items-center gap-2 text-[11.5px]">
            <span>
              {peers.length} {peers.length === 1 ? "peer" : "peers"}
            </span>
            {board && (
              <>
                <span>·</span>
                <span className="font-mono">BOARD.md rev {board.revision}</span>
              </>
            )}
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            title="Refresh board & activity"
            onClick={() => void orbitActions.fetchWorkspaceBoardAndEvents(hostId, workspaceId)}
            className="text-muted-foreground hover:text-foreground inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] dark:hover:bg-white/[0.07]"
          >
            <RefreshCw className="h-[13px] w-[13px]" />
          </button>
          <button
            type="button"
            onClick={() => setEditingBoard((v) => !v)}
            className="border-border hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[12px]"
          >
            {editingBoard ? (
              <>
                <Eye className="h-[12.5px] w-[12.5px]" />
                <span>Preview</span>
              </>
            ) : (
              <>
                <Edit3 className="h-[12.5px] w-[12.5px]" />
                <span>Edit</span>
              </>
            )}
          </button>
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1" viewportClassName="p-3.5 space-y-4">
        {/* 1. Mission Goal */}
        <section className="border-border/70 bg-elevated/45 space-y-1.5 rounded-xl border p-3">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground flex items-center gap-1.5 text-[11.5px] font-medium">
              <Target className="h-[12.5px] w-[12.5px]" />
              <span>Mission Goal</span>
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
            <div className="space-y-1.5">
              <input
                value={goalDraft}
                onChange={(e) => setGoalDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void handleSaveGoal();
                  if (e.key === "Escape") setEditingGoal(false);
                }}
                placeholder="Shared objective visible to all peers in this workspace…"
                className="border-border bg-canvas text-foreground w-full rounded-md border px-2 py-1 text-[12.5px] outline-none"
              />
              <div className="flex justify-end gap-1.5">
                <button
                  type="button"
                  onClick={() => setEditingGoal(false)}
                  className="text-muted-foreground hover:text-foreground cursor-pointer px-2 py-0.5 text-[11.5px]"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  onClick={() => void handleSaveGoal()}
                  className="bg-foreground text-background inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium"
                >
                  <Check className="h-[11px] w-[11px]" />
                  <span>Save</span>
                </button>
              </div>
            </div>
          ) : (
            <p className="text-foreground/85 text-[12.5px] leading-relaxed">
              {workspace?.mission_goal || "No mission goal set yet — click Edit to align all peers."}
            </p>
          )}
        </section>

        {/* 2. Live Peer Roster */}
        {peers.length > 0 && (
          <section className="space-y-1.5">
            <div className="text-muted-foreground text-[11.5px] font-medium">
              Active Peers ({peers.length})
            </div>
            <div className="space-y-1">
              {peers.map((peer) => {
                const isFocused = peer.agent_id === selectedAgentId;
                const handle = peer.handle ? `@${peer.handle}` : peer.title;
                return (
                  <div
                    key={peer.agent_id}
                    className={cn(
                      "border-border/60 flex items-center justify-between gap-2 rounded-lg border px-2.5 py-1.5 text-[12px]",
                      isFocused ? "bg-foreground/[0.05]" : "bg-elevated/35",
                    )}
                  >
                    <button
                      type="button"
                      onClick={() => orbitActions.selectAgent(hostId, peer.agent_id)}
                      className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
                    >
                      {peer.is_running ? (
                        <Loader2 className="h-[12px] w-[12px] shrink-0 animate-spin text-sky-500" />
                      ) : (
                        <ProviderIcon
                          provider={peer.provider}
                          className="h-[13px] w-[13px] shrink-0 opacity-80"
                        />
                      )}
                      <span className="text-foreground truncate font-medium">{handle}</span>
                      <span className="text-muted-foreground truncate text-[11.5px]">
                        {peer.provider_label}
                      </span>
                      {peer.worktree_branch && (
                        <span className="bg-muted text-muted-foreground inline-flex items-center gap-0.5 rounded px-1.5 py-0.5 font-mono text-[10.5px]">
                          <GitBranch className="h-[10px] w-[10px]" />
                          {peer.worktree_branch}
                        </span>
                      )}
                    </button>
                    <button
                      type="button"
                      title="Open in side-by-side split tab"
                      onClick={() => orbitActions.openPeerSplitTab(hostId, peer.agent_id, handle)}
                      className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] shrink-0 cursor-pointer items-center justify-center rounded hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
                    >
                      <Columns2 className="h-[12px] w-[12px]" />
                    </button>
                  </div>
                );
              })}
            </div>
          </section>
        )}

        {/* 3. Shared Blackboard (BOARD.md) */}
        <section className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground text-[11.5px] font-medium">
              Shared Blackboard (BOARD.md)
            </span>
            {board?.updated_by && (
              <span className="text-muted-foreground/75 text-[11px]">
                Updated by {board.updated_by}
              </span>
            )}
          </div>

          {editingBoard ? (
            <div className="space-y-2">
              <textarea
                rows={12}
                value={boardDraft}
                onChange={(e) => setBoardDraft(e.target.value)}
                placeholder="# Mission Board&#10;&#10;## Goal&#10;...&#10;&#10;## Decisions & Handoffs&#10;..."
                className="border-border bg-canvas text-foreground w-full rounded-xl border p-3 font-mono text-[12px] leading-relaxed outline-none"
              />
              <div className="flex justify-end gap-2">
                <button
                  type="button"
                  onClick={() => {
                    setBoardDraft(board?.content ?? "");
                    setEditingBoard(false);
                  }}
                  className="border-border hover:bg-accent cursor-pointer rounded-lg border px-2.5 py-1 text-[12px]"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  disabled={savingBoard}
                  onClick={() => void handleSaveBoard()}
                  className="bg-foreground text-background inline-flex cursor-pointer items-center gap-1.5 rounded-lg px-3 py-1 text-[12px] font-medium disabled:opacity-40"
                >
                  {savingBoard ? (
                    <Loader2 className="h-[12px] w-[12px] animate-spin" />
                  ) : (
                    <Save className="h-[12px] w-[12px]" />
                  )}
                  <span>Save BOARD.md</span>
                </button>
              </div>
            </div>
          ) : board?.content?.trim() ? (
            <div className="border-border/70 bg-elevated/35 rounded-xl border p-3.5 text-[13px] select-text">
              {previewBlocks.length > 0 ? (
                <MarkdownBlocksView blocks={previewBlocks} />
              ) : (
                <pre className="text-foreground/85 whitespace-pre-wrap font-mono text-[12px] leading-relaxed">
                  {board.content}
                </pre>
              )}
            </div>
          ) : (
            <div className="border-border/70 text-muted-foreground rounded-xl border border-dashed p-4 text-center text-[12.5px]">
              No notes on BOARD.md yet. Agents can write here with{" "}
              <code className="bg-muted rounded px-1 py-0.5 text-[11px]">
                workspace_board_update
              </code>
              , or click <strong>Edit</strong> above to add shared context.
            </div>
          )}
        </section>

        {/* 4. Activity & Conflict Radar Feed */}
        <section className="space-y-1.5">
          <div className="text-muted-foreground text-[11.5px] font-medium">
            Activity & Conflict Radar ({events.length})
          </div>
          {events.length === 0 ? (
            <div className="text-muted-foreground/75 text-[12px]">
              No workspace events recorded yet.
            </div>
          ) : (
            <div className="space-y-1">
              {events.slice(0, 40).map((ev) => (
                <div
                  key={ev.seq}
                  className={cn(
                    "flex items-start gap-2 rounded-lg px-2.5 py-1.5 text-[12px]",
                    ev.kind === "conflict_warning"
                      ? "border border-amber-500/35 bg-amber-500/10 text-amber-800 dark:text-amber-200"
                      : "bg-elevated/30 text-foreground/85",
                  )}
                >
                  <div className="mt-0.5">
                    <EventIcon kind={ev.kind} />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="leading-snug break-words">
                      {ev.agent_handle && (
                        <span className="text-foreground mr-1 font-medium">
                          @{ev.agent_handle}
                        </span>
                      )}
                      <span>{ev.summary}</span>
                    </div>
                    {ev.file_path && (
                      <div className="text-muted-foreground mt-0.5 truncate font-mono text-[11px]">
                        {ev.file_path}
                      </div>
                    )}
                  </div>
                  <span className="text-muted-foreground/65 shrink-0 font-mono text-[10.5px]">
                    {formatEventTime(ev.created_at)}
                  </span>
                </div>
              ))}
            </div>
          )}
        </section>
      </ScrollArea>
    </div>
  );
});
