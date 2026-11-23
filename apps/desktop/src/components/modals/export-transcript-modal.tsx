import { useEffect, useRef, useState } from "react";
import { Check, Copy, Download, Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { Button } from "@/components/ui/button";
import { ToggleSwitch } from "@/components/settings/settings-primitives";
import {
  type ExportFormat,
  type ExportedTranscript,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const FORMAT_OPTIONS: ReadonlyArray<{ id: ExportFormat; label: string }> = [
  { id: "markdown", label: "Markdown" },
  { id: "json", label: "JSON" },
  { id: "html", label: "HTML" },
];

function formatCompactBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

export function ExportTranscriptModal() {
  const target = useOrbitSelector((s) => s.exportDialogTarget);
  const hostId = target?.hostId ?? null;
  const agentId = target?.agentId ?? null;
  const open = Boolean(target);
  const targetIdentity = hostId && agentId ? `${hostId}::${agentId}` : null;

  const threadTitle = useOrbitSelector((s) => {
    if (!target) return "";
    const tKey = threadScopeKey(target.hostId, target.agentId);
    const custom = s.threadCustomTitles[tKey];
    if (custom) return custom;
    const ag =
      s.hostDetails[target.hostId]?.agents.find((a) => a.agent_id === target.agentId) ??
      s.stages[tKey]?.agent;
    return ag?.title || "Conversation";
  });

  const initialDir = useOrbitSelector((s) => {
    if (!target) return null;
    const ag = s.hostDetails[target.hostId]?.agents.find((a) => a.agent_id === target.agentId);
    return ag?.cwd ?? null;
  });

  const [format, setFormat] = useState<ExportFormat>("markdown");
  const [includeThinking, setIncludeThinking] = useState(true);
  const [includeToolCalls, setIncludeToolCalls] = useState(true);
  const [preview, setPreview] = useState<ExportedTranscript | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [copied, setCopied] = useState(false);
  const reqSeqRef = useRef(0);
  const saveSeqRef = useRef(0);
  const activeIdentityRef = useRef<string | null>(targetIdentity);
  const copyTimerRef = useRef<number | null>(null);
  const radioGroupRef = useRef<HTMLDivElement | null>(null);

  activeIdentityRef.current = targetIdentity;

  const clearCopyTimer = () => {
    if (copyTimerRef.current !== null) {
      window.clearTimeout(copyTimerRef.current);
      copyTimerRef.current = null;
    }
  };

  useEffect(() => {
    return () => {
      reqSeqRef.current += 1;
      saveSeqRef.current += 1;
      clearCopyTimer();
    };
  }, []);

  useEffect(() => {
    if (!open || !hostId || !agentId) {
      reqSeqRef.current += 1;
      saveSeqRef.current += 1;
      clearCopyTimer();
      setPreview(null);
      setLoading(false);
      setSaving(false);
      setError(null);
      setCopied(false);
      return;
    }
    const seq = ++reqSeqRef.current;
    setLoading(true);
    setError(null);
    void orbitIpc
      .exportAgentTranscript(hostId, agentId, {
        format,
        include_thinking: includeThinking,
        include_tool_calls: includeToolCalls,
        include_timestamps: true,
      })
      .then((res) => {
        if (reqSeqRef.current !== seq) return;
        setPreview(res);
      })
      .catch((err) => {
        if (reqSeqRef.current !== seq) return;
        setPreview(null);
        setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (reqSeqRef.current === seq) {
          setLoading(false);
        }
      });
  }, [open, hostId, agentId, format, includeThinking, includeToolCalls]);

  const handleCopy = async () => {
    if (!preview) return;
    try {
      await navigator.clipboard.writeText(preview.content);
      setCopied(true);
      clearCopyTimer();
      copyTimerRef.current = window.setTimeout(() => {
        copyTimerRef.current = null;
        setCopied(false);
      }, 1500);
      orbitActions.showToast("Copied export to clipboard");
    } catch {
      orbitActions.showToast("Failed to copy export");
    }
  };

  const handleSave = async () => {
    if (!hostId || !agentId || !targetIdentity || saving) return;
    const mySaveSeq = ++saveSeqRef.current;
    const myIdentity = targetIdentity;
    setSaving(true);
    try {
      const savedPath = await orbitIpc.saveExportedTranscript(
        hostId,
        agentId,
        {
          format,
          include_thinking: includeThinking,
          include_tool_calls: includeToolCalls,
          include_timestamps: true,
        },
        initialDir,
      );
      if (savedPath) {
        const basename = savedPath.split(/[\\/]/).pop() || savedPath;
        orbitActions.showToast(`Saved ${basename}`);
        if (saveSeqRef.current === mySaveSeq && activeIdentityRef.current === myIdentity) {
          orbitActions.closeExportDialog();
        }
      }
    } catch (err) {
      if (saveSeqRef.current === mySaveSeq && activeIdentityRef.current === myIdentity) {
        const message = err instanceof Error ? err.message : String(err);
        setError(message);
      }
    } finally {
      if (saveSeqRef.current === mySaveSeq) {
        setSaving(false);
      }
    }
  };

  const handleFormatKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight" && e.key !== "Home" && e.key !== "End") {
      return;
    }
    e.preventDefault();
    const curIdx = FORMAT_OPTIONS.findIndex((o) => o.id === format);
    const nextIdx =
      e.key === "Home"
        ? 0
        : e.key === "End"
          ? FORMAT_OPTIONS.length - 1
          : e.key === "ArrowRight"
            ? (curIdx + 1) % FORMAT_OPTIONS.length
            : (curIdx - 1 + FORMAT_OPTIONS.length) % FORMAT_OPTIONS.length;
    const nextOpt = FORMAT_OPTIONS[nextIdx];
    if (nextOpt) {
      setFormat(nextOpt.id);
      const nextBtn = radioGroupRef.current?.querySelector<HTMLButtonElement>(
        `[data-export-format="${nextOpt.id}"]`,
      );
      nextBtn?.focus();
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) orbitActions.closeExportDialog();
      }}
    >
      <DialogContent className="max-w-[420px] gap-4 p-5">
        <DialogHeader className="gap-1 pr-6">
          <DialogTitle className="text-[15px]">Export Conversation</DialogTitle>
          <DialogDescription className="truncate text-[12.5px]">
            {threadTitle}
          </DialogDescription>
        </DialogHeader>

        {/* Format segmented control */}
        <div className="space-y-1.5">
          <span className="text-muted-foreground text-[12px] font-medium">Format</span>
          <div
            ref={radioGroupRef}
            role="radiogroup"
            aria-label="Export format"
            onKeyDown={handleFormatKeyDown}
            className="bg-muted grid grid-cols-3 gap-1 rounded-[10px] p-1"
          >
            {FORMAT_OPTIONS.map((opt) => {
              const active = format === opt.id;
              return (
                <button
                  key={opt.id}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  tabIndex={active ? 0 : -1}
                  data-export-format={opt.id}
                  onClick={() => setFormat(opt.id)}
                  className={cn(
                    "focus-visible:ring-foreground/25 cursor-pointer rounded-[7px] py-1.5 text-[12.5px] font-medium transition-colors outline-none focus-visible:ring-1",
                    active
                      ? "bg-background text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {opt.label}
                </button>
              );
            })}
          </div>
        </div>

        {/* Compact inclusion switches */}
        <div className="divide-y divide-black/[0.06] rounded-[11px] border border-black/[0.07] dark:divide-white/[0.07] dark:border-white/[0.08]">
          <div className="flex items-center justify-between gap-3 px-3 py-2.5">
            <span className="text-[13px] font-normal">Include thinking</span>
            <ToggleSwitch
              checked={includeThinking}
              onChange={setIncludeThinking}
              ariaLabel="Include thinking"
            />
          </div>
          <div className="flex items-center justify-between gap-3 px-3 py-2.5">
            <span className="text-[13px] font-normal">Include tool calls</span>
            <ToggleSwitch
              checked={includeToolCalls}
              onChange={setIncludeToolCalls}
              ariaLabel="Include tool calls"
            />
          </div>
        </div>

        {error && (
          <p className="rounded-lg bg-rose-500/10 px-3 py-2 text-[12px] text-rose-600 dark:text-rose-400">
            {error}
          </p>
        )}

        {/* Live metadata footer & actions */}
        <div className="flex items-center justify-between gap-3 pt-1">
          <div className="text-muted-foreground min-w-0 text-[12px] tabular-nums">
            {loading && !preview ? (
              <span className="inline-flex items-center gap-1.5">
                <Loader2 className="size-3 animate-spin" />
                <span>Preparing…</span>
              </span>
            ) : preview ? (
              <div className="flex flex-col">
                <span>
                  {preview.turn_count} {preview.turn_count === 1 ? "turn" : "turns"} ·{" "}
                  {formatCompactBytes(preview.byte_len)}
                </span>
                <span className="text-muted-foreground/70 truncate font-mono text-[11px]">
                  {preview.suggested_filename}
                </span>
              </div>
            ) : (
              <span>No messages</span>
            )}
          </div>

          <div className="flex shrink-0 items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={!preview || loading}
              onClick={() => void handleCopy()}
              className="h-[31px] gap-1.5 rounded-[8px] px-3 text-[12.5px]"
            >
              {copied ? (
                <Check className="size-3.5 text-emerald-500" />
              ) : (
                <Copy className="size-3.5" />
              )}
              <span>{copied ? "Copied" : "Copy"}</span>
            </Button>
            <Button
              type="button"
              size="sm"
              disabled={!preview || loading || saving}
              onClick={() => void handleSave()}
              className="h-[31px] gap-1.5 rounded-[8px] px-3.5 text-[12.5px]"
            >
              {saving ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <Download className="size-3.5" />
              )}
              <span>Save…</span>
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
