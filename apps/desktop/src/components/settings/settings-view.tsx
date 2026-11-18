import { useMemo, useState } from "react";
import {
  Bell,
  Check,
  Copy,
  Cpu,
  ExternalLink,
  GitBranch,
  Keyboard,
  Loader2,
  Monitor,
  Moon,
  Plus,
  Radio,
  RefreshCw,
  Search,
  Server,
  Settings2,
  ShieldCheck,
  Sun,
  Terminal as TerminalIcon,
  Trash2,
  Wifi,
  WifiOff,
  X,
} from "lucide-react";
import {
  type HostCardView,
  type ProviderKind,
  type SessionMode,
  type SshHostItem,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { modSymbol } from "@/lib/keybindings";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { ShortcutKbd } from "@/components/ui/overlays";
import {
  type SettingsSection,
  type TerminalCursorStyle,
  type ThemeMode,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const SECTIONS: {
  id: SettingsSection;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
}[] = [
  { id: "general", label: "General", icon: Settings2 },
  { id: "hosts", label: "Hosts & Relay", icon: Server },
  { id: "providers", label: "Providers & Quotas", icon: Cpu },
  { id: "worktrees", label: "Git & Worktrees", icon: GitBranch },
  { id: "editor_terminal", label: "Editor & Terminal", icon: TerminalIcon },
  { id: "keybindings", label: "Keybindings", icon: Keyboard },
];

function encodeHostPairingUri(host: HostCardView): string {
  const offer = {
    host_id: host.host_id,
    hostname: host.display_name,
    daemon_pubkey_b64: host.daemon_pubkey_b64,
    direct_endpoints: host.direct_endpoints,
    relay_url: host.relay_url,
    pairing_secret: "orbit-desktop-export",
  };
  const bytes = new TextEncoder().encode(JSON.stringify(offer));
  let bin = "";
  bytes.forEach((b) => {
    bin += String.fromCharCode(b);
  });
  const b64Url = btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return `orbit://pair/${b64Url}`;
}

function ToggleSwitch({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border border-transparent transition-colors",
        checked ? "bg-foreground" : "bg-muted-foreground/25",
      )}
    >
      <span
        className={cn(
          "pointer-events-none inline-block size-4 rounded-full shadow-xs transition-transform",
          checked ? "bg-background translate-x-4" : "translate-x-0.5 bg-white",
        )}
      />
    </button>
  );
}

function SettingSectionHeader({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <div className="space-y-1 pb-2">
      <h2 className="text-foreground text-[18px] font-semibold tracking-[-0.015em]">{title}</h2>
      <p className="text-muted-foreground text-[13px] leading-relaxed">{description}</p>
    </div>
  );
}

function SettingCard({ children }: { children: React.ReactNode }) {
  return (
    <div className="border-border bg-background/55 divide-border divide-y overflow-hidden rounded-[14px] border">
      {children}
    </div>
  );
}

