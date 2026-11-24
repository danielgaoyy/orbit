import { useCallback, useEffect, useState } from "react";
import {
  Bot,
  Check,
  ChevronRight,
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
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { useI18n } from "@/lib/i18n";
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
  hostId?: string;
}

function formatHermesVersion(raw?: string | null): string {
  const trimmed = (raw ?? "").trim();
  if (!trimmed) return "ready";
  return trimmed.replace(/^(?:hermes\s+agent|hermes)\s+/i, "").trim() || trimmed;
}

export function HostPersonalRouterSection({ hostId }: HostPersonalRouterSectionProps) {
  const { t } = useI18n();
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
  const [advancedOpen, setAdvancedOpen] = useState(false);

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

  const effectiveHostId =
    primarySelection?.host_id ??
    hostId ??
    fleetHosts.find((h) => h.router_enabled)?.host_id ??
    fleetHosts.find((h) => h.online)?.host_id ??
    fleetHosts[0]?.host_id ??
    null;

  const scanFleetHosts = useCallback(async () => {
    setDiscovering(true);
    try {
      const [hosts, primary] = await Promise.all([
        orbitIpc.discoverFleetRouterHosts(),
        orbitIpc.getPrimaryRouterSelection(),
      ]);
      setFleetHosts(hosts);
      setPrimarySelection(primary);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setDiscovering(false);
    }
  }, []);

  const loadStatus = useCallback(async (targetHostId: string | null) => {
    if (!targetHostId) {
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const snap = await orbitIpc.getPersonalRouterStatus(targetHostId);
      setStatus(snap);
      setDraft(snap.config);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    setError(null);
    void scanFleetHosts();
  }, [scanFleetHosts]);

  useEffect(() => {
    void loadStatus(effectiveHostId);
  }, [effectiveHostId, loadStatus]);

  const handleEngineSwitch = async (value: string) => {
    if (!effectiveHostId) return;
    const provider: ProviderKind = value.startsWith("acp:") ? "acp" : (value as ProviderKind);
    const acpId = value.startsWith("acp:") ? value.slice(4) : null;
    const nextCfg: PersonalRouterConfig = {
      ...draft,
      enabled: true,
      provider,
      acp_agent_id: acpId,
      model: null,
      effort: null,
    };
    setDraft(nextCfg);
    setSaving(true);
    setError(null);
    try {
      const snap = await orbitIpc.configurePersonalRouter(effectiveHostId, nextCfg, true);
      setStatus(snap);
      setDraft(snap.config);
      if (snap.router_agent?.agent_id) {
        const nextPrimary = await orbitIpc.setPrimaryRouterSelection({
          host_id: effectiveHostId,
          agent_id: snap.router_agent.agent_id,
          updated_at_ms: Date.now(),
        });
        setPrimarySelection(nextPrimary);
      }
      void scanFleetHosts();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleSave = async (activate: boolean) => {
    if (!effectiveHostId) return;
    setSaving(true);
    setError(null);
    try {
      const nextCfg: PersonalRouterConfig = {
        ...draft,
        enabled: activate ? true : draft.enabled,
      };
      const snap = await orbitIpc.configurePersonalRouter(
        effectiveHostId,
        nextCfg,
        activate,
        webhookSecretDraft.trim() ? webhookSecretDraft.trim() : undefined,
      );
      setStatus(snap);
      setDraft(snap.config);
      setWebhookSecretDraft("");
      if (activate && snap.router_agent?.agent_id) {
        const nextPrimary = await orbitIpc.setPrimaryRouterSelection({
          host_id: effectiveHostId,
          agent_id: snap.router_agent.agent_id,
          updated_at_ms: Date.now(),
        });
        setPrimarySelection(nextPrimary);
        orbitActions.selectAgent(effectiveHostId, snap.router_agent.agent_id);
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
    const isAlreadyPrimary = primarySelection?.host_id === target.host_id;
    if (isAlreadyPrimary && target.active_router_agent_id) {
      orbitActions.selectAgent(target.host_id, target.active_router_agent_id);
      orbitActions.closeSettings();
      return;
    }

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
      setStatus(snap);
      setDraft(snap.config);
      if (snap.router_agent?.agent_id) {
        orbitActions.selectAgent(target.host_id, snap.router_agent.agent_id);
        orbitActions.closeSettings();
      }
      void scanFleetHosts();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActivatingHostId(null);
    }
  };

  const handleExportBridge = async () => {
    if (!effectiveHostId) return;
    try {
      const bundle = await orbitIpc.exportHermesBridgeConfig(effectiveHostId);
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
    <div className="space-y-5 pb-8">
      {/* Header */}
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-ink-primary flex items-center gap-2 text-[15px] font-semibold">
            <Sparkles className="text-ink-secondary h-4 w-4" />
            {t("settings.personalAgentTitle")}
          </h3>
          <p className="text-ink-secondary mt-1 text-[12.5px] leading-relaxed">
            {t("settings.personalAgentDesc")}
          </p>
        </div>
        {effectiveHostId && status?.router_agent?.agent_id && (
          <Button
            type="button"
            size="sm"
            onClick={() => {
              orbitActions.selectAgent(effectiveHostId, status.router_agent!.agent_id);
              orbitActions.closeSettings();
            }}
          >
            <Bot className="h-3.5 w-3.5" />
            {t("settings.personalAgentOpenSession")}
          </Button>
        )}
      </div>

      {error && (
        <div className="border-destructive/30 bg-destructive/10 text-destructive rounded-lg border px-3 py-2 text-[12px]">
          {error}
        </div>
      )}

      {/* One-Click Multi-Host Fleet Selector */}
      <div className="border-border-subtle bg-surface-raised rounded-xl border p-4">
        <div className="flex items-center justify-between gap-2">
          <div>
            <div className="text-ink-primary flex items-center gap-1.5 text-[13px] font-semibold">
              <Server className="text-ink-muted h-3.5 w-3.5" />
              {t("settings.personalAgentChooseHost")}
            </div>
            <p className="text-ink-secondary mt-0.5 text-[12px]">
              {t("settings.personalAgentChooseHostDesc")}
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
            {discovering
              ? t("settings.personalAgentScanning")
              : t("settings.personalAgentScanFleet")}
          </Button>
        </div>

        <div className="mt-3 space-y-2">
          {fleetHosts.length === 0 && !discovering && (
            <div className="text-ink-muted py-3 text-center text-[12px]">
              {t("settings.personalAgentNoHosts")}
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
                        {t("settings.personalAgentActiveBadge")}
                      </span>
                    )}
                    <span className="bg-surface-recessed text-ink-muted rounded-md px-1.5 py-0.5 text-[10.5px]">
                      {h.is_local ? t("common.local") : t("common.remote")}
                    </span>
                    {h.hermes_installed && (
                      <span className="bg-surface-recessed text-ink-secondary rounded-md px-1.5 py-0.5 font-mono text-[10.5px]">
                        Hermes {formatHermesVersion(h.hermes_version)}
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
                  {h.recommendation_reason && (
                    <div className="text-ink-muted mt-1 text-[11.5px]">
                      {h.recommendation_reason}
                    </div>
                  )}
                </div>
                <Button
                  type="button"
                  variant={isPrimary ? "outline" : "default"}
                  size="sm"
                  disabled={!h.online || isBusy}
                  onClick={() => void handleSelectPrimaryHost(h)}
                >
                  {isBusy
                    ? t("settings.personalAgentActivating")
                    : isPrimary && h.active_router_agent_id
                      ? t("settings.personalAgentOpenAction")
                      : t("settings.personalAgentUseAction")}
                </Button>
              </div>
            );
          })}
        </div>
      </div>

      {/* Compact Inline Engine Switcher */}
      {effectiveHostId && (
        <div className="border-border-subtle bg-surface-raised flex flex-wrap items-center justify-between gap-3 rounded-xl border px-4 py-3">
          <div className="min-w-0">
            <div className="text-ink-primary text-[12.5px] font-medium">
              {t("settings.personalAgentEngineLabel")}
            </div>
            <div className="text-ink-muted text-[11.5px]">
              {t("settings.personalAgentEngineHint")}
            </div>
          </div>
          <select
            disabled={loading || saving}
            value={
              draft.provider === "acp" ? `acp:${draft.acp_agent_id ?? "hermes"}` : draft.provider
            }
            onChange={(e) => void handleEngineSwitch(e.target.value)}
            className="border-border-default bg-surface-base text-ink-primary h-8 min-w-[220px] rounded-lg border px-2.5 text-[12.5px]"
          >
            <option value="acp:hermes">Hermes Agent (ACP · Recommended)</option>
            <option value="claude_code">Claude Code</option>
            <option value="codex">OpenAI Codex</option>
            <option value="antigravity">Antigravity (agy)</option>
          </select>
        </div>
      )}

      {/* Collapsed Advanced Policy, Memory & External Hermes Bridge */}
      {effectiveHostId && (
        <Collapsible
          open={advancedOpen}
          onOpenChange={setAdvancedOpen}
          className="border-border-subtle bg-surface-raised rounded-xl border px-4 py-3"
        >
          <CollapsibleTrigger className="group/adv text-ink-secondary hover:text-ink-primary flex w-full cursor-pointer items-center justify-between gap-2 text-left text-[12.5px] font-medium outline-none">
            <span>{t("settings.personalAgentAdvancedOptions")}</span>
            <ChevronRight className="text-ink-muted size-3.5 shrink-0 transition-transform duration-150 group-data-[panel-open]/adv:rotate-90" />
          </CollapsibleTrigger>

          <CollapsibleContent className="space-y-4 pt-4">
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label className="space-y-1">
                <span className="text-ink-secondary text-[11.5px] font-medium">
                  {t("settings.personalAgentScopeLabel")}
                </span>
                <select
                  value={draft.scope}
                  onChange={(e) =>
                    setDraft((d) => ({ ...d, scope: e.target.value as RouterScope }))
                  }
                  className="border-border-default bg-surface-base text-ink-primary h-8 w-full rounded-lg border px-2.5 text-[12.5px]"
                >
                  <option value="global_fleet">Global Multi-Host Fleet (Recommended)</option>
                  <option value="local_host_only">Local Host Only</option>
                </select>
              </label>

              <label className="space-y-1">
                <span className="text-ink-secondary flex items-center gap-1 text-[11.5px] font-medium">
                  <ShieldCheck className="h-3.5 w-3.5 text-emerald-600 dark:text-emerald-400" />
                  {t("settings.personalAgentGuardianLabel")}
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
                    Medium-risk pre-review by Router; High/Critical hard-blocked for Human
                  </option>
                  <option value="assist_only">
                    Assist Only (Router summarizes; Human resolves all)
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
              <span className="text-ink-primary">{t("settings.personalAgentShareHermesMemory")}</span>
            </label>

            <label className="block space-y-1">
              <span className="text-ink-secondary text-[11.5px] font-medium">
                {t("settings.personalAgentCustomDiscipline")}
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
                  {t("settings.personalAgentWebhookUrl")}
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
                  {t("settings.personalAgentWebhookSecret")}{" "}
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
                onClick={() => void handleSave(false)}
              >
                {t("settings.personalAgentSavePolicy")}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void handleExportBridge()}
              >
                <ExternalLink className="h-3.5 w-3.5" />
                {t("settings.personalAgentExportBridge")}
              </Button>
            </div>

            {bridgeBundle && (
              <div className="border-border-subtle bg-surface-base space-y-3 rounded-lg border p-3">
                <div className="text-ink-primary text-[12.5px] font-semibold">
                  External Hermes Gateway MCP Bridge ({bridgeBundle.host_name})
                </div>
                <p className="text-ink-secondary text-[12px]">
                  Add this block to{" "}
                  <code className="bg-surface-recessed rounded px-1 font-mono">
                    ~/.hermes/config.yaml
                  </code>
                  .
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
                      <span className="text-ink-secondary text-[11.5px] font-medium">
                        {item.title}
                      </span>
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
          </CollapsibleContent>
        </Collapsible>
      )}
    </div>
  );
}
