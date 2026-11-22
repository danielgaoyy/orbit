import { useEffect, useMemo, useState } from "react";
import {
  Check,
  Copy,
  Loader2,
  MessageSquare,
  Pencil,
  Plus,
  RefreshCw,
  Send,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import {
  type ImChannelRecord,
  type ImConnectionStatus,
  type ImPlatform,
  type ImThreadBinding,
  PROVIDER_LIST,
  type ProviderKind,
  type SessionMode,
  type UpsertImChannelRequest,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_CHANNELS: readonly ImChannelRecord[] = [];
const EMPTY_BINDINGS: readonly ImThreadBinding[] = [];

const PLATFORM_META: Record<
  ImPlatform,
  {
    label: string;
    badgeClass: string;
    transportHint: string;
    showAppId: boolean;
    appIdLabel: string;
    appIdPlaceholder: string;
    primaryLabel: string;
    primaryPlaceholder: string;
    signingLabel: string;
    signingPlaceholder: string;
    signingRequiredHint: string;
    chatPlaceholder: string;
    userPlaceholder: string;
  }
> = {
  lark: {
    label: "Lark / 飞书",
    badgeClass: "bg-sky-500/10 text-sky-700 dark:text-sky-300",
    transportHint: "Webhook callback + Tenant API. Prefix App ID with lark: for open.larksuite.com.",
    showAppId: true,
    appIdLabel: "App ID",
    appIdPlaceholder: "cli_a1b2c3d4... (or lark:cli_...)",
    primaryLabel: "App Secret",
    primaryPlaceholder: "Lark / Feishu App Secret",
    signingLabel: "Verification Token / Encrypt Key (required for webhooks)",
    signingPlaceholder: "Verification Token or Encrypt Key",
    signingRequiredHint: "Verified in constant time on every inbound Lark callback.",
    chatPlaceholder: "oc_7c9a...",
    userPlaceholder: "ou_3f8b...",
  },
  telegram: {
    label: "Telegram",
    badgeClass: "bg-blue-500/10 text-blue-700 dark:text-blue-300",
    transportHint: "Built-in long-polling works behind NAT with no public URL.",
    showAppId: false,
    appIdLabel: "",
    appIdPlaceholder: "",
    primaryLabel: "Bot Token",
    primaryPlaceholder: "123456789:AAH...",
    signingLabel: "Webhook Secret Token (optional)",
    signingPlaceholder: "Only needed if using webhook mode",
    signingRequiredHint: "Optional X-Telegram-Bot-Api-Secret-Token for public webhooks.",
    chatPlaceholder: "-1001234567890",
    userPlaceholder: "123456789",
  },
  slack: {
    label: "Slack",
    badgeClass: "bg-violet-500/10 text-violet-700 dark:text-violet-300",
    transportHint: "Events API + Interactivity webhook with X-Slack-Signature verification.",
    showAppId: false,
    appIdLabel: "",
    appIdPlaceholder: "",
    primaryLabel: "Bot Token",
    primaryPlaceholder: "xoxb-...",
    signingLabel: "Signing Secret (required for webhooks)",
    signingPlaceholder: "Slack App Signing Secret",
    signingRequiredHint: "HMAC-SHA256 + 5-minute replay window enforced.",
    chatPlaceholder: "C0123456789",
    userPlaceholder: "U0123456789",
  },
  discord: {
    label: "Discord",
    badgeClass: "bg-indigo-500/10 text-indigo-700 dark:text-indigo-300",
    transportHint: "Polls allowlisted channel IDs behind NAT, or accepts signed webhooks.",
    showAppId: false,
    appIdLabel: "",
    appIdPlaceholder: "",
    primaryLabel: "Bot Token",
    primaryPlaceholder: "Discord Bot Token",
    signingLabel: "Webhook Secret (optional)",
    signingPlaceholder: "Optional X-Orbit-IM-Secret header",
    signingRequiredHint: "Required only for inbound HTTP webhooks.",
    chatPlaceholder: "123456789012345678",
    userPlaceholder: "987654321098765432",
  },
};

const PLATFORM_OPTIONS: SettingSelectOption<ImPlatform>[] = [
  { value: "lark", label: "Lark / 飞书" },
  { value: "telegram", label: "Telegram" },
  { value: "slack", label: "Slack" },
  { value: "discord", label: "Discord" },
];

type PresetSessionMode = "default" | "accept_edits" | "auto" | "plan" | "full_access";

const MODE_OPTIONS: SettingSelectOption<PresetSessionMode>[] = [
  { value: "default", label: "Default" },
  { value: "accept_edits", label: "Accept Edits" },
  { value: "auto", label: "Auto (Guardian)" },
  { value: "plan", label: "Plan" },
  { value: "full_access", label: "Full Access" },
];

function toPresetSessionMode(mode: SessionMode): PresetSessionMode {
  if (
    mode === "default" ||
    mode === "accept_edits" ||
    mode === "auto" ||
    mode === "plan" ||
    mode === "full_access"
  ) {
    return mode;
  }
  return "default";
}

function mergeDraftIntoIds(values: string[], draft: string): string[] {
  const pieces = draft
    .split(/[\s,]+/)
    .map((s) => s.trim())
    .filter(Boolean);
  if (pieces.length === 0) return values;
  const next = [...values];
  for (const p of pieces) {
    if (!next.includes(p)) next.push(p);
  }
  return next;
}

function statusDotClass(status: ImConnectionStatus, enabled: boolean): string {
  if (!enabled) return "bg-muted-foreground/35";
  switch (status) {
    case "connected":
      return "bg-emerald-500";
    case "connecting":
      return "bg-amber-500 animate-pulse";
    case "error":
      return "bg-rose-500";
    default:
      return "bg-muted-foreground/45";
  }
}

function statusLabel(status: ImConnectionStatus, enabled: boolean): string {
  if (!enabled) return "Disabled";
  switch (status) {
    case "connected":
      return "Connected";
    case "connecting":
      return "Connecting";
    case "error":
      return "Error";
    default:
      return "Idle";
  }
}

function IdChipInput({
  label,
  placeholder,
  values,
  draft,
  onDraftChange,
  onChange,
}: {
  label: string;
  placeholder: string;
  values: string[];
  draft: string;
  onDraftChange: (next: string) => void;
  onChange: (next: string[]) => void;
}) {
  const flushDraft = () => {
    const merged = mergeDraftIntoIds(values, draft);
    if (merged !== values) {
      onChange(merged);
    }
    if (draft.trim()) {
      onDraftChange("");
    }
  };

  return (
    <div className="space-y-1.5">
      <label className="text-foreground block text-[12px] font-medium">{label}</label>
      <div className="flex items-center gap-1.5">
        <input
          type="text"
          value={draft}
          onChange={(e) => onDraftChange(e.target.value)}
          onBlur={flushDraft}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing || e.keyCode === 229) return;
            if (e.key === "Enter") {
              e.preventDefault();
              flushDraft();
            }
          }}
          placeholder={placeholder}
          className="border-border bg-background text-foreground placeholder:text-muted-foreground/55 focus:border-foreground/35 h-[30px] flex-1 rounded-lg border px-2.5 font-mono text-[12px] outline-none"
        />
        <button
          type="button"
          onClick={flushDraft}
          disabled={!draft.trim()}
          className="border-border bg-background text-foreground hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40"
        >
          <Plus className="h-[12px] w-[12px]" />
          Add
        </button>
      </div>
      {values.length > 0 && (
        <div className="flex flex-wrap gap-1 pt-0.5">
          {values.map((id) => (
            <span
              key={id}
              className="border-border bg-muted/40 text-foreground inline-flex items-center gap-1 rounded-md border px-2 py-0.5 font-mono text-[11px]"
            >
              <span>{id}</span>
              <button
                type="button"
                aria-label={`Remove ${id}`}
                onClick={() => onChange(values.filter((x) => x !== id))}
                className="text-muted-foreground hover:text-foreground cursor-pointer"
              >
                <X className="h-[11px] w-[11px]" />
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

interface ChannelFormState {
  channelId: string | null;
  name: string;
  platform: ImPlatform;
  enabled: boolean;
  appId: string;
  primarySecret: string;
  signingSecret: string;
  clearExistingSecrets: boolean;
  hasExistingPrimary: boolean;
  hasExistingSigning: boolean;
  maskedPrimary: string | null;
  maskedSigning: string | null;
  allowedChatIds: string[];
  allowedUserIds: string[];
  defaultWorkspaceCwd: string;
  defaultProvider: ProviderKind;
  defaultMode: SessionMode;
  forwardApprovals: boolean;
}

function ChannelModal({
  initial,
  workspaceOptions,
  saving,
  serverError,
  onClose,
  onSubmit,
}: {
  initial: ChannelFormState;
  workspaceOptions: string[];
  saving: boolean;
  serverError: string | null;
  onClose: () => void;
  onSubmit: (req: UpsertImChannelRequest) => Promise<void>;
}) {
  const [form, setForm] = useState<ChannelFormState>(initial);
  const [chatDraft, setChatDraft] = useState("");
  const [userDraft, setUserDraft] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const meta = PLATFORM_META[form.platform];
  const isEditing = Boolean(form.channelId);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.isComposing || e.keyCode === 229 || saving) return;
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose, saving]);

  const providerOptions: SettingSelectOption<ProviderKind>[] = useMemo(
    () =>
      PROVIDER_LIST.map((p) => ({
        value: p.id,
        label: p.label,
      })),
    [],
  );

  const handleSave = async () => {
    setLocalError(null);
    if (!form.name.trim()) {
      setLocalError("Channel name is required.");
      return;
    }
    if (!form.defaultWorkspaceCwd.trim()) {
      setLocalError("Default workspace path is required.");
      return;
    }

    // Flush any pending un-submitted IDs typed into chip inputs
    const finalChatIds = mergeDraftIntoIds(form.allowedChatIds, chatDraft);
    const finalUserIds = mergeDraftIntoIds(form.allowedUserIds, userDraft);
    if (chatDraft.trim()) setChatDraft("");
    if (userDraft.trim()) setUserDraft("");
    if (finalChatIds !== form.allowedChatIds || finalUserIds !== form.allowedUserIds) {
      setForm((f) => ({
        ...f,
        allowedChatIds: finalChatIds,
        allowedUserIds: finalUserIds,
      }));
    }

    await onSubmit({
      channel_id: form.channelId,
      name: form.name.trim(),
      platform: form.platform,
      enabled: form.enabled,
      app_id: meta.showAppId ? form.appId.trim() || null : null,
      primary_secret: form.primarySecret.trim() || null,
      signing_secret: form.signingSecret.trim() || null,
      keep_existing_secrets: !form.clearExistingSecrets,
      allowed_chat_ids: finalChatIds,
      allowed_user_ids: finalUserIds,
      default_workspace_cwd: form.defaultWorkspaceCwd.trim(),
      default_provider: form.defaultProvider,
      default_mode: form.defaultMode,
      forward_approvals: form.forwardApprovals,
    });
  };

  const displayError = localError ?? serverError;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4 backdrop-blur-[2px]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !saving) onClose();
      }}
    >
      <div className="border-border bg-background flex max-h-[86vh] w-full max-w-[520px] flex-col overflow-hidden rounded-[14px] border shadow-2xl">
        <div className="border-border flex items-center justify-between border-b px-5 py-3.5">
          <div>
            <h3 className="text-foreground text-[14.5px] font-semibold">
              {isEditing ? "Edit IM Channel" : "Add IM Channel"}
            </h3>
            <p className="text-muted-foreground mt-0.5 text-[11.5px]">{meta.transportHint}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={saving}
            className="text-muted-foreground hover:text-foreground hover:bg-accent flex h-7 w-7 cursor-pointer items-center justify-center rounded-lg transition-colors"
            aria-label="Close"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="space-y-4 overflow-y-auto px-5 py-4">
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <label className="text-foreground block text-[12px] font-medium">Platform</label>
              <SettingSelect
                value={form.platform}
                options={PLATFORM_OPTIONS}
                disabled={isEditing || saving}
                triggerClassName="w-full"
                onChange={(platform) => setForm((f) => ({ ...f, platform }))}
              />
            </div>

            <div className="space-y-1.5">
              <label className="text-foreground block text-[12px] font-medium">Channel Name</label>
              <input
                type="text"
                value={form.name}
                disabled={saving}
                onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
                placeholder={`e.g. ${meta.label} Ops`}
                className="border-border bg-background text-foreground placeholder:text-muted-foreground/55 focus:border-foreground/35 h-[30px] w-full rounded-lg border px-2.5 text-[12.5px] outline-none"
              />
            </div>
          </div>

          <div className="border-border bg-muted/20 space-y-3 rounded-xl border p-3.5">
            <div className="flex items-center justify-between gap-2">
              <span className="text-foreground text-[12px] font-medium">Credentials</span>
              {isEditing && (form.hasExistingPrimary || form.hasExistingSigning) && (
                <label className="text-muted-foreground flex cursor-pointer items-center gap-1.5 text-[11px]">
                  <input
                    type="checkbox"
                    checked={form.clearExistingSecrets}
                    onChange={(e) =>
                      setForm((f) => ({ ...f, clearExistingSecrets: e.target.checked }))
                    }
                    className="rounded"
                  />
                  Overwrite / clear saved secrets
                </label>
              )}
            </div>

            {meta.showAppId && (
              <div className="space-y-1">
                <label className="text-muted-foreground block text-[11.5px] font-medium">
                  {meta.appIdLabel}
                </label>
                <input
                  type="text"
                  value={form.appId}
                  disabled={saving}
                  onChange={(e) => setForm((f) => ({ ...f, appId: e.target.value }))}
                  placeholder={meta.appIdPlaceholder}
                  className="border-border bg-background text-foreground placeholder:text-muted-foreground/55 h-[30px] w-full rounded-lg border px-2.5 font-mono text-[12px] outline-none"
                />
              </div>
            )}

            <div className="space-y-1">
              <div className="flex items-center justify-between">
                <label className="text-muted-foreground text-[11.5px] font-medium">
                  {meta.primaryLabel}
                </label>
                {isEditing && form.maskedPrimary && !form.clearExistingSecrets && (
                  <span className="text-muted-foreground font-mono text-[10.5px]">
                    Saved: {form.maskedPrimary}
                  </span>
                )}
              </div>
              <input
                type="password"
                value={form.primarySecret}
                disabled={saving}
                onChange={(e) => setForm((f) => ({ ...f, primarySecret: e.target.value }))}
                placeholder={
                  isEditing && form.hasExistingPrimary && !form.clearExistingSecrets
                    ? "Leave blank to keep saved secret"
                    : meta.primaryPlaceholder
                }
                className="border-border bg-background text-foreground placeholder:text-muted-foreground/55 h-[30px] w-full rounded-lg border px-2.5 font-mono text-[12px] outline-none"
              />
            </div>

            <div className="space-y-1">
              <div className="flex items-center justify-between">
                <label className="text-muted-foreground text-[11.5px] font-medium">
                  {meta.signingLabel}
                </label>
                {isEditing && form.maskedSigning && !form.clearExistingSecrets && (
                  <span className="text-muted-foreground font-mono text-[10.5px]">
                    Saved: {form.maskedSigning}
                  </span>
                )}
              </div>
              <input
                type="password"
                value={form.signingSecret}
                disabled={saving}
                onChange={(e) => setForm((f) => ({ ...f, signingSecret: e.target.value }))}
                placeholder={
                  isEditing && form.hasExistingSigning && !form.clearExistingSecrets
                    ? "Leave blank to keep saved secret"
                    : meta.signingPlaceholder
                }
                className="border-border bg-background text-foreground placeholder:text-muted-foreground/55 h-[30px] w-full rounded-lg border px-2.5 font-mono text-[12px] outline-none"
              />
              <p className="text-muted-foreground/80 text-[11px]">{meta.signingRequiredHint}</p>
            </div>
          </div>

          <div className="space-y-3">
            <IdChipInput
              label="Allowed Chat / Channel IDs"
              placeholder={meta.chatPlaceholder}
              values={form.allowedChatIds}
              draft={chatDraft}
              onDraftChange={setChatDraft}
              onChange={(allowedChatIds) => setForm((f) => ({ ...f, allowedChatIds }))}
            />
            <IdChipInput
              label="Allowed User IDs"
              placeholder={meta.userPlaceholder}
              values={form.allowedUserIds}
              draft={userDraft}
              onDraftChange={setUserDraft}
              onChange={(allowedUserIds) => setForm((f) => ({ ...f, allowedUserIds }))}
            />
            <p className="text-muted-foreground text-[11px] leading-snug">
              Default-deny: inbound commands are rejected until at least one chat ID or user ID is
              allowlisted.
            </p>
          </div>

          <div className="space-y-1.5">
            <label className="text-foreground block text-[12px] font-medium">
              Default Workspace
            </label>
            <input
              type="text"
              list="orbit-im-workspaces"
              value={form.defaultWorkspaceCwd}
              disabled={saving}
              onChange={(e) => setForm((f) => ({ ...f, defaultWorkspaceCwd: e.target.value }))}
              placeholder="/path/to/registered/workspace"
              className="border-border bg-background text-foreground placeholder:text-muted-foreground/55 h-[30px] w-full rounded-lg border px-2.5 font-mono text-[12px] outline-none"
            />
            {workspaceOptions.length > 0 && (
              <datalist id="orbit-im-workspaces">
                {workspaceOptions.map((dir) => (
                  <option key={dir} value={dir} />
                ))}
              </datalist>
            )}
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <label className="text-foreground block text-[12px] font-medium">
                Default Provider
              </label>
              <SettingSelect
                value={form.defaultProvider}
                options={providerOptions}
                disabled={saving}
                triggerClassName="w-full"
                onChange={(defaultProvider) => setForm((f) => ({ ...f, defaultProvider }))}
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-foreground block text-[12px] font-medium">Default Mode</label>
              <SettingSelect
                value={toPresetSessionMode(form.defaultMode)}
                options={MODE_OPTIONS}
                disabled={saving}
                triggerClassName="w-full"
                onChange={(defaultMode) => setForm((f) => ({ ...f, defaultMode }))}
              />
            </div>
          </div>

          <div className="border-border flex items-center justify-between rounded-xl border px-3.5 py-2.5">
            <div className="pr-3">
              <div className="text-foreground text-[12.5px] font-medium">
                Forward Tool Approvals
              </div>
              <div className="text-muted-foreground text-[11px]">
                Send interactive approve / deny cards to this channel when bound agents request
                permission.
              </div>
            </div>
            <ToggleSwitch
              checked={form.forwardApprovals}
              disabled={saving}
              ariaLabel="Forward tool approvals"
              onChange={(forwardApprovals) => setForm((f) => ({ ...f, forwardApprovals }))}
            />
          </div>

          {displayError && (
            <div className="rounded-lg border border-rose-500/25 bg-rose-500/8 px-3 py-2 text-[12px] text-rose-700 dark:text-rose-300">
              {displayError}
            </div>
          )}
        </div>

        <div className="border-border flex items-center justify-end gap-2 border-t px-5 py-3">
          <button
            type="button"
            disabled={saving}
            onClick={onClose}
            className="border-border bg-background text-foreground hover:bg-accent h-[30px] cursor-pointer rounded-lg border px-3 text-[12.5px] font-medium transition-colors"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={saving}
            onClick={() => void handleSave()}
            className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            {saving && <Loader2 className="h-[12.5px] w-[12.5px] animate-spin" />}
            {isEditing ? "Save Changes" : "Create Channel"}
          </button>
        </div>
      </div>
    </div>
  );
}

