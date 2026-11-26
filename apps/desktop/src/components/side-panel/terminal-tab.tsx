import { Suspense, lazy, memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, Columns2, PictureInPicture2, Plus, Rows2, X } from "lucide-react";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { DragShield } from "@/components/ui/drag-feedback";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { ResizeSash } from "@/components/ui/resize-sash";
import { useInlineSortable } from "@/lib/drag/use-inline-sortable";
import { modSymbol } from "@/lib/keybindings";
import { type TerminalSummary, orbitIpc } from "@/lib/orbit-ipc";
import { useI18n } from "@/lib/i18n";
import { adjustSplitPaneRatios } from "@/lib/xterm-osc133";
import {
  type SidePanelTab,
  type TerminalSplitDirection,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
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
 * Kept mounted across Side Panel tab switches (`visible` controls focus/fit) and across
 * horizontal/vertical split transitions (via flat CSS-ordered flex children) so WebGL xterm
 * instances preserve full scrollback and interactive processes without respawning.
 */
export const TerminalTab = memo(function TerminalTab({
  tab,
  selectedHostId: propHostId,
  visible,
}: {
  tab?: SidePanelTab;
  selectedHostId: string;
  visible: boolean;
}) {
  const selectedHostId = tab?.hostId ?? propHostId;
  const { t } = useI18n();
  const mod = modSymbol();
  const terminalTabs = useOrbitSelector((s) => s.terminalTabs);
  const terminalSplits = useOrbitSelector((s) => s.terminalSplits);
  const activeTerminalTabKey = useOrbitSelector((s) => s.activeTerminalTabKey);
  const selectedCwd = tab?.cwd ?? "~";
  const hostTerminals = useOrbitSelector((s) =>
    selectedHostId ? (s.hostDetails[selectedHostId]?.terminals ?? EMPTY_TERMINALS) : EMPTY_TERMINALS,
  );

  const splitContainerRef = useRef<HTMLDivElement | null>(null);
  const terminalTabStripRef = useRef<HTMLDivElement | null>(null);

  const [pendingCloseTab, setPendingCloseTab] = useState<{
    key: string;
    terminalId: string | null;
    title: string;
    cwd: string;
  } | null>(null);

  useEffect(() => {
    if (!pendingCloseTab) return;
    const stillMatches = terminalTabs.some(
      (tab) => tab.key === pendingCloseTab.key && tab.terminalId === pendingCloseTab.terminalId,
    );
    if (!stillMatches) {
      setPendingCloseTab(null);
    }
  }, [pendingCloseTab, terminalTabs]);

  const hostTabs = useMemo(
    () => terminalTabs.filter((tab) => tab.hostId === selectedHostId),
    [terminalTabs, selectedHostId],
  );

  const activeHostTab = useMemo(
    () => hostTabs.find((tab) => tab.key === activeTerminalTabKey) ?? hostTabs[0] ?? null,
    [hostTabs, activeTerminalTabKey],
  );

  const activeSplit = useMemo(() => {
    if (!activeHostTab) return null;
    const match = terminalSplits.find(
      (g) => g.hostId === selectedHostId && g.paneKeys.includes(activeHostTab.key),
    );
    if (!match) return null;
    const liveKeys = match.paneKeys.filter((k) => hostTabs.some((tab) => tab.key === k));
    if (liveKeys.length < 2) return null;
    const normalizedRatios =
      match.ratios.length === liveKeys.length
        ? match.ratios
        : Array.from({ length: liveKeys.length }, () => 1 / liveKeys.length);
    return {
      ...match,
      paneKeys: liveKeys,
      ratios: normalizedRatios,
    };
  }, [terminalSplits, selectedHostId, activeHostTab, hostTabs]);

  const splitKeyToDirection = useMemo(() => {
    const map = new Map<string, TerminalSplitDirection>();
    for (const g of terminalSplits) {
      if (g.hostId !== selectedHostId || g.paneKeys.length < 2) continue;
      for (const k of g.paneKeys) {
        map.set(k, g.direction);
      }
    }
    return map;
  }, [terminalSplits, selectedHostId]);

  const detachedTerminals = useMemo(() => {
    if (hostTerminals.length === 0) return [];
    return hostTerminals.filter(
      (term) => !hostTabs.some((openTab) => openTab.terminalId === term.terminal_id),
    );
  }, [hostTerminals, hostTabs]);

  const handleRequestSplit = useCallback((direction: TerminalSplitDirection) => {
    orbitActions.splitActiveTerminalPane(direction);
  }, []);

  const hostTabKeys = useMemo(() => hostTabs.map((tab) => tab.key), [hostTabs]);
  const termTabSortable = useInlineSortable({
    axis: "x",
    itemIds: hostTabKeys,
    scrollViewportRef: terminalTabStripRef,
    ignoreSelector: "button",
    onCommit: (fromIdx, toIdx) => {
      orbitActions.reorderTerminalTabs(selectedHostId, fromIdx, toIdx);
    },
  });

  const splitRightShortcut = mod === "⌘" ? "⌘⇧D" : "Ctrl+Shift+D";
  const splitDownShortcut = mod === "⌘" ? "⌘⌥D" : "Ctrl+Alt+D";

  return (
    <div
      className={cn(
        "bg-surface-recessed flex min-h-0 flex-1 flex-col overflow-hidden select-none",
        !visible && "hidden",
      )}
    >
      <DragShield active={termTabSortable.isDragging} cursor="grabbing" />

      {/* Compact PTY Session Sub-Bar — seamless with Workbench chrome */}
      <div className="border-border-subtle bg-surface-base flex h-[36px] shrink-0 items-center justify-between gap-1.5 border-b px-2.5">
        <div
          ref={terminalTabStripRef}
          onWheel={(e) => {
            const el = terminalTabStripRef.current;
            if (!el || el.scrollWidth <= el.clientWidth) return;
            if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
              el.scrollLeft += e.deltaY;
            }
          }}
          className="flex h-full min-w-0 flex-1 items-center gap-1 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          {hostTabs.map((tab, idx) => {
            const focused = tab.key === activeHostTab?.key;
            const visibleInSplit = Boolean(activeSplit && activeSplit.paneKeys.includes(tab.key));
            const splitDir = splitKeyToDirection.get(tab.key);
            const isDragged = termTabSortable.sortState?.fromIndex === idx;
            const isSettling = isDragged && termTabSortable.sortState?.phase === "settling";

            return (
              <div
                key={tab.key}
                ref={(el) => termTabSortable.registerItem(tab.key, el)}
                onPointerDown={
                  hostTabs.length > 1
                    ? (e) => termTabSortable.handlePointerDown(e, idx)
                    : undefined
                }
                onDragStart={(e) => e.preventDefault()}
                onClick={() => {
                  if (termTabSortable.shouldSuppressClick()) return;
                  orbitActions.setActiveTerminalTab(tab.key);
                }}
                style={termTabSortable.getNeighborStyle(idx)}
                className={cn(
                  "group relative inline-flex h-[24px] shrink-0 touch-none cursor-pointer items-center gap-1.5 rounded-[7px] border px-2 text-[11px] whitespace-nowrap [-webkit-user-drag:none]",
                  isDragged
                    ? cn(
                        "drag-lifted-surface border-transparent text-ink-primary z-10",
                        focused && "font-medium",
                        !isSettling && "cursor-grabbing",
                      )
                    : cn(
                        termTabSortable.isActive
                          ? "transition-transform duration-150 ease-[var(--ease-out-expo)] motion-reduce:transition-none"
                          : "transition-colors",
                        focused
                          ? "border-border-default bg-surface-raised text-ink-primary font-medium"
                          : visibleInSplit
                            ? "border-border-subtle bg-surface-recessed/70 text-ink-secondary"
                            : "text-ink-tertiary hover:bg-accent hover:text-ink-primary border-transparent",
                      ),
                )}
              >
                <span
                  className={cn(
                    "size-1.5 shrink-0 rounded-full",
                    focused ? "bg-signal-success" : "bg-ink-muted/50",
                  )}
                />
                {splitDir &&
                  (splitDir === "vertical" ? (
                    <Rows2 className="text-ink-faint size-3 shrink-0" />
                  ) : (
                    <Columns2 className="text-ink-faint size-3 shrink-0" />
                  ))}
                <span className="max-w-36 truncate font-mono">
                  {tab.title} · {shortDir(tab.cwd)}
                </span>
                <button
                  type="button"
                  title={t("terminal.closePane")}
                  aria-label={`Close ${tab.title}`}
                  onPointerDown={(e) => e.stopPropagation()}
                  onClick={(e) => {
                    e.stopPropagation();
                    setPendingCloseTab({
                      key: tab.key,
                      terminalId: tab.terminalId,
                      title: tab.title,
                      cwd: tab.cwd,
                    });
                  }}
                  className={cn(
                    "rounded p-0.5 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-black/10 dark:hover:bg-white/15",
                    termTabSortable.isActive && "pointer-events-none",
                  )}
                >
                  <X className="h-[11px] w-[11px]" />
                </button>
              </div>
            );
          })}

          <SimpleTooltip label={t("terminal.newShell")}>
            <button
              type="button"
              aria-label={t("terminal.newShell")}
              onClick={() => orbitActions.spawnNewTerminalTab(selectedHostId, selectedCwd || "~")}
              className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-lg transition-colors"
            >
              <Plus className="h-[14px] w-[14px]" />
            </button>
          </SimpleTooltip>

          {detachedTerminals.length > 0 && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[24px] shrink-0 cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] whitespace-nowrap transition-colors"
                >
                  <span>Attach ({detachedTerminals.length})</span>
                  <ChevronDown className="h-[11.5px] w-[11.5px]" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="min-w-[208px] max-w-[280px]">
                <DropdownMenuGroup>
                  <DropdownMenuLabel>Surviving Shim Shells</DropdownMenuLabel>
                  {detachedTerminals.map((term) => (
                    <DropdownMenuItem
                      key={term.terminal_id}
                      onSelect={() =>
                        orbitActions.attachExistingTerminal(
                          selectedHostId,
                          term.terminal_id,
                          term.cwd,
                          term.title,
                        )
                      }
                    >
                      <span className="truncate font-mono text-[12px]">
                        {term.title} · {shortDir(term.cwd)}
                      </span>
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-0.5">
          {activeHostTab && (
            <>
              <SimpleTooltip label={t("terminal.splitRight")} shortcut={splitRightShortcut}>
                <button
                  type="button"
                  aria-label={t("terminal.splitRight")}
                  onClick={() => handleRequestSplit("horizontal")}
                  className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-lg transition-colors"
                >
                  <Columns2 className="h-[13.5px] w-[13.5px]" />
                </button>
              </SimpleTooltip>
              <SimpleTooltip label={t("terminal.splitDown")} shortcut={splitDownShortcut}>
                <button
                  type="button"
                  aria-label={t("terminal.splitDown")}
                  onClick={() => handleRequestSplit("vertical")}
                  className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-lg transition-colors"
                >
                  <Rows2 className="h-[13.5px] w-[13.5px]" />
                </button>
              </SimpleTooltip>
            </>
          )}

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
      </div>

      {/* Persistent Multi-Host WebGL Xterm Pool — flat CSS-ordered flex container keeps every
          live XtermPane mounted across host switches, split/unsplit, and direction flips. */}
      <div
        ref={splitContainerRef}
        className={cn(
          "relative flex min-h-0 flex-1 overflow-hidden",
          activeSplit?.direction === "vertical" ? "flex-col" : "flex-row",
        )}
      >
        {terminalTabs.length > 0 && (
          <Suspense
            fallback={
              <div className="text-muted-foreground flex h-full w-full items-center justify-center text-[12.5px]">
                {t("sidePanel.initializingTerminal")}
              </div>
            }
          >
            {terminalTabs.map((tab) => {
              const splitIdx =
                activeSplit && tab.hostId === selectedHostId
                  ? activeSplit.paneKeys.indexOf(tab.key)
                  : -1;
              const isInActiveSplit = splitIdx >= 0;
              const isPaneVisible = isInActiveSplit
                ? true
                : !activeSplit &&
                  tab.hostId === selectedHostId &&
                  tab.key === activeHostTab?.key;
              const isFocusedPane =
                tab.hostId === selectedHostId && tab.key === activeHostTab?.key;
              const ratio =
                isInActiveSplit && activeSplit
                  ? (activeSplit.ratios[splitIdx] ?? 1 / activeSplit.paneKeys.length)
                  : 1;

              return (
                <div
                  key={tab.key}
                  style={
                    isPaneVisible
                      ? isInActiveSplit
                        ? {
                            flexBasis: 0,
                            flexGrow: ratio,
                            flexShrink: 1,
                            order: splitIdx * 2,
                          }
                        : { flexBasis: "100%", flexGrow: 1, flexShrink: 1, order: 0 }
                      : undefined
                  }
                  className={cn(
                    "relative min-h-0 min-w-0 overflow-hidden",
                    !isPaneVisible && "hidden",
                  )}
                >
                  <LazyXtermPane
                    hostId={tab.hostId}
                    terminalId={tab.terminalId}
                    slot={tab.slot}
                    active={isPaneVisible}
                    focused={isFocusedPane}
                    visible={visible && hostTabs.length > 0}
                    paneTitle={`${tab.title} · ${shortDir(tab.cwd)}`}
                    isSplit={isInActiveSplit}
                    onPaneFocus={() => {
                      if (activeTerminalTabKey !== tab.key) {
                        orbitActions.setActiveTerminalTab(tab.key);
                      }
                    }}
                    onRequestSplit={handleRequestSplit}
                    onRequestClosePane={
                      isInActiveSplit
                        ? () =>
                            setPendingCloseTab({
                              key: tab.key,
                              terminalId: tab.terminalId,
                              title: tab.title,
                              cwd: tab.cwd,
                            })
                        : undefined
                    }
                  />
                </div>
              );
            })}

            {activeSplit &&
              activeSplit.paneKeys.slice(0, -1).map((paneKey, idx) => {
                const isVertical = activeSplit.direction === "vertical";
                return (
                  <ResizeSash
                    key={`divider:${activeSplit.id}:${paneKey}`}
                    orientation={isVertical ? "horizontal" : "vertical"}
                    variant="split"
                    style={{ order: idx * 2 + 1 }}
                    ariaLabel={t("terminal.resizeSplit")}
                    title={t("terminal.resizeSplitHint")}
                    onDragStart={() => {
                      const container = splitContainerRef.current;
                      if (!container) return null;
                      const totalSpan = isVertical ? container.clientHeight : container.clientWidth;
                      if (totalSpan <= 0) return null;
                      return {
                        splitId: activeSplit.id,
                        startRatios: activeSplit.ratios.slice(),
                        totalSpan,
                      };
                    }}
                    onDragMove={(delta, snap) => {
                      const next = adjustSplitPaneRatios(
                        snap.startRatios,
                        idx,
                        delta / snap.totalSpan,
                        0.18,
                      );
                      orbitActions.setTerminalSplitRatios(snap.splitId, next);
                    }}
                    onDragCancel={(snap) => {
                      orbitActions.setTerminalSplitRatios(snap.splitId, snap.startRatios);
                    }}
                    onReset={() => {
                      const equal = Array.from(
                        { length: activeSplit.paneKeys.length },
                        () => 1 / activeSplit.paneKeys.length,
                      );
                      orbitActions.setTerminalSplitRatios(activeSplit.id, equal);
                    }}
                    onKeyboardStep={(mult) => {
                      const next = adjustSplitPaneRatios(
                        activeSplit.ratios,
                        idx,
                        mult * 0.04,
                        0.18,
                      );
                      orbitActions.setTerminalSplitRatios(activeSplit.id, next);
                    }}
                  />
                );
              })}
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
