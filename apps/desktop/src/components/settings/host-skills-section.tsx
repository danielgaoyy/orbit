import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  BookOpen,
  Code2,
  GitFork,
  Import,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import {
  SettingCard,
  SettingDrawer,
  SettingGroupLabel,
  SettingResourceRow,
  SettingSelect,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { OrchestrationSkills } from "@/components/settings/orchestration-skills";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Field, FieldLabel, FieldGroup } from "@/components/ui/field";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { Input } from "@/components/ui/input";
import { ShortcutKbd } from "@/components/ui/kbd";
import { Textarea } from "@/components/ui/textarea";
import { isImeKeyEvent } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import {
  type ManagedSkillRecord,
  type OrchestrationSkillInfo,
  type SkillSource,
  type SkillWriteScope,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { t, useLocale, type MessageKey } from "@/lib/i18n";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

type SkillSourceFilter = "all" | "user" | "project" | "claude" | "codex" | "agy" | "builtin";

const EMPTY_BUNDLES: readonly OrchestrationSkillInfo[] = [];
const EMPTY_SKILLS: readonly ManagedSkillRecord[] = [];

const SOURCE_FILTERS: readonly SkillSourceFilter[] = [
  "all",
  "user",
  "project",
  "claude",
  "codex",
  "agy",
  "builtin",
];
const SOURCE_LABELS: Record<SkillSourceFilter, MessageKey> = {
  all: "sidebar.filterAll",
  user: "managedSkills.user",
  project: "permissionRules.scopeProject",
  claude: "managedSkills.claude",
  codex: "managedSkills.codex",
  agy: "managedSkills.agy",
  builtin: "managedSkills.builtin",
};

function matchesSourceFilter(source: SkillSource, filter: SkillSourceFilter): boolean {
  switch (filter) {
    case "all":
      return true;
    case "user":
      return source === "orbit_user";
    case "project":
      return source === "orbit_project";
    case "claude":
      return source === "claude_user" || source === "claude_project";
    case "codex":
      return source === "codex_user";
    case "agy":
      return source === "antigravity_user";
    case "builtin":
      return source === "builtin";
  }
}

function sourceLabel(source: SkillSource): string {
  if (source === "claude_project") return `Claude · ${t("permissionRules.scopeProject")}`;
  const filter = SOURCE_FILTERS.find((item) => item !== "all" && matchesSourceFilter(source, item));
  return filter ? t(SOURCE_LABELS[filter]) : source;
}

function slugifyName(raw: string): string {
  return raw
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^[-_]+|[-_]+$/g, "")
    .slice(0, 64);
}

const SLUG_RE = /^[a-z0-9][a-z0-9_-]{0,63}$/;

export function HostSkillsSection({ hostId }: { hostId: string }) {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const state = useOrbitSelector(
    (s) => s.fleet.hosts.find((host) => host.host_id === hostId)?.connection_state,
  );
  const online = state === "online_direct" || state === "online_relay";
  const cwd = selectedHostId === hostId ? selectedCwd : hostDetail?.workspaces[0]?.cwd;
  const activeCwd = cwd && cwd !== "~" ? cwd : null;
  return (
    <ManagedSkills
      key={JSON.stringify([hostId, activeCwd])}
      hostId={hostId}
      activeCwd={activeCwd}
      online={online}
      bundles={hostDetail?.orchestration_skills ?? EMPTY_BUNDLES}
      storeSkills={hostDetail?.managed_skills ?? EMPTY_SKILLS}
    />
  );
}

