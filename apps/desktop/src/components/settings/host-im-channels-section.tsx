import { useEffect, useMemo, useState } from "react";
import {
  Check,
  Copy,
  Loader2,
  MessageSquare,
  Pencil,
  Plus,
  Radio,
  RefreshCw,
  Send,
  ShieldCheck,
  Trash2,
  Webhook,
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { useI18n } from "@/lib/i18n";
import {
  type ImChannelRecord,
  type ImConnectionStatus,
  type ImPlatform,
  type ImThreadBinding,
  type ImTransportMode,
  PROVIDER_LIST,
  type ProviderKind,
  type SessionMode,
  type UpsertImChannelRequest,
} from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

const EMPTY_CHANNELS: readonly ImChannelRecord[] = [];
const EMPTY_BINDINGS: readonly ImThreadBinding[] = [];

const PLATFORM_META: Record<
  ImPlatform,
  {
    label: string;
    badgeClass: string;
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
    showAppId: true,
    appIdLabel: "App ID",
    appIdPlaceholder: "cli_a1b2c3d4... (or lark:cli_...)",
    primaryLabel: "App Secret",
    primaryPlaceholder: "Lark / Feishu App Secret",
    signingLabel: "Verification Token / Encrypt Key",
    signingPlaceholder: "Verification Token or Encrypt Key",
    signingRequiredHint: "Verified in constant time on every inbound Lark callback.",
    chatPlaceholder: "oc_7c9a...",
    userPlaceholder: "ou_3f8b...",
  },
  telegram: {
    label: "Telegram",
    badgeClass: "bg-blue-500/10 text-blue-700 dark:text-blue-300",
    showAppId: false,
    appIdLabel: "",
    appIdPlaceholder: "",
    primaryLabel: "Bot Token",
    primaryPlaceholder: "123456789:AAH...",
    signingLabel: "Webhook Secret Token",
    signingPlaceholder: "X-Telegram-Bot-Api-Secret-Token",
    signingRequiredHint: "Verified in constant time on every inbound Telegram webhook.",
    chatPlaceholder: "-1001234567890",
    userPlaceholder: "123456789",
  },
  slack: {
    label: "Slack",
    badgeClass: "bg-violet-500/10 text-violet-700 dark:text-violet-300",
    showAppId: false,
    appIdLabel: "",
    appIdPlaceholder: "",
    primaryLabel: "Bot Token",
    primaryPlaceholder: "xoxb-...",
    signingLabel: "Signing Secret",
    signingPlaceholder: "Slack App Signing Secret",
    signingRequiredHint: "HMAC-SHA256 + 5-minute replay window enforced.",
    chatPlaceholder: "C0123456789",
    userPlaceholder: "U0123456789",
  },
  discord: {
    label: "Discord",
    badgeClass: "bg-indigo-500/10 text-indigo-700 dark:text-indigo-300",
    showAppId: false,
    appIdLabel: "",
    appIdPlaceholder: "",
    primaryLabel: "Bot Token",
    primaryPlaceholder: "Discord Bot Token",
    signingLabel: "Webhook Secret",
    signingPlaceholder: "",
    signingRequiredHint: "",
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

function resolveTransportMode(ch: ImChannelRecord): ImTransportMode {
  return ch.transport_mode ?? "long_connection";
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
  if (!enabled) return "bg-ink-faint";
  switch (status) {
    case "connected":
      return "bg-signal-success";
    case "connecting":
    case "reconnecting":
      return "bg-signal-warning animate-pulse";
    case "error":
      return "bg-signal-danger";
    default:
      return "bg-ink-faint";
  }
}

function statusBadgeVariant(
  status: ImConnectionStatus,
  enabled: boolean,
): "success" | "warning" | "destructive" | "secondary" {
  if (!enabled) return "secondary";
  switch (status) {
    case "connected":
      return "success";
    case "connecting":
    case "reconnecting":
      return "warning";
    case "error":
      return "destructive";
    default:
      return "secondary";
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
  const { t } = useI18n();
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
    <Field>
      <FieldLabel className="text-ink-primary">{label}</FieldLabel>
      <div className="flex items-center gap-1.5">
        <Input
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
          className="h-[30px] flex-1 font-mono text-[12px]"
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={flushDraft}
          disabled={!draft.trim()}
        >
          <Plus className="size-[12px]" />
          {t("im.addId")}
        </Button>
      </div>
      {values.length > 0 && (
        <div className="flex flex-wrap gap-1 pt-0.5">
          {values.map((id) => (
            <span
              key={id}
              className="border-border bg-surface-recessed text-ink-primary inline-flex items-center gap-1 rounded-md border px-2 py-0.5 font-mono text-[11px]"
            >
              <span>{id}</span>
              <button
                type="button"
                aria-label={t("im.removeId", { id })}
                onClick={() => onChange(values.filter((x) => x !== id))}
                className="text-ink-muted hover:text-ink-primary cursor-pointer"
              >
                <X className="size-[11px]" />
              </button>
            </span>
          ))}
        </div>
      )}
    </Field>
  );
}

interface ChannelFormState {
  channelId: string | null;
  name: string;
  platform: ImPlatform;
  transportMode: ImTransportMode;
  enabled: boolean;
  appId: string;
  primarySecret: string;
  signingSecret: string;
  appToken: string;
  publicKey: string;
  clearExistingSecrets: boolean;
  hasExistingPrimary: boolean;
  hasExistingSigning: boolean;
  hasExistingAppToken: boolean;
  maskedPrimary: string | null;
  maskedSigning: string | null;
  maskedAppToken: string | null;
  allowedChatIds: string[];
  allowedUserIds: string[];
  defaultWorkspaceCwd: string;
  defaultProvider: ProviderKind;
  defaultMode: SessionMode;
  forwardApprovals: boolean;
  routeToPersonalRouter: boolean;
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
  const { t } = useI18n();
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

  const transportOptions: SettingSelectOption<ImTransportMode>[] = useMemo(
    () => [
      { value: "long_connection", label: t("im.modeLongConnection") },
      { value: "webhook", label: t("im.modeWebhook") },
    ],
    [t],
  );

  const transportHint = useMemo(() => {
    const isLong = form.transportMode === "long_connection";
    switch (form.platform) {
      case "lark":
        return isLong ? t("im.larkLongConnHint") : t("im.larkWebhookHint");
      case "slack":
        return isLong ? t("im.slackLongConnHint") : t("im.slackWebhookHint");
      case "discord":
        return isLong ? t("im.discordLongConnHint") : t("im.discordWebhookHint");
      case "telegram":
        return isLong ? t("im.telegramLongConnHint") : t("im.telegramWebhookHint");
    }
  }, [form.platform, form.transportMode, t]);

  const showSlackAppToken =
    form.platform === "slack" && form.transportMode === "long_connection";
  const showDiscordPublicKey = form.platform === "discord" && form.transportMode === "webhook";
  const showSigningSecret =
    form.transportMode === "webhook" && form.platform !== "discord";
  const hasAnySavedSecret =
    form.hasExistingPrimary || form.hasExistingSigning || form.hasExistingAppToken;

  const handleSave = async () => {
    setLocalError(null);
    if (!form.name.trim()) {
      setLocalError(t("im.errNameRequired"));
      return;
    }
    if (!form.defaultWorkspaceCwd.trim()) {
      setLocalError(t("im.errWorkspaceRequired"));
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
      transport_mode: form.transportMode,
      app_id: meta.showAppId ? form.appId.trim() || null : null,
      primary_secret: form.primarySecret.trim() || null,
      signing_secret: form.signingSecret.trim() || null,
      app_token: form.platform === "slack" ? form.appToken.trim() || null : null,
      public_key: form.platform === "discord" ? form.publicKey.trim() || null : null,
      keep_existing_secrets: !form.clearExistingSecrets,
      allowed_chat_ids: finalChatIds,
      allowed_user_ids: finalUserIds,
      default_workspace_cwd: form.defaultWorkspaceCwd.trim(),
      default_provider: form.defaultProvider,
      default_mode: form.defaultMode,
      forward_approvals: form.forwardApprovals,
      routing_mode: form.routeToPersonalRouter ? "personal_router" : "per_thread_agent",
      router_mirror_policy: "milestones_and_approvals",
    });
  };

  const displayError = localError ?? serverError;

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !saving) onClose();
      }}
    >
      <DialogContent className="flex max-h-[86vh] max-w-[520px] flex-col overflow-hidden p-0">
        <DialogHeader className="border-border-subtle border-b px-5 py-3.5">
          <DialogTitle>
            {isEditing ? t("im.editChannel") : t("im.addChannelTitle")}
          </DialogTitle>
          <DialogDescription>{transportHint}</DialogDescription>
        </DialogHeader>

        <div className="orbit-native-scroll min-h-0 flex-1 overflow-y-auto overscroll-contain px-5 py-4">
          <FieldGroup className="gap-4">
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <Field>
                <FieldLabel className="text-ink-primary">{t("im.platform")}</FieldLabel>
                <SettingSelect
                  value={form.platform}
                  options={PLATFORM_OPTIONS}
                  disabled={isEditing || saving}
                  triggerClassName="w-full"
                  onChange={(platform) => setForm((f) => ({ ...f, platform }))}
                />
              </Field>

              <Field>
                <FieldLabel htmlFor="im-channel-name" className="text-ink-primary">
                  {t("im.channelName")}
                </FieldLabel>
                <Input
                  id="im-channel-name"
                  type="text"
                  value={form.name}
                  disabled={saving}
                  onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
                  placeholder={t("im.channelNamePlaceholder", { platform: meta.label })}
                />
              </Field>
            </div>

            <Field>
              <FieldLabel className="text-ink-primary">{t("im.connectionMode")}</FieldLabel>
              <SettingSelect
                value={form.transportMode}
                options={transportOptions}
                disabled={saving}
                triggerClassName="w-full"
                onChange={(transportMode) => setForm((f) => ({ ...f, transportMode }))}
              />
              <FieldDescription className="text-[11px]">{transportHint}</FieldDescription>
            </Field>

            <div className="border-border-subtle bg-surface-recessed rounded-xl border p-3.5">
              <FieldGroup className="gap-3">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-ink-primary text-[12px] font-medium">
                    {t("im.credentials")}
                  </span>
                  {isEditing && hasAnySavedSecret && (
                    <label className="text-ink-muted flex cursor-pointer items-center gap-1.5 text-[11px] select-none">
                      <Checkbox
                        size="sm"
                        checked={form.clearExistingSecrets}
                        onCheckedChange={(checked) =>
                          setForm((f) => ({ ...f, clearExistingSecrets: checked }))
                        }
                      />
                      <span>{t("im.clearSavedSecrets")}</span>
                    </label>
                  )}
                </div>

                {meta.showAppId && (
                  <Field>
                    <FieldLabel htmlFor="im-app-id" className="text-[12px]">
                      {meta.appIdLabel}
                    </FieldLabel>
                    <Input
                      id="im-app-id"
                      type="text"
                      value={form.appId}
                      disabled={saving}
                      onChange={(e) => setForm((f) => ({ ...f, appId: e.target.value }))}
                      placeholder={meta.appIdPlaceholder}
                      className="font-mono text-[12px]"
                    />
                  </Field>
                )}

                <Field>
                  <div className="flex items-center justify-between">
                    <FieldLabel htmlFor="im-primary-secret" className="text-[12px]">
                      {meta.primaryLabel}
                    </FieldLabel>
                    {isEditing && form.maskedPrimary && !form.clearExistingSecrets && (
                      <span className="text-ink-muted font-mono text-[11px]">
                        {t("im.savedMasked", { masked: form.maskedPrimary })}
                      </span>
                    )}
                  </div>
                  <Input
                    id="im-primary-secret"
                    type="password"
                    value={form.primarySecret}
                    disabled={saving}
                    onChange={(e) => setForm((f) => ({ ...f, primarySecret: e.target.value }))}
                    placeholder={
                      isEditing && form.hasExistingPrimary && !form.clearExistingSecrets
                        ? t("im.keepSavedPlaceholder")
                        : meta.primaryPlaceholder
                    }
                    className="font-mono text-[12px]"
                  />
                </Field>

                {showSlackAppToken && (
                  <Field>
                    <div className="flex items-center justify-between">
                      <FieldLabel htmlFor="im-app-token" className="text-[12px]">
                        {t("im.slackAppTokenLabel")}
                      </FieldLabel>
                      {isEditing && form.maskedAppToken && !form.clearExistingSecrets && (
                        <span className="text-ink-muted font-mono text-[11px]">
                          {t("im.savedMasked", { masked: form.maskedAppToken })}
                        </span>
                      )}
                    </div>
                    <Input
                      id="im-app-token"
                      type="password"
                      value={form.appToken}
                      disabled={saving}
                      onChange={(e) => setForm((f) => ({ ...f, appToken: e.target.value }))}
                      placeholder={
                        isEditing && form.hasExistingAppToken && !form.clearExistingSecrets
                          ? t("im.keepSavedPlaceholder")
                          : t("im.slackAppTokenPlaceholder")
                      }
                      className="font-mono text-[12px]"
                    />
                    <FieldDescription className="text-[11px]">
                      {t("im.slackAppTokenHint")}
                    </FieldDescription>
                  </Field>
                )}

                {showDiscordPublicKey && (
                  <Field>
                    <FieldLabel htmlFor="im-discord-public-key" className="text-[12px]">
                      {t("im.discordPublicKeyLabel")}
                    </FieldLabel>
                    <Input
                      id="im-discord-public-key"
                      type="text"
                      value={form.publicKey}
                      disabled={saving}
                      onChange={(e) => setForm((f) => ({ ...f, publicKey: e.target.value }))}
                      placeholder={t("im.discordPublicKeyPlaceholder")}
                      className="font-mono text-[12px]"
                    />
                    <FieldDescription className="text-[11px]">
                      {t("im.discordPublicKeyHint")}
                    </FieldDescription>
                  </Field>
                )}

                {showSigningSecret && (
                  <Field>
                    <div className="flex items-center justify-between">
                      <FieldLabel htmlFor="im-signing-secret" className="text-[12px]">
                        {meta.signingLabel}
                      </FieldLabel>
                      {isEditing && form.maskedSigning && !form.clearExistingSecrets && (
                        <span className="text-ink-muted font-mono text-[11px]">
                          {t("im.savedMasked", { masked: form.maskedSigning })}
                        </span>
                      )}
                    </div>
                    <Input
                      id="im-signing-secret"
                      type="password"
                      value={form.signingSecret}
                      disabled={saving}
                      onChange={(e) => setForm((f) => ({ ...f, signingSecret: e.target.value }))}
                      placeholder={
                        isEditing && form.hasExistingSigning && !form.clearExistingSecrets
                          ? t("im.keepSavedPlaceholder")
                          : meta.signingPlaceholder
                      }
                      className="font-mono text-[12px]"
                    />
                    <FieldDescription className="text-[11px]">
                      {meta.signingRequiredHint}
                    </FieldDescription>
                  </Field>
                )}
              </FieldGroup>
            </div>

            <FieldGroup className="gap-3">
              <IdChipInput
                label={t("im.allowedChatIds")}
                placeholder={meta.chatPlaceholder}
                values={form.allowedChatIds}
                draft={chatDraft}
                onDraftChange={setChatDraft}
                onChange={(allowedChatIds) => setForm((f) => ({ ...f, allowedChatIds }))}
              />
              <IdChipInput
                label={t("im.allowedUserIds")}
                placeholder={meta.userPlaceholder}
                values={form.allowedUserIds}
                draft={userDraft}
                onDraftChange={setUserDraft}
                onChange={(allowedUserIds) => setForm((f) => ({ ...f, allowedUserIds }))}
              />
              <FieldDescription className="text-[11px]">
                {t("im.allowlistDefaultDenyHint")}
              </FieldDescription>
            </FieldGroup>

            <Field>
              <FieldLabel htmlFor="im-default-workspace" className="text-ink-primary">
                {t("im.defaultWorkspace")}
              </FieldLabel>
              <Input
                id="im-default-workspace"
                type="text"
                list="orbit-im-workspaces"
                value={form.defaultWorkspaceCwd}
                disabled={saving}
                onChange={(e) => setForm((f) => ({ ...f, defaultWorkspaceCwd: e.target.value }))}
                placeholder="/path/to/registered/workspace"
                className="font-mono text-[12px]"
              />
              {workspaceOptions.length > 0 && (
                <datalist id="orbit-im-workspaces">
                  {workspaceOptions.map((dir) => (
                    <option key={dir} value={dir} />
                  ))}
                </datalist>
              )}
            </Field>

            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <Field>
                <FieldLabel className="text-ink-primary">{t("im.defaultProvider")}</FieldLabel>
                <SettingSelect
                  value={form.defaultProvider}
                  options={providerOptions}
                  disabled={saving}
                  triggerClassName="w-full"
                  onChange={(defaultProvider) => setForm((f) => ({ ...f, defaultProvider }))}
                />
              </Field>
              <Field>
                <FieldLabel className="text-ink-primary">{t("im.defaultMode")}</FieldLabel>
                <SettingSelect
                  value={toPresetSessionMode(form.defaultMode)}
                  options={MODE_OPTIONS}
                  disabled={saving}
                  triggerClassName="w-full"
                  onChange={(defaultMode) => setForm((f) => ({ ...f, defaultMode }))}
                />
              </Field>
            </div>

            <div className="border-border-subtle flex items-center justify-between rounded-xl border px-3.5 py-2.5">
              <div className="pr-3">
                <div className="text-ink-primary text-[13px] font-medium">
                  {t("im.routePersonalRouter")}
                </div>
                <div className="text-ink-secondary text-[11px]">
                  {t("im.routePersonalRouterDesc")}
                </div>
              </div>
              <ToggleSwitch
                checked={form.routeToPersonalRouter}
                disabled={saving}
                ariaLabel={t("im.routePersonalRouter")}
                onChange={(routeToPersonalRouter) =>
                  setForm((f) => ({ ...f, routeToPersonalRouter }))
                }
              />
            </div>

            <div className="border-border-subtle flex items-center justify-between rounded-xl border px-3.5 py-2.5">
              <div className="pr-3">
                <div className="text-ink-primary text-[13px] font-medium">
                  {t("im.forwardApprovals")}
                </div>
                <div className="text-ink-secondary text-[11px]">
                  {t("im.forwardApprovalsDesc")}
                </div>
              </div>
              <ToggleSwitch
                checked={form.forwardApprovals}
                disabled={saving}
                ariaLabel={t("im.forwardApprovals")}
                onChange={(forwardApprovals) => setForm((f) => ({ ...f, forwardApprovals }))}
              />
            </div>

            {displayError && (
              <FieldError className="border-signal-danger-border bg-signal-danger-bg text-signal-danger rounded-lg border px-3 py-2">
                {displayError}
              </FieldError>
            )}
          </FieldGroup>
        </div>

        <DialogFooter className="border-border-subtle border-t px-5 py-3">
          <Button type="button" variant="outline" size="sm" disabled={saving} onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button type="button" size="sm" disabled={saving} onClick={() => void handleSave()}>
            {saving && <Loader2 className="size-[12.5px] animate-spin" />}
            {isEditing ? t("im.saveChanges") : t("im.createChannel")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function HostImChannelsSection({ hostId }: { hostId: string }) {
  const { t } = useI18n();
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
  const [testMessage, setTestMessage] = useState("");
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

  const hasTransitioningChannel = useMemo(
    () =>
      channels.some(
        (ch) => ch.enabled && (ch.status === "connecting" || ch.status === "reconnecting"),
      ),
    [channels],
  );

  // Live refresh while any channel is connecting or reconnecting so WS handshakes update in place
  useEffect(() => {
    if (!hostId || !isOnline || !hasTransitioningChannel) return;
    const timer = window.setInterval(() => {
      void orbitActions.loadImChannels(hostId);
    }, 3_000);
    return () => window.clearInterval(timer);
  }, [hostId, isOnline, hasTransitioningChannel]);

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
      transportMode: "long_connection",
      enabled: true,
      appId: "",
      primarySecret: "",
      signingSecret: "",
      appToken: "",
      publicKey: "",
      clearExistingSecrets: false,
      hasExistingPrimary: false,
      hasExistingSigning: false,
      hasExistingAppToken: false,
      maskedPrimary: null,
      maskedSigning: null,
      maskedAppToken: null,
      allowedChatIds: [],
      allowedUserIds: [],
      defaultWorkspaceCwd: workspaceOptions[0] ?? "",
      defaultProvider: "claude_code",
      defaultMode: "default",
      forwardApprovals: true,
      routeToPersonalRouter: false,
    });
  };

  const openEditModal = (ch: ImChannelRecord) => {
    setModalServerError(null);
    setModalInitial({
      channelId: ch.channel_id,
      name: ch.name,
      platform: ch.platform,
      transportMode: resolveTransportMode(ch),
      enabled: ch.enabled,
      appId: ch.app_id ?? "",
      primarySecret: "",
      signingSecret: "",
      appToken: "",
      publicKey: ch.public_key ?? "",
      clearExistingSecrets: false,
      hasExistingPrimary: ch.has_primary_secret,
      hasExistingSigning: ch.has_signing_secret,
      hasExistingAppToken: Boolean(ch.has_app_token),
      maskedPrimary: ch.masked_primary_secret ?? null,
      maskedSigning: ch.masked_signing_secret ?? null,
      maskedAppToken: ch.masked_app_token ?? null,
      allowedChatIds: [...ch.allowed_chat_ids],
      allowedUserIds: [...ch.allowed_user_ids],
      defaultWorkspaceCwd: ch.default_workspace_cwd,
      defaultProvider: ch.default_provider,
      defaultMode: ch.default_mode,
      forwardApprovals: ch.forward_approvals,
      routeToPersonalRouter: ch.routing_mode === "personal_router",
    });
  };

  const handleToggleEnabled = (ch: ImChannelRecord, nextEnabled: boolean) => {
    void orbitActions.upsertImChannel(hostId, {
      channel_id: ch.channel_id,
      name: ch.name,
      platform: ch.platform,
      enabled: nextEnabled,
      transport_mode: resolveTransportMode(ch),
      app_id: ch.app_id ?? null,
      primary_secret: null,
      signing_secret: null,
      app_token: null,
      public_key: ch.public_key ?? null,
      keep_existing_secrets: true,
      allowed_chat_ids: ch.allowed_chat_ids,
      allowed_user_ids: ch.allowed_user_ids,
      default_workspace_cwd: ch.default_workspace_cwd,
      default_provider: ch.default_provider,
      default_mode: ch.default_mode,
      forward_approvals: ch.forward_approvals,
      routing_mode: ch.routing_mode ?? "per_thread_agent",
      router_mirror_policy: ch.router_mirror_policy ?? "milestones_and_approvals",
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
      (testMessage || t("im.testMessageDefault")).trim(),
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

  const formatStatusPillLabel = (ch: ImChannelRecord): string => {
    if (!ch.enabled) return t("im.statusDisabled");
    const mode = resolveTransportMode(ch);
    switch (ch.status) {
      case "connected":
        if (mode === "webhook") return t("im.statusWebhookReady");
        return ch.platform === "telegram"
          ? t("im.statusConnected")
          : t("im.statusConnectedWs");
      case "connecting":
        return t("im.statusConnecting");
      case "reconnecting":
        return t("im.statusReconnecting");
      case "error":
        return t("im.statusError");
      default:
        return t("im.statusIdle");
    }
  };

  const formatTransportLabel = (ch: ImChannelRecord): string => {
    const mode = resolveTransportMode(ch);
    if (mode === "webhook") return t("im.modeBadgeWebhook");
    return ch.platform === "telegram" ? t("im.modeBadgePolling") : t("im.modeBadgeWs");
  };

  return (
    <div className="flex flex-col gap-6">
      <SettingSectionHeader
        title={t("im.title")}
        description={t("im.description")}
        action={
          <div className="flex items-center gap-1.5">
            <Button
              type="button"
              variant="outline"
              size="icon-sm"
              disabled={!isOnline || loading}
              onClick={() => void orbitActions.loadImChannels(hostId)}
              title={t("im.refreshChannels")}
            >
              <RefreshCw className={cn("size-[13.5px]", loading && "animate-spin")} />
            </Button>
            <Button type="button" size="sm" disabled={!isOnline} onClick={openCreateModal}>
              <Plus className="size-[13.5px]" />
              {t("im.addChannel")}
            </Button>
          </div>
        }
      />

      {!isOnline && (
        <div className="border-border-subtle bg-surface-recessed text-ink-secondary rounded-xl border px-4 py-3 text-[13px]">
          {t("im.hostOffline")}
        </div>
      )}

      {error && !modalInitial && (
        <div className="border-signal-danger-border bg-signal-danger-bg text-signal-danger rounded-xl border px-4 py-3 text-[13px]">
          {error}
        </div>
      )}

      <div className="flex flex-col gap-2.5">
        <SettingGroupLabel>
          {t("im.configuredChannels")}
          {channels.length > 0 ? ` (${channels.length})` : ""}
        </SettingGroupLabel>

        {loading ? (
          <SettingCard>
            <div className="text-ink-secondary flex items-center gap-2 px-4 py-5 text-[13px]">
              <Loader2 className="size-3.5 animate-spin" />
              {t("im.loading")}
            </div>
          </SettingCard>
        ) : channels.length === 0 ? (
          <SettingCard>
            <div className="flex flex-col items-center justify-center gap-2 px-6 py-8 text-center">
              <MessageSquare className="text-ink-muted size-5" />
              <div className="text-ink-primary text-[13px] font-medium">{t("im.emptyTitle")}</div>
              <p className="text-ink-secondary max-w-[360px] text-[12px] leading-relaxed">
                {t("im.emptyDesc")}
              </p>
            </div>
          </SettingCard>
        ) : (
          <SettingCard>
            {channels.map((ch) => {
              const meta = PLATFORM_META[ch.platform];
              const transportMode = resolveTransportMode(ch);
              const fullWebhookUrl = `${baseWebhookUrl}${ch.webhook_path}`;
              const allowlistTotal = ch.allowed_chat_ids.length + ch.allowed_user_ids.length;
              const boundCount = bindingsCountByChannel.get(ch.channel_id) ?? 0;
              const isTesting = testingId === ch.channel_id;
              const isConfirmingDelete = confirmDeleteId === ch.channel_id;
              const lastTest = testResult?.channelId === ch.channel_id ? testResult : null;
              const pillLabel = formatStatusPillLabel(ch);
              const showStatusDetail =
                Boolean(ch.status_message) &&
                ch.status_message?.trim().toLowerCase() !== pillLabel.trim().toLowerCase();

              return (
                <div key={ch.channel_id} className="flex flex-col gap-2.5 px-4 py-3.5">
                  <div className="flex items-start justify-between gap-3">
                    <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <span className="text-ink-primary truncate text-[13px] font-medium">
                          {ch.name}
                        </span>
                        <span
                          className={cn(
                            "rounded-[6px] px-1.5 py-0.5 text-[11px] font-medium",
                            meta.badgeClass,
                          )}
                        >
                          {meta.label}
                        </span>
                        <Badge variant={statusBadgeVariant(ch.status, ch.enabled)}>
                          <span
                            className={cn(
                              "size-[6px] shrink-0 rounded-full",
                              statusDotClass(ch.status, ch.enabled),
                            )}
                          />
                          {pillLabel}
                        </Badge>
                        <Badge variant="outline">
                          {transportMode === "webhook" ? (
                            <Webhook className="size-[11px]" />
                          ) : (
                            <Radio className="size-[11px]" />
                          )}
                          {formatTransportLabel(ch)}
                        </Badge>
                        {ch.forward_approvals && (
                          <Badge variant="secondary">
                            <ShieldCheck className="size-[11px]" />
                            {t("im.badgeApprovals")}
                          </Badge>
                        )}
                      </div>

                      <div className="text-ink-secondary flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px]">
                        {showStatusDetail && (
                          <span
                            className={cn(
                              ch.status === "error" && "text-signal-danger",
                              ch.status === "reconnecting" && "text-signal-warning",
                            )}
                          >
                            {ch.status_message}
                          </span>
                        )}
                        <span className={cn(allowlistTotal === 0 && "text-signal-warning")}>
                          {allowlistTotal === 0
                            ? t("im.allowlistEmpty")
                            : t("im.allowlistSummary", {
                                chats: ch.allowed_chat_ids.length,
                                users: ch.allowed_user_ids.length,
                              })}
                        </span>
                        {boundCount > 0 && (
                          <span>{t("im.activeSessions", { count: boundCount })}</span>
                        )}
                        {ch.message_count > 0 && (
                          <span>{t("im.messagesCount", { count: ch.message_count })}</span>
                        )}
                      </div>

                      <div className="text-ink-muted flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-[11px]">
                        <span className="truncate">{ch.default_workspace_cwd}</span>
                        {ch.masked_primary_secret && (
                          <span className="bg-surface-recessed rounded px-1.5 py-0.5 text-[11px]">
                            {t("im.secretMasked", { masked: ch.masked_primary_secret })}
                          </span>
                        )}
                        {ch.masked_app_token && (
                          <span className="bg-surface-recessed rounded px-1.5 py-0.5 text-[11px]">
                            {t("im.appTokenMasked", { masked: ch.masked_app_token })}
                          </span>
                        )}
                      </div>

                      {transportMode === "webhook" && (
                        <div className="flex items-center gap-1.5 pt-0.5">
                          <code className="border-border-subtle bg-surface-recessed text-ink-secondary max-w-full truncate rounded border px-2 py-0.5 font-mono text-[11px]">
                            {fullWebhookUrl}
                          </code>
                          <button
                            type="button"
                            onClick={() => void handleCopyWebhook(ch.channel_id, fullWebhookUrl)}
                            className="text-ink-muted hover:text-ink-primary inline-flex h-6 cursor-pointer items-center gap-1 rounded px-1.5 text-[11px] transition-colors"
                            title={t("im.copyWebhookUrl")}
                          >
                            {copiedId === ch.channel_id ? (
                              <Check className="text-signal-success size-3" />
                            ) : (
                              <Copy className="size-3" />
                            )}
                          </button>
                        </div>
                      )}
                    </div>

                    <div className="flex shrink-0 items-center gap-1.5">
                      <Button
                        type="button"
                        variant="outline"
                        size="xs"
                        disabled={!isOnline}
                        onClick={() => {
                          setConfirmDeleteId(null);
                          if (isTesting) {
                            setTestingId(null);
                          } else {
                            setTestingId(ch.channel_id);
                            setTestChatId(ch.allowed_chat_ids[0] ?? "");
                            setTestMessage(t("im.testMessageDefault"));
                            setTestResult(null);
                          }
                        }}
                      >
                        <Send className="size-[11px]" />
                        {t("im.test")}
                      </Button>
                      <button
                        type="button"
                        disabled={!isOnline}
                        onClick={() => {
                          setConfirmDeleteId(null);
                          openEditModal(ch);
                        }}
                        title={t("im.editChannel")}
                        className="text-ink-muted hover:text-ink-primary hover:bg-surface-recessed flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                      >
                        <Pencil className="size-[12.5px]" />
                      </button>
                      {isConfirmingDelete ? (
                        <div className="flex items-center gap-1">
                          <Button
                            type="button"
                            variant="destructive"
                            size="xs"
                            disabled={!isOnline}
                            onClick={() => {
                              setConfirmDeleteId(null);
                              void orbitActions.deleteImChannel(hostId, ch.channel_id);
                            }}
                          >
                            {t("common.delete")}
                          </Button>
                          <Button
                            type="button"
                            variant="outline"
                            size="xs"
                            onClick={() => setConfirmDeleteId(null)}
                          >
                            {t("common.cancel")}
                          </Button>
                        </div>
                      ) : (
                        <button
                          type="button"
                          disabled={!isOnline}
                          onClick={() => setConfirmDeleteId(ch.channel_id)}
                          title={t("common.delete")}
                          className="text-ink-muted hover:bg-signal-danger-bg hover:text-signal-danger flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                        >
                          <Trash2 className="size-[12.5px]" />
                        </button>
                      )}
                      <div className="pl-1">
                        <ToggleSwitch
                          checked={ch.enabled}
                          disabled={!isOnline || saving}
                          ariaLabel={ch.name}
                          onChange={(next) => handleToggleEnabled(ch, next)}
                        />
                      </div>
                    </div>
                  </div>

                  {isTesting && (
                    <div className="border-border-subtle bg-surface-recessed flex flex-col gap-2 rounded-lg border p-3">
                      <div className="flex flex-col gap-2 sm:flex-row">
                        <Input
                          type="text"
                          value={testChatId}
                          onChange={(e) => setTestChatId(e.target.value)}
                          placeholder={t("im.testChatPlaceholder", {
                            example: meta.chatPlaceholder,
                          })}
                          className="h-[28px] w-full font-mono text-[12px] sm:w-[190px]"
                        />
                        <Input
                          type="text"
                          value={testMessage}
                          onChange={(e) => setTestMessage(e.target.value)}
                          placeholder={t("im.testMessagePlaceholder")}
                          className="h-[28px] flex-1 text-[12px]"
                        />
                        <Button
                          type="button"
                          size="xs"
                          disabled={testBusy || !testChatId.trim()}
                          onClick={() => void handleSendTest(ch)}
                          className="h-[28px] shrink-0 px-3"
                        >
                          {testBusy ? (
                            <Loader2 className="size-3 animate-spin" />
                          ) : (
                            <Send className="size-3" />
                          )}
                          {t("common.send")}
                        </Button>
                      </div>
                      {lastTest && (
                        <div
                          className={cn(
                            "text-[12px]",
                            lastTest.ok ? "text-signal-success" : "text-signal-danger",
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
              setModalServerError(t("im.errSaveFailed"));
            }
          }}
        />
      )}
    </div>
  );
}
