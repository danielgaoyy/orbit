import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  Check,
  Copy,
  FileCode2,
  Link2,
  Loader2,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
} from "lucide-react";
import { ScrollArea } from "@/components/ui/scroll-area";
import { SettingSelect, type SettingSelectOption } from "@/components/settings/settings-primitives";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import {
  type InstructionFileEntry,
  type InstructionFilesSnapshotView,
  type InstructionScope,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const SCOPE_LABELS: Record<InstructionScope, string> = {
  global_user: "Global",
  repo_root: "Repo",
  ancestor_dir: "Ancestor",
  workspace_dir: "Local",
  local_override: "Override",
};

const SCOPE_TONE_CLASSES: Record<InstructionScope, string> = {
  global_user: "bg-foreground/[0.06] text-muted-foreground border-border/60",
  repo_root: "bg-sky-500/[0.08] text-sky-700 border-sky-500/20 dark:text-sky-300",
  ancestor_dir: "bg-violet-500/[0.08] text-violet-700 border-violet-500/20 dark:text-violet-300",
  workspace_dir: "bg-emerald-500/[0.08] text-emerald-700 border-emerald-500/20 dark:text-emerald-300",
  local_override: "bg-amber-500/[0.10] text-amber-700 border-amber-500/25 dark:text-amber-300",
};

const PROVIDER_LABELS: Record<string, string> = {
  universal: "Universal",
  orbit: "Orbit",
  claude_code: "Claude Code",
  codex: "Codex",
  gemini_cli: "Gemini CLI",
  cursor_agent: "Cursor",
  copilot: "Copilot",
};

function estimateTokensClient(text: string): number {
  if (!text) return 0;
  let cjk = 0;
  let other = 0;
  for (let i = 0; i < text.length; i += 1) {
    const code = text.charCodeAt(i);
    if (
      (code >= 0x3040 && code <= 0x30ff) ||
      (code >= 0x3400 && code <= 0x4dbf) ||
      (code >= 0x4e00 && code <= 0x9fff) ||
      (code >= 0xac00 && code <= 0xd7af) ||
      (code >= 0xf900 && code <= 0xfaff)
    ) {
      cjk += 1;
    } else {
      other += 1;
    }
  }
  return Math.max(1, Math.ceil((cjk * 5 + other) / 4));
}

function formatTokensCompact(tokens: number): string {
  if (tokens >= 1000) {
    return `${(tokens / 1000).toFixed(1)}k tok`;
  }
  return `${tokens} tok`;
}

function starterTemplateForFilename(filename: string): string {
  switch (filename) {
    case "AGENTS.md":
      return [
        "# AGENTS.md",
        "",
        "## Architecture & Conventions",
        "- Keep changes focused, minimal, and consistent with existing patterns.",
        "- Add or update tests for behavioral changes.",
        "",
        "## Verification",
        "- Run lint, typecheck, and tests before finishing.",
        "",
      ].join("\n");
    case "CLAUDE.md":
      return [
        "# CLAUDE.md",
        "",
        "## Project Overview",
        "- Describe the stack, directory layout, and entrypoints.",
        "",
        "## Commands",
        "- Build: ``",
        "- Test: ``",
        "",
      ].join("\n");
    case "AGENTS.override.md":
    case "AGENTS.local.md":
    case "CLAUDE.local.md":
      return [
        "# Local Overrides",
        "",
        "<!-- Machine- or user-specific rules (highest precedence). -->",
        "- Prefer concise diffs and verify locally.",
        "",
      ].join("\n");
    default:
      return "# Instructions\n\n- Add workspace agent guidelines here.\n";
  }
}

function isEtagConflictMessage(message: string): boolean {
  const lower = message.toLowerCase();
  return lower.includes("etag_conflict") || lower.includes("changed on disk");
}

function compactDirLabel(cwd: string): string {
  const trimmed = cwd.replace(/\/+$/, "");
  const parts = trimmed.split("/").filter(Boolean);
  if (parts.length <= 2) return trimmed || "/";
  return `…/${parts.slice(-2).join("/")}`;
}

export function InstructionsSection() {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedWorkspaceCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const instructionsTarget = useOrbitSelector((s) => s.instructionsTarget);
  const activeView = useOrbitSelector((s) => s.activeView);
  const commandPaletteOpen = useOrbitSelector((s) => s.commandPaletteOpen);

  const ime = useImeCompositionGuard();
  const mod = modSymbol();

  const defaultHostId =
    instructionsTarget?.hostId ?? selectedHostId ?? fleet.hosts[0]?.host_id ?? null;

  const [activeHostId, setActiveHostId] = useState<string | null>(defaultHostId);

  useEffect(() => {
    if (instructionsTarget?.hostId) {
      setActiveHostId(instructionsTarget.hostId);
    }
  }, [instructionsTarget?.hostId]);

  const workspaceOptions = useMemo(() => {
    if (!activeHostId) return [];
    const detail = hostDetails[activeHostId];
    const seen = new Set<string>();
    const dirs: string[] = [];

    const pushDir = (raw: string | null | undefined) => {
      const cleaned = raw?.trim();
      if (!cleaned || seen.has(cleaned)) return;
      seen.add(cleaned);
      dirs.push(cleaned);
    };

    if (instructionsTarget && instructionsTarget.hostId === activeHostId) {
      pushDir(instructionsTarget.cwd);
    }
    if (activeHostId === selectedHostId) {
      if (selectedAgentId && detail) {
        const ag = (detail.agents ?? []).find((a) => a.agent_id === selectedAgentId);
        pushDir(ag?.cwd);
      }
      pushDir(selectedWorkspaceCwd);
    }
    if (detail) {
      for (const ws of detail.workspaces ?? []) {
        if (!ws.archived_at) pushDir(ws.cwd);
      }
      for (const proj of detail.discovered_projects ?? []) {
        pushDir(proj.path);
      }
      for (const ag of detail.agents ?? []) {
        pushDir(ag.cwd);
      }
    }
    return dirs;
  }, [
    activeHostId,
    hostDetails,
    instructionsTarget,
    selectedHostId,
    selectedAgentId,
    selectedWorkspaceCwd,
  ]);

  const [activeCwd, setActiveCwd] = useState<string | null>(
    () => instructionsTarget?.cwd ?? workspaceOptions[0] ?? null,
  );

  useEffect(() => {
    if (instructionsTarget?.cwd && instructionsTarget.hostId === activeHostId) {
      setActiveCwd(instructionsTarget.cwd);
      return;
    }
    if (!activeCwd || !workspaceOptions.includes(activeCwd)) {
      setActiveCwd(workspaceOptions[0] ?? null);
    }
  }, [activeHostId, activeCwd, instructionsTarget, workspaceOptions]);

  const [snapshot, setSnapshot] = useState<InstructionFilesSnapshotView | null>(null);
  const [listLoading, setListLoading] = useState(false);
  const [listError, setListError] = useState<string | null>(null);

  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [fileLoading, setFileLoading] = useState(false);
  const [loadedContent, setLoadedContent] = useState("");
  const [draftContent, setDraftContent] = useState("");
  const [loadedLineEnding, setLoadedLineEnding] = useState<"lf" | "crlf">("lf");
  const [loadedEtag, setLoadedEtag] = useState<string | null>(null);
  const [loadedExists, setLoadedExists] = useState(false);

  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [etagConflict, setEtagConflict] = useState(false);
  const [copiedMerged, setCopiedMerged] = useState(false);
  const [justSaved, setJustSaved] = useState(false);

  const listSeqRef = useRef(0);
  const readSeqRef = useRef(0);
  const savedTimerRef = useRef<number | null>(null);
  const copiedTimerRef = useRef<number | null>(null);
  const focusTimerRef = useRef<number | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  // Clean up all timers on unmount
  useEffect(() => {
    return () => {
      if (savedTimerRef.current !== null) window.clearTimeout(savedTimerRef.current);
      if (copiedTimerRef.current !== null) window.clearTimeout(copiedTimerRef.current);
      if (focusTimerRef.current !== null) window.clearTimeout(focusTimerRef.current);
    };
  }, []);

  const dirty = draftContent !== loadedContent || (!loadedExists && draftContent.length > 0);

  const confirmDiscardIfDirty = useCallback((): boolean => {
    if (!dirty) return true;
    return window.confirm("Discard unsaved changes to this instruction file?");
  }, [dirty]);

  const selectedEntry: InstructionFileEntry | null = useMemo(() => {
    if (!snapshot || !selectedPath) return null;
    return (snapshot.files ?? []).find((f) => f.path === selectedPath) ?? null;
  }, [snapshot, selectedPath]);

  const loadFile = useCallback(
    async (
      hostId: string,
      cwd: string,
      entry: InstructionFileEntry,
      opts?: { prefillTemplate?: boolean },
    ) => {
      const seq = ++readSeqRef.current;
      setSelectedPath(entry.path);
      setFileLoading(true);
      setLoadedContent("");
      setDraftContent("");
      setLoadedLineEnding("lf");
      setLoadedEtag(null);
      setLoadedExists(false);
      setSaveError(null);
      setEtagConflict(false);
      try {
        const res = await orbitIpc.readInstructionFile(hostId, cwd, entry.path);
        if (readSeqRef.current !== seq) return;
        const rawContent = res.content ?? "";
        const isCrlf = rawContent.includes("\r\n");
        const normalized = rawContent.replace(/\r\n/g, "\n");
        setLoadedLineEnding(isCrlf ? "crlf" : "lf");
        setLoadedExists(Boolean(res.exists));
        setLoadedEtag(res.etag ?? null);
        setLoadedContent(normalized);
        if (!res.exists && opts?.prefillTemplate) {
          setDraftContent(starterTemplateForFilename(entry.filename));
        } else {
          setDraftContent(normalized);
        }
      } catch (err) {
        if (readSeqRef.current !== seq) return;
        setSaveError(String(err));
      } finally {
        if (readSeqRef.current === seq) {
          setFileLoading(false);
        }
      }
    },
    [],
  );

  const loadHierarchy = useCallback(
    async (hostId: string, cwd: string, preferredPath?: string | null) => {
      const seq = ++listSeqRef.current;
      setListLoading(true);
      setListError(null);
      try {
        const rawSnap = await orbitIpc.listInstructionFiles(hostId, cwd);
        if (listSeqRef.current !== seq) return;
        const files = (rawSnap.files ?? []).map((f) => ({
          ...f,
          imports: Array.isArray(f.imports) ? f.imports : [],
        }));
        const snap: InstructionFilesSnapshotView = {
          ...rawSnap,
          files,
        };
        setSnapshot(snap);

        const nextSelection =
          (preferredPath && files.find((f) => f.path === preferredPath)) ||
          files.find((f) => f.exists) ||
          files[0] ||
          null;

        if (nextSelection) {
          void loadFile(hostId, cwd, nextSelection);
        } else {
          readSeqRef.current += 1;
          setSelectedPath(null);
          setFileLoading(false);
          setLoadedContent("");
          setDraftContent("");
          setLoadedLineEnding("lf");
          setLoadedEtag(null);
          setLoadedExists(false);
        }
      } catch (err) {
        if (listSeqRef.current !== seq) return;
        readSeqRef.current += 1;
        setSnapshot(null);
        setSelectedPath(null);
        setFileLoading(false);
        setListError(String(err));
      } finally {
        if (listSeqRef.current === seq) {
          setListLoading(false);
        }
      }
    },
    [loadFile],
  );

  useEffect(() => {
    if (!activeHostId || !activeCwd) {
      listSeqRef.current += 1;
      readSeqRef.current += 1;
      setListLoading(false);
      setFileLoading(false);
      setSnapshot(null);
      setSelectedPath(null);
      setLoadedContent("");
      setDraftContent("");
      setLoadedLineEnding("lf");
      setLoadedEtag(null);
      setLoadedExists(false);
      setSaveError(null);
      setEtagConflict(false);
      return;
    }
    void loadHierarchy(activeHostId, activeCwd);
  }, [activeHostId, activeCwd, loadHierarchy]);

  const handleSave = useCallback(
    async (overwriteAfterConflict = false) => {
      if (!activeHostId || !activeCwd || !selectedPath || saving || fileLoading) return;
      const saveReadSeq = readSeqRef.current;
      const saveListSeq = listSeqRef.current;
      const normalizedDraft = draftContent;
      const wireContent =
        loadedLineEnding === "crlf"
          ? normalizedDraft.replace(/\r?\n/g, "\r\n")
          : normalizedDraft;

      setSaving(true);
      setSaveError(null);
      try {
        let expectedEtag = loadedEtag;
        if (overwriteAfterConflict) {
          const latest = await orbitIpc.readInstructionFile(activeHostId, activeCwd, selectedPath);
          if (readSeqRef.current !== saveReadSeq) return;
          expectedEtag = latest.exists ? (latest.etag ?? null) : null;
        }
        const saved = await orbitIpc.saveInstructionFile({
          hostId: activeHostId,
          cwd: activeCwd,
          path: selectedPath,
          content: wireContent,
          expectedEtag,
        });
        if (readSeqRef.current !== saveReadSeq) return;
        setLoadedContent(normalizedDraft);
        setLoadedEtag(saved.etag);
        setLoadedExists(true);
        setEtagConflict(false);
        setJustSaved(true);
        if (savedTimerRef.current !== null) window.clearTimeout(savedTimerRef.current);
        savedTimerRef.current = window.setTimeout(() => {
          savedTimerRef.current = null;
          setJustSaved(false);
        }, 1600);

        // Refresh hierarchy metadata (tokens, imports, merged_preview) without clobbering editor
        const rawSnap = await orbitIpc.listInstructionFiles(activeHostId, activeCwd);
        if (listSeqRef.current !== saveListSeq) return;
        setSnapshot({
          ...rawSnap,
          files: (rawSnap.files ?? []).map((f) => ({
            ...f,
            imports: Array.isArray(f.imports) ? f.imports : [],
          })),
        });
      } catch (err) {
        if (readSeqRef.current !== saveReadSeq) return;
        const msg = String(err);
        if (isEtagConflictMessage(msg)) {
          setEtagConflict(true);
          setSaveError(null);
        } else {
          setSaveError(msg);
        }
      } finally {
        if (readSeqRef.current === saveReadSeq) {
          setSaving(false);
        }
      }
    },
    [
      activeHostId,
      activeCwd,
      selectedPath,
      saving,
      fileLoading,
      loadedEtag,
      loadedLineEnding,
      draftContent,
    ],
  );

  const handleReloadFromDisk = useCallback(() => {
    if (!activeHostId || !activeCwd || !selectedEntry) return;
    void loadFile(activeHostId, activeCwd, selectedEntry);
  }, [activeHostId, activeCwd, selectedEntry, loadFile]);

  const handleCopyMerged = useCallback(async () => {
    if (!snapshot?.merged_preview) return;
    try {
      await navigator.clipboard.writeText(snapshot.merged_preview);
      setCopiedMerged(true);
      if (copiedTimerRef.current !== null) window.clearTimeout(copiedTimerRef.current);
      copiedTimerRef.current = window.setTimeout(() => {
        copiedTimerRef.current = null;
        setCopiedMerged(false);
      }, 1600);
    } catch (err) {
      setSaveError(`Failed to copy merged instructions: ${String(err)}`);
    }
  }, [snapshot?.merged_preview]);

  const handleQuickCreate = useCallback(
    (entry: InstructionFileEntry) => {
      if (!activeHostId || !activeCwd) return;
      if (entry.path !== selectedPath && !confirmDiscardIfDirty()) return;
      void loadFile(activeHostId, activeCwd, entry, { prefillTemplate: true }).then(() => {
        if (focusTimerRef.current !== null) window.clearTimeout(focusTimerRef.current);
        focusTimerRef.current = window.setTimeout(() => {
          focusTimerRef.current = null;
          textareaRef.current?.focus();
        }, 30);
      });
    },
    [activeHostId, activeCwd, selectedPath, confirmDiscardIfDirty, loadFile],
  );

  // Cmd+S / Ctrl+S shortcut strictly scoped to active Settings > Instructions view
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "s") {
        if (
          e.defaultPrevented ||
          e.isComposing ||
          activeView !== "settings" ||
          commandPaletteOpen ||
          !selectedPath ||
          fileLoading ||
          saving ||
          (!dirty && loadedExists)
        ) {
          return;
        }
        e.preventDefault();
        void handleSave(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    activeView,
    commandPaletteOpen,
    selectedPath,
    fileLoading,
    saving,
    dirty,
    loadedExists,
    handleSave,
  ]);

  const hostSelectOptions = useMemo<SettingSelectOption<string>[]>(
    () =>
      fleet.hosts.map((h) => ({
        value: h.host_id,
        label: h.display_name,
        description: h.hostname !== h.display_name ? h.hostname : undefined,
      })),
    [fleet.hosts],
  );

  const cwdSelectOptions = useMemo<SettingSelectOption<string>[]>(
    () =>
      workspaceOptions.map((dir) => ({
        value: dir,
        label: compactDirLabel(dir),
        description: dir,
        mono: true,
      })),
    [workspaceOptions],
  );

  const existingFiles = useMemo(
    () => snapshot?.files.filter((f) => f.exists) ?? [],
    [snapshot],
  );

  // Prioritize workspace targets (AGENTS.md, CLAUDE.md, AGENTS.override.md) so they are never clipped behind global entries
  const missingCreateTargets = useMemo(() => {
    const missing = (snapshot?.files ?? []).filter(
      (f) =>
        !f.exists &&
        (f.filename === "AGENTS.md" ||
          f.filename === "CLAUDE.md" ||
          f.filename === "AGENTS.override.md"),
    );
    const workspaceTargets = missing.filter((f) => f.scope !== "global_user");
    const globalTargets = missing.filter((f) => f.scope === "global_user");
    return [...workspaceTargets, ...globalTargets];
  }, [snapshot]);

  const liveWireContent = useMemo(
    () =>
      loadedLineEnding === "crlf" ? draftContent.replace(/\r?\n/g, "\r\n") : draftContent,
    [draftContent, loadedLineEnding],
  );
  const liveDraftTokens = useMemo(() => estimateTokensClient(draftContent), [draftContent]);
  const liveLineCount = useMemo(
    () => (draftContent.length === 0 ? 0 : draftContent.split("\n").length),
    [draftContent],
  );
  const liveByteSize = useMemo(
    () => new TextEncoder().encode(liveWireContent).length,
    [liveWireContent],
  );

  return (
    <div className="flex h-full min-h-[540px] flex-col gap-4 select-none">
      {/* Header Bar */}
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 space-y-1">
          <div className="flex items-center gap-2.5">
            <h2 className="text-foreground text-[17.5px] font-semibold tracking-[-0.015em]">
              Project Instructions
            </h2>
            {snapshot && (
              <span className="border-border/70 bg-muted/60 text-muted-foreground inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 font-mono text-[11px] tabular-nums">
                <span>{existingFiles.length} active</span>
                <span className="opacity-45">·</span>
                <span>~{formatTokensCompact(snapshot.total_tokens)}</span>
              </span>
            )}
          </div>
          <p className="text-muted-foreground text-[12.5px]">
            Effective precedence stack from Global user rules to local overrides.
          </p>
        </div>

        <div className="flex items-center gap-1.5">
          <button
            type="button"
            disabled={!snapshot?.merged_preview}
            onClick={() => void handleCopyMerged()}
            className="border-border bg-background text-foreground/85 hover:bg-accent hover:text-foreground inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-45"
          >
            {copiedMerged ? (
              <>
                <Check className="h-[13px] w-[13px] text-emerald-500" />
                <span>Copied</span>
              </>
            ) : (
              <>
                <Copy className="h-[13px] w-[13px] opacity-75" />
                <span>Copy merged</span>
              </>
            )}
          </button>
          <button
            type="button"
            aria-label="Refresh instruction hierarchy"
            disabled={!activeHostId || !activeCwd || listLoading}
            onClick={() => {
              if (!activeHostId || !activeCwd) return;
              if (!confirmDiscardIfDirty()) return;
              void loadHierarchy(activeHostId, activeCwd, selectedPath);
            }}
            className="border-border bg-background text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[30px] w-[30px] cursor-pointer items-center justify-center rounded-lg border transition-colors disabled:cursor-not-allowed disabled:opacity-45"
          >
            <RefreshCw className={cn("h-[13.5px] w-[13.5px]", listLoading && "animate-spin")} />
          </button>
        </div>
      </div>

      {/* Host + Workspace Selector Strip */}
      <div className="flex flex-wrap items-center gap-2">
        {fleet.hosts.length > 1 && (
          <SettingSelect
            value={activeHostId}
            options={hostSelectOptions}
            onChange={(hostId) => {
              if (hostId === activeHostId) return;
              if (!confirmDiscardIfDirty()) return;
              setActiveHostId(hostId);
              orbitActions.setInstructionsTarget(null);
            }}
            placeholder="Select host…"
            triggerClassName="min-w-[140px] max-w-[190px]"
          />
        )}
        <SettingSelect
          value={activeCwd}
          options={cwdSelectOptions}
          onChange={(cwd) => {
            if (cwd === activeCwd) return;
            if (!confirmDiscardIfDirty()) return;
            setActiveCwd(cwd);
            if (activeHostId) {
              orbitActions.setInstructionsTarget({ hostId: activeHostId, cwd });
            }
          }}
          placeholder="Select workspace…"
          disabled={cwdSelectOptions.length === 0}
          triggerClassName="min-w-[220px] max-w-[420px] flex-1"
          menuClassName="max-w-[420px]"
        />
      </div>

      {/* Error / Empty Workspace Guard */}
      {!activeHostId || !activeCwd ? (
        <div className="border-border/70 bg-muted/25 text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2 rounded-xl border border-dashed p-8 text-center">
          <FileCode2 className="h-6 w-6 opacity-45" />
          <p className="text-foreground/80 text-[13px] font-medium">No workspace selected</p>
          <p className="max-w-[320px] text-[12px]">
            Open or add a project folder on a connected host to inspect and edit its instruction
            hierarchy.
          </p>
        </div>
      ) : listError ? (
        <div className="flex items-start gap-2.5 rounded-xl border border-rose-500/25 bg-rose-500/[0.06] px-3.5 py-3 text-[12.5px] text-rose-700 dark:text-rose-300">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          <div className="min-w-0 flex-1 break-words">{listError}</div>
        </div>
      ) : (
        /* Main Split Pane: Hierarchy Stack (Left) + Monospace Editor (Right) */
        <div className="border-border/80 bg-background/70 flex min-h-0 flex-1 overflow-hidden rounded-xl border">
          {/* Left Pane: Precedence Hierarchy Stack */}
          <div className="border-border/70 bg-muted/20 flex w-[248px] shrink-0 flex-col border-r">
            <div className="border-border/60 flex items-center justify-between border-b px-3 py-2">
              <span className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
                Precedence
              </span>
              <span className="text-muted-foreground/70 text-[10.5px]">Low → High</span>
            </div>

            <ScrollArea className="min-h-0 flex-1" viewportClassName="p-2">
              {listLoading && !snapshot ? (
                <div className="text-muted-foreground flex items-center justify-center gap-2 py-8 text-[12px]">
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  <span>Discovering…</span>
                </div>
              ) : (
                <div className="space-y-1">
                  {(snapshot?.files ?? []).map((entry) => {
                    const isSelected = entry.path === selectedPath;
                    const isDirtySelected = isSelected && dirty;
                    return (
                      <button
                        key={entry.id}
                        type="button"
                        onClick={() => {
                          if (!activeHostId || !activeCwd || entry.path === selectedPath) return;
                          if (!confirmDiscardIfDirty()) return;
                          void loadFile(activeHostId, activeCwd, entry, {
                            prefillTemplate: !entry.exists,
                          });
                        }}
                        className={cn(
                          "group flex w-full cursor-pointer flex-col gap-1 rounded-lg px-2.5 py-2 text-left transition-colors",
                          isSelected
                            ? "bg-foreground/[0.07] text-foreground dark:bg-white/[0.09]"
                            : "text-foreground/80 hover:bg-foreground/[0.04] dark:hover:bg-white/[0.05]",
                          !entry.exists && !isSelected && "opacity-65 hover:opacity-95",
                        )}
                      >
                        <div className="flex items-center justify-between gap-1.5">
                          <div className="flex min-w-0 items-center gap-1.5">
                            <span className="text-muted-foreground/70 w-3.5 shrink-0 font-mono text-[10.5px] tabular-nums">
                              {entry.precedence_order}
                            </span>
                            <span className="truncate font-mono text-[12px] font-medium">
                              {entry.filename}
                            </span>
                            {isDirtySelected && (
                              <span
                                className="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-500"
                                title="Unsaved changes"
                              />
                            )}
                          </div>
                          <span
                            className={cn(
                              "shrink-0 rounded border px-1.5 py-[1px] text-[10px] font-medium",
                              SCOPE_TONE_CLASSES[entry.scope],
                            )}
                          >
                            {SCOPE_LABELS[entry.scope]}
                          </span>
                        </div>

                        <div className="flex items-center justify-between gap-2 pl-5">
                          <span className="text-muted-foreground/75 truncate font-mono text-[10.5px]">
                            {entry.relative_path}
                          </span>
                          <span className="text-muted-foreground/75 shrink-0 font-mono text-[10.5px] tabular-nums">
                            {entry.exists ? formatTokensCompact(entry.approx_tokens) : "New"}
                          </span>
                        </div>

                        {(entry.imports?.length ?? 0) > 0 && (
                          <div className="text-muted-foreground/75 flex items-center gap-1 pl-5 text-[10.5px]">
                            <Link2 className="h-2.5 w-2.5 shrink-0" />
                            <span>
                              {entry.imports.length}{" "}
                              {entry.imports.length === 1 ? "import" : "imports"}
                            </span>
                          </div>
                        )}
                      </button>
                    );
                  })}
                </div>
              )}
            </ScrollArea>

            {/* Quick Create Buttons for Missing Canonical Layers */}
            {missingCreateTargets.length > 0 && (
              <div className="border-border/60 space-y-1 border-t p-2">
                {missingCreateTargets.map((target) => {
                  const createLabel =
                    target.scope === "global_user" ? target.relative_path : target.filename;
                  return (
                    <button
                      key={`create-${target.id}`}
                      type="button"
                      onClick={() => handleQuickCreate(target)}
                      className="text-muted-foreground hover:bg-accent hover:text-foreground flex w-full cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-left text-[11.5px] transition-colors"
                    >
                      <Plus className="h-3 w-3 shrink-0 opacity-75" />
                      <span className="truncate font-mono">Create {createLabel}</span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>

          {/* Right Pane: Crisp Monospace Editor */}
          <div className="flex min-w-0 flex-1 flex-col">
            {selectedEntry ? (
              <>
                {/* Editor Top Toolbar */}
                <div className="border-border/60 flex items-center justify-between gap-2 border-b px-3.5 py-2">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="text-foreground truncate font-mono text-[12px] font-medium">
                      {selectedEntry.relative_path}
                    </span>
                    <span
                      className={cn(
                        "shrink-0 rounded border px-1.5 py-[1px] text-[10px] font-medium",
                        SCOPE_TONE_CLASSES[selectedEntry.scope],
                      )}
                    >
                      {SCOPE_LABELS[selectedEntry.scope]}
                    </span>
                    {selectedEntry.provider_hint &&
                      PROVIDER_LABELS[selectedEntry.provider_hint] && (
                        <span className="text-muted-foreground/75 hidden text-[11px] sm:inline">
                          {PROVIDER_LABELS[selectedEntry.provider_hint]}
                        </span>
                      )}
                    {dirty && (
                      <span className="inline-flex items-center gap-1 text-[11px] font-medium text-amber-600 dark:text-amber-400">
                        <span className="h-1.5 w-1.5 rounded-full bg-amber-500" />
                        Unsaved
                      </span>
                    )}
                    {!dirty && justSaved && (
                      <span className="inline-flex items-center gap-1 text-[11px] font-medium text-emerald-600 dark:text-emerald-400">
                        <Check className="h-3 w-3" />
                        Saved
                      </span>
                    )}
                  </div>

                  <div className="flex shrink-0 items-center gap-1.5">
                    {dirty && (
                      <button
                        type="button"
                        onClick={() => {
                          setDraftContent(loadedContent);
                          setEtagConflict(false);
                          setSaveError(null);
                        }}
                        className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] transition-colors"
                      >
                        <RotateCcw className="h-3 w-3" />
                        <span>Revert</span>
                      </button>
                    )}
                    <button
                      type="button"
                      disabled={saving || fileLoading || (!dirty && loadedExists)}
                      onClick={() => void handleSave(false)}
                      className="bg-foreground text-background hover:bg-foreground/90 inline-flex h-[26px] cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-[11.5px] font-medium transition-opacity disabled:cursor-not-allowed disabled:opacity-40"
                    >
                      {saving ? (
                        <Loader2 className="h-3 w-3 animate-spin" />
                      ) : (
                        <Save className="h-3 w-3" />
                      )}
                      <span>{loadedExists ? "Save" : "Create"}</span>
                      <span className="opacity-65">{mod}S</span>
                    </button>
                  </div>
                </div>

                {/* Resolved @import Strip */}
                {(selectedEntry.imports?.length ?? 0) > 0 && (
                  <div className="border-border/50 bg-muted/25 flex flex-wrap items-center gap-1.5 border-b px-3.5 py-1.5">
                    <span className="text-muted-foreground inline-flex items-center gap-1 text-[11px]">
                      <Link2 className="h-3 w-3" />
                      Imports:
                    </span>
                    {(selectedEntry.imports ?? []).map((imp) => (
                      <span
                        key={imp}
                        className="border-border/60 bg-background/80 text-foreground/80 rounded border px-1.5 py-0.5 font-mono text-[10.5px]"
                      >
                        @{imp}
                      </span>
                    ))}
                  </div>
                )}

                {/* Optimistic Concurrency Etag Conflict Banner */}
                {etagConflict && (
                  <div className="flex flex-wrap items-center justify-between gap-2 border-b border-amber-500/30 bg-amber-500/[0.08] px-3.5 py-2 text-[12px] text-amber-800 dark:text-amber-300">
                    <div className="flex items-center gap-2">
                      <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                      <span>File changed on disk since it was loaded.</span>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <button
                        type="button"
                        onClick={handleReloadFromDisk}
                        className="bg-background/80 hover:bg-background cursor-pointer rounded-md border border-amber-500/35 px-2 py-0.5 text-[11.5px] font-medium transition-colors"
                      >
                        Reload from disk
                      </button>
                      <button
                        type="button"
                        disabled={saving}
                        onClick={() => void handleSave(true)}
                        className="cursor-pointer rounded-md bg-amber-600 px-2 py-0.5 text-[11.5px] font-medium text-white transition-colors hover:bg-amber-700 disabled:opacity-50"
                      >
                        Overwrite disk
                      </button>
                    </div>
                  </div>
                )}

                {/* Generic Save / Read Error Banner */}
                {saveError && (
                  <div className="flex items-center justify-between gap-2 border-b border-rose-500/25 bg-rose-500/[0.07] px-3.5 py-2 text-[12px] text-rose-700 dark:text-rose-300">
                    <span className="truncate">{saveError}</span>
                    <button
                      type="button"
                      onClick={() => setSaveError(null)}
                      className="shrink-0 cursor-pointer text-[11px] font-medium text-rose-600/80 hover:text-rose-600"
                    >
                      Dismiss
                    </button>
                  </div>
                )}

                {/* Monospace Editor Surface */}
                <div className="relative min-h-0 flex-1">
                  {fileLoading ? (
                    <div className="text-muted-foreground flex h-full items-center justify-center gap-2 text-[12.5px]">
                      <Loader2 className="h-4 w-4 animate-spin" />
                      <span>Loading file…</span>
                    </div>
                  ) : (
                    <textarea
                      ref={textareaRef}
                      value={draftContent}
                      spellCheck={false}
                      onChange={(e) => setDraftContent(e.target.value)}
                      {...ime.compositionProps}
                      onKeyDown={(e) => {
                        if (ime.shouldIgnoreKeyDown(e)) return;
                        if (e.key === "Tab") {
                          e.preventDefault();
                          const el = e.currentTarget;
                          const start = el.selectionStart;
                          const end = el.selectionEnd;
                          const next =
                            draftContent.slice(0, start) + "  " + draftContent.slice(end);
                          setDraftContent(next);
                          window.requestAnimationFrame(() => {
                            el.selectionStart = start + 2;
                            el.selectionEnd = start + 2;
                          });
                        }
                      }}
                      placeholder={`Write ${selectedEntry.filename} instructions… (supports @path/to/file.md imports)`}
                      className="selectable-text text-foreground placeholder:text-muted-foreground/45 h-full w-full resize-none bg-transparent p-3.5 font-mono text-[12.5px] leading-[1.6] outline-none"
                    />
                  )}
                </div>

                {/* Editor Footer Status */}
                <div className="border-border/60 bg-muted/15 text-muted-foreground flex items-center justify-between gap-2 border-t px-3.5 py-1.5 font-mono text-[11px] tabular-nums">
                  <span className="truncate opacity-75">{selectedEntry.path}</span>
                  <div className="flex shrink-0 items-center gap-2.5">
                    <span>{loadedLineEnding.toUpperCase()}</span>
                    <span className="opacity-40">·</span>
                    <span>{liveLineCount} lines</span>
                    <span className="opacity-40">·</span>
                    <span>{(liveByteSize / 1024).toFixed(1)} KB</span>
                    <span className="opacity-40">·</span>
                    <span>~{formatTokensCompact(liveDraftTokens)}</span>
                  </div>
                </div>
              </>
            ) : (
              <div className="text-muted-foreground flex flex-1 items-center justify-center text-[12.5px]">
                Select an instruction layer to view or edit.
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
