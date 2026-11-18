import { Suspense, lazy, useMemo } from "react";
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

export function TerminalDock() {
  const terminalOpen = useOrbitSelector((s) => s.terminalOpen);
  const terminalHeight = useOrbitSelector((s) => s.terminalHeight);
  const terminalTabs = useOrbitSelector((s) => s.terminalTabs);
  const activeTerminalTabKey = useOrbitSelector((s) => s.activeTerminalTabKey);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
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

  if (!selectedHostId) return null;

  const startResize = (e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startY = e.clientY;
    const startH = terminalHeight;

    const onMove = (ev: MouseEvent) => {
      const delta = startY - ev.clientY;
      orbitActions.setTerminalHeight(startH + delta);
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  return (
    <section
      style={{ height: `${terminalHeight}px` }}
      className={cn(
        "border-border text-foreground relative flex shrink-0 flex-col border-t bg-[#F7F8FA] select-none dark:bg-[#0B0E14]",
        !terminalOpen && "hidden",
      )}
    >
      {/* Top Drag Resize Handle */}
      <div
        onMouseDown={startResize}
        className="hover:bg-primary/40 absolute top-0 left-0 z-20 h-1 w-full cursor-row-resize transition-colors"
      />

      {/* Compact Terminal Tab Header */}
      <div className="border-border bg-sidebar flex h-8 shrink-0 items-center justify-between border-b px-2.5">
        <div className="flex min-w-0 items-center gap-1 overflow-x-auto">
          {hostTabs.map((tab) => {
            const active = tab.key === activeTerminalTabKey;
            return (
              <div
                key={tab.key}
                onClick={() => orbitActions.setActiveTerminalTab(tab.key)}
                className={cn(
                  "group inline-flex h-6 cursor-pointer items-center gap-1.5 rounded px-2 text-[11px] transition-colors",
                  active
                    ? "bg-accent text-foreground font-medium"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                )}
              >
                <TerminalIcon className="size-3 shrink-0 opacity-75" />
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
                  <X className="size-2.5" />
                </button>
              </div>
            );
          })}

          <button
            type="button"
            title="New detached PTY shell"
            onClick={() => orbitActions.spawnNewTerminalTab(selectedHostId, selectedCwd || "~")}
            className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex size-6 cursor-pointer items-center justify-center rounded"
          >
            <Plus className="size-3.5" />
          </button>

          {detachedTerminals.length > 0 && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-6 cursor-pointer items-center gap-1 rounded px-1.5 text-[10.5px]"
                >
                  <span>Attach ({detachedTerminals.length})</span>
                  <ChevronDown className="size-2.5" />
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
                    <span className="truncate font-mono">
                      {t.title} · {shortDir(t.cwd)}
                    </span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        </div>

        <button
          type="button"
          title="Hide terminal drawer"
          onClick={() => orbitActions.toggleTerminalDock(false)}
          className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex size-6 cursor-pointer items-center justify-center rounded"
        >
          <X className="size-3.5" />
        </button>
      </div>

      {/* Persistent Xterm Pool (keeps ALL tabs mounted across host & drawer switches!) */}
      <div className="min-h-0 flex-1">
        {terminalTabs.length === 0 ? (
          <div className="flex h-full items-center justify-center text-xs text-zinc-500">
            Launching remote PTY shim...
          </div>
        ) : (
          <Suspense
            fallback={
              <div className="flex h-full items-center justify-center text-xs text-zinc-500">
                Initializing WebGL terminal...
              </div>
            }
          >
            {terminalTabs.map((tab) => (
              <LazyXtermPane
                key={tab.key}
                hostId={tab.hostId}
                slot={tab.slot}
                active={tab.hostId === selectedHostId && tab.key === activeTerminalTabKey}
                visible={terminalOpen}
              />
            ))}
          </Suspense>
        )}
      </div>
    </section>
  );
}
