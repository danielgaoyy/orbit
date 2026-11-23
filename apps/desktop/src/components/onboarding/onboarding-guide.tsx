import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  FolderPlus,
  Loader2,
  MessageSquarePlus,
  Plus,
  QrCode,
  RefreshCw,
  Server,
  Terminal,
} from "lucide-react";
import { PairingQrCard } from "@/components/modals/pairing-qr-dialog";
import {
  HostStatusIndicator,
  hostConnectionLabel,
} from "@/components/settings/settings-primitives";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  Tabs,
  TabsList,
  TabsTrigger,
} from "@/components/ui/dialog-tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useI18n } from "@/lib/i18n";
import { type ProviderReadinessInfo, orbitIpc } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type StepIndex = 0 | 1 | 2;

export function OnboardingGuide() {
  const open = useOrbitSelector((s) => s.onboardingOpen);

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          orbitActions.closeOnboardingGuide(true);
        }
      }}
    >
      <DialogContent className="max-w-[548px] gap-0 overflow-hidden p-0">
        {open && <OnboardingGuideBody />}
      </DialogContent>
    </Dialog>
  );
}

function OnboardingGuideBody() {
  const { t } = useI18n();
  const fleet = useOrbitSelector((s) => s.fleet);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);

  const [step, setStep] = useState<StepIndex>(0);
  const [reconnecting, setReconnecting] = useState(false);

  // Step 2: Real local provider readiness probe
  const [providers, setProviders] = useState<ProviderReadinessInfo[]>([]);
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [copiedCmdKey, setCopiedCmdKey] = useState<string | null>(null);

  // Step 3: Optional expandable mobile pairing QR
  const [showMobileQr, setShowMobileQr] = useState(false);

  const orderedHosts = useMemo(() => {
    const local = fleet.hosts.filter((h) => isLocalHostCard(h));
    const remote = fleet.hosts.filter((h) => !isLocalHostCard(h));
    return [...local, ...remote];
  }, [fleet.hosts]);

  const onlineHostCount = useMemo(
    () =>
      orderedHosts.filter(
        (h) =>
          h.connection_state === "online_direct" || h.connection_state === "online_relay",
      ).length,
    [orderedHosts],
  );

  const activePairingHost = useMemo(() => {
    return (
      orderedHosts.find((h) => h.host_id === selectedHostId) ??
      orderedHosts.find((h) => isLocalHostCard(h)) ??
      orderedHosts[0] ??
      null
    );
  }, [orderedHosts, selectedHostId]);

  const runProbe = useCallback(async () => {
    setProbing(true);
    setProbeError(null);
    try {
      const items = await orbitIpc.probeLocalProviderReadiness();
      setProviders(items);
    } catch (err) {
      setProbeError(err instanceof Error ? err.message : String(err));
    } finally {
      setProbing(false);
    }
  }, []);

  useEffect(() => {
    void runProbe();
  }, [runProbe]);

  const providerSummary = useMemo(() => {
    let installed = 0;
    let ready = 0;
    for (const p of providers) {
      if (p.installed) {
        installed += 1;
        if (p.auth_status === "authenticated") {
          ready += 1;
        }
      }
    }
    return { installed, ready };
  }, [providers]);

  const copyCommand = async (key: string, cmd: string) => {
    if (!cmd.trim()) return;
    try {
      await navigator.clipboard.writeText(cmd);
      setCopiedCmdKey(key);
      window.setTimeout(
        () => setCopiedCmdKey((prev) => (prev === key ? null : prev)),
        1500,
      );
    } catch {
      // Ignore clipboard errors
    }
  };

  const handleReconnectAll = async () => {
    setReconnecting(true);
    try {
      await orbitIpc.connectAllHosts();
      await orbitActions.reloadFleetAndHosts(false);
    } finally {
      window.setTimeout(() => setReconnecting(false), 350);
    }
  };

  const handleAddRemoteHost = () => {
    orbitActions.closeOnboardingGuide(true);
    orbitActions.openSettings("add_host");
  };

  const handleChooseWorkspace = () => {
    orbitActions.closeOnboardingGuide(true);
    void orbitActions.promptAddProject();
  };

  const handleStartConversation = () => {
    orbitActions.closeOnboardingGuide(true);
    orbitActions.startNewThreadDraft();
  };

  const steps: readonly { idx: StepIndex; label: string }[] = [
    { idx: 0, label: t("onboarding.stepHost") },
    { idx: 1, label: t("onboarding.stepClis") },
    { idx: 2, label: t("onboarding.stepStart") },
  ];

  return (
    <div className="flex flex-col">
      {/* Header */}
      <div className="border-border border-b px-6 pt-5 pb-4">
        <DialogHeader className="gap-1 pr-6">
          <DialogTitle className="text-[16px] font-semibold tracking-[-0.015em]">
            {t("onboarding.title")}
          </DialogTitle>
          <DialogDescription className="text-[12.5px]">
            {t("onboarding.subtitle")}
          </DialogDescription>
        </DialogHeader>

        {/* Official shadcn Tabs 3-Step Switcher */}
        <Tabs
          value={String(step)}
          onValueChange={(next) => {
            if (typeof next === "string") {
              setStep(Number(next) as StepIndex);
            }
          }}
          className="mt-4"
        >
          <TabsList
            aria-label={t("onboarding.title")}
            className="grid h-auto w-full grid-cols-3 gap-1 rounded-[10px] p-1"
          >
            {steps.map((s) => {
              const active = step === s.idx;
              return (
                <TabsTrigger
                  key={s.idx}
                  value={String(s.idx)}
                  className="h-[28px] rounded-[7px] px-2.5 text-[12px]"
                >
                  <span
                    className={cn(
                      "inline-flex h-[16px] w-[16px] items-center justify-center rounded-full font-mono text-[10.5px]",
                      active
                        ? "bg-action-primary-bg text-action-primary-fg"
                        : "bg-border-default/60 text-ink-muted",
                    )}
                  >
                    {s.idx + 1}
                  </span>
                  <span>{s.label}</span>
                </TabsTrigger>
              );
            })}
          </TabsList>
        </Tabs>
      </div>

      {/* Step Content */}
      <div className="px-6 py-5">
        {step === 0 && (
          <div className="space-y-4">
            <div className="flex items-start justify-between gap-3">
              <div className="space-y-1">
                <h3 className="text-foreground text-[14px] font-semibold">
                  {t("onboarding.step1Title")}
                </h3>
                <p className="text-muted-foreground text-[12.5px] leading-relaxed">
                  {t("onboarding.step1Desc")}
                </p>
              </div>
              {orderedHosts.length > 0 && (
                <span className="bg-muted text-muted-foreground shrink-0 rounded-md px-2 py-0.5 text-[11px] font-medium tabular-nums">
                  {t("onboarding.hostsSummary", {
                    online: onlineHostCount,
                    total: orderedHosts.length,
                  })}
                </span>
              )}
            </div>

            <div className="border-border bg-background/60 divide-border divide-y overflow-hidden rounded-[12px] border">
              {orderedHosts.length === 0 ? (
                <div className="text-muted-foreground flex items-center gap-2.5 px-4 py-4 text-[12.5px]">
                  <Loader2 className="h-[14px] w-[14px] animate-spin opacity-75" />
                  <span>{t("onboarding.noHostsYet")}</span>
                </div>
              ) : (
                orderedHosts.map((host) => {
                  const isLocal = isLocalHostCard(host);
                  return (
                    <div
                      key={host.host_id}
                      className="flex items-center justify-between gap-3 px-4 py-3"
                    >
                      <div className="flex min-w-0 items-center gap-2.5">
                        <Server className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
                        <div className="min-w-0">
                          <div className="flex items-center gap-1.5">
                            <span className="text-foreground truncate text-[13px] font-medium">
                              {host.display_name}
                            </span>
                            {isLocal && (
                              <span className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-[10.5px]">
                                {t("common.local")}
                              </span>
                            )}
                          </div>
                          <div className="text-muted-foreground mt-0.5 flex items-center gap-1.5 text-[11.5px]">
                            <HostStatusIndicator state={host.connection_state} />
                            <span>{hostConnectionLabel(host.connection_state)}</span>
                            {host.daemon_version && (
                              <span className="font-mono opacity-75">
                                · v{host.daemon_version.replace(/^v/i, "")}
                              </span>
                            )}
                          </div>
                        </div>
                      </div>
                    </div>
                  );
                })
              )}
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                disabled={reconnecting}
                onClick={() => void handleReconnectAll()}
                className="border-border hover:bg-accent inline-flex h-[32px] cursor-pointer items-center gap-1.5 rounded-lg border px-3 text-[12.5px] font-medium transition-colors disabled:opacity-50"
              >
                <RefreshCw className={cn("h-[13px] w-[13px]", reconnecting && "animate-spin")} />
                <span>{t("banner.reconnect")}</span>
              </button>
              <button
                type="button"
                onClick={handleAddRemoteHost}
                className="border-border hover:bg-accent inline-flex h-[32px] cursor-pointer items-center gap-1.5 rounded-lg border px-3 text-[12.5px] font-medium transition-colors"
              >
                <Plus className="h-[13px] w-[13px]" />
                <span>{t("onboarding.addRemoteHost")}</span>
              </button>
            </div>
          </div>
        )}

        {step === 1 && (
          <div className="space-y-3.5">
            <div className="flex items-start justify-between gap-3">
              <div className="space-y-1">
                <h3 className="text-foreground text-[14px] font-semibold">
                  {t("onboarding.step2Title")}
                </h3>
                <p className="text-muted-foreground text-[12.5px] leading-relaxed">
                  {t("onboarding.step2Desc")}
                </p>
              </div>
              <button
                type="button"
                disabled={probing}
                onClick={() => void runProbe()}
                className="border-border hover:bg-accent inline-flex h-[28px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
              >
                <RefreshCw className={cn("h-[12.5px] w-[12.5px]", probing && "animate-spin")} />
                <span>{t("common.refresh")}</span>
              </button>
            </div>

            {providers.length > 0 && !probeError && (
              <div className="text-muted-foreground text-[11.5px] tabular-nums">
                {t("onboarding.cliSummary", {
                  ready: providerSummary.ready,
                  installed: providerSummary.installed,
                })}
              </div>
            )}

            <div className="border-border bg-background/60 overflow-hidden rounded-[12px] border">
              {probing && providers.length === 0 ? (
                <div className="text-muted-foreground flex items-center gap-2.5 px-4 py-5 text-[12.5px]">
                  <Loader2 className="h-[14px] w-[14px] animate-spin" />
                  <span>{t("onboarding.probingClis")}</span>
                </div>
              ) : probeError ? (
                <div className="text-muted-foreground flex items-center justify-between gap-3 px-4 py-4 text-[12.5px]">
                  <span className="inline-flex items-center gap-2">
                    <AlertCircle className="h-[14px] w-[14px] text-rose-500" />
                    <span>{t("onboarding.probeFailed")}</span>
                  </span>
                  <button
                    type="button"
                    onClick={() => void runProbe()}
                    className="border-border hover:bg-accent cursor-pointer rounded-md border px-2 py-1 text-[11.5px] font-medium"
                  >
                    {t("common.retry")}
                  </button>
                </div>
              ) : (
                <ScrollArea viewportClassName="max-h-[268px]">
                  <div className="divide-border divide-y">
                    {providers.map((item) => {
                      const isReady = item.installed && item.auth_status === "authenticated";
                      const needsLogin = item.installed && item.auth_status !== "authenticated";
                      const actionCmd = !item.installed
                        ? item.install_hint
                        : needsLogin
                          ? item.login_hint
                          : "";
                      const cmdKey = `${item.kind}:${item.installed ? "login" : "install"}`;
                      const isCopied = copiedCmdKey === cmdKey;

                      return (
                        <div
                          key={item.kind}
                          className="flex items-center justify-between gap-3 px-4 py-2.5"
                        >
                          <div className="min-w-0 flex-1 space-y-1">
                            <div className="flex items-center gap-2">
                              <Terminal className="text-muted-foreground h-[13px] w-[13px] shrink-0 opacity-75" />
                              <span className="text-foreground truncate text-[13px] font-medium">
                                {item.display_name}
                              </span>
                              <span
                                className={cn(
                                  "shrink-0 rounded-full px-2 py-0.5 text-[10.5px] font-medium",
                                  isReady
                                    ? "bg-emerald-500/12 text-emerald-700 dark:text-emerald-400"
                                    : needsLogin
                                      ? "bg-amber-500/12 text-amber-700 dark:text-amber-400"
                                      : "bg-muted text-muted-foreground",
                                )}
                              >
                                {isReady
                                  ? t("onboarding.statusReady")
                                  : needsLogin
                                    ? t("onboarding.statusLoginRequired")
                                    : t("onboarding.statusNotInstalled")}
                              </span>
                            </div>

                            {actionCmd && (
                              <div className="text-muted-foreground truncate font-mono text-[11px] pl-[21px]">
                                {actionCmd}
                              </div>
                            )}
                          </div>

                          {actionCmd ? (
                            <button
                              type="button"
                              title={
                                !item.installed
                                  ? t("onboarding.copyInstallCmd")
                                  : t("onboarding.copyLoginCmd")
                              }
                              onClick={() => void copyCommand(cmdKey, actionCmd)}
                              className="border-border hover:bg-accent inline-flex h-[26px] shrink-0 cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium transition-colors"
                            >
                              {isCopied ? (
                                <Check className="h-[11.5px] w-[11.5px] text-emerald-500" />
                              ) : (
                                <Copy className="text-muted-foreground h-[11.5px] w-[11.5px]" />
                              )}
                              <span>{isCopied ? t("common.copied") : t("common.copy")}</span>
                            </button>
                          ) : (
                            <Check className="h-[14px] w-[14px] shrink-0 text-emerald-600 dark:text-emerald-400" />
                          )}
                        </div>
                      );
                    })}
                  </div>
                </ScrollArea>
              )}
            </div>
          </div>
        )}

        {step === 2 && (
          <div className="space-y-4">
            <div className="space-y-1">
              <h3 className="text-foreground text-[14px] font-semibold">
                {t("onboarding.step3Title")}
              </h3>
              <p className="text-muted-foreground text-[12.5px] leading-relaxed">
                {t("onboarding.step3Desc")}
              </p>
            </div>

            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              <button
                type="button"
                onClick={handleChooseWorkspace}
                className="border-border bg-background/70 hover:bg-accent/60 group flex cursor-pointer flex-col items-start gap-1.5 rounded-[12px] border p-3.5 text-left transition-colors"
              >
                <div className="bg-muted text-foreground inline-flex h-[30px] w-[30px] items-center justify-center rounded-lg">
                  <FolderPlus className="h-[15px] w-[15px] stroke-[1.75]" />
                </div>
                <span className="text-foreground text-[13px] font-medium">
                  {t("onboarding.chooseFolder")}
                </span>
                <span className="text-muted-foreground text-[11.5px] leading-snug">
                  {t("onboarding.chooseFolderDesc")}
                </span>
              </button>

              <button
                type="button"
                onClick={handleStartConversation}
                className="border-border bg-background/70 hover:bg-accent/60 group flex cursor-pointer flex-col items-start gap-1.5 rounded-[12px] border p-3.5 text-left transition-colors"
              >
                <div className="bg-muted text-foreground inline-flex h-[30px] w-[30px] items-center justify-center rounded-lg">
                  <MessageSquarePlus className="h-[15px] w-[15px] stroke-[1.75]" />
                </div>
                <span className="text-foreground text-[13px] font-medium">
                  {t("onboarding.startConversation")}
                </span>
                <span className="text-muted-foreground text-[11.5px] leading-snug">
                  {t("onboarding.startConversationDesc")}
                </span>
              </button>
            </div>

            {/* Optional Expandable Mobile Pairing QR */}
            {activePairingHost && (
              <div className="border-border bg-background/60 overflow-hidden rounded-[12px] border">
                <button
                  type="button"
                  onClick={() => setShowMobileQr((v) => !v)}
                  className="hover:bg-accent/40 flex w-full cursor-pointer items-center justify-between gap-3 px-4 py-3 text-left transition-colors"
                >
                  <div className="flex min-w-0 items-center gap-2.5">
                    <QrCode className="text-muted-foreground h-[15px] w-[15px] shrink-0" />
                    <div className="min-w-0">
                      <div className="text-foreground text-[12.5px] font-medium">
                        {t("onboarding.pairMobileToggle")}
                      </div>
                      <div className="text-muted-foreground text-[11.5px]">
                        {t("onboarding.pairMobileDesc")}
                      </div>
                    </div>
                  </div>
                  {showMobileQr ? (
                    <ChevronDown className="text-muted-foreground h-[15px] w-[15px] shrink-0" />
                  ) : (
                    <ChevronRight className="text-muted-foreground h-[15px] w-[15px] shrink-0" />
                  )}
                </button>

                {showMobileQr && (
                  <div className="border-border border-t px-4 py-4">
                    <PairingQrCard
                      hostId={activePairingHost.host_id}
                      hostName={activePairingHost.display_name}
                      compact
                    />
                  </div>
                )}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Footer Navigation */}
      <div className="border-border flex items-center justify-between border-t px-6 py-3.5">
        <div>
          {step === 0 ? (
            <button
              type="button"
              onClick={() => orbitActions.closeOnboardingGuide(true)}
              className="text-muted-foreground hover:text-foreground cursor-pointer rounded-md px-2 py-1 text-[12.5px] font-medium transition-colors"
            >
              {t("onboarding.skip")}
            </button>
          ) : (
            <button
              type="button"
              onClick={() => setStep((prev) => (prev > 0 ? ((prev - 1) as StepIndex) : 0))}
              className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center rounded-lg border px-3 text-[12.5px] font-medium transition-colors"
            >
              {t("onboarding.back")}
            </button>
          )}
        </div>

        <div className="flex items-center gap-2">
          {step < 2 ? (
            <button
              type="button"
              onClick={() => setStep((prev) => (prev < 2 ? ((prev + 1) as StepIndex) : 2))}
              className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity hover:opacity-90"
            >
              {t("onboarding.next")}
            </button>
          ) : (
            <button
              type="button"
              onClick={() => orbitActions.closeOnboardingGuide(true)}
              className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity hover:opacity-90"
            >
              {t("onboarding.done")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