function ManagedSkills({
  hostId,
  activeCwd,
  online,
  storeSkills,
  bundles,
}: {
  hostId: string;
  activeCwd: string | null;
  online: boolean;
  storeSkills: readonly ManagedSkillRecord[];
  bundles: readonly OrchestrationSkillInfo[];
}) {
  useLocale();
  const [skills, setSkills] = useState<readonly ManagedSkillRecord[]>(storeSkills);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState("");
  const [sourceFilter, setSourceFilter] = useState<SkillSourceFilter>("all");

  // Active drawer: create or import
  const [activeDrawer, setActiveDrawer] = useState<"none" | "create" | "import">("none");

  // Create form state
  const [newScope, setNewScope] = useState<SkillWriteScope>("user");
  const [newName, setNewName] = useState("");
  const [newSlug, setNewSlug] = useState("");
  const [slugTouched, setSlugTouched] = useState(false);
  const [newDesc, setNewDesc] = useState("");
  const [newBody, setNewBody] = useState("");
  const [creating, setCreating] = useState(false);

  // Import form state
  const [importScope, setImportScope] = useState<SkillWriteScope>("user");
  const [importSlugOverride, setImportSlugOverride] = useState("");
  const [importSource, setImportSource] = useState("");
  const [importing, setImporting] = useState(false);

  const [detailOpen, setDetailOpen] = useState(false);
  const [selectedSkillId, setSelectedSkillId] = useState<string | null>(null);
  const [loadedSkillId, setLoadedSkillId] = useState<string | null>(null);
  const [loadingContent, setLoadingContent] = useState(false);
  const [loadedContent, setLoadedContent] = useState("");
  const [editorContent, setEditorContent] = useState("");
  const [loadedEtag, setLoadedEtag] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [conflictMessage, setConflictMessage] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [discardEdits, setDiscardEdits] = useState(false);
  const [forking, setForking] = useState(false);
  const [togglingId, setTogglingId] = useState<string | null>(null);

  const readSeqRef = useRef(0);
  const listSeqRef = useRef(0);
  const hasLoadedScopedRef = useRef(false);
  const selectedSkillIdRef = useRef<string | null>(null);
  selectedSkillIdRef.current = selectedSkillId;

  // Only seed initial preview from store before scoped refreshSkills resolves;
  // never let generic `cwd: None` store snapshots clobber an already-loaded cwd-scoped list.
  useEffect(() => {
    if (!hasLoadedScopedRef.current && storeSkills.length > 0) {
      setSkills(storeSkills);
    }
  }, [storeSkills]);

  const clearEditorState = useCallback(() => {
    readSeqRef.current += 1;
    setSelectedSkillId(null);
    setLoadedSkillId(null);
    setLoadingContent(false);
    setLoadedContent("");
    setEditorContent("");
    setLoadedEtag(null);
    setConflictMessage(null);
    setConfirmDeleteId(null);
  }, []);

  const refreshSkills = useCallback(async () => {
    if (!online) return;
    const seq = listSeqRef.current + 1;
    listSeqRef.current = seq;
    setLoading(true);
    try {
      const fresh = await orbitIpc.listManagedSkills(hostId, activeCwd);
      if (listSeqRef.current === seq) {
        hasLoadedScopedRef.current = true;
        setSkills(fresh);
      }
    } catch (err) {
      if (listSeqRef.current === seq) {
        orbitActions.showToast(String(err));
      }
    } finally {
      if (listSeqRef.current === seq) {
        setLoading(false);
      }
    }
  }, [hostId, activeCwd, online]);

  useEffect(() => {
    void refreshSkills();
  }, [refreshSkills]);

  const selectedSkill = useMemo(
    () => skills.find((s) => s.id === selectedSkillId) ?? null,
    [skills, selectedSkillId],
  );

  const loadSkillContent = useCallback(
    async (skillId: string) => {
      const seq = readSeqRef.current + 1;
      readSeqRef.current = seq;
      setLoadingContent(true);
      setLoadedSkillId(null);
      setLoadedContent("");
      setEditorContent("");
      setLoadedEtag(null);
      setConflictMessage(null);
      setConfirmDeleteId(null);
      try {
        const res = await orbitIpc.readManagedSkill(hostId, skillId, activeCwd);
        if (readSeqRef.current === seq && selectedSkillIdRef.current === skillId) {
          setLoadedSkillId(res.skill_id);
          setLoadedContent(res.content);
          setEditorContent(res.content);
          setLoadedEtag(res.etag);
        }
      } catch (err) {
        if (readSeqRef.current === seq) {
          orbitActions.showToast(String(err));
        }
      } finally {
        if (readSeqRef.current === seq) {
          setLoadingContent(false);
        }
      }
    },
    [hostId, activeCwd],
  );

  const isDirty = Boolean(
    selectedSkill?.editable &&
    loadedSkillId === selectedSkill.id &&
    !loadingContent &&
    editorContent !== loadedContent,
  );

  const openSkillEditor = (id: string) => {
    setSelectedSkillId(id);
    selectedSkillIdRef.current = id;
    setDetailOpen(true);
    void loadSkillContent(id);
  };

  const handleSaveEditor = useCallback(async () => {
    if (
      !selectedSkill ||
      !selectedSkill.editable ||
      loadedSkillId !== selectedSkill.id ||
      loadingContent ||
      saving
    ) {
      return;
    }
    const targetId = selectedSkill.id;

    const seq = readSeqRef.current;
    const scope: SkillWriteScope = selectedSkill.source === "orbit_project" ? "project" : "user";
    setSaving(true);
    setConflictMessage(null);
    try {
      const updated = await orbitIpc.upsertManagedSkill(hostId, {
        scope,
        cwd: activeCwd,
        slug: selectedSkill.slug,
        name: selectedSkill.name,
        description: selectedSkill.description,
        content: editorContent,
        enabled: selectedSkill.enabled,
        expectedEtag: loadedEtag,
      });
      hasLoadedScopedRef.current = true;
      setSkills(updated);
      const reread = await orbitIpc.readManagedSkill(hostId, targetId, activeCwd);
      if (selectedSkillIdRef.current === targetId && readSeqRef.current === seq) {
        setLoadedSkillId(reread.skill_id);
        setLoadedContent(reread.content);
        setEditorContent(reread.content);
        setLoadedEtag(reread.etag);
      }
    } catch (err) {
      const msg = String(err);
      if (
        selectedSkillIdRef.current === targetId &&
        readSeqRef.current === seq &&
        (msg.toLowerCase().includes("etag") || msg.toLowerCase().includes("conflict"))
      ) {
        setConflictMessage(t("managedSkills.conflict"));
      } else {
        orbitActions.showToast(msg);
      }
    } finally {
      setSaving(false);
    }
  }, [
    selectedSkill,
    loadedSkillId,
    loadingContent,
    saving,
    hostId,
    activeCwd,
    editorContent,
    loadedEtag,
  ]);

  const handleToggleEnabled = async (skill: ManagedSkillRecord, nextEnabled: boolean) => {
    setTogglingId(skill.id);
    try {
      const updated = await orbitIpc.toggleManagedSkill(hostId, skill.id, nextEnabled, activeCwd);
      hasLoadedScopedRef.current = true;
      setSkills(updated);
    } catch (err) {
      orbitActions.showToast(String(err));
    } finally {
      setTogglingId(null);
    }
  };

  const handleDeleteSkill = async (skill: ManagedSkillRecord) => {
    const updated = await orbitIpc.deleteManagedSkill(hostId, skill.id, activeCwd);
    hasLoadedScopedRef.current = true;
    setSkills(updated);
    clearEditorState();
  };

  const handleForkToOrbit = async (skill: ManagedSkillRecord, scope: SkillWriteScope) => {
    if (forking) return;
    setForking(true);
    try {
      const content =
        loadedSkillId === skill.id && !loadingContent
          ? loadedContent
          : (await orbitIpc.readManagedSkill(hostId, skill.id, activeCwd)).content;
      const updated = await orbitIpc.upsertManagedSkill(hostId, {
        scope,
        cwd: activeCwd,
        slug: skill.slug,
        name: skill.name,
        description: skill.description,
        content,
        enabled: true,
        expectedEtag: null,
      });
      hasLoadedScopedRef.current = true;
      setSkills(updated);
      const nextId = `${scope === "project" ? "orbit_project" : "orbit_user"}:${skill.slug}`;
      openSkillEditor(nextId);
    } catch (err) {
      orbitActions.showToast(String(err));
    } finally {
      setForking(false);
    }
  };

  const handleCreateSkill = async () => {
    const slug = newSlug.trim();
    if (!SLUG_RE.test(slug)) {
      orbitActions.showToast(t("managedSkills.slugError"));
      return;
    }
    if (newScope === "project" && !activeCwd) {
      orbitActions.showToast(t("managedSkills.needProject"));
      return;
    }
    setCreating(true);
    try {
      const name = newName.trim() || slug;
      const desc = newDesc.trim();
      const body = newBody.trim() || `# ${name}\n`;
      const updated = await orbitIpc.upsertManagedSkill(hostId, {
        scope: newScope,
        cwd: activeCwd,
        slug,
        name,
        description: desc,
        content: body,
        enabled: true,
        expectedEtag: null,
      });
      hasLoadedScopedRef.current = true;
      setSkills(updated);
      setActiveDrawer("none");
      setNewName("");
      setNewSlug("");
      setSlugTouched(false);
      setNewDesc("");
      setNewBody("");
      const createdId = `${newScope === "project" ? "orbit_project" : "orbit_user"}:${slug}`;
      openSkillEditor(createdId);
    } catch (err) {
      orbitActions.showToast(String(err));
    } finally {
      setCreating(false);
    }
  };

  const handleImportSkill = async () => {
    if (!importSource.trim()) {
      orbitActions.showToast(t("managedSkills.importSource"));
      return;
    }
    if (importScope === "project" && !activeCwd) {
      orbitActions.showToast(t("managedSkills.needProject"));
      return;
    }
    const slugOverride = importSlugOverride.trim() || null;
    if (slugOverride && !SLUG_RE.test(slugOverride)) {
      orbitActions.showToast(t("managedSkills.slugError"));
      return;
    }
    setImporting(true);
    try {
      const beforeIds = new Set(skills.map((s) => s.id));
      const updated = await orbitIpc.importManagedSkill(hostId, {
        scope: importScope,
        cwd: activeCwd,
        sourceMarkdownOrPath: importSource,
        slugOverride,
      });
      hasLoadedScopedRef.current = true;
      setSkills(updated);
      setActiveDrawer("none");
      setImportSource("");
      setImportSlugOverride("");
      const newlyAdded = updated.find((s) => !beforeIds.has(s.id));
      if (newlyAdded) {
        openSkillEditor(newlyAdded.id);
      } else {
        orbitActions.showToast(t("managedSkills.imported"));
      }
    } catch (err) {
      orbitActions.showToast(String(err));
    } finally {
      setImporting(false);
    }
  };

  // The daemon lists bundled workflows in both catalogs. Match only its built-in
  // source so separately editable user and project copies remain available.
  const bundledRecords = useMemo(() => {
    const ids = new Set(bundles.map((bundle) => bundle.id));
    return new Map(
      skills
        .filter((skill) => skill.source === "builtin" && ids.has(skill.slug))
        .map((skill) => [skill.slug, skill]),
    );
  }, [skills, bundles]);
  const workflows = useMemo(
    () => skills.filter((skill) => skill.source !== "builtin" || !bundledRecords.has(skill.slug)),
    [skills, bundledRecords],
  );
  const filterCounts = useMemo(() => {
    const counts: Record<SkillSourceFilter, number> = {
      all: workflows.length,
      user: 0,
      project: 0,
      claude: 0,
      codex: 0,
      agy: 0,
      builtin: 0,
    };
    for (const s of workflows) {
      for (const f of SOURCE_FILTERS) {
        if (f !== "all" && matchesSourceFilter(s.source, f)) {
          counts[f] += 1;
        }
      }
    }
    return counts;
  }, [workflows]);

  const filteredSkills = useMemo(() => {
    const q = search.trim().toLowerCase();
    return workflows.filter((s) => {
      if (!matchesSourceFilter(s.source, sourceFilter)) return false;
      if (!q) return true;
      return (
        s.slug.toLowerCase().includes(q) ||
        s.name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q) ||
        s.triggers.some((t) => t.toLowerCase().includes(q))
      );
    });
  }, [workflows, sourceFilter, search]);

  const isCreate = activeDrawer === "create";
  const busy = importing || creating;
  const writeScope = isCreate ? newScope : importScope;

  return (
    <>
      <OrchestrationSkills
        hostId={hostId}
        skills={bundles}
        online={online}
        onSynced={() => void refreshSkills()}
        actionsForSkill={(id) => {
          const skill = bundledRecords.get(id);
          if (!skill) return null;
          return (
            <>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => openSkillEditor(skill.id)}
                aria-label={skill.name + " · SKILL.md"}
              >
                SKILL.md
              </Button>
              <ToggleSwitch
                checked={skill.enabled}
                ariaLabel={skill.name}
                disabled={!online || togglingId !== null}
                onChange={(next) => void handleToggleEnabled(skill, next)}
              />
            </>
          );
        }}
      />
      <section data-setting-id="managedSkills.title" className="flex flex-col gap-2">
        <SettingGroupLabel
          action={
            <div className="flex items-center gap-1">
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={t("common.refresh")}
                disabled={loading || !online}
                onClick={() => void refreshSkills()}
              >
                <RefreshCw />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                data-setting-id="managedSkills.import"
                disabled={!online}
                onClick={() => setActiveDrawer("import")}
              >
                <Import data-icon="inline-start" />
                {t("managedSkills.import")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                data-setting-id="managedSkills.create"
                disabled={!online}
                onClick={() => setActiveDrawer("create")}
              >
                <Plus data-icon="inline-start" />
                {t("managedSkills.create")}
              </Button>
            </div>
          }
        >
          {t("managedSkills.title")} · {workflows.length}
        </SettingGroupLabel>
        <div className="flex items-center gap-2">
          <Input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label={t("managedSkills.filter")}
            placeholder={t("managedSkills.filter")}
            className="flex-1 text-[13px]"
          />
          <SettingSelect
            value={sourceFilter}
            onChange={setSourceFilter}
            ariaLabel={t("managedSkills.source")}
            options={SOURCE_FILTERS.filter(
              (id) => id !== "builtin" || filterCounts.builtin > 0 || sourceFilter === id,
            ).map((id) => ({
              value: id,
              label: t(SOURCE_LABELS[id]),
              badge: String(filterCounts[id]),
            }))}
          />
        </div>
        <SettingCard>
          {filteredSkills.length === 0 ? (
            <p className="text-ink-muted px-4 py-5 text-center text-[13px]">
              {t(
                loading
                  ? "common.loading"
                  : workflows.length > 0
                    ? "managedSkills.noMatches"
                    : "managedSkills.empty",
              )}
            </p>
          ) : (
            filteredSkills.map((skill) => (
              <SettingResourceRow
                key={skill.id}
                title={skill.name}
                description={skill.description}
                status={
                  <span>
                    {sourceLabel(skill.source)} · <code>/{skill.slug}</code>
                  </span>
                }
                icon={<BookOpen />}
                onClick={() => openSkillEditor(skill.id)}
                action={
                  <ToggleSwitch
                    checked={skill.enabled}
                    ariaLabel={skill.name}
                    disabled={togglingId !== null || !online}
                    onChange={(next) => void handleToggleEnabled(skill, next)}
                  />
                }
              />
            ))
          )}
        </SettingCard>
        <SettingDrawer
          open={activeDrawer !== "none"}
          onOpenChange={(open) => {
            if (!open && !busy) setActiveDrawer("none");
          }}
          title={t(isCreate ? "managedSkills.create" : "managedSkills.import")}
        >
          <form
            className="flex flex-col gap-4"
            onSubmit={(e) => {
              e.preventDefault();
              if (!busy && online) void (isCreate ? handleCreateSkill() : handleImportSkill());
            }}
          >
            <FieldGroup>
              <Field>
                <FieldLabel>{t("managedSkills.scope")}</FieldLabel>
                <SettingSelect<SkillWriteScope>
                  value={writeScope}
                  disabled={busy}
                  ariaLabel={t("managedSkills.scope")}
                  onChange={isCreate ? setNewScope : setImportScope}
                  options={[
                    {
                      value: "user",
                      label: t("managedSkills.user"),
                      description: "~/.orbit/skills",
                    },
                    {
                      value: "project",
                      label: t("permissionRules.scopeProject"),
                      disabled: !activeCwd,
                      description: activeCwd
                        ? activeCwd + "/.orbit/skills"
                        : t("managedSkills.needProject"),
                    },
                  ]}
                />
                <span className="text-ink-muted break-all font-mono text-[11px]">
                  {writeScope === "user" ? "~/.orbit/skills" : activeCwd + "/.orbit/skills"}
                </span>
              </Field>
              {isCreate ? (
                <>
                  <Field>
                    <FieldLabel htmlFor="managed-skill-name">{t("managedSkills.name")}</FieldLabel>
                    <Input
                      id="managed-skill-name"
                      value={newName}
                      disabled={busy}
                      onChange={(e) => {
                        const name = e.target.value;
                        setNewName(name);
                        if (!slugTouched) setNewSlug(slugifyName(name));
                      }}
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="managed-skill-slug">{t("managedSkills.slug")}</FieldLabel>
                    <Input
                      id="managed-skill-slug"
                      value={newSlug}
                      required
                      pattern="[a-z0-9][a-z0-9_\-]{0,63}"
                      disabled={busy}
                      onChange={(e) => {
                        setSlugTouched(true);
                        setNewSlug(e.target.value.toLowerCase());
                      }}
                      className="font-mono"
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="managed-skill-description">
                      {t("managedSkills.description")}
                    </FieldLabel>
                    <Input
                      id="managed-skill-description"
                      value={newDesc}
                      disabled={busy}
                      onChange={(e) => setNewDesc(e.target.value)}
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="managed-skill-body">
                      {t("managedSkills.instructions")}
                    </FieldLabel>
                    <Textarea
                      id="managed-skill-body"
                      rows={8}
                      value={newBody}
                      disabled={busy}
                      onChange={(e) => setNewBody(e.target.value)}
                      className="font-mono text-[13px]"
                    />
                  </Field>
                </>
              ) : (
                <>
                  <Field>
                    <FieldLabel htmlFor="managed-skill-import">
                      {t("managedSkills.importSource")}
                    </FieldLabel>
                    <Textarea
                      id="managed-skill-import"
                      rows={8}
                      value={importSource}
                      disabled={busy}
                      required
                      onChange={(e) => setImportSource(e.target.value)}
                      className="font-mono text-[13px]"
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="managed-skill-import-slug">
                      {t("managedSkills.override")}
                    </FieldLabel>
                    <Input
                      id="managed-skill-import-slug"
                      value={importSlugOverride}
                      disabled={busy}
                      onChange={(e) => setImportSlugOverride(e.target.value.toLowerCase())}
                      className="font-mono"
                    />
                  </Field>
                </>
              )}
            </FieldGroup>
            <Button type="submit" size="sm" disabled={!online || busy} className="self-end">
              {busy && <OrbitSpinner size={13} />}
              {t(isCreate ? "managedSkills.create" : "managedSkills.import")}
            </Button>
          </form>
        </SettingDrawer>
        {selectedSkill && (
          <SettingDrawer
            open={detailOpen}
            onOpenChangeComplete={(visible) => {
              if (!visible) clearEditorState();
            }}
            title={selectedSkill.name}
            description={selectedSkill.description}
            onOpenChange={(open) => {
              if (open || forking || saving) return;
              if (isDirty) setDiscardEdits(true);
              else setDetailOpen(false);
            }}
          >
            <div className="flex flex-col gap-4">
              <div className="text-ink-muted flex flex-col gap-1 text-[11px]">
                <code className="select-text break-all">{selectedSkill.path}</code>
                <span>
                  {sourceLabel(selectedSkill.source)} ·{" "}
                  {selectedSkill.approx_tokens.toLocaleString()} tokens
                </span>
                <code>
                  /{selectedSkill.slug} {selectedSkill.argument_hint}
                </code>
                {selectedSkill.is_command && <span>{t("managedSkills.command")}</span>}
                {(selectedSkill.has_scripts || selectedSkill.has_references) && (
                  <span className="inline-flex items-center gap-1.5 font-mono">
                    <Code2 className="size-3" />
                    {[
                      selectedSkill.has_scripts && "scripts/",
                      selectedSkill.has_references && "references/",
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </span>
                )}
                {selectedSkill.triggers.length > 0 && (
                  <span>{selectedSkill.triggers.map((trigger) => "#" + trigger).join(" · ")}</span>
                )}
                {selectedSkill.allowed_tools.length > 0 && (
                  <code>{selectedSkill.allowed_tools.join(" · ")}</code>
                )}
              </div>
              {conflictMessage && (
                <div
                  role="alert"
                  className="border-border-subtle text-ink-primary flex items-center gap-2 rounded-lg border p-3 text-[13px]"
                >
                  <AlertTriangle className="size-4 shrink-0" />
                  <span>{conflictMessage}</span>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => void loadSkillContent(selectedSkill.id)}
                  >
                    {t("sidePanel.editorConflictReload")}
                  </Button>
                </div>
              )}
              {loadingContent ? (
                <OrbitSpinner size={14} />
              ) : loadedSkillId !== selectedSkill.id ? (
                <Button
                  variant="outline"
                  disabled={!online}
                  onClick={() => void loadSkillContent(selectedSkill.id)}
                >
                  {t("sidePanel.editorConflictReload")}
                </Button>
              ) : selectedSkill.editable ? (
                <>
                  <Textarea
                    rows={14}
                    spellCheck={false}
                    aria-label="SKILL.md"
                    value={editorContent}
                    disabled={saving || !online}
                    onChange={(e) => setEditorContent(e.target.value)}
                    className="font-mono text-[13px]"
                    onKeyDown={(e) => {
                      if (
                        !isImeKeyEvent(e) &&
                        !e.altKey &&
                        (e.metaKey || e.ctrlKey) &&
                        e.key.toLowerCase() === "s"
                      ) {
                        e.preventDefault();
                        if (isDirty) void handleSaveEditor();
                      }
                    }}
                  />
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      disabled={saving || !online}
                      aria-label={t("common.delete")}
                      onClick={() => setConfirmDeleteId(selectedSkill.id)}
                    >
                      <Trash2 />
                    </Button>
                    <div className="flex items-center gap-2">
                      {isDirty && (
                        <Button
                          size="sm"
                          variant="ghost"
                          disabled={saving}
                          onClick={() => setEditorContent(loadedContent)}
                        >
                          {t("review.discard")}
                        </Button>
                      )}
                      <Button
                        size="sm"
                        disabled={!isDirty || saving || !online}
                        onClick={() => void handleSaveEditor()}
                      >
                        {saving && <OrbitSpinner size={13} />}
                        <span>{t("common.save")}</span>
                        <ShortcutKbd keys={`${modSymbol()}S`} size="xs" variant="inverted" />
                      </Button>
                    </div>
                  </div>
                </>
              ) : (
                <>
                  <pre className="border-border-subtle bg-surface-recessed text-ink-secondary overflow-auto rounded-lg border p-3 font-mono text-[13px] whitespace-pre-wrap select-text">
                    {loadedContent}
                  </pre>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={!online || forking}
                      onClick={() => void handleForkToOrbit(selectedSkill, "user")}
                    >
                      <GitFork data-icon="inline-start" />
                      {t("managedSkills.forkUser")}
                    </Button>
                    {activeCwd && (
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={!online || forking}
                        onClick={() => void handleForkToOrbit(selectedSkill, "project")}
                      >
                        {t("managedSkills.forkProject")}
                      </Button>
                    )}
                  </div>
                </>
              )}
            </div>
            <ConfirmDialog
              open={discardEdits}
              onOpenChange={setDiscardEdits}
              tone="danger"
              title={t("review.discardFileConfirmTitle", { name: selectedSkill.name })}
              confirmLabel={t("review.discard")}
              onConfirm={() => {
                setDiscardEdits(false);
                setDetailOpen(false);
              }}
            />
            <ConfirmDialog
              open={confirmDeleteId === selectedSkill.id}
              onOpenChange={(open) => {
                if (!open) setConfirmDeleteId(null);
              }}
              tone="danger"
              title={t("managedSkills.delete", { name: selectedSkill.name })}
              confirmLabel={t("common.delete")}
              onConfirm={() => handleDeleteSkill(selectedSkill)}
            />
          </SettingDrawer>
        )}
      </section>
    </>
  );
}
