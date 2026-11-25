import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Download,
  Globe,
  KeyRound,
  Loader2,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Terminal,
  Trash2,
  Wrench,
  X,
} from "lucide-react";
import {
  SettingCard,
  SettingDrawer,
  SettingGroupLabel,
  SettingResourceRow,
  SettingSelect,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  type ExternalMcpServerRecord,
  type McpProbeResult,
  type McpServerScope,
  type McpWriteScope,
  type McpTransportKind,
  type UpsertExternalMcpServerRequest,
} from "@/lib/orbit-ipc";
import { t } from "@/lib/i18n";
import { isAlreadyExistsError, suggestAvailableName } from "@/lib/name-conflict";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface KvDraftRow {
  id: string;
  key: string;
  value: string;
  existingMasked?: string | null;
}

const EMPTY_SERVERS: readonly ExternalMcpServerRecord[] = [];
const SECRET_KEY_RE = /(TOKEN|SECRET|KEY|PASS|AUTH|CREDENTIAL|COOKIE)/i;
const BLOCKED_ENV_RE =
  /^(LD_PRELOAD|LD_|DYLD_|ORBIT_|NODE_OPTIONS$|PYTHONSTARTUP$|PYTHONPATH$|PYTHONHOME$|BASH_ENV$|ENV$|IFS$|PROMPT_COMMAND$|GIT_SSH_COMMAND$|GIT_EXEC_PATH$|PERL5OPT$|RUBYOPT$|JAVA_TOOL_OPTIONS$|_JAVA_OPTIONS$|GCONV_PATH$|GLIBC_TUNABLES$)/i;

function isSecretKey(key: string): boolean {
  return SECRET_KEY_RE.test(key.trim());
}

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
    .map((a) =>
      /[\s"'\\]/.test(a)
        ? `"${a.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`
        : a,
    )
    .join(" ");
}

function scopeBadgeLabel(scope: McpServerScope): string {
  switch (scope) {
    case "orbit_user":
      return t("devTools.global");
    case "project_dot_mcp":
      return t("devTools.project");
    case "claude_user":
      return "Claude";
    case "codex_user":
      return "Codex";
    case "antigravity_user":
      return "Antigravity";
  }
}

function makeRowId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
}

function initKvRows(
  maskedMap: Record<string, string> | undefined,
): KvDraftRow[] {
  if (!maskedMap) return [];
  return Object.entries(maskedMap)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, val]) => {
      const secret = isSecretKey(key);
      return {
        id: makeRowId(),
        key,
        value: secret ? "" : val,
        existingMasked: secret ? val : null,
      };
    });
}

