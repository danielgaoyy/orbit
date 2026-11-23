import { memo } from "react";
import { ExternalLink } from "lucide-react";
import { orbitIpc } from "@/lib/orbit-ipc";
import type { ToastAction, UndoArchiveEntry } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export const ToastViewport = memo(function ToastViewport({
  message,
  action,
  undoEntry,
  exiting,
  onUndoArchive,
}: {
  message: string | null;
  action: ToastAction | null;
  undoEntry: UndoArchiveEntry | null;
  exiting: boolean;
  onUndoArchive: () => void;
}) {
  if (!message) return null;

  const isInteractive = Boolean(undoEntry || action);

  return (
    <div
      role="status"
      aria-live="polite"
      className={cn(
        "floating-surface text-ink-primary fixed right-4 bottom-4 z-50 flex items-center gap-2.5 rounded-[12px] border px-3.5 py-2 text-[13px]",
        isInteractive ? "pointer-events-auto" : "pointer-events-none",
        exiting ? "orbit-toast-exit" : "orbit-toast-enter",
      )}
    >
      <span>{message}</span>
      {action && (
        <button
          type="button"
          onClick={() => {
            if (action.url) {
              void orbitIpc.openExternalBrowserUrl(action.url);
            }
            action.onClick?.();
          }}
          className="text-signal-info hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-[6px] px-2 py-0.5 text-[12px] font-medium transition-colors"
        >
          <span>{action.label}</span>
          {action.url && <ExternalLink className="size-3 shrink-0" />}
        </button>
      )}
      {undoEntry && (
        <button
          type="button"
          onClick={onUndoArchive}
          className="text-signal-info hover:bg-accent cursor-pointer rounded-[6px] px-2 py-0.5 text-[12px] font-medium transition-colors"
        >
          Undo
        </button>
      )}
    </div>
  );
});
