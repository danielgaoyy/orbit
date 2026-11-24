import { useCallback, useEffect, useState } from "react";
import {
  Bot,
  Check,
  Copy,
  ExternalLink,
  RefreshCw,
  Server,
  ShieldCheck,
  Sparkles,
  Star,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  type HermesBridgeBundle,
  type PersonalRouterConfig,
  type PrimaryRouterSelection,
  type ProviderKind,
  type RouterHostCapabilities,
  type RouterPreReviewMode,
  type RouterScope,
  type RouterStatusSnapshot,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { orbitActions } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface HostPersonalRouterSectionProps {
  hostId: string;
}

export function HostPersonalRouterSection({ hostId }: HostPersonalRouterSectionProps) {
  const [status, setStatus] = useState<RouterStatusSnapshot | null>(null);
  const [fleetHosts, setFleetHosts] = useState<RouterHostCapabilities[]>([]);
  const [primarySelection, setPrimarySelection] = useState<PrimaryRouterSelection | null>(null);
  const [loading, setLoading] = useState(true);
  const [discovering, setDiscovering] = useState(false);
  const [saving, setSaving] = useState(false);
  const [activatingHostId, setActivatingHostId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [bridgeBundle, setBridgeBundle] = useState<HermesBridgeBundle | null>(null);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [webhookSecretDraft, setWebhookSecretDraft] = useState("");

  const [draft, setDraft] = useState<PersonalRouterConfig>({
    enabled: false,
    router_agent_id: null,
    provider: "acp",
    acp_agent_id: "hermes",
    model: null,
    effort: null,
    cwd: null,
    scope: "global_fleet",
    pre_review_mode: "medium_risk_only",
    share_host_hermes_memory: true,
    soul_prompt_override: null,
    external_webhook_url: null,
    has_external_webhook_secret: false,
  });

  const loadStatus = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [snap, primary] = await Promise.all([
        orbitIpc.getPersonalRouterStatus(hostId),
        orbitIpc.getPrimaryRouterSelection(),
      ]);
      setStatus(snap);
      setDraft(snap.config);
      setPrimarySelection(primary);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [hostId]);

  const scanFleetHosts = useCallback(async () => {
    setDiscovering(true);
    try {
      const hosts = await orbitIpc.discoverFleetRouterHosts();
      setFleetHosts(hosts);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setDiscovering(false);
    }
  }, []);

  useEffect(() => {
    void loadStatus();
    void scanFleetHosts();
  }, [loadStatus, scanFleetHosts]);

  const handleSave = async (activate: boolean) => {
    setSaving(true);
    setError(null);
    try {
      const nextCfg: PersonalRouterConfig = {
        ...draft,
        enabled: activate ? true : draft.enabled,
      };
      const snap = await orbitIpc.configurePersonalRouter(
        hostId,
        nextCfg,
        activate,
        webhookSecretDraft.trim() ? webhookSecretDraft.trim() : undefined,
      );
      setStatus(snap);
      setDraft(snap.config);
      setWebhookSecretDraft("");
      if (activate && snap.router_agent?.agent_id) {
        const nextPrimary = await orbitIpc.setPrimaryRouterSelection({
          host_id: hostId,
          agent_id: snap.router_agent.agent_id,
          updated_at_ms: Date.now(),
        });
        setPrimarySelection(nextPrimary);
        orbitActions.selectAgent(hostId, snap.router_agent.agent_id);
        orbitActions.closeSettings();
      }
      void scanFleetHosts();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleSelectPrimaryHost = async (target: RouterHostCapabilities) => {
    setActivatingHostId(target.host_id);
    setError(null);
    try {
      const provider: ProviderKind = target.hermes_installed
        ? "acp"
        : (target.available_providers[0] ?? "claude_code");
      const acpId = target.hermes_installed ? "hermes" : null;
      const snap = await orbitIpc.configurePersonalRouter(
        target.host_id,
        {
          enabled: true,
          router_agent_id: target.active_router_agent_id ?? null,
          provider,
          acp_agent_id: acpId,
          model: null,
          effort: null,
          cwd: null,
          scope: "global_fleet",
          pre_review_mode: "medium_risk_only",
          share_host_hermes_memory: true,
          soul_prompt_override: null,
          external_webhook_url: null,
          has_external_webhook_secret: false,
        },
        true,
      );
      const nextPrimary = await orbitIpc.setPrimaryRouterSelection({
        host_id: target.host_id,
        agent_id: snap.router_agent?.agent_id ?? target.active_router_agent_id ?? null,
        updated_at_ms: Date.now(),
      });
      setPrimarySelection(nextPrimary);
      if (snap.router_agent?.agent_id) {
        orbitActions.selectAgent(target.host_id, snap.router_agent.agent_id);
        orbitActions.closeSettings();
      }
      if (target.host_id === hostId) {
        setStatus(snap);
        setDraft(snap.config);
      }
      void scanFleetHosts();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActivatingHostId(null);
    }
  };

  const handleExportBridge = async () => {
    try {
      const bundle = await orbitIpc.exportHermesBridgeConfig(hostId);
      setBridgeBundle(bundle);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const copyText = async (key: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedKey(key);
      window.setTimeout(() => setCopiedKey(null), 1600);
    } catch {
      // ignore clipboard errors
    }
  };

  return (
    <div className="space-y-6 pb-8">
      {/* Header */}
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-ink-primary flex items-center gap-2 text-[15px] font-semibold">
            <Sparkles className="text-ink-secondary h-4 w-4" />
            Personal Router
          </h3>
          <p className="text-ink-secondary mt-1 text-[12.5px] leading-relaxed">
            Designate a single primary Hermes or coding harness across your Fleet as your persistent
            Personal Router. It maintains long-term memory, inspects remote project context across
            all connected hosts, dispatches Workers asynchronously, and bridges Feishu bidirectionally.
          </p>
        </div>
        {status?.router_agent?.agent_id && (
          <Button
            type="button"
            size="sm"
            onClick={() => {
              orbitActions.selectAgent(hostId, status.router_agent!.agent_id);
              orbitActions.closeSettings();
            }}
          >
            <Bot className="h-3.5 w-3.5" />
            Open Router Session
          </Button>
        )}
      </div>

      {error && (
        <div className="border-destructive/30 bg-destructive/10 text-destructive rounded-lg border px-3 py-2 text-[12px]">
          {error}
        </div>
      )}

      {/* Multi-Host Fleet Primary Router Selector */}
      <div className="border-border-subtle bg-surface-raised rounded-xl border p-4">
        <div className="flex items-center justify-between gap-2">
          <div>
            <div className="text-ink-primary flex items-center gap-1.5 text-[13px] font-semibold">
              <Server className="text-ink-muted h-3.5 w-3.5" />
              Primary Router Host Selection (Multi-Host Fleet)
            </div>
            <p className="text-ink-secondary mt-0.5 text-[12px]">
              Choose which connected host (e.g., always-on remote Mac mini, cloud Devbox, or this
              machine) runs your canonical Primary Hermes. All clients route here regardless of
              which machine you are currently on.
            </p>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={discovering}
            onClick={() => void scanFleetHosts()}
          >
            <RefreshCw className={cn("h-3 w-3", discovering && "animate-spin")} />
            {discovering ? "Scanning..." : "Scan Fleet"}
          </Button>
        </div>

        <div className="mt-3 space-y-2">
          {fleetHosts.length === 0 && !discovering && (
            <div className="text-ink-muted py-3 text-center text-[12px]">
              No connected hosts detected yet.
            </div>
          )}
          {fleetHosts.map((h) => {
            const isBusy = activatingHostId === h.host_id;
            const isPrimary = primarySelection?.host_id === h.host_id;
            return (
              <div
                key={h.host_id}
                className={cn(
                  "bg-surface-base flex flex-wrap items-center justify-between gap-3 rounded-lg border px-3 py-2.5 transition-colors",
                  isPrimary ? "border-border-strong" : "border-border-subtle",
                )}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span
                      className={cn(
                        "h-2 w-2 rounded-full",
                        h.online ? "bg-emerald-500" : "bg-ink-faint",
                      )}
                    />
                    <span className="text-ink-primary text-[13px] font-medium">{h.host_name}</span>
                    {isPrimary && (
                      <span className="bg-accent text-ink-primary inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[10.5px] font-medium">
                        <Star className="h-2.5 w-2.5 fill-current" />
                        Primary Router
                      </span>
                    )}
                    <span className="bg-surface-recessed text-ink-muted rounded-md px-1.5 py-0.5 text-[10.5px]">
                      {h.is_local ? "Local" : "Remote"}
                    </span>
                    {h.hermes_installed && (
                      <span className="bg-surface-recessed text-ink-secondary rounded-md px-1.5 py-0.5 font-mono text-[10.5px]">
                        Hermes {h.hermes_version ? h.hermes_version : "ready"}
                      </span>
                    )}
                    {h.hermes_has_existing_memory && (
                      <span className="rounded-md bg-emerald-500/12 px-1.5 py-0.5 text-[10.5px] font-medium text-emerald-700 dark:text-emerald-400">
                        ~/.hermes memory
                      </span>
                    )}
                    {h.hermes_gateway_running && (
                      <span className="rounded-md bg-blue-500/12 px-1.5 py-0.5 text-[10.5px] font-medium text-blue-700 dark:text-blue-400">
                        Gateway active
                      </span>
                    )}
                  </div>
                  <div className="text-ink-muted mt-1 text-[11.5px]">
                    {h.recommendation_reason ?? (h.online ? "Online host" : "Offline")}
                  </div>
                </div>
                <Button
                  type="button"
                  variant={isPrimary ? "outline" : "default"}
                  size="sm"
                  disabled={!h.online || isBusy}
                  onClick={() => void handleSelectPrimaryHost(h)}
                >
                  {isBusy
                    ? "Activating..."
                    : isPrimary
                      ? "Open Primary Router"
                      : h.hermes_installed
                        ? "Set as Primary (Hermes)"
                        : "Set as Primary"}
                </Button>
              </div>
            );
          })}
        </div>
      </div>

      {/* Host Router Configuration */}
      <div className="border-border-subtle bg-surface-raised space-y-4 rounded-xl border p-4">
        <div className="text-ink-primary text-[13px] font-semibold">
          Host Router Policy & Memory Interop
        </div>

        {loading ? (
          <div className="text-ink-muted py-4 text-[12px]">Loading configuration...</div>
        ) : (
          <>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label className="space-y-1">
                <span className="text-ink-secondary text-[11.5px] font-medium">Router Engine</span>
                <select
                  value={
                    draft.provider === "acp"
                      ? `acp:${draft.acp_agent_id ?? "hermes"}`
                      : draft.provider
                  }
                  onChange={(e) => {
                    const v = e.target.value;
                    if (v.startsWith("acp:")) {
                      setDraft((d) => ({ ...d, provider: "acp", acp_agent_id: v.slice(4) }));
                    } else {
                      setDraft((d) => ({
                        ...d,
                        provider: v as ProviderKind,
                        acp_agent_id: null,
                      }));
                    }
                  }}
                  className="border-border-default bg-surface-base text-ink-primary h-8 w-full rounded-lg border px-2.5 text-[12.5px]"
                >
                  <option value="acp:hermes">Hermes Agent (ACP · Recommended)</option>
                  <option value="claude_code">Claude Code</option>
                  <option value="codex">OpenAI Codex</option>
                  <option value="antigravity">Antigravity (agy)</option>
                </select>
              </label>

              <label className="space-y-1">
                <span className="text-ink-secondary text-[11.5px] font-medium">
                  Fleet Visibility Scope
                </span>
                <select
                  value={draft.scope}
                  onChange={(e) => setDraft((d) => ({ ...d, scope: e.target.value as RouterScope }))}
                  className="border-border-default bg-surface-base text-ink-primary h-8 w-full rounded-lg border px-2.5 text-[12.5px]"
                >
                  <option value="global_fleet">Global Multi-Host Fleet (Recommended)</option>
                  <option value="local_host_only">Local Host Only</option>
                </select>
              </label>

              <label className="space-y-1 sm:col-span-2">
                <span className="text-ink-secondary flex items-center gap-1 text-[11.5px] font-medium">
                  <ShieldCheck className="h-3.5 w-3.5 text-emerald-600 dark:text-emerald-400" />
                  3-Tier Guardian Pre-Review (Tier-3 High/Critical always requires human approval)
                </span>
                <select
                  value={draft.pre_review_mode}
                  onChange={(e) =>
                    setDraft((d) => ({
                      ...d,
                      pre_review_mode: e.target.value as RouterPreReviewMode,
                    }))
                  }
                  className="border-border-default bg-surface-base text-ink-primary h-8 w-full rounded-lg border px-2.5 text-[12.5px]"
                >
                  <option value="medium_risk_only">
                    Medium-risk pre-review by Router; High/Critical hard-blocked for Human (Recommended)
                  </option>
                  <option value="assist_only">
                    Assist Only (Router summarizes context; Human resolves all approvals)
                  </option>
                  <option value="disabled">Disabled (All approvals go directly to Human)</option>
                </select>
              </label>
            </div>

            <label className="flex items-center gap-2 text-[12.5px]">
              <input
                type="checkbox"
                checked={draft.share_host_hermes_memory}
                onChange={(e) =>
                  setDraft((d) => ({ ...d, share_host_hermes_memory: e.target.checked }))
                }
                className="rounded"
              />
              <span className="text-ink-primary">
                Share & interoperate with host{" "}
                <code className="bg-surface-recessed rounded px-1 font-mono text-[11px]">
                  ~/.hermes/memories/
                </code>{" "}
                and skills
              </span>
            </label>

            <label className="block space-y-1">
              <span className="text-ink-secondary text-[11.5px] font-medium">
                Custom Routing Discipline (Appended to Router SOUL)
              </span>
              <textarea
                rows={3}
                value={draft.soul_prompt_override ?? ""}
                onChange={(e) =>
                  setDraft((d) => ({
                    ...d,
                    soul_prompt_override: e.target.value || null,
                  }))
                }
                placeholder="e.g., Route Rust backend work to Claude Code on Devbox, frontend UI to Codex; keep updates concise."
                className="border-border-default bg-surface-base text-ink-primary w-full rounded-lg border px-2.5 py-2 text-[12px]"
              />
            </label>

            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label className="space-y-1">
                <span className="text-ink-secondary text-[11.5px] font-medium">
                  External Hermes Inbound Webhook URL (Optional)
                </span>
                <input
                  type="url"
                  value={draft.external_webhook_url ?? ""}
                  onChange={(e) =>
                    setDraft((d) => ({ ...d, external_webhook_url: e.target.value || null }))
                  }
                  placeholder="http://127.0.0.1:8642/webhooks/orbit"
                  className="border-border-default bg-surface-base text-ink-primary h-8 w-full rounded-lg border px-2.5 text-[12px]"
                />
              </label>
              <label className="space-y-1">
                <span className="text-ink-secondary text-[11.5px] font-medium">
                  Webhook Bearer Secret{" "}
                  {draft.has_external_webhook_secret ? "(Configured)" : "(Optional)"}
                </span>
                <input
                  type="password"
                  value={webhookSecretDraft}
                  onChange={(e) => setWebhookSecretDraft(e.target.value)}
                  placeholder="Leave blank to keep unchanged"
                  className="border-border-default bg-surface-base text-ink-primary h-8 w-full rounded-lg border px-2.5 text-[12px]"
                />
              </label>
            </div>

            <div className="flex flex-wrap items-center gap-2 pt-1">
              <Button
                type="button"
                size="sm"
                disabled={saving}
                onClick={() => void handleSave(true)}
              >
                <Sparkles className="h-3.5 w-3.5" />
                {saving ? "Activating..." : "Save & Activate Primary Router"}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={saving}
                onClick={() => void handleSave(false)}
              >
                Save Policy Only
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void handleExportBridge()}
              >
                <ExternalLink className="h-3.5 w-3.5" />
                Export External Hermes MCP Bridge
              </Button>
            </div>
          </>
        )}
      </div>

      {/* External Hermes Bridge Export */}
      {bridgeBundle && (
        <div className="border-border-subtle bg-surface-raised space-y-3 rounded-xl border p-4">
          <div className="text-ink-primary text-[13px] font-semibold">
            External Hermes Gateway Bidirectional MCP Bridge ({bridgeBundle.host_name})
          </div>
          <p className="text-ink-secondary text-[12px]">
            Add this block to{" "}
            <code className="bg-surface-recessed rounded px-1 font-mono">
              ~/.hermes/config.yaml
            </code>
            . Any Worker conversation or Mission Workspace created by external Hermes reflects live
            across Orbit Desktop and Mobile.
          </p>
          {[
            {
              key: "mcp",
              title: "1. Orbit Streamable HTTP MCP",
              body: bridgeBundle.mcp_yaml_snippet,
            },
            {
              key: "toolset",
              title: "2. Lightweight ACP Toolset Scoping",
              body: bridgeBundle.acp_toolset_yaml_snippet,
            },
          ].map((item) => (
            <div key={item.key} className="space-y-1">
              <div className="flex items-center justify-between">
                <span className="text-ink-secondary text-[11.5px] font-medium">{item.title}</span>
                <button
                  type="button"
                  onClick={() => void copyText(item.key, item.body)}
                  className="text-ink-muted hover:text-ink-primary inline-flex cursor-pointer items-center gap-1 text-[11.5px]"
                >
                  {copiedKey === item.key ? (
                    <>
                      <Check className="h-3 w-3 text-emerald-500" />
                      Copied
                    </>
                  ) : (
                    <>
                      <Copy className="h-3 w-3" />
                      Copy
                    </>
                  )}
                </button>
              </div>
              <pre className="bg-surface-recessed text-ink-primary overflow-x-auto rounded-lg p-2.5 font-mono text-[11px] leading-relaxed">
                {item.body}
              </pre>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
