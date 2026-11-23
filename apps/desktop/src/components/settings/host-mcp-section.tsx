import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
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
  SettingSectionHeader,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import {
  type ExternalMcpServerRecord,
  type McpProbeResult,
  type McpServerScope,
  type McpTransportKind,
  type McpWriteScope,
  type UpsertExternalMcpServerRequest,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type McpScopeFilter = "all" | "managed" | "discovered";

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
    .map((a) => (/[\s"'\\]/.test(a) ? `"${a.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"` : a))
    .join(" ");
}

function scopeBadgeLabel(scope: McpServerScope): string {
  switch (scope) {
    case "orbit_user":
      return "User";
    case "project_dot_mcp":
      return "Project";
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

function initKvRows(maskedMap: Record<string, string> | undefined): KvDraftRow[] {
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
    <div className="space-y-1.5">
      <div className="flex items-center justify-between">
        <span className="text-muted-foreground text-[11.5px] font-medium">{label}</span>
        <button
          type="button"
          onClick={() => onChange([...rows, { id: makeRowId(), key: "", value: "" }])}
          className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 text-[11.5px] font-medium transition-colors"
        >
          <Plus className="h-[11.5px] w-[11.5px]" />
          <span>{addLabel}</span>
        </button>
      </div>

      {rows.length === 0 ? (
        <div className="border-border/70 text-muted-foreground/75 rounded-lg border border-dashed px-3 py-2 text-[11.5px]">
          None configured
        </div>
      ) : (
        <div className="space-y-1.5">
          {rows.map((row) => {
            const secret = isSecretKey(row.key);
            const placeholder =
              row.existingMasked && secret
                ? `${row.existingMasked} (blank to keep)`
                : valuePlaceholder;
            return (
              <div key={row.id} className="flex items-center gap-1.5">
                <input
                  type="text"
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
                  className="border-border bg-background text-foreground placeholder:text-muted-foreground/55 h-[30px] w-[42%] rounded-lg border px-2.5 font-mono text-[12px] outline-none focus:border-foreground/40"
                />
                <div className="relative flex-1">
                  <input
                    type={secret ? "password" : "text"}
                    value={row.value}
                    onChange={(e) =>
                      onChange(
                        rows.map((r) => (r.id === row.id ? { ...r, value: e.target.value } : r)),
                      )
                    }
                    placeholder={placeholder}
                    className="border-border bg-background text-foreground placeholder:text-muted-foreground/55 h-[30px] w-full rounded-lg border px-2.5 font-mono text-[12px] outline-none focus:border-foreground/40"
                  />
                </div>
                <button
                  type="button"
                  aria-label={`Remove ${row.key || "row"}`}
                  onClick={() => onChange(rows.filter((r) => r.id !== row.id))}
                  className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[28px] w-[28px] shrink-0 cursor-pointer items-center justify-center rounded-md transition-colors"
                >
                  <X className="h-[13px] w-[13px]" />
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function McpServerModal({
  hostId,
  workspaceCwd,
  editing,
  importingFrom,
  onClose,
  onSave,
}: {
  hostId: string;
  workspaceCwd: string | null;
  editing: ExternalMcpServerRecord | null;
  importingFrom: ExternalMcpServerRecord | null;
  onClose: () => void;
  onSave: (req: UpsertExternalMcpServerRequest) => Promise<void>;
}) {
  const seed = editing ?? importingFrom;
  const isEdit = Boolean(editing);

  const [scope, setScope] = useState<McpWriteScope>(() =>
    editing?.scope === "project_dot_mcp" ? "project" : "user",
  );
  const [transportKind, setTransportKind] = useState<"stdio" | "streamable_http">(() =>
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
  const [envRows, setEnvRows] = useState<KvDraftRow[]>(() => initKvRows(seed?.masked_env));
  const [headerRows, setHeaderRows] = useState<KvDraftRow[]>(() =>
    initKvRows(seed?.masked_headers),
  );
  const [enabled, setEnabled] = useState(() => seed?.enabled ?? true);
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
    const cleanName = name.trim();
    if (!/^[A-Za-z0-9_][A-Za-z0-9._-]{0,63}$/.test(cleanName)) {
      setError("Name must use ASCII letters, digits, `.`, `_`, or `-`.");
      return;
    }
    if (cleanName.toLowerCase() === "orbit") {
      setError("`orbit` is reserved for Orbit's built-in MCP server.");
      return;
    }
    if (scope === "project" && !workspaceCwd) {
      setError("Select a workspace before saving to project `.mcp.json`.");
      return;
    }

    const env: Record<string, string> = {};
    let keptAnySecret = false;
    for (const row of envRows) {
      const k = row.key.trim();
      if (!k) continue;
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(k)) {
        setError(`Invalid env variable name "${k}".`);
        return;
      }
      if (BLOCKED_ENV_RE.test(k)) {
        setError(`Environment variable "${k}" is blocked for security reasons.`);
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
        setError(`Invalid HTTP header name "${k}".`);
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
        setError("Command is required for stdio MCP servers.");
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
        setError("Streamable HTTP URL must start with http:// or https://.");
        return;
      }
      transport = {
        kind: "streamable_http",
        url: cleanUrl,
        header_keys: Object.keys(headers),
      };
    }

    const keepExistingSecrets = Boolean(seed) && keptAnySecret;

    setError(null);
    setSaving(true);
    try {
      await onSave({
        host_id: hostId,
        scope,
        cwd: workspaceCwd,
        name: cleanName,
        transport,
        env: transportKind === "stdio" ? env : {},
        headers: transportKind === "streamable_http" ? headers : {},
        keep_existing_secrets: keepExistingSecrets,
        enabled,
      });
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !saving) onClose();
      }}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4 backdrop-blur-[2px]"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={isEdit ? "Edit MCP Server" : "Add MCP Server"}
        className="border-border bg-elevated text-foreground floating-surface flex max-h-[85vh] w-full max-w-[490px] flex-col overflow-hidden rounded-[14px] border p-0"
      >
        <div className="flex items-center justify-between px-5 pt-5 pb-4">
          <div>
            <h3 className="text-[15px] font-semibold tracking-[-0.01em]">
              {isEdit
                ? "Edit MCP Server"
                : importingFrom
                  ? "Import MCP Server"
                  : "Add MCP Server"}
            </h3>
            <p className="text-muted-foreground mt-0.5 text-[12px]">
              {importingFrom
                ? `Import "${importingFrom.name}" into Orbit. Existing secrets stay on the host.`
                : "Configure a stdio or Streamable HTTP MCP server on this host."}
            </p>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-lg transition-colors"
          >
            <X className="h-[15px] w-[15px]" />
          </button>
        </div>

        <form
          onSubmit={(e) => void handleSubmit(e)}
          className="orbit-native-scroll min-h-0 flex-1 space-y-3.5 overflow-y-auto overscroll-contain px-5 pb-5"
        >
          {/* Scope + Transport toggles */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1">
              <span className="text-muted-foreground text-[11.5px] font-medium">Scope</span>
              <div className="border-border bg-background/60 grid grid-cols-2 gap-0.5 rounded-[9px] border p-0.5">
                <button
                  type="button"
                  disabled={isEdit}
                  onClick={() => setScope("user")}
                  className={cn(
                    "h-[26px] cursor-pointer rounded-[7px] text-[12px] transition-colors disabled:cursor-not-allowed disabled:opacity-65",
                    scope === "user"
                      ? "bg-foreground/9 text-foreground font-medium dark:bg-white/[0.11]"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  User
                </button>
                <button
                  type="button"
                  disabled={isEdit || !workspaceCwd}
                  title={workspaceCwd ? `${workspaceCwd}/.mcp.json` : "No active workspace"}
                  onClick={() => setScope("project")}
                  className={cn(
                    "h-[26px] cursor-pointer rounded-[7px] text-[12px] transition-colors disabled:cursor-not-allowed disabled:opacity-45",
                    scope === "project"
                      ? "bg-foreground/9 text-foreground font-medium dark:bg-white/[0.11]"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  Project .mcp.json
                </button>
              </div>
            </div>

            <div className="space-y-1">
              <span className="text-muted-foreground text-[11.5px] font-medium">Transport</span>
              <div className="border-border bg-background/60 grid grid-cols-2 gap-0.5 rounded-[9px] border p-0.5">
                <button
                  type="button"
                  onClick={() => setTransportKind("stdio")}
                  className={cn(
                    "h-[26px] cursor-pointer rounded-[7px] text-[12px] transition-colors",
                    transportKind === "stdio"
                      ? "bg-foreground/9 text-foreground font-medium dark:bg-white/[0.11]"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  stdio
                </button>
                <button
                  type="button"
                  onClick={() => setTransportKind("streamable_http")}
                  className={cn(
                    "h-[26px] cursor-pointer rounded-[7px] text-[12px] transition-colors",
                    transportKind === "streamable_http"
                      ? "bg-foreground/9 text-foreground font-medium dark:bg-white/[0.11]"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  HTTP / SSE
                </button>
              </div>
            </div>
          </div>

          <label className="block space-y-1">
            <span className="text-muted-foreground text-[11.5px] font-medium">Server Name</span>
            <input
              type="text"
              autoFocus={!isEdit}
              value={name}
              disabled={isEdit}
              onChange={(e) => setName(e.target.value)}
              placeholder="github"
              className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 h-[32px] w-full rounded-lg border px-2.5 font-mono text-[12.5px] outline-none focus:border-foreground/40 disabled:opacity-60"
            />
          </label>

          {transportKind === "stdio" ? (
            <>
              <div className="grid grid-cols-2 gap-3">
                <label className="block space-y-1">
                  <span className="text-muted-foreground text-[11.5px] font-medium">Command</span>
                  <input
                    type="text"
                    value={command}
                    onChange={(e) => setCommand(e.target.value)}
                    placeholder="npx or uvx"
                    className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 h-[32px] w-full rounded-lg border px-2.5 font-mono text-[12.5px] outline-none focus:border-foreground/40"
                  />
                </label>
                <label className="block space-y-1">
                  <span className="text-muted-foreground text-[11.5px] font-medium">Arguments</span>
                  <input
                    type="text"
                    value={argsText}
                    onChange={(e) => setArgsText(e.target.value)}
                    placeholder="-y @modelcontextprotocol/server-github"
                    className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 h-[32px] w-full rounded-lg border px-2.5 font-mono text-[12.5px] outline-none focus:border-foreground/40"
                  />
                </label>
              </div>

              <KvEditorRows
                label="Environment Variables"
                addLabel="Add variable"
                keyPlaceholder="GITHUB_TOKEN"
                valuePlaceholder="Value"
                rows={envRows}
                onChange={setEnvRows}
              />
            </>
          ) : (
            <>
              <label className="block space-y-1">
                <span className="text-muted-foreground text-[11.5px] font-medium">
                  Endpoint URL
                </span>
                <input
                  type="text"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="https://mcp.example.com/mcp"
                  className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 h-[32px] w-full rounded-lg border px-2.5 font-mono text-[12.5px] outline-none focus:border-foreground/40"
                />
              </label>

              <KvEditorRows
                label="HTTP Headers"
                addLabel="Add header"
                keyPlaceholder="Authorization"
                valuePlaceholder="Bearer …"
                rows={headerRows}
                onChange={setHeaderRows}
              />
            </>
          )}

          <div className="flex items-center justify-between pt-0.5">
            <span className="text-muted-foreground text-[12px]">Enabled</span>
            <ToggleSwitch
              checked={enabled}
              onChange={setEnabled}
              ariaLabel="Enable MCP server"
            />
          </div>

          {error && (
            <div className="rounded-lg border border-red-500/25 bg-red-500/10 px-3 py-2 text-[12px] text-red-600 dark:text-red-400">
              {error}
            </div>
          )}

          <div className="flex items-center justify-end gap-2 pt-2">
            <button
              type="button"
              disabled={saving}
              onClick={onClose}
              className="border-border hover:bg-accent h-[30px] cursor-pointer rounded-lg border px-3 text-[12.5px] font-medium transition-colors disabled:opacity-50"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={saving}
              className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-medium transition-opacity hover:opacity-90 disabled:opacity-50"
            >
              {saving && <Loader2 className="h-[12.5px] w-[12.5px] animate-spin" />}
              <span>{isEdit ? "Save Changes" : importingFrom ? "Import" : "Add Server"}</span>
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

export function HostMcpSection({ hostId }: { hostId: string }) {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostCard = fleet.hosts.find((h) => h.host_id === hostId) ?? null;
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const servers = useOrbitSelector((s) => s.externalMcpServersByHost[hostId] ?? EMPTY_SERVERS);

  const workspaceCwd = selectedHostId === hostId ? selectedCwd.trim() || null : null;

  const [filter, setFilter] = useState<McpScopeFilter>("all");
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [probingIds, setProbingIds] = useState<Record<string, boolean>>({});
  const [localProbes, setLocalProbes] = useState<Record<string, McpProbeResult>>({});
  const [togglingIds, setTogglingIds] = useState<Record<string, boolean>>({});
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [modalState, setModalState] = useState<{
    open: boolean;
    editing: ExternalMcpServerRecord | null;
    importingFrom: ExternalMcpServerRecord | null;
  }>({ open: false, editing: null, importingFrom: null });

  const mountedRef = useRef(true);
  const activeHostRef = useRef(hostId);
  const fetchSeqRef = useRef(0);
  const probeSeqRef = useRef<Record<string, number>>({});

  const refreshServers = useCallback(async () => {
    const reqHost = hostId;
    const seq = ++fetchSeqRef.current;
    setLoading(true);
    setLoadError(null);
    try {
      await orbitActions.refreshExternalMcpServers(reqHost, workspaceCwd);
      if (mountedRef.current && activeHostRef.current === reqHost && fetchSeqRef.current === seq) {
        setLocalProbes({});
      }
    } catch (err) {
      if (!mountedRef.current || activeHostRef.current !== reqHost || fetchSeqRef.current !== seq) {
        return;
      }
      setLoadError(err instanceof Error ? err.message : String(err));
    } finally {
      if (mountedRef.current && activeHostRef.current === reqHost && fetchSeqRef.current === seq) {
        setLoading(false);
      }
    }
  }, [hostId, workspaceCwd]);

  useEffect(() => {
    mountedRef.current = true;
    activeHostRef.current = hostId;
    void refreshServers();
    return () => {
      mountedRef.current = false;
    };
  }, [hostId, refreshServers]);

  const showHostBadge =
    Boolean(hostCard) &&
    (fleet.hosts.length > 1 || (hostCard !== null && !isLocalHostCard(hostCard)));

  const managedCount = useMemo(() => servers.filter((s) => s.editable).length, [servers]);
  const discoveredCount = useMemo(() => servers.filter((s) => !s.editable).length, [servers]);

  const filteredServers = useMemo(() => {
    return servers.filter((s) => {
      if (filter === "managed") return s.editable;
      if (filter === "discovered") return !s.editable;
      return true;
    });
  }, [servers, filter]);

  const handleProbe = async (serverId: string) => {
    const reqHost = hostId;
    const seq = (probeSeqRef.current[serverId] ?? 0) + 1;
    probeSeqRef.current[serverId] = seq;
    setProbingIds((prev) => ({ ...prev, [serverId]: true }));
    try {
      const result = await orbitActions.probeExternalMcpServer(reqHost, serverId, workspaceCwd);
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

  const handleToggle = async (server: ExternalMcpServerRecord, nextEnabled: boolean) => {
    const reqHost = hostId;
    setTogglingIds((prev) => ({ ...prev, [server.id]: true }));
    try {
      await orbitActions.toggleExternalMcpServer(reqHost, server.id, nextEnabled, workspaceCwd);
    } catch (err) {
      if (mountedRef.current && activeHostRef.current === reqHost) {
        orbitActions.showToast(
          err instanceof Error ? err.message : "Failed to update MCP server",
        );
      }
    } finally {
      if (mountedRef.current && activeHostRef.current === reqHost) {
        setTogglingIds((prev) => {
          const next = { ...prev };
          delete next[server.id];
          return next;
        });
      }
    }
  };

  const handleDelete = async (server: ExternalMcpServerRecord) => {
    const reqHost = hostId;
    setConfirmDeleteId(null);
    try {
      await orbitActions.deleteExternalMcpServer(reqHost, server.id, workspaceCwd);
      if (!mountedRef.current || activeHostRef.current !== reqHost) return;
      setLocalProbes((prev) => {
        const next = { ...prev };
        delete next[server.id];
        return next;
      });
      orbitActions.showToast(`Removed MCP server "${server.name}"`);
    } catch (err) {
      if (mountedRef.current && activeHostRef.current === reqHost) {
        orbitActions.showToast(
          err instanceof Error ? err.message : "Failed to delete MCP server",
        );
      }
    }
  };

  const handleSave = async (req: UpsertExternalMcpServerRequest) => {
    await orbitActions.upsertExternalMcpServer(req);
    if (mountedRef.current && activeHostRef.current === req.host_id) {
      setLocalProbes({});
      orbitActions.showToast(`Saved MCP server "${req.name}"`);
    }
  };

  return (
    <div className="space-y-5">
      <SettingSectionHeader
        title="MCP Servers"
        description="External stdio and Streamable HTTP Model Context Protocol servers on this host."
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
              disabled={loading}
              onClick={() => void refreshServers()}
              className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
            >
              <RefreshCw className={cn("h-[13px] w-[13px]", loading && "animate-spin")} />
              <span>Refresh</span>
            </button>
            <button
              type="button"
              onClick={() =>
                setModalState({ open: true, editing: null, importingFrom: null })
              }
              className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-2.5 text-[12px] font-medium transition-opacity hover:opacity-90"
            >
              <Plus className="h-[13.5px] w-[13.5px]" />
              <span>Add MCP Server</span>
            </button>
          </div>
        }
      />

      {/* Filter bar */}
      <div className="flex flex-wrap items-center justify-between gap-2.5">
        <div
          role="tablist"
          aria-label="Filter MCP server scope"
          className="border-border bg-background/60 inline-flex items-center gap-0.5 rounded-[9px] border p-0.5"
        >
          {(
            [
              { id: "all", label: "All", count: servers.length },
              { id: "managed", label: "Managed", count: managedCount },
              { id: "discovered", label: "Discovered", count: discoveredCount },
            ] as const
          ).map((tab) => {
            const active = filter === tab.id;
            return (
              <button
                key={tab.id}
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => setFilter(tab.id)}
                className={cn(
                  "inline-flex h-[26px] cursor-pointer items-center gap-1.5 rounded-[7px] px-2.5 text-[12px] transition-colors",
                  active
                    ? "bg-foreground/9 text-foreground font-medium dark:bg-white/[0.11]"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                <span>{tab.label}</span>
                <span className="text-muted-foreground/75 text-[11px] tabular-nums">
                  {tab.count}
                </span>
              </button>
            );
          })}
        </div>

        {workspaceCwd && (
          <span
            title={workspaceCwd}
            className="text-muted-foreground/75 max-w-[260px] truncate font-mono text-[11px]"
          >
            {workspaceCwd}
          </span>
        )}
      </div>

      {loadError && (
        <div className="rounded-lg border border-red-500/25 bg-red-500/10 px-3 py-2 text-[12px] text-red-600 dark:text-red-400">
          {loadError}
        </div>
      )}

      {/* Server List */}
      {filteredServers.length === 0 ? (
        <SettingCard>
          <div className="text-muted-foreground px-4 py-8 text-center text-[12.5px]">
            {loading
              ? "Loading MCP servers…"
              : filter === "managed"
                ? "No Orbit or project MCP servers yet. Click Add MCP Server to configure one."
                : filter === "discovered"
                  ? "No external MCP servers detected in Claude, Codex, or Antigravity configs."
                  : "No external MCP servers configured on this host."}
          </div>
        </SettingCard>
      ) : (
        <SettingCard>
          {filteredServers.map((server) => {
            const expanded = expandedId === server.id;
            const probe = localProbes[server.id] ?? server.last_probe ?? null;
            const isProbing = Boolean(probingIds[server.id]);
            const isToggling = Boolean(togglingIds[server.id]);
            const confirmingDelete = confirmDeleteId === server.id;
            const isHttp = server.transport.kind === "streamable_http";
            const summaryLine =
              server.transport.kind === "stdio"
                ? [server.transport.command, ...server.transport.args].join(" ")
                : server.transport.url;

            const maskedEntries = isHttp
              ? Object.entries(server.masked_headers ?? {})
              : Object.entries(server.masked_env ?? {});

            return (
              <div key={server.id} className="px-4 py-3">
                <div className="flex items-center justify-between gap-3">
                  <button
                    type="button"
                    aria-expanded={expanded}
                    onClick={() => setExpandedId(expanded ? null : server.id)}
                    className="flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 text-left"
                  >
                    {expanded ? (
                      <ChevronDown className="text-muted-foreground/70 h-[13.5px] w-[13.5px] shrink-0" />
                    ) : (
                      <ChevronRight className="text-muted-foreground/70 h-[13.5px] w-[13.5px] shrink-0" />
                    )}
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <span
                          className={cn(
                            "truncate text-[13.5px] font-medium",
                            server.enabled ? "text-foreground" : "text-muted-foreground",
                          )}
                        >
                          {server.name}
                        </span>
                        <span className="border-border bg-background/70 text-muted-foreground inline-flex shrink-0 items-center gap-1 rounded border px-1.5 py-0.5 text-[10.5px] font-medium">
                          {isHttp ? (
                            <Globe className="h-[10px] w-[10px]" />
                          ) : (
                            <Terminal className="h-[10px] w-[10px]" />
                          )}
                          {isHttp ? "HTTP" : "stdio"}
                        </span>
                        <span className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10.5px] font-medium">
                          {scopeBadgeLabel(server.scope)}
                        </span>
                        {probe && probe.ok && (
                          <span className="inline-flex shrink-0 items-center gap-1 rounded bg-emerald-500/10 px-1.5 py-0.5 text-[10.5px] font-medium text-emerald-600 tabular-nums dark:text-emerald-400">
                            {probe.tools.length} {probe.tools.length === 1 ? "tool" : "tools"} ·{" "}
                            {probe.latency_ms}ms
                          </span>
                        )}
                      </div>

                      <div className="text-muted-foreground/75 mt-0.5 truncate font-mono text-[11.5px]">
                        {summaryLine}
                      </div>

                      {probe && !probe.ok && (
                        <div className="mt-0.5 truncate text-[11.5px] text-red-600 dark:text-red-400">
                          {probe.error ?? "Probe failed"}
                        </div>
                      )}
                    </div>
                  </button>

                  <div className="flex shrink-0 items-center gap-1.5">
                    <button
                      type="button"
                      disabled={isProbing}
                      onClick={() => void handleProbe(server.id)}
                      className="border-border hover:bg-accent text-foreground/85 inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium transition-colors disabled:opacity-50"
                    >
                      {isProbing ? (
                        <Loader2 className="h-[11.5px] w-[11.5px] animate-spin" />
                      ) : (
                        <Play className="h-[10.5px] w-[10.5px]" />
                      )}
                      <span>Test</span>
                    </button>

                    {server.editable ? (
                      <>
                        {confirmingDelete ? (
                          <div className="flex items-center gap-1">
                            <button
                              type="button"
                              onClick={() => void handleDelete(server)}
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
                              aria-label={`Edit ${server.name}`}
                              onClick={() =>
                                setModalState({
                                  open: true,
                                  editing: server,
                                  importingFrom: null,
                                })
                              }
                              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
                            >
                              <Pencil className="h-[12px] w-[12px]" />
                            </button>
                            <button
                              type="button"
                              aria-label={`Delete ${server.name}`}
                              onClick={() => setConfirmDeleteId(server.id)}
                              className="text-muted-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors hover:text-red-500"
                            >
                              <Trash2 className="h-[12px] w-[12px]" />
                            </button>
                          </>
                        )}

                        <div className="pl-1">
                          <ToggleSwitch
                            checked={server.enabled}
                            disabled={isToggling}
                            ariaLabel={`Toggle ${server.name}`}
                            onChange={(next) => void handleToggle(server, next)}
                          />
                        </div>
                      </>
                    ) : (
                      <button
                        type="button"
                        onClick={() =>
                          setModalState({
                            open: true,
                            editing: null,
                            importingFrom: server,
                          })
                        }
                        className="border-border hover:bg-accent text-muted-foreground hover:text-foreground inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium transition-colors"
                      >
                        <Download className="h-[11px] w-[11px]" />
                        <span>Import</span>
                      </button>
                    )}
                  </div>
                </div>

                {expanded && (
                  <div className="border-border/60 mt-2.5 space-y-2.5 border-t pt-2.5 pl-6 text-[12px]">
                    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[11.5px]">
                      <span className="text-muted-foreground">
                        Source:{" "}
                        <code className="text-foreground/85 font-mono">{server.source_path}</code>
                      </span>
                      {probe?.server_name && (
                        <span className="text-muted-foreground">
                          Server:{" "}
                          <code className="text-foreground/85 font-mono">
                            {probe.server_name}
                            {probe.server_version ? ` ${probe.server_version}` : ""}
                          </code>
                        </span>
                      )}
                      {probe?.protocol_version && (
                        <span className="text-muted-foreground">
                          MCP <code className="text-foreground/85 font-mono">{probe.protocol_version}</code>
                        </span>
                      )}
                    </div>

                    {maskedEntries.length > 0 && (
                      <div className="space-y-1">
                        <div className="text-muted-foreground flex items-center gap-1 text-[11px] font-medium">
                          <KeyRound className="h-[11px] w-[11px]" />
                          <span>{isHttp ? "Headers" : "Environment"}</span>
                        </div>
                        <div className="flex flex-wrap gap-1.5">
                          {maskedEntries.map(([k, v]) => (
                            <span
                              key={k}
                              className="border-border/70 bg-background/70 text-foreground/85 inline-flex items-center gap-1 rounded border px-1.5 py-0.5 font-mono text-[11px]"
                            >
                              <span>{k}</span>
                              {v && <span className="text-muted-foreground">= {v}</span>}
                            </span>
                          ))}
                        </div>
                      </div>
                    )}

                    {probe && probe.ok && (
                      <div className="space-y-1.5">
                        <div className="text-muted-foreground flex items-center gap-1 text-[11px] font-medium">
                          <Wrench className="h-[11px] w-[11px]" />
                          <span>
                            Tools ({probe.tools.length})
                          </span>
                        </div>
                        {probe.tools.length === 0 ? (
                          <div className="text-muted-foreground/75 text-[11.5px]">
                            Server reported no tools.
                          </div>
                        ) : (
                          <div className="orbit-native-scroll border-border/60 bg-background/50 max-h-[210px] divide-y divide-border/50 overflow-y-auto overscroll-contain rounded-lg border">
                            {probe.tools.map((tool) => (
                              <div key={tool.name} className="px-2.5 py-1.5">
                                <div className="text-foreground font-mono text-[11.5px] font-medium">
                                  {tool.name}
                                </div>
                                {tool.description && (
                                  <div className="text-muted-foreground mt-0.5 line-clamp-2 text-[11px] leading-snug">
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

      {modalState.open && (
        <McpServerModal
          hostId={hostId}
          workspaceCwd={workspaceCwd}
          editing={modalState.editing}
          importingFrom={modalState.importingFrom}
          onClose={() => setModalState({ open: false, editing: null, importingFrom: null })}
          onSave={handleSave}
        />
      )}
    </div>
  );
}
