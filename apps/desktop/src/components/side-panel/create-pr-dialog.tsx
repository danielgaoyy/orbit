import { useEffect, useRef, useState } from "react";
import { AlertCircle, GitPullRequest, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
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

        <FieldGroup className="gap-3">
          <Field>
            <FieldLabel htmlFor="orbit-pr-title">{t("review.prTitleLabel")}</FieldLabel>
            <Input
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
            />
          </Field>

          <Field>
            <FieldLabel htmlFor="orbit-pr-body">{t("review.prBodyLabel")}</FieldLabel>
            <Textarea
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
              className="resize-y leading-relaxed"
            />
          </Field>

          <Field>
            <FieldLabel htmlFor="orbit-pr-base">{t("review.prBaseLabel")}</FieldLabel>
            <Input
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
              className="h-[30px] font-mono text-[12px]"
            />
          </Field>

          <Field orientation="horizontal" className="justify-start gap-2 pt-0.5">
            <Checkbox
              id="orbit-pr-draft"
              size="sm"
              checked={draft}
              disabled={submitting}
              onCheckedChange={setDraft}
            />
            <FieldLabel htmlFor="orbit-pr-draft" className="cursor-pointer font-normal">
              {t("review.prDraftLabel")}
            </FieldLabel>
          </Field>
        </FieldGroup>

        {errorMessage && (
          <FieldError className="border-signal-danger-border bg-signal-danger-bg flex items-start gap-2 rounded-[10px] border px-3 py-2 leading-relaxed">
            <AlertCircle className="mt-0.5 h-[13.5px] w-[13.5px] shrink-0" />
            <span className="selectable-text min-w-0 break-words">{errorMessage}</span>
          </FieldError>
        )}

        <DialogFooter className="pt-0.5">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={submitting}
            onClick={() => onOpenChange(false)}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={!canSubmit}
            onClick={() => void handleSubmit()}
          >
            {submitting && <Loader2 className="h-[12.5px] w-[12.5px] animate-spin" />}
            <span>{draft ? t("review.prSubmitDraft") : t("review.prSubmit")}</span>
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
