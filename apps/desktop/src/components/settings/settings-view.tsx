import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import {
  BarChart3,
  Bell,
  Blocks,
  Check,
  ChevronDown,
  Cloud,
  Cpu,
  Database,
  FileText,
  Keyboard,
  MessageSquare,
  Mic,
  Palette,
  Plug,
  RotateCcw,
  Search,
  Server,
  Settings2,
  ShieldCheck,
  Sparkles,
  X,
} from "lucide-react";
import { AppearanceSection } from "./appearance-section";
import { CloudSyncSection } from "./cloud-sync-section";
import { DataDiagnosticsSection } from "./data-diagnostics-section";
import { GeneralSection } from "./general-section";
import { HostAgentsSection } from "./host-agents-section";
import { HostGuardianSection } from "./host-guardian-section";
import { HostImChannelsSection } from "./host-im-channels-section";
import { HostPersonalRouterSection } from "./host-personal-router-section";
import { HostUsageSection } from "./host-usage-section";
import { HostVoiceSection } from "./host-voice-section";
import { HostsFleetSection } from "./hosts-fleet-section";
import { InstructionsMemorySection } from "./instructions-memory-section";
import { McpToolsSection } from "./mcp-tools-section";
import { NotificationsSection } from "./notifications-section";
import {
  LEGACY_SETTING_ANCHORS,
  SETTINGS_CORE_SECTIONS,
  SETTINGS_NAV_GROUPS,
  resolveActiveSettingsLocation,
  resolveSettingsSectionTarget,
  searchSettingsEntries,
  type SettingsCoreSectionId,
  type SettingsSearchResult,
} from "./settings-nav";
import { useModifiedSettings } from "./settings-preferences";
import {
  HostStatusIndicator,
  ModifiedSettingsContext,
  SettingPageHeader,
  SettingsDrawerPortalContext,
  settingsHostTransport,
} from "./settings-primitives";
import { ShortcutsSection } from "./shortcuts-section";
import { SkillsPluginsSection } from "./skills-plugins-section";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { ShortcutKbd } from "@/components/ui/kbd";
import { ListRow } from "@/components/ui/list-row";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { PanelErrorBoundary } from "@/components/ui/panel-error-boundary";
import { hasOpenSettingsBlockingOverlay } from "@/lib/file-viewer-shortcuts";
import { translate, useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import { isMacDesktop, isWebRuntime, type HostCardView } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

const icons = {
  general: Settings2,
  appearance: Palette,
  shortcuts: Keyboard,
  notifications: Bell,
  voice: Mic,
  cloud_sync: Cloud,
  hosts: Server,
  agents: Cpu,
  personal_router: Sparkles,
  safety: ShieldCheck,
  usage: BarChart3,
  instructions_memory: FileText,
  mcp_tools: Plug,
  skills_plugins: Blocks,
  im_channels: MessageSquare,
  data_diagnostics: Database,
} satisfies Record<SettingsCoreSectionId, typeof Settings2>;

const widePages = new Set<SettingsCoreSectionId>([
  "hosts",
  "usage",
  "mcp_tools",
  "skills_plugins",
  "im_channels",
  "data_diagnostics",
]);
const scopedPages = new Set<SettingsCoreSectionId>([
  "agents",
  "safety",
  "usage",
  "voice",
  "instructions_memory",
  "mcp_tools",
  "skills_plugins",
  "im_channels",
  "data_diagnostics",
]);

function ScopeBar({
  hosts,
  activeHost,
  onChange,
}: {
  hosts: HostCardView[];
  activeHost?: HostCardView;
  onChange: (id: string) => void;
}) {
  const { t } = useI18n();
  return (
    <div data-slot="settings-scope-bar" className="flex min-w-0 items-center">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            size="sm"
            variant="ghost"
            aria-label={t("settings.switchHost")}
            className="max-w-full text-[13px]"
          >
            <HostStatusIndicator state={activeHost?.connection_state ?? "disconnected"} />
            <span className="truncate">{activeHost?.display_name ?? t("settings.switchHost")}</span>
            {activeHost && (
              <span className="text-ink-muted text-[11px]">· {settingsHostTransport(activeHost)}</span>
            )}
            <ChevronDown className="size-[13px]" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="settings-scope-menu min-w-[220px]">
          {hosts.map((host) => (
            <DropdownMenuItem key={host.host_id} onSelect={() => onChange(host.host_id)}>
              <HostStatusIndicator state={host.connection_state} />
              <span className="flex-1 truncate">{host.display_name}</span>
              <span className="text-ink-muted text-[11px]">{settingsHostTransport(host)}</span>
              {activeHost?.host_id === host.host_id && <Check className="size-[13px]" />}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

// An RPC-backed section may mount its anchor after the page itself. Observe only
// the current canvas, and cancel when navigation or a later jump supersedes it.
function revealSetting(canvas: HTMLElement, id: string) {
  let frame = 0;
  let pulseTimer = 0;
  let highlighted: HTMLElement | null = null;
  const selector = `[data-setting-id="${CSS.escape(id)}"]`;
  const observer = new MutationObserver(() => findAnchor());
  const findAnchor = (allowFallback = false) => {
    const target =
      canvas.querySelector<HTMLElement>(selector) ??
      (allowFallback ? canvas.querySelector<HTMLElement>("[data-setting-fallback]") : null);
    if (!target) return false;
    observer.disconnect();
    let parent: HTMLElement | null = target;
    while (parent && parent !== canvas) {
      if (parent instanceof HTMLDetailsElement) parent.open = true;
      if (parent.matches('[data-slot="collapsible"]')) {
        parent
          .querySelector<HTMLElement>(':scope > [data-slot="collapsible-trigger"][aria-expanded="false"]')
          ?.click();
      }
      parent = parent.parentElement;
    }
    target.querySelector<HTMLElement>('[data-slot="collapsible-trigger"][aria-expanded="false"]')?.click();
    frame = window.requestAnimationFrame(() => {
      if (!target.isConnected) return;
      target.scrollIntoView({
        behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth",
        block: "center",
      });
      highlighted = target;
      target.dataset.settingsHighlight = "true";
      pulseTimer = window.setTimeout(() => delete target.dataset.settingsHighlight, 1200);
    });
    return true;
  };
  if (!findAnchor()) observer.observe(canvas, { childList: true, subtree: true });
  const fallbackTimer = window.setTimeout(() => {
    if (!highlighted) findAnchor(true);
  }, 2000);
  return () => {
    observer.disconnect();
    window.cancelAnimationFrame(frame);
    window.clearTimeout(fallbackTimer);
    window.clearTimeout(pulseTimer);
    if (highlighted) delete highlighted.dataset.settingsHighlight;
  };
}

export function SettingsView({ closing = false }: { closing?: boolean }) {
  const { locale, t } = useI18n();
  const section = useOrbitSelector((s) => s.settingsSection);
  const hosts = useOrbitSelector((s) => s.fleet.hosts);
  const workspaceHostId = useOrbitSelector((s) => s.selectedHostId);
  const instructionsTarget = useOrbitSelector((s) => s.instructionsTarget);
  const preferences = useModifiedSettings();
  const { coreSection, subTab, hostId: linkedHostId } = resolveActiveSettingsLocation(section);
  const orderedHosts = useMemo(
    () => [...hosts].sort((a, b) => Number(isLocalHostCard(b)) - Number(isLocalHostCard(a))),
    [hosts],
  );
  const requestedHost =
    linkedHostId ?? (coreSection === "instructions_memory" ? instructionsTarget?.hostId : null);
  const [pickedHostId, setPickedHostId] = useState<string | null>(workspaceHostId);
  useEffect(() => {
    if (requestedHost) setPickedHostId(requestedHost);
  }, [requestedHost]);
  // Never substitute another machine for an explicit host deep link.
  const activeHost =
    orderedHosts.find((h) => h.host_id === (requestedHost ?? pickedHostId)) ??
    (requestedHost ? undefined : orderedHosts[0]);
  const hostId = activeHost?.host_id ?? null;
  const pageLabel = t(SETTINGS_CORE_SECTIONS.find((item) => item.id === coreSection)!.labelKey);
  const scoped = scopedPages.has(coreSection);
  const splitEditor = coreSection === "instructions_memory" && subTab !== "rules" && subTab !== "memory";

  const closeBtn = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const canvas = useRef<HTMLDivElement>(null);
  const [dialogHost, setDialogHost] = useState<HTMLDivElement | null>(null);
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [jump, setJump] = useState<{ section: typeof section; id: string } | null>(null);
  const searching = query.trim().length > 0;
  const results = useMemo(
    () =>
      searchSettingsEntries(query, {
        t: (key) => translate(locale, key),
        hasHost: Boolean(hostId),
        isDesktop: !isWebRuntime(),
        modifiedIds: preferences.modifiedIds,
      }),
    [query, hostId, preferences.modifiedIds, locale],
  );
  const selected = Math.min(selectedIndex, results.length - 1);
  const updateQuery = (value: string) => {
    setQuery(value);
    setSelectedIndex(0);
  };
  const navigate = (target: string, id = hostId) => {
    const next = resolveSettingsSectionTarget(target, id, section);
    setJump(null);
    orbitActions.setSettingsSection(next);
    return next;
  };
  const selectResult = (result: SettingsSearchResult) => {
    const next = navigate(result.target);
    if (result.settingId) setJump({ section: next, id: result.settingId });
    updateQuery("");
    canvas.current?.focus({ preventScroll: true });
  };

  useLayoutEffect(() => {
    closeBtn.current?.focus({ preventScroll: true });
  }, []);
  useLayoutEffect(() => {
    if (canvas.current) canvas.current.scrollTop = 0;
  }, [coreSection, subTab, hostId]);
  useEffect(() => {
    const id = jump?.section === section ? jump.id : subTab && LEGACY_SETTING_ANCHORS[subTab];
    if (id && canvas.current) return revealSetting(canvas.current, id);
  }, [section, subTab, jump, hostId]);
  useEffect(() => {
    if (closing) return;
    const handleKey = (event: globalThis.KeyboardEvent) => {
      const find =
        (isMacDesktop() ? event.metaKey : event.ctrlKey) &&
        !event.shiftKey &&
        !event.altKey &&
        event.key.toLowerCase() === "f";
      if (!find && event.key !== "Escape") return;
      if (
        event.isComposing ||
        event.defaultPrevented ||
        orbitActions.getSnapshot().commandPaletteOpen ||
        document.querySelector('[data-orbit-shortcut-recorder="true"]') ||
        hasOpenSettingsBlockingOverlay()
      )
        return;
      if (find) {
        event.preventDefault();
        search.current?.focus();
        search.current?.select();
      } else if (event.key === "Escape" && (query || event.target === search.current)) {
        event.preventDefault();
        event.stopPropagation();
        if (query) updateQuery("");
        else orbitActions.closeSettings();
      }
    };
    document.addEventListener("keydown", handleKey, true);
    return () => document.removeEventListener("keydown", handleKey, true);
  }, [closing, query]);

  const searchKeys = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing || !results.length) return;
    if (event.key === "Enter" && selected >= 0) {
      event.preventDefault();
      selectResult(results[selected]);
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const index = (selected + (event.key === "ArrowDown" ? 1 : -1) + results.length) % results.length;
      setSelectedIndex(index);
      document.getElementById(`settings-result-${index}`)?.scrollIntoView({ block: "nearest" });
    }
  };

  const renderPage = () => {
    switch (coreSection) {
      case "general":
        return <GeneralSection />;
      case "appearance":
        return <AppearanceSection />;
      case "shortcuts":
        return <ShortcutsSection jumpTo={jump?.section === section ? jump.id : undefined} />;
      case "notifications":
        return <NotificationsSection />;
      case "cloud_sync":
        return <CloudSyncSection />;
      case "hosts":
        return (
          <HostsFleetSection
            hosts={orderedHosts}
            hostId={hostId}
            view={subTab === "connection" || subTab === "add_host" ? subTab : "overview"}
            onSelectHost={(id) => navigate(`host:${id}:connection`)}
            onOverview={() => navigate("hosts")}
          />
        );
      case "personal_router":
        return <HostPersonalRouterSection />;
      case "agents":
        return <HostAgentsSection hostId={hostId} />;
      case "safety":
        return <HostGuardianSection hostId={hostId} />;
      case "data_diagnostics":
        return <DataDiagnosticsSection hostId={hostId} />;
      case "instructions_memory":
        return (
          <InstructionsMemorySection
            hostId={hostId}
            tab={subTab === "memory" || subTab === "rules" ? subTab : "instructions"}
            onTabChange={(tab) => navigate(tab)}
          />
        );
    }
    if (!hostId)
      return (
        <SettingPageHeader
          title={pageLabel}
          description={t("settings.hostNeeded")}
          action={
            <Button variant="outline" onClick={() => navigate("add_host")}>
              {t("settings.connectHost")}
            </Button>
          }
        />
      );
    switch (coreSection) {
      case "voice":
        return <HostVoiceSection hostId={hostId} />;
      case "usage":
        return <HostUsageSection hostId={hostId} />;
      case "mcp_tools":
        return <McpToolsSection hostId={hostId} />;
      case "skills_plugins":
        return <SkillsPluginsSection hostId={hostId} />;
      case "im_channels":
        return <HostImChannelsSection hostId={hostId} />;
    }
  };

  const showScopeBar =
    scoped &&
    orderedHosts.length > 0 &&
    (orderedHosts.length > 1 || !isLocalHostCard(orderedHosts[0]));

  return (
    <ModifiedSettingsContext.Provider value={preferences}>
      <SettingsDrawerPortalContext.Provider value={dialogHost}>
        <div
          data-slot="settings-backdrop"
          data-closing={closing || undefined}
          inert={closing}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget && !hasOpenSettingsBlockingOverlay()) {
              orbitActions.closeSettings();
            }
          }}
          className="settings-backdrop fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-5 select-none dark:bg-black/55"
        >
          <div
            ref={setDialogHost}
            role="dialog"
            aria-modal="true"
            aria-label={t("common.settings")}
            data-slot="settings-view"
            data-closing={closing || undefined}
            className="settings-view settings-dialog border-border-default bg-surface-base text-ink-primary floating-surface relative flex h-[min(820px,88vh)] w-[min(1180px,92vw)] overflow-hidden rounded-[16px] border text-[13px]"
          >
            <aside className="border-border-subtle flex w-[240px] max-w-[38vw] shrink-0 flex-col border-r px-[12px] pt-[14px]">
              <div className="text-ink-primary px-2 pb-2.5 text-[15px] leading-[20px] font-semibold tracking-[-0.015em]">
                {t("common.settings")}
              </div>
              <div className="relative mb-2.5 flex items-center">
                <Search
                  aria-hidden
                  className="text-ink-muted pointer-events-none absolute left-2.5 size-[14px]"
                />
                <Input
                  ref={search}
                  aria-label={t("settings.searchAria")}
                  placeholder={t("settings.searchPlaceholder")}
                  value={query}
                  onChange={(event) => updateQuery(event.target.value)}
                  onKeyDown={searchKeys}
                  aria-controls={searching ? "settings-results" : undefined}
                  aria-activedescendant={searching && selected >= 0 ? `settings-result-${selected}` : undefined}
                  className={cn(
                    "h-[30px] pl-[28px] text-[13px] shadow-none",
                    query ? "pr-[28px]" : "pr-[48px]",
                  )}
                />
                <div className="absolute right-[6px] flex items-center">
                  {query ? (
                    <IconButton
                      size="xs"
                      tooltip={t("settings.clearSearch")}
                      onClick={() => {
                        updateQuery("");
                        search.current?.focus();
                      }}
                    >
                      <X />
                    </IconButton>
                  ) : (
                    <ShortcutKbd
                      keys={`${modSymbol()}F`}
                      size="xs"
                      variant="subtle"
                      className="pointer-events-none"
                    />
                  )}
                </div>
              </div>
              <nav
                aria-label={t("common.settings")}
                className="orbit-native-scroll min-h-0 flex-1 overflow-y-auto overscroll-contain pb-3"
              >
                {searching ? (
                  <div id="settings-results">
                    <p className="text-ink-muted px-2 pb-1 text-[11px] font-semibold">
                      {t("settings.searchResultsLabel")}
                    </p>
                    {results.map((result, index) => {
                      const label = t(result.labelKey);
                      return (
                        <div key={result.id} className="group/result flex items-center">
                          <ListRow
                            id={`settings-result-${index}`}
                            selected={index === selected}
                            subtitle={result.parentLabelKey && t(result.parentLabelKey)}
                            onMouseEnter={() => setSelectedIndex(index)}
                            onClick={() => selectResult(result)}
                          >
                            {label}
                          </ListRow>
                          {result.settingId && preferences.modifiedIds.has(result.settingId) && (
                            <IconButton
                              size="xs"
                              aria-label={t("settings.resetNamed", { name: label })}
                              onClick={() => preferences.reset(result.settingId!)}
                            >
                              <RotateCcw />
                            </IconButton>
                          )}
                        </div>
                      );
                    })}
                    {!results.length && (
                      <p className="text-ink-muted px-2 py-3 text-[13px]">{t("settings.noMatches")}</p>
                    )}
                  </div>
                ) : (
                  SETTINGS_NAV_GROUPS.map((group) => (
                    <section key={group.id} className="settings-rail-group">
                      <h2 className="text-ink-muted px-2 pb-1 text-[11px] leading-[16px] font-semibold tracking-[0.05em] uppercase">
                        {t(group.labelKey)}
                      </h2>
                      {SETTINGS_CORE_SECTIONS.filter((item) => item.group === group.id).map((item) => {
                        const Icon = icons[item.id];
                        return (
                          <ListRow
                            key={item.id}
                            data-settings-section={item.id}
                            selected={coreSection === item.id}
                            aria-current={coreSection === item.id ? "page" : undefined}
                            leading={<Icon className="size-[15px] stroke-[1.7]" />}
                            onClick={() => navigate(item.id)}
                          >
                            {t(item.labelKey)}
                          </ListRow>
                        );
                      })}
                    </section>
                  ))
                )}
              </nav>
            </aside>
            <main className="flex min-h-0 min-w-0 flex-1 flex-col">
              <div
                className={cn(
                  "flex h-[44px] shrink-0 items-center justify-between gap-3 px-5",
                  showScopeBar && "border-border-subtle border-b",
                )}
              >
                {showScopeBar ? (
                  <ScopeBar
                    hosts={orderedHosts}
                    activeHost={activeHost}
                    onChange={(id) => {
                      setPickedHostId(id);
                      if (instructionsTarget) orbitActions.setInstructionsTarget(null);
                      navigate(subTab ?? coreSection, id);
                    }}
                  />
                ) : (
                  <div />
                )}
                <IconButton
                  ref={closeBtn}
                  size="md"
                  tooltip={t("settings.closeSettings")}
                  shortcut="Esc"
                  onClick={() => orbitActions.closeSettings()}
                >
                  <X className="size-[15px]" />
                </IconButton>
              </div>
              <div
                ref={canvas}
                data-settings-canvas=""
                tabIndex={-1}
                className={cn(
                  "orbit-native-scroll min-h-0 flex-1 px-[clamp(20px,3vw,40px)] pb-[32px] outline-none",
                  showScopeBar ? "pt-[20px]" : "pt-[4px]",
                  splitEditor ? "flex flex-col overflow-hidden" : "overflow-y-auto overscroll-contain",
                )}
              >
                <div
                  className={cn(
                    "mx-auto w-full",
                    splitEditor
                      ? "flex min-h-0 flex-1 flex-col"
                      : widePages.has(coreSection)
                        ? "max-w-[880px]"
                        : "max-w-[680px]",
                  )}
                >
                  <PanelErrorBoundary
                    key={`${coreSection}:${scoped || coreSection === "hosts" ? hostId : ""}`}
                    panelName={pageLabel}
                  >
                    {renderPage()}
                  </PanelErrorBoundary>
                </div>
              </div>
            </main>
          </div>
        </div>
      </SettingsDrawerPortalContext.Provider>
    </ModifiedSettingsContext.Provider>
  );
}
