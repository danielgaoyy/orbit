import { useMemo, useState } from "react";
import { Archive, RotateCcw, Trash2 } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingResourceRow,
  SettingSelect,
} from "@/components/settings/settings-primitives";
import { useSidebarRelativeClock } from "@/components/sidebar/sidebar-primitives";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Empty, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { useI18n } from "@/lib/i18n";
import { basenameOfProjectPath, matchesProjectDirectory } from "@/lib/projects";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";

interface ArchivedItem {
  agentId: string;
  project: string;
  title: string;
  archivedAtMs: number;
}

const BUCKETS = ["dataOps.today", "dataOps.thisWeek", "dataOps.earlier"] as const;

export function ArchivedSection({ hostId }: { hostId: string | null }) {
  const { locale, t } = useI18n();
  const detail = useOrbitSelector((s) => (hostId ? s.hostDetails[hostId] : null));
  const sidebarView = useOrbitSelector((s) => s.sidebarView);
  const archived = useOrbitSelector((s) => s.archivedThreads);
  const titles = useOrbitSelector((s) => s.threadCustomTitles);
  const [query, setQuery] = useState("");
  const [project, setProject] = useState<string | null>(null);
  const [toDelete, setToDelete] = useState<ArchivedItem[] | null>(null);
  const nowMs = useSidebarRelativeClock(true);

  const entries = useMemo(() => {
    if (!hostId || !detail) return [];
    const records: ArchivedItem[] = [];
    const seen = new Set<string>();
    const projects = sidebarView?.projects.filter((p) => p.host_id === hostId) ?? [];
    for (const agent of detail.agents) {
      if (agent.background || seen.has(agent.agent_id)) continue;
      const key = threadScopeKey(hostId, agent.agent_id);
      if (!agent.archived_at_rfc3339 && !archived[key]) continue;
      seen.add(agent.agent_id);
      records.push({
        agentId: agent.agent_id,
        project:
          projects.find((p) => matchesProjectDirectory(p.directories, agent.cwd))?.name ||
          basenameOfProjectPath(agent.cwd),
        title: titles[key] || agent.title,
        archivedAtMs:
          Date.parse(agent.archived_at_rfc3339 ?? "") || archived[key]?.archivedAtMs || 0,
      });
    }
    return records.sort((a, b) => b.archivedAtMs - a.archivedAtMs);
  }, [hostId, detail, archived, titles, sidebarView]);

  const projects = [...new Set(entries.map((item) => item.project))].sort();
  const selectedProject = project !== null && projects.includes(project) ? project : null;
  const today = new Date(nowMs);
  today.setHours(0, 0, 0, 0);
  const weekStart = new Date(today);
  weekStart.setDate(weekStart.getDate() - 6);
  const filtered = entries.filter((item) => {
    const needle = query.trim().toLowerCase();
    return (
      (selectedProject === null || item.project === selectedProject) &&
      (item.title.toLowerCase().includes(needle) || item.project.toLowerCase().includes(needle))
    );
  });
  const grouped: ArchivedItem[][] = [[], [], []];
  for (const item of filtered) {
    grouped[
      item.archivedAtMs >= today.getTime() ? 0 : item.archivedAtMs >= weekStart.getTime() ? 1 : 2
    ].push(item);
  }
  const date = new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" });

  return (
    <section data-setting-id="dataOps.archive" className="flex flex-col gap-2">
      <SettingGroupLabel
        action={
          entries.length > 0 && (
            <Button variant="ghost" size="sm" onClick={() => setToDelete(entries)}>
              {t("dataOps.emptyArchive")}
            </Button>
          )
        }
      >
        {t("dataOps.archive")} · {entries.length}
      </SettingGroupLabel>
      {entries.length > 0 && (
        <div className="flex items-center gap-2">
          <Input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("dataOps.archiveSearch")}
            aria-label={t("dataOps.archiveSearch")}
            className="flex-1 text-[13px]"
          />
          {projects.length > 1 && (
            <SettingSelect
              value={selectedProject === null ? 0 : projects.indexOf(selectedProject) + 1}
              ariaLabel={t("permissionRules.scopeProject")}
              options={[
                { value: 0, label: t("sidebar.filterAll") },
                ...projects.map((label, i) => ({ value: i + 1, label })),
              ]}
              onChange={(id) => setProject(projects[id - 1] ?? null)}
            />
          )}
        </div>
      )}
      {filtered.length === 0 && (
        <SettingCard>
          <Empty className="gap-2 py-6">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Archive />
              </EmptyMedia>
              <EmptyTitle className="text-[13px]">
                {t(entries.length ? "sidebar.noMatchingThreads" : "dataOps.noArchive")}
              </EmptyTitle>
            </EmptyHeader>
            {entries.length > 0 && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setQuery("");
                  setProject(null);
                }}
              >
                {t("sidebar.clearThreadFilter")}
              </Button>
            )}
          </Empty>
        </SettingCard>
      )}
      {hostId &&
        grouped.map(
          (items, index) =>
            items.length > 0 && (
              <div key={BUCKETS[index]} className="flex flex-col gap-1.5">
                <span className="text-ink-muted text-[11px]">{t(BUCKETS[index])}</span>
                <SettingCard>
                  {items.map((item) => (
                    <SettingResourceRow
                      key={item.agentId}
                      title={item.title}
                      description={[
                        item.project,
                        item.archivedAtMs ? date.format(item.archivedAtMs) : "",
                      ]
                        .filter(Boolean)
                        .join(" · ")}
                      onClick={() =>
                        orbitActions.restoreArchivedThread(hostId, item.agentId, { open: true })
                      }
                      action={
                        <div className="flex items-center gap-1">
                          <Button
                            variant="outline"
                            size="sm"
                            onClick={() =>
                              orbitActions.restoreArchivedThread(hostId, item.agentId, {
                                open: false,
                              })
                            }
                          >
                            <RotateCcw data-icon="inline-start" />
                            {t("dataOps.restore")}
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            aria-label={t("dataOps.deleteThread", { name: item.title })}
                            onClick={() => setToDelete([item])}
                          >
                            <Trash2 />
                          </Button>
                        </div>
                      }
                    />
                  ))}
                </SettingCard>
              </div>
            ),
        )}
      <ConfirmDialog
        open={toDelete !== null}
        onOpenChange={(open) => {
          if (!open) setToDelete(null);
        }}
        tone="danger"
        confirmLabel={t("common.delete")}
        title={
          toDelete?.length === 1
            ? t("dataOps.deleteThread", { name: toDelete[0].title })
            : t("dataOps.emptyArchive")
        }
        description={t("dataOps.deleteConfirm", { count: toDelete?.length ?? 0 })}
        onConfirm={async () => {
          if (!hostId || !toDelete) return;
          if (toDelete.length === 1) {
            await orbitActions.deleteThreadPermanently(hostId, toDelete[0].agentId);
            return;
          }
          await orbitActions.deleteArchivedThreadsBatch(
            toDelete.map(({ agentId }) => ({ hostId, agentId })),
          );
        }}
      />
    </section>
  );
}
