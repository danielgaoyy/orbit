import * as React from "react";
import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import { AlertCircle, Loader2 } from "lucide-react";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export type ConfirmDefaultAction = "confirm" | "secondary" | "cancel";

/**
 * Resolves the default focused / Enter-triggered action for `ConfirmDialog`.
 * Destructive (`tone === "danger"`) dialogs default to `"cancel"` unless a caller
 * explicitly overrides `defaultAction`, preventing accidental Enter confirmation.
 */
export function resolveConfirmDefaultAction(
  tone: "default" | "danger",
  defaultAction?: ConfirmDefaultAction,
): ConfirmDefaultAction {
  if (defaultAction !== undefined) {
    return defaultAction;
  }
  return tone === "danger" ? "cancel" : "confirm";
}

export interface ConfirmDialogSecondaryAction {
  label: string;
  onClick: () => void | Promise<void>;
  variant?: "default" | "outline";
}

export interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: React.ReactNode;
  detail?: React.ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  tone?: "default" | "danger";
  secondaryAction?: ConfirmDialogSecondaryAction;
  defaultAction?: ConfirmDefaultAction;
  onConfirm: () => void | Promise<void>;
}

/**
 * Unified restrained confirmation dialog built on Base UI Dialog primitives.
 * Monochrome surface hierarchy with subtle danger accent reserved strictly for
 * destructive confirmation CTAs. Supports Esc to dismiss, Enter to trigger the
 * default action, inline error recovery on failed async mutations, and an
 * optional safe alternative action (e.g. Detach vs Kill).
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  detail,
  confirmLabel,
  cancelLabel,
  tone = "default",
  secondaryAction,
  defaultAction,
  onConfirm,
}: ConfirmDialogProps) {
  const { t } = useI18n();
  const resolvedDefaultAction = resolveConfirmDefaultAction(tone, defaultAction);
  const resolvedConfirmLabel = confirmLabel ?? t("common.confirm");
  const resolvedCancelLabel = cancelLabel ?? t("common.cancel");
  const [submitting, setSubmitting] = React.useState<"confirm" | "secondary" | null>(null);
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);
  const mountedRef = React.useRef(true);
  const actionGenRef = React.useRef(0);
  const confirmButtonRef = React.useRef<HTMLButtonElement | null>(null);
  const secondaryButtonRef = React.useRef<HTMLButtonElement | null>(null);
  const cancelButtonRef = React.useRef<HTMLButtonElement | null>(null);

  React.useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      actionGenRef.current += 1;
    };
  }, []);

  React.useEffect(() => {
    if (!open) {
      actionGenRef.current += 1;
      setSubmitting(null);
      setErrorMessage(null);
    }
  }, [open]);

  const busy = submitting !== null;

  const runAction = React.useCallback(
    async (kind: "confirm" | "secondary", fn: () => void | Promise<void>) => {
      if (submitting !== null) return;
      const gen = ++actionGenRef.current;
      setSubmitting(kind);
      setErrorMessage(null);
      try {
        await fn();
        if (mountedRef.current && actionGenRef.current === gen) {
          onOpenChange(false);
        }
      } catch (err) {
        if (mountedRef.current && actionGenRef.current === gen) {
          const message = err instanceof Error ? err.message : String(err);
          setErrorMessage(message.trim() || t("common.actionFailed"));
        }
      } finally {
        if (mountedRef.current && actionGenRef.current === gen) {
          setSubmitting(null);
        }
      }
    },
    [onOpenChange, submitting],
  );

  const handleOpenChange = React.useCallback(
    (nextOpen: boolean) => {
      if (!nextOpen && busy) return;
      onOpenChange(nextOpen);
    },
    [busy, onOpenChange],
  );

  const initialFocusRef = React.useMemo(() => {
    if (resolvedDefaultAction === "secondary" && secondaryAction) {
      return secondaryButtonRef;
    }
    if (resolvedDefaultAction === "cancel") {
      return cancelButtonRef;
    }
    return confirmButtonRef;
  }, [resolvedDefaultAction, secondaryAction]);

  const handlePopupKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (busy || e.defaultPrevented || e.nativeEvent.isComposing || e.key !== "Enter") {
      return;
    }
    const target = e.target as HTMLElement | null;
    const tag = target?.tagName ?? "";
    if (
      target?.isContentEditable ||
      tag === "BUTTON" ||
      tag === "INPUT" ||
      tag === "TEXTAREA" ||
      tag === "A"
    ) {
      return;
    }
    e.preventDefault();
    if (resolvedDefaultAction === "cancel") {
      handleOpenChange(false);
    } else if (resolvedDefaultAction === "secondary" && secondaryAction) {
      void runAction("secondary", secondaryAction.onClick);
    } else {
      void runAction("confirm", onConfirm);
    }
  };

  return (
    <BaseDialog.Root open={open} onOpenChange={handleOpenChange} disablePointerDismissal={busy}>
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className="orbit-dialog-overlay fixed inset-0 z-50 bg-black/40 backdrop-blur-[2px]" />
        <BaseDialog.Popup
          role="alertdialog"
          aria-modal="true"
          data-slot="confirm-dialog"
          initialFocus={initialFocusRef}
          onKeyDown={handlePopupKeyDown}
          className="bg-elevated text-foreground floating-surface orbit-dialog-content fixed top-1/2 left-1/2 z-50 flex w-[calc(100vw-2rem)] max-w-[400px] -translate-x-1/2 -translate-y-1/2 flex-col gap-4 rounded-[15px] border border-black/[0.09] p-5 outline-none dark:border-white/[0.10]"
        >
          <div className="space-y-1.5">
            <BaseDialog.Title className="text-foreground text-[14.5px] leading-snug font-semibold tracking-[-0.012em]">
              {title}
            </BaseDialog.Title>
            {description && (
              <BaseDialog.Description className="text-muted-foreground text-[12.5px] leading-[1.55]">
                {description}
              </BaseDialog.Description>
            )}
          </div>

          {detail && (
            <div className="border-border bg-canvas/70 text-muted-foreground rounded-[10px] border px-3 py-2 text-[12px] leading-relaxed">
              {detail}
            </div>
          )}

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
              ref={cancelButtonRef}
              type="button"
              disabled={busy}
              onClick={() => handleOpenChange(false)}
              className="border-border bg-background text-foreground/80 hover:bg-accent hover:text-foreground focus-visible:ring-foreground/20 inline-flex h-[30px] cursor-pointer items-center justify-center rounded-lg border px-3 text-[12.5px] font-medium transition-colors outline-none focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-45"
            >
              {resolvedCancelLabel}
            </button>

            {secondaryAction && (
              <button
                ref={secondaryButtonRef}
                type="button"
                disabled={busy}
                onClick={() => void runAction("secondary", secondaryAction.onClick)}
                className={cn(
                  "focus-visible:ring-foreground/25 inline-flex h-[30px] cursor-pointer items-center justify-center gap-1.5 rounded-lg px-3 text-[12.5px] font-medium transition-colors outline-none focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-45",
                  secondaryAction.variant === "outline"
                    ? "border-border bg-background text-foreground hover:bg-accent border"
                    : "bg-foreground text-background hover:opacity-90",
                )}
              >
                {submitting === "secondary" && (
                  <Loader2 className="h-[12.5px] w-[12.5px] animate-spin" />
                )}
                <span>{secondaryAction.label}</span>
              </button>
            )}

            <button
              ref={confirmButtonRef}
              type="button"
              disabled={busy}
              onClick={() => void runAction("confirm", onConfirm)}
              className={cn(
                "inline-flex h-[30px] cursor-pointer items-center justify-center gap-1.5 rounded-lg px-3 text-[12.5px] font-medium transition-colors outline-none focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-45",
                tone === "danger"
                  ? "border border-rose-500/30 bg-rose-500/[0.10] text-rose-600 hover:bg-rose-500/[0.16] focus-visible:ring-rose-500/30 dark:border-rose-400/30 dark:bg-rose-500/[0.15] dark:text-rose-300 dark:hover:bg-rose-500/[0.22]"
                  : "bg-foreground text-background focus-visible:ring-foreground/25 hover:opacity-90",
              )}
            >
              {submitting === "confirm" && (
                <Loader2 className="h-[12.5px] w-[12.5px] animate-spin" />
              )}
              <span>{resolvedConfirmLabel}</span>
            </button>
          </div>
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
}
