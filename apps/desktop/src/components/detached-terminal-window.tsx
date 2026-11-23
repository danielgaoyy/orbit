import { Suspense, lazy, useEffect } from "react";
import { CornerDownLeft } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { SimpleTooltip } from "@/components/ui/overlays";
import { useI18n } from "@/lib/i18n";
import { isMacDesktop, isTauriRuntime, orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const LazyXtermPane = lazy(() => import("@/components/xterm-pane"));

export interface DetachedTerminalParams {
  hostId: string;
  terminalId: string;
  title: string;
  cwd: string;
}

export function parseDetachedTerminalParams(search: string): DetachedTerminalParams | null {
  const params = new URLSearchParams(search);
  if (params.get("detachedTerminal") !== "1") {
    return null;
  }
  const hostId = params.get("hostId")?.trim() ?? "";
  const terminalId = params.get("terminalId")?.trim() ?? "";
  if (!hostId || !terminalId) {
    return null;
  }
  const title = params.get("title")?.trim() || "Orbit Terminal";
  const cwd = params.get("cwd")?.trim() || "";
  return { hostId, terminalId, title, cwd };
}

function shortDir(path: string): string {
  if (!path || path === "~") return "~";
  const clean = path.replace(/\/+$/, "");
  return clean.split("/").pop() || clean;
}

/**
 * Minimal chrome-free floating terminal window opened via `open_detached_terminal_window`.
 * Deliberately bypasses `orbitActions.init()` so secondary windows never replace the main
 * window's `subscribe_core_events` subscriber, instead streaming directly over binary PTY IPC.
 */
export function DetachedTerminalWindow({
  hostId,
  terminalId,
  title,
  cwd,
}: DetachedTerminalParams) {
  const { t } = useI18n();
  const isMac = isMacDesktop();

  useEffect(() => {
    document.title = title;
  }, [title]);

  // Keep theme, locale, and terminal typography live-synced when preferences change in main window
  useEffect(() => {
    if (typeof window === "undefined") return;
    const onStorage = () => {
      orbitActions.syncAppearanceFromStorage();
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  // Support Cmd/Ctrl+W to close floating window cleanly without killing the remote PTY shim
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const mod = isMac ? e.metaKey : e.ctrlKey;
      if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "w") {
        e.preventDefault();
        if (isTauriRuntime()) {
          void getCurrentWindow().close();
        } else {
          window.close();
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [isMac]);

  const handleReattach = () => {
    void orbitIpc.reattachDetachedTerminal(hostId, terminalId, title, cwd);
  };

  return (
    <div className="bg-canvas text-foreground flex h-screen w-screen flex-col overflow-hidden select-none dark:bg-[#0B0E14]">
      {/* Restrained draggable window header */}
      <header
        data-tauri-drag-region
        className={cn(
          "border-border/70 bg-canvas flex h-[36px] shrink-0 items-center justify-between gap-2 border-b pr-2.5 dark:bg-[#0E121A]",
          isMac ? "pl-[78px]" : "pl-3",
        )}
      >
        <div
          data-tauri-drag-region
          className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden"
        >
          <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-emerald-500" />
          <span
            data-tauri-drag-region
            className="text-foreground truncate font-mono text-[12px] font-medium"
          >
            {title}
          </span>
          {cwd && (
            <span
              data-tauri-drag-region
              className="text-muted-foreground truncate font-mono text-[11px]"
            >
              · {shortDir(cwd)}
            </span>
          )}
        </div>

        <SimpleTooltip label={t("terminal.reattachTooltip")}>
          <button
            type="button"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={handleReattach}
            className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[24px] shrink-0 cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium transition-colors"
          >
            <CornerDownLeft className="h-[12px] w-[12px]" />
            <span>{t("terminal.reattach")}</span>
          </button>
        </SimpleTooltip>
      </header>

      <div className="relative min-h-0 flex-1">
        <Suspense
          fallback={
            <div className="text-muted-foreground flex h-full items-center justify-center text-[12.5px]">
              {t("sidePanel.initializingTerminal")}
            </div>
          }
        >
          <LazyXtermPane
            hostId={hostId}
            terminalId={terminalId}
            active
            visible
            detached
          />
        </Suspense>
      </div>
    </div>
  );
}
