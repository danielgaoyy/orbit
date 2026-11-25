import { memo, useCallback, useEffect, useMemo, useState } from "react";
import {
  Check,
  Copy,
  Loader2,
  MessageSquare,
  Plus,
  RefreshCw,
  Send,
  Trash2,
  X,
} from "lucide-react";
import {
  SettingCard,
  SettingDrawer,
  SettingResourceRow,
  SettingGroupLabel,
  SettingPageHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
  useHostIsOnline,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { IconButton } from "@/components/ui/icon-button";
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { type MessageKey, useI18n } from "@/lib/i18n";
import {
  type ImChannelRecord,
  type ImConnectionStatus,
  type ImPlatform,
  type ImThreadBinding,
  type ImTransportMode,
  PROVIDER_LIST,
  type ProviderKind,
  type SessionMode,
  type RouterImMirrorPolicy,
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
    showAppId: boolean;
    appIdLabel: string;
    appIdPlaceholder: string;
    primaryLabel: string;
    primaryPlaceholder: string;
    signingLabel: string;
    signingPlaceholder: string;
    signingHintKey?: MessageKey;
    chatPlaceholder: string;
    userPlaceholder: string;
  }
> = {
  lark: {
    label: "Lark / 飞书",
    showAppId: true,
    appIdLabel: "App ID",
    appIdPlaceholder: "cli_a1b2c3d4... (or lark:cli_...)",
    primaryLabel: "App Secret",
    primaryPlaceholder: "Lark / Feishu App Secret",
    signingLabel: "Verification Token / Encrypt Key",
    signingPlaceholder: "Verification Token or Encrypt Key",
    signingHintKey: "im.larkSigningHint",
    chatPlaceholder: "oc_7c9a...",
    userPlaceholder: "ou_3f8b...",
  },
  telegram: {
    label: "Telegram",
    showAppId: false,
    appIdLabel: "",
    appIdPlaceholder: "",
    primaryLabel: "Bot Token",
    primaryPlaceholder: "123456789:AAH...",
    signingLabel: "Webhook Secret Token",
    signingPlaceholder: "X-Telegram-Bot-Api-Secret-Token",
    signingHintKey: "im.telegramSigningHint",
    chatPlaceholder: "-1001234567890",
    userPlaceholder: "123456789",
  },
  slack: {
    label: "Slack",
    showAppId: false,
    appIdLabel: "",
    appIdPlaceholder: "",
    primaryLabel: "Bot Token",
    primaryPlaceholder: "xoxb-...",
    signingLabel: "Signing Secret",
    signingPlaceholder: "Slack App Signing Secret",
    signingHintKey: "im.slackSigningHint",
    chatPlaceholder: "C0123456789",
    userPlaceholder: "U0123456789",
  },
  discord: {
    label: "Discord",
    showAppId: false,
    appIdLabel: "",
    appIdPlaceholder: "",
    primaryLabel: "Bot Token",
    primaryPlaceholder: "Discord Bot Token",
    signingLabel: "Webhook Secret",
    signingPlaceholder: "",
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
          aria-label={label}
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
          className="h-[30px] flex-1 font-mono text-[13px]"
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
              className="border-border-subtle bg-surface-recessed text-ink-primary inline-flex items-center gap-1 rounded-md border px-2 py-0.5 font-mono text-[11px]"
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
  mirrorPolicy: RouterImMirrorPolicy;
}

function ChannelForm({
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
  const canSubmit = Boolean(form.name.trim()) && Boolean(form.defaultWorkspaceCwd.trim());

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
      router_mirror_policy: form.mirrorPolicy,
    });
  };

  const displayError = localError ?? serverError;

  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        if (!saving && canSubmit) void handleSave();
      }}
    >
      <FieldGroup className="gap-4">
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Field>
            <FieldLabel className="text-ink-primary">{t("im.platform")}</FieldLabel>
            <SettingSelect
              ariaLabel={t("im.platform")}
              value={form.platform}
              options={PLATFORM_OPTIONS}
              disabled={isEditing || saving}
              triggerClassName="w-full"
              onChange={(platform) => {
                setForm((f) => ({
                  ...f, platform, appId: "", primarySecret: "", signingSecret: "",
                  appToken: "", publicKey: "", allowedChatIds: [], allowedUserIds: [],
                }));
                setChatDraft("");
                setUserDraft("");
              }}
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
            ariaLabel={t("im.connectionMode")}
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
              <span className="text-ink-primary text-[13px] font-medium">
                {t("im.credentials")}
              </span>
              {isEditing && hasAnySavedSecret && (
                <label
                  className={cn(
                    "text-ink-muted flex cursor-pointer items-center gap-1.5 text-[11px] select-none",
                    saving && "pointer-events-none opacity-55",
                  )}
                >
                  <Checkbox
                    size="sm"
                    checked={form.clearExistingSecrets}
                    disabled={saving}
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
                <FieldLabel htmlFor="im-app-id" className="text-[13px]">
                  {meta.appIdLabel}
                </FieldLabel>
                <Input
                  id="im-app-id"
                  type="text"
                  value={form.appId}
                  disabled={saving}
                  onChange={(e) => setForm((f) => ({ ...f, appId: e.target.value }))}
                  placeholder={meta.appIdPlaceholder}
                  className="font-mono text-[13px]"
                />
              </Field>
            )}

            <Field>
              <div className="flex items-center justify-between">
                <FieldLabel htmlFor="im-primary-secret" className="text-[13px]">
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
                className="font-mono text-[13px]"
              />
            </Field>

            {showSlackAppToken && (
              <Field>
                <div className="flex items-center justify-between">
                  <FieldLabel htmlFor="im-app-token" className="text-[13px]">
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
                  className="font-mono text-[13px]"
                />
                <FieldDescription className="text-[11px]">
                  {t("im.slackAppTokenHint")}
                </FieldDescription>
              </Field>
            )}

            {showDiscordPublicKey && (
              <Field>
                <FieldLabel htmlFor="im-discord-public-key" className="text-[13px]">
                  {t("im.discordPublicKeyLabel")}
                </FieldLabel>
                <Input
                  id="im-discord-public-key"
                  type="text"
                  value={form.publicKey}
                  disabled={saving}
                  onChange={(e) => setForm((f) => ({ ...f, publicKey: e.target.value }))}
                  placeholder={t("im.discordPublicKeyPlaceholder")}
                  className="font-mono text-[13px]"
                />
                <FieldDescription className="text-[11px]">
                  {t("im.discordPublicKeyHint")}
                </FieldDescription>
              </Field>
            )}

            {showSigningSecret && (
              <Field>
                <div className="flex items-center justify-between">
                  <FieldLabel htmlFor="im-signing-secret" className="text-[13px]">
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
                  className="font-mono text-[13px]"
                />
                <FieldDescription className="text-[11px]">
                  {meta.signingHintKey && t(meta.signingHintKey)}
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
            className="font-mono text-[13px]"
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
              ariaLabel={t("im.defaultProvider")}
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
              ariaLabel={t("im.defaultMode")}
              value={toPresetSessionMode(form.defaultMode)}
              options={[
                { value: "default", label: t("common.default") },
                { value: "accept_edits", label: t("composer.modeAcceptEdits") },
                { value: "auto", label: t("composer.modeAuto") + " (Guardian)" },
                { value: "plan", label: t("plan.label") },
                { value: "full_access", label: t("composer.modeFullAccess") },
              ]}
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

      <div className="bg-surface-overlay border-border-subtle sticky -bottom-5 -mx-5 -mb-5 flex justify-end gap-2 border-t px-5 py-3">
        <Button type="button" variant="outline" size="sm" disabled={saving} onClick={onClose}>
          {t("common.cancel")}
        </Button>
        <Button type="submit" size="sm" disabled={saving || !canSubmit}>
          {saving && <Loader2 className="size-[12.5px] animate-spin" />}
          {isEditing ? t("im.saveChanges") : t("im.createChannel")}
        </Button>
      </div>
    </form>

  );
}

