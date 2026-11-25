import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  Brain,
  Check,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
  SettingSelect,
  useHostIsOnline,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { Textarea } from "@/components/ui/textarea";
import { useI18n } from "@/lib/i18n";
import {
  type MemoryCategory,
  type MemoryStatus,
  type ProjectMemoryItem,
  type ProjectMemorySearchHit,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { orbitActions } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const ALL_CATEGORIES: readonly MemoryCategory[] = [
  "architecture",
  "convention",
  "debugging",
  "workflow",
  "decision",
  "preference",
];

function splitCommaList(raw: string): string[] {
  return Array.from(
    new Set(
      raw
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean),
    ),
  );
}

interface MemoryRowEntry {
  item: ProjectMemoryItem;
  score: number | null;
}

const MemoryEditorDialog = memo(function MemoryEditorDialog({
  open,
  onOpenChange,
  hostId,
  activeCwd,
  editingItem,
  categoryLabel,
  onSaved,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  hostId: string;
  activeCwd: string;
  editingItem: ProjectMemoryItem | null;
  categoryLabel: (cat: MemoryCategory) => string;
  onSaved: () => Promise<void>;
}) {
  const { t } = useI18n();
  const [draftTitle, setDraftTitle] = useState(editingItem?.title ?? "");
  const [draftContent, setDraftContent] = useState(editingItem?.content ?? "");
  const [draftCategory, setDraftCategory] = useState<MemoryCategory>(
    editingItem?.category ?? "convention",
  );
  const [draftStatus, setDraftStatus] = useState<MemoryStatus>(
    editingItem?.status ?? "confirmed",
  );
  const [draftFilePaths, setDraftFilePaths] = useState(
    editingItem ? editingItem.file_paths.join(", ") : "",
  );
  const [draftTags, setDraftTags] = useState(editingItem ? editingItem.tags.join(", ") : "");
  const [saving, setSaving] = useState(false);

  const categoryOptions = useMemo(
    () => ALL_CATEGORIES.map((cat) => ({ value: cat, label: categoryLabel(cat) })),
    [categoryLabel],
  );
  const statusOptions = useMemo(
    () => [
      { value: "confirmed" as const, label: t("memory.statusConfirmed") },
      { value: "hypothesis" as const, label: t("memory.statusHypothesis") },
      { value: "needs_revalidation" as const, label: t("memory.statusNeedsRevalidation") },
    ],
    [t],
  );

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!activeCwd || !draftTitle.trim() || !draftContent.trim()) return;
    setSaving(true);
    try {
      await orbitIpc.upsertProjectMemory({
        host_id: hostId,
        cwd: activeCwd,
        id: editingItem?.id ?? null,
        title: draftTitle.trim(),
        content: draftContent.trim(),
        category: draftCategory,
        status: draftStatus,
        file_paths: splitCommaList(draftFilePaths),
        tags: splitCommaList(draftTags),
      });
      onOpenChange(false);
      orbitActions.showToast(t("memory.savedToast"));
      await onSaved();
    } catch (err) {
      orbitActions.showToast(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[520px]">
        <DialogHeader>
          <DialogTitle>
            {editingItem ? t("memory.editDialogTitle") : t("memory.createDialogTitle")}
          </DialogTitle>
          <DialogDescription>{activeCwd}</DialogDescription>
        </DialogHeader>
        <form onSubmit={(e) => void handleSubmit(e)} className="flex flex-col gap-4 pt-1">
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="memory-title">{t("memory.fieldTitle")}</FieldLabel>
              <Input
                id="memory-title"
                value={draftTitle}
                onChange={(e) => setDraftTitle(e.target.value)}
                placeholder={t("memory.fieldTitlePlaceholder")}
                required
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="memory-content">{t("memory.fieldContent")}</FieldLabel>
              <Textarea
                id="memory-content"
                rows={4}
                value={draftContent}
                onChange={(e) => setDraftContent(e.target.value)}
                placeholder={t("memory.fieldContentPlaceholder")}
                required
              />
            </Field>
            <div className="grid grid-cols-2 gap-3">
              <Field>
                <FieldLabel>{t("memory.fieldCategory")}</FieldLabel>
                <SettingSelect<MemoryCategory>
                  align="start"
                  value={draftCategory}
                  onChange={setDraftCategory}
                  options={categoryOptions}
                />
              </Field>
              <Field>
                <FieldLabel>{t("memory.fieldStatus")}</FieldLabel>
                <SettingSelect<MemoryStatus>
                  align="start"
                  value={draftStatus}
                  onChange={setDraftStatus}
                  options={statusOptions}
                />
              </Field>
            </div>
            <Field>
              <FieldLabel htmlFor="memory-files">{t("memory.fieldFiles")}</FieldLabel>
              <Input
                id="memory-files"
                value={draftFilePaths}
                onChange={(e) => setDraftFilePaths(e.target.value)}
                placeholder="src/auth.rs, docs/spec.md"
                className="font-mono text-[13px]"
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="memory-tags">{t("memory.fieldTags")}</FieldLabel>
              <Input
                id="memory-tags"
                value={draftTags}
                onChange={(e) => setDraftTags(e.target.value)}
                placeholder="rust, security"
              />
            </Field>
          </FieldGroup>
          <div className="flex justify-end gap-2 pt-1">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={saving}
              onClick={() => onOpenChange(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"
              size="sm"
              disabled={saving || !draftTitle.trim() || !draftContent.trim()}
            >
              {saving && <OrbitSpinner size={12} />}
              {t("common.save")}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
});

export const HostMemorySection = memo(function HostMemorySection({
  hostId,
  cwd: activeCwd,
}: {
  hostId: string;
  cwd: string;
}) {
  const { t } = useI18n();
  const isOnline = useHostIsOnline(hostId);

  const [searchQuery, setSearchQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<MemoryStatus | "all">("all");
  const [categoryFilter, setCategoryFilter] = useState<MemoryCategory | "all">("all");
  const [entries, setEntries] = useState<MemoryRowEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ProjectMemoryItem | null>(null);

  // Create / Edit Dialog target (form keystrokes live isolated inside MemoryEditorDialog)
  const [editorState, setEditorState] = useState<{
    open: boolean;
    item: ProjectMemoryItem | null;
    nonce: number;
  }>({ open: false, item: null, nonce: 0 });

  const request = useRef(0);
  const loadMemories = useCallback(async () => {
    const seq = ++request.current;
    if (!activeCwd) {
      setEntries([]);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const q = searchQuery.trim();
      if (q) {
        const hits: ProjectMemorySearchHit[] = await orbitIpc.recallProjectMemory(
          hostId,
          activeCwd,
          q,
          { limit: 50, includeUnconfirmed: true },
        );
        const filtered = hits
          .filter(
            (h) =>
              (statusFilter === "all" || h.item.status === statusFilter) &&
              (categoryFilter === "all" || h.item.category === categoryFilter),
          )
          .map((h) => ({ item: h.item, score: h.score }));
        if (request.current === seq) setEntries(filtered);
      } else {
        const items = await orbitIpc.listProjectMemories(hostId, activeCwd, {
          status: statusFilter === "all" ? null : statusFilter,
          category: categoryFilter === "all" ? null : categoryFilter,
          limit: 100,
        });
        if (request.current === seq) setEntries(items.map((item) => ({ item, score: null })));
      }
    } catch (err) {
      if (seq === request.current) setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (seq === request.current) setLoading(false);
    }
  }, [activeCwd, categoryFilter, hostId, searchQuery, statusFilter]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void loadMemories();
    }, searchQuery.trim() ? 180 : 0);
    return () => {
      request.current += 1;
      window.clearTimeout(timer);
    };
  }, [loadMemories, searchQuery]);

  const openCreateDialog = useCallback(() => {
    setEditorState((prev) => ({ open: true, item: null, nonce: prev.nonce + 1 }));
  }, []);

  const openEditDialog = useCallback((item: ProjectMemoryItem) => {
    setEditorState((prev) => ({ open: true, item, nonce: prev.nonce + 1 }));
  }, []);

  const handleEditorOpenChange = useCallback((open: boolean) => {
    setEditorState((prev) => (prev.open === open ? prev : { ...prev, open }));
  }, []);

  const handleConfirmMemory = useCallback(
    async (item: ProjectMemoryItem) => {
      if (!activeCwd) return;
      setConfirmingId(item.id);
      try {
        const updated = await orbitIpc.confirmProjectMemory(hostId, activeCwd, item.id);
        setEntries((prev) =>
          prev.map((row) => (row.item.id === updated.id ? { ...row, item: updated } : row)),
        );
        orbitActions.showToast(t("memory.confirmedToast"));
      } catch (err) {
        orbitActions.showToast(err instanceof Error ? err.message : String(err));
      } finally {
        setConfirmingId(null);
      }
    },
    [activeCwd, hostId, t],
  );

  const handleDeleteMemory = useCallback(async () => {
    if (!activeCwd || !deleteTarget) return;
    await orbitIpc.deleteProjectMemory(hostId, activeCwd, deleteTarget.id);
    setEntries((prev) => prev.filter((row) => row.item.id !== deleteTarget.id));
    setDeleteTarget(null);
    orbitActions.showToast(t("memory.deletedToast"));
  }, [activeCwd, deleteTarget, hostId, t]);

  const categoryLabel = useCallback(
    (cat: MemoryCategory): string => {
      switch (cat) {
        case "architecture":
          return t("memory.catArchitecture");
        case "convention":
          return t("memory.catConvention");
        case "debugging":
          return t("memory.catDebugging");
        case "workflow":
          return t("memory.catWorkflow");
        case "decision":
          return t("memory.catDecision");
        case "preference":
          return t("memory.catPreference");
      }
    },
    [t],
  );

  const statusLabel = (st: MemoryStatus): string => {
    switch (st) {
      case "confirmed":
        return t("memory.statusConfirmed");
      case "hypothesis":
        return t("memory.statusHypothesis");
      case "needs_revalidation":
        return t("memory.statusNeedsRevalidation");
    }
  };

  const revalidationCount = entries.filter(
    (row) => row.item.status === "needs_revalidation",
  ).length;

  return (
    <div data-setting-id="memory.title" className="flex flex-col gap-6">
      <SettingSectionHeader
        subpage
        title={t("memory.title")}
        description={t("memory.subtitle")}
        badge={
          revalidationCount > 0 ? (
            <Badge
              variant="outline"
              className="border-signal-warning/40 bg-signal-warning/10 text-signal-warning gap-1 text-[11px]"
            >
              <AlertTriangle className="size-3" />
              {t("memory.revalidationBadge", { count: revalidationCount })}
            </Badge>
          ) : undefined
        }
        action={
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={loading || !isOnline || !activeCwd}
              onClick={() => void loadMemories()}
            >
              <RefreshCw data-icon="inline-start" className={loading ? "animate-spin" : undefined} />
              {t("common.refresh")}
            </Button>
            <Button
              size="sm"
              disabled={!isOnline || !activeCwd}
              onClick={openCreateDialog}
            >
              <Plus data-icon="inline-start" />
              {t("memory.addMemory")}
            </Button>
          </div>
        }
      />

      {error && (
        <div className="border-signal-danger/30 bg-signal-danger/8 text-signal-danger rounded-[10px] border px-3.5 py-2.5 text-[13px]">
          {error}
        </div>
      )}

      {/* Recall and filters */}
      <SettingCard>
        <div className="flex flex-col gap-3 px-4 py-3.5">
          <div data-setting-id="memory.searchPlaceholder" className="relative flex-1">
            <Search className="text-ink-faint pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2" />
            <Input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder={t("memory.searchPlaceholder")}
              disabled={!activeCwd || !isOnline}
              className="pl-8 text-[13px]"
            />
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <SettingSelect<MemoryStatus | "all">
              align="start"
              value={statusFilter}
              onChange={setStatusFilter}
              options={[
                { value: "all", label: t("memory.filterAllStatuses") },
                { value: "needs_revalidation", label: t("memory.statusNeedsRevalidation") },
                { value: "confirmed", label: t("memory.statusConfirmed") },
                { value: "hypothesis", label: t("memory.statusHypothesis") },
              ]}
            />
            <SettingSelect<MemoryCategory | "all">
              align="start"
              value={categoryFilter}
              onChange={setCategoryFilter}
              options={[
                { value: "all", label: t("memory.filterAllCategories") },
                ...ALL_CATEGORIES.map((cat) => ({
                  value: cat,
                  label: categoryLabel(cat),
                })),
              ]}
            />
          </div>
        </div>
      </SettingCard>

      {/* Memory Items */}
      <div className="flex flex-col gap-2">
        <SettingGroupLabel>
          {t("memory.listHeading", { count: entries.length })}
        </SettingGroupLabel>

        {!activeCwd ? (
          <SettingCard>
            <Empty className="border-0 py-8">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <Brain />
                </EmptyMedia>
                <EmptyTitle>{t("memory.noWorkspaceTitle")}</EmptyTitle>
                <EmptyDescription>{t("memory.noWorkspaceDesc")}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          </SettingCard>
        ) : loading && entries.length === 0 ? (
          <SettingCard>
            <div className="text-ink-muted flex items-center gap-2.5 px-4 py-6 text-[13px]">
              <OrbitSpinner size={14} />
              <span>{t("common.loading")}</span>
            </div>
          </SettingCard>
        ) : entries.length === 0 ? (
          <SettingCard>
            <Empty className="border-0 py-8">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <Brain />
                </EmptyMedia>
                <EmptyTitle>{t("memory.emptyTitle")}</EmptyTitle>
                <EmptyDescription>{t("memory.emptyDesc")}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          </SettingCard>
        ) : (
          <div className="flex flex-col gap-2.5">
            {entries.map(({ item, score }) => {
              const needsRevalidation = item.status === "needs_revalidation";
              const isConfirming = confirmingId === item.id;
              return (
                <div
                  key={item.id}
                  className={cn(
                    "bg-surface-raised flex flex-col gap-2.5 rounded-[12px] border px-4 py-3.5 transition-colors",
                    needsRevalidation
                      ? "border-signal-warning/45 bg-signal-warning/[0.035]"
                      : "border-border-subtle",
                  )}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0 flex-1 space-y-1">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <span className="text-ink-primary text-[13px] font-semibold">
                          {item.title}
                        </span>
                        <Badge variant="outline" className="text-[11px]">
                          {categoryLabel(item.category)}
                        </Badge>
                        <Badge
                          variant="outline"
                          className={cn(
                            "text-[11px]",
                            needsRevalidation
                              ? "border-signal-warning/45 bg-signal-warning/10 text-signal-warning"
                              : item.status === "confirmed"
                                ? "border-signal-success/35 bg-signal-success/10 text-signal-success"
                                : "text-ink-muted",
                          )}
                        >
                          {statusLabel(item.status)}
                        </Badge>
                        {score !== null && (
                          <span className="text-ink-faint font-mono text-[11px] tabular-nums">
                            {score.toFixed(2)}
                          </span>
                        )}
                      </div>
                      <p className="text-ink-secondary text-[13px] leading-[1.5] whitespace-pre-wrap">
                        {item.content}
                      </p>
                    </div>

                    <div className="flex shrink-0 items-center gap-1.5">
                      {item.status !== "confirmed" && (
                        <Button
                          variant={needsRevalidation ? "default" : "outline"}
                          size="xs"
                          disabled={isConfirming || !isOnline}
                          onClick={() => void handleConfirmMemory(item)}
                        >
                          {isConfirming ? (
                            <OrbitSpinner size={11} />
                          ) : (
                            <Check data-icon="inline-start" />
                          )}
                          {t("memory.confirmButton")}
                        </Button>
                      )}
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        aria-label={t("common.edit")}
                        disabled={!isOnline}
                        onClick={() => openEditDialog(item)}
                      >
                        <Pencil className="text-ink-muted" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        aria-label={t("common.delete")}
                        disabled={!isOnline}
                        onClick={() => setDeleteTarget(item)}
                      >
                        <Trash2 className="text-ink-muted hover:text-signal-danger" />
                      </Button>
                    </div>
                  </div>

                  {(item.file_paths.length > 0 || item.tags.length > 0) && (
                    <div className="flex flex-wrap items-center gap-1.5 pt-0.5">
                      {item.file_paths.map((fp) => (
                        <Badge
                          key={`fp:${fp}`}
                          variant="secondary"
                          className="font-mono text-[11px]"
                        >
                          {fp}
                        </Badge>
                      ))}
                      {item.tags.map((tag) => (
                        <Badge key={`tag:${tag}`} variant="outline" className="text-[11px]">
                          #{tag}
                        </Badge>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Create / Edit Project Memory Dialog (isolated form state) */}
      {editorState.open && (
        <MemoryEditorDialog
          key={`${editorState.item?.id ?? "new"}:${editorState.nonce}`}
          open={editorState.open}
          onOpenChange={handleEditorOpenChange}
          hostId={hostId}
          activeCwd={activeCwd}
          editingItem={editorState.item}
          categoryLabel={categoryLabel}
          onSaved={loadMemories}
        />
      )}

      <ConfirmDialog
        open={Boolean(deleteTarget)}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
        title={t("memory.deleteConfirmTitle")}
        description={
          deleteTarget ? t("memory.deleteConfirmDesc", { title: deleteTarget.title }) : ""
        }
        confirmLabel={t("common.delete")}
        tone="danger"
        defaultAction="cancel"
        onConfirm={handleDeleteMemory}
      />
    </div>
  );
});
