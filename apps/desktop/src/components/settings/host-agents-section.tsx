import { useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Download,
  Link2,
  Plus,
  RefreshCw,
  Server,
  Sparkles,
  Trash2,
  Zap,
} from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  PROVIDER_LIST,
  formatProviderShortLabel,
  type AgentProfile,
  type OrchestrationConfig,
  type ProviderKind,
  type SessionMode,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type PresetSessionMode = "auto" | "accept_edits" | "plan" | "full_access" | "default";

const IDLE_TIMEOUT_OPTIONS: readonly SettingSelectOption<number>[] = [
  { value: 600, label: "10 min" },
  { value: 1200, label: "20 min" },
  { value: 1800, label: "30 min" },
  { value: 3600, label: "60 min" },
];

const PROFILE_MODE_OPTIONS: readonly SettingSelectOption<PresetSessionMode>[] = [
  { value: "auto", label: "Auto (Guardian)" },
  { value: "accept_edits", label: "Accept Edits" },
  { value: "plan", label: "Plan (Read-only)" },
  { value: "full_access", label: "Full Access" },
  { value: "default", label: "Ask" },
];

const DEFAULT_ORCH_CONFIG: OrchestrationConfig = {
  mcp_inject_enabled: true,
  idle_cleanup_enabled: true,
  idle_timeout_secs: 1800,
  resume_on_startup: false,
  resume_prompt: "Continue where you left off.",
  installed_skills: [],
  keep_awake_while_running: true,
};

