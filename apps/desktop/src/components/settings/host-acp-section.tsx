import { useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  TerminalSquare,
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
  SettingGroup,
  SettingDrawer,
  SettingResourceRow,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/dialog-tabs";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  type AcpAgentAvailability,
  type AcpAgentSpec,
  type AcpProbeResult,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { useI18n } from "@/lib/i18n";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
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

function parseEnvLines(raw: string): { env: Record<string, string>; error: boolean } {
  const env: Record<string, string> = Object.create(null);
  for (const line of raw.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq <= 0) {
      return { env, error: true };
    }
    const key = trimmed.slice(0, eq).trim();
    const val = trimmed.slice(eq + 1);
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) {
      return { env, error: true };
    }
    env[key] = val;
  }
  return { env, error: false };
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
    return (
      parsed.protocol === "https:" && Boolean(parsed.hostname) && !parsed.username && !parsed.password
    );
  } catch {
    return false;
  }
}

function CustomAcpAgentDrawer({
  editing,
  onClose,
  onSave,
}: {
  editing: AcpAgentSpec | null;
  onClose: () => void;
  onSave: (spec: AcpAgentSpec) => Promise<void>;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(true);
  const [id, setId] = useState(editing?.id ?? "");
  const [name, setName] = useState(editing?.name ?? "");
  const [description, setDescription] = useState(editing?.description ?? "");
  const [command, setCommand] = useState(editing?.command ?? "");
  const [argsText, setArgsText] = useState(editing ? formatArgsLine(editing.args) : "");
  const [envText, setEnvText] = useState(editing ? formatEnvLines(editing.env) : "");
  const [installHint, setInstallHint] = useState(editing?.install_hint ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const cleanId = id.trim();
    const cleanName = name.trim() || cleanId;
    const cleanCmd = command.trim();

    if (!/^[a-z0-9][a-z0-9._-]{0,63}$/.test(cleanId)) {
      setError(t("settings.acpInvalidId"));
      return;
    }
    if (!cleanCmd) {
      setError(t("settings.acpMissingCommand"));
      return;
    }
    const parsedEnv =
      editing && envText === formatEnvLines(editing.env)
        ? { env: editing.env, error: false }
        : parseEnvLines(envText);
    if (parsedEnv.error) {
      setError(t("settings.acpInvalidEnvironment"));
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
        args:
          editing && argsText === formatArgsLine(editing.args) ? editing.args : parseArgsLine(argsText),
        env: parsedEnv.env,
        install_hint: installHint.trim() || null,
        homepage: editing?.homepage ?? null,
        version: editing?.version ?? null,
        source: "custom",
        enabled: editing?.enabled ?? true,
      });
      setOpen(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <SettingDrawer
      open={open}
      onOpenChange={(next) => {
        if (!saving) setOpen(next);
      }}
      onOpenChangeComplete={(visible) => {
        if (!visible) onClose();
      }}
      title={t(editing ? "settings.acpEdit" : "settings.acpAdd")}
      description={t("settings.acpDescription")}
    >
      <form onSubmit={(e) => void handleSubmit(e)}>
        <FieldGroup className="gap-3">
          <div className="grid grid-cols-2 gap-3">
            <Field>
              <FieldLabel htmlFor="acp-id" className="text-[11px]">
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
              <FieldLabel htmlFor="acp-name" className="text-[11px]">
                {t("settings.agentName")}
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
              <FieldLabel htmlFor="acp-command" className="text-[11px]">
                {t("settings.acpCommand")}
              </FieldLabel>
              <Input
                id="acp-command"
                type="text"
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                placeholder="/usr/local/bin/agent"
                className="font-mono"
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="acp-args" className="text-[11px]">
                {t("settings.acpArguments")}
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
            <FieldLabel htmlFor="acp-env" className="text-[11px]">
              {t("settings.acpEnvironment")}
            </FieldLabel>
            <Textarea
              id="acp-env"
              rows={3}
              value={envText}
              onChange={(e) => setEnvText(e.target.value)}
              placeholder={"API_BASE=https://api.example.com\nLOG_LEVEL=warn"}
              className="min-h-[72px] resize-none font-mono text-[13px]"
            />
          </Field>

          <Field>
            <FieldLabel htmlFor="acp-description" className="text-[11px]">
              {t("settings.acpDescLabel")}
            </FieldLabel>
            <Input
              id="acp-description"
              type="text"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t("settings.acpDescLabel")}
            />
          </Field>

          <Field>
            <FieldLabel htmlFor="acp-install-hint" className="text-[11px]">
              {t("settings.acpInstallHint")}
            </FieldLabel>
            <Input
              id="acp-install-hint"
              type="text"
              value={installHint}
              onChange={(e) => setInstallHint(e.target.value)}
              placeholder="brew install my-coder"
              className="font-mono text-[13px]"
            />
          </Field>

          {error && (
            <FieldError className="border-signal-danger-border bg-signal-danger-bg rounded-lg border px-3 py-2">
              {error}
            </FieldError>
          )}

          <div className="flex justify-end gap-2 pt-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={saving}
              onClick={() => setOpen(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" size="sm" disabled={saving}>
              {saving && <Loader2 className="h-[12.5px] w-[12.5px] animate-spin" />}
              <span>{t(editing ? "common.save" : "settings.acpAdd")}</span>
            </Button>
          </div>
        </FieldGroup>
      </form>
    </SettingDrawer>
  );
}

export function HostAcpSection({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId]);
  const selectedCwd = useOrbitSelector((s) =>
    s.selectedHostId === hostId ? s.selectedWorkspaceCwd : null,
  );

  const acpAgents = hostDetail?.acp_agents ?? EMPTY_ACP_AGENTS;

  const [filter, setFilter] = useState<AcpFilter>("all");
  const [query, setQuery] = useState("");
  const [detailsOpen, setDetailsOpen] = useState(false);
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
    void orbitIpc.refreshAcpAgents(hostId).catch((error) => {
      if (mountedRef.current) orbitActions.showToast(String(error));
    });
    return () => {
      mountedRef.current = false;
      if (copyTimerRef.current !== null) {
        window.clearTimeout(copyTimerRef.current);
        copyTimerRef.current = null;
      }
    };
  }, [hostId]);

  const installedCount = useMemo(() => acpAgents.filter((a) => a.installed).length, [acpAgents]);
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
      const summary = t("settings.acpRefreshResult", { ready, total: result.agents.length });
      orbitActions.showToast([summary, result.warning].filter(Boolean).join(" · "));
    } catch (err) {
      if (!mountedRef.current || activeHostRef.current !== reqHost || refreshSeqRef.current !== seq) {
        return;
      }
      orbitActions.showToast(err instanceof Error ? err.message : String(err));
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
  };

  const handleDeleteCustom = async (spec: AcpAgentSpec) => {
    await orbitIpc.deleteCustomAcpAgent(hostId, spec.id);
    if (!mountedRef.current) return;
    setProbeResults((previous) => {
      const next = { ...previous };
      delete next[spec.id];
      return next;
    });
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

  const selected = acpAgents.find((agent) => agent.spec.id === expandedId);
  const probe = expandedId ? probeResults[expandedId] : null;
  const deleting = acpAgents.find((agent) => agent.spec.id === confirmDeleteId);
  return (
    <SettingGroup
      title={t("settings.subTabAcpRegistry")}
      settingId="settings.subTabAcpRegistry"
      action={
        <div className="flex gap-2">
          <Button
            aria-label={t("common.refresh")}
            size="icon-sm"
            variant="ghost"
            disabled={refreshing}
            onClick={() => void handleRefreshRegistry()}
          >
            <RefreshCw className={cn(refreshing && "animate-spin")} />
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => setModalState({ open: true, editing: null })}
          >
            <Plus />
            {t("settings.acpAdd")}
          </Button>
        </div>
      }
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Tabs
          value={filter}
          onValueChange={(value) => {
            if (value === "all" || value === "custom" || value === "installed") setFilter(value);
          }}
        >
          <TabsList aria-label={t("settings.subTabAcpRegistry")}>
            <TabsTrigger value="installed">
              {t("settings.agentInstalled")} · {installedCount}
            </TabsTrigger>
            <TabsTrigger value="all">
              {t("settings.acpAll")} · {acpAgents.length}
            </TabsTrigger>
            <TabsTrigger value="custom">
              {t("settings.acpCustom")} · {customCount}
            </TabsTrigger>
          </TabsList>
        </Tabs>
        <div className="relative w-[180px]">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-[13px] -translate-y-1/2 text-ink-muted" />
          <Input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            aria-label={t("settings.acpFilter")}
            placeholder={t("settings.acpFilter")}
            className="h-[30px] pl-8"
          />
        </div>
      </div>
      <SettingCard>
        {!filteredAgents.length && (
          <p className="px-4 py-3 text-[13px] text-ink-secondary">{t("settings.acpEmpty")}</p>
        )}
        {filteredAgents.map(({ spec, installed }) => (
          <SettingResourceRow
            key={spec.id}
            title={spec.name}
            icon={<TerminalSquare className="size-[15px]" />}
            description={[spec.command, ...spec.args].join(" ")}
            status={t(installed ? "settings.agentInstalled" : "settings.agentNotInstalled")}
            onClick={() => {
              setExpandedId(spec.id);
              setDetailsOpen(true);
            }}
            action={
              spec.source === "custom" && (
                <>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={t("settings.acpEditName", { name: spec.name })}
                    onClick={() => setModalState({ open: true, editing: spec })}
                  >
                    <Pencil />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={t("settings.acpDeleteName", { name: spec.name })}
                    onClick={() => setConfirmDeleteId(spec.id)}
                  >
                    <Trash2 />
                  </Button>
                </>
              )
            }
          />
        ))}
      </SettingCard>
      {selected && (
        <SettingDrawer
          open={detailsOpen}
          onOpenChange={setDetailsOpen}
          onOpenChangeComplete={(visible) => {
            if (!visible) setExpandedId(null);
          }}
          title={selected.spec.name}
          description={selected.spec.description}
        >
          <div className="grid gap-4 break-words text-[13px] text-ink-secondary">
            <code className="break-all rounded-lg bg-surface-recessed p-3">
              {[selected.resolved_path || selected.spec.command, ...selected.spec.args].join(" ")}
            </code>
            <div className="flex items-center justify-between gap-2">
              <span>{selected.spec.id}</span>
              {isHttpsUrl(selected.spec.homepage) && (
                <Button
                  size="sm"
                  variant="link"
                  onClick={() => {
                    if (isHttpsUrl(selected.spec.homepage))
                      void orbitIpc
                        .openExternalBrowserUrl(selected.spec.homepage)
                        .catch((error) => orbitActions.showToast(String(error)));
                  }}
                >
                  <ExternalLink />
                  {t("settings.acpDocs")}
                </Button>
              )}
            </div>
            {selected.installed && (
              <Button
                disabled={!!probingIds[selected.spec.id]}
                variant="outline"
                onClick={() => void handleProbe(selected.spec.id)}
              >
                {probingIds[selected.spec.id] ? <Loader2 className="animate-spin" /> : <Play />}
                {t("settings.acpTest")}
              </Button>
            )}
            {probe && (
              <div role="status" className="space-y-2 rounded-lg bg-surface-recessed p-3">
                {probe.ok ? (
                  <>
                    <div className="text-signal-success">
                      {[
                        probe.server_name ?? selected.spec.name,
                        probe.server_version,
                        probe.protocol_version && `ACP v${probe.protocol_version}`,
                        `${probe.latency_ms}ms`,
                      ]
                        .filter(Boolean)
                        .join(" · ")}
                    </div>
                    {!!probe.capabilities_summary.length && (
                      <p>{probe.capabilities_summary.join(", ")}</p>
                    )}
                    {!!probe.auth_methods.length && <p>{probe.auth_methods.join(", ")}</p>}
                  </>
                ) : (
                  <span className="text-signal-danger">
                    {probe.error ?? t("settings.acpTestFailed")}
                  </span>
                )}
              </div>
            )}
            {!selected.installed && selected.spec.install_hint && (
              <div className="flex min-w-0 items-center gap-2 rounded-lg bg-surface-recessed p-3">
                <code className="flex-1 break-all">{selected.spec.install_hint}</code>
                <Button
                  aria-label={t("common.copy")}
                  size="icon-sm"
                  variant="ghost"
                  onClick={() => {
                    if (selected.spec.install_hint)
                      void handleCopyInstall(selected.spec.id, selected.spec.install_hint);
                  }}
                >
                  {copiedHintId === selected.spec.id ? <Check /> : <Copy />}
                </Button>
              </div>
            )}
          </div>
        </SettingDrawer>
      )}
      {modalState.open && (
        <CustomAcpAgentDrawer
          editing={modalState.editing}
          onClose={() => setModalState({ open: false, editing: null })}
          onSave={handleSaveCustom}
        />
      )}
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(open) => {
          if (!open) setConfirmDeleteId(null);
        }}
        title={t("settings.acpDeleteName", { name: deleting?.spec.name ?? "" })}
        tone="danger"
        confirmLabel={t("common.delete")}
        onConfirm={async () => {
          if (deleting) await handleDeleteCustom(deleting.spec);
        }}
      />
    </SettingGroup>
  );
}
