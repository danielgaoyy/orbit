import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
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
  useHostIsOnline,
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
import { isAlreadyExistsError, suggestAvailableName } from "@/lib/name-conflict";
import { modSymbol } from "@/lib/keybindings";
import {
  type ManagedSkillRecord,
  type OrchestrationSkillInfo,
  type SkillSource,
  type SkillWriteScope,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { t, useLocale, type MessageKey } from "@/lib/i18n";
import { cn } from "@/lib/utils";
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

const SkillCreateImportDrawer = memo(function SkillCreateImportDrawer({
  mode,
  onClose,
  hostId,
  activeCwd,
  online,
  existingSkillIds,
  onMutated,
  onOpenExisting,
}: {
  mode: "none" | "create" | "import";
  onClose: () => void;
  hostId: string;
  activeCwd: string | null;
  online: boolean;
  existingSkillIds: readonly string[];
  onMutated: (updated: readonly ManagedSkillRecord[], openId?: string | null) => void;
  onOpenExisting: (skillId: string) => void;
}) {
  useLocale();
  const [newScope, setNewScope] = useState<SkillWriteScope>("user");
  const [newName, setNewName] = useState("");
  const [newSlug, setNewSlug] = useState("");
  const [slugTouched, setSlugTouched] = useState(false);
  const [newDesc, setNewDesc] = useState("");
  const [newBody, setNewBody] = useState("");
  const [creating, setCreating] = useState(false);

  const [importScope, setImportScope] = useState<SkillWriteScope>("user");
  const [importSlugOverride, setImportSlugOverride] = useState("");
  const [importSource, setImportSource] = useState("");
  const [importing, setImporting] = useState(false);

  // Create-only conflict state: which slug collided, a free suggested name, and whether the
  // collision came from the create or import tab. Drives the inline prompt plus the danger
  // replace dialog, so a conflict never resolves into a silent overwrite.
  const [conflict, setConflict] = useState<{
    slug: string;
    suggestion: string;
    kind: "create" | "import";
  } | null>(null);
  const [replaceTarget, setReplaceTarget] = useState<string | null>(null);
  const slugInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    setConflict(null);
    setReplaceTarget(null);
  }, [mode]);

  const isCreate = mode === "create";
  const busy = creating || importing;
  const writeScope = isCreate ? newScope : importScope;
  const canSubmit =
    (writeScope !== "project" || Boolean(activeCwd)) &&
    (isCreate ? Boolean(newSlug.trim()) : Boolean(importSource.trim()));

  const scopeOptions = useMemo(
    () => [
      {
        value: "user" as const,
        label: t("managedSkills.user"),
        description: "~/.orbit/skills",
      },
      {
        value: "project" as const,
        label: t("permissionRules.scopeProject"),
        disabled: !activeCwd,
        description: activeCwd ? activeCwd + "/.orbit/skills" : t("managedSkills.needProject"),
      },
    ],
    [activeCwd],
  );

  /** Skill ids that would collide with a write into the given scope. */
  const takenSlugs = useCallback(
    (scope: SkillWriteScope) => {
      const prefix = scope === "project" ? "orbit_project:" : "orbit_user:";
      return existingSkillIds
        .filter((id) => id.startsWith(prefix))
        .map((id) => id.slice(prefix.length));
    },
    [existingSkillIds],
  );

  const skillIdFor = (scope: SkillWriteScope, slug: string) =>
    `${scope === "project" ? "orbit_project" : "orbit_user"}:${slug}`;

  const resetCreateFields = () => {
    setNewName("");
    setNewSlug("");
    setSlugTouched(false);
    setNewDesc("");
    setNewBody("");
  };

  /**
   * Create a skill. `replace` is only ever true after the user confirms the danger dialog; the
   * default path is create-only so an existing `SKILL.md` can never be overwritten silently.
   */
  const handleCreateSkill = async (replace = false) => {
    const slug = newSlug.trim();
    if (!SLUG_RE.test(slug)) {
      orbitActions.showToast(t("managedSkills.slugError"));
      return;
    }
    if (newScope === "project" && !activeCwd) {
      orbitActions.showToast(t("managedSkills.needProject"));
      return;
    }
    // Instant precheck against the in-memory list so the common case never round-trips.
    if (!replace && takenSlugs(newScope).includes(slug)) {
      setConflict({
        slug,
        suggestion: suggestAvailableName(slug, takenSlugs(newScope)),
        kind: "create",
      });
      slugInputRef.current?.focus();
      return;
    }
    setCreating(true);
    try {
      const name = newName.trim() || slug;
      const desc = newDesc.trim();
      const body = newBody.trim() || `# ${name}\n`;
      const createdId = skillIdFor(newScope, slug);
      // Replacing an existing skill requires its current etag, so the confirmed overwrite is
      // still version-checked rather than a blind write.
      const expectedEtag = replace
        ? (await orbitIpc.readManagedSkill(hostId, createdId, activeCwd)).etag
        : null;
      const updated = await orbitIpc.upsertManagedSkill(hostId, {
        scope: newScope,
        cwd: activeCwd,
        slug,
        name,
        description: desc,
        content: body,
        enabled: true,
        expectedEtag,
        createOnly: !replace,
      });
      setConflict(null);
      resetCreateFields();
      onClose();
      onMutated(updated, createdId);
    } catch (err) {
      // The daemon is the authority: a conflict can still appear if the list snapshot was stale.
      if (!replace && isAlreadyExistsError(err)) {
        setConflict({
          slug,
          suggestion: suggestAvailableName(slug, takenSlugs(newScope)),
          kind: "create",
        });
        slugInputRef.current?.focus();
      } else {
        orbitActions.showToast(String(err));
      }
    } finally {
      setCreating(false);
    }
  };

  /**
   * Import a skill. `replaceSlug` is only set after the user confirms the danger dialog; the
   * import is then pinned to that exact slug and version-checked against its current etag.
   */
  const handleImportSkill = async (replaceSlug: string | null = null) => {
    const replace = replaceSlug !== null;
    if (!importSource.trim()) {
      orbitActions.showToast(t("managedSkills.importSource"));
      return;
    }
    if (importScope === "project" && !activeCwd) {
      orbitActions.showToast(t("managedSkills.needProject"));
      return;
    }
    const slugOverride = replaceSlug ?? (importSlugOverride.trim() || null);
    if (slugOverride && !SLUG_RE.test(slugOverride)) {
      orbitActions.showToast(t("managedSkills.slugError"));
      return;
    }
    // Only the explicit override can be prechecked — otherwise the slug is derived server-side
    // from the Markdown frontmatter or heading, so the daemon's create-only guard is what catches
    // the collision.
    if (!replace && slugOverride && takenSlugs(importScope).includes(slugOverride)) {
      setConflict({
        slug: slugOverride,
        suggestion: suggestAvailableName(slugOverride, takenSlugs(importScope)),
        kind: "import",
      });
      return;
    }
    setImporting(true);
    try {
      const beforeIds = new Set(existingSkillIds);
      const expectedEtag = replaceSlug
        ? (
            await orbitIpc.readManagedSkill(
              hostId,
              skillIdFor(importScope, replaceSlug),
              activeCwd,
            )
          ).etag
        : null;
      const updated = await orbitIpc.importManagedSkill(hostId, {
        scope: importScope,
        cwd: activeCwd,
        sourceMarkdownOrPath: importSource,
        slugOverride,
        expectedEtag,
        createOnly: !replace,
      });
      setConflict(null);
      setImportSource("");
      setImportSlugOverride("");
      onClose();
      // With create-only enforced, a successful import always yields a new record. Keep the
      // diff only to decide which skill to open, and no longer report success when nothing was
      // added — the old `else` branch said "imported" while an overwrite had just happened.
      const newlyAdded = updated.find((s) => !beforeIds.has(s.id));
      const targetId = newlyAdded?.id ?? (replaceSlug ? skillIdFor(importScope, replaceSlug) : null);
      onMutated(updated, targetId);
      if (newlyAdded) {
        orbitActions.showToast(t("managedSkills.imported"));
      }
    } catch (err) {
      if (!replace && isAlreadyExistsError(err)) {
        // The daemon reports the resolved slug in its message; fall back to the override.
        const resolved = /`([^`]+)`/.exec(String(err))?.[1] ?? slugOverride;
        if (resolved) {
          setConflict({
            slug: resolved,
            suggestion: suggestAvailableName(resolved, takenSlugs(importScope)),
            kind: "import",
          });
        } else {
          orbitActions.showToast(String(err));
        }
      } else {
        orbitActions.showToast(String(err));
      }
    } finally {
      setImporting(false);
    }
  };

  return (
    <SettingDrawer
      open={mode !== "none"}
      onOpenChange={(open) => {
        if (!open && !busy) onClose();
      }}
      title={t(isCreate ? "managedSkills.create" : "managedSkills.import")}
    >
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          if (!busy && online && canSubmit) {
            void (isCreate ? handleCreateSkill() : handleImportSkill(null));
          }
        }}
      >
        <FieldGroup>
          <Field>
            <FieldLabel>{t("managedSkills.scope")}</FieldLabel>
            <SettingSelect<SkillWriteScope>
              value={writeScope}
              disabled={busy}
              ariaLabel={t("managedSkills.scope")}
              onChange={(nextScope) => {
                if (isCreate) setNewScope(nextScope);
                else setImportScope(nextScope);
                setConflict(null);
                setReplaceTarget(null);
              }}
              options={scopeOptions}
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
                    if (!slugTouched) {
                      setNewSlug(slugifyName(name));
                      setConflict(null);
                      setReplaceTarget(null);
                    }
                  }}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="managed-skill-slug">{t("managedSkills.slug")}</FieldLabel>
                <Input
                  id="managed-skill-slug"
                  ref={slugInputRef}
                  value={newSlug}
                  required
                  pattern="[a-z0-9][a-z0-9_\-]{0,63}"
                  disabled={busy}
                  onChange={(e) => {
                    setSlugTouched(true);
                    setNewSlug(e.target.value.toLowerCase());
                    setConflict(null);
                    setReplaceTarget(null);
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
                  onChange={(e) => {
                    setImportSource(e.target.value);
                    setConflict(null);
                    setReplaceTarget(null);
                  }}
                  className="font-mono text-[13px]"
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="managed-skill-import-slug">
                  {t("managedSkills.override")}
                </FieldLabel>
                <Input
                  id="managed-skill-import-slug"
                  ref={slugInputRef}
                  value={importSlugOverride}
                  disabled={busy}
                  onChange={(e) => {
                    setImportSlugOverride(e.target.value.toLowerCase());
                    setConflict(null);
                    setReplaceTarget(null);
                  }}
                  className="font-mono"
                />
              </Field>
            </>
          )}
        </FieldGroup>
        {conflict && (
          <div className="border-border-subtle bg-surface-raised flex flex-col gap-2 rounded-md border p-3">
            <span className="text-ink-primary text-[13px] font-medium">
              {t("managedSkills.conflictTitle")}
            </span>
            <span className="text-ink-muted text-[11px]">
              {t("managedSkills.conflictExists", { slug: conflict.slug })}
            </span>
            <div className="flex flex-wrap items-center gap-2 pt-1">
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={busy}
                onClick={() => {
                  if (conflict.kind === "create") {
                    setSlugTouched(true);
                    setNewSlug(conflict.suggestion);
                  } else {
                    setImportSlugOverride(conflict.suggestion);
                  }
                  setConflict(null);
                  slugInputRef.current?.focus();
                }}
              >
                {t("managedSkills.conflictRename", { slug: conflict.suggestion })}
              </Button>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={busy}
                onClick={() => {
                  const target = skillIdFor(
                    conflict.kind === "create" ? newScope : importScope,
                    conflict.slug,
                  );
                  setConflict(null);
                  onClose();
                  onOpenExisting(target);
                }}
              >
                {t("managedSkills.conflictOpen")}
              </Button>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                disabled={busy}
                className="text-signal-danger"
                onClick={() => setReplaceTarget(conflict.slug)}
              >
                {t("managedSkills.conflictReplace")}
              </Button>
            </div>
          </div>
        )}
        <Button
          type="submit"
          size="sm"
          disabled={!online || busy || !canSubmit}
          className="self-end"
        >
          {busy && <OrbitSpinner size={13} />}
          {t(isCreate ? "managedSkills.create" : "managedSkills.import")}
        </Button>
      </form>
      <ConfirmDialog
        open={replaceTarget !== null}
        onOpenChange={(open) => {
          if (!open) setReplaceTarget(null);
        }}
        tone="danger"
        title={t("managedSkills.conflictReplaceTitle", { slug: replaceTarget ?? "" })}
        description={t("managedSkills.conflictReplaceBody", { slug: replaceTarget ?? "" })}
        confirmLabel={t("managedSkills.conflictReplace")}
        onConfirm={async () => {
          const slug = replaceTarget;
          const kind = conflict?.kind ?? (isCreate ? "create" : "import");
          setReplaceTarget(null);
          if (!slug) return;
          await (kind === "create" ? handleCreateSkill(true) : handleImportSkill(slug));
        }}
      />
    </SettingDrawer>
  );
});

export const HostSkillsSection = memo(function HostSkillsSection({
  hostId,
}: {
  hostId: string;
}) {
  const online = useHostIsOnline(hostId);
  const activeCwd = useOrbitSelector((s) => {
    const cwd =
      s.selectedHostId === hostId
        ? s.selectedWorkspaceCwd
        : s.hostDetails[hostId]?.workspaces[0]?.cwd;
    return cwd && cwd !== "~" ? cwd : null;
  });
  const bundles = useOrbitSelector(
    (s) => s.hostDetails[hostId]?.orchestration_skills ?? EMPTY_BUNDLES,
  );
  const storeSkills = useOrbitSelector(
    (s) => s.hostDetails[hostId]?.managed_skills ?? EMPTY_SKILLS,
  );
  return (
    <ManagedSkills
      key={JSON.stringify([hostId, activeCwd])}
      hostId={hostId}
      activeCwd={activeCwd}
      online={online}
      bundles={bundles}
      storeSkills={storeSkills}
    />
  );
});

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
  const [loading, setLoading] = useState(() => online);
  const [search, setSearch] = useState("");
  const [sourceFilter, setSourceFilter] = useState<SkillSourceFilter>("all");

  // Active drawer: create or import (form keystrokes isolated in SkillCreateImportDrawer)
  const [activeDrawer, setActiveDrawer] = useState<"none" | "create" | "import">("none");

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
  // Fork (copy to user/project) conflict: the copy's slug already exists in the target scope.
  const [forkConflict, setForkConflict] = useState<{
    skill: ManagedSkillRecord;
    scope: SkillWriteScope;
    slug: string;
    suggestion: string;
  } | null>(null);
  const [forkReplaceOpen, setForkReplaceOpen] = useState(false);

  const readSeqRef = useRef(0);
  const listSeqRef = useRef(0);
  const toggleSeqRef = useRef(0);
  const toggleChainRef = useRef<Promise<void>>(Promise.resolve());
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
    if (!online) {
      setLoading(false);
      return;
    }
    const seq = listSeqRef.current + 1;
    listSeqRef.current = seq;
    setLoading(true);
    try {
      const fresh = await orbitIpc.listManagedSkills(hostId, activeCwd);
      if (listSeqRef.current === seq) {
        hasLoadedScopedRef.current = true;
        toggleSeqRef.current += 1;
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

  const handleToggleEnabled = useCallback(
    (skill: ManagedSkillRecord, nextEnabled: boolean) => {
      const seq = ++toggleSeqRef.current;
      setSkills((prev) =>
        prev.map((item) => (item.id === skill.id ? { ...item, enabled: nextEnabled } : item)),
      );
      toggleChainRef.current = toggleChainRef.current.then(async () => {
        try {
          const updated = await orbitIpc.toggleManagedSkill(
            hostId,
            skill.id,
            nextEnabled,
            activeCwd,
          );
          if (toggleSeqRef.current === seq) {
            hasLoadedScopedRef.current = true;
            setSkills(updated);
          }
        } catch (err) {
          setSkills((prev) =>
            prev.map((item) =>
              item.id === skill.id ? { ...item, enabled: !nextEnabled } : item,
            ),
          );
          orbitActions.showToast(String(err));
        }
      });
    },
    [hostId, activeCwd],
  );

  const handleDeleteSkill = async (skill: ManagedSkillRecord) => {
    const updated = await orbitIpc.deleteManagedSkill(hostId, skill.id, activeCwd);
    hasLoadedScopedRef.current = true;
    setSkills(updated);
    clearEditorState();
  };

  /**
   * Copy a skill into an Orbit scope. Create-only by default: a same-slug skill already in the
   * target scope opens the conflict prompt instead of being overwritten. `mode.slug` renames the
   * copy; `mode.replace` (only after the danger confirmation) overwrites with an etag check.
   */
  const handleForkToOrbit = async (
    skill: ManagedSkillRecord,
    scope: SkillWriteScope,
    mode: { slug?: string; replace?: boolean } = {},
  ) => {
    if (forking) return;
    const slug = mode.slug ?? skill.slug;
    const prefix = scope === "project" ? "orbit_project:" : "orbit_user:";
    const nextId = `${prefix}${slug}`;
    const taken = skills.filter((s) => s.id.startsWith(prefix)).map((s) => s.slug);
    if (!mode.replace && taken.includes(slug)) {
      setForkConflict({ skill, scope, slug, suggestion: suggestAvailableName(slug, taken) });
      return;
    }
    setForking(true);
    try {
      const content =
        loadedSkillId === skill.id && !loadingContent
          ? loadedContent
          : (await orbitIpc.readManagedSkill(hostId, skill.id, activeCwd)).content;
      const expectedEtag = mode.replace
        ? (await orbitIpc.readManagedSkill(hostId, nextId, activeCwd)).etag
        : null;
      const updated = await orbitIpc.upsertManagedSkill(hostId, {
        scope,
        cwd: activeCwd,
        slug,
        name: skill.name,
        description: skill.description,
        content,
        enabled: true,
        expectedEtag,
        createOnly: !mode.replace,
      });
      setForkConflict(null);
      hasLoadedScopedRef.current = true;
      setSkills(updated);
      openSkillEditor(nextId);
    } catch (err) {
      if (!mode.replace && isAlreadyExistsError(err)) {
        setForkConflict({ skill, scope, slug, suggestion: suggestAvailableName(slug, taken) });
      } else {
        orbitActions.showToast(String(err));
      }
    } finally {
      setForking(false);
    }
  };

  const existingSkillIds = useMemo(() => skills.map((s) => s.id), [skills]);
  const handleCloseCreateImport = useCallback(() => setActiveDrawer("none"), []);
  const handleSkillMutated = useCallback(
    (updated: readonly ManagedSkillRecord[], openId?: string | null) => {
      hasLoadedScopedRef.current = true;
      setSkills(updated);
      if (openId) openSkillEditor(openId);
    },
    [openSkillEditor],
  );

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
                disabled={!online}
                onChange={(next) => handleToggleEnabled(skill, next)}
              />
            </>
          );
        }}
      />
      <section data-setting-id="managedSkills.title" className="flex flex-col gap-3">
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
                <RefreshCw className={cn(loading && "animate-spin")} />
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
                loading && workflows.length === 0
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
                    disabled={!online}
                    onChange={(next) => handleToggleEnabled(skill, next)}
                  />
                }
              />
            ))
          )}
        </SettingCard>
        <SkillCreateImportDrawer
          mode={activeDrawer}
          onClose={handleCloseCreateImport}
          hostId={hostId}
          activeCwd={activeCwd}
          online={online}
          existingSkillIds={existingSkillIds}
          onMutated={handleSkillMutated}
          onOpenExisting={openSkillEditor}
        />
        <ConfirmDialog
          open={forkConflict !== null && !forkReplaceOpen}
          onOpenChange={(open) => {
            if (!open) setForkConflict(null);
          }}
          title={t("managedSkills.conflictTitle")}
          description={t("managedSkills.conflictExists", { slug: forkConflict?.slug ?? "" })}
          detail={
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="text-signal-danger -mx-2"
              onClick={() => setForkReplaceOpen(true)}
            >
              {t("managedSkills.conflictReplace")}
            </Button>
          }
          confirmLabel={t("managedSkills.conflictRename", {
            slug: forkConflict?.suggestion ?? "",
          })}
          secondaryAction={{
            label: t("managedSkills.conflictOpen"),
            variant: "outline",
            onClick: () => {
              const target = forkConflict;
              setForkConflict(null);
              if (target) {
                openSkillEditor(
                  `${target.scope === "project" ? "orbit_project" : "orbit_user"}:${target.slug}`,
                );
              }
            },
          }}
          onConfirm={async () => {
            const target = forkConflict;
            setForkConflict(null);
            if (target) {
              await handleForkToOrbit(target.skill, target.scope, { slug: target.suggestion });
            }
          }}
        />
        <ConfirmDialog
          open={forkReplaceOpen}
          onOpenChange={(open) => {
            if (!open) {
              setForkReplaceOpen(false);
              setForkConflict(null);
            }
          }}
          tone="danger"
          title={t("managedSkills.conflictReplaceTitle", { slug: forkConflict?.slug ?? "" })}
          description={t("managedSkills.conflictReplaceBody", { slug: forkConflict?.slug ?? "" })}
          confirmLabel={t("managedSkills.conflictReplace")}
          onConfirm={async () => {
            const target = forkConflict;
            setForkReplaceOpen(false);
            setForkConflict(null);
            if (target) {
              await handleForkToOrbit(target.skill, target.scope, { replace: true });
            }
          }}
        />
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
                  <span className="inline-flex items-center gap-2 font-mono">
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
