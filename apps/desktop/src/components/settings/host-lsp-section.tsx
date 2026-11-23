import { useEffect, useMemo, useState } from "react";
import { ChevronsUpDown, Check, Loader2, RefreshCw, RotateCw } from "lucide-react";
import {
  SettingCard,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import type { LspServerState, LspServerStatus } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_SERVERS: readonly LspServerStatus[] = [];

function statusDotClass(state: LspServerState): string {
  switch (state) {
    case "ready":
      return "bg-emerald-500";
    case "starting":
      return "bg-amber-500 animate-pulse";
    case "error":
      return "bg-rose-500";
    case "stopped":
      return "bg-zinc-400 dark:bg-zinc-500";
    case "unavailable":
      return "bg-zinc-300/70 dark:bg-zinc-700";
  }
}

function statusLabel(state: LspServerState): string {
  switch (state) {
    case "ready":
      return "Ready";
    case "starting":
      return "Starting";
    case "error":
      return "Error";
    case "stopped":
      return "Idle";
    case "unavailable":
      return "Not installed";
  }
}

function formatWorkspaceShortName(cwd: string): string {
  const clean = cwd.replace(/\/+$/, "");
  const parts = clean.split("/");
  return parts[parts.length - 1] || clean || "~";
}

function formatLanguages(server: LspServerStatus): string {
  if (server.language_ids.length > 0) {
    return server.language_ids.slice(0, 3).join(", ");
  }
  if (server.file_extensions.length > 0) {
    return server.file_extensions
      .slice(0, 4)
      .map((e) => `.${e}`)
      .join(", ");
  }
  return "Custom";
}

export function HostLspSection({ hostId }: { hostId: string }) {
  const hostCard = useOrbitSelector(
    (s) => s.fleet.hosts.find((h) => h.host_id === hostId) ?? null,
  );
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedWorkspaceCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);

  const workspaces = useMemo(() => {
    const seen = new Set<string>();
    const list: Array<{ cwd: string; label: string }> = [];
    if (selectedHostId === hostId && selectedWorkspaceCwd && selectedWorkspaceCwd !== "~") {
      seen.add(selectedWorkspaceCwd);
      list.push({
        cwd: selectedWorkspaceCwd,
        label: formatWorkspaceShortName(selectedWorkspaceCwd),
      });
    }
    for (const ws of hostDetail?.workspaces ?? []) {
      if (!ws.cwd || ws.cwd === "~" || ws.archived_at || seen.has(ws.cwd)) continue;
      seen.add(ws.cwd);
      list.push({
        cwd: ws.cwd,
        label: ws.title || formatWorkspaceShortName(ws.cwd),
      });
    }
    for (const proj of hostDetail?.discovered_projects ?? []) {
      if (!proj.path || proj.path === "~" || seen.has(proj.path)) continue;
      seen.add(proj.path);
      list.push({
        cwd: proj.path,
        label: proj.name || formatWorkspaceShortName(proj.path),
      });
    }
    return list;
  }, [hostDetail, hostId, selectedHostId, selectedWorkspaceCwd]);

  const [pickedCwd, setPickedCwd] = useState<string>(
    () => workspaces[0]?.cwd ?? "",
  );

  useEffect(() => {
    if (!pickedCwd && workspaces.length > 0) {
      setPickedCwd(workspaces[0].cwd);
    } else if (pickedCwd && workspaces.length > 0 && !workspaces.some((w) => w.cwd === pickedCwd)) {
      setPickedCwd(workspaces[0].cwd);
    }
  }, [pickedCwd, workspaces]);

  const scopeKey = hostId && pickedCwd ? `${hostId}::${pickedCwd}` : "";
  const entry = useOrbitSelector((s) =>
    scopeKey ? (s.lspServersByWorkspace[scopeKey] ?? null) : null,
  );
  const servers = entry?.servers ?? EMPTY_SERVERS;
  const isLoading = entry?.status === "loading";
  const [restartingId, setRestartingId] = useState<string | null>(null);

  const isOnline =
    hostCard?.connection_state === "online_direct" ||
    hostCard?.connection_state === "online_relay";

  useEffect(() => {
    if (isOnline && hostId && pickedCwd) {
      void orbitActions.loadLspServers(hostId, pickedCwd);
    }
  }, [isOnline, hostId, pickedCwd]);

  const handleRestart = async (serverId: string) => {
    if (!pickedCwd || restartingId) return;
    setRestartingId(serverId);
    try {
      await orbitActions.restartLspServer(hostId, pickedCwd, serverId);
    } finally {
      setRestartingId(null);
    }
  };

  const activeWorkspace = workspaces.find((w) => w.cwd === pickedCwd) ?? null;

  return (
    <div className="space-y-5">
      <SettingSectionHeader
        title="Language Servers"
        description="Lazy-started stdio LSP servers for diagnostics, symbols, and go-to-definition. Idle servers shut down automatically."
        action={
          pickedCwd && isOnline ? (
            <button
              type="button"
              disabled={isLoading}
              onClick={() => void orbitActions.loadLspServers(hostId, pickedCwd)}
              className="border-border bg-background hover:bg-accent text-foreground inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
            >
              <RefreshCw className={cn("h-[12.5px] w-[12.5px]", isLoading && "animate-spin")} />
              Refresh
            </button>
          ) : undefined
        }
      />

      {!isOnline ? (
        <SettingCard>
          <div className="text-muted-foreground px-4 py-6 text-center text-[13px]">
            Connect to this host to inspect workspace language servers.
          </div>
        </SettingCard>
      ) : workspaces.length === 0 ? (
        <SettingCard>
          <div className="text-muted-foreground px-4 py-6 text-center text-[13px]">
            Open a workspace on this host to discover language servers.
          </div>
        </SettingCard>
      ) : (
        <>
          {/* Compact Workspace Picker */}
          <div className="flex items-center justify-between gap-3">
            <span className="text-muted-foreground text-[12px] font-medium">Workspace</span>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="border-border bg-background hover:bg-accent text-foreground inline-flex h-[30px] max-w-[360px] cursor-pointer items-center gap-2 rounded-lg border px-2.5 text-[12.5px] transition-colors outline-none"
                >
                  <span className="truncate font-medium">
                    {activeWorkspace?.label ?? formatWorkspaceShortName(pickedCwd)}
                  </span>
                  <span className="text-muted-foreground/75 truncate font-mono text-[11px]">
                    {pickedCwd}
                  </span>
                  <ChevronsUpDown className="text-muted-foreground h-[12.5px] w-[12.5px] shrink-0" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="max-h-[260px] w-[340px] overflow-auto p-1">
                {workspaces.map((ws) => (
                  <DropdownMenuItem
                    key={ws.cwd}
                    onSelect={() => setPickedCwd(ws.cwd)}
                    className="flex items-center justify-between gap-2 py-1.5"
                  >
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[12.5px] font-medium">{ws.label}</span>
                      <span className="text-muted-foreground block truncate font-mono text-[10.5px]">
                        {ws.cwd}
                      </span>
                    </span>
                    {ws.cwd === pickedCwd && <Check className="h-[13px] w-[13px] shrink-0" />}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>

          {entry?.status === "error" && entry.error && (
            <div className="rounded-lg border border-rose-500/25 bg-rose-500/[0.07] px-3 py-2 text-[12px] text-rose-700 dark:text-rose-300">
              {entry.error}
            </div>
          )}

          <SettingCard>
            {isLoading && servers.length === 0 ? (
              <div className="text-muted-foreground flex items-center justify-center gap-2 px-4 py-8 text-[12.5px]">
                <Loader2 className="h-[14px] w-[14px] animate-spin" />
                <span>Scanning language servers…</span>
              </div>
            ) : servers.length === 0 ? (
              <div className="text-muted-foreground px-4 py-7 text-center text-[12.5px]">
                No language servers detected for this workspace.
              </div>
            ) : (
              <div className="divide-border divide-y">
                {servers.map((srv) => {
                  const canRestart = srv.state !== "unavailable";
                  const isRestarting = restartingId === srv.id;
                  return (
                    <div
                      key={srv.id}
                      className="flex items-center justify-between gap-3 px-4 py-2.5"
                    >
                      <div className="flex min-w-0 items-center gap-2.5">
                        <span
                          title={statusLabel(srv.state)}
                          className={cn(
                            "size-[7px] shrink-0 rounded-full",
                            statusDotClass(srv.state),
                          )}
                        />
                        <div className="min-w-0">
                          <div className="flex items-center gap-2">
                            <span className="text-foreground truncate text-[13px] font-medium">
                              {srv.name}
                            </span>
                            <span className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10.5px]">
                              {formatLanguages(srv)}
                            </span>
                            {srv.source === "project_json" && (
                              <span className="border-border text-muted-foreground shrink-0 rounded border px-1.5 py-0.5 font-mono text-[10px]">
                                .lsp.json
                              </span>
                            )}
                          </div>
                          <div className="text-muted-foreground mt-0.5 flex items-center gap-2 truncate text-[11.5px]">
                            <span>{statusLabel(srv.state)}</span>
                            {srv.diagnostics_count > 0 && (
                              <>
                                <span className="opacity-40">·</span>
                                <span className="tabular-nums">
                                  {srv.diagnostics_count} diagnostic
                                  {srv.diagnostics_count === 1 ? "" : "s"}
                                </span>
                              </>
                            )}
                            {srv.binary_path && (
                              <>
                                <span className="opacity-40">·</span>
                                <span
                                  title={srv.binary_path}
                                  className="truncate font-mono text-[10.5px] opacity-75"
                                >
                                  {srv.binary_path}
                                </span>
                              </>
                            )}
                            {srv.error && !srv.binary_path && (
                              <>
                                <span className="opacity-40">·</span>
                                <span className="truncate opacity-80">{srv.error}</span>
                              </>
                            )}
                          </div>
                        </div>
                      </div>

                      {canRestart && (
                        <button
                          type="button"
                          disabled={isRestarting}
                          onClick={() => void handleRestart(srv.id)}
                          title={`Restart ${srv.name}`}
                          className="border-border bg-background hover:bg-accent text-foreground inline-flex h-[26px] shrink-0 cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium transition-colors disabled:opacity-50"
                        >
                          {isRestarting ? (
                            <Loader2 className="h-[11.5px] w-[11.5px] animate-spin" />
                          ) : (
                            <RotateCw className="h-[11.5px] w-[11.5px]" />
                          )}
                          Restart
                        </button>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </SettingCard>

          <p className="text-muted-foreground/80 text-[11.5px] leading-relaxed">
            Override allowlisted servers per repository via{" "}
            <code className="bg-muted rounded px-1 py-0.5 font-mono text-[11px]">.lsp.json</code>{" "}
            or{" "}
            <code className="bg-muted rounded px-1 py-0.5 font-mono text-[11px]">
              .orbit/lsp.json
            </code>
            , or configure custom host binaries in{" "}
            <code className="bg-muted rounded px-1 py-0.5 font-mono text-[11px]">
              $ORBIT_HOME/lsp.json
            </code>
            .
          </p>
        </>
      )}
    </div>
  );
}
