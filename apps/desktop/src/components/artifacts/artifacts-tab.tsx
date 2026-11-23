import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AppWindow,
  Blocks,
  Check,
  ChevronDown,
  Code2,
  Copy,
  Download,
  Eye,
  FileCode2,
  FileText,
  Globe,
  History,
  Image as ImageIcon,
  Loader2,
  MessageSquarePlus,
  RefreshCw,
  RotateCcw,
  Trash2,
  Workflow,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { ArtifactSandboxFrame } from "@/components/artifacts/artifact-sandbox-frame";
import { formatArtifactInlineComment } from "@/components/artifacts/mcp-app-bridge";
import { DiagramBlock } from "@/components/diagram-block";
import { MarkdownBlocksView, MarkdownLinkProvider } from "@/components/markdown-blocks";
import { ScrollArea } from "@/components/ui/scroll-area";
import { resolveTokenColor } from "@/lib/code-highlight";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import {
  type ArtifactDetail,
  type ArtifactKind,
  type ArtifactSummary,
  type HighlightedLineView,
  type RichBlockView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { formatFileSizeCompact } from "@/lib/session-insights";
import { type SidePanelTab, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type ArtifactViewMode = "preview" | "source" | "history";

function ArtifactKindIcon({
  kind,
  className = "h-[13.5px] w-[13.5px] shrink-0",
}: {
  kind: ArtifactKind;
  className?: string;
}) {
  switch (kind) {
    case "markdown":
      return <FileText className={className} />;
    case "html":
      return <Globe className={className} />;
    case "mcp_app":
      return <AppWindow className={className} />;
    case "code":
      return <FileCode2 className={className} />;
    case "svg":
      return <ImageIcon className={className} />;
    case "mermaid":
      return <Workflow className={className} />;
  }
}

function artifactKindLabel(kind: ArtifactKind): string {
  switch (kind) {
    case "markdown":
      return "Markdown";
    case "html":
      return "HTML";
    case "mcp_app":
      return "MCP App";
    case "code":
      return "Code";
    case "svg":
      return "SVG";
    case "mermaid":
      return "Mermaid";
  }
}

function sanitizeDownloadExtension(summary: ArtifactSummary): string {
  let raw: string;
  switch (summary.kind) {
    case "markdown":
      raw = "md";
      break;
    case "html":
    case "mcp_app":
      raw = "html";
      break;
    case "svg":
      raw = "svg";
      break;
    case "mermaid":
      raw = "mmd";
      break;
    case "code":
      raw = summary.language?.trim().toLowerCase() || "txt";
      break;
  }
  const clean = raw.replace(/[^a-z0-9]/g, "").slice(0, 12);
  return /^[a-z0-9]{1,12}$/.test(clean) ? clean : "txt";
}

function computeSafeFence(content: string): string {
  let maxRun = 0;
  let current = 0;
  for (let i = 0; i < content.length; i += 1) {
    if (content[i] === "`") {
      current += 1;
      if (current > maxRun) maxRun = current;
    } else {
      current = 0;
    }
  }
  return "`".repeat(Math.max(3, maxRun + 1));
}

function formatRelativeTime(iso: string): string {
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

/**
 * Isolated SVG preview rendered strictly via an `<img>` Blob URL.
 *
 * Browsers guarantee ZERO script execution, ZERO inline event handlers, and ZERO external
 * subresource fetches for SVG loaded through `<img>`, eliminating host-DOM XSS completely.
 */
const SafeSvgPreview = memo(function SafeSvgPreview({
  title,
  svgContent,
}: {
  title: string;
  svgContent: string;
}) {
  const [zoom, setZoom] = useState(1);
  const [blobUrl, setBlobUrl] = useState<string | null>(null);
  const [imgError, setImgError] = useState(false);

  useEffect(() => {
    setImgError(false);
    const blob = new Blob([svgContent], { type: "image/svg+xml;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    setBlobUrl(url);
    return () => {
      URL.revokeObjectURL(url);
    };
  }, [svgContent]);

  return (
    <div className="group/svg relative flex min-h-0 flex-1 flex-col overflow-hidden">
      <ScrollArea
        orientation="both"
        className="min-h-0 flex-1"
        viewportClassName="flex min-h-full items-center justify-center p-6"
      >
        {imgError ? (
          <div className="text-muted-foreground text-center text-[12px]">
            Unable to render SVG image (switch to Source to inspect raw XML).
          </div>
        ) : blobUrl ? (
          <img
            src={blobUrl}
            alt={title}
            onError={() => setImgError(true)}
            style={{
              transform: `scale(${zoom})`,
              transformOrigin: "center center",
            }}
            className="max-w-full rounded-lg transition-transform duration-150 select-none"
          />
        ) : null}
      </ScrollArea>

      {/* Subtle floating zoom pill (hover-revealed or pinned when zoomed) */}
      <div
        className={cn(
          "border-border/75 bg-background/90 text-muted-foreground absolute right-3 bottom-3 flex items-center gap-0.5 rounded-lg border p-0.5 shadow-xs backdrop-blur-md transition-opacity duration-150 select-none",
          zoom !== 1
            ? "opacity-100"
            : "opacity-0 group-focus-within/svg:opacity-100 group-hover/svg:opacity-100",
        )}
      >
        <button
          type="button"
          title="Zoom out"
          onClick={() => setZoom((z) => Math.max(0.25, Math.round((z - 0.25) * 100) / 100))}
          className="hover:text-foreground inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded"
        >
          <ZoomOut className="h-3 w-3" />
        </button>
        <span className="tabular-nums min-w-[36px] text-center font-mono text-[10.5px]">
          {Math.round(zoom * 100)}%
        </span>
        <button
          type="button"
          title="Zoom in"
          onClick={() => setZoom((z) => Math.min(4, Math.round((z + 0.25) * 100) / 100))}
          className="hover:text-foreground inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded"
        >
          <ZoomIn className="h-3 w-3" />
        </button>
        {zoom !== 1 && (
          <button
            type="button"
            title="Reset zoom"
            onClick={() => setZoom(1)}
            className="hover:text-foreground inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded"
          >
            <RotateCcw className="h-3 w-3" />
          </button>
        )}
      </div>
    </div>
  );
});

/**
 * Restrained inline comment composer matching Orbit's Diff `InlineCommentComposer` pattern.
 */
const InlineArtifactCommentComposer = memo(function InlineArtifactCommentComposer({
  label,
  excerpt,
  onCancel,
  onSave,
}: {
  label: string;
  excerpt?: string | null;
  onCancel: () => void;
  onSave: (comment: string) => void;
}) {
  const [text, setText] = useState("");
  const commentIme = useImeCompositionGuard();
  const submit = () => {
    const trimmed = text.trim();
    if (!trimmed) return;
    onSave(trimmed);
    setText("");
  };

  return (
    <div className="border-border bg-elevated/95 shrink-0 border-b px-3 py-2 font-sans">
      <div className="text-muted-foreground mb-1 flex items-center justify-between gap-2 text-[11px]">
        <span className="truncate font-mono">{label}</span>
        <button
          type="button"
          aria-label="Cancel comment"
          onClick={onCancel}
          className="hover:text-foreground inline-flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center rounded"
        >
          <X className="h-3 w-3" />
        </button>
      </div>
      {excerpt && (
        <div className="border-border/70 bg-background/70 text-muted-foreground mb-1.5 line-clamp-2 rounded border px-2 py-1 font-mono text-[11px]">
          {excerpt}
        </div>
      )}
      <textarea
        autoFocus
        rows={2}
        value={text}
        onChange={(e) => setText(e.target.value)}
        {...commentIme.compositionProps}
        onKeyDown={(e) => {
          if (commentIme.shouldIgnoreKeyDown(e)) return;
          if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey || !e.shiftKey)) {
            e.preventDefault();
            submit();
          }
        }}
        placeholder="Comment on this artifact for the agent… (Enter to attach)"
        className="border-border bg-background text-foreground placeholder:text-muted-foreground w-full resize-none rounded-md border px-2 py-1.5 text-[12px] leading-relaxed outline-none focus:border-foreground/30"
      />
      <div className="mt-1.5 flex items-center justify-end gap-1.5">
        <button
          type="button"
          onClick={onCancel}
          className="text-muted-foreground hover:text-foreground cursor-pointer rounded-md px-2 py-0.5 text-[11.5px]"
        >
          Cancel
        </button>
        <button
          type="button"
          disabled={!text.trim()}
          onClick={submit}
          className="bg-foreground text-background cursor-pointer rounded-md px-2.5 py-0.5 text-[11.5px] font-medium transition-opacity hover:opacity-90 disabled:cursor-default disabled:opacity-40"
        >
          Attach comment
        </button>
      </div>
    </div>
  );
});

export const ArtifactsTab = memo(function ArtifactsTab({ tab }: { tab: SidePanelTab }) {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const resolvedTheme = useOrbitSelector((s) => s.resolvedTheme);
  const isDark = resolvedTheme === "dark";
  const { t } = useI18n();

  // Prefer active composer's host so multi-host comments/actions always match the active session
  const hostId = selectedHostId || tab.hostId || "";
  const hostDetail = useOrbitSelector((s) => (hostId ? (s.hostDetails[hostId] ?? null) : null));

  const activeAgent = useMemo(
    () =>
      selectedAgentId
        ? (hostDetail?.agents.find((a) => a.agent_id === selectedAgentId) ?? null)
        : null,
    [hostDetail, selectedAgentId],
  );
  const scopedWorkspaceId = activeAgent?.workspace_id || tab.workspaceId || "";
  const cwd = activeAgent?.cwd || "~";

  // Show workspace-scoped artifacts first; if none in active workspace, fall back to host artifacts
  const artifacts = useMemo<ArtifactSummary[]>(() => {
    const all = hostDetail?.artifacts ?? [];
    if (!scopedWorkspaceId) return all;
    const inWorkspace = all.filter((a) => a.workspace_id === scopedWorkspaceId);
    return inWorkspace.length > 0 ? inWorkspace : all;
  }, [hostDetail?.artifacts, scopedWorkspaceId]);

  const [selectedArtifactId, setSelectedArtifactId] = useState<string | null>(
    tab.artifactId ?? null,
  );
  const [requestedVersion, setRequestedVersion] = useState<number | null>(null);
  const [viewMode, setViewMode] = useState<ArtifactViewMode>("preview");
  const [selectorOpen, setSelectorOpen] = useState(false);

  const [detail, setDetail] = useState<ArtifactDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [confirmDeleteOpen, setConfirmDeleteOpen] = useState(false);

  // Parsed rich markdown / syntax-highlighted source blocks from Rust backend
  const [markdownBlocks, setMarkdownBlocks] = useState<RichBlockView[]>([]);
  const [highlightedLines, setHighlightedLines] = useState<HighlightedLineView[]>([]);

  // Inline comment selection state
  const [selectionStart, setSelectionStart] = useState<number | null>(null);
  const [selectionEnd, setSelectionEnd] = useState<number | null>(null);
  const [commentTarget, setCommentTarget] = useState<{
    startLine?: number | null;
    endLine?: number | null;
    section?: string | null;
    excerpt?: string | null;
  } | null>(null);

  const selectorContainerRef = useRef<HTMLDivElement | null>(null);

  // Close selector dropdown on Escape or click-outside
  useEffect(() => {
    if (!selectorOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setSelectorOpen(false);
      }
    };
    const onPointerDown = (e: MouseEvent) => {
      if (
        selectorContainerRef.current &&
        !selectorContainerRef.current.contains(e.target as Node)
      ) {
        setSelectorOpen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("mousedown", onPointerDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("mousedown", onPointerDown);
    };
  }, [selectorOpen]);

  // Sync external focus requests (`openArtifactsTab({ artifactId })`)
  useEffect(() => {
    if (tab.artifactId) {
      setSelectedArtifactId(tab.artifactId);
      setRequestedVersion(null);
      setSelectorOpen(false);
      setConfirmDeleteOpen(false);
    }
  }, [tab.artifactId, tab.focusNonce]);

  // Keep a valid artifact selected without clobbering an explicit `tab.artifactId` while refreshing
  useEffect(() => {
    if (artifacts.length === 0) {
      if (!tab.artifactId || selectedArtifactId !== tab.artifactId) {
        setSelectedArtifactId(null);
        setDetail(null);
      }
      return;
    }
    if (!selectedArtifactId) {
      setSelectedArtifactId(artifacts[0].artifact_id);
      setRequestedVersion(null);
      return;
    }
    if (!artifacts.some((a) => a.artifact_id === selectedArtifactId)) {
      if (selectedArtifactId === tab.artifactId) {
        // Explicit deep-link target may still be hydrating via `getArtifact` / `listArtifacts`
        return;
      }
      setSelectedArtifactId(artifacts[0].artifact_id);
      setRequestedVersion(null);
    }
  }, [artifacts, selectedArtifactId, tab.artifactId]);

  const activeSummary = useMemo(
    () => artifacts.find((a) => a.artifact_id === selectedArtifactId) ?? detail?.summary ?? null,
    [artifacts, selectedArtifactId, detail?.summary],
  );

  // Refresh host artifacts list when opening or switching host
  const handleRefreshList = useCallback(async () => {
    if (!hostId) return;
    try {
      await orbitIpc.listArtifacts(hostId, null, null);
    } catch {
      // Quiet fail; cached list remains visible
    }
  }, [hostId]);

  useEffect(() => {
    void handleRefreshList();
  }, [handleRefreshList]);

  // Immediately clear stale content & UI state whenever host or selected artifact changes
  useEffect(() => {
    setDetail(null);
    setMarkdownBlocks([]);
    setHighlightedLines([]);
    setLoadError(null);
    setSelectionStart(null);
    setSelectionEnd(null);
    setCommentTarget(null);
    setConfirmDeleteOpen(false);
  }, [hostId, selectedArtifactId]);

  // Fetch active artifact content (latest or selected historical revision)
  useEffect(() => {
    if (!hostId || !selectedArtifactId) {
      setDetail(null);
      setLoadError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setLoadError(null);
    void orbitIpc
      .getArtifact(hostId, selectedArtifactId, requestedVersion)
      .then((loaded) => {
        if (cancelled) return;
        setDetail(loaded);
        setLoading(false);
      })
      .catch((err) => {
        if (cancelled) return;
        setLoadError(String(err));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [hostId, selectedArtifactId, requestedVersion, activeSummary?.version]);

  // Reset line selection & delete confirmation when switching revision
  useEffect(() => {
    setSelectionStart(null);
    setSelectionEnd(null);
    setCommentTarget(null);
    setConfirmDeleteOpen(false);
  }, [requestedVersion]);

  // Auto-dismiss inline delete confirmation after 4 seconds
  useEffect(() => {
    if (!confirmDeleteOpen) return;
    const timer = window.setTimeout(() => setConfirmDeleteOpen(false), 4000);
    return () => window.clearTimeout(timer);
  }, [confirmDeleteOpen]);

  // Active detail is valid ONLY if it matches the currently selected artifact ID
  const currentDetail =
    detail && selectedArtifactId && detail.summary.artifact_id === selectedArtifactId
      ? detail
      : null;

  // Parse Markdown preview or syntax-highlighted source via Rust `parseMarkdownPreview`
  useEffect(() => {
    if (!currentDetail) {
      setMarkdownBlocks([]);
      setHighlightedLines([]);
      return;
    }
    let cancelled = false;
    const kind = currentDetail.summary.kind;
    if (kind === "markdown" && viewMode === "preview") {
      void orbitIpc
        .parseMarkdownPreview(currentDetail.content, false)
        .then((blocks) => {
          if (!cancelled) setMarkdownBlocks(blocks);
        })
        .catch(() => {
          if (!cancelled) setMarkdownBlocks([]);
        });
    }
    if (viewMode === "source" || kind === "code") {
      const rawLang =
        currentDetail.summary.language?.trim() ||
        (kind === "html" || kind === "mcp_app"
          ? "html"
          : kind === "svg"
            ? "xml"
            : kind === "markdown"
              ? "markdown"
              : "text");
      const safeLang = /^[a-z0-9_+-]+$/i.test(rawLang) ? rawLang : "text";
      const fence = computeSafeFence(currentDetail.content);
      const fenced = `${fence}${safeLang}\n${currentDetail.content}\n${fence}`;
      void orbitIpc
        .parseMarkdownPreview(fenced, false)
        .then((blocks) => {
          if (cancelled) return;
          const codeBlock = blocks.find((b) => b.kind === "code_block");
          if (codeBlock && codeBlock.kind === "code_block") {
            setHighlightedLines(codeBlock.lines);
          } else {
            setHighlightedLines([]);
          }
        })
        .catch(() => {
          if (!cancelled) setHighlightedLines([]);
        });
    }
    return () => {
      cancelled = true;
    };
  }, [currentDetail, viewMode]);

  const rawLines = useMemo(
    () => (currentDetail ? currentDetail.content.split(/\r?\n/) : []),
    [currentDetail],
  );

  const normalizedRange = useMemo(() => {
    if (selectionStart == null) return null;
    const a = selectionStart;
    const b = selectionEnd ?? selectionStart;
    return { start: Math.min(a, b), end: Math.max(a, b) };
  }, [selectionStart, selectionEnd]);

  const handleLineClick = (lineNum: number, shiftKey: boolean) => {
    if (shiftKey && selectionStart != null) {
      setSelectionEnd(lineNum);
    } else if (selectionStart === lineNum && (selectionEnd == null || selectionEnd === lineNum)) {
      setSelectionStart(null);
      setSelectionEnd(null);
    } else {
      setSelectionStart(lineNum);
      setSelectionEnd(lineNum);
    }
  };

  const openInlineCommentFromSelection = () => {
    if (!currentDetail) return;
    if ((viewMode === "source" || currentDetail.summary.kind === "code") && normalizedRange) {
      const snippet = rawLines
        .slice(normalizedRange.start - 1, Math.min(normalizedRange.end, normalizedRange.start + 5))
        .join("\n")
        .trim();
      setCommentTarget({
        startLine: normalizedRange.start,
        endLine: normalizedRange.end,
        excerpt: snippet || null,
      });
      return;
    }
    const browserSel = typeof window !== "undefined" ? window.getSelection()?.toString().trim() : "";
    setCommentTarget({
      section: viewMode === "preview" ? "Preview" : "Source",
      excerpt: browserSel ? browserSel.slice(0, 240) : null,
    });
  };

  const handleSaveComment = (commentText: string) => {
    if (!currentDetail || !commentTarget) return;
    const formatted = formatArtifactInlineComment({
      title: currentDetail.summary.title,
      version: currentDetail.summary.version,
      startLine: commentTarget.startLine,
      endLine: commentTarget.endLine,
      section: commentTarget.section,
      comment: commentText,
    });
    const quoteBody = commentTarget.excerpt
      ? `${formatted}\n> ${commentTarget.excerpt.replace(/\n/g, "\n> ")}`
      : formatted;
    orbitActions.quoteDiffLineToComposer(
      `Artifact "${currentDetail.summary.title}" (v${currentDetail.summary.version})`,
      commentTarget.startLine ?? null,
      quoteBody,
    );
    setCommentTarget(null);
    setSelectionStart(null);
    setSelectionEnd(null);
    orbitActions.showToast("Attached artifact comment to composer");
  };

  const handleCopy = async () => {
    if (!currentDetail) return;
    try {
      await navigator.clipboard.writeText(currentDetail.content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch {
      orbitActions.showToast("Failed to copy artifact");
    }
  };

  const handleDownload = () => {
    if (!currentDetail) return;
    const safeBase =
      currentDetail.summary.title
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-|-$/g, "") || currentDetail.summary.artifact_id;
    const ext = sanitizeDownloadExtension(currentDetail.summary);
    const blob = new Blob([currentDetail.content], {
      type: "application/octet-stream",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${safeBase}-v${currentDetail.summary.version}.${ext}`;
    a.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  };

  const handleDeleteConfirmed = async () => {
    if (!hostId || !currentDetail || deleting) return;
    setDeleting(true);
    setConfirmDeleteOpen(false);
    try {
      await orbitIpc.deleteArtifact(hostId, currentDetail.summary.artifact_id);
      setDetail(null);
      setRequestedVersion(null);
      orbitActions.showToast(`Deleted "${currentDetail.summary.title}"`);
    } catch (err) {
      orbitActions.showToast(`Failed to delete artifact: ${String(err)}`);
    } finally {
      setDeleting(false);
    }
  };

  const mermaidSyntheticBlock = useMemo(() => {
    if (!currentDetail || currentDetail.summary.kind !== "mermaid") return null;
    return {
      kind: "code_block" as const,
      language: "mermaid",
      raw_code: currentDetail.content,
      lines: [],
    };
  }, [currentDetail]);

  const latestVersion = Math.max(
    activeSummary?.version ?? 0,
    currentDetail?.revisions[0]?.version ?? 0,
  );
  const isHistoricalVersion =
    Boolean(currentDetail) &&
    latestVersion > 0 &&
    currentDetail!.summary.version < latestVersion;

  if (artifacts.length === 0 && !loading && !currentDetail) {
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2.5 px-6 text-center">
        <Blocks className="text-muted-foreground/45 h-7 w-7 stroke-[1.5]" />
        <div className="text-foreground/85 text-[13px] font-medium">
          {t("sidePanel.noArtifacts")}
        </div>
        <p className="text-muted-foreground max-w-[260px] text-[12px] leading-relaxed">
          {t("sidePanel.noArtifactsDesc")}
        </p>
        <button
          type="button"
          onClick={() => void handleRefreshList()}
          className="border-border hover:bg-accent text-muted-foreground hover:text-foreground mt-1 inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] transition-colors"
        >
          <RefreshCw className="h-3 w-3" />
          <span>{t("common.refresh")}</span>
        </button>
      </div>
    );
  }

  return (
    <div ref={selectorContainerRef} className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Slim Header: Selector + Segmented Switch + Actions */}
      <div className="border-border/80 flex h-[38px] shrink-0 items-center justify-between gap-2 border-b px-2.5">
        {/* Artifact Switcher Trigger */}
        <div className="relative flex min-w-0 flex-1 items-center gap-1.5">
          <button
            type="button"
            aria-expanded={selectorOpen}
            aria-haspopup="listbox"
            onClick={() => setSelectorOpen((v) => !v)}
            className="hover:bg-accent/70 flex max-w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-1 text-left transition-colors"
          >
            {activeSummary && (
              <ArtifactKindIcon
                kind={activeSummary.kind}
                className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0"
              />
            )}
            <span className="text-foreground truncate text-[12.5px] font-medium">
              {activeSummary?.title ?? t("sidePanel.selectArtifact")}
            </span>
            {activeSummary && (
              <span className="text-muted-foreground/75 shrink-0 font-mono text-[11px]">
                v{currentDetail?.summary.version ?? latestVersion}
              </span>
            )}
            {artifacts.length > 1 && (
              <ChevronDown
                className={cn(
                  "text-muted-foreground h-3 w-3 shrink-0 transition-transform",
                  selectorOpen && "rotate-180",
                )}
              />
            )}
          </button>
        </div>

        {/* Segmented Preview | Source | History */}
        <div className="bg-muted/65 flex shrink-0 items-center rounded-md p-0.5 text-[11px]">
          {(
            [
              { mode: "preview", label: t("sidePanel.viewPreview") },
              { mode: "source", label: t("sidePanel.viewSource") },
              { mode: "history", label: t("sidePanel.viewHistory") },
            ] as const
          ).map(({ mode, label }) => (
            <button
              key={mode}
              type="button"
              onClick={() => setViewMode(mode)}
              className={cn(
                "inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-[5px] px-2 transition-colors",
                viewMode === mode
                  ? "bg-background text-foreground font-medium shadow-2xs"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {mode === "preview" && <Eye className="h-3 w-3" />}
              {mode === "source" && <Code2 className="h-3 w-3" />}
              {mode === "history" && <History className="h-3 w-3" />}
              <span>{label}</span>
            </button>
          ))}
        </div>

        {/* Compact Action Icons */}
        <div className="flex shrink-0 items-center gap-0.5">
          <button
            type="button"
            title="Comment on artifact"
            disabled={!currentDetail}
            onClick={openInlineCommentFromSelection}
            className="text-muted-foreground hover:text-foreground inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] disabled:opacity-40 dark:hover:bg-white/[0.07]"
          >
            <MessageSquarePlus className="h-[13.5px] w-[13.5px]" />
          </button>
          <button
            type="button"
            title="Copy source"
            disabled={!currentDetail}
            onClick={() => void handleCopy()}
            className="text-muted-foreground hover:text-foreground inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] disabled:opacity-40 dark:hover:bg-white/[0.07]"
          >
            {copied ? (
              <Check className="h-[13.5px] w-[13.5px] text-emerald-500" />
            ) : (
              <Copy className="h-[13.5px] w-[13.5px]" />
            )}
          </button>
          <button
            type="button"
            title="Download artifact"
            disabled={!currentDetail}
            onClick={handleDownload}
            className="text-muted-foreground hover:text-foreground inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] disabled:opacity-40 dark:hover:bg-white/[0.07]"
          >
            <Download className="h-[13.5px] w-[13.5px]" />
          </button>
          {confirmDeleteOpen ? (
            <div className="flex items-center gap-1 pl-0.5">
              <button
                type="button"
                disabled={!currentDetail || deleting}
                onClick={() => void handleDeleteConfirmed()}
                className="cursor-pointer rounded-md bg-rose-600 px-2 py-0.5 text-[11px] font-medium text-white hover:bg-rose-500 disabled:opacity-40"
              >
                Delete
              </button>
              <button
                type="button"
                aria-label="Cancel delete"
                onClick={() => setConfirmDeleteOpen(false)}
                className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded"
              >
                <X className="h-3 w-3" />
              </button>
            </div>
          ) : (
            <button
              type="button"
              title="Delete artifact"
              disabled={!currentDetail || deleting}
              onClick={() => setConfirmDeleteOpen(true)}
              className="text-muted-foreground hover:text-rose-500 inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] disabled:opacity-40 dark:hover:bg-white/[0.07]"
            >
              <Trash2 className="h-[13.5px] w-[13.5px]" />
            </button>
          )}
        </div>
      </div>

      {/* Collapsible Artifact Selector List */}
      {selectorOpen && (
        <div
          role="listbox"
          className="orbit-native-scroll border-border/80 bg-elevated/45 max-h-[196px] shrink-0 overflow-y-auto overscroll-contain border-b p-1.5"
        >
          <div className="flex flex-col gap-0.5">
            {artifacts.map((item) => {
              const isSelected = item.artifact_id === selectedArtifactId;
              return (
                <button
                  key={item.artifact_id}
                  role="option"
                  aria-selected={isSelected}
                  type="button"
                  onClick={() => {
                    setSelectedArtifactId(item.artifact_id);
                    setRequestedVersion(null);
                    setSelectorOpen(false);
                  }}
                  className={cn(
                    "flex w-full cursor-pointer items-center justify-between gap-2 rounded-lg px-2.5 py-1.5 text-left text-[12px] transition-colors",
                    isSelected
                      ? "bg-black/[0.06] text-foreground font-medium dark:bg-white/[0.09]"
                      : "text-foreground/80 hover:bg-black/[0.04] dark:hover:bg-white/[0.05]",
                  )}
                >
                  <span className="flex min-w-0 items-center gap-2">
                    <ArtifactKindIcon
                      kind={item.kind}
                      className="text-muted-foreground h-[13px] w-[13px] shrink-0"
                    />
                    <span className="truncate">{item.title}</span>
                  </span>
                  <span className="text-muted-foreground/75 flex shrink-0 items-center gap-1.5 font-mono text-[10.5px]">
                    <span>{artifactKindLabel(item.kind)}</span>
                    <span>·</span>
                    <span>v{item.version}</span>
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      )}

      {/* Historical Version Banner */}
      {isHistoricalVersion && currentDetail && (
        <div className="border-border/70 bg-amber-500/[0.07] flex shrink-0 items-center justify-between gap-2 border-b px-3 py-1 text-[11.5px] text-amber-800 dark:text-amber-200">
          <span>
            Viewing historical revision <strong>v{currentDetail.summary.version}</strong> (latest is
            v{latestVersion})
          </span>
          <button
            type="button"
            onClick={() => setRequestedVersion(null)}
            className="cursor-pointer font-medium underline underline-offset-2"
          >
            Latest
          </button>
        </div>
      )}

      {/* Selection Action Pill in Source/Code Mode */}
      {normalizedRange &&
        !commentTarget &&
        (viewMode === "source" || currentDetail?.summary.kind === "code") && (
          <div className="border-border/70 bg-elevated/60 flex shrink-0 items-center justify-between gap-2 border-b px-3 py-1 text-[11.5px]">
            <span className="text-muted-foreground font-mono">
              Selected{" "}
              {normalizedRange.start === normalizedRange.end
                ? `L${normalizedRange.start}`
                : `L${normalizedRange.start}-L${normalizedRange.end}`}{" "}
              (Shift-click to extend)
            </span>
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={() => {
                  setSelectionStart(null);
                  setSelectionEnd(null);
                }}
                className="text-muted-foreground hover:text-foreground cursor-pointer text-[11px]"
              >
                Clear
              </button>
              <button
                type="button"
                onClick={openInlineCommentFromSelection}
                className="bg-foreground text-background inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11px] font-medium"
              >
                <MessageSquarePlus className="h-3 w-3" />
                <span>Comment</span>
              </button>
            </div>
          </div>
        )}

      {/* Inline Comment Composer */}
      {commentTarget && currentDetail && (
        <InlineArtifactCommentComposer
          label={
            commentTarget.startLine
              ? `${currentDetail.summary.title} · v${currentDetail.summary.version} · ${
                  commentTarget.endLine && commentTarget.endLine > commentTarget.startLine
                    ? `L${commentTarget.startLine}-L${commentTarget.endLine}`
                    : `L${commentTarget.startLine}`
                }`
              : `${currentDetail.summary.title} · v${currentDetail.summary.version}${
                  commentTarget.section ? ` · ${commentTarget.section}` : ""
                }`
          }
          excerpt={commentTarget.excerpt}
          onCancel={() => setCommentTarget(null)}
          onSave={handleSaveComment}
        />
      )}

      {/* Main Viewer Body */}
      <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
        {loading && !currentDetail ? (
          <div className="text-muted-foreground flex flex-1 items-center justify-center gap-2 text-[12.5px]">
            <Loader2 className="h-4 w-4 animate-spin" />
            <span>Loading artifact…</span>
          </div>
        ) : loadError ? (
          <div className="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center text-[12.5px]">
            <span className="text-rose-500">Failed to load artifact</span>
            <span className="font-mono text-[11.5px] opacity-80">{loadError}</span>
          </div>
        ) : !currentDetail ? null : viewMode === "history" ? (
          <ScrollArea className="min-h-0 flex-1" viewportClassName="p-3 space-y-1.5">
            {currentDetail.revisions.map((rev) => {
              const isCurrent = rev.version === currentDetail.summary.version;
              return (
                <button
                  key={rev.version}
                  type="button"
                  onClick={() => {
                    setRequestedVersion(rev.version === latestVersion ? null : rev.version);
                    setViewMode("preview");
                  }}
                  className={cn(
                    "border-border/65 flex w-full cursor-pointer items-center justify-between gap-2 rounded-xl border px-3 py-2 text-left transition-colors",
                    isCurrent
                      ? "border-foreground/25 bg-foreground/[0.04]"
                      : "bg-elevated/35 hover:bg-elevated/70",
                  )}
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 text-[12px]">
                      <span className="font-mono font-semibold">v{rev.version}</span>
                      {rev.version === latestVersion && (
                        <span className="bg-muted text-muted-foreground rounded-full px-1.5 py-[1px] text-[10px]">
                          latest
                        </span>
                      )}
                      <span className="text-muted-foreground text-[11px]">
                        {formatRelativeTime(rev.created_at)}
                      </span>
                    </div>
                    {rev.summary_note && (
                      <div className="text-muted-foreground mt-0.5 truncate text-[11.5px]">
                        {rev.summary_note}
                      </div>
                    )}
                  </div>
                  <span className="text-muted-foreground/75 shrink-0 font-mono text-[11px]">
                    {formatFileSizeCompact(rev.byte_len)}
                  </span>
                </button>
              );
            })}
          </ScrollArea>
        ) : viewMode === "source" || currentDetail.summary.kind === "code" ? (
          <ScrollArea
            orientation="both"
            className="min-h-0 flex-1"
            viewportClassName="py-2 font-mono text-[12px] leading-[1.6]"
          >
            {rawLines.map((lineText, idx) => {
              const lineNum = idx + 1;
              const isSelected =
                normalizedRange !== null &&
                lineNum >= normalizedRange.start &&
                lineNum <= normalizedRange.end;
              const tokens = highlightedLines[idx]?.tokens ?? null;

              return (
                <div
                  key={lineNum}
                  onClick={(e) => handleLineClick(lineNum, e.shiftKey)}
                  className={cn(
                    "group/line flex min-w-fit cursor-pointer px-2 whitespace-pre transition-colors",
                    isSelected
                      ? "bg-primary/12 dark:bg-primary/20"
                      : "hover:bg-black/[0.03] dark:hover:bg-white/[0.04]",
                  )}
                >
                  <span className="text-muted-foreground/55 mr-3 inline-block w-9 shrink-0 text-right tabular-nums select-none">
                    {lineNum}
                  </span>
                  <span className="selectable-text flex-1 pr-4">
                    {tokens && tokens.length > 0
                      ? tokens.map((tok, ti) => (
                          <span
                            key={ti}
                            style={{
                              color: resolveTokenColor(tok, isDark),
                              fontWeight: tok.bold ? 600 : undefined,
                              fontStyle: tok.italic ? "italic" : undefined,
                            }}
                          >
                            {tok.text}
                          </span>
                        ))
                      : lineText || " "}
                  </span>
                </div>
              );
            })}
          </ScrollArea>
        ) : currentDetail.summary.kind === "html" || currentDetail.summary.kind === "mcp_app" ? (
          <ArtifactSandboxFrame
            artifactId={currentDetail.summary.artifact_id}
            title={currentDetail.summary.title}
            version={currentDetail.summary.version}
            html={currentDetail.content}
            isMcpApp={currentDetail.summary.kind === "mcp_app"}
            resourceUri={currentDetail.summary.resource_uri}
          />
        ) : currentDetail.summary.kind === "svg" ? (
          <SafeSvgPreview
            title={currentDetail.summary.title}
            svgContent={currentDetail.content}
          />
        ) : currentDetail.summary.kind === "mermaid" && mermaidSyntheticBlock ? (
          <ScrollArea className="min-h-0 flex-1" viewportClassName="p-4">
            <DiagramBlock
              kind="mermaid"
              block={mermaidSyntheticBlock}
              isDark={isDark}
              isStreamingBlock={false}
            />
          </ScrollArea>
        ) : (
          <ScrollArea className="min-h-0 flex-1" viewportClassName="p-4 select-text">
            {markdownBlocks.length > 0 ? (
              <MarkdownLinkProvider cwd={cwd}>
                <MarkdownBlocksView blocks={markdownBlocks} />
              </MarkdownLinkProvider>
            ) : (
              <div className="text-foreground/85 whitespace-pre-wrap text-[13px] leading-relaxed">
                {currentDetail.content}
              </div>
            )}
          </ScrollArea>
        )}
      </div>
    </div>
  );
});
