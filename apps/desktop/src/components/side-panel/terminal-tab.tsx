import { Suspense, lazy, memo, useEffect, useMemo, useState } from "react";
import { ChevronDown, PictureInPicture2, Plus, X } from "lucide-react";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { type TerminalSummary, orbitIpc } from "@/lib/orbit-ipc";
import { useI18n } from "@/lib/i18n";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const LazyXtermPane = lazy(() => import("@/components/xterm-pane"));
const EMPTY_TERMINALS: readonly TerminalSummary[] = [];

function shortDir(p: string): string {
  if (!p || p === "~") return "~";
  const clean = p.replace(/\/+$/, "");
  return clean.split("/").pop() || clean;
}

/**
 * Multi-Host Detached PTY Surface inside the Right Side Panel.
 * Kept mounted across Side Panel tab switches (`visible` controls focus/fit) so WebGL xterm
 * instances preserve full scrollback and interactive processes without respawning.
 */
export const TerminalTab = memo(function TerminalTab({
  selectedHostId,
  visible,
}: {
  selectedHostId: string;
  visible: boolean;
}) {
  const { t } = useI18n();
  const terminalTabs = useOrbitSelector((s) => s.terminalTabs);
  const activeTerminalTabKey = useOrbitSelector((s) => s.activeTerminalTabKey);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const hostTerminals = useOrbitSelector((s) =>
    selectedHostId ? (s.hostDetails[selectedHostId]?.terminals ?? EMPTY_TERMINALS) : EMPTY_TERMINALS,
  );

  const [pendingCloseTab, setPendingCloseTab] = useState<{
    key: string;
    terminalId: string | null;
    title: string;
    cwd: string;
  } | null>(null);

  useEffect(() => {
    if (!pendingCloseTab) return;
    const stillMatches = terminalTabs.some(
      (t) => t.key === pendingCloseTab.key && t.terminalId === pendingCloseTab.terminalId,
    );
    if (!stillMatches) {
      setPendingCloseTab(null);
    }
  }, [pendingCloseTab, terminalTabs]);

  const hostTabs = useMemo(
    () => terminalTabs.filter((t) => t.hostId === selectedHostId),
    [terminalTabs, selectedHostId],
  );

  const activeHostTab = useMemo(
    () => hostTabs.find((tab) => tab.key === activeTerminalTabKey) ?? hostTabs[0] ?? null,
    [hostTabs, activeTerminalTabKey],
  );

  const detachedTerminals = useMemo(() => {
    if (hostTerminals.length === 0) return [];
    return hostTerminals.filter(
      (t) => !hostTabs.some((openTab) => openTab.terminalId === t.terminal_id),
    );
  }, [hostTerminals, hostTabs]);

  return (
    <div
      className={cn(
        "bg-canvas flex min-h-0 flex-1 flex-col overflow-hidden select-none dark:bg-[#0B0E14]",
        !visible && "hidden",
      )}
    >
      {/* Compact PTY Session Sub-Bar */}
      <div className="border-border/70 bg-canvas flex h-[38px] shrink-0 items-center justify-between gap-1.5 border-b px-3 dark:bg-[#0E121A]">
        <div className="flex min-w-0 items-center gap-1 overflow-x-auto">
          {hostTabs.map((tab) => {
            const active = tab.key === activeTerminalTabKey;
            return (
              <div
                key={tab.key}
                onClick={() => orbitActions.setActiveTerminalTab(tab.key)}
                className={cn(
                  "group inline-flex h-[26px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[11.5px] transition-colors",
                  active
                    ? "border-border bg-elevated text-foreground font-medium shadow-2xs dark:bg-white/[0.07]"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground border-transparent",
                )}
              >
                <span
                  className={cn(
                    "h-1.5 w-1.5 shrink-0 rounded-full",
                    active ? "bg-emerald-500" : "bg-muted-foreground/40",
                  )}
                />
                <span className="max-w-36 truncate font-mono">
                  {tab.title} · {shortDir(tab.cwd)}
                </span>
                <button
                  type="button"
                  title="Close shell"
                  aria-label={`Close ${tab.title}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    setPendingCloseTab({
                      key: tab.key,
                      terminalId: tab.terminalId,
                      title: tab.title,
                      cwd: tab.cwd,
                    });
                  }}
                  className="rounded p-0.5 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-black/10 dark:hover:bg-white/15"
                >
                  <X className="h-[11px] w-[11px]" />
                </button>
              </div>
            );
          })}

          <button
            type="button"
            title="New detached PTY shell"
            onClick={() => orbitActions.spawnNewTerminalTab(selectedHostId, selectedCwd || "~")}
            className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-lg transition-colors"
          >
            <Plus className="h-[14px] w-[14px]" />
          </button>

          {detachedTerminals.length > 0 && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] transition-colors"
                >
                  <span>Attach ({detachedTerminals.length})</span>
                  <ChevronDown className="h-[11.5px] w-[11.5px]" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-56">
                <DropdownMenuLabel>Surviving Shim Shells</DropdownMenuLabel>
                {detachedTerminals.map((t) => (
                  <DropdownMenuItem
                    key={t.terminal_id}
                    onSelect={() =>
                      orbitActions.attachExistingTerminal(
                        selectedHostId,
                        t.terminal_id,
                        t.cwd,
                        t.title,
                      )
                    }
                  >
                    <span className="truncate font-mono text-[12px]">
                      {t.title} · {shortDir(t.cwd)}
                    </span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        </div>

        {activeHostTab?.terminalId && (
          <SimpleTooltip label={t("terminal.detachToWindow")}>
            <button
              type="button"
              aria-label={t("terminal.detachToWindow")}
              onClick={() => {
                const target = activeHostTab;
                if (!target.terminalId) return;
                const tid = target.terminalId;
                void orbitIpc
                  .openDetachedTerminalWindow(target.hostId, tid, target.title, target.cwd)
                  .then((label) => {
                    if (label) {
                      void orbitActions.closeTerminalTab(target.key, false, tid);
                    }
                  })
                  .catch((err) => {
                    orbitActions.showToast(String(err));
                  });
              }}
              className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-lg transition-colors"
            >
              <PictureInPicture2 className="h-[13.5px] w-[13.5px]" />
            </button>
          </SimpleTooltip>
        )}
      </div>

      {/* Persistent Multi-Host WebGL Xterm Pool (never unmount other hosts' live PTYs on host switch) */}
      <div className="relative min-h-0 flex-1">
        {terminalTabs.length > 0 && (
          <Suspense
            fallback={
              <div className="text-muted-foreground flex h-full items-center justify-center text-[12.5px]">
                {t("sidePanel.initializingTerminal")}
              </div>
            }
          >
            {terminalTabs.map((tab) => (
              <LazyXtermPane
                key={tab.key}
                hostId={tab.hostId}
                terminalId={tab.terminalId}
                slot={tab.slot}
                active={tab.hostId === selectedHostId && tab.key === activeTerminalTabKey}
                visible={visible && hostTabs.length > 0}
              />
            ))}
          </Suspense>
        )}
        {hostTabs.length === 0 && (
          <div className="bg-sidebar text-muted-foreground absolute inset-0 z-10 flex h-full flex-col items-center justify-center gap-2 text-[12.5px]">
            <span>{t("sidePanel.noActiveShell")}</span>
            <button
              type="button"
              onClick={() => orbitActions.spawnNewTerminalTab(selectedHostId, selectedCwd || "~")}
              className="border-border bg-elevated text-foreground hover:bg-accent orbit-press cursor-pointer rounded-lg border px-3 py-1.5 text-[12px] font-medium"
            >
              {t("sidePanel.launchShell")}
            </button>
          </div>
        )}
      </div>

      <ConfirmDialog
        open={pendingCloseTab !== null}
        onOpenChange={(open) => {
          if (!open) setPendingCloseTab(null);
        }}
        title={
          pendingCloseTab
            ? t("sidePanel.closeShellTitle", { title: pendingCloseTab.title })
            : t("sidePanel.closeShellFallbackTitle")
        }
        description={t("sidePanel.closeShellDesc")}
        detail={
          pendingCloseTab ? (
            <span className="font-mono text-[11.5px]">
              {pendingCloseTab.title} · {shortDir(pendingCloseTab.cwd)}
            </span>
          ) : undefined
        }
        secondaryAction={{
          label: t("sidePanel.detach"),
          variant: "default",
          onClick: async () => {
            if (!pendingCloseTab) return;
            const detached = await orbitActions.closeTerminalTab(
              pendingCloseTab.key,
              false,
              pendingCloseTab.terminalId,
            );
            if (detached) {
              orbitActions.showToast(
                t("sidePanel.detachedToast", { title: pendingCloseTab.title }),
              );
            }
          },
        }}
        confirmLabel={t("sidePanel.killShell")}
        tone="danger"
        defaultAction="secondary"
        onConfirm={async () => {
          if (!pendingCloseTab) return;
          await orbitActions.closeTerminalTab(
            pendingCloseTab.key,
            true,
            pendingCloseTab.terminalId,
          );
        }}
      />
    </div>
  );
});
