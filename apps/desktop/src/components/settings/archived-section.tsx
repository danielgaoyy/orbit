import { useEffect, useMemo, useState } from "react";
import { Archive, ArrowUpRight, RotateCcw, Search, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import {
  HostStatusIndicator,
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
import {
  formatCompactRelativeTime,
  useSidebarRelativeClock,
} from "@/components/sidebar/sidebar-primitives";
import { type AgentCardView, type HostCardView } from "@/lib/orbit-ipc";
import {
  basenameOfProjectPath,
  isLocalHostCard,
  matchesProjectDirectory,
} from "@/lib/projects";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface ArchivedItem {
  key: string;
  hostId: string;
  host: HostCardView | null;
  agent: AgentCardView;
  projectName: string;
  effectiveTitle: string;
  archivedAtMs: number;
}

type TimeBucket = "Today" | "This Week" | "Earlier";

function classifyTimeBucket(tsMs: number, nowMs: number): TimeBucket {
  const startOfToday = new Date(nowMs);
  startOfToday.setHours(0, 0, 0, 0);
  if (tsMs >= startOfToday.getTime()) return "Today";
  const sevenDaysAgo = startOfToday.getTime() - 6 * 24 * 60 * 60 * 1000;
  if (tsMs >= sevenDaysAgo) return "This Week";
  return "Earlier";
}

function formatArchivedAgo(tsMs: number, nowMs: number): string {
  const label = formatCompactRelativeTime(new Date(tsMs).toISOString(), nowMs);
  if (!label) return "";
  return label === "now" ? "just now" : `${label} ago`;
}

export function ArchivedSection() {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const sidebarView = useOrbitSelector((s) => s.sidebarView);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const threadCustomTitles = useOrbitSelector((s) => s.threadCustomTitles);

  const [query, setQuery] = useState("");
  const [selectedProject, setSelectedProject] = useState<string>("__all__");
  const [confirmDeleteKey, setConfirmDeleteKey] = useState<string | null>(null);
  const [emptyDialogOpen, setEmptyDialogOpen] = useState(false);
  const [emptying, setEmptying] = useState(false);

  const nowMs = useSidebarRelativeClock(true);
  const multiHostMode = fleet.hosts.length > 1;

  // Auto-reset inline 2-step delete confirmation after 3s
  useEffect(() => {
    if (!confirmDeleteKey) return;
    const id = window.setTimeout(() => setConfirmDeleteKey(null), 3000);
    return () => window.clearTimeout(id);
  }, [confirmDeleteKey]);

  const hostById = useMemo(
    () => new Map(fleet.hosts.map((h) => [h.host_id, h])),
    [fleet.hosts],
  );

  const rawProjects = sidebarView?.projects ?? [];

  const allEntries = useMemo<ArchivedItem[]>(() => {
    const seen = new Set<string>();
    const entries: ArchivedItem[] = [];

    for (const [hostId, detail] of Object.entries(hostDetails)) {
      const host = hostById.get(hostId) ?? null;
      for (const ag of detail.agents) {
        if (ag.background) continue;
        const tKey = threadScopeKey(hostId, ag.agent_id);
        const localMeta = archivedThreads[tKey];
        if (!ag.archived_at_rfc3339 && !localMeta) continue;
        if (seen.has(tKey)) continue;
        seen.add(tKey);

        const matchedProj = rawProjects.find(
          (p) => p.host_id === hostId && matchesProjectDirectory(p.directories, ag.cwd),
        );
        const archivedAtMs = ag.archived_at_rfc3339
          ? Date.parse(ag.archived_at_rfc3339) || Date.now()
          : (localMeta?.archivedAtMs ?? Date.now());

        entries.push({
          key: tKey,
          hostId,
          host,
          agent: ag,
          projectName: matchedProj?.name || basenameOfProjectPath(ag.cwd),
          effectiveTitle: threadCustomTitles[tKey] || ag.title,
          archivedAtMs,
        });
      }
    }

    entries.sort((a, b) => b.archivedAtMs - a.archivedAtMs);
    return entries;
  }, [archivedThreads, hostById, hostDetails, rawProjects, threadCustomTitles]);

  const projectCounts = useMemo(() => {
    const map = new Map<string, number>();
    for (const item of allEntries) {
      map.set(item.projectName, (map.get(item.projectName) ?? 0) + 1);
    }
    return Array.from(map.entries()).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  }, [allEntries]);

  // Reset selected project pill if it no longer exists
  useEffect(() => {
    if (
      selectedProject !== "__all__" &&
      !projectCounts.some(([name]) => name === selectedProject)
    ) {
      setSelectedProject("__all__");
    }
  }, [projectCounts, selectedProject]);

  const filteredEntries = useMemo(() => {
    const q = query.trim().toLowerCase();
    return allEntries.filter((item) => {
      if (selectedProject !== "__all__" && item.projectName !== selectedProject) {
        return false;
      }
      if (!q) return true;
      return (
        item.effectiveTitle.toLowerCase().includes(q) ||
        item.projectName.toLowerCase().includes(q) ||
        (item.host?.display_name.toLowerCase().includes(q) ?? false)
      );
    });
  }, [allEntries, query, selectedProject]);

  const groupedEntries = useMemo(() => {
    const buckets: Record<TimeBucket, ArchivedItem[]> = {
      Today: [],
      "This Week": [],
      Earlier: [],
    };
    for (const item of filteredEntries) {
      buckets[classifyTimeBucket(item.archivedAtMs, nowMs)].push(item);
    }
    return (["Today", "This Week", "Earlier"] as TimeBucket[])
      .map((label) => ({ label, items: buckets[label] }))
      .filter((g) => g.items.length > 0);
  }, [filteredEntries, nowMs]);

  const handleEmptyArchive = async () => {
    if (emptying || allEntries.length === 0) return;
    setEmptying(true);
    try {
      await orbitActions.deleteArchivedThreadsBatch(
        allEntries.map((item) => ({ hostId: item.hostId, agentId: item.agent.agent_id })),
      );
      setEmptyDialogOpen(false);
    } finally {
      setEmptying(false);
    }
  };

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Archived"
        description="Restore archived conversations back to your sidebar, or permanently remove them."
        badge={
          allEntries.length > 0 ? (
            <span className="bg-muted text-muted-foreground rounded-full px-2 py-0.5 text-[11.5px] font-medium tabular-nums">
              {allEntries.length}
            </span>
          ) : undefined
        }
        action={
          allEntries.length > 0 ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => setEmptyDialogOpen(true)}
              className="text-muted-foreground hover:text-rose-600 dark:hover:text-rose-400"
            >
              Empty Archive…
            </Button>
          ) : undefined
        }
      />

      {allEntries.length === 0 ? (
        <SettingCard>
          <div className="flex flex-col items-center justify-center px-6 py-12 text-center">
            <span className="bg-muted/70 text-muted-foreground mb-3 inline-flex h-10 w-10 items-center justify-center rounded-full">
              <Archive className="h-[18px] w-[18px] stroke-[1.75]" />
            </span>
            <p className="text-foreground text-[13.5px] font-medium">No archived conversations</p>
            <p className="text-muted-foreground mt-1 max-w-xs text-[12.5px] leading-relaxed">
              Archive completed conversations from the sidebar to keep your active workspace clean.
            </p>
          </div>
        </SettingCard>
      ) : (
        <>
          {/* Search & Project Filter Bar */}
          <div className="space-y-2.5">
            <div className="relative">
              <Search className="text-ink-faint pointer-events-none absolute top-1/2 left-3 h-[14px] w-[14px] -translate-y-1/2" />
              <Input
                type="text"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search archived conversations or projects…"
                className="h-[34px] rounded-[9px] pr-8 pl-9 text-[13px]"
              />
              {query && (
                <button
                  type="button"
                  aria-label="Clear search"
                  onClick={() => setQuery("")}
                  className="text-muted-foreground hover:text-foreground absolute top-1/2 right-2 inline-flex h-[20px] w-[20px] -translate-y-1/2 cursor-pointer items-center justify-center rounded-md"
                >
                  <X className="h-[13px] w-[13px]" />
                </button>
              )}
            </div>

            {projectCounts.length > 1 && (
              <div className="flex flex-wrap items-center gap-1.5">
                <button
                  type="button"
                  onClick={() => setSelectedProject("__all__")}
                  className={cn(
                    "inline-flex h-[25px] cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[12px] transition-colors",
                    selectedProject === "__all__"
                      ? "bg-foreground text-background font-medium"
                      : "bg-muted/70 text-muted-foreground hover:text-foreground",
                  )}
                >
                  <span>All</span>
                  <span className="tabular-nums opacity-70">{allEntries.length}</span>
                </button>
                {projectCounts.map(([name, count]) => (
                  <button
                    key={name}
                    type="button"
                    onClick={() => setSelectedProject(name)}
                    className={cn(
                      "inline-flex h-[25px] max-w-[180px] cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[12px] transition-colors",
                      selectedProject === name
                        ? "bg-foreground text-background font-medium"
                        : "bg-muted/70 text-muted-foreground hover:text-foreground",
                    )}
                  >
                    <span className="truncate">{name}</span>
                    <span className="shrink-0 tabular-nums opacity-70">{count}</span>
                  </button>
                ))}
              </div>
            )}
          </div>

          {groupedEntries.length === 0 ? (
            <SettingCard>
              <div className="px-4 py-8 text-center">
                <p className="text-muted-foreground text-[13px]">
                  No archived conversations match your filter.
                </p>
                <button
                  type="button"
                  onClick={() => {
                    setQuery("");
                    setSelectedProject("__all__");
                  }}
                  className="text-foreground/80 hover:text-foreground mt-2 cursor-pointer text-[12.5px] font-medium underline underline-offset-4"
                >
                  Clear filters
                </button>
              </div>
            </SettingCard>
          ) : (
            <div className="space-y-5">
              {groupedEntries.map((group) => (
                <div key={group.label} className="space-y-2">
                  <SettingGroupLabel>{group.label}</SettingGroupLabel>
                  <SettingCard>
                    {group.items.map((item) => {
                      const isConfirming = confirmDeleteKey === item.key;
                      const showRemoteHost =
                        multiHostMode && item.host && !isLocalHostCard(item.host);
                      const archivedAgo = formatArchivedAgo(item.archivedAtMs, nowMs);

                      return (
                        <div
                          key={item.key}
                          className="group/row flex items-center justify-between gap-3 px-4 py-3 transition-colors hover:bg-black/[0.02] dark:hover:bg-white/[0.025]"
                        >
                          <button
                            type="button"
                            onClick={() => {
                              orbitActions.restoreArchivedThread(item.hostId, item.agent.agent_id, {
                                open: true,
                              });
                            }}
                            title="Restore and open conversation"
                            className="group/title flex min-w-0 flex-1 cursor-pointer flex-col items-start gap-0.5 text-left"
                          >
                            <span className="flex w-full min-w-0 items-center gap-1.5">
                              <span className="text-foreground truncate text-[13.5px] font-medium group-hover/title:underline group-hover/title:underline-offset-2">
                                {item.effectiveTitle}
                              </span>
                              <ArrowUpRight className="text-muted-foreground/60 h-[13px] w-[13px] shrink-0 opacity-0 transition-opacity group-hover/title:opacity-100" />
                            </span>
                            <span className="text-muted-foreground flex max-w-full items-center gap-1.5 text-[12px]">
                              <span className="truncate">{item.projectName}</span>
                              {showRemoteHost && item.host && (
                                <>
                                  <span className="opacity-45">·</span>
                                  <span className="inline-flex shrink-0 items-center gap-1">
                                    <HostStatusIndicator state={item.host.connection_state} />
                                    <span className="max-w-[110px] truncate">
                                      {item.host.display_name}
                                    </span>
                                  </span>
                                </>
                              )}
                              {archivedAgo && (
                                <>
                                  <span className="opacity-45">·</span>
                                  <span className="shrink-0 tabular-nums">
                                    Archived {archivedAgo}
                                  </span>
                                </>
                              )}
                            </span>
                          </button>

                          <div className="flex shrink-0 items-center gap-1.5">
                            <button
                              type="button"
                              onMouseDown={(e) => e.preventDefault()}
                              onClick={() => {
                                setConfirmDeleteKey(null);
                                orbitActions.restoreArchivedThread(
                                  item.hostId,
                                  item.agent.agent_id,
                                  { open: false },
                                );
                              }}
                              className="border-border/80 bg-background/80 text-foreground/85 hover:bg-accent hover:text-foreground inline-flex h-[27px] cursor-pointer items-center gap-1.5 rounded-[7px] border px-2.5 text-[12px] font-medium transition-colors"
                            >
                              <RotateCcw className="h-[12px] w-[12px] stroke-[1.8]" />
                              <span>Restore</span>
                            </button>

                            {isConfirming ? (
                              <button
                                type="button"
                                onClick={() => {
                                  setConfirmDeleteKey(null);
                                  void orbitActions.deleteThreadPermanently(
                                    item.hostId,
                                    item.agent.agent_id,
                                  );
                                }}
                                onBlur={() => setConfirmDeleteKey(null)}
                                autoFocus
                                className="inline-flex h-[27px] cursor-pointer items-center rounded-[7px] bg-rose-600 px-2.5 text-[12px] font-medium text-white transition-colors hover:bg-rose-700 dark:bg-rose-500 dark:hover:bg-rose-600"
                              >
                                Delete?
                              </button>
                            ) : (
                              <button
                                type="button"
                                title="Delete permanently"
                                aria-label={`Delete ${item.effectiveTitle} permanently`}
                                onClick={() => setConfirmDeleteKey(item.key)}
                                className="text-muted-foreground inline-flex h-[27px] w-[27px] cursor-pointer items-center justify-center rounded-[7px] transition-colors hover:bg-rose-500/10 hover:text-rose-600 dark:hover:text-rose-400"
                              >
                                <Trash2 className="h-[13.5px] w-[13.5px] stroke-[1.75]" />
                              </button>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </SettingCard>
                </div>
              ))}
            </div>
          )}
        </>
      )}

      {/* Empty Archive Confirmation Dialog */}
      <Dialog
        open={emptyDialogOpen}
        onOpenChange={(open) => {
          if (!open && !emptying) setEmptyDialogOpen(false);
        }}
      >
        <DialogContent className="max-w-md gap-4 p-5 select-none">
          <DialogHeader className="gap-1.5 pr-6">
            <DialogTitle className="text-[15.5px]">Empty Archive</DialogTitle>
            <DialogDescription className="text-foreground/80 text-[13.5px] leading-relaxed">
              Permanently delete{" "}
              <strong className="text-foreground font-semibold">
                {allEntries.length === 1
                  ? "1 archived conversation"
                  : `${allEntries.length} archived conversations`}
              </strong>
              ? This action cannot be undone.
            </DialogDescription>
          </DialogHeader>

          <p className="text-muted-foreground text-[12.5px]">
            Project files on disk will not be affected.
          </p>

          <div className="flex justify-end gap-2.5 pt-1">
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={emptying}
              onClick={() => setEmptyDialogOpen(false)}
            >
              Cancel
            </Button>
            <Button
              type="button"
              variant="destructive"
              size="sm"
              autoFocus
              disabled={emptying}
              onClick={() => void handleEmptyArchive()}
            >
              {emptying ? "Deleting…" : "Delete All"}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
