import { Suspense, lazy, memo, useMemo } from "react";
import { ChevronDown, Plus, Terminal as TerminalIcon, X } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const LazyXtermPane = lazy(() => import("@/components/xterm-pane"));

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
  const terminalTabs = useOrbitSelector((s) => s.terminalTabs);
  const activeTerminalTabKey = useOrbitSelector((s) => s.activeTerminalTabKey);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );

  const hostTabs = useMemo(
    () => terminalTabs.filter((t) => t.hostId === selectedHostId),
    [terminalTabs, selectedHostId],
  );

  const detachedTerminals = useMemo(() => {
    if (!hostDetail) return [];
    return hostDetail.terminals.filter(
      (t) => !hostTabs.some((openTab) => openTab.terminalId === t.terminal_id),
    );
  }, [hostDetail, hostTabs]);

  return (
    <div
      className={cn(
        "flex min-h-0 flex-1 flex-col overflow-hidden bg-[#F7F8FA] select-none dark:bg-[#0B0E14]",
        !visible && "hidden",
      )}
    >
      {/* Compact PTY Session Sub-Bar */}
      <div className="border-border bg-sidebar flex h-[36px] shrink-0 items-center justify-between gap-1.5 border-b px-2.5">
        <div className="flex min-w-0 items-center gap-1 overflow-x-auto">
          {hostTabs.map((tab) => {
            const active = tab.key === activeTerminalTabKey;
            return (
              <div
                key={tab.key}
                onClick={() => orbitActions.setActiveTerminalTab(tab.key)}
                className={cn(
                  "group inline-flex h-[24px] cursor-pointer items-center gap-1.5 rounded-md px-2 text-[11.5px] transition-colors",
                  active
                    ? "bg-accent text-foreground font-medium"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                )}
              >
                <TerminalIcon className="h-[12.5px] w-[12.5px] shrink-0 opacity-75" />
                <span className="max-w-36 truncate font-mono">
                  {tab.title} · {shortDir(tab.cwd)}
                </span>
                <button
                  type="button"
                  title="Kill shell"
                  onClick={(e) => {
                    e.stopPropagation();
                    orbitActions.closeTerminalTab(tab.key, true);
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
            className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
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
      </div>

      {/* Persistent Multi-Host WebGL Xterm Pool (never unmount other hosts' live PTYs on host switch) */}
      <div className="relative min-h-0 flex-1">
        {terminalTabs.length > 0 && (
          <Suspense
            fallback={
              <div className="text-muted-foreground flex h-full items-center justify-center text-[12.5px]">
                Initializing WebGL terminal…
              </div>
            }
          >
            {terminalTabs.map((tab) => (
              <LazyXtermPane
                key={tab.key}
                hostId={tab.hostId}
                slot={tab.slot}
                active={tab.hostId === selectedHostId && tab.key === activeTerminalTabKey}
                visible={visible && hostTabs.length > 0}
              />
            ))}
          </Suspense>
        )}
        {hostTabs.length === 0 && (
          <div className="bg-sidebar text-muted-foreground absolute inset-0 z-10 flex h-full flex-col items-center justify-center gap-2 text-[12.5px]">
            <span>No active shell on this host.</span>
            <button
              type="button"
              onClick={() => orbitActions.spawnNewTerminalTab(selectedHostId, selectedCwd || "~")}
              className="border-border bg-elevated text-foreground hover:bg-accent orbit-press cursor-pointer rounded-lg border px-3 py-1.5 text-[12px] font-medium"
            >
              Launch Shell
            </button>
          </div>
        )}
      </div>
    </div>
  );
});
