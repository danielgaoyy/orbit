import { useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  ExternalLink,
  Loader2,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react";
import {
  SettingCard,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Tabs,
  TabsList,
  TabsTrigger,
} from "@/components/ui/dialog-tabs";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  type AcpAgentAvailability,
  type AcpAgentSpec,
  type AcpProbeResult,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type AcpFilter = "installed" | "all" | "custom";

const EMPTY_ACP_AGENTS: readonly AcpAgentAvailability[] = [];

function parseArgsLine(raw: string): string[] {
  const out: string[] = [];
  let cur = "";
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < raw.length; i += 1) {
    const ch = raw[i];
    if (quote) {
      if (ch === quote) {
        quote = null;
      } else if (ch === "\\" && quote === '"' && i + 1 < raw.length) {
        cur += raw[i + 1];
        i += 1;
      } else {
        cur += ch;
      }
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    if (/\s/.test(ch)) {
      if (cur.length > 0) {
        out.push(cur);
        cur = "";
      }
      continue;
    }
    cur += ch;
  }
  if (cur.length > 0) out.push(cur);
  return out;
}

function formatArgsLine(args: string[]): string {
  return args
    .map((a) => (/[\s"'\\]/.test(a) ? `"${a.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"` : a))
    .join(" ");
}

function parseEnvLines(raw: string): { env: Record<string, string>; error: string | null } {
  const env: Record<string, string> = {};
  for (const line of raw.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq <= 0) {
      return { env, error: `Invalid env line "${trimmed}" (expected KEY=VALUE)` };
    }
    const key = trimmed.slice(0, eq).trim();
    const val = trimmed.slice(eq + 1);
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) {
      return { env, error: `Invalid env variable name "${key}"` };
    }
    env[key] = val;
  }
  return { env, error: null };
}

function formatEnvLines(env: Record<string, string>): string {
  return Object.entries(env)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
}

function isHttpsUrl(raw: string | null | undefined): raw is string {
  if (!raw) return false;
  const trimmed = raw.trim();
  if (!trimmed || /[\s\u0000-\u001f\u007f]/.test(trimmed)) return false;
  try {
    const parsed = new URL(trimmed);
    return parsed.protocol === "https:" && Boolean(parsed.hostname) && !parsed.username && !parsed.password;
  } catch {
    return false;
  }
}

function CustomAcpAgentModal({
  editing,
  onClose,
  onSave,
}: {
  editing: AcpAgentSpec | null;
  onClose: () => void;
  onSave: (spec: AcpAgentSpec) => Promise<void>;
}) {
  const [id, setId] = useState(editing?.id ?? "");
  const [name, setName] = useState(editing?.name ?? "");
  const [description, setDescription] = useState(editing?.description ?? "");
  const [command, setCommand] = useState(editing?.command ?? "");
  const [argsText, setArgsText] = useState(editing ? formatArgsLine(editing.args) : "");
  const [envText, setEnvText] = useState(editing ? formatEnvLines(editing.env) : "");
  const [installHint, setInstallHint] = useState(editing?.install_hint ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !saving) {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, saving]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const cleanId = id.trim();
    const cleanName = name.trim() || cleanId;
    const cleanCmd = command.trim();

    if (!/^[a-z0-9][a-z0-9._-]{0,63}$/.test(cleanId)) {
      setError("ID must match [a-z0-9][a-z0-9._-]{0,63} (lowercase letters, digits, ., _, -).");
      return;
    }
    if (!cleanCmd) {
      setError("Command is required.");
      return;
    }
    const parsedEnv = parseEnvLines(envText);
    if (parsedEnv.error) {
      setError(parsedEnv.error);
      return;
    }

    setError(null);
    setSaving(true);
    try {
      await onSave({
        id: cleanId,
        name: cleanName,
        description: description.trim(),
        command: cleanCmd,
        args: parseArgsLine(argsText),
        env: parsedEnv.env,
        install_hint: installHint.trim() || null,
        homepage: editing?.homepage ?? null,
        version: editing?.version ?? null,
        source: "custom",
        enabled: editing?.enabled ?? true,
      });
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !saving) onClose();
      }}
    >
      <DialogContent closeDisabled={saving} className="max-w-[460px] gap-4 p-5">
        <DialogHeader className="pr-8">
          <DialogTitle>
            {editing ? "Edit Custom ACP Agent" : "Add Custom ACP Agent"}
          </DialogTitle>
          <DialogDescription>
            Configure any stdio JSON-RPC 2.0 ACP executable on this host.
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={(e) => void handleSubmit(e)}>
          <FieldGroup className="gap-3">
            <div className="grid grid-cols-2 gap-3">
              <Field>
                <FieldLabel htmlFor="acp-id" className="text-[11.5px]">
                  ID
                </FieldLabel>
                <Input
                  id="acp-id"
                  type="text"
                  autoFocus={!editing}
                  value={id}
                  disabled={Boolean(editing)}
                  onChange={(e) => setId(e.target.value)}
                  placeholder="my-coder"
                  className="font-mono"
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="acp-name" className="text-[11.5px]">
                  Name
                </FieldLabel>
                <Input
                  id="acp-name"
                  type="text"
                  autoFocus={Boolean(editing)}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="My Coder"
                />
              </Field>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <Field>
                <FieldLabel htmlFor="acp-command" className="text-[11.5px]">
                  Command
                </FieldLabel>
                <Input
                  id="acp-command"
                  type="text"
                  value={command}
                  onChange={(e) => setCommand(e.target.value)}
                  placeholder="opencode or /usr/local/bin/agent"
                  className="font-mono"
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="acp-args" className="text-[11.5px]">
                  Arguments
                </FieldLabel>
                <Input
                  id="acp-args"
                  type="text"
                  value={argsText}
                  onChange={(e) => setArgsText(e.target.value)}
                  placeholder="acp --stdio"
                  className="font-mono"
                />
              </Field>
            </div>

            <Field>
              <FieldLabel htmlFor="acp-env" className="text-[11.5px]">
                Environment (optional, KEY=VALUE per line)
              </FieldLabel>
              <Textarea
                id="acp-env"
                rows={3}
                value={envText}
                onChange={(e) => setEnvText(e.target.value)}
                placeholder={"API_BASE=https://api.example.com\nLOG_LEVEL=warn"}
                className="min-h-[72px] resize-none font-mono text-[12px]"
              />
            </Field>

            <Field>
              <FieldLabel htmlFor="acp-description" className="text-[11.5px]">
                Description (optional)
              </FieldLabel>
              <Input
                id="acp-description"
                type="text"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Concise summary of this ACP agent"
              />
            </Field>

            <Field>
              <FieldLabel htmlFor="acp-install-hint" className="text-[11.5px]">
                Install hint (optional)
              </FieldLabel>
              <Input
                id="acp-install-hint"
                type="text"
                value={installHint}
                onChange={(e) => setInstallHint(e.target.value)}
                placeholder="brew install my-coder"
                className="font-mono text-[12px]"
              />
            </Field>

            {error && (
              <FieldError className="border-signal-danger-border bg-signal-danger-bg rounded-lg border px-3 py-2">
                {error}
              </FieldError>
            )}

            <DialogFooter className="pt-2">
              <Button type="button" variant="outline" size="sm" disabled={saving} onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" size="sm" disabled={saving}>
                {saving && <Loader2 className="h-[12.5px] w-[12.5px] animate-spin" />}
                <span>{editing ? "Save Changes" : "Add Agent"}</span>
              </Button>
            </DialogFooter>
          </FieldGroup>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function HostAcpSection({ hostId }: { hostId: string }) {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostCard = fleet.hosts.find((h) => h.host_id === hostId) ?? null;
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);

  const acpAgents = hostDetail?.acp_agents ?? EMPTY_ACP_AGENTS;

  const [filter, setFilter] = useState<AcpFilter>("all");
  const [query, setQuery] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [probingIds, setProbingIds] = useState<Record<string, boolean>>({});
  const [probeResults, setProbeResults] = useState<Record<string, AcpProbeResult>>({});
  const [copiedHintId, setCopiedHintId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [modalState, setModalState] = useState<{ open: boolean; editing: AcpAgentSpec | null }>({
    open: false,
    editing: null,
  });

  const activeHostRef = useRef(hostId);
  const mountedRef = useRef(true);
  const refreshSeqRef = useRef(0);
  const probeSeqRef = useRef<Record<string, number>>({});
  const copyTimerRef = useRef<number | null>(null);

  useEffect(() => {
    mountedRef.current = true;
    activeHostRef.current = hostId;
    void orbitIpc.refreshAcpAgents(hostId);
    return () => {
      mountedRef.current = false;
      if (copyTimerRef.current !== null) {
        window.clearTimeout(copyTimerRef.current);
        copyTimerRef.current = null;
      }
    };
  }, [hostId]);

  const showHostBadge =
    Boolean(hostCard) &&
    (fleet.hosts.length > 1 || (hostCard !== null && !isLocalHostCard(hostCard)));

  const installedCount = useMemo(
    () => acpAgents.filter((a) => a.installed).length,
    [acpAgents],
  );
  const customCount = useMemo(
    () => acpAgents.filter((a) => a.spec.source === "custom").length,
    [acpAgents],
  );

  // Default to "Installed" when at least one ACP agent is installed on initial load
  const [didInitFilter, setDidInitFilter] = useState(false);
  useEffect(() => {
    if (!didInitFilter && acpAgents.length > 0) {
      setDidInitFilter(true);
      if (installedCount > 0) {
        setFilter("installed");
      }
    }
  }, [didInitFilter, acpAgents.length, installedCount]);

  const filteredAgents = useMemo(() => {
    const q = query.trim().toLowerCase();
    return acpAgents.filter((item) => {
      if (filter === "installed" && !item.installed) return false;
      if (filter === "custom" && item.spec.source !== "custom") return false;
      if (!q) return true;
      return (
        item.spec.name.toLowerCase().includes(q) ||
        item.spec.id.toLowerCase().includes(q) ||
        item.spec.command.toLowerCase().includes(q) ||
        item.spec.description.toLowerCase().includes(q)
      );
    });
  }, [acpAgents, filter, query]);

  const handleRefreshRegistry = async () => {
    const reqHost = hostId;
    const seq = ++refreshSeqRef.current;
    setRefreshing(true);
    try {
      const result = await orbitIpc.refreshAcpRegistry(reqHost);
      if (!mountedRef.current || activeHostRef.current !== reqHost || refreshSeqRef.current !== seq) {
        return;
      }
      const ready = result.agents.filter((a) => a.installed).length;
      if (result.warning) {
        orbitActions.showToast(
          `ACP registry (${ready}/${result.agents.length} ready): ${result.warning}`,
        );
      } else {
        orbitActions.showToast(`ACP registry refreshed (${ready}/${result.agents.length} ready)`);
      }
    } catch (err) {
      if (!mountedRef.current || activeHostRef.current !== reqHost || refreshSeqRef.current !== seq) {
        return;
      }
      orbitActions.showToast(
        err instanceof Error ? err.message : "Failed to refresh ACP registry",
      );
    } finally {
      if (mountedRef.current && activeHostRef.current === reqHost && refreshSeqRef.current === seq) {
        setRefreshing(false);
      }
    }
  };

  const handleProbe = async (agentId: string) => {
    const reqHost = hostId;
    const seq = (probeSeqRef.current[agentId] ?? 0) + 1;
    probeSeqRef.current[agentId] = seq;
    setProbingIds((prev) => ({ ...prev, [agentId]: true }));
    try {
      const res = await orbitIpc.testAcpAgent(reqHost, agentId, selectedCwd || null);
      if (
        !mountedRef.current ||
        activeHostRef.current !== reqHost ||
        probeSeqRef.current[agentId] !== seq
      ) {
        return;
      }
      setProbeResults((prev) => ({ ...prev, [agentId]: res }));
    } catch (err) {
      if (
        !mountedRef.current ||
        activeHostRef.current !== reqHost ||
        probeSeqRef.current[agentId] !== seq
      ) {
        return;
      }
      setProbeResults((prev) => ({
        ...prev,
        [agentId]: {
          agent_id: agentId,
          ok: false,
          capabilities_summary: [],
          auth_methods: [],
          latency_ms: 0,
          error: err instanceof Error ? err.message : String(err),
        },
      }));
    } finally {
      if (
        mountedRef.current &&
        activeHostRef.current === reqHost &&
        probeSeqRef.current[agentId] === seq
      ) {
        setProbingIds((prev) => {
          const next = { ...prev };
          delete next[agentId];
          return next;
        });
      }
    }
  };

  const handleSaveCustom = async (spec: AcpAgentSpec) => {
    const reqHost = hostId;
    await orbitIpc.upsertCustomAcpAgent(reqHost, spec);
    if (mountedRef.current && activeHostRef.current === reqHost) {
      orbitActions.showToast(`Saved ACP agent "${spec.name}"`);
    }
  };

  const handleDeleteCustom = async (spec: AcpAgentSpec) => {
    const reqHost = hostId;
    setConfirmDeleteId(null);
    try {
      await orbitIpc.deleteCustomAcpAgent(reqHost, spec.id);
      if (!mountedRef.current || activeHostRef.current !== reqHost) return;
      setProbeResults((prev) => {
        const next = { ...prev };
        delete next[spec.id];
        return next;
      });
      orbitActions.showToast(`Removed custom ACP agent "${spec.name}"`);
    } catch (err) {
      if (!mountedRef.current || activeHostRef.current !== reqHost) return;
      orbitActions.showToast(
        err instanceof Error ? err.message : "Failed to delete custom ACP agent",
      );
    }
  };

  const handleCopyInstall = async (agentId: string, hint: string) => {
    try {
      await navigator.clipboard.writeText(hint);
      if (!mountedRef.current) return;
      setCopiedHintId(agentId);
      if (copyTimerRef.current !== null) {
        window.clearTimeout(copyTimerRef.current);
      }
      copyTimerRef.current = window.setTimeout(() => {
        copyTimerRef.current = null;
        if (mountedRef.current) {
          setCopiedHintId((cur) => (cur === agentId ? null : cur));
        }
      }, 1400);
    } catch {
      // ignore clipboard errors
    }
  };

  return (
    <div className="space-y-5">
      <SettingSectionHeader
        title="ACP Agents"
        description="Universal Agent Client Protocol (ACP) coding agents available on this host."
        badge={
          showHostBadge && hostCard ? (
            <span className="bg-muted text-muted-foreground rounded-md px-2 py-0.5 text-[11.5px] font-medium">
              {hostCard.display_name}
            </span>
          ) : null
        }
        action={
          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={refreshing}
              onClick={() => void handleRefreshRegistry()}
              className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
            >
              <RefreshCw className={cn("h-[13px] w-[13px]", refreshing && "animate-spin")} />
              <span>Refresh registry</span>
            </button>
            <button
              type="button"
              onClick={() => setModalState({ open: true, editing: null })}
              className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-2.5 text-[12px] font-medium transition-opacity hover:opacity-90"
            >
              <Plus className="h-[13.5px] w-[13.5px]" />
              <span>Add custom</span>
            </button>
          </div>
        }
      />

      {/* Compact filter + search bar */}
      <div className="flex flex-col gap-2.5 sm:flex-row sm:items-center sm:justify-between">
        <Tabs
          value={filter}
          onValueChange={(next) => {
            if (typeof next === "string") setFilter(next as AcpFilter);
          }}
        >
          <TabsList aria-label="Filter ACP agent category">
            {(
              [
                { id: "installed", label: "Installed", count: installedCount },
                { id: "all", label: "All Catalog", count: acpAgents.length },
                { id: "custom", label: "Custom", count: customCount },
              ] as const
            ).map((tab) => (
              <TabsTrigger key={tab.id} value={tab.id}>
                <span>{tab.label}</span>
                <span className="text-ink-faint font-mono text-[11px] font-normal tabular-nums">
                  {tab.count}
                </span>
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>

        <div className="relative w-full sm:w-[220px]">
          <Search className="text-ink-faint pointer-events-none absolute top-1/2 left-2.5 h-[13.5px] w-[13.5px] -translate-y-1/2" />
          <Input
            type="text"
            aria-label="Filter ACP agents"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter agents…"
            className="h-[30px] pl-8"
          />
        </div>
      </div>

      {/* Agent List */}
      {filteredAgents.length === 0 ? (
        <SettingCard>
          <div className="text-muted-foreground px-4 py-8 text-center text-[12.5px]">
            {filter === "installed" && acpAgents.length > 0
              ? "No ACP agents detected on PATH yet. Switch to All Catalog to browse install hints, or add a custom agent."
              : filter === "custom"
                ? "No custom ACP agents configured on this host."
                : "No ACP agents match your filter."}
          </div>
        </SettingCard>
      ) : (
        <SettingCard>
          {filteredAgents.map((item) => {
            const { spec, installed, resolved_path } = item;
            const expanded = expandedId === spec.id;
            const probe = probeResults[spec.id] ?? null;
            const isProbing = Boolean(probingIds[spec.id]);
            const isCustom = spec.source === "custom";
            const confirmingDelete = confirmDeleteId === spec.id;
            const cmdLine =
              spec.args.length > 0 ? `${spec.command} ${spec.args.join(" ")}` : spec.command;

            return (
              <div key={spec.id} className="px-4 py-3">
                <div className="flex items-center justify-between gap-3">
                  <button
                    type="button"
                    aria-expanded={expanded}
                    onClick={() => setExpandedId(expanded ? null : spec.id)}
                    className="flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 text-left"
                  >
                    {expanded ? (
                      <ChevronDown className="text-muted-foreground/70 h-[13.5px] w-[13.5px] shrink-0" />
                    ) : (
                      <ChevronRight className="text-muted-foreground/70 h-[13.5px] w-[13.5px] shrink-0" />
                    )}
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="text-foreground truncate text-[13.5px] font-medium">
                          {spec.name}
                        </span>
                        {isCustom && (
                          <span className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10.5px] font-medium">
                            Custom
                          </span>
                        )}
                        <span className="text-muted-foreground/70 hidden truncate font-mono text-[11.5px] sm:inline">
                          {cmdLine}
                        </span>
                      </div>

                      {probe && (
                        <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11.5px]">
                          {probe.ok ? (
                            <span className="text-emerald-600 dark:text-emerald-400">
                              ✓ {probe.server_name ?? spec.name}
                              {probe.server_version ? ` ${probe.server_version}` : ""}
                              {probe.protocol_version ? ` · ACP v${probe.protocol_version}` : ""}
                              {` · ${probe.latency_ms}ms`}
                            </span>
                          ) : (
                            <span className="text-red-600 dark:text-red-400">
                              {probe.error ?? "Handshake failed"}
                            </span>
                          )}
                        </div>
                      )}
                    </div>
                  </button>

                  <div className="flex shrink-0 items-center gap-1.5">
                    {installed && (
                      <button
                        type="button"
                        disabled={isProbing}
                        onClick={() => void handleProbe(spec.id)}
                        className="border-border hover:bg-accent text-foreground/85 inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium transition-colors disabled:opacity-50"
                      >
                        {isProbing ? (
                          <Loader2 className="h-[11.5px] w-[11.5px] animate-spin" />
                        ) : (
                          <Play className="h-[10.5px] w-[10.5px]" />
                        )}
                        <span>Test</span>
                      </button>
                    )}

                    {isCustom && (
                      <>
                        {confirmingDelete ? (
                          <div className="flex items-center gap-1">
                            <button
                              type="button"
                              onClick={() => void handleDeleteCustom(spec)}
                              className="inline-flex h-[24px] cursor-pointer items-center rounded-md border border-red-500/35 bg-red-500/10 px-2 text-[11px] font-medium text-red-600 transition-colors hover:bg-red-500/20 dark:text-red-400"
                            >
                              Remove
                            </button>
                            <button
                              type="button"
                              onClick={() => setConfirmDeleteId(null)}
                              className="border-border text-muted-foreground hover:text-foreground inline-flex h-[24px] cursor-pointer items-center rounded-md border px-2 text-[11px] transition-colors"
                            >
                              Cancel
                            </button>
                          </div>
                        ) : (
                          <>
                            <button
                              type="button"
                              aria-label={`Edit ${spec.name}`}
                              onClick={() => setModalState({ open: true, editing: spec })}
                              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
                            >
                              <Pencil className="h-[12px] w-[12px]" />
                            </button>
                            <button
                              type="button"
                              aria-label={`Delete ${spec.name}`}
                              onClick={() => setConfirmDeleteId(spec.id)}
                              className="text-muted-foreground hover:text-red-500 hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
                            >
                              <Trash2 className="h-[12px] w-[12px]" />
                            </button>
                          </>
                        )}
                      </>
                    )}

                    <span
                      className={cn(
                        "inline-flex h-[24px] items-center gap-1.5 rounded-md border px-2 text-[11.5px] font-medium",
                        installed
                          ? "border-emerald-500/25 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                          : "border-border text-muted-foreground",
                      )}
                    >
                      <span
                        className={cn(
                          "h-[5.5px] w-[5.5px] rounded-full",
                          installed ? "bg-emerald-500" : "bg-zinc-400",
                        )}
                      />
                      {installed ? "Ready" : "Not installed"}
                    </span>
                  </div>
                </div>

                {expanded && (
                  <div className="border-border/60 mt-2.5 space-y-2 border-t pt-2.5 pl-6 text-[12px]">
                    {spec.description && (
                      <p className="text-muted-foreground leading-relaxed">{spec.description}</p>
                    )}

                    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[11.5px]">
                      <span className="text-muted-foreground">
                        ID: <code className="text-foreground/85 font-mono">{spec.id}</code>
                      </span>
                      {resolved_path && (
                        <span className="text-muted-foreground">
                          Binary:{" "}
                          <code className="text-foreground/85 font-mono">{resolved_path}</code>
                        </span>
                      )}
                      {isHttpsUrl(spec.homepage) && (
                        <button
                          type="button"
                          onClick={() => {
                            const url = spec.homepage;
                            if (!isHttpsUrl(url)) return;
                            void orbitIpc.openExternalBrowserUrl(url).catch((err) => {
                              orbitActions.showToast(
                                err instanceof Error ? err.message : "Failed to open documentation link",
                              );
                            });
                          }}
                          className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 underline-offset-2 hover:underline"
                        >
                          <span>Docs</span>
                          <ExternalLink className="h-[11px] w-[11px]" />
                        </button>
                      )}
                    </div>

                    {!installed && spec.install_hint && (
                      <div className="border-border/80 bg-muted/40 flex items-center justify-between gap-2 rounded-lg border px-2.5 py-1.5">
                        <code className="text-foreground/85 truncate font-mono text-[11.5px]">
                          {spec.install_hint}
                        </code>
                        <button
                          type="button"
                          onClick={() => void handleCopyInstall(spec.id, spec.install_hint!)}
                          className="text-muted-foreground hover:text-foreground inline-flex shrink-0 cursor-pointer items-center gap-1 text-[11px]"
                        >
                          {copiedHintId === spec.id ? (
                            <>
                              <Check className="h-[12px] w-[12px] text-emerald-500" />
                              <span>Copied</span>
                            </>
                          ) : (
                            <>
                              <Copy className="h-[12px] w-[12px]" />
                              <span>Copy</span>
                            </>
                          )}
                        </button>
                      </div>
                    )}

                    {probe?.ok &&
                      (probe.capabilities_summary.length > 0 || probe.auth_methods.length > 0) && (
                        <div className="text-muted-foreground flex flex-wrap gap-x-4 gap-y-1 text-[11.5px]">
                          {probe.capabilities_summary.length > 0 && (
                            <span>Capabilities: {probe.capabilities_summary.join(", ")}</span>
                          )}
                          {probe.auth_methods.length > 0 && (
                            <span>Auth: {probe.auth_methods.join(", ")}</span>
                          )}
                        </div>
                      )}
                  </div>
                )}
              </div>
            );
          })}
        </SettingCard>
      )}

      {modalState.open && (
        <CustomAcpAgentModal
          editing={modalState.editing}
          onClose={() => setModalState({ open: false, editing: null })}
          onSave={handleSaveCustom}
        />
      )}
    </div>
  );
}