function SettingRow({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-3 px-4.5 py-3.5 sm:flex-row sm:items-center sm:justify-between">
      <div className="min-w-0 space-y-0.5 pr-4">
        <div className="text-foreground text-[13.5px] font-medium">{title}</div>
        {description && (
          <div className="text-muted-foreground text-[12px] leading-relaxed">{description}</div>
        )}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function GeneralSection() {
  const theme = useOrbitSelector((s) => s.theme);
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const notifyOnApproval = useOrbitSelector((s) => s.notifyOnApproval);
  const notifyOnTurnComplete = useOrbitSelector((s) => s.notifyOnTurnComplete);
  const mod = modSymbol();

  const [notifPerm, setNotifPerm] = useState<string>(() =>
    typeof window !== "undefined" && "Notification" in window
      ? Notification.permission
      : "unsupported",
  );

  const requestOsNotifications = async () => {
    if (typeof window === "undefined" || !("Notification" in window)) return;
    const res = await Notification.requestPermission();
    setNotifPerm(res);
  };

  const themes: {
    id: ThemeMode;
    label: string;
    icon: React.ComponentType<{ className?: string }>;
  }[] = [
    { id: "system", label: "System", icon: Monitor },
    { id: "light", label: "Light", icon: Sun },
    { id: "dark", label: "Dark", icon: Moon },
  ];

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="General"
        description="Appearance, prompt submission behavior, and background attention alerts."
      />

      <SettingCard>
        <SettingRow
          title="Theme"
          description="Follow macOS system appearance or lock light/dark palette."
        >
          <div className="bg-muted/85 inline-flex items-center gap-0.5 rounded-[10px] p-1">
            {themes.map((t) => {
              const Icon = t.icon;
              const active = theme === t.id;
              return (
                <button
                  key={t.id}
                  type="button"
                  onClick={() => orbitActions.setTheme(t.id)}
                  className={cn(
                    "inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-[7px] px-2.5 text-xs font-medium transition-colors",
                    active
                      ? "bg-elevated text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  <Icon className="size-3.5 opacity-80" />
                  <span>{t.label}</span>
                </button>
              );
            })}
          </div>
        </SettingRow>

        <SettingRow
          title="Send prompt with Enter"
          description={
            enterToSend
              ? "Press Enter to send or queue; press Shift+Enter for a new line."
              : `Press ${mod}Enter to send; press Enter for a new line.`
          }
        >
          <ToggleSwitch
            checked={enterToSend}
            onChange={(v) => orbitActions.setEnterToSend(v)}
          />
        </SettingRow>
      </SettingCard>

      {/* Notifications */}
      <div className="space-y-2">
        <div className="flex items-center justify-between px-0.5">
          <span className="text-muted-foreground text-xs font-medium">Notifications</span>
          {notifPerm === "default" && (
            <button
              type="button"
              onClick={() => void requestOsNotifications()}
              className="text-foreground inline-flex cursor-pointer items-center gap-1 text-xs font-medium hover:underline"
            >
              <Bell className="size-3" /> Enable OS Notifications
            </button>
          )}
        </div>
        <SettingCard>
          <SettingRow
            title="Approval & question alerts"
            description="Notify when a background agent blocks awaiting tool permission or clarification."
          >
            <ToggleSwitch
              checked={notifyOnApproval}
              onChange={(v) => orbitActions.setNotificationPrefs({ notifyOnApproval: v })}
            />
          </SettingRow>
          <SettingRow
            title="Background turn completion"
            description="Notify when a running task finishes while the Orbit window is unfocused."
          >
            <ToggleSwitch
              checked={notifyOnTurnComplete}
              onChange={(v) => orbitActions.setNotificationPrefs({ notifyOnTurnComplete: v })}
            />
          </SettingRow>
        </SettingCard>
      </div>
    </div>
  );
}

function HostsAndRelaySection() {
  const fleet = useOrbitSelector((s) => s.fleet);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const sshHosts = useOrbitSelector((s) => s.sshHosts);

  const [pairingUri, setPairingUri] = useState("");
  const [pairError, setPairError] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const [relayTargetHostId, setRelayTargetHostId] = useState<string>(
    selectedHostId ?? fleet.hosts[0]?.host_id ?? "",
  );
  const effectiveRelayHost = useMemo(
    () =>
      fleet.hosts.find((h) => h.host_id === relayTargetHostId) ??
      fleet.hosts.find((h) => h.host_id === selectedHostId) ??
      fleet.hosts[0] ??
      null,
    [fleet.hosts, relayTargetHostId, selectedHostId],
  );

  const [relayInput, setRelayInput] = useState(effectiveRelayHost?.relay_url ?? "");
  const [relayStatusMsg, setRelayStatusMsg] = useState<string | null>(null);

  const handlePair = async () => {
    if (!pairingUri.trim()) return;
    setPairError(null);
    try {
      const card = await orbitIpc.pairHost(pairingUri.trim());
      orbitActions.selectHost(card.host_id);
      setPairingUri("");
      orbitActions.showToast(`Paired host ${card.display_name}`);
    } catch (err) {
      setPairError(String(err));
    }
  };

  const handleCopyPairing = async (host: HostCardView) => {
    await navigator.clipboard.writeText(encodeHostPairingUri(host));
    setCopiedId(host.host_id);
    setTimeout(() => setCopiedId(null), 1600);
  };

  const handleSaveRelay = async (clear = false) => {
    if (!effectiveRelayHost) return;
    const val = clear || !relayInput.trim() ? null : relayInput.trim();
    await orbitIpc.updateHostRelay(effectiveRelayHost.host_id, val);
    if (clear) setRelayInput("");
    orbitActions.showToast(val ? "Updated E2EE Relay endpoint" : "Reverted to LAN Direct only");
  };

  const handleDeploySshRelay = async (ssh: SshHostItem) => {
    if (!effectiveRelayHost) return;
    setRelayStatusMsg(`Deploying allowlisted zero-knowledge Relay to ${ssh.alias}...`);
    try {
      const url = await orbitIpc.provisionSshRelay({
        targetHostId: effectiveRelayHost.host_id,
        sshAlias: ssh.alias,
        fallbackHostname: ssh.host_name,
        relayPort: 7878,
      });
      setRelayInput(url);
      setRelayStatusMsg(`Active on ${url}`);
    } catch (err) {
      setRelayStatusMsg(`Failed: ${String(err)}`);
    }
  };

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Hosts & Zero-Knowledge Relay"
        description="Manage local and remote Orbit Daemons across your Devboxes, workstations, and private Noise_NK E2EE relays."
      />

      {/* Pair New Host */}
      <div className="space-y-2">
        <div className="text-foreground text-xs font-medium">Pair Host</div>
        <div className="border-border bg-elevated/55 space-y-2.5 rounded-xl border p-4">
          <div className="text-muted-foreground text-xs">
            Run <code className="bg-muted rounded px-1.5 py-0.5 font-mono">orbit pair</code> on any
            machine and paste the <code className="font-mono">orbit://pair/...</code> credential URI:
          </div>
          <div className="flex gap-2">
            <input
              type="text"
              value={pairingUri}
              onChange={(e) => setPairingUri(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void handlePair();
              }}
              placeholder="orbit://pair/eyJob3N0X2lkIjoi..."
              className="border-border bg-background h-8 flex-1 rounded-md border px-3 font-mono text-xs outline-none focus:border-primary/60"
            />
            <button
              type="button"
              onClick={() => void handlePair()}
              className="bg-primary text-primary-foreground inline-flex h-8 shrink-0 cursor-pointer items-center gap-1.5 rounded-md px-3.5 text-xs font-medium"
            >
              <Plus className="size-3.5" /> Pair & Connect
            </button>
          </div>
          {pairError && <div className="text-xs text-rose-500">{pairError}</div>}
        </div>
      </div>

      {/* Paired Hosts Fleet List */}
      <div className="space-y-2">
        <div className="text-foreground text-xs font-medium">
          Paired Fleet ({fleet.hosts.length})
        </div>
        <SettingCard>
          {fleet.hosts.length === 0 ? (
            <div className="text-muted-foreground p-6 text-center text-xs">
              No hosts paired yet.
            </div>
          ) : (
            fleet.hosts.map((h) => (
              <div key={h.host_id} className="space-y-2.5 px-4 py-3.5">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 items-center gap-2">
                    {h.connection_state === "online_direct" ? (
                      <Wifi className="size-3.5 shrink-0 text-emerald-500" />
                    ) : h.connection_state === "online_relay" ? (
                      <ShieldCheck className="size-3.5 shrink-0 text-indigo-500" />
                    ) : h.connection_state.startsWith("connecting") ? (
                      <Loader2 className="size-3.5 shrink-0 animate-spin text-amber-500" />
                    ) : (
                      <WifiOff className="text-muted-foreground size-3.5 shrink-0" />
                    )}
                    <span className="text-foreground truncate text-sm font-semibold">
                      {h.display_name}
                    </span>
                    <span className="text-muted-foreground bg-muted rounded px-1.5 py-0.5 font-mono text-[10px]">
                      {h.connection_state.replace("_", " ")}
                    </span>
                  </div>

                  <div className="flex shrink-0 items-center gap-1.5">
                    <button
                      type="button"
                      onClick={() => void handleCopyPairing(h)}
                      className="border-border hover:bg-accent inline-flex h-7 cursor-pointer items-center gap-1 rounded-md border px-2 text-[11px]"
                    >
                      {copiedId === h.host_id ? (
                        <Check className="size-3 text-emerald-500" />
                      ) : (
                        <Copy className="size-3" />
                      )}
                      <span>{copiedId === h.host_id ? "Copied URI" : "Copy Mobile URI"}</span>
                    </button>
                    <button
                      type="button"
                      title="Reconnect"
                      onClick={() => {
                        void orbitIpc.connectHost(h.host_id);
                        void orbitIpc.refreshHost(h.host_id);
                      }}
                      className="border-border hover:bg-accent inline-flex size-7 cursor-pointer items-center justify-center rounded-md border"
                    >
                      <RefreshCw className="size-3" />
                    </button>
                    {fleet.hosts.length > 1 && (
                      <button
                        type="button"
                        title="Unpair host"
                        onClick={() => void orbitIpc.removeHost(h.host_id)}
                        className="inline-flex size-7 cursor-pointer items-center justify-center rounded-md border border-rose-500/25 text-rose-500 hover:bg-rose-500/10"
                      >
                        <Trash2 className="size-3" />
                      </button>
                    )}
                  </div>
                </div>

                <div className="text-muted-foreground grid grid-cols-1 gap-1 font-mono text-[11px] sm:grid-cols-2">
                  <div className="truncate">ID: {h.host_id}</div>
                  <div className="truncate">Pubkey: {h.daemon_pubkey_b64.slice(0, 22)}…</div>
                  <div className="truncate sm:col-span-2">
                    Endpoints: {h.direct_endpoints.join(", ") || "none"}
                    {h.relay_url ? ` · Relay: ${h.relay_url}` : ""}
                  </div>
                </div>
              </div>
            ))
          )}
        </SettingCard>
      </div>

      {/* Zero-Knowledge Relay Provisioner */}
      {effectiveRelayHost && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-foreground text-xs font-medium">
              Private Noise E2EE Relay
            </span>
            {fleet.hosts.length > 1 && (
              <div className="flex gap-1">
                {fleet.hosts.map((h) => (
                  <button
                    key={h.host_id}
                    type="button"
                    onClick={() => {
                      setRelayTargetHostId(h.host_id);
                      setRelayInput(h.relay_url ?? "");
                    }}
                    className={cn(
                      "cursor-pointer rounded px-2 py-0.5 text-[11px]",
                      effectiveRelayHost.host_id === h.host_id
                        ? "bg-accent text-foreground font-medium"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {h.display_name}
                  </button>
                ))}
              </div>
            )}
          </div>

          <div className="border-border bg-elevated/55 space-y-3 rounded-xl border p-4">
            <p className="text-muted-foreground text-xs leading-relaxed">
              Orbit Relays forward only <code className="font-mono">Noise_NK</code> ciphertext and
              enforce <code className="font-mono">--allow-pubkey</code> admission locking.
            </p>

            <div className="flex gap-2">
              <input
                type="text"
                value={relayInput}
                onChange={(e) => setRelayInput(e.target.value)}
                placeholder="ws://your-vps:7878 or wss://relay.example.com"
                className="border-border bg-background h-8 flex-1 rounded-md border px-3 font-mono text-xs outline-none focus:border-primary/60"
              />
              <button
                type="button"
                onClick={() => void handleSaveRelay(false)}
                className="bg-primary text-primary-foreground inline-flex h-8 cursor-pointer items-center rounded-md px-3 text-xs font-medium"
              >
                Save Relay
              </button>
              {effectiveRelayHost.relay_url && (
                <button
                  type="button"
                  onClick={() => void handleSaveRelay(true)}
                  className="border-border hover:bg-accent inline-flex h-8 cursor-pointer items-center rounded-md border px-2.5 text-xs"
                >
                  Clear
                </button>
              )}
            </div>

            {sshHosts.length > 0 && (
              <div className="space-y-1.5 pt-1">
                <div className="text-muted-foreground text-[11px] font-medium">
                  One-Click SSH Provisioning (~/.ssh/config)
                </div>
                <div className="border-border divide-border max-h-44 divide-y overflow-y-auto rounded-lg border">
                  {sshHosts.map((s) => (
                    <div
                      key={s.alias}
                      className="flex items-center justify-between px-3 py-2 text-xs"
                    >
                      <div>
                        <span className="font-medium">{s.alias}</span>
                        <span className="text-muted-foreground ml-2 font-mono text-[11px]">
                          {s.display_target}
                        </span>
                      </div>
                      <button
                        type="button"
                        onClick={() => void handleDeploySshRelay(s)}
                        className="border-border hover:bg-accent inline-flex h-6.5 cursor-pointer items-center gap-1 rounded border px-2.5 text-[11px] font-medium"
                      >
                        <Radio className="size-3" /> Deploy & Attach
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {relayStatusMsg && <div className="text-primary text-xs">{relayStatusMsg}</div>}
          </div>
        </div>
      )}
    </div>
  );
}

function ProvidersAndQuotasSection() {
  const draftProvider = useOrbitSelector((s) => s.draftProvider);
  const draftMode = useOrbitSelector((s) => s.draftMode);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );

  const providers: { id: ProviderKind; label: string; bin: string; protocol: string }[] = [
    { id: "claude_code", label: "Claude Code", bin: "claude", protocol: "stream-json control" },
    { id: "codex", label: "OpenAI Codex", bin: "codex", protocol: "app-server JSON-RPC 2.0" },
    { id: "antigravity", label: "Antigravity", bin: "agy", protocol: "ACP stdio + jsonl delta" },
  ];
  const detectedProviders = hostDetail
    ? providers.filter((p) => hostDetail.host.available_providers.includes(p.id))
    : providers;

  const modes: { id: SessionMode; label: string }[] = [
    { id: "default", label: "Always Ask" },
    { id: "accept_edits", label: "Accept Edits" },
    { id: "full_access", label: "Full Access" },
  ];

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Providers & Quotas"
        description="Configure default agent harnesses, execution postures, and monitor live subscription windows."
      />

      <div className="space-y-2">
        <div className="text-foreground text-xs font-medium">New Session Defaults</div>
        <SettingCard>
          <SettingRow
            title="Default Agent Engine"
            description="Pre-selected harness when opening a new session (⌘N)."
          >
            <div className="bg-muted inline-flex rounded-lg p-0.5 text-xs">
              {(detectedProviders.length > 0 ? detectedProviders : providers).map((p) => (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => orbitActions.setDraftConfig({ provider: p.id })}
                  className={cn(
                    "inline-flex cursor-pointer items-center gap-1.5 rounded-md px-2.5 py-1 font-medium transition-colors",
                    draftProvider === p.id
                      ? "bg-background text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  <ProviderIcon provider={p.id} className="size-3.5" />
                  <span>{p.label.replace("OpenAI ", "")}</span>
                </button>
              ))}
            </div>
          </SettingRow>

          <SettingRow
            title="Default Access Permissions"
            description="Initial approval boundary for new sessions (cycle anytime with ⇧Tab)."
          >
            <div className="bg-muted inline-flex rounded-lg p-0.5 text-xs">
              {modes.map((m) => (
                <button
                  key={String(m.id)}
                  type="button"
                  onClick={() => orbitActions.setDraftConfig({ mode: m.id })}
                  className={cn(
                    "cursor-pointer rounded-md px-2.5 py-1 font-medium transition-colors",
                    draftMode === m.id
                      ? "bg-background text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {m.label}
                </button>
              ))}
            </div>
          </SettingRow>
        </SettingCard>
      </div>

      {/* Installed CLIs on Current Host */}
      {hostDetail && (
        <div className="space-y-2">
          <div className="text-foreground text-xs font-medium">
            Detected CLI Binaries on {hostDetail.host.display_name}
          </div>
          <SettingCard>
            {providers.map((p) => {
              const detected = hostDetail.host.available_providers.includes(p.id);
              return (
                <SettingRow
                  key={p.id}
                  title={p.label}
                  description={`Binary: ${p.bin} · Driver: ${p.protocol}`}
                >
                  <span
                    className={cn(
                      "inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-[11px] font-medium",
                      detected
                        ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                        : "border-border text-muted-foreground",
                    )}
                  >
                    <span
                      className={cn(
                        "size-1.5 rounded-full",
                        detected ? "bg-emerald-500" : "bg-zinc-400",
                      )}
                    />
                    {detected ? "Detected" : "Not on PATH"}
                  </span>
                </SettingRow>
              );
            })}
          </SettingCard>
        </div>
      )}

      {/* Live Subscription Quotas */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-foreground text-xs font-medium">Live Provider Quotas</span>
          {selectedHostId && (
            <button
              type="button"
              onClick={() => void orbitIpc.refreshQuotas(selectedHostId)}
              className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 text-xs"
            >
              <RefreshCw className="size-3" /> Refresh
            </button>
          )}
        </div>
        <SettingCard>
          {!hostDetail || hostDetail.quotas.length === 0 ? (
            <div className="text-muted-foreground p-6 text-center text-xs">
              No quota snapshots available from the active host.
            </div>
          ) : (
            hostDetail.quotas.map((q) => (
              <div key={q.provider} className="space-y-2.5 px-4 py-3.5">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <ProviderIcon provider={q.provider} className="size-3.5" />
                    <span className="text-foreground text-xs font-semibold capitalize">
                      {q.provider.replace("_", " ")}
                    </span>
                    {q.account_label && (
                      <span className="text-muted-foreground text-xs">({q.account_label})</span>
                    )}
                  </div>
                  {q.credits_remaining !== null && (
                    <span className="tabular-nums font-mono text-xs text-emerald-600 dark:text-emerald-400">
                      {q.credits_remaining.toFixed(2)} credits
                    </span>
                  )}
                </div>

                {q.error ? (
                  <div className="text-muted-foreground text-xs">{q.error}</div>
                ) : (
                  <div className="space-y-2">
                    {q.windows.map((w) => (
                      <div key={w.label} className="space-y-1">
                        <div className="text-muted-foreground tabular-nums flex justify-between text-[11px]">
                          <span>{w.label}</span>
                          <span>{Math.round(w.used_percent)}% used</span>
                        </div>
                        <div className="bg-muted h-1.5 w-full overflow-hidden rounded-full">
                          <div
                            className={cn(
                              "h-full transition-all",
                              w.used_percent >= 80 ? "bg-amber-500" : "bg-primary",
                            )}
                            style={{ width: `${Math.min(100, Math.max(0, w.used_percent))}%` }}
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ))
          )}
        </SettingCard>
      </div>
    </div>
  );
}

function WorktreesAndReviewSection() {
  const draftUseWorktree = useOrbitSelector((s) => s.draftUseWorktree);
  const worktreeBranchPrefix = useOrbitSelector((s) => s.worktreeBranchPrefix);
  const diffLayout = useOrbitSelector((s) => s.diffLayout);
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);

  const presetPrefixes = ["auto", "orbit/", "feat/", "fix/"];

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Git & Worktrees"
        description="Isolate parallel coding agents in dedicated Git worktrees and customize code review presentation."
      />

      <div className="space-y-2">
        <div className="text-foreground text-xs font-medium">Parallel Worktree Isolation</div>
        <SettingCard>
          <SettingRow
            title="Isolate new threads in Git worktrees by default"
            description="Provisions a clean checkout under ~/.orbit/worktrees/ so concurrent agents never overwrite each other's files."
          >
            <ToggleSwitch
              checked={draftUseWorktree}
              onChange={(v) => orbitActions.setDraftConfig({ useWorktree: v })}
            />
          </SettingRow>

          <SettingRow
            title="Automatic branch naming convention"
            description="Choose 'auto' for daemon semantic branch naming or enforce a consistent Git branch prefix."
          >
            <div className="flex items-center gap-1.5">
              <div className="bg-muted inline-flex rounded-lg p-0.5 text-xs">
                {presetPrefixes.map((pfx) => (
                  <button
                    key={pfx}
                    type="button"
                    onClick={() => orbitActions.setWorktreePrefs({ worktreeBranchPrefix: pfx })}
                    className={cn(
                      "cursor-pointer rounded-md px-2 py-1 font-mono text-[11px] transition-colors",
                      worktreeBranchPrefix === pfx
                        ? "bg-background text-foreground font-medium shadow-2xs"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {pfx}
                  </button>
                ))}
              </div>
              <input
                type="text"
                value={worktreeBranchPrefix}
                onChange={(e) =>
                  orbitActions.setWorktreePrefs({ worktreeBranchPrefix: e.target.value })
                }
                className="border-border bg-background h-7.5 w-24 rounded-md border px-2 font-mono text-xs outline-none focus:border-primary/60"
              />
            </div>
          </SettingRow>
        </SettingCard>
      </div>

      <div className="space-y-2">
        <div className="text-foreground text-xs font-medium">Timeline & Code Review</div>
        <SettingCard>
          <SettingRow
            title="Default Diff Inspector layout"
            description="Preferred comparison view when opening the right-hand Review panel (⌘D)."
          >
            <div className="bg-muted inline-flex rounded-lg p-0.5 text-xs">
              <button
                type="button"
                onClick={() => orbitActions.setDiffLayout("unified")}
                className={cn(
                  "cursor-pointer rounded-md px-2.5 py-1 font-medium",
                  diffLayout === "unified"
                    ? "bg-background text-foreground shadow-2xs"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                Unified
              </button>
              <button
                type="button"
                onClick={() => orbitActions.setDiffLayout("split")}
                className={cn(
                  "cursor-pointer rounded-md px-2.5 py-1 font-medium",
                  diffLayout === "split"
                    ? "bg-background text-foreground shadow-2xs"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                Split (Side-by-Side)
              </button>
            </div>
          </SettingRow>

          <SettingRow
            title="Coalesce read-only exploration steps"
            description="Group consecutive file reads, searches, and thinking traces into calm 1-line summaries."
          >
            <ToggleSwitch
              checked={coalesceExploration}
              onChange={(v) => orbitActions.setWorktreePrefs({ coalesceExploration: v })}
            />
          </SettingRow>
        </SettingCard>
      </div>
    </div>
  );
}

function EditorAndTerminalSection() {
  const externalEditors = useOrbitSelector((s) => s.externalEditors);
  const preferredEditor = useOrbitSelector((s) => s.preferredEditor);
  const terminalFontSize = useOrbitSelector((s) => s.terminalFontSize);
  const terminalCursorStyle = useOrbitSelector((s) => s.terminalCursorStyle);
  const terminalScrollback = useOrbitSelector((s) => s.terminalScrollback);
  const mod = modSymbol();

  const effectivePreferred =
    preferredEditor ?? externalEditors.find((e) => e.installed)?.kind ?? "cursor";

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Editor & Terminal"
        description="Configure one-click IDE handoff (local & SSH remote) and customize the persistent WebGL PTY terminal."
      />

      {/* External Editor Handoff */}
      <div className="space-y-2">
        <div className="text-foreground text-xs font-medium">
          Default External Editor ({mod}O)
        </div>
        <SettingCard>
          <div className="grid grid-cols-2 gap-2 p-3 sm:grid-cols-3">
            {externalEditors.map((ed) => {
              const selected = effectivePreferred === ed.kind;
              return (
                <button
                  key={ed.kind}
                  type="button"
                  onClick={() => orbitActions.setPreferredEditor(ed.kind)}
                  className={cn(
                    "flex cursor-pointer items-center justify-between rounded-lg border px-3 py-2.5 text-left text-xs transition-colors",
                    selected
                      ? "border-primary bg-primary/[0.08] text-foreground font-semibold"
                      : "border-border hover:bg-accent/50 text-foreground/85",
                  )}
                >
                  <span className="flex items-center gap-2 truncate">
                    <ExternalLink className="size-3.5 shrink-0 opacity-70" />
                    <span className="truncate">{ed.label}</span>
                  </span>
                  {ed.installed && (
                    <span className="size-1.5 shrink-0 rounded-full bg-emerald-500" title="Detected" />
                  )}
                </button>
              );
            })}
          </div>
        </SettingCard>
      </div>

      {/* Integrated PTY Terminal */}
      <div className="space-y-2">
        <div className="text-foreground text-xs font-medium">Integrated PTY Terminal</div>
        <SettingCard>
          <SettingRow
            title="Font size"
            description="Monospace font size in pixels for WebGL terminal panes."
          >
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => orbitActions.setTerminalPrefs({ fontSize: terminalFontSize - 0.5 })}
                className="border-border hover:bg-accent inline-flex size-7 cursor-pointer items-center justify-center rounded-md border font-mono text-xs"
              >
                -
              </button>
              <span className="tabular-nums w-12 text-center font-mono text-xs font-medium">
                {terminalFontSize}px
              </span>
              <button
                type="button"
                onClick={() => orbitActions.setTerminalPrefs({ fontSize: terminalFontSize + 0.5 })}
                className="border-border hover:bg-accent inline-flex size-7 cursor-pointer items-center justify-center rounded-md border font-mono text-xs"
              >
                +
              </button>
            </div>
          </SettingRow>

          <SettingRow
            title="Cursor style"
            description="Visual cursor shape inside active shell sessions."
          >
            <div className="bg-muted inline-flex rounded-lg p-0.5 text-xs">
              {(["bar", "block", "underline"] as TerminalCursorStyle[]).map((style) => (
                <button
                  key={style}
                  type="button"
                  onClick={() => orbitActions.setTerminalPrefs({ cursorStyle: style })}
                  className={cn(
                    "cursor-pointer rounded-md px-2.5 py-1 font-medium capitalize",
                    terminalCursorStyle === style
                      ? "bg-background text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {style}
                </button>
              ))}
            </div>
          </SettingRow>

          <SettingRow
            title="Scrollback buffer"
            description="Maximum lines retained in client terminal scrollback."
          >
            <div className="bg-muted inline-flex rounded-lg p-0.5 text-xs">
              {[2000, 5000, 10000, 25000].map((lines) => (
                <button
                  key={lines}
                  type="button"
                  onClick={() => orbitActions.setTerminalPrefs({ scrollback: lines })}
                  className={cn(
                    "tabular-nums cursor-pointer rounded-md px-2.5 py-1 font-mono text-[11px]",
                    terminalScrollback === lines
                      ? "bg-background text-foreground font-medium shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {lines.toLocaleString()}
                </button>
              ))}
            </div>
          </SettingRow>
        </SettingCard>
      </div>
    </div>
  );
}

function KeybindingsSection() {
  const [filter, setFilter] = useState("");
  const mod = modSymbol();

  const bindings = useMemo(
    () => [
      { group: "Global Navigation", action: "Open Command Palette", keys: `${mod}K`, scope: "Global" },
      { group: "Global Navigation", action: "Open / Close Settings", keys: `${mod},`, scope: "Global" },
      { group: "Global Navigation", action: "New Agent Session", keys: `${mod}N`, scope: "Workspace" },
      {
        group: "Global Navigation",
        action: "Jump to Next Pending Approval",
        keys: `${mod}⇧A`,
        scope: "Multi-Host",
      },
      {
        group: "Global Navigation",
        action: "Switch Active Thread 1–9",
        keys: `${mod}1 .. ${mod}9`,
        scope: "Workspace",
      },
      {
        group: "Panels & Handoff",
        action: "Toggle Fleet Sidebar (Zen Mode)",
        keys: `${mod}B`,
        scope: "Workspace",
      },
      {
        group: "Panels & Handoff",
        action: "Toggle Diff Review Inspector",
        keys: `${mod}D`,
        scope: "Workspace",
      },
      {
        group: "Panels & Handoff",
        action: "Toggle Persistent PTY Terminal",
        keys: `${mod}J / ⌃\``,
        scope: "Global",
      },
      {
        group: "Panels & Handoff",
        action: "Open Workspace in External Editor",
        keys: `${mod}O`,
        scope: "Workspace",
      },
      {
        group: "Composer & Steering",
        action: "Cycle Access Permissions (Always Ask / Accept Edits / Full Access)",
        keys: "⇧Tab",
        scope: "Composer",
      },
      {
        group: "Composer & Steering",
        action: "Send Prompt (or Queue when running)",
        keys: "Enter",
        scope: "Composer",
      },
      {
        group: "Composer & Steering",
        action: "Steer Immediately (Interrupt & Send)",
        keys: "⌥Enter",
        scope: "Running Turn",
      },
      {
        group: "Composer & Steering",
        action: "Interrupt Active Agent Turn",
        keys: "Esc Esc",
        scope: "Running Turn",
      },
      {
        group: "Approvals & Safety",
        action: "Allow Pending Tool Request",
        keys: `${mod}Y`,
        scope: "Approval Dock",
      },
      {
        group: "Approvals & Safety",
        action: "Always Allow Tool in Session",
        keys: `${mod}⇧Y`,
        scope: "Approval Dock",
      },
      {
        group: "Approvals & Safety",
        action: "Deny Pending Tool Request",
        keys: `${mod}⌫`,
        scope: "Approval Dock",
      },
    ],
    [mod],
  );

  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return bindings;
    return bindings.filter(
      (b) =>
        b.action.toLowerCase().includes(q) ||
        b.group.toLowerCase().includes(q) ||
        b.keys.toLowerCase().includes(q),
    );
  }, [bindings, filter]);

  return (
    <div className="space-y-5">
      <SettingSectionHeader
        title="Keybindings"
        description="Focus-guarded ergonomic keyboard shortcuts across macOS, Linux, and Windows."
      />

      <div className="border-border bg-elevated/55 flex h-8.5 items-center gap-2 rounded-lg border px-3">
        <Search className="text-muted-foreground size-3.5 shrink-0" />
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter shortcuts..."
          className="text-foreground placeholder:text-muted-foreground w-full bg-transparent text-xs outline-none"
        />
      </div>

      <SettingCard>
        {visible.map((b, idx) => (
          <div
            key={`${b.action}-${idx}`}
            className="flex items-center justify-between gap-4 px-4 py-2.5 text-xs"
          >
            <div className="min-w-0">
              <div className="text-foreground font-medium">{b.action}</div>
              <div className="text-muted-foreground text-[10.5px]">
                {b.group} · <span className="font-mono">{b.scope}</span>
              </div>
            </div>
            <ShortcutKbd keys={b.keys} className="shrink-0" />
          </div>
        ))}
      </SettingCard>
    </div>
  );
}

export function SettingsView() {
  const section = useOrbitSelector((s) => s.settingsSection);

  return (
    <div
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) {
          orbitActions.closeSettings();
        }
      }}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6 backdrop-blur-[2px] select-none"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        className="border-border bg-elevated text-foreground floating-surface relative flex h-[min(720px,86vh)] w-[min(1040px,92vw)] overflow-hidden rounded-[18px] border"
      >
        <button
          type="button"
          aria-label="Close settings"
          onClick={() => orbitActions.closeSettings()}
          className="text-muted-foreground hover:text-foreground hover:bg-accent absolute top-4 right-4 z-10 inline-flex size-8 cursor-pointer items-center justify-center rounded-[10px] transition-colors"
        >
          <X className="size-4" />
        </button>

        {/* Left Settings Modal Rail (240px Antigravity Alignment) */}
        <aside className="border-border bg-sidebar/70 flex w-[240px] shrink-0 flex-col border-r px-3.5 py-5">
          <div className="px-3 pb-3.5 text-[15px] font-semibold tracking-[-0.01em]">
            Settings
          </div>

          <nav className="flex-1 space-y-0.5">
            {SECTIONS.map((item) => {
              const Icon = item.icon;
              const active = section === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => orbitActions.setSettingsSection(item.id)}
                  className={cn(
                    "flex h-9 w-full cursor-pointer items-center gap-2.5 rounded-[10px] px-3 text-left text-[13.5px] transition-colors",
                    active
                      ? "bg-accent text-foreground font-medium"
                      : "text-muted-foreground hover:bg-accent/55 hover:text-foreground",
                  )}
                >
                  <Icon className="size-4 shrink-0 opacity-80" />
                  <span>{item.label}</span>
                </button>
              );
            })}
          </nav>

          <div className="text-muted-foreground/60 px-3 pt-2 text-[11.5px]">
            Orbit v0.2
          </div>
        </aside>

        {/* Right Scrollable Settings Pane */}
        <div className="selectable-text flex-1 overflow-y-auto px-10 py-8">
          <div className="mx-auto w-full max-w-[660px] pb-8">
            {section === "general" && <GeneralSection />}
            {section === "hosts" && <HostsAndRelaySection />}
            {section === "providers" && <ProvidersAndQuotasSection />}
            {section === "worktrees" && <WorktreesAndReviewSection />}
            {section === "editor_terminal" && <EditorAndTerminalSection />}
            {section === "keybindings" && <KeybindingsSection />}
          </div>
        </div>
      </div>
    </div>
  );
}
