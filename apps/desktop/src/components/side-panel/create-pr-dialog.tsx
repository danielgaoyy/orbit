import { useEffect, useRef, useState } from "react";
import { AlertCircle, GitPullRequest, Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { Checkbox } from "@/components/ui/checkbox";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { orbitIpc, type GitCreatePrResult } from "@/lib/orbit-ipc";

export interface CreatePrDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  hostId: string;
  cwd: string;
  branch: string | null;
  defaultTitle: string;
  onCreated: (result: GitCreatePrResult) => void;
}

/**
 * Restrained GitHub Pull Request composer dialog built on Orbit's Dialog primitives.
 * Invokes `orbitIpc.gitCreatePr` directly and preserves user edits on inline error recovery.
 */
export function CreatePrDialog({
  open,
  onOpenChange,
  hostId,
  cwd,
  branch,
  defaultTitle,
  onCreated,
}: CreatePrDialogProps) {
  const { t } = useI18n();
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [base, setBase] = useState("");
  const [draft, setDraft] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const mountedRef = useRef(true);
  const submitGenRef = useRef(0);
  const titleIme = useImeCompositionGuard();
  const bodyIme = useImeCompositionGuard();
  const baseIme = useImeCompositionGuard();

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      submitGenRef.current += 1;
    };
  }, []);

  useEffect(() => {
    if (open) {
      setTitle(defaultTitle);
      setBody("");
      setBase("");
      setDraft(false);
      setErrorMessage(null);
      setSubmitting(false);
    } else {
      submitGenRef.current += 1;
    }
  }, [open, defaultTitle]);

  const trimmedTitle = title.trim();
  const canSubmit = Boolean(trimmedTitle) && !submitting;

  const handleSubmit = async () => {
    if (!canSubmit) return;
    const gen = ++submitGenRef.current;
    setSubmitting(true);
    setErrorMessage(null);
    try {
      const result = await orbitIpc.gitCreatePr({
        hostId,
        cwd,
        title: trimmedTitle,
        body,
        base: base.trim() || null,
        draft,
      });
      if (!mountedRef.current || submitGenRef.current !== gen) return;
      onOpenChange(false);
      onCreated(result);
    } catch (err) {
      if (!mountedRef.current || submitGenRef.current !== gen) return;
      const message = err instanceof Error ? err.message : String(err);
      setErrorMessage(message.trim() || t("common.actionFailed"));
    } finally {
      if (mountedRef.current && submitGenRef.current === gen) {
        setSubmitting(false);
      }
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && submitting) return;
        onOpenChange(nextOpen);
      }}
      disablePointerDismissal={submitting}
    >
      <DialogContent closeDisabled={submitting} className="max-w-[460px] gap-4 p-5">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-[15px]">
            <GitPullRequest className="text-muted-foreground h-[15px] w-[15px] shrink-0" />
            <span>{t("review.createPrModalTitle")}</span>
          </DialogTitle>
          <DialogDescription className="text-[12.5px]">
            {branch
              ? t("review.createPrModalDesc", { branch })
              : t("review.createPrModalDescDetached")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          <div className="space-y-1">
            <label
              htmlFor="orbit-pr-title"
              className="text-foreground/85 block text-[11.5px] font-medium"
            >
              {t("review.prTitleLabel")}
            </label>
            <input
              id="orbit-pr-title"
              type="text"
              autoFocus
              disabled={submitting}
              value={title}
              onChange={(e) => setTitle(e.target.value.replace(/[\r\n]+/g, " "))}
              {...titleIme.compositionProps}
              onKeyDown={(e) => {
                if (titleIme.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  void handleSubmit();
                }
              }}
              placeholder={t("review.prTitlePlaceholder")}
              className="border-border bg-background text-foreground placeholder:text-muted-foreground focus:border-foreground/30 h-[32px] w-full rounded-lg border px-2.5 text-[12.5px] transition-colors outline-none disabled:opacity-55"
            />
          </div>

          <div className="space-y-1">
            <label
              htmlFor="orbit-pr-body"
              className="text-foreground/85 block text-[11.5px] font-medium"
            >
              {t("review.prBodyLabel")}
            </label>
            <textarea
              id="orbit-pr-body"
              rows={5}
              disabled={submitting}
              value={body}
              onChange={(e) => setBody(e.target.value)}
              {...bodyIme.compositionProps}
              onKeyDown={(e) => {
                if (bodyIme.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  void handleSubmit();
                }
              }}
              placeholder={t("review.prBodyPlaceholder")}
              className="border-border bg-background text-foreground placeholder:text-muted-foreground focus:border-foreground/30 w-full resize-y rounded-lg border px-2.5 py-2 text-[12.5px] leading-relaxed transition-colors outline-none disabled:opacity-55"
            />
          </div>

          <div className="space-y-1">
            <label
              htmlFor="orbit-pr-base"
              className="text-foreground/85 block text-[11.5px] font-medium"
            >
              {t("review.prBaseLabel")}
            </label>
            <input
              id="orbit-pr-base"
              type="text"
              disabled={submitting}
              value={base}
              onChange={(e) => setBase(e.target.value)}
              {...baseIme.compositionProps}
              onKeyDown={(e) => {
                if (baseIme.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  void handleSubmit();
                }
              }}
              placeholder={t("review.prBasePlaceholder")}
              className="border-border bg-background text-foreground placeholder:text-muted-foreground focus:border-foreground/30 h-[30px] w-full rounded-lg border px-2.5 font-mono text-[12px] transition-colors outline-none disabled:opacity-55"
            />
          </div>

          <label className="text-foreground/85 flex cursor-pointer items-center gap-2 pt-0.5 text-[12px] select-none">
            <Checkbox
              id="orbit-pr-draft"
              size="sm"
              checked={draft}
              disabled={submitting}
              onCheckedChange={setDraft}
            />
            <span>{t("review.prDraftLabel")}</span>
          </label>
        </div>

        {errorMessage && (
          <div
            role="alert"
            className="flex items-start gap-2 rounded-[10px] border border-rose-500/25 bg-rose-500/[0.08] px-3 py-2 text-[12px] leading-relaxed text-rose-600 dark:text-rose-300"
          >
            <AlertCircle className="mt-0.5 h-[13.5px] w-[13.5px] shrink-0" />
            <span className="selectable-text min-w-0 break-words">{errorMessage}</span>
          </div>
        )}

        <div className="flex items-center justify-end gap-2 pt-0.5">
          <button
            type="button"
            disabled={submitting}
            onClick={() => onOpenChange(false)}
            className="border-border bg-background text-foreground/80 hover:bg-accent hover:text-foreground inline-flex h-[30px] cursor-pointer items-center justify-center rounded-lg border px-3 text-[12.5px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            disabled={!canSubmit}
            onClick={() => void handleSubmit()}
            className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center justify-center gap-1.5 rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity hover:opacity-90 disabled:pointer-events-none disabled:opacity-40"
          >
            {submitting && <Loader2 className="h-[12.5px] w-[12.5px] animate-spin" />}
            <span>{draft ? t("review.prSubmitDraft") : t("review.prSubmit")}</span>
          </button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