const ImTestMessageDrawer = memo(function ImTestMessageDrawer({
  hostId,
  channel,
  baseWebhookUrl,
  isOnline,
  copiedId,
  onCopyWebhook,
  onClose,
}: {
  hostId: string;
  channel: ImChannelRecord | null;
  baseWebhookUrl: string;
  isOnline: boolean;
  copiedId: string | null;
  onCopyWebhook: (channelId: string, fullUrl: string) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [testChatId, setTestChatId] = useState(channel?.allowed_chat_ids[0] ?? "");
  const [testMessage, setTestMessage] = useState(() => t("im.testMessageDefault"));
  const [testBusy, setTestBusy] = useState(false);
  const [testResult, setTestResult] = useState<{
    channelId: string;
    ok: boolean;
    detail: string;
  } | null>(null);

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

  return (
    <SettingDrawer
      open={Boolean(channel)}
      title={t("im.testChannelTitle", { name: channel?.name ?? "" })}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      {channel && (
        <div className="flex flex-col gap-5">
          <div className="text-ink-secondary flex flex-col gap-1 text-[13px]">
            <span>
              {PLATFORM_META[channel.platform].label} ·{" "}
              {resolveTransportMode(channel) === "webhook"
                ? t("im.modeBadgeWebhook")
                : channel.platform === "telegram"
                  ? t("im.modeBadgePolling")
                  : t("im.modeBadgeWs")}
            </span>
            {channel.status_message && <span>{channel.status_message}</span>}
            <span>{t("im.allowlistSummary", {
              chats: channel.allowed_chat_ids.length, users: channel.allowed_user_ids.length,
            })}</span>
            <span>{t("im.messagesCount", { count: channel.message_count })}</span>
            <code className="text-ink-muted break-all text-[11px]">{channel.default_workspace_cwd}</code>
          </div>
          {resolveTransportMode(channel) === "webhook" && (
            <div className="bg-surface-recessed flex items-center justify-between gap-2 rounded-lg p-3">
              <code className="min-w-0 break-all text-[11px]">{baseWebhookUrl}{channel.webhook_path}</code>
              <IconButton
                size="sm"
                tooltip={t("im.copyWebhookUrl")}
                onClick={() => onCopyWebhook(channel.channel_id, baseWebhookUrl + channel.webhook_path)}
              >
                {copiedId === channel.channel_id ? <Check /> : <Copy />}
              </IconButton>
            </div>
          )}
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="im-test-chat">{t("im.testChatLabel")}</FieldLabel>
              <Input
                id="im-test-chat"
                value={testChatId}
                onChange={(e) => setTestChatId(e.target.value)}
                placeholder={PLATFORM_META[channel.platform].chatPlaceholder}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="im-test-message">{t("im.testMessagePlaceholder")}</FieldLabel>
              <Input
                id="im-test-message"
                value={testMessage}
                onChange={(e) => setTestMessage(e.target.value)}
              />
            </Field>
          </FieldGroup>
          <Button
            type="button" size="sm" className="self-end"
            disabled={!isOnline || testBusy || !testChatId.trim()}
            onClick={() => void handleSendTest(channel)}
          >
            {testBusy ? <Loader2 className="animate-spin" /> : <Send data-icon="inline-start" />}
            {t("common.send")}
          </Button>
          {testResult?.channelId === channel.channel_id && (
            <p role="status" className={testResult.ok ? "text-signal-success" : "text-signal-danger"}>
              {testResult.detail}
            </p>
          )}
        </div>
      )}
    </SettingDrawer>
  );
});

export const HostImChannelsSection = memo(function HostImChannelsSection({
  hostId,
}: {
  hostId: string;
}) {
  const { t } = useI18n();
  const isOnline = useHostIsOnline(hostId);
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const fleetProjects = useOrbitSelector((s) => s.fleet.projects);
  const imState = useOrbitSelector((s) => s.imChannelsByHost[hostId]);

  const channels = imState?.channels ?? EMPTY_CHANNELS;
  const bindings = imState?.bindings ?? EMPTY_BINDINGS;
  const baseWebhookUrl = imState?.baseWebhookUrl || "http://127.0.0.1:7788";
  const loading = imState?.status === "loading" && channels.length === 0;
  const saving = Boolean(imState?.saving);
  const error = imState?.error ?? null;

  const [drawerInitial, setDrawerInitial] = useState<ChannelFormState | null>(null);
  const [formServerError, setFormServerError] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);

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
    if (!hostId || !isOnline || !hasTransitioningChannel || saving || drawerInitial || confirmDeleteId) return;
    const timer = window.setInterval(() => {
      void orbitActions.loadImChannels(hostId);
    }, 3_000);
    return () => window.clearInterval(timer);
  }, [hostId, isOnline, hasTransitioningChannel, saving, drawerInitial, confirmDeleteId]);

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

  const openCreateDrawer = () => {
    setFormServerError(null);
    setDrawerInitial({
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
      mirrorPolicy: "milestones_and_approvals",
    });
  };

  const openEditDrawer = (ch: ImChannelRecord) => {
    setFormServerError(null);
    setDrawerInitial({
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
      mirrorPolicy: ch.router_mirror_policy ?? "milestones_and_approvals",
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

  const handleCopyWebhook = useCallback(async (channelId: string, fullUrl: string) => {
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
  }, []);

  const handleCloseTestDrawer = useCallback(() => setTestingId(null), []);

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

  const testChannel = channels.find((ch) => ch.channel_id === testingId);
  const deleteChannel = channels.find((ch) => ch.channel_id === confirmDeleteId);

  return (
    <div className="flex flex-col gap-6">
      <SettingPageHeader
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
              <RefreshCw className={cn(loading && "animate-spin")} />
            </Button>
            <Button type="button" size="sm" data-setting-id="im.addChannel" disabled={!isOnline} onClick={openCreateDrawer}>
              <Plus data-icon="inline-start" />
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

      {error && !drawerInitial && (
        <div className="border-signal-danger-border bg-signal-danger-bg text-signal-danger rounded-xl border px-4 py-3 text-[13px]">
          {error}
        </div>
      )}

      <div data-setting-id="im.configuredChannels" className="flex flex-col gap-2.5">
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
              <p className="text-ink-secondary max-w-[360px] text-[13px] leading-relaxed">
                {t("im.emptyDesc")}
              </p>
            </div>
          </SettingCard>
        ) : (
          <SettingCard>
            {channels.map((ch) => {
              const meta = PLATFORM_META[ch.platform];
              const boundCount = bindingsCountByChannel.get(ch.channel_id) ?? 0;
              const hasAllowlist = Boolean(ch.allowed_chat_ids.length || ch.allowed_user_ids.length);
              const pillLabel = formatStatusPillLabel(ch);
              const detail = ch.status_message?.trim();

              return (
                <SettingResourceRow
                  key={ch.channel_id}
                  icon={<MessageSquare />}
                  title={ch.name}
                  description={
                    <span className="flex flex-wrap items-baseline gap-x-2">
                      <span>{meta.label} · {formatTransportLabel(ch)}</span>
                      {!hasAllowlist && <span className="text-signal-warning">{t("im.allowlistEmpty")}</span>}
                      {boundCount > 0 && <span>{t("im.activeSessions", { count: boundCount })}</span>}
                      {detail && detail !== pillLabel && ch.status === "error" && (
                        <span className="text-signal-danger">{detail}</span>
                      )}
                    </span>
                  }
                  status={
                    <Badge variant={statusBadgeVariant(ch.status, ch.enabled)}>
                      <span className={cn("size-[6px] rounded-full", statusDotClass(ch.status, ch.enabled))} />
                      {pillLabel}
                    </Badge>
                  }
                  onClick={isOnline ? () => openEditDrawer(ch) : undefined}
                  action={
                    <div className="flex items-center gap-2">
                      <IconButton
                        size="sm"
                        tooltip={t("im.test")}
                        disabled={!isOnline}
                        onClick={() => setTestingId(ch.channel_id)}
                      >
                        <Send />
                      </IconButton>
                      <IconButton
                        size="sm"
                        tone="danger"
                        tooltip={t("common.delete")}
                        disabled={!isOnline}
                        onClick={() => setConfirmDeleteId(ch.channel_id)}
                      >
                        <Trash2 />
                      </IconButton>
                      <ToggleSwitch
                        checked={ch.enabled}
                        disabled={!isOnline}
                        ariaLabel={ch.name}
                        onChange={(next) => handleToggleEnabled(ch, next)}
                      />
                    </div>
                  }
                />
              );
            })}
          </SettingCard>
        )}
      </div>

      <ConfirmDialog
        open={Boolean(deleteChannel)}
        onOpenChange={(open) => { if (!open) setConfirmDeleteId(null); }}
        title={t("im.deleteNamedChannel", { name: deleteChannel?.name ?? "" })}
        confirmLabel={t("common.delete")}
        tone="danger"
        onConfirm={async () => {
          if (!deleteChannel || !isOnline) throw new Error(t("im.hostOffline"));
          const snapshot = await orbitActions.deleteImChannel(hostId, deleteChannel.channel_id);
          if (!snapshot) throw new Error(t("common.actionFailed"));
        }}
      />

      <ImTestMessageDrawer
        key={testChannel?.channel_id ?? "closed"}
        hostId={hostId}
        channel={testChannel ?? null}
        baseWebhookUrl={baseWebhookUrl}
        isOnline={isOnline}
        copiedId={copiedId}
        onCopyWebhook={handleCopyWebhook}
        onClose={handleCloseTestDrawer}
      />

      <SettingDrawer
        open={Boolean(drawerInitial)}
        title={drawerInitial?.channelId ? t("im.editChannel") : t("im.addChannelTitle")}
        onOpenChange={(open) => { if (!open && !saving) setDrawerInitial(null); }}
      >
        {drawerInitial && (
          <ChannelForm
            initial={drawerInitial}
            workspaceOptions={workspaceOptions}
            saving={saving}
            serverError={formServerError ?? error}
            onClose={() => setDrawerInitial(null)}
            onSubmit={async (req) => {
              if (!isOnline) {
                setFormServerError(t("im.hostOffline"));
                return;
              }
              setFormServerError(null);
              const snapshot = await orbitActions.upsertImChannel(hostId, req);
              if (snapshot) setDrawerInitial(null);
              else setFormServerError(orbitActions.getSnapshot().imChannelsByHost[hostId]?.error ?? t("im.errSaveFailed"));
            }}
          />
        )}
      </SettingDrawer>
    </div>
  );
});
