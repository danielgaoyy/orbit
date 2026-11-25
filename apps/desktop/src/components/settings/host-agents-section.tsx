import { useState } from "react";
import { Plus, RefreshCw, Trash2 } from "lucide-react";
import { HostAcpSection } from "./host-acp-section";
import {
  SettingCard,
  SettingDrawer,
  SettingGroup,
  SettingResourceRow,
  SettingRow,
  SettingPageHeader,
  SettingSelect,
  ToggleSwitch,
} from "./settings-primitives";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { useI18n } from "@/lib/i18n";
import {
  ACP_PROVIDER_META,
  PROVIDER_LIST,
  orbitIpc,
  type AgentProfile,
  type OrchestrationConfig,
  type ProviderKind,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function ProfileEditor({
  hostId,
  profile,
  initialProvider,
  close,
}: {
  hostId: string;
  profile: AgentProfile | null;
  initialProvider: ProviderKind;
  close: () => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<AgentProfile>(
    () =>
      profile ?? {
        profile_id: "",
        name: "",
        provider: initialProvider,
        model: null,
        effort: null,
        fast_mode: false,
        mode: "accept_edits",
        when_to_use: "",
        system_prompt_append: null,
        mcp_policy: { enabled: true, disabled_tools: [] },
        created_at: "",
        updated_at: "",
      },
  );
  const [open, setOpen] = useState(true);
  const [tools, setTools] = useState(draft.mcp_policy.disabled_tools.join(", "));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  function patch(change: Partial<AgentProfile>) {
    setDraft((previous) => ({ ...previous, ...change }));
  }
  const providerOptions = PROVIDER_LIST.map((provider) => ({
    value: provider.id,
    label: provider.label,
  }));
  if (!providerOptions.some((option) => option.value === draft.provider))
    providerOptions.push({ value: draft.provider, label: draft.acp_agent_id || draft.provider });
  const modeOptions = [
    { value: "default", label: t("composer.modeAlwaysAsk") },
    { value: "auto", label: t("composer.modeAuto") },
    { value: "accept_edits", label: t("composer.modeAcceptEdits") },
    { value: "plan", label: t("settings.agentPlan") },
    { value: "full_access", label: t("composer.modeFullAccess") },
  ] as const;
  async function save() {
    if (saving || !draft.name.trim()) return;
    setSaving(true);
    setError("");
    try {
      const now = new Date().toISOString();
      await orbitIpc.upsertProfile(hostId, {
        ...draft,
        name: draft.name.trim(),
        model: draft.model?.trim() || null,
        created_at: draft.created_at || now,
        updated_at: now,
        mcp_policy: {
          ...draft.mcp_policy,
          disabled_tools: tools
            .split(",")
            .map((name) => name.trim())
            .filter(Boolean),
        },
      });
      setOpen(false);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSaving(false);
    }
  }
  return (
    <SettingDrawer
      open={open}
      onOpenChange={(next) => {
        if (!saving) setOpen(next);
      }}
      onOpenChangeComplete={(visible) => {
        if (!visible) close();
      }}
      title={t(profile ? "settings.agentEditProfile" : "settings.agentNewProfile")}
    >
      <form
        className="grid gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <Field>
          <FieldLabel htmlFor="profile-name">{t("settings.agentName")}</FieldLabel>
          <Input
            id="profile-name"
            autoFocus
            required
            value={draft.name}
            onChange={(event) => patch({ name: event.target.value })}
          />
        </Field>
        <Field>
          <FieldLabel>{t("im.defaultProvider")}</FieldLabel>
          <SettingSelect
            align="start"
            ariaLabel={t("im.defaultProvider")}
            value={draft.provider}
            options={providerOptions}
            onChange={(provider) =>
              patch({ provider, acp_agent_id: provider === "acp" ? draft.acp_agent_id : null })
            }
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="profile-model">{t("settings.agentModel")}</FieldLabel>
          <Input
            id="profile-model"
            value={draft.model ?? ""}
            onChange={(event) => patch({ model: event.target.value })}
          />
        </Field>
        <Field>
          <FieldLabel>{t("composer.accessPermissions")}</FieldLabel>
          <SettingSelect
            align="start"
            ariaLabel={t("composer.accessPermissions")}
            value={typeof draft.mode === "string" ? draft.mode : null}
            placeholder={t("settings.agentCustomMode")}
            options={modeOptions}
            onChange={(mode) => patch({ mode })}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="profile-role">{t("settings.agentWhen")}</FieldLabel>
          <Input
            id="profile-role"
            value={draft.when_to_use}
            onChange={(event) => patch({ when_to_use: event.target.value })}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="profile-prompt">{t("settings.agentPrompt")}</FieldLabel>
          <Textarea
            id="profile-prompt"
            value={draft.system_prompt_append ?? ""}
            onChange={(event) => patch({ system_prompt_append: event.target.value || null })}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="profile-tools">{t("settings.agentDisabledTools")}</FieldLabel>
          <Input id="profile-tools" value={tools} onChange={(event) => setTools(event.target.value)} />
        </Field>
        <SettingCard>
          <SettingRow title={t("settings.agentMcpInjector")}>
            <ToggleSwitch
              ariaLabel={t("settings.agentMcpInjector")}
              checked={draft.mcp_policy.enabled}
              onChange={(enabled) => patch({ mcp_policy: { ...draft.mcp_policy, enabled } })}
            />
          </SettingRow>
          <SettingRow title={t("settings.agentFast")}>
            <ToggleSwitch
              ariaLabel={t("settings.agentFast")}
              checked={!!draft.fast_mode}
              onChange={(fast_mode) => patch({ fast_mode })}
            />
          </SettingRow>
        </SettingCard>
        {error && (
          <p role="alert" className="text-signal-danger">
            {error}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" disabled={saving} onClick={() => setOpen(false)}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" disabled={saving || !draft.name.trim()}>
            {t("common.save")}
          </Button>
        </div>
      </form>
    </SettingDrawer>
  );
}

function DefaultProviderRow({ available }: { available?: readonly ProviderKind[] }) {
  const { t } = useI18n();
  const provider = useOrbitSelector((s) => s.draftProvider);
  const acpAgentId = useOrbitSelector((s) => s.draftAcpAgentId);
  const options = PROVIDER_LIST.map((item) => ({
    value: item.id,
    label: item.label,
    disabled: available !== undefined && !available.includes(item.id),
  }));
  if (provider === "acp") {
    options.push({value: provider, label: acpAgentId ?? ACP_PROVIDER_META.label, disabled: true});
  }
  return (
    <SettingRow settingId="im.defaultProvider" title={t("im.defaultProvider")}>
      <SettingSelect
        value={provider}
        options={options}
        ariaLabel={t("im.defaultProvider")}
        onChange={(provider) => orbitActions.setDraftConfig({ provider })}
      />
    </SettingRow>
  );
}

export function HostAgentsSection({ hostId }: { hostId: string | null }) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <SettingPageHeader
        title={t("settings.navAgents")}
        description={t(hostId ? "settings.agentPageDesc" : "settings.hostNeeded")}
      />
      {hostId ? (
        <HostAgentControls key={hostId} hostId={hostId} />
      ) : (
        <>
          <SettingCard>
            <DefaultProviderRow />
          </SettingCard>
          <Button
            className="self-start"
            size="sm"
            variant="outline"
            onClick={() => orbitActions.openSettings("add_host")}
          >
            {t("settings.connectHost")}
          </Button>
        </>
      )}
    </div>
  );
}

function HostAgentControls({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const detail = useOrbitSelector((s) => s.hostDetails[hostId]);
  const host = useOrbitSelector((s) => s.fleet.hosts.find((card) => card.host_id === hostId));
  const [showAll, setShowAll] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editor, setEditor] = useState<AgentProfile | "new" | null>(null);
  const [deleting, setDeleting] = useState<AgentProfile | null>(null);
  const available = detail?.host.available_providers ?? host?.available_providers ?? [];
  const installed = PROVIDER_LIST.filter((provider) => available.includes(provider.id));
  const config = detail?.orchestration_config;
  const awake = detail?.keep_awake_status;
  const profiles = detail?.profiles ?? [];

  async function refresh() {
    setRefreshing(true);
    try {
      await orbitIpc.refreshProviderModels(hostId, true);
    } catch (error) {
      orbitActions.showToast(String(error));
    } finally {
      setRefreshing(false);
    }
  }
  async function update(patch: Partial<OrchestrationConfig>) {
    if (!config || saving) return;
    setSaving(true);
    try {
      await orbitIpc.updateOrchestrationConfig(hostId, { ...config, ...patch });
    } catch (error) {
      orbitActions.showToast(String(error));
    } finally {
      setSaving(false);
    }
  }
  const runtimeDisabled = !config || saving;
  return (
    <div className="flex flex-col gap-6">
      <SettingGroup
        title={t("settings.agentInstalledClis")}
        settingId="settings.agentInstalledClis"
        action={
          <Button size="sm" variant="outline" disabled={refreshing} onClick={() => void refresh()}>
            <RefreshCw className={cn(refreshing && "animate-spin")} />
            {t("settings.agentRefreshModels")}
          </Button>
        }
      >
        <SettingCard>
          {(showAll || !installed.length ? PROVIDER_LIST : installed).map((provider) => {
            const count =
              detail?.provider_catalogs?.find((catalog) => catalog.provider === provider.id)?.models
                .length ?? 0;
            return (
              <SettingResourceRow
                key={provider.id}
                icon={<ProviderIcon provider={provider.id} className="size-4" />}
                title={provider.label}
                description={provider.bin}
                status={
                  available.includes(provider.id)
                    ? count
                      ? t("settings.agentModelCount", { count })
                      : t("settings.agentInstalled")
                    : t("settings.agentNotInstalled")
                }
              />
            );
          })}
          {installed.length > 0 && installed.length < PROVIDER_LIST.length && (
            <Button variant="ghost" className="w-full" onClick={() => setShowAll((value) => !value)}>
              {t(showAll ? "common.hide" : "settings.agentShowAll")}
            </Button>
          )}
          <DefaultProviderRow available={available} />
        </SettingCard>
      </SettingGroup>
      <SettingGroup title={t("settings.subTabProvidersRuntime")}>
        <SettingCard>
          <SettingRow
            settingId="settings.agentMcpInjector"
            title={t("settings.agentMcpInjector")}
            description={t("settings.agentMcpDesc")}
          >
            <ToggleSwitch
              ariaLabel={t("settings.agentMcpInjector")}
              disabled={runtimeDisabled}
              checked={config?.mcp_inject_enabled ?? true}
              onChange={(mcp_inject_enabled) => void update({ mcp_inject_enabled })}
            />
          </SettingRow>
          <SettingRow
            settingId="settings.agentKeepAwake"
            title={t("settings.agentKeepAwake")}
            description={
              awake?.active
                ? t("settings.agentAwakeActive", { count: awake.active_agents })
                : t("settings.agentAwakeDesc")
            }
          >
            <ToggleSwitch
              ariaLabel={t("settings.agentKeepAwake")}
              disabled={runtimeDisabled}
              checked={config?.keep_awake_while_running ?? true}
              onChange={(keep_awake_while_running) => void update({ keep_awake_while_running })}
            />
          </SettingRow>
          <SettingRow
            settingId="settings.agentIdleCleanup"
            title={t("settings.agentIdleCleanup")}
            description={t("settings.agentIdleDesc")}
          >
            <div className="flex items-center gap-2">
              <SettingSelect
                size="sm"
                value={config?.idle_timeout_secs ?? 1800}
                disabled={runtimeDisabled || !config?.idle_cleanup_enabled}
                ariaLabel={t("settings.agentIdleCleanup")}
                options={[...new Set([600, 1200, 1800, 3600, config?.idle_timeout_secs ?? 1800])]
                  .sort((a, b) => a - b)
                  .map((value) => ({ value, label: t("settings.agentMinutes", { count: value / 60 }) }))}
                onChange={(idle_timeout_secs) => void update({ idle_timeout_secs })}
              />
              <ToggleSwitch
                disabled={runtimeDisabled}
                checked={config?.idle_cleanup_enabled ?? true}
                ariaLabel={t("settings.agentIdleCleanup")}
                onChange={(idle_cleanup_enabled) => void update({ idle_cleanup_enabled })}
              />
            </div>
          </SettingRow>
          <SettingRow
            settingId="settings.agentAutoResume"
            title={t("settings.agentAutoResume")}
            description={t("settings.agentResumeDesc")}
          >
            <ToggleSwitch
              ariaLabel={t("settings.agentAutoResume")}
              disabled={runtimeDisabled}
              checked={config?.resume_on_startup ?? false}
              onChange={(resume_on_startup) => void update({ resume_on_startup })}
            />
          </SettingRow>
        </SettingCard>
      </SettingGroup>
      <HostAcpSection hostId={hostId} />
      <SettingGroup
        title={t("settings.agentProfiles")}
        settingId="settings.agentProfiles"
        action={
          <Button size="sm" variant="outline" onClick={() => setEditor("new")}>
            <Plus />
            {t("settings.agentNewProfile")}
          </Button>
        }
      >
        <SettingCard>
          {!profiles.length && (
            <p className="px-4 py-3 text-[13px] text-ink-secondary">{t("settings.agentNoProfiles")}</p>
          )}
          {profiles.map((profile) => (
            <SettingResourceRow
              key={profile.profile_id}
              icon={<ProviderIcon provider={profile.provider} className="size-4" />}
              title={profile.name}
              description={profile.when_to_use}
              status={[profile.model, profile.fast_mode && t("settings.agentFast")]
                .filter(Boolean)
                .join(" · ")}
              onClick={() => setEditor(profile)}
              action={
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t("settings.agentDeleteProfile", { name: profile.name })}
                  onClick={() => setDeleting(profile)}
                >
                  <Trash2 />
                </Button>
              }
            />
          ))}
        </SettingCard>
      </SettingGroup>
      {editor && (
        <ProfileEditor
          profile={editor === "new" ? null : editor}
          initialProvider={installed[0]?.id ?? "claude_code"}
          hostId={hostId}
          close={() => setEditor(null)}
        />
      )}
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(open) => {
          if (!open) setDeleting(null);
        }}
        title={t("settings.agentDeleteProfile", { name: deleting?.name ?? "" })}
        description={t("settings.agentDeleteProfileDesc")}
        confirmLabel={t("common.delete")}
        tone="danger"
        onConfirm={async () => {
          if (deleting) await orbitIpc.deleteProfile(hostId, deleting.profile_id);
        }}
      />
    </div>
  );
}
