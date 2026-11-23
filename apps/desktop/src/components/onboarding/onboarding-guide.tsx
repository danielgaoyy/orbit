import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  FolderPlus,
  Laptop,
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useI18n } from "@/lib/i18n";
import { type LocalMachineInfo, type ProviderReadinessInfo, orbitIpc } from "@/lib/orbit-ipc";
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
  const [localMachine, setLocalMachine] = useState<LocalMachineInfo | null>(null);
  const [connectingLocal, setConnectingLocal] = useState(false);
  const [localConnectError, setLocalConnectError] = useState<string | null>(null);

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

  useEffect(() => {
    let active = true;
    void orbitIpc
      .getLocalMachineInfo()
      .then((info) => {
        if (active) setLocalMachine(info);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [orderedHosts.length]);

  const localAlreadyPaired = useMemo(
    () =>
      orderedHosts.some(
        (h) =>
          (localMachine?.host_id && h.host_id === localMachine.host_id) ||
          isLocalHostCard(h),
      ),
    [localMachine?.host_id, orderedHosts],
  );

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

  const handleConnectLocal = async () => {
    setLocalConnectError(null);
    setConnectingLocal(true);
    try {
      const card = await orbitIpc.pairLocalHost();
      await orbitActions.reloadFleetAndHosts(false);
      orbitActions.selectHost(card.host_id);
      orbitActions.showToast(`Connected ${card.display_name}`);
    } catch (err) {
      setLocalConnectError(err instanceof Error ? err.message : String(err));
    } finally {
      setConnectingLocal(false);
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
                <div className="space-y-3 px-4 py-3.5">
                  <div className="text-muted-foreground flex items-center gap-2.5 text-[12.5px]">
                    <Laptop className="h-[14px] w-[14px] shrink-0 opacity-75" />
                    <span>{t("onboarding.noHostsYet")}</span>
                  </div>
                  {localMachine && !localAlreadyPaired && (
                    <div className="border-border/80 bg-background/70 flex items-center justify-between gap-3 rounded-[10px] border px-3.5 py-2.5">
                      <div className="min-w-0">
                        <div className="text-foreground flex items-center gap-1.5 text-[13px] font-medium">
                          <span className="truncate">{localMachine.hostname}</span>
                          <Badge variant="secondary" className="px-1.5 py-0 text-[10.5px]">
                            {t("common.local")}
                          </Badge>
                        </div>
                        <div className="text-muted-foreground truncate text-[11.5px]">
                          {t("onboarding.connectLocalHint")}
                        </div>
                      </div>
                      <Button
                        type="button"
                        size="sm"
                        disabled={connectingLocal}
                        onClick={() => void handleConnectLocal()}
                        className="shrink-0"
                      >
                        {connectingLocal && (
                          <OrbitSpinner size={12.5} className="size-[12.5px]" />
                        )}
                        <span>
                          {connectingLocal
                            ? t("onboarding.connectingLocal")
                            : t("onboarding.connectThisMachine")}
                        </span>
                      </Button>
                    </div>
                  )}
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
                              <Badge variant="secondary" className="px-1.5 py-0 text-[10.5px]">
                                {t("common.local")}
                              </Badge>
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

            {localConnectError && (
              <div className="selectable-text text-signal-danger px-0.5 text-[12px]">
                {localConnectError}
              </div>
            )}

            <div className="flex flex-wrap items-center gap-2">
              {orderedHosts.length > 0 && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={reconnecting}
                  onClick={() => void handleReconnectAll()}
                  className="h-[32px] text-[12.5px]"
                >
                  <RefreshCw className={cn("size-[13px]", reconnecting && "animate-spin")} />
                  <span>{t("banner.reconnect")}</span>
                </Button>
              )}
              {orderedHosts.length > 0 && localMachine && !localAlreadyPaired && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={connectingLocal}
                  onClick={() => void handleConnectLocal()}
                  className="h-[32px] text-[12.5px]"
                >
                  {connectingLocal ? (
                    <OrbitSpinner size={13} className="size-[13px]" />
                  ) : (
                    <Laptop className="size-[13px]" />
                  )}
                  <span>
                    {connectingLocal
                      ? t("onboarding.connectingLocal")
                      : t("onboarding.connectThisMachine")}
                  </span>
                </Button>
              )}
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleAddRemoteHost}
                className="h-[32px] text-[12.5px]"
              >
                <Plus className="size-[13px]" />
                <span>{t("onboarding.addRemoteHost")}</span>
              </Button>
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
              <Button
                type="button"
                variant="outline"
                size="xs"
                disabled={probing}
                onClick={() => void runProbe()}
                className="h-[28px] shrink-0 gap-1.5 rounded-lg"
              >
                <RefreshCw className={cn("size-[12.5px]", probing && "animate-spin")} />
                <span>{t("common.refresh")}</span>
              </Button>
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
                  <OrbitSpinner size={14} />
                  <span>{t("onboarding.probingClis")}</span>
                </div>
              ) : probeError ? (
                <div className="text-muted-foreground flex items-center justify-between gap-3 px-4 py-4 text-[12.5px]">
                  <span className="inline-flex items-center gap-2">
                    <AlertCircle className="text-signal-danger h-[14px] w-[14px]" />
                    <span>{t("onboarding.probeFailed")}</span>
                  </span>
                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    onClick={() => void runProbe()}
                    className="text-[11.5px]"
                  >
                    {t("common.retry")}
                  </Button>
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
                              <Badge
                                variant={isReady ? "success" : needsLogin ? "warning" : "secondary"}
                                className="rounded-full text-[10.5px]"
                              >
                                {isReady
                                  ? t("onboarding.statusReady")
                                  : needsLogin
                                    ? t("onboarding.statusLoginRequired")
                                    : t("onboarding.statusNotInstalled")}
                              </Badge>
                            </div>

                            {actionCmd && (
                              <div className="text-muted-foreground truncate font-mono text-[11px] pl-[21px]">
                                {actionCmd}
                              </div>
                            )}
                          </div>

                          {actionCmd ? (
                            <Button
                              type="button"
                              variant="outline"
                              size="xs"
                              title={
                                !item.installed
                                  ? t("onboarding.copyInstallCmd")
                                  : t("onboarding.copyLoginCmd")
                              }
                              onClick={() => void copyCommand(cmdKey, actionCmd)}
                              className="shrink-0 gap-1 px-2 text-[11.5px]"
                            >
                              {isCopied ? (
                                <Check className="text-signal-success size-[11.5px]" />
                              ) : (
                                <Copy className="text-muted-foreground size-[11.5px]" />
                              )}
                              <span>{isCopied ? t("common.copied") : t("common.copy")}</span>
                            </Button>
                          ) : (
                            <Check className="text-signal-success h-[14px] w-[14px] shrink-0" />
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
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => orbitActions.closeOnboardingGuide(true)}
              className="px-2 text-[12.5px]"
            >
              {t("onboarding.skip")}
            </Button>
          ) : (
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setStep((prev) => (prev > 0 ? ((prev - 1) as StepIndex) : 0))}
              className="text-[12.5px]"
            >
              {t("onboarding.back")}
            </Button>
          )}
        </div>

        <div className="flex items-center gap-2">
          {step < 2 ? (
            <Button
              type="button"
              size="sm"
              onClick={() => setStep((prev) => (prev < 2 ? ((prev + 1) as StepIndex) : 2))}
              className="px-3.5 text-[12.5px]"
            >
              {t("onboarding.next")}
            </Button>
          ) : (
            <Button
              type="button"
              size="sm"
              onClick={() => orbitActions.closeOnboardingGuide(true)}
              className="px-3.5 text-[12.5px]"
            >
              {t("onboarding.done")}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