function KvEditorRows({
  label,
  addLabel,
  keyPlaceholder,
  valuePlaceholder,
  rows,
  onChange,
}: {
  label: string;
  addLabel: string;
  keyPlaceholder: string;
  valuePlaceholder: string;
  rows: KvDraftRow[];
  onChange: (next: KvDraftRow[]) => void;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-ink-muted text-[11px] font-medium">{label}</span>
        <button
          type="button"
          onClick={() =>
            onChange([...rows, { id: makeRowId(), key: "", value: "" }])
          }
          className="text-ink-muted hover:text-ink-primary inline-flex cursor-pointer items-center gap-1 text-[11px] font-medium transition-colors"
        >
          <Plus className="h-[12px] w-[12px]" />
          <span>{addLabel}</span>
        </button>
      </div>

      {rows.length === 0 ? (
        <div className="border-border-subtle/70 text-ink-muted/75 rounded-lg border border-dashed px-3 py-2 text-[11px]">
          {t("devTools.none")}
        </div>
      ) : (
        <div className="space-y-2">
          {rows.map((row) => {
            const secret = isSecretKey(row.key);
            const placeholder =
              row.existingMasked && secret
                ? t("devTools.blankToKeep", { value: row.existingMasked })
                : valuePlaceholder;
            return (
              <div key={row.id} className="flex items-center gap-2">
                <Input
                  type="text"
                  aria-label={keyPlaceholder}
                  value={row.key}
                  onChange={(e) =>
                    onChange(
                      rows.map((r) =>
                        r.id === row.id
                          ? { ...r, key: e.target.value, existingMasked: null }
                          : r,
                      ),
                    )
                  }
                  placeholder={keyPlaceholder}
                  className="h-[30px] w-[42%] font-mono text-[13px]"
                />
                <div className="relative flex-1">
                  <Input
                    type={secret ? "password" : "text"}
                    aria-label={valuePlaceholder}
                    value={row.value}
                    onChange={(e) =>
                      onChange(
                        rows.map((r) =>
                          r.id === row.id ? { ...r, value: e.target.value } : r,
                        ),
                      )
                    }
                    placeholder={placeholder}
                    className="h-[30px] font-mono text-[13px]"
                  />
                </div>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label={t("common.delete")}
                  onClick={() => onChange(rows.filter((r) => r.id !== row.id))}
                  className="h-[28px] w-[28px] shrink-0"
                >
                  <X className="h-[13px] w-[13px]" />
                </Button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function McpServerDrawer({
  open,
  onClosed,
  hostId,
  importProjects,
  workspaceCwd,
  editing,
  importingFrom,
  existingServers,
  onOpenExisting,
  onClose,
  onSave,
}: {
  open: boolean;
  onClosed: () => void;
  importProjects: readonly {
    value: string;
    label: string;
    description: string;
  }[];
  hostId: string;
  workspaceCwd: string | null;
  editing: ExternalMcpServerRecord | null;
  importingFrom: ExternalMcpServerRecord | null;
  /** Servers already listed for `workspaceCwd`, used for the instant create-only precheck. */
  existingServers: readonly ExternalMcpServerRecord[];
  onOpenExisting: (server: ExternalMcpServerRecord) => void;
  onClose: () => void;
  onSave: (req: UpsertExternalMcpServerRequest) => Promise<void>;
}) {
  const seed = editing ?? importingFrom;
  const isEdit = Boolean(editing);

  const [destination, setDestination] = useState(workspaceCwd ?? "");
  const targetCwd = importingFrom ? destination || null : workspaceCwd;
  const scope: McpWriteScope = targetCwd ? "project" : "user";
  const [transportKind, setTransportKind] = useState<
    "stdio" | "streamable_http"
  >(() =>
    seed?.transport.kind === "streamable_http" ? "streamable_http" : "stdio",
  );
  const [name, setName] = useState(() => seed?.name ?? "");
  const [command, setCommand] = useState(() =>
    seed?.transport.kind === "stdio" ? seed.transport.command : "",
  );
  const [argsText, setArgsText] = useState(() =>
    seed?.transport.kind === "stdio" ? formatArgsLine(seed.transport.args) : "",
  );
  const [url, setUrl] = useState(() =>
    seed?.transport.kind === "streamable_http" ? seed.transport.url : "",
  );
  const [envRows, setEnvRows] = useState<KvDraftRow[]>(() =>
    initKvRows(seed?.masked_env),
  );
  const [headerRows, setHeaderRows] = useState<KvDraftRow[]>(() =>
    initKvRows(seed?.masked_headers),
  );
  const [enabled, setEnabled] = useState(() => seed?.enabled ?? true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Create-only conflict for "add" and "import from": the name already exists in the target scope.
  const [conflict, setConflict] = useState<{ name: string; suggestion: string } | null>(null);
  const [replaceOpen, setReplaceOpen] = useState(false);
  const nameInputRef = useRef<HTMLInputElement | null>(null);
  const targetScopeId = scope === "project" ? "project_dot_mcp" : "orbit_user";
  // The listed project servers belong to `workspaceCwd`; an import into another project can
  // only be checked by the daemon's create-only guard.
  const precheckable = scope === "user" || targetCwd === workspaceCwd;
  const takenNames = useMemo(
    () => existingServers.filter((sv) => sv.scope === targetScopeId).map((sv) => sv.name),
    [existingServers, targetScopeId],
  );
  const canSubmit =
    Boolean(name.trim()) &&
    (transportKind === "stdio" ? Boolean(command.trim()) : Boolean(url.trim()));

  /**
   * Save the server. New servers ("add", "import from") are create-only; `replace` is only true
   * after the user confirms the danger dialog. MCP config entries carry no version/etag, so a
   * confirmed replace is an explicit upsert of that one entry.
   */
  const submit = async (replace: boolean) => {
    if (saving) return;
    const cleanName = name.trim();
    if (!/^[A-Za-z0-9_][A-Za-z0-9._-]{0,63}$/.test(cleanName)) {
      setError(t("devTools.nameInvalid"));
      return;
    }
    if (cleanName.toLowerCase() === "orbit") {
      setError(t("devTools.nameReserved"));
      return;
    }

    const env: Record<string, string> = {};
    let keptAnySecret = false;
    for (const row of envRows) {
      const k = row.key.trim();
      if (!k) continue;
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(k)) {
        setError(t("devTools.invalidEnv", { name: k }));
        return;
      }
      if (BLOCKED_ENV_RE.test(k)) {
        setError(t("devTools.blockedEnv", { name: k }));
        return;
      }
      if (row.existingMasked && isSecretKey(k) && !row.value) {
        env[k] = "";
        keptAnySecret = true;
      } else {
        env[k] = row.value;
      }
    }

    const headers: Record<string, string> = {};
    for (const row of headerRows) {
      const k = row.key.trim();
      if (!k) continue;
      if (/[\s\r\n:]/.test(k)) {
        setError(t("devTools.invalidHeader", { name: k }));
        return;
      }
      if (row.existingMasked && isSecretKey(k) && !row.value) {
        headers[k] = "";
        keptAnySecret = true;
      } else {
        headers[k] = row.value;
      }
    }

    let transport: McpTransportKind;
    if (transportKind === "stdio") {
      const cleanCmd = command.trim();
      if (!cleanCmd) {
        setError(t("devTools.requireCommand"));
        return;
      }
      transport = {
        kind: "stdio",
        command: cleanCmd,
        args: parseArgsLine(argsText),
      };
    } else {
      const cleanUrl = url.trim();
      if (!/^https?:\/\//i.test(cleanUrl)) {
        setError(t("devTools.requireHttp"));
        return;
      }
      transport = {
        kind: "streamable_http",
        url: cleanUrl,
        header_keys: Object.keys(headers),
      };
    }

    const keepExistingSecrets = Boolean(seed) && keptAnySecret;
    const createOnly = !isEdit && !replace;

    if (createOnly && precheckable && takenNames.includes(cleanName)) {
      setError(null);
      setConflict({ name: cleanName, suggestion: suggestAvailableName(cleanName, takenNames) });
      nameInputRef.current?.focus();
      return;
    }

    setError(null);
    setSaving(true);
    try {
      await onSave({
        host_id: hostId,
        scope,
        cwd: targetCwd,
        name: cleanName,
        transport,
        env: transportKind === "stdio" ? env : {},
        headers: transportKind === "streamable_http" ? headers : {},
        keep_existing_secrets: keepExistingSecrets,
        enabled,
        create_only: createOnly,
      });
      setConflict(null);
      onClose();
    } catch (err) {
      if (createOnly && isAlreadyExistsError(err)) {
        setConflict({ name: cleanName, suggestion: suggestAvailableName(cleanName, takenNames) });
        nameInputRef.current?.focus();
      } else {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      setSaving(false);
    }
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    void submit(false);
  };

  return (
    <SettingDrawer
      open={open}
      onOpenChangeComplete={(visible) => {
        if (!visible) onClosed();
      }}
      onOpenChange={(open) => {
        if (!open && !saving) onClose();
      }}
      title={
        isEdit
          ? t("devTools.editServer")
          : importingFrom
            ? t("devTools.importServer")
            : t("devTools.addServer")
      }
      description={
        scope === "user" ? t("devTools.global") : (targetCwd ?? undefined)
      }
    >
      <form onSubmit={handleSubmit} className="flex flex-col">
        <FieldGroup>
          {importingFrom && importProjects.length > 0 && (
            <Field>
              <FieldLabel>{t("devTools.scope")}</FieldLabel>
              <SettingSelect
                value={destination}
                ariaLabel={t("devTools.scope")}
                onChange={(next) => {
                  setDestination(next);
                  setConflict(null);
                  setReplaceOpen(false);
                }}
                options={[
                  { value: "", label: t("devTools.global") },
                  ...importProjects,
                ]}
              />
            </Field>
          )}
          <Field>
            <FieldLabel>{t("devTools.transport")}</FieldLabel>
            <SettingSelect
              ariaLabel={t("devTools.transport")}
              value={transportKind}
              options={[
                { value: "stdio", label: "stdio" },
                { value: "streamable_http", label: "Streamable HTTP" },
              ]}
              onChange={setTransportKind}
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="mcp-server-name" className="text-[11px]">
              {t("devTools.name")}
            </FieldLabel>
            <Input
              id="mcp-server-name"
              ref={nameInputRef}
              type="text"
              autoFocus={!isEdit}
              value={name}
              disabled={isEdit}
              onChange={(e) => {
                setName(e.target.value);
                setConflict(null);
              }}
              placeholder="github"
              className="font-mono"
            />
            {conflict && (
              <div className="border-border-subtle bg-surface-raised mt-1 flex flex-col gap-2 rounded-md border p-3">
                <span className="text-ink-primary text-[13px] font-medium">
                  {t("devTools.conflictTitle")}
                </span>
                <span className="text-ink-muted text-[11px]">
                  {t("devTools.conflictExists", { name: conflict.name })}
                </span>
                <div className="flex flex-wrap items-center gap-2 pt-1">
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    disabled={saving}
                    onClick={() => {
                      setName(conflict.suggestion);
                      setConflict(null);
                      nameInputRef.current?.focus();
                    }}
                  >
                    {t("devTools.conflictRename", { name: conflict.suggestion })}
                  </Button>
                  {precheckable &&
                    existingServers.some(
                      (sv) => sv.scope === targetScopeId && sv.name === conflict.name,
                    ) && (
                      <Button
                        type="button"
                        size="sm"
                        variant="outline"
                        disabled={saving}
                        onClick={() => {
                          const existing = existingServers.find(
                            (sv) => sv.scope === targetScopeId && sv.name === conflict.name,
                          );
                          setConflict(null);
                          if (existing) onOpenExisting(existing);
                        }}
                      >
                        {t("devTools.conflictOpen")}
                      </Button>
                    )}
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    disabled={saving}
                    className="text-signal-danger"
                    onClick={() => setReplaceOpen(true)}
                  >
                    {t("devTools.conflictReplace")}
                  </Button>
                </div>
              </div>
            )}
          </Field>

          {transportKind === "stdio" ? (
            <>
              <div className="grid grid-cols-2 gap-3">
                <Field>
                  <FieldLabel
                    htmlFor="mcp-server-command"
                    className="text-[11px]"
                  >
                    {t("devTools.command")}
                  </FieldLabel>
                  <Input
                    id="mcp-server-command"
                    type="text"
                    value={command}
                    onChange={(e) => setCommand(e.target.value)}
                    placeholder="npx"
                    className="font-mono"
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="mcp-server-args" className="text-[11px]">
                    {t("devTools.args")}
                  </FieldLabel>
                  <Input
                    id="mcp-server-args"
                    type="text"
                    value={argsText}
                    onChange={(e) => setArgsText(e.target.value)}
                    placeholder="-y @modelcontextprotocol/server-github"
                    className="font-mono"
                  />
                </Field>
              </div>

              <KvEditorRows
                label={t("devTools.env")}
                addLabel={t("devTools.addVar")}
                keyPlaceholder="GITHUB_TOKEN"
                valuePlaceholder={t("devTools.value")}
                rows={envRows}
                onChange={setEnvRows}
              />
            </>
          ) : (
            <>
              <Field>
                <FieldLabel htmlFor="mcp-server-url" className="text-[11px]">
                  {t("devTools.endpoint")}
                </FieldLabel>
                <Input
                  id="mcp-server-url"
                  type="text"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="https://mcp.example.com/mcp"
                  className="font-mono"
                />
              </Field>

              <KvEditorRows
                label={t("devTools.headers")}
                addLabel={t("devTools.addHeader")}
                keyPlaceholder="Authorization"
                valuePlaceholder="Bearer …"
                rows={headerRows}
                onChange={setHeaderRows}
              />
            </>
          )}

          <Field orientation="horizontal" className="pt-1">
            <FieldLabel className="font-normal">
              {t("devTools.enabled")}
            </FieldLabel>
            <ToggleSwitch
              checked={enabled}
              disabled={saving}
              onChange={setEnabled}
              ariaLabel={t("devTools.enabled")}
            />
          </Field>

          {error && (
            <div className="rounded-lg border border-signal-danger-border bg-signal-danger-bg px-3 py-2 text-[13px] text-signal-danger">
              {error}
            </div>
          )}

          <div className="flex justify-end gap-2 pt-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={saving}
              onClick={onClose}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" size="sm" disabled={saving || !canSubmit}>
              {saving && <Loader2 className="h-[13px] w-[13px] animate-spin" />}
              <span>
                {isEdit
                  ? t("common.save")
                  : importingFrom
                    ? t("devTools.importServer")
                    : t("devTools.addServer")}
              </span>
            </Button>
          </div>
        </FieldGroup>
      </form>
      <ConfirmDialog
        open={replaceOpen}
        onOpenChange={setReplaceOpen}
        tone="danger"
        title={t("devTools.conflictReplaceTitle", { name: conflict?.name ?? name.trim() })}
        description={t("devTools.conflictReplaceBody", { name: conflict?.name ?? name.trim() })}
        confirmLabel={t("devTools.conflictReplace")}
        onConfirm={async () => {
          setReplaceOpen(false);
          setConflict(null);
          await submit(true);
        }}
      />
    </SettingDrawer>
  );
}

export function HostMcpSection({
  hostId,
  workspaceCwd = null,
  importProjects = [],
  onSelectDestination,
}: {
  hostId: string;
  workspaceCwd?: string | null;
  importProjects?: readonly {
    value: string;
    label: string;
    description: string;
  }[];
  onSelectDestination?: (cwd: string) => void;
}) {
  const online = useOrbitSelector((s) => {
    const state = s.fleet.hosts.find(
      (h) => h.host_id === hostId,
    )?.connection_state;
    return state === "online_direct" || state === "online_relay";
  });
  const [servers, setServers] =
    useState<readonly ExternalMcpServerRecord[]>(EMPTY_SERVERS);
  const [loading, setLoading] = useState(() => online);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [probingIds, setProbingIds] = useState<Record<string, boolean>>({});
  const [localProbes, setLocalProbes] = useState<
    Record<string, McpProbeResult>
  >({});
  const [removing, setRemoving] = useState<ExternalMcpServerRecord | null>(
    null,
  );
  const [editor, setEditor] = useState<{
    open: boolean;
    editing: ExternalMcpServerRecord | null;
    importingFrom: ExternalMcpServerRecord | null;
  } | null>(null);

  const mountedRef = useRef(true);
  const activeHostRef = useRef(hostId);
  const fetchSeqRef = useRef(0);
  const probeSeqRef = useRef<Record<string, number>>({});
  const toggleSeqRef = useRef(0);
  const toggleChainRef = useRef<Promise<void>>(Promise.resolve());

  const refreshServers = useCallback(async (options?: { silent?: boolean }) => {
    if (!mountedRef.current) return;
    if (!online) {
      setLoading(false);
      return;
    }
    const reqHost = hostId;
    const seq = ++fetchSeqRef.current;
    if (!options?.silent) {
      setLoading(true);
    }
    setLoadError(null);
    try {
      const snapshot = await orbitActions.refreshExternalMcpServers(
        reqHost,
        workspaceCwd,
      );
      if (
        mountedRef.current &&
        activeHostRef.current === reqHost &&
        fetchSeqRef.current === seq
      ) {
        setServers(snapshot);
        setLocalProbes({});
      }
    } catch (err) {
      if (
        !mountedRef.current ||
        activeHostRef.current !== reqHost ||
        fetchSeqRef.current !== seq
      ) {
        return;
      }
      setLoadError(err instanceof Error ? err.message : String(err));
    } finally {
      if (
        !options?.silent &&
        mountedRef.current &&
        activeHostRef.current === reqHost &&
        fetchSeqRef.current === seq
      ) {
        setLoading(false);
      }
    }
  }, [hostId, workspaceCwd, online]);

  useEffect(() => {
    mountedRef.current = true;
    activeHostRef.current = hostId;
    void refreshServers();
    return () => {
      mountedRef.current = false;
    };
  }, [hostId, refreshServers]);

  const filteredServers = useMemo(
    () =>
      servers.filter(
        (server) =>
          (server.scope === "project_dot_mcp") === Boolean(workspaceCwd),
      ),
    [servers, workspaceCwd],
  );

  const handleProbe = async (serverId: string) => {
    const reqHost = hostId;
    const seq = (probeSeqRef.current[serverId] ?? 0) + 1;
    probeSeqRef.current[serverId] = seq;
    setProbingIds((prev) => ({ ...prev, [serverId]: true }));
    try {
      const result = await orbitActions.probeExternalMcpServer(
        reqHost,
        serverId,
        workspaceCwd,
      );
      if (
        !mountedRef.current ||
        activeHostRef.current !== reqHost ||
        probeSeqRef.current[serverId] !== seq
      ) {
        return;
      }
      setLocalProbes((prev) => ({ ...prev, [serverId]: result }));
      if (result.ok && result.tools.length > 0) {
        setExpandedId(serverId);
      }
    } catch (err) {
      if (
        !mountedRef.current ||
        activeHostRef.current !== reqHost ||
        probeSeqRef.current[serverId] !== seq
      ) {
        return;
      }
      setLocalProbes((prev) => ({
        ...prev,
        [serverId]: {
          ok: false,
          tools: [],
          latency_ms: 0,
          error: err instanceof Error ? err.message : String(err),
          probed_at_ms: Date.now(),
        },
      }));
    } finally {
      if (
        mountedRef.current &&
        activeHostRef.current === reqHost &&
        probeSeqRef.current[serverId] === seq
      ) {
        setProbingIds((prev) => {
          const next = { ...prev };
          delete next[serverId];
          return next;
        });
      }
    }
  };

  const handleToggle = (
    server: ExternalMcpServerRecord,
    nextEnabled: boolean,
  ) => {
    const reqHost = hostId;
    const seq = ++toggleSeqRef.current;
    setServers((prev) =>
      prev.map((item) =>
        item.id === server.id ? { ...item, enabled: nextEnabled } : item,
      ),
    );
    toggleChainRef.current = toggleChainRef.current.then(async () => {
      try {
        await orbitActions.toggleExternalMcpServer(
          reqHost,
          server.id,
          nextEnabled,
          workspaceCwd,
        );
        if (
          mountedRef.current &&
          activeHostRef.current === reqHost &&
          toggleSeqRef.current === seq
        ) {
          await refreshServers({ silent: true });
        }
      } catch (err) {
        if (mountedRef.current && activeHostRef.current === reqHost) {
          setServers((prev) =>
            prev.map((item) =>
              item.id === server.id ? { ...item, enabled: !nextEnabled } : item,
            ),
          );
          orbitActions.showToast(
            err instanceof Error ? err.message : t("common.actionFailed"),
          );
        }
      }
    });
  };

  const handleDelete = async () => {
    if (!removing) return;
    await orbitActions.deleteExternalMcpServer(
      hostId,
      removing.id,
      workspaceCwd,
    );
    await refreshServers();
  };

  const handleSave = async (req: UpsertExternalMcpServerRequest) => {
    await orbitActions.upsertExternalMcpServer(req);
    if (!mountedRef.current) return;
    if (req.cwd && req.cwd !== workspaceCwd && onSelectDestination) {
      onSelectDestination(req.cwd);
    } else {
      await refreshServers();
    }
  };

  return (
    <section data-setting-id="settings.subTabMcp" className="space-y-3">
      <SettingGroupLabel
        action={
          <div className="flex items-center gap-2">
            <IconButton
              disabled={loading || !online}
              tooltip={t("common.refresh")}
              onClick={() => void refreshServers()}
            >
              <RefreshCw className={cn(loading && "animate-spin")} />
            </IconButton>
            <Button
              data-setting-id="devTools.addServer"
              size="sm"
              className="text-[13px]"
              disabled={!online}
              onClick={() =>
                setEditor({ open: true, editing: null, importingFrom: null })
              }
            >
              <Plus /> {t("devTools.addServer")}
            </Button>
          </div>
        }
      >
        {t("devTools.servers")}
      </SettingGroupLabel>
      {loadError && (
        <div className="rounded-lg border border-signal-danger-border bg-signal-danger-bg px-3 py-2 text-[13px] text-signal-danger">
          {loadError}
        </div>
      )}

      {/* Server List */}
      {!online || filteredServers.length === 0 ? (
        <SettingCard>
          <div className="text-ink-muted px-4 py-8 text-center text-[13px]">
            {!online
              ? t("devTools.offline")
              : loading
                ? t("common.loading")
                : t("devTools.emptyServers")}
          </div>
        </SettingCard>
      ) : (
        <SettingCard>
          {filteredServers.map((server) => {
            const expanded = expandedId === server.id;
            const probe = localProbes[server.id] ?? server.last_probe ?? null;
            const isProbing = Boolean(probingIds[server.id]);
            const isHttp = server.transport.kind === "streamable_http";
            const summaryLine =
              server.transport.kind === "stdio"
                ? [server.transport.command, ...server.transport.args].join(" ")
                : server.transport.url;

            const maskedEntries = isHttp
              ? Object.entries(server.masked_headers ?? {})
              : Object.entries(server.masked_env ?? {});

            return (
              <div key={server.id}>
                <SettingResourceRow
                  title={server.name}
                  description={<span className="font-mono">{summaryLine}</span>}
                  icon={isHttp ? <Globe /> : <Terminal />}
                  status={scopeBadgeLabel(server.scope)}
                  onClick={() => setExpandedId(expanded ? null : server.id)}
                  action={
                    <div className="flex items-center gap-1">
                      <IconButton
                        tooltip={t("devTools.test")}
                        disabled={isProbing}
                        onClick={() => void handleProbe(server.id)}
                      >
                        {isProbing ? (
                          <Loader2 className="animate-spin" />
                        ) : (
                          <Play />
                        )}
                      </IconButton>
                      {server.editable ? (
                        <>
                          <IconButton
                            tooltip={t("common.edit")}
                            onClick={() =>
                              setEditor({
                                open: true,
                                editing: server,
                                importingFrom: null,
                              })
                            }
                          >
                            <Pencil />
                          </IconButton>
                          <IconButton
                            tooltip={t("common.delete")}
                            tone="danger"
                            onClick={() => setRemoving(server)}
                          >
                            <Trash2 />
                          </IconButton>
                          <ToggleSwitch
                            checked={server.enabled}
                            disabled={!online}
                            ariaLabel={t("devTools.toggle", {
                              name: server.name,
                            })}
                            onChange={(enabled) =>
                              handleToggle(server, enabled)
                            }
                          />
                        </>
                      ) : (
                        <IconButton
                          tooltip={t("devTools.importServer")}
                          onClick={() =>
                            setEditor({
                              open: true,
                              editing: null,
                              importingFrom: server,
                            })
                          }
                        >
                          <Download />
                        </IconButton>
                      )}
                    </div>
                  }
                />
                {probe && (
                  <p
                    className={cn(
                      "px-4 pb-2 text-[11px]",
                      probe.ok ? "text-signal-success" : "text-signal-danger",
                    )}
                  >
                    {probe.ok
                      ? t("devTools.probeResult", {
                          count: probe.tools.length,
                          ms: probe.latency_ms,
                        })
                      : probe.error || t("devTools.probeFailed")}
                  </p>
                )}
                {expanded && (
                  <div className="border-border-subtle bg-surface-recessed space-y-3 border-t px-4 py-3 text-[13px]">
                    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px]">
                      <span className="text-ink-muted">
                        {t("devTools.source")} ·{" "}
                        <code className="text-ink-primary/85 font-mono">
                          {server.source_path}
                        </code>
                      </span>
                      {probe?.server_name && (
                        <span className="text-ink-muted">
                          {t("devTools.server")} ·{" "}
                          <code className="text-ink-primary/85 font-mono">
                            {probe.server_name}
                            {probe.server_version
                              ? ` ${probe.server_version}`
                              : ""}
                          </code>
                        </span>
                      )}
                      {probe?.protocol_version && (
                        <span className="text-ink-muted">
                          MCP{" "}
                          <code className="text-ink-primary/85 font-mono">
                            {probe.protocol_version}
                          </code>
                        </span>
                      )}
                    </div>

                    {maskedEntries.length > 0 && (
                      <div className="space-y-1">
                        <div className="text-ink-muted flex items-center gap-1 text-[11px] font-medium">
                          <KeyRound className="h-[11px] w-[11px]" />
                          <span>
                            {isHttp ? t("devTools.headers") : t("devTools.env")}
                          </span>
                        </div>
                        <div className="flex flex-wrap gap-2">
                          {maskedEntries.map(([k, v]) => (
                            <span
                              key={k}
                              className="border-border-subtle/70 bg-surface-raised/70 text-ink-primary/85 inline-flex items-center gap-1 rounded border px-2 py-1 font-mono text-[11px]"
                            >
                              <span>{k}</span>
                              {v && (
                                <span className="text-ink-muted">= {v}</span>
                              )}
                            </span>
                          ))}
                        </div>
                      </div>
                    )}

                    {probe && probe.ok && (
                      <div className="space-y-2">
                        <div className="text-ink-muted flex items-center gap-1 text-[11px] font-medium">
                          <Wrench className="h-[11px] w-[11px]" />
                          <span>
                            {t("devTools.tools", { count: probe.tools.length })}
                          </span>
                        </div>
                        {probe.tools.length === 0 ? (
                          <div className="text-ink-muted/75 text-[11px]">
                            {t("devTools.emptyTools")}
                          </div>
                        ) : (
                          <div className="orbit-native-scroll border-border-subtle/60 bg-surface-raised/50 max-h-[210px] divide-y divide-border-subtle/50 overflow-y-auto overscroll-contain rounded-lg border">
                            {probe.tools.map((tool) => (
                              <div key={tool.name} className="px-3 py-2">
                                <div className="text-ink-primary font-mono text-[11px] font-medium">
                                  {tool.name}
                                </div>
                                {tool.description && (
                                  <div className="text-ink-muted mt-1 line-clamp-2 text-[11px] leading-snug">
                                    {tool.description}
                                  </div>
                                )}
                              </div>
                            ))}
                          </div>
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

      <ConfirmDialog
        open={Boolean(removing)}
        onOpenChange={(open) => {
          if (!open) setRemoving(null);
        }}
        title={t("devTools.removeServer", { name: removing?.name ?? "" })}
        tone="danger"
        confirmLabel={t("common.delete")}
        onConfirm={handleDelete}
      />
      {editor && (
        <McpServerDrawer
          // Remount when switching target (e.g. "open existing" from a conflict) so the form
          // re-seeds from the selected server instead of keeping the draft.
          key={editor.editing?.id ?? editor.importingFrom?.id ?? "new"}
          open={editor.open}
          onClosed={() => setEditor(null)}
          importProjects={importProjects}
          hostId={hostId}
          workspaceCwd={workspaceCwd}
          editing={editor.editing}
          importingFrom={editor.importingFrom}
          existingServers={servers}
          onOpenExisting={(server) =>
            setEditor({ open: true, editing: server, importingFrom: null })
          }
          onClose={() =>
            setEditor((state) => state && { ...state, open: false })
          }
          onSave={handleSave}
        />
      )}
    </section>
  );
}