export function HostAgentsSection({ hostId }: { hostId: string }) {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostCard = fleet.hosts.find((h) => h.host_id === hostId) ?? null;
  const otherFleetHostsCount = fleet.hosts.filter((h) => h.host_id !== hostId).length;
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const draftProvider = useOrbitSelector((s) => s.draftProvider);

  const [refreshingModels, setRefreshingModels] = useState(false);
  const [showUninstalledProviders, setShowUninstalledProviders] = useState(false);

  const showHostBadge =
    Boolean(hostCard) &&
    (fleet.hosts.length > 1 || (hostCard !== null && !isLocalHostCard(hostCard)));

  // New Profile form state
  const [addingProfile, setAddingProfile] = useState(false);
  const [profName, setProfName] = useState("");
  const [profProvider, setProfProvider] = useState<ProviderKind | null>(null);
  const [profModel, setProfModel] = useState("");
  const [profMode, setProfMode] = useState<SessionMode>("accept_edits");
  const [profWhenToUse, setProfWhenToUse] = useState("");
  const [profSystemPrompt, setProfSystemPrompt] = useState("");
  const [profDisabledTools, setProfDisabledTools] = useState("");
  const [profMcpEnabled, setProfMcpEnabled] = useState(true);
  const [profFastMode, setProfFastMode] = useState(false);

  // Connected Host form state
  const [peerUri, setPeerUri] = useState("");
  const [peerLabel, setPeerLabel] = useState("");

  // Destructive confirmation state
  const [pendingDeleteProfile, setPendingDeleteProfile] = useState<AgentProfile | null>(null);
  const [pendingRemovePeer, setPendingRemovePeer] = useState<{
    registration_id: string;
    label: string;
  } | null>(null);

  const available =
    hostDetail?.host.available_providers ?? hostCard?.available_providers ?? [];
  const installedProviders = PROVIDER_LIST.filter((p) => available.includes(p.id));
  const uninstalledProviders = PROVIDER_LIST.filter((p) => !available.includes(p.id));
  const effectiveProfProvider: ProviderKind =
    profProvider ?? installedProviders[0]?.id ?? "claude_code";
  const profileProviderOptions: SettingSelectOption<ProviderKind>[] =
    installedProviders.length > 0 && uninstalledProviders.length > 0
      ? [
          ...installedProviders.map((p) => ({
            value: p.id,
            label: p.label,
            group: "Installed",
          })),
          ...uninstalledProviders.map((p) => ({
            value: p.id,
            label: p.label,
            group: "Other Supported",
          })),
        ]
      : PROVIDER_LIST.map((p) => ({ value: p.id, label: p.label }));
  const visibleProviders =
    installedProviders.length === 0 || showUninstalledProviders
      ? [...installedProviders, ...uninstalledProviders]
      : installedProviders;
  const catalogs = hostDetail?.provider_catalogs ?? [];
  const orchConfig = hostDetail?.orchestration_config ?? DEFAULT_ORCH_CONFIG;
  const keepAwakeStatus = hostDetail?.keep_awake_status ?? null;
  const keepAwakeEnabled = orchConfig.keep_awake_while_running ?? true;
  const profiles = hostDetail?.profiles ?? [];
  const skills = hostDetail?.orchestration_skills ?? [];
  const connectedHosts = hostDetail?.connected_hosts ?? [];

  const handleRefreshModels = async () => {
    setRefreshingModels(true);
    try {
      await orbitIpc.refreshProviderModels(hostId, true);
      orbitActions.showToast("Refreshed model catalog");
    } finally {
      window.setTimeout(() => setRefreshingModels(false), 450);
    }
  };

  const patchOrchConfig = async (patch: Partial<OrchestrationConfig>) => {
    await orbitIpc.updateOrchestrationConfig(hostId, {
      ...orchConfig,
      ...patch,
    });
    orbitActions.showToast("Updated orchestration runtime settings");
  };

  const handleCreateProfile = async () => {
    if (!profName.trim()) {
      orbitActions.showToast("Profile name is required");
      return;
    }
    const now = new Date().toISOString();
    const disabled = profDisabledTools
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    const profile: AgentProfile = {
      profile_id: "",
      name: profName.trim(),
      provider: effectiveProfProvider,
      model: profModel.trim() || null,
      effort: null,
      fast_mode: profFastMode,
      mode: profMode,
      when_to_use: profWhenToUse.trim(),
      system_prompt_append: profSystemPrompt.trim() || null,
      mcp_policy: {
        enabled: profMcpEnabled,
        disabled_tools: disabled,
      },
      created_at: now,
      updated_at: now,
    };
    await orbitIpc.upsertProfile(hostId, profile);
    setProfName("");
    setProfModel("");
    setProfWhenToUse("");
    setProfSystemPrompt("");
    setProfDisabledTools("");
    setProfFastMode(false);
    setAddingProfile(false);
    orbitActions.showToast(`Saved profile "${profile.name}"`);
  };

  const handleToggleSkill = async (skillId: string, currentlyInstalled: boolean) => {
    const installedIds = new Set(skills.filter((s) => s.installed).map((s) => s.id));
    if (currentlyInstalled) {
      installedIds.delete(skillId);
    } else {
      installedIds.add(skillId);
    }
    await orbitIpc.syncOrchestrationSkills(hostId, Array.from(installedIds));
    orbitActions.showToast(
      currentlyInstalled ? `Removed /${skillId} skill` : `Installed /${skillId} skill`,
    );
  };

  const handleInstallAllSkills = async () => {
    const allIds = skills.map((s) => s.id);
    await orbitIpc.syncOrchestrationSkills(hostId, allIds);
    orbitActions.showToast(`Synced ${allIds.length} orchestration skills`);
  };

  const handleAddPeer = async () => {
    if (!peerUri.trim()) {
      orbitActions.showToast("Paste an orbit://pair/... URI");
      return;
    }
    await orbitIpc.addConnectedHost(hostId, peerUri.trim(), peerLabel.trim() || null);
    setPeerUri("");
    setPeerLabel("");
    orbitActions.showToast("Added connected host");
  };

  const handleSyncFleetHosts = async () => {
    const count = await orbitIpc.syncFleetConnectedHosts(hostId);
    orbitActions.showToast(
      count > 0
        ? `Synced ${count} fleet host(s) into daemon mesh`
        : "No other fleet hosts available to sync",
    );
  };

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Agents & Orchestration"
        description="Installed CLIs, built-in Orbit MCP orchestration, reusable Profiles, bundled Skills, and Cross-Host Mesh."
        badge={
          showHostBadge && hostCard ? (
            <span className="bg-muted text-muted-foreground rounded-md px-2 py-0.5 text-[11.5px] font-medium">
              {hostCard.display_name}
            </span>
          ) : null
        }
        action={
          <button
            type="button"
            disabled={refreshingModels}
            onClick={() => void handleRefreshModels()}
            className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
          >
            <RefreshCw
              className={cn("h-[13px] w-[13px]", refreshingModels && "animate-spin")}
            />
            <span>Refresh Models</span>
          </button>
        }
      />

      {/* 1. Installed CLIs */}
      <div className="space-y-2">
        <SettingGroupLabel>
          Installed CLIs ({installedProviders.length}/{PROVIDER_LIST.length})
        </SettingGroupLabel>
        <SettingCard>
          {visibleProviders.map((p) => {
            const detected = available.includes(p.id);
            const catalog = catalogs.find((c) => c.provider === p.id);
            const modelCount = catalog?.models.length ?? 0;
            const isDefault = detected && draftProvider === p.id;

            return (
              <SettingRow
                key={p.id}
                title={
                  <span className="flex items-center gap-2">
                    <ProviderIcon provider={p.id} className="size-4" />
                    <span>{p.label}</span>
                  </span>
                }
                description={
                  detected
                    ? `${p.bin}${modelCount > 0 ? ` · ${modelCount} models` : ""}`
                    : `${p.bin} not found on PATH`
                }
              >
                <div className="flex items-center gap-2">
                  {detected &&
                    (isDefault ? (
                      <span className="text-muted-foreground inline-flex h-[26px] items-center gap-1 rounded-md px-2 text-[11.5px] font-medium">
                        <Check className="h-[12.5px] w-[12.5px] stroke-[2]" />
                        <span>Default</span>
                      </span>
                    ) : (
                      <button
                        type="button"
                        onClick={() => orbitActions.setDraftConfig({ provider: p.id })}
                        className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] cursor-pointer items-center rounded-md px-2 text-[11.5px] font-medium transition-colors"
                      >
                        Set default
                      </button>
                    ))}

                  <span
                    className={cn(
                      "inline-flex h-[24px] items-center gap-1.5 rounded-md border px-2 text-[11.5px] font-medium",
                      detected
                        ? "border-emerald-500/25 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                        : "border-border text-muted-foreground",
                    )}
                  >
                    <span
                      className={cn(
                        "h-[5.5px] w-[5.5px] rounded-full",
                        detected ? "bg-emerald-500" : "bg-zinc-400",
                      )}
                    />
                    {detected ? "Ready" : "Not installed"}
                  </span>
                </div>
              </SettingRow>
            );
          })}

          {installedProviders.length > 0 && uninstalledProviders.length > 0 && (
            <button
              type="button"
              onClick={() => setShowUninstalledProviders((v) => !v)}
              className="text-muted-foreground hover:text-foreground hover:bg-accent/40 flex w-full cursor-pointer items-center justify-between px-4 py-2.5 text-left text-[12px] transition-colors"
            >
              <span>
                {showUninstalledProviders
                  ? "Hide uninstalled CLIs"
                  : `${uninstalledProviders.length} more supported CLIs (not installed)`}
              </span>
              {showUninstalledProviders ? (
                <ChevronDown className="size-3.5 opacity-70" />
              ) : (
                <ChevronRight className="size-3.5 opacity-70" />
              )}
            </button>
          )}
        </SettingCard>
      </div>

      {/* 2. Orchestration Runtime Settings */}
      <div className="space-y-2">
        <SettingGroupLabel>Orchestration & Runtime</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title="Built-in Orbit MCP Orchestrator"
            description="Auto-inject the local Streamable HTTP MCP server (/mcp/agents) into supported agent sessions so agents can delegate subagents, create worktrees, and schedule tasks."
          >
            <ToggleSwitch
              checked={orchConfig.mcp_inject_enabled}
              onChange={(next) => void patchOrchConfig({ mcp_inject_enabled: next })}
              aria-label="Built-in Orbit MCP Orchestrator"
            />
          </SettingRow>

          <SettingRow
            title="Idle Runtime Cleanup"
            description="Automatically release idle provider processes after inactivity while preserving full session resume state and protecting active subagent trees."
          >
            <div className="flex items-center gap-2.5">
              <SettingSelect<number>
                size="sm"
                value={orchConfig.idle_timeout_secs}
                options={IDLE_TIMEOUT_OPTIONS}
                disabled={!orchConfig.idle_cleanup_enabled}
                ariaLabel="Idle runtime cleanup timeout"
                triggerClassName="min-w-[88px]"
                menuClassName="min-w-[120px]"
                onChange={(secs) => void patchOrchConfig({ idle_timeout_secs: secs })}
              />
              <ToggleSwitch
                checked={orchConfig.idle_cleanup_enabled}
                onChange={(next) => void patchOrchConfig({ idle_cleanup_enabled: next })}
                aria-label="Idle Runtime Cleanup"
              />
            </div>
          </SettingRow>

          <SettingRow
            title="Auto-Resume on Daemon Startup"
            description="Automatically send a resume prompt to non-background agents that were mid-turn if the host rebooted."
          >
            <ToggleSwitch
              checked={orchConfig.resume_on_startup}
              onChange={(next) => void patchOrchConfig({ resume_on_startup: next })}
              aria-label="Auto-Resume on Daemon Startup"
            />
          </SettingRow>

          <SettingRow
            title={
              <span className="inline-flex items-center gap-2">
                <span>Keep awake while running</span>
                {keepAwakeStatus?.active && (
                  <span
                    title={
                      keepAwakeStatus.backend && keepAwakeStatus.backend !== "noop"
                        ? `Preventing idle sleep via ${keepAwakeStatus.backend}`
                        : "Preventing idle sleep"
                    }
                    className="inline-flex h-[20px] items-center gap-1.5 rounded-full border border-emerald-500/25 bg-emerald-500/10 px-2 text-[11px] font-medium text-emerald-600 dark:text-emerald-400"
                  >
                    <span className="h-[5px] w-[5px] rounded-full bg-emerald-500" />
                    <span>
                      Active
                      {keepAwakeStatus.active_agents > 0
                        ? ` · ${keepAwakeStatus.active_agents}`
                        : ""}
                    </span>
                  </span>
                )}
              </span>
            }
            description="Prevent system sleep while agents are active."
          >
            <ToggleSwitch
              checked={keepAwakeEnabled}
              onChange={(next) => void patchOrchConfig({ keep_awake_while_running: next })}
              aria-label="Keep awake while running"
            />
          </SettingRow>
        </SettingCard>
      </div>

      {/* 3. Agent Profiles */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <SettingGroupLabel>Agent Profiles ({profiles.length})</SettingGroupLabel>
          <button
            type="button"
            onClick={() => setAddingProfile((v) => !v)}
            className="border-border hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium"
          >
            <Plus className="h-[12px] w-[12px]" />
            <span>New Profile</span>
          </button>
        </div>

        {addingProfile && (
          <div className="border-border bg-elevated/70 space-y-3 rounded-xl border p-3.5 text-[12.5px]">
            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              <input
                value={profName}
                onChange={(e) => setProfName(e.target.value)}
                placeholder="Profile Name (e.g. Claude Architect)"
                className="border-border bg-canvas rounded-lg border px-2.5 py-1.5 outline-none"
              />
              <SettingSelect<ProviderKind>
                align="start"
                value={effectiveProfProvider}
                options={profileProviderOptions}
                ariaLabel="Profile provider"
                triggerClassName="w-full min-w-0 bg-canvas"
                onChange={(provider) => setProfProvider(provider)}
              />
              <input
                value={profModel}
                onChange={(e) => setProfModel(e.target.value)}
                placeholder="Model slug (optional)"
                className="border-border bg-canvas rounded-lg border px-2.5 py-1.5 outline-none"
              />
              <SettingSelect<PresetSessionMode>
                align="start"
                value={typeof profMode === "string" ? profMode : "accept_edits"}
                options={PROFILE_MODE_OPTIONS}
                ariaLabel="Profile permission mode"
                triggerClassName="w-full min-w-0 bg-canvas"
                onChange={(mode) => setProfMode(mode)}
              />
            </div>
            <input
              value={profWhenToUse}
              onChange={(e) => setProfWhenToUse(e.target.value)}
              placeholder="When to use (helps orchestrators pick this profile automatically)"
              className="border-border bg-canvas w-full rounded-lg border px-2.5 py-1.5 outline-none"
            />
            <textarea
              rows={2}
              value={profSystemPrompt}
              onChange={(e) => setProfSystemPrompt(e.target.value)}
              placeholder="System prompt append (optional)"
              className="border-border bg-canvas w-full rounded-lg border px-2.5 py-1.5 outline-none"
            />
            <input
              value={profDisabledTools}
              onChange={(e) => setProfDisabledTools(e.target.value)}
              placeholder="Disabled MCP tools (comma-separated, e.g. delete_agent,kill_terminal)"
              className="border-border bg-canvas w-full rounded-lg border px-2.5 py-1.5 font-mono text-[11.5px] outline-none"
            />
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex flex-wrap items-center gap-4">
                <div className="inline-flex items-center gap-2">
                  <ToggleSwitch
                    checked={profMcpEnabled}
                    onChange={setProfMcpEnabled}
                    ariaLabel="Inject Orbit MCP tools"
                  />
                  <span
                    className="cursor-pointer select-none"
                    onClick={() => setProfMcpEnabled((v) => !v)}
                  >
                    Inject Orbit MCP tools
                  </span>
                </div>
                <div
                  className="inline-flex items-center gap-2"
                  title="Lower latency, higher quota usage"
                >
                  <ToggleSwitch
                    checked={profFastMode}
                    onChange={setProfFastMode}
                    ariaLabel="Fast mode"
                  />
                  <span
                    className="cursor-pointer select-none"
                    onClick={() => setProfFastMode((v) => !v)}
                  >
                    Fast mode
                  </span>
                </div>
              </div>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setAddingProfile(false)}
                  className="border-border hover:bg-accent cursor-pointer rounded-lg border px-2.5 py-1"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  onClick={() => void handleCreateProfile()}
                  className="bg-foreground text-background cursor-pointer rounded-lg px-3 py-1 font-medium"
                >
                  Save Profile
                </button>
              </div>
            </div>
          </div>
        )}

        <SettingCard>
          {profiles.length === 0 ? (
            <div className="text-muted-foreground px-4 py-4 text-[12.5px]">
              No specialist profiles yet. Create profiles like &ldquo;Claude Architect&rdquo;,
              &ldquo;Codex Implementer&rdquo;, or &ldquo;Antigravity Reviewer&rdquo; so
              orchestrating agents can discover and delegate by role.
            </div>
          ) : (
            profiles.map((p) => (
              <SettingRow
                key={p.profile_id}
                title={
                  <span className="flex items-center gap-2">
                    <ProviderIcon provider={p.provider} className="size-3.5" />
                    <span>{p.name}</span>
                    {p.model && (
                      <code className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-[11px]">
                        {p.model}
                      </code>
                    )}
                    {p.fast_mode && (
                      <span className="inline-flex items-center gap-1 rounded-md border border-amber-500/30 bg-amber-500/10 px-1.5 py-0.5 text-[10.5px] font-medium text-amber-700 dark:border-amber-400/30 dark:bg-amber-400/12 dark:text-amber-300">
                        <Zap className="size-2.5 fill-current stroke-[2]" />
                        <span>Fast</span>
                      </span>
                    )}
                  </span>
                }
                description={
                  p.when_to_use ||
                  `Mode: ${typeof p.mode === "string" ? p.mode : "custom"} · MCP ${p.mcp_policy.enabled ? "on" : "off"}`
                }
              >
                <button
                  type="button"
                  aria-label={`Delete profile ${p.name}`}
                  onClick={() => setPendingDeleteProfile(p)}
                  className="text-muted-foreground hover:text-rose-500 hover:bg-accent cursor-pointer rounded-md p-1.5 transition-colors"
                >
                  <Trash2 className="h-[13.5px] w-[13.5px]" />
                </button>
              </SettingRow>
            ))
          )}
        </SettingCard>
      </div>

      {/* 4. Bundled Orchestration Skills */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <SettingGroupLabel>Orchestration Skills</SettingGroupLabel>
          {skills.length > 0 && (
            <button
              type="button"
              onClick={() => void handleInstallAllSkills()}
              className="border-border hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium"
            >
              <Download className="h-[12px] w-[12px]" />
              <span>Install / Update All</span>
            </button>
          )}
        </div>
        <SettingCard>
          {skills.map((s) => (
            <SettingRow
              key={s.id}
              title={
                <span className="flex items-center gap-2">
                  <Sparkles className="text-muted-foreground h-[13.5px] w-[13.5px]" />
                  <span>{s.name}</span>
                  <code className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-[11px]">
                    {s.slash_command}
                  </code>
                  {s.outdated && (
                    <span className="rounded bg-amber-500/15 px-1.5 py-0.5 text-[10.5px] text-amber-600 dark:text-amber-400">
                      Outdated
                    </span>
                  )}
                </span>
              }
              description={s.description}
            >
              <button
                type="button"
                onClick={() => void handleToggleSkill(s.id, s.installed && !s.outdated)}
                className={cn(
                  "cursor-pointer rounded-lg border px-2.5 py-1 text-[11.5px] font-medium transition-colors",
                  s.installed && !s.outdated
                    ? "border-emerald-500/25 bg-emerald-500/10 text-emerald-600 hover:bg-rose-500/10 hover:text-rose-600 dark:text-emerald-400"
                    : "border-border hover:bg-accent text-foreground",
                )}
              >
                {s.outdated ? "Update" : s.installed ? "Installed" : "Install"}
              </button>
            </SettingRow>
          ))}
        </SettingCard>
      </div>

      {/* 5. Connected Hosts (Cross-Host Orchestration Mesh) */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <SettingGroupLabel>Connected Hosts Mesh ({connectedHosts.length})</SettingGroupLabel>
          {otherFleetHostsCount > 0 && (
            <button
              type="button"
              onClick={() => void handleSyncFleetHosts()}
              className="border-border hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium"
            >
              <Link2 className="h-[12px] w-[12px]" />
              <span>Sync {otherFleetHostsCount} Fleet Host(s)</span>
            </button>
          )}
        </div>

        <SettingCard>
          {connectedHosts.length === 0 ? (
            <div className="text-muted-foreground px-4 py-3.5 text-[12.5px]">
              No peer daemons connected yet. Register another Orbit Daemon so agents on this host
              can delegate tasks across machines via <code className="text-[11.5px]">host_id</code>.
            </div>
          ) : (
            connectedHosts.map((peer) => (
              <SettingRow
                key={peer.registration_id}
                title={
                  <span className="flex items-center gap-2">
                    <Server className="text-muted-foreground h-[13.5px] w-[13.5px]" />
                    <span>{peer.label}</span>
                    <span
                      className={cn(
                        "rounded px-1.5 py-0.5 text-[10.5px] font-medium",
                        peer.status === "online"
                          ? "bg-emerald-500/12 text-emerald-600 dark:text-emerald-400"
                          : peer.status === "connecting"
                            ? "bg-blue-500/12 text-blue-600 dark:text-blue-400"
                            : peer.status === "error"
                              ? "bg-rose-500/12 text-rose-600 dark:text-rose-400"
                              : "bg-muted text-muted-foreground",
                      )}
                    >
                      {peer.status}
                    </span>
                  </span>
                }
                description={
                  peer.last_error
                    ? peer.last_error
                    : `${peer.hostname ?? peer.host_id ?? "pending handshake"}${peer.available_providers.length > 0 ? ` · ${peer.available_providers.map((p) => formatProviderShortLabel(p)).join(", ")}` : ""}`
                }
              >
                <div className="flex items-center gap-2.5">
                  <ToggleSwitch
                    checked={peer.enabled}
                    ariaLabel={`Enable connected peer ${peer.label}`}
                    onChange={(enabled) =>
                      void orbitIpc.updateConnectedHost(hostId, peer.registration_id, {
                        enabled,
                      })
                    }
                  />
                  <button
                    type="button"
                    aria-label={`Remove connected peer ${peer.label}`}
                    onClick={() =>
                      setPendingRemovePeer({
                        registration_id: peer.registration_id,
                        label: peer.label,
                      })
                    }
                    className="text-muted-foreground hover:text-rose-500 hover:bg-accent cursor-pointer rounded-md p-1.5 transition-colors"
                  >
                    <Trash2 className="h-[13.5px] w-[13.5px]" />
                  </button>
                </div>
              </SettingRow>
            ))
          )}

          <div className="border-border flex flex-wrap items-center gap-2 border-t px-4 py-3">
            <input
              value={peerLabel}
              onChange={(e) => setPeerLabel(e.target.value)}
              placeholder="Label (optional)"
              className="border-border bg-canvas w-36 rounded-lg border px-2.5 py-1.5 text-[12px] outline-none"
            />
            <input
              value={peerUri}
              onChange={(e) => setPeerUri(e.target.value)}
              placeholder="Paste orbit://pair/... URI from another daemon"
              className="border-border bg-canvas min-w-[200px] flex-1 rounded-lg border px-2.5 py-1.5 font-mono text-[12px] outline-none"
            />
            <button
              type="button"
              onClick={() => void handleAddPeer()}
              className="bg-foreground text-background cursor-pointer rounded-lg px-3 py-1.5 text-[12px] font-medium"
            >
              Add Peer
            </button>
          </div>
        </SettingCard>
      </div>

      <ConfirmDialog
        open={pendingDeleteProfile !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDeleteProfile(null);
        }}
        title={
          pendingDeleteProfile
            ? `Delete profile "${pendingDeleteProfile.name}"?`
            : "Delete agent profile?"
        }
        description="Orchestrating agents on this host will no longer be able to delegate tasks using this profile."
        confirmLabel="Delete profile"
        tone="danger"
        onConfirm={async () => {
          if (!pendingDeleteProfile) return;
          const target = pendingDeleteProfile;
          await orbitIpc.deleteProfile(hostId, target.profile_id);
          orbitActions.showToast(`Deleted profile "${target.name}"`);
        }}
      />

      <ConfirmDialog
        open={pendingRemovePeer !== null}
        onOpenChange={(open) => {
          if (!open) setPendingRemovePeer(null);
        }}
        title={
          pendingRemovePeer
            ? `Remove peer "${pendingRemovePeer.label}"?`
            : "Remove connected peer?"
        }
        description="Agents on this host will no longer be able to delegate tasks to this peer daemon."
        confirmLabel="Remove peer"
        tone="danger"
        onConfirm={async () => {
          if (!pendingRemovePeer) return;
          const target = pendingRemovePeer;
          await orbitIpc.removeConnectedHost(hostId, target.registration_id);
          orbitActions.showToast(`Removed peer "${target.label}"`);
        }}
      />
    </div>
  );
}
