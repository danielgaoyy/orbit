import { useCallback, useEffect, useRef, useState } from "react";
import { Bot, Check, ChevronRight, Copy, ExternalLink, RefreshCw, Server } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  SettingCard,
  SettingGroup,
  SettingPageHeader,
  SettingResourceRow,
  SettingRow,
  SettingSelect,
  ToggleSwitch,
} from "./settings-primitives";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Field, FieldLabel } from "@/components/ui/field";
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
  const requestRef = useRef(0);

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
    const request = ++requestRef.current;
    setBridgeBundle(null);
    setWebhookSecretDraft("");
    setStatus(null);
    if (!targetHostId) {
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const snap = await orbitIpc.getPersonalRouterStatus(targetHostId);
      if (request !== requestRef.current) return;
      setStatus(snap);
      setDraft(snap.config);
    } catch (err) {
      if (request === requestRef.current) setError(String(err));
    } finally {
      if (request === requestRef.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    setError(null);
    void scanFleetHosts();
  }, [scanFleetHosts]);

  useEffect(() => {
    void loadStatus(effectiveHostId);
    return () => {
      requestRef.current++;
    };
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
    const isPrimary = primarySelection?.host_id === target.host_id;
    if (isPrimary && target.active_router_agent_id) {
      orbitActions.selectAgent(target.host_id, target.active_router_agent_id);
      orbitActions.closeSettings();
      return;
    }

    setActivatingHostId(target.host_id);
    setError(null);
    try {
      const existingSnap = await orbitIpc.getPersonalRouterStatus(target.host_id);
      const existing = existingSnap.config;
      if (isPrimary && existing.enabled && existingSnap.router_agent?.agent_id) {
        orbitActions.selectAgent(target.host_id, existingSnap.router_agent.agent_id);
        orbitActions.closeSettings();
        return;
      }

      const useFallbackProvider =
        !target.hermes_installed &&
        !existing.enabled &&
        !existing.router_agent_id &&
        existing.provider === "acp" &&
        (existing.acp_agent_id ?? "hermes") === "hermes";
      const provider: ProviderKind = useFallbackProvider
        ? (target.available_providers[0] ?? "claude_code")
        : existing.provider;
      const acpAgentId = useFallbackProvider ? null : (existing.acp_agent_id ?? null);

      const nextCfg: PersonalRouterConfig = {
        ...existing,
        enabled: true,
        router_agent_id: existing.router_agent_id ?? target.active_router_agent_id ?? null,
        provider,
        acp_agent_id: acpAgentId,
      };

      const snap = await orbitIpc.configurePersonalRouter(target.host_id, nextCfg, true);
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

  const engine = draft.provider === "acp" ? `acp:${draft.acp_agent_id ?? "hermes"}` : draft.provider;
  const engines = [
    { value: "acp:hermes", label: "Hermes Agent (ACP)" },
    { value: "claude_code", label: "Claude Code" },
    { value: "codex", label: "OpenAI Codex" },
    { value: "antigravity", label: "Antigravity (agy)" },
  ];
  if (!engines.some((option) => option.value === engine)) engines.push({ value: engine, label: engine });
  const pending = saving || loading || !!activatingHostId;
  return (
    <div className="flex flex-col gap-6">
      <SettingPageHeader
        title={t("settings.personalAgentTitle")}
        description={t("settings.personalAgentDesc")}
        action={
          effectiveHostId &&
          status?.router_agent?.agent_id && (
            <Button
              size="sm"
              variant="outline"
              disabled={pending}
              onClick={() => {
                if (status.router_agent?.agent_id) {
                  orbitActions.selectAgent(effectiveHostId, status.router_agent.agent_id);
                  orbitActions.closeSettings();
                }
              }}
            >
              <Bot />
              {t("settings.personalAgentOpenSession")}
            </Button>
          )
        }
      />
      {error && (
        <p
          role="alert"
          className="rounded-lg border border-signal-danger-border bg-signal-danger-bg p-3 text-[13px] text-signal-danger"
        >
          {error}
        </p>
      )}
      <SettingGroup
        title={t("settings.personalAgentChooseHost")}
        settingId="settings.personalAgentChooseHost"
        action={
          <Button
            size="sm"
            variant="outline"
            disabled={discovering || pending}
            onClick={() => void scanFleetHosts()}
          >
            <RefreshCw className={cn(discovering && "animate-spin")} />
            {t(discovering ? "settings.personalAgentScanning" : "settings.personalAgentScanFleet")}
          </Button>
        }
      >
        <p className="text-[13px] text-ink-secondary">{t("settings.personalAgentChooseHostDesc")}</p>
        <SettingCard>
          {!fleetHosts.length && (
            <p className="px-4 py-3 text-[13px] text-ink-muted">
              {t(discovering ? "common.loading" : "settings.personalAgentNoHosts")}
            </p>
          )}
          {fleetHosts.map((host) => {
            const primary = primarySelection?.host_id === host.host_id;
            const capabilities = [
              t(host.is_local ? "common.local" : "common.remote"),
              host.hermes_installed && `Hermes ${formatHermesVersion(host.hermes_version)}`,
              host.hermes_has_existing_memory && "~/.hermes/memories",
              host.hermes_gateway_running && t("settings.agentGatewayActive"),
              host.recommendation_reason,
            ]
              .filter(Boolean)
              .join(" · ");
            return (
              <SettingResourceRow
                key={host.host_id}
                icon={
                  <Server
                    className={cn("size-[15px]", host.online ? "text-ink-secondary" : "text-ink-faint")}
                  />
                }
                title={host.host_name}
                description={capabilities}
                status={
                  primary
                    ? t("settings.personalAgentActiveBadge")
                    : t(host.online ? "common.online" : "common.offline")
                }
                action={
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={!host.online || pending}
                    onClick={() => void handleSelectPrimaryHost(host)}
                  >
                    {t(
                      activatingHostId === host.host_id
                        ? "settings.personalAgentActivating"
                        : primary && host.active_router_agent_id
                          ? "settings.personalAgentOpenAction"
                          : "settings.personalAgentUseAction",
                    )}
                  </Button>
                }
              />
            );
          })}
        </SettingCard>
      </SettingGroup>
      {effectiveHostId && (
        <>
          <SettingCard>
            <SettingRow
              settingId="settings.personalAgentEngineLabel"
              title={t("settings.personalAgentEngineLabel")}
              description={t("settings.personalAgentEngineHint")}
            >
              <SettingSelect
                disabled={pending || !status}
                value={engine}
                options={engines}
                ariaLabel={t("settings.personalAgentEngineLabel")}
                onChange={(value) => void handleEngineSwitch(value)}
              />
            </SettingRow>
          </SettingCard>
          <details className="group/router rounded-xl border border-border-subtle bg-surface-raised">
            <summary
              data-setting-id="settings.personalAgentAdvancedOptions"
              className="flex cursor-pointer list-none items-center justify-between gap-2 px-4 py-3 text-[13px] font-medium [&::-webkit-details-marker]:hidden"
            >
              {t("settings.personalAgentAdvancedOptions")}
              <ChevronRight className="size-[14px] text-ink-muted group-open/router:rotate-90" />
            </summary>
            <fieldset
              disabled={pending || !status}
              className="space-y-3 border-t border-border-subtle px-4 py-3"
            >
              <SettingRow title={t("settings.personalAgentScopeLabel")}>
                <SettingSelect<RouterScope>
                  value={draft.scope}
                  ariaLabel={t("settings.personalAgentScopeLabel")}
                  options={[
                    { value: "global_fleet", label: t("settings.agentFleetScope") },
                    { value: "local_host_only", label: t("settings.agentLocalScope") },
                  ]}
                  onChange={(scope) => setDraft((d) => ({ ...d, scope }))}
                />
              </SettingRow>
              <SettingRow title={t("settings.personalAgentGuardianLabel")}>
                <SettingSelect<RouterPreReviewMode>
                  value={draft.pre_review_mode}
                  ariaLabel={t("settings.personalAgentGuardianLabel")}
                  options={[
                    { value: "medium_risk_only", label: t("settings.agentReviewMedium") },
                    { value: "assist_only", label: t("settings.agentAssistOnly") },
                    { value: "disabled", label: t("settings.agentReviewDisabled") },
                  ]}
                  onChange={(pre_review_mode) => setDraft((d) => ({ ...d, pre_review_mode }))}
                />
              </SettingRow>
              <SettingRow title={t("settings.personalAgentShareHermesMemory")}>
                <ToggleSwitch
                  checked={draft.share_host_hermes_memory}
                  ariaLabel={t("settings.personalAgentShareHermesMemory")}
                  onChange={(share_host_hermes_memory) =>
                    setDraft((d) => ({ ...d, share_host_hermes_memory }))
                  }
                />
              </SettingRow>
              <Field>
                <FieldLabel htmlFor="router-discipline">
                  {t("settings.personalAgentCustomDiscipline")}
                </FieldLabel>
                <Textarea
                  id="router-discipline"
                  value={draft.soul_prompt_override ?? ""}
                  onChange={(event) =>
                    setDraft((d) => ({ ...d, soul_prompt_override: event.target.value || null }))
                  }
                  rows={3}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="router-webhook">{t("settings.personalAgentWebhookUrl")}</FieldLabel>
                <Input
                  id="router-webhook"
                  value={draft.external_webhook_url ?? ""}
                  type="url"
                  placeholder="http://127.0.0.1:8642/webhooks/orbit"
                  onChange={(event) =>
                    setDraft((d) => ({ ...d, external_webhook_url: event.target.value || null }))
                  }
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="router-secret">
                  {t("settings.personalAgentWebhookSecret")}
                </FieldLabel>
                <Input
                  id="router-secret"
                  type="password"
                  autoComplete="new-password"
                  value={webhookSecretDraft}
                  placeholder={draft.has_external_webhook_secret ? "••••••••" : undefined}
                  onChange={(event) => setWebhookSecretDraft(event.target.value)}
                />
              </Field>
              <div className="flex flex-wrap gap-2">
                <Button size="sm" onClick={() => void handleSave(false)}>
                  {t("settings.personalAgentSavePolicy")}
                </Button>
                <Button variant="outline" size="sm" onClick={() => void handleExportBridge()}>
                  <ExternalLink />
                  {t("settings.personalAgentExportBridge")}
                </Button>
              </div>
              {bridgeBundle && (
                <div className="space-y-3 rounded-lg bg-surface-recessed p-3">
                  <div className="text-[13px] font-medium">
                    {bridgeBundle.host_name} · ~/.hermes/config.yaml
                  </div>
                  {[
                    { key: "mcp", title: "Streamable HTTP MCP", body: bridgeBundle.mcp_yaml_snippet },
                    { key: "toolset", title: "ACP", body: bridgeBundle.acp_toolset_yaml_snippet },
                  ].map((item) => (
                    <div key={item.key}>
                      <div className="flex items-center justify-between text-[11px]">
                        <span>{item.title}</span>
                        <Button
                          size="icon-sm"
                          variant="ghost"
                          aria-label={t("common.copy")}
                          onClick={() => void copyText(item.key, item.body)}
                        >
                          {copiedKey === item.key ? <Check /> : <Copy />}
                        </Button>
                      </div>
                      <pre className="overflow-x-auto font-mono text-[11px]">{item.body}</pre>
                    </div>
                  ))}
                </div>
              )}
            </fieldset>
          </details>
        </>
      )}
    </div>
  );
}