export function HostImChannelsSection({ hostId }: { hostId: string }) {
  const host = useOrbitSelector((s) => s.fleet.hosts.find((h) => h.host_id === hostId) ?? null);
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const fleetProjects = useOrbitSelector((s) => s.fleet.projects);
  const imState = useOrbitSelector((s) => s.imChannelsByHost[hostId]);

  const channels = imState?.channels ?? EMPTY_CHANNELS;
  const bindings = imState?.bindings ?? EMPTY_BINDINGS;
  const baseWebhookUrl = imState?.baseWebhookUrl || "http://127.0.0.1:7788";
  const loading = imState?.status === "loading" && channels.length === 0;
  const saving = Boolean(imState?.saving);
  const error = imState?.error ?? null;
  const isOnline =
    host?.connection_state === "online_direct" || host?.connection_state === "online_relay";

  const [modalInitial, setModalInitial] = useState<ChannelFormState | null>(null);
  const [modalServerError, setModalServerError] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [testChatId, setTestChatId] = useState("");
  const [testMessage, setTestMessage] = useState("Orbit IM test — connection verified.");
  const [testBusy, setTestBusy] = useState(false);
  const [testResult, setTestResult] = useState<{
    channelId: string;
    ok: boolean;
    detail: string;
  } | null>(null);

  useEffect(() => {
    if (hostId && isOnline) {
      void orbitActions.loadImChannels(hostId);
    }
  }, [hostId, isOnline]);

  const workspaceOptions = useMemo(() => {
    const dirs = new Set<string>();
    for (const ws of hostDetail?.workspaces ?? []) {
      if (ws.cwd && !ws.archived_at) dirs.add(ws.cwd);
    }
    for (const proj of fleetProjects ?? []) {
      if (proj.host_id === hostId) {
        for (const d of proj.directories) {
          if (d) dirs.add(d);
        }
      }
    }
    return Array.from(dirs);
  }, [hostDetail, fleetProjects, hostId]);

  const openCreateModal = () => {
    setModalServerError(null);
    setModalInitial({
      channelId: null,
      name: "",
      platform: "lark",
      enabled: true,
      appId: "",
      primarySecret: "",
      signingSecret: "",
      clearExistingSecrets: false,
      hasExistingPrimary: false,
      hasExistingSigning: false,
      maskedPrimary: null,
      maskedSigning: null,
      allowedChatIds: [],
      allowedUserIds: [],
      defaultWorkspaceCwd: workspaceOptions[0] ?? "",
      defaultProvider: "claude_code",
      defaultMode: "default",
      forwardApprovals: true,
    });
  };

  const openEditModal = (ch: ImChannelRecord) => {
    setModalServerError(null);
    setModalInitial({
      channelId: ch.channel_id,
      name: ch.name,
      platform: ch.platform,
      enabled: ch.enabled,
      appId: ch.app_id ?? "",
      primarySecret: "",
      signingSecret: "",
      clearExistingSecrets: false,
      hasExistingPrimary: ch.has_primary_secret,
      hasExistingSigning: ch.has_signing_secret,
      maskedPrimary: ch.masked_primary_secret ?? null,
      maskedSigning: ch.masked_signing_secret ?? null,
      allowedChatIds: [...ch.allowed_chat_ids],
      allowedUserIds: [...ch.allowed_user_ids],
      defaultWorkspaceCwd: ch.default_workspace_cwd,
      defaultProvider: ch.default_provider,
      defaultMode: ch.default_mode,
      forwardApprovals: ch.forward_approvals,
    });
  };

  const handleToggleEnabled = (ch: ImChannelRecord, nextEnabled: boolean) => {
    void orbitActions.upsertImChannel(hostId, {
      channel_id: ch.channel_id,
      name: ch.name,
      platform: ch.platform,
      enabled: nextEnabled,
      app_id: ch.app_id ?? null,
      primary_secret: null,
      signing_secret: null,
      keep_existing_secrets: true,
      allowed_chat_ids: ch.allowed_chat_ids,
      allowed_user_ids: ch.allowed_user_ids,
      default_workspace_cwd: ch.default_workspace_cwd,
      default_provider: ch.default_provider,
      default_mode: ch.default_mode,
      forward_approvals: ch.forward_approvals,
    });
  };

  const handleCopyWebhook = async (channelId: string, fullUrl: string) => {
    try {
      if (!navigator.clipboard?.writeText) return;
      await navigator.clipboard.writeText(fullUrl);
      setCopiedId(channelId);
      window.setTimeout(() => {
        setCopiedId((cur) => (cur === channelId ? null : cur));
      }, 1800);
    } catch {
      // Clipboard API denied or unavailable; do not mark as copied
    }
  };

  const handleSendTest = async (ch: ImChannelRecord) => {
    if (!testChatId.trim()) return;
    setTestBusy(true);
    const res = await orbitActions.testImChannel(
      hostId,
      ch.channel_id,
      testChatId.trim(),
      testMessage.trim(),
    );
    setTestBusy(false);
    setTestResult({ channelId: ch.channel_id, ok: res.ok, detail: res.detail });
  };

  const bindingsCountByChannel = useMemo(() => {
    const counts = new Map<string, number>();
    for (const b of bindings) {
      counts.set(b.channel_id, (counts.get(b.channel_id) ?? 0) + 1);
    }
    return counts;
  }, [bindings]);

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="IM Channels"
        description="Chat with agents and approve tool requests remotely from Lark / 飞书, Telegram, Slack, or Discord."
        action={
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              disabled={!isOnline || loading}
              onClick={() => void orbitActions.loadImChannels(hostId)}
              title="Refresh channels"
              className="border-border bg-background text-muted-foreground hover:text-foreground hover:bg-accent flex h-[30px] w-[30px] cursor-pointer items-center justify-center rounded-lg border transition-colors disabled:opacity-45"
            >
              <RefreshCw className={cn("h-[13.5px] w-[13.5px]", loading && "animate-spin")} />
            </button>
            <button
              type="button"
              disabled={!isOnline}
              onClick={openCreateModal}
              className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-medium transition-opacity hover:opacity-90 disabled:opacity-45"
            >
              <Plus className="h-[13.5px] w-[13.5px]" />
              Add Channel
            </button>
          </div>
        }
      />

      {!isOnline && (
        <div className="border-border bg-muted/30 text-muted-foreground rounded-xl border px-4 py-3 text-[12.5px]">
          Host is offline. Reconnect to configure IM channels.
        </div>
      )}

      {error && !modalInitial && (
        <div className="rounded-xl border border-rose-500/25 bg-rose-500/8 px-4 py-3 text-[12.5px] text-rose-700 dark:text-rose-300">
          {error}
        </div>
      )}

      <div className="space-y-2.5">
        <SettingGroupLabel>
          Configured Channels {channels.length > 0 ? `(${channels.length})` : ""}
        </SettingGroupLabel>

        {loading ? (
          <SettingCard>
            <div className="text-muted-foreground flex items-center gap-2 px-4 py-5 text-[12.5px]">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              Loading IM channels…
            </div>
          </SettingCard>
        ) : channels.length === 0 ? (
          <SettingCard>
            <div className="flex flex-col items-center justify-center gap-2 px-6 py-8 text-center">
              <MessageSquare className="text-muted-foreground/60 h-5 w-5" />
              <div className="text-foreground text-[13px] font-medium">No IM channels yet</div>
              <p className="text-muted-foreground max-w-[360px] text-[12px] leading-relaxed">
                Connect a bot for Lark / 飞书, Telegram, Slack, or Discord to run prompts and
                resolve approvals from your phone.
              </p>
            </div>
          </SettingCard>
        ) : (
          <SettingCard>
            {channels.map((ch) => {
              const meta = PLATFORM_META[ch.platform];
              const fullWebhookUrl = `${baseWebhookUrl}${ch.webhook_path}`;
              const allowlistTotal = ch.allowed_chat_ids.length + ch.allowed_user_ids.length;
              const boundCount = bindingsCountByChannel.get(ch.channel_id) ?? 0;
              const isTesting = testingId === ch.channel_id;
              const isConfirmingDelete = confirmDeleteId === ch.channel_id;
              const lastTest = testResult?.channelId === ch.channel_id ? testResult : null;

              return (
                <div key={ch.channel_id} className="space-y-2.5 px-4 py-3.5">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0 flex-1 space-y-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span
                          className={cn(
                            "h-[7px] w-[7px] shrink-0 rounded-full",
                            statusDotClass(ch.status, ch.enabled),
                          )}
                          title={statusLabel(ch.status, ch.enabled)}
                        />
                        <span className="text-foreground truncate text-[13.5px] font-medium">
                          {ch.name}
                        </span>
                        <span
                          className={cn(
                            "rounded-md px-1.5 py-0.5 text-[10.5px] font-medium",
                            meta.badgeClass,
                          )}
                        >
                          {meta.label}
                        </span>
                        {ch.forward_approvals && (
                          <span className="inline-flex items-center gap-1 rounded-md bg-emerald-500/10 px-1.5 py-0.5 text-[10.5px] font-medium text-emerald-700 dark:text-emerald-300">
                            <ShieldCheck className="h-[11px] w-[11px]" />
                            Approvals
                          </span>
                        )}
                      </div>

                      <div className="text-muted-foreground flex flex-wrap items-center gap-x-3 gap-y-1 text-[11.5px]">
                        <span>
                          {statusLabel(ch.status, ch.enabled)}
                          {ch.status_message ? ` · ${ch.status_message}` : ""}
                        </span>
                        <span
                          className={cn(
                            allowlistTotal === 0 && "text-amber-600 dark:text-amber-400",
                          )}
                        >
                          {allowlistTotal === 0
                            ? "Allowlist empty (denied)"
                            : `Allowlist: ${ch.allowed_chat_ids.length} chats, ${ch.allowed_user_ids.length} users`}
                        </span>
                        {boundCount > 0 && <span>{boundCount} active sessions</span>}
                        {ch.message_count > 0 && <span>{ch.message_count} msgs</span>}
                      </div>

                      <div className="text-muted-foreground/85 flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-[11px]">
                        <span className="truncate">{ch.default_workspace_cwd}</span>
                        {ch.masked_primary_secret && (
                          <span className="bg-muted/50 rounded px-1.5 py-0.5 text-[10.5px]">
                            Secret {ch.masked_primary_secret}
                          </span>
                        )}
                      </div>

                      {(ch.platform === "lark" ||
                        ch.platform === "slack" ||
                        ch.has_signing_secret) && (
                        <div className="flex items-center gap-1.5 pt-0.5">
                          <code className="border-border bg-muted/35 text-muted-foreground max-w-full truncate rounded border px-2 py-0.5 font-mono text-[10.5px]">
                            {fullWebhookUrl}
                          </code>
                          <button
                            type="button"
                            onClick={() => void handleCopyWebhook(ch.channel_id, fullWebhookUrl)}
                            className="text-muted-foreground hover:text-foreground inline-flex h-6 cursor-pointer items-center gap-1 rounded px-1.5 text-[11px] transition-colors"
                            title="Copy webhook URL"
                          >
                            {copiedId === ch.channel_id ? (
                              <Check className="h-3 w-3 text-emerald-500" />
                            ) : (
                              <Copy className="h-3 w-3" />
                            )}
                          </button>
                        </div>
                      )}
                    </div>

                    <div className="flex shrink-0 items-center gap-1.5">
                      <button
                        type="button"
                        disabled={!isOnline}
                        onClick={() => {
                          setConfirmDeleteId(null);
                          if (isTesting) {
                            setTestingId(null);
                          } else {
                            setTestingId(ch.channel_id);
                            setTestChatId(ch.allowed_chat_ids[0] ?? "");
                            setTestResult(null);
                          }
                        }}
                        className="border-border bg-background text-foreground hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium transition-colors"
                      >
                        <Send className="h-[11px] w-[11px]" />
                        Test
                      </button>
                      <button
                        type="button"
                        disabled={!isOnline}
                        onClick={() => {
                          setConfirmDeleteId(null);
                          openEditModal(ch);
                        }}
                        title="Edit channel"
                        className="text-muted-foreground hover:text-foreground hover:bg-accent flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                      >
                        <Pencil className="h-[12.5px] w-[12.5px]" />
                      </button>
                      {isConfirmingDelete ? (
                        <div className="flex items-center gap-1">
                          <button
                            type="button"
                            disabled={!isOnline}
                            onClick={() => {
                              setConfirmDeleteId(null);
                              void orbitActions.deleteImChannel(hostId, ch.channel_id);
                            }}
                            className="h-[26px] cursor-pointer rounded-md bg-rose-600 px-2 text-[11px] font-medium text-white hover:bg-rose-700"
                          >
                            Delete
                          </button>
                          <button
                            type="button"
                            onClick={() => setConfirmDeleteId(null)}
                            className="border-border bg-background text-muted-foreground hover:text-foreground h-[26px] cursor-pointer rounded-md border px-1.5 text-[11px]"
                          >
                            Cancel
                          </button>
                        </div>
                      ) : (
                        <button
                          type="button"
                          disabled={!isOnline}
                          onClick={() => setConfirmDeleteId(ch.channel_id)}
                          title="Delete channel"
                          className="text-muted-foreground hover:bg-rose-500/10 hover:text-rose-600 flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                        >
                          <Trash2 className="h-[12.5px] w-[12.5px]" />
                        </button>
                      )}
                      <div className="pl-1">
                        <ToggleSwitch
                          checked={ch.enabled}
                          disabled={!isOnline || saving}
                          ariaLabel={`Toggle ${ch.name}`}
                          onChange={(next) => handleToggleEnabled(ch, next)}
                        />
                      </div>
                    </div>
                  </div>

                  {isTesting && (
                    <div className="border-border bg-muted/25 space-y-2 rounded-lg border p-3">
                      <div className="flex flex-col gap-2 sm:flex-row">
                        <input
                          type="text"
                          value={testChatId}
                          onChange={(e) => setTestChatId(e.target.value)}
                          placeholder={`Target Chat ID (${meta.chatPlaceholder})`}
                          className="border-border bg-background text-foreground h-[28px] w-full rounded-md border px-2.5 font-mono text-[11.5px] outline-none sm:w-[190px]"
                        />
                        <input
                          type="text"
                          value={testMessage}
                          onChange={(e) => setTestMessage(e.target.value)}
                          placeholder="Test message"
                          className="border-border bg-background text-foreground h-[28px] flex-1 rounded-md border px-2.5 text-[11.5px] outline-none"
                        />
                        <button
                          type="button"
                          disabled={testBusy || !testChatId.trim()}
                          onClick={() => void handleSendTest(ch)}
                          className="bg-foreground text-background inline-flex h-[28px] shrink-0 cursor-pointer items-center justify-center gap-1 rounded-md px-3 text-[11.5px] font-medium disabled:opacity-45"
                        >
                          {testBusy ? (
                            <Loader2 className="h-3 w-3 animate-spin" />
                          ) : (
                            <Send className="h-3 w-3" />
                          )}
                          Send
                        </button>
                      </div>
                      {lastTest && (
                        <div
                          className={cn(
                            "text-[11.5px]",
                            lastTest.ok
                              ? "text-emerald-700 dark:text-emerald-300"
                              : "text-rose-700 dark:text-rose-300",
                          )}
                        >
                          {lastTest.detail}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </SettingCard>
        )}
      </div>

      {modalInitial && (
        <ChannelModal
          initial={modalInitial}
          workspaceOptions={workspaceOptions}
          saving={saving}
          serverError={error ?? modalServerError}
          onClose={() => {
            setModalServerError(null);
            setModalInitial(null);
          }}
          onSubmit={async (req) => {
            setModalServerError(null);
            const snap = await orbitActions.upsertImChannel(hostId, req);
            if (snap) {
              setModalInitial(null);
            } else {
              setModalServerError("Failed to save IM channel. Check the details and try again.");
            }
          }}
        />
      )}
    </div>
  );
}
