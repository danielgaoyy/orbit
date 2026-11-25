import {
  type Ref,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  AlertTriangle,
  Check,
  Copy,
  FileCode2,
  ExternalLink,
  Link2,
  Loader2,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
} from "lucide-react";
import { createPortal } from "react-dom";
import { useI18n, type MessageKey } from "@/lib/i18n";
import { isLocalHostCard } from "@/lib/projects";
import { Button } from "@/components/ui/button";
import { ShortcutKbd } from "@/components/ui/kbd";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import {
  type InstructionFileEntry,
  type InstructionFilesSnapshotView,
  type InstructionScope,
  isTauriRuntime,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const SCOPE_LABELS: Record<InstructionScope, MessageKey> = {
  global_user: "context.scopeGlobal",
  repo_root: "context.scopeRepo",
  ancestor_dir: "context.scopeAncestor",
  workspace_dir: "context.scopeLocal",
  local_override: "context.scopeOverride",
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

interface InstructionDraft {
  path: string;
  text: string;
  etag: string | null;
  original: string;
  exists: boolean;
}

// Renderer-session drafts are scoped to the originating host and workspace. Only
// restore a path that the daemon still lists for that workspace.
const draftsByWorkspace = new Map<string, InstructionDraft>();

export interface InstructionsGuard {
  beforeChange: (action: () => void) => void;
}

export function InstructionsSection({
  hostId: activeHostId, cwd: activeCwd, headerActions, ref,
}: {
  hostId: string | null;
  cwd: string | null;
  headerActions: HTMLDivElement | null;
  ref: Ref<InstructionsGuard>;
}) {
  const { t } = useI18n();
  const host = useOrbitSelector((s) => s.fleet.hosts.find((item) => item.host_id === activeHostId));
  const editors = useOrbitSelector((s) => s.externalEditors);
  const preferredEditor = useOrbitSelector((s) => s.preferredEditor);
  const activeView = useOrbitSelector((s) => s.activeView);
  const commandPaletteOpen = useOrbitSelector((s) => s.commandPaletteOpen);
  const installedEditors = editors.filter((editor) =>
    editor.installed && editor.kind !== "terminal" && editor.kind !== "file_manager",
  );
  const editor = installedEditors.find((item) => item.kind === preferredEditor) ?? installedEditors[0];
  const ime = useImeCompositionGuard();
  const mod = modSymbol();

  const [snapshot, setSnapshot] = useState<InstructionFilesSnapshotView | null>(null);
  const [listLoading, setListLoading] = useState(false);
  const [listError, setListError] = useState<string | null>(null);

  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [fileLoading, setFileLoading] = useState(false);
  const [fileReady, setFileReady] = useState(false);
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
  const [discardDialogOpen, setDiscardDialogOpen] = useState(false);
  const pendingDiscardActionRef = useRef<(() => void) | null>(null);

  const listSeqRef = useRef(0);
  const readSeqRef = useRef(0);
  const savedTimerRef = useRef<number | null>(null);
  const copiedTimerRef = useRef<number | null>(null);
  const focusTimerRef = useRef<number | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  // Clean up all timers on unmount
  useEffect(() => {
    return () => {
      listSeqRef.current += 1;
      readSeqRef.current += 1;
      if (savedTimerRef.current !== null) window.clearTimeout(savedTimerRef.current);
      if (copiedTimerRef.current !== null) window.clearTimeout(copiedTimerRef.current);
      if (focusTimerRef.current !== null) window.clearTimeout(focusTimerRef.current);
    };
  }, []);

  const dirty = draftContent !== loadedContent || (!loadedExists && draftContent.length > 0);

  const workspaceKey = JSON.stringify([activeHostId, activeCwd]);
  useLayoutEffect(() => {
    if (!fileReady || !selectedPath) return;
    const cached = draftsByWorkspace.get(workspaceKey);
    if (!dirty) {
      if (cached?.path === selectedPath) draftsByWorkspace.delete(workspaceKey);
      return;
    }
    // Keep the original revision until the user explicitly resolves a conflict.
    const base = etagConflict && cached?.path === selectedPath ? cached : null;
    draftsByWorkspace.set(workspaceKey, {
      path: selectedPath,
      text: draftContent,
      etag: base ? base.etag : loadedEtag,
      original: base ? base.original : loadedContent,
      exists: base ? base.exists : loadedExists,
    });
  }, [workspaceKey, selectedPath, fileReady, dirty, draftContent, loadedContent, loadedEtag, loadedExists, etagConflict]);

  const runWithDiscardGuard = useCallback(
    (action: () => void) => {
      if (!dirty) {
        action();
        return;
      }
      pendingDiscardActionRef.current = action;
      setDiscardDialogOpen(true);
    },
    [dirty],
  );

  useImperativeHandle(ref, () => ({ beforeChange: runWithDiscardGuard }), [runWithDiscardGuard]);

  const selectedEntry: InstructionFileEntry | null = useMemo(() => {
    if (!snapshot || !selectedPath) return null;
    return (snapshot.files ?? []).find((f) => f.path === selectedPath) ?? null;
  }, [snapshot, selectedPath]);

  const loadFile = useCallback(
    async (
      hostId: string,
      cwd: string,
      entry: InstructionFileEntry,
      opts?: { prefillTemplate?: boolean; restore?: InstructionDraft },
    ) => {
      const seq = ++readSeqRef.current;
      setSelectedPath(entry.path);
      setSaving(false);
      setJustSaved(false);
      setFileLoading(true);
      setFileReady(false);
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
        setFileReady(true);
        if (opts?.restore) {
          const previous = opts.restore;
          setDraftContent(previous.text);
          const changedOnDisk = previous.etag !== (res.etag ?? null) ||
            previous.exists !== res.exists || previous.original !== normalized;
          setEtagConflict(previous.text !== normalized && changedOnDisk);
        } else if (!res.exists && opts?.prefillTemplate) {
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

        const cached = draftsByWorkspace.get(workspaceKey);
        const nextPath = preferredPath ?? cached?.path;
        const nextSelection =
          (nextPath && files.find((f) => f.path === nextPath)) ||
          files.find((f) => f.exists) ||
          files[0] ||
          null;

        if (nextSelection) {
          void loadFile(hostId, cwd, nextSelection, { restore: cached?.path === nextSelection.path ? cached : undefined });
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
    [loadFile, workspaceKey],
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
      if (!activeHostId || !activeCwd || !selectedPath || saving || !fileReady || (etagConflict && !overwriteAfterConflict)) return;
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
      fileReady,
      etagConflict,
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
    } catch {
      orbitActions.showToast(t("common.copyFailed"));
    }
  }, [snapshot?.merged_preview, t]);

  const handleQuickCreate = useCallback(
    (entry: InstructionFileEntry) => {
      if (!activeHostId || !activeCwd) return;
      const openTemplate = () => {
        void loadFile(activeHostId, activeCwd, entry, { prefillTemplate: true }).then(() => {
          if (focusTimerRef.current !== null) window.clearTimeout(focusTimerRef.current);
          focusTimerRef.current = window.setTimeout(() => {
            focusTimerRef.current = null;
            textareaRef.current?.focus();
          }, 30);
        });
      };
      runWithDiscardGuard(openTemplate);
    },
    [activeHostId, activeCwd, runWithDiscardGuard, loadFile],
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
          discardDialogOpen ||
          !selectedPath ||
          !fileReady ||
          etagConflict ||
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
    discardDialogOpen,
    selectedPath,
    fileReady,
    etagConflict,
    saving,
    dirty,
    loadedExists,
    handleSave,
  ]);


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
    <div data-setting-id="settings.subTabInstructions" className="flex min-h-0 min-w-0 flex-1 flex-col gap-3">
      {headerActions && createPortal(
        <div className="flex flex-wrap items-center gap-2">
          {isTauriRuntime() && host && isLocalHostCard(host) && editor && loadedExists && selectedEntry && (
            <Button variant="outline" size="sm" className="text-[13px] @max-[640px]:w-[30px] @max-[640px]:px-0"
              aria-label={t("palette.openInEditor", { editor: editor.label })}
              title={t("palette.openInEditor", { editor: editor.label })}
              onClick={() => {
                void orbitIpc.openInEditor(editor.kind, selectedEntry.path, null)
                  .catch((err: unknown) => orbitActions.showToast(String(err)));
              }}>
              <ExternalLink data-icon="inline-start" />
              <span className="@max-[640px]:hidden">{t("palette.openInEditor", { editor: editor.label })}</span>
            </Button>
          )}
          <Button data-setting-id="context.copyMerged" variant="outline" size="sm" className="text-[13px] @max-[640px]:w-[30px] @max-[640px]:px-0"
            aria-label={t("context.copyMerged")} title={t("context.copyMerged")}
            disabled={!snapshot?.merged_preview} onClick={() => void handleCopyMerged()}>
            {copiedMerged ? <Check data-icon="inline-start" /> : <Copy data-icon="inline-start" />}
            <span className="@max-[640px]:hidden">{copiedMerged ? t("common.copied") : t("context.copyMerged")}</span>
          </Button>
          <Button variant="ghost" size="icon" aria-label={t("common.refresh")}
            disabled={!activeHostId || !activeCwd || listLoading}
            onClick={() => {
              if (activeHostId && activeCwd) {
                runWithDiscardGuard(() => { void loadHierarchy(activeHostId, activeCwd, selectedPath); });
              }
            }}>
            <RefreshCw className={cn("size-[15px]", listLoading && "animate-spin")} />
          </Button>
        </div>, headerActions)}

      {/* Error / Empty Workspace Guard */}
      {!activeHostId || !activeCwd ? (
        <div className="border-border-subtle bg-surface-recessed text-ink-muted flex flex-1 flex-col items-center justify-center gap-2 rounded-xl border border-dashed p-8 text-center">
          <FileCode2 className="h-6 w-6 opacity-45" />
          <p className="text-ink-secondary text-[13px] font-medium">{t("memory.noWorkspaceTitle")}</p>
          <p className="max-w-[320px] text-[13px]">
            {t("context.noProjectHint")}
          </p>
        </div>
      ) : listError ? (
        <div className="flex items-start gap-2.5 rounded-xl border border-signal-danger-border bg-signal-danger-bg px-3.5 py-3 text-[13px] text-signal-danger">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          <div className="min-w-0 flex-1 break-words">{listError}</div>
        </div>
      ) : (
        /* Main Split Pane: Hierarchy Stack (Left) + Monospace Editor (Right) */
        <div className="border-border-subtle bg-surface-base flex min-h-0 flex-1 overflow-hidden rounded-xl border">
          {/* Left Pane: Precedence Hierarchy Stack */}
          <div className="border-border-subtle bg-surface-recessed flex w-[clamp(155px,26%,260px)] shrink-0 flex-col border-r">
            <div className="border-border-subtle flex items-center justify-between border-b px-3 py-2">
              <span className="text-ink-muted text-[11px] font-medium tracking-wide uppercase">
                {t("context.precedence")}
              </span>
              <span className="text-ink-faint text-[11px]">{t("context.precedenceOrder")}</span>
            </div>

            <ScrollArea className="min-h-0 flex-1" viewportClassName="p-2">
              {listLoading && !snapshot ? (
                <div className="text-ink-muted flex items-center justify-center gap-2 py-8 text-[13px]">
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  <span>{t("common.loading")}</span>
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
                          runWithDiscardGuard(() => {
                            void loadFile(activeHostId, activeCwd, entry, {
                              prefillTemplate: !entry.exists,
                            });
                          });
                        }}
                        className={cn(
                          "group flex w-full cursor-pointer flex-col gap-1 rounded-lg px-2.5 py-2 text-left transition-colors",
                          isSelected
                            ? "bg-surface-raised text-ink-primary"
                            : "text-ink-secondary hover:bg-surface-raised",
                          !entry.exists && !isSelected && "opacity-65 hover:opacity-95",
                        )}
                      >
                        <div className="flex items-center justify-between gap-1.5">
                          <div className="flex min-w-0 items-center gap-1.5">
                            <span className="text-ink-muted w-3.5 shrink-0 font-mono text-[11px] tabular-nums">
                              {entry.precedence_order}
                            </span>
                            <span className="truncate font-mono text-[13px] font-medium">
                              {entry.filename}
                            </span>
                            {isDirtySelected && (
                              <span
                                className="h-1.5 w-1.5 shrink-0 rounded-full bg-signal-warning"
                                title={t("sidePanel.unsavedChanges")}
                              />
                            )}
                          </div>
                          <span
                            className="border-border-subtle bg-surface-recessed text-ink-muted shrink-0 rounded border px-1.5 py-px text-[11px] font-medium"
                          >
                            {t(SCOPE_LABELS[entry.scope])}
                          </span>
                        </div>

                        <div className="flex items-center justify-between gap-2 pl-5">
                          <span className="text-ink-muted truncate font-mono text-[11px]">
                            {entry.relative_path}
                          </span>
                          <span className="text-ink-muted shrink-0 font-mono text-[11px] tabular-nums">
                            {entry.exists ? formatTokensCompact(entry.approx_tokens) : t("context.newFile")}
                          </span>
                        </div>

                        {(entry.imports?.length ?? 0) > 0 && (
                          <div className="text-ink-muted flex items-center gap-1 pl-5 text-[11px]">
                            <Link2 className="h-2.5 w-2.5 shrink-0" />
                            <span>
                              {t("context.importCount", { count: entry.imports.length })}
                            </span>
                          </div>
                        )}
                      </button>
                    );
                  })}
                </div>
              )}
            </ScrollArea>

            {snapshot && (
              <div className="border-border-subtle text-ink-muted border-t px-3 py-1.5 text-[11px] tabular-nums">
                {t("context.activeFiles", { count: snapshot.files.filter((file) => file.exists).length })} · ~{formatTokensCompact(snapshot.total_tokens)}
              </div>
            )}
            {/* Quick Create Buttons for Missing Canonical Layers */}
            {missingCreateTargets.length > 0 && (
              <div className="orbit-native-scroll border-border-subtle max-h-[30%] shrink-0 space-y-1 overflow-y-auto border-t p-2">
                {missingCreateTargets.map((target) => {
                  const createLabel =
                    target.scope === "global_user" ? target.relative_path : target.filename;
                  return (
                    <button
                      key={`create-${target.id}`}
                      type="button"
                      onClick={() => handleQuickCreate(target)}
                      className="text-ink-muted hover:bg-surface-recessed hover:text-ink-primary flex w-full cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-left text-[11px] transition-colors"
                    >
                      <Plus className="h-3 w-3 shrink-0 opacity-75" />
                      <span className="truncate font-mono">{t("context.createFile", { file: createLabel })}</span>
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
                <div className="border-border-subtle flex items-center justify-between gap-2 border-b px-3.5 py-2">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="text-ink-primary truncate font-mono text-[13px] font-medium">
                      {selectedEntry.relative_path}
                    </span>
                    <span
                      className="border-border-subtle bg-surface-recessed text-ink-muted shrink-0 rounded border px-1.5 py-px text-[11px] font-medium"
                    >
                      {t(SCOPE_LABELS[selectedEntry.scope])}
                    </span>
                    {selectedEntry.provider_hint &&
                      PROVIDER_LABELS[selectedEntry.provider_hint] && (
                        <span className="text-ink-muted hidden text-[11px] sm:inline">
                          {PROVIDER_LABELS[selectedEntry.provider_hint]}
                        </span>
                      )}
                    {dirty && (
                      <span className="inline-flex items-center gap-1 text-[11px] font-medium text-signal-warning">
                        <span className="h-1.5 w-1.5 rounded-full bg-signal-warning" />
                        {t("sidePanel.editorStatusUnsaved")}
                      </span>
                    )}
                    {!dirty && justSaved && (
                      <span className="inline-flex items-center gap-1 text-[11px] font-medium text-signal-success">
                        <Check className="h-3 w-3" />
                        {t("sidePanel.editorStatusSaved")}
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
                        className="text-ink-muted hover:bg-surface-recessed hover:text-ink-primary inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11px] transition-colors"
                      >
                        <RotateCcw className="h-3 w-3" />
                        <span>{t("context.revert")}</span>
                      </button>
                    )}
                    <Button
                      type="button"
                      size="xs"
                      disabled={saving || !fileReady || etagConflict || (!dirty && loadedExists)}
                      onClick={() => void handleSave(false)}
                      className="gap-1.5 text-[13px]"
                    >
                      {saving ? (
                        <Loader2 className="h-3 w-3 animate-spin" />
                      ) : (
                        <Save className="h-3 w-3" />
                      )}
                      <span>{loadedExists ? t("common.save") : t("approval.create")}</span>
                      <ShortcutKbd keys={`${mod}S`} size="xs" variant="inverted" />
                    </Button>
                  </div>
                </div>

                {/* Resolved @import Strip */}
                {(selectedEntry.imports?.length ?? 0) > 0 && (
                  <div className="border-border-subtle bg-surface-recessed flex flex-wrap items-center gap-1.5 border-b px-3.5 py-1.5">
                    <span className="text-ink-muted inline-flex items-center gap-1 text-[11px]">
                      <Link2 className="h-3 w-3" />
                      {t("context.imports")}
                    </span>
                    {(selectedEntry.imports ?? []).map((imp) => (
                      <span
                        key={imp}
                        className="border-border-subtle bg-surface-base text-ink-secondary rounded border px-1.5 py-0.5 font-mono text-[11px]"
                      >
                        @{imp}
                      </span>
                    ))}
                  </div>
                )}

                {/* Optimistic Concurrency Etag Conflict Banner */}
                {etagConflict && (
                  <div className="flex flex-wrap items-center justify-between gap-2 border-b border-signal-warning-border bg-signal-warning-bg px-3.5 py-2 text-[13px] text-signal-warning">
                    <div className="flex items-center gap-2">
                      <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                      <span>{t("sidePanel.editorConflictTitle")}</span>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <button
                        type="button"
                        onClick={handleReloadFromDisk}
                        className="bg-surface-base hover:bg-surface-base cursor-pointer rounded-md border border-signal-warning-border px-2 py-0.5 text-[11px] font-medium transition-colors"
                      >
                        {t("sidePanel.editorConflictReload")}
                      </button>
                      <button
                        type="button"
                        disabled={saving}
                        onClick={() => void handleSave(true)}
                        className="cursor-pointer rounded-md border border-signal-warning-border bg-signal-warning-bg px-2 py-0.5 text-[13px] font-medium text-signal-warning transition-colors hover:bg-surface-raised disabled:opacity-50"
                      >
                        {t("sidePanel.editorConflictOverwrite")}
                      </button>
                    </div>
                  </div>
                )}

                {/* Generic Save / Read Error Banner */}
                {saveError && (
                  <div className="flex items-center justify-between gap-2 border-b border-signal-danger-border bg-signal-danger-bg px-3.5 py-2 text-[13px] text-signal-danger">
                    <span className="truncate">{saveError}</span>
                    <button
                      type="button"
                      onClick={() => setSaveError(null)}
                      className="shrink-0 cursor-pointer text-[11px] font-medium text-signal-danger hover:text-signal-danger"
                    >
                      {t("sidePanel.editorDismissError")}
                    </button>
                  </div>
                )}

                {/* Monospace Editor Surface */}
                <div className="relative min-h-0 flex-1">
                  {fileLoading ? (
                    <div className="text-ink-muted flex h-full items-center justify-center gap-2 text-[13px]">
                      <Loader2 className="h-4 w-4 animate-spin" />
                      <span>{t("common.loading")}</span>
                    </div>
                  ) : (
                    <textarea
                      ref={textareaRef}
                      value={draftContent}
                      disabled={!fileReady}
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
                      aria-label={selectedEntry.filename}
                      placeholder={t("context.editorPlaceholder", { file: selectedEntry.filename })}
                      className="selectable-text text-ink-primary placeholder:text-ink-muted h-full w-full resize-none bg-transparent p-3.5 font-mono text-[13px] leading-[1.6] outline-none"
                    />
                  )}
                </div>

                {/* Editor Footer Status */}
                <div className="border-border-subtle bg-surface-recessed text-ink-muted flex items-center justify-between gap-2 border-t px-3.5 py-1.5 font-mono text-[11px] tabular-nums">
                  <span className="truncate opacity-75">{selectedEntry.path}</span>
                  <div className="flex shrink-0 items-center gap-2.5">
                    <span>{loadedLineEnding.toUpperCase()}</span>
                    <span className="opacity-40">·</span>
                    <span>{t("common.linesCount", { count: liveLineCount })}</span>
                    <span className="opacity-40">·</span>
                    <span>{(liveByteSize / 1024).toFixed(1)} KB</span>
                    <span className="opacity-40">·</span>
                    <span>~{formatTokensCompact(liveDraftTokens)}</span>
                  </div>
                </div>
              </>
            ) : (
              <div className="text-ink-muted flex flex-1 items-center justify-center text-[13px]">
                {t("context.pickLayer")}
              </div>
            )}
          </div>
        </div>
      )}

      <ConfirmDialog
        open={discardDialogOpen}
        onOpenChange={(open) => {
          setDiscardDialogOpen(open);
          if (!open) pendingDiscardActionRef.current = null;
        }}
        title={t("context.discardTitle")}
        description={selectedEntry ? t("context.discardDescription", { file: selectedEntry.filename }) : undefined}
        confirmLabel={t("review.discard")}
        tone="danger"
        onConfirm={() => {
          draftsByWorkspace.delete(workspaceKey);
          const action = pendingDiscardActionRef.current;
          pendingDiscardActionRef.current = null;
          action?.();
        }}
      />
    </div>
  );
}
