import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  BookOpen,
  ChevronDown,
  ChevronRight,
  Code2,
  GitFork,
  Import,
  Loader2,
  Plus,
  RefreshCw,
  Save,
  Search,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import {
  SettingCard,
  SettingSectionHeader,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { isImeKeyEvent } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import {
  type ManagedSkillRecord,
  type SkillSource,
  type SkillWriteScope,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type SkillSourceFilter = "all" | "user" | "project" | "claude" | "codex" | "agy" | "builtin";

const EMPTY_SKILLS: readonly ManagedSkillRecord[] = [];

const SOURCE_FILTERS: { id: SkillSourceFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "user", label: "User" },
  { id: "project", label: "Project" },
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
  { id: "agy", label: "AGY" },
  { id: "builtin", label: "Built-in" },
];

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

function sourceBadgeLabel(source: SkillSource): string {
  switch (source) {
    case "orbit_user":
      return "User";
    case "orbit_project":
      return "Project";
    case "claude_user":
      return "Claude";
    case "claude_project":
      return "Claude · Project";
    case "codex_user":
      return "Codex";
    case "antigravity_user":
      return "AGY";
    case "builtin":
      return "Built-in";
  }
}

function sourceBadgeClass(source: SkillSource): string {
  switch (source) {
    case "orbit_user":
    case "orbit_project":
      return "border-foreground/15 bg-foreground/[0.06] text-foreground/85";
    case "claude_user":
    case "claude_project":
      return "border-amber-500/25 bg-amber-500/10 text-amber-700 dark:text-amber-300";
    case "codex_user":
      return "border-emerald-500/25 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
    case "antigravity_user":
      return "border-sky-500/25 bg-sky-500/10 text-sky-700 dark:text-sky-300";
    case "builtin":
      return "border-border bg-muted/60 text-muted-foreground";
  }
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

function formatTokenCount(tokens: number): string {
  if (tokens >= 1000) {
    return `${(tokens / 1000).toFixed(1)}k tok`;
  }
  return `${tokens} tok`;
}

export function HostSkillsSection({ hostId }: { hostId: string }) {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostCard = fleet.hosts.find((h) => h.host_id === hostId) ?? null;
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedWorkspaceCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const storeSkills = hostDetail?.managed_skills ?? EMPTY_SKILLS;

  const activeCwd = useMemo(() => {
    const normalize = (raw?: string | null) => {
      const t = raw?.trim();
      return t && t !== "~" ? t : null;
    };
    if (selectedHostId === hostId) {
      const c = normalize(selectedWorkspaceCwd);
      if (c) return c;
    }
    const ws = hostDetail?.workspaces[0];
    return normalize(ws?.cwd);
  }, [selectedHostId, hostId, selectedWorkspaceCwd, hostDetail?.workspaces]);

  const showHostBadge =
    Boolean(hostCard) &&
    (fleet.hosts.length > 1 || (hostCard !== null && !isLocalHostCard(hostCard)));

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

  // Expanded / inline editor state
  const [selectedSkillId, setSelectedSkillId] = useState<string | null>(null);
  const [loadedSkillId, setLoadedSkillId] = useState<string | null>(null);
  const [loadingContent, setLoadingContent] = useState(false);
  const [loadedContent, setLoadedContent] = useState("");
  const [editorContent, setEditorContent] = useState("");
  const [loadedEtag, setLoadedEtag] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [conflictMessage, setConflictMessage] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [pendingSkillSwitch, setPendingSkillSwitch] = useState<ManagedSkillRecord | null>(null);
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
        orbitActions.showToast(`Failed to load skills: ${String(err)}`);
      }
    } finally {
      if (listSeqRef.current === seq) {
        setLoading(false);
      }
    }
  }, [hostId, activeCwd]);

  // Reset editor/selection cleanly when host or active project cwd changes
  useEffect(() => {
    hasLoadedScopedRef.current = false;
    clearEditorState();
    void refreshSkills();
  }, [hostId, activeCwd, clearEditorState, refreshSkills]);

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
          orbitActions.showToast(`Failed to read skill: ${String(err)}`);
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

  const applySelectSkill = useCallback(
    (skill: ManagedSkillRecord) => {
      if (selectedSkillId === skill.id) {
        clearEditorState();
        return;
      }
      setSelectedSkillId(skill.id);
      selectedSkillIdRef.current = skill.id;
      void loadSkillContent(skill.id);
    },
    [selectedSkillId, clearEditorState, loadSkillContent],
  );

  const handleSelectSkill = (skill: ManagedSkillRecord) => {
    if (isDirty) {
      setPendingSkillSwitch(skill);
      return;
    }
    applySelectSkill(skill);
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
    const targetSlug = selectedSkill.slug;
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
      orbitActions.showToast(`Saved /${targetSlug}`);
    } catch (err) {
      const msg = String(err);
      if (
        selectedSkillIdRef.current === targetId &&
        readSeqRef.current === seq &&
        (msg.toLowerCase().includes("etag") || msg.toLowerCase().includes("conflict"))
      ) {
        setConflictMessage("File changed on disk. Reload before saving to avoid overwriting.");
      } else {
        orbitActions.showToast(`Save failed: ${msg}`);
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
      orbitActions.showToast(`Toggle failed: ${String(err)}`);
    } finally {
      setTogglingId(null);
    }
  };

  const handleDeleteSkill = async (skill: ManagedSkillRecord) => {
    try {
      const updated = await orbitIpc.deleteManagedSkill(hostId, skill.id, activeCwd);
      hasLoadedScopedRef.current = true;
      setSkills(updated);
      clearEditorState();
      orbitActions.showToast(`Deleted /${skill.slug}`);
    } catch (err) {
      orbitActions.showToast(`Delete failed: ${String(err)}`);
    }
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
      setSelectedSkillId(nextId);
      selectedSkillIdRef.current = nextId;
      void loadSkillContent(nextId);
      orbitActions.showToast(
        `Forked /${skill.slug} to Orbit ${scope === "project" ? "Project" : "User"}`,
      );
    } catch (err) {
      orbitActions.showToast(`Fork failed: ${String(err)}`);
    } finally {
      setForking(false);
    }
  };

  const handleCreateSkill = async () => {
    const slug = newSlug.trim();
    if (!SLUG_RE.test(slug)) {
      orbitActions.showToast("Slug must match ^[a-z0-9][a-z0-9_-]{0,63}$");
      return;
    }
    if (newScope === "project" && !activeCwd) {
      orbitActions.showToast("Open a project workspace first for project-scoped skills");
      return;
    }
    setCreating(true);
    try {
      const name = newName.trim() || slug;
      const desc = newDesc.trim();
      const body =
        newBody.trim() || `# ${name}\n\nDescribe the workflow, constraints, and verification steps.`;
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
      setSelectedSkillId(createdId);
      selectedSkillIdRef.current = createdId;
      void loadSkillContent(createdId);
      orbitActions.showToast(`Created /${slug}`);
    } catch (err) {
      orbitActions.showToast(`Create failed: ${String(err)}`);
    } finally {
      setCreating(false);
    }
  };

  const handleImportSkill = async () => {
    if (!importSource.trim()) {
      orbitActions.showToast("Paste Markdown or enter a local SKILL.md path");
      return;
    }
    if (importScope === "project" && !activeCwd) {
      orbitActions.showToast("Open a project workspace first for project-scoped import");
      return;
    }
    const slugOverride = importSlugOverride.trim() || null;
    if (slugOverride && !SLUG_RE.test(slugOverride)) {
      orbitActions.showToast("Slug override must match ^[a-z0-9][a-z0-9_-]{0,63}$");
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
        setSelectedSkillId(newlyAdded.id);
        selectedSkillIdRef.current = newlyAdded.id;
        void loadSkillContent(newlyAdded.id);
        orbitActions.showToast(`Imported /${newlyAdded.slug}`);
      } else {
        orbitActions.showToast("Imported skill");
      }
    } catch (err) {
      orbitActions.showToast(`Import failed: ${String(err)}`);
    } finally {
      setImporting(false);
    }
  };

  const filterCounts = useMemo(() => {
    const counts: Record<SkillSourceFilter, number> = {
      all: skills.length,
      user: 0,
      project: 0,
      claude: 0,
      codex: 0,
      agy: 0,
      builtin: 0,
    };
    for (const s of skills) {
      for (const f of SOURCE_FILTERS) {
        if (f.id !== "all" && matchesSourceFilter(s.source, f.id)) {
          counts[f.id] += 1;
        }
      }
    }
    return counts;
  }, [skills]);

  const filteredSkills = useMemo(() => {
    const q = search.trim().toLowerCase();
    return skills.filter((s) => {
      if (!matchesSourceFilter(s.source, sourceFilter)) return false;
      if (!q) return true;
      return (
        s.slug.toLowerCase().includes(q) ||
        s.name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q) ||
        s.triggers.some((t) => t.toLowerCase().includes(q))
      );
    });
  }, [skills, sourceFilter, search]);

  const enabledCount = useMemo(() => skills.filter((s) => s.enabled).length, [skills]);

  return (
    <div className="space-y-5">
      <SettingSectionHeader
        title="Skills"
        description={`Discover, toggle, edit, and import SKILL.md workflows (${enabledCount}/${skills.length} enabled).`}
        badge={
          showHostBadge && hostCard ? (
            <span className="bg-muted text-muted-foreground rounded-md px-2 py-0.5 text-[11.5px] font-medium">
              {hostCard.display_name}
            </span>
          ) : null
        }
        action={
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={() => setActiveDrawer((d) => (d === "import" ? "none" : "import"))}
              className={cn(
                "border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors",
                activeDrawer === "import" && "bg-accent",
              )}
            >
              <Import className="h-[13px] w-[13px]" />
              <span>Import</span>
            </button>
            <button
              type="button"
              onClick={() => setActiveDrawer((d) => (d === "create" ? "none" : "create"))}
              className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-2.5 text-[12px] font-medium transition-opacity hover:opacity-90"
            >
              <Plus className="h-[13px] w-[13px]" />
              <span>New Skill</span>
            </button>
            <button
              type="button"
              aria-label="Refresh skills"
              disabled={loading}
              onClick={() => void refreshSkills()}
              className="border-border hover:bg-accent inline-flex h-[30px] w-[30px] cursor-pointer items-center justify-center rounded-lg border transition-colors disabled:opacity-50"
            >
              <RefreshCw className={cn("h-[13px] w-[13px]", loading && "animate-spin")} />
            </button>
          </div>
        }
      />

      {/* Create Skill Drawer */}
      {activeDrawer === "create" && (
        <div className="border-border bg-background/80 space-y-3 rounded-[13px] border p-4 text-[12.5px]">
          <div className="flex items-center justify-between">
            <span className="text-foreground text-[13px] font-medium">New Skill</span>
            <button
              type="button"
              aria-label="Close new skill drawer"
              onClick={() => setActiveDrawer("none")}
              className="text-muted-foreground hover:text-foreground cursor-pointer rounded p-1"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>

          <div className="flex items-center gap-1.5">
            {(["user", "project"] as const).map((sc) => {
              const disabled = sc === "project" && !activeCwd;
              return (
                <button
                  key={sc}
                  type="button"
                  disabled={disabled}
                  title={
                    disabled
                      ? "Select a project workspace first"
                      : sc === "user"
                        ? "~/.orbit/skills"
                        : `${activeCwd}/.orbit/skills`
                  }
                  onClick={() => setNewScope(sc)}
                  className={cn(
                    "cursor-pointer rounded-md border px-2.5 py-1 text-[11.5px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40",
                    newScope === sc
                      ? "border-foreground/20 bg-foreground text-background"
                      : "border-border text-muted-foreground hover:text-foreground",
                  )}
                >
                  {sc === "user" ? "User (~/.orbit/skills)" : "Project (.orbit/skills)"}
                </button>
              );
            })}
          </div>

          <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
            <input
              value={newName}
              onChange={(e) => {
                const next = e.target.value;
                setNewName(next);
                if (!slugTouched) setNewSlug(slugifyName(next));
              }}
              placeholder="Name (e.g. Release Checklist)"
              className="border-border bg-canvas rounded-lg border px-2.5 py-1.5 outline-none"
            />
            <input
              value={newSlug}
              onChange={(e) => {
                setSlugTouched(true);
                setNewSlug(e.target.value.toLowerCase());
              }}
              placeholder="slug (e.g. release-checklist)"
              className="border-border bg-canvas rounded-lg border px-2.5 py-1.5 font-mono text-[12px] outline-none"
            />
          </div>

          <input
            value={newDesc}
            onChange={(e) => setNewDesc(e.target.value)}
            placeholder="Short description"
            className="border-border bg-canvas w-full rounded-lg border px-2.5 py-1.5 outline-none"
          />

          <textarea
            rows={5}
            value={newBody}
            onChange={(e) => setNewBody(e.target.value)}
            placeholder="Markdown instructions (YAML frontmatter is added automatically if omitted)"
            className="border-border bg-canvas w-full rounded-lg border px-2.5 py-2 font-mono text-[12px] leading-relaxed outline-none"
          />

          <div className="flex items-center justify-end gap-2">
            <button
              type="button"
              onClick={() => setActiveDrawer("none")}
              className="border-border hover:bg-accent cursor-pointer rounded-lg border px-2.5 py-1 text-[12px]"
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={creating}
              onClick={() => void handleCreateSkill()}
              className="bg-foreground text-background inline-flex cursor-pointer items-center gap-1.5 rounded-lg px-3 py-1 text-[12px] font-medium disabled:opacity-50"
            >
              {creating && <Loader2 className="h-3 w-3 animate-spin" />}
              <span>Create Skill</span>
            </button>
          </div>
        </div>
      )}

      {/* Import Markdown Drawer */}
      {activeDrawer === "import" && (
        <div className="border-border bg-background/80 space-y-3 rounded-[13px] border p-4 text-[12.5px]">
          <div className="flex items-center justify-between">
            <span className="text-foreground text-[13px] font-medium">Import Markdown</span>
            <button
              type="button"
              aria-label="Close import drawer"
              onClick={() => setActiveDrawer("none")}
              className="text-muted-foreground hover:text-foreground cursor-pointer rounded p-1"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>

          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-1.5">
              {(["user", "project"] as const).map((sc) => {
                const disabled = sc === "project" && !activeCwd;
                return (
                  <button
                    key={sc}
                    type="button"
                    disabled={disabled}
                    onClick={() => setImportScope(sc)}
                    className={cn(
                      "cursor-pointer rounded-md border px-2.5 py-1 text-[11.5px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40",
                      importScope === sc
                        ? "border-foreground/20 bg-foreground text-background"
                        : "border-border text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {sc === "user" ? "User" : "Project"}
                  </button>
                );
              })}
            </div>
            <input
              value={importSlugOverride}
              onChange={(e) => setImportSlugOverride(e.target.value.toLowerCase())}
              placeholder="Slug override (optional)"
              className="border-border bg-canvas w-48 rounded-lg border px-2.5 py-1 font-mono text-[11.5px] outline-none"
            />
          </div>

          <textarea
            rows={5}
            value={importSource}
            onChange={(e) => setImportSource(e.target.value)}
            placeholder="Paste SKILL.md Markdown content, or enter a local .md path inside your active workspace or skills directory"
            className="border-border bg-canvas w-full rounded-lg border px-2.5 py-2 font-mono text-[12px] leading-relaxed outline-none"
          />

          <div className="flex items-center justify-end gap-2">
            <button
              type="button"
              onClick={() => setActiveDrawer("none")}
              className="border-border hover:bg-accent cursor-pointer rounded-lg border px-2.5 py-1 text-[12px]"
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={importing}
              onClick={() => void handleImportSkill()}
              className="bg-foreground text-background inline-flex cursor-pointer items-center gap-1.5 rounded-lg px-3 py-1 text-[12px] font-medium disabled:opacity-50"
            >
              {importing && <Loader2 className="h-3 w-3 animate-spin" />}
              <span>Import Skill</span>
            </button>
          </div>
        </div>
      )}

      {/* Search + Source Filter Pills */}
      <div className="space-y-2.5">
        <div className="relative">
          <Search className="text-muted-foreground pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2" />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search skills by name, slug, or trigger…"
            className="border-border bg-background/60 text-foreground placeholder:text-muted-foreground/75 h-[34px] w-full rounded-[10px] border pr-8 pl-8.5 text-[12.5px] outline-none focus:border-foreground/30"
          />
          {search && (
            <button
              type="button"
              aria-label="Clear search"
              onClick={() => setSearch("")}
              className="text-muted-foreground hover:text-foreground absolute top-1/2 right-2.5 -translate-y-1/2 cursor-pointer"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          {SOURCE_FILTERS.map((f) => {
            const active = sourceFilter === f.id;
            const count = filterCounts[f.id];
            return (
              <button
                key={f.id}
                type="button"
                onClick={() => setSourceFilter(f.id)}
                className={cn(
                  "inline-flex h-[26px] cursor-pointer items-center gap-1.5 rounded-full border px-2.5 text-[11.5px] font-medium transition-colors",
                  active
                    ? "border-foreground/20 bg-foreground text-background"
                    : "border-border bg-background/50 text-muted-foreground hover:bg-accent hover:text-foreground",
                )}
              >
                <span>{f.label}</span>
                <span
                  className={cn(
                    "text-[10.5px] tabular-nums",
                    active ? "text-background/75" : "text-muted-foreground/70",
                  )}
                >
                  {count}
                </span>
              </button>
            );
          })}
        </div>
      </div>

      {/* Discovered Skills List + Inline Editor/Preview */}
      <SettingCard>
        {filteredSkills.length === 0 ? (
          <div className="text-muted-foreground px-4 py-6 text-center text-[12.5px]">
            {skills.length === 0
              ? "No skills discovered yet. Create or import a SKILL.md workflow to get started."
              : "No skills match the current filter."}
          </div>
        ) : (
          filteredSkills.map((skill) => {
            const expanded = selectedSkillId === skill.id;
            const deletingThis = confirmDeleteId === skill.id;
            return (
              <div key={skill.id} className="transition-colors">
                <div className="flex items-start justify-between gap-3 px-4 py-3">
                  <button
                    type="button"
                    onClick={() => handleSelectSkill(skill)}
                    className="flex min-w-0 flex-1 cursor-pointer items-start gap-2.5 text-left"
                  >
                    <span className="text-muted-foreground mt-0.5 shrink-0">
                      {expanded ? (
                        <ChevronDown className="h-3.5 w-3.5" />
                      ) : (
                        <ChevronRight className="h-3.5 w-3.5" />
                      )}
                    </span>
                    <div className="min-w-0 flex-1 space-y-1">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <span
                          className={cn(
                            "text-[13px] font-medium",
                            skill.enabled ? "text-foreground" : "text-muted-foreground",
                          )}
                        >
                          {skill.name}
                        </span>
                        <code className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 font-mono text-[11px]">
                          /{skill.slug}
                        </code>
                        <span
                          className={cn(
                            "rounded border px-1.5 py-0.5 text-[10.5px] font-medium",
                            sourceBadgeClass(skill.source),
                          )}
                        >
                          {sourceBadgeLabel(skill.source)}
                        </span>
                        <span className="text-muted-foreground/80 text-[10.5px] tabular-nums">
                          ~{formatTokenCount(skill.approx_tokens)}
                        </span>
                        {skill.has_scripts && (
                          <span
                            title="Contains scripts/ directory"
                            className="bg-muted/70 text-muted-foreground inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px]"
                          >
                            <Code2 className="h-2.5 w-2.5" />
                            <span>scripts</span>
                          </span>
                        )}
                        {skill.has_references && (
                          <span
                            title="Contains references/ directory"
                            className="bg-muted/70 text-muted-foreground inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px]"
                          >
                            <BookOpen className="h-2.5 w-2.5" />
                            <span>refs</span>
                          </span>
                        )}
                      </div>
                      {skill.description && (
                        <p className="text-muted-foreground line-clamp-1 text-[12px] leading-snug">
                          {skill.description}
                        </p>
                      )}
                    </div>
                  </button>

                  <div className="shrink-0 pt-0.5">
                    <ToggleSwitch
                      checked={skill.enabled}
                      disabled={togglingId === skill.id}
                      ariaLabel={`Enable ${skill.name}`}
                      onChange={(next) => void handleToggleEnabled(skill, next)}
                    />
                  </div>
                </div>

                {/* Inline SKILL.md Editor / Read-only Preview */}
                {expanded && (
                  <div className="border-border/70 bg-muted/20 space-y-3 border-t px-4 py-3.5">
                    <div className="flex flex-wrap items-center justify-between gap-2 text-[11.5px]">
                      <div
                        className="text-muted-foreground min-w-0 truncate font-mono"
                        title={skill.path}
                      >
                        {skill.path}
                      </div>
                      <div className="flex items-center gap-1.5">
                        {skill.editable ? (
                          <>
                            {isDirty && (
                              <span className="text-[11px] font-medium text-amber-600 dark:text-amber-400">
                                Unsaved
                              </span>
                            )}
                            {isDirty && (
                              <button
                                type="button"
                                onClick={() => setEditorContent(loadedContent)}
                                className="border-border hover:bg-accent cursor-pointer rounded-md border px-2 py-1 text-[11.5px]"
                              >
                                Revert
                              </button>
                            )}
                            <button
                              type="button"
                              disabled={!isDirty || saving}
                              onClick={() => void handleSaveEditor()}
                              className="bg-foreground text-background inline-flex cursor-pointer items-center gap-1 rounded-md px-2.5 py-1 text-[11.5px] font-medium disabled:opacity-40"
                            >
                              {saving ? (
                                <Loader2 className="h-3 w-3 animate-spin" />
                              ) : (
                                <Save className="h-3 w-3" />
                              )}
                              <span>Save</span>
                            </button>
                            {deletingThis ? (
                              <div className="flex items-center gap-1">
                                <button
                                  type="button"
                                  onClick={() => void handleDeleteSkill(skill)}
                                  className="cursor-pointer rounded-md bg-rose-600 px-2 py-1 text-[11.5px] font-medium text-white hover:bg-rose-700"
                                >
                                  Confirm Delete
                                </button>
                                <button
                                  type="button"
                                  onClick={() => setConfirmDeleteId(null)}
                                  className="border-border hover:bg-accent cursor-pointer rounded-md border px-2 py-1 text-[11.5px]"
                                >
                                  Cancel
                                </button>
                              </div>
                            ) : (
                              <button
                                type="button"
                                aria-label={`Delete ${skill.name}`}
                                onClick={() => setConfirmDeleteId(skill.id)}
                                className="text-muted-foreground hover:text-rose-500 hover:bg-accent cursor-pointer rounded-md p-1.5"
                              >
                                <Trash2 className="h-3.5 w-3.5" />
                              </button>
                            )}
                          </>
                        ) : (
                          <>
                            <span className="text-muted-foreground text-[11px]">Read-only</span>
                            <button
                              type="button"
                              disabled={forking}
                              onClick={() => void handleForkToOrbit(skill, "user")}
                              className="border-border bg-background hover:bg-accent inline-flex cursor-pointer items-center gap-1.5 rounded-md border px-2.5 py-1 text-[11.5px] font-medium disabled:opacity-50"
                            >
                              {forking ? (
                                <Loader2 className="h-3 w-3 animate-spin" />
                              ) : (
                                <GitFork className="h-3 w-3" />
                              )}
                              <span>Fork to Orbit</span>
                            </button>
                            {activeCwd && (
                              <button
                                type="button"
                                disabled={forking}
                                onClick={() => void handleForkToOrbit(skill, "project")}
                                className="border-border bg-background hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-md border px-2 py-1 text-[11.5px] disabled:opacity-50"
                              >
                                <span>Fork to Project</span>
                              </button>
                            )}
                          </>
                        )}
                      </div>
                    </div>

                    {(skill.triggers.length > 0 || skill.allowed_tools.length > 0) && (
                      <div className="flex flex-wrap items-center gap-1.5 text-[11px]">
                        {skill.triggers.map((t) => (
                          <span
                            key={`t:${t}`}
                            className="bg-background/80 border-border text-muted-foreground rounded border px-1.5 py-0.5"
                          >
                            #{t}
                          </span>
                        ))}
                        {skill.allowed_tools.map((tool) => (
                          <span
                            key={`tool:${tool}`}
                            className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 font-mono text-[10.5px]"
                          >
                            {tool}
                          </span>
                        ))}
                      </div>
                    )}

                    {conflictMessage && (
                      <div className="flex items-center justify-between gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-[12px] text-amber-700 dark:text-amber-300">
                        <span className="flex items-center gap-1.5">
                          <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                          <span>{conflictMessage}</span>
                        </span>
                        <button
                          type="button"
                          onClick={() => void loadSkillContent(skill.id)}
                          className="cursor-pointer font-medium underline"
                        >
                          Reload
                        </button>
                      </div>
                    )}

                    {loadingContent || loadedSkillId !== skill.id ? (
                      <div className="text-muted-foreground flex items-center gap-2 py-6 text-[12px]">
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        <span>Loading SKILL.md…</span>
                      </div>
                    ) : skill.editable ? (
                      <div className="space-y-1.5">
                        <textarea
                          rows={12}
                          value={editorContent}
                          onChange={(e) => setEditorContent(e.target.value)}
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
                          spellCheck={false}
                          className="border-border bg-canvas text-foreground w-full rounded-lg border p-3 font-mono text-[12px] leading-relaxed outline-none focus:border-foreground/30"
                        />
                        <div className="text-muted-foreground/75 flex items-center justify-between text-[11px]">
                          <span className="inline-flex items-center gap-1">
                            <Sparkles className="h-3 w-3" />
                            <span>Type /{skill.slug} in the composer to invoke</span>
                          </span>
                          <span>{modSymbol()}S to save</span>
                        </div>
                      </div>
                    ) : (
                      <pre className="border-border bg-canvas/80 text-foreground/90 max-h-72 overflow-auto rounded-lg border p-3 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap select-text">
                        {loadedContent}
                      </pre>
                    )}
                  </div>
                )}
              </div>
            );
          })
        )}
      </SettingCard>

      <ConfirmDialog
        open={pendingSkillSwitch !== null}
        onOpenChange={(open) => {
          if (!open) setPendingSkillSwitch(null);
        }}
        title="Discard unsaved changes?"
        description={
          selectedSkill
            ? `Your unsaved edits to /${selectedSkill.slug} will be lost.`
            : "Your unsaved changes to this skill will be lost."
        }
        confirmLabel="Discard"
        tone="danger"
        onConfirm={() => {
          const target = pendingSkillSwitch;
          setPendingSkillSwitch(null);
          if (target) applySelectSkill(target);
        }}
      />
    </div>
  );
}
