import {
  type ComponentType,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  BarChart3,
  Bell,
  Check,
  ChevronsUpDown,
  Cpu,
  FileText,
  Palette,
  Plug,
  Plus,
  Search,
  Server,
  Settings2,
  X,
} from "lucide-react";
import { AddHostSection } from "@/components/settings/add-host-section";
import { AppearanceSection } from "@/components/settings/appearance-section";
import { ArchivedSection } from "@/components/settings/archived-section";
import { CloudSyncSection } from "@/components/settings/cloud-sync-section";
import { GeneralSection } from "@/components/settings/general-section";
import { HostAcpSection } from "@/components/settings/host-acp-section";
import { HostAgentsSection } from "@/components/settings/host-agents-section";
import { HostConnectionSection } from "@/components/settings/host-connection-section";
import { HostGuardianSection } from "@/components/settings/host-guardian-section";
import { HostHooksSection } from "@/components/settings/host-hooks-section";
import { HostImChannelsSection } from "@/components/settings/host-im-channels-section";
import { HostLspSection } from "@/components/settings/host-lsp-section";
import { HostMcpSection } from "@/components/settings/host-mcp-section";
import { HostPersonalRouterSection } from "@/components/settings/host-personal-router-section";
import { HostRulesSection } from "@/components/settings/host-rules-section";
import { HostSkillsSection } from "@/components/settings/host-skills-section";
import { HostUsageSection } from "@/components/settings/host-usage-section";
import { InstructionsSection } from "@/components/settings/instructions-section";
import { NotificationsSection } from "@/components/settings/notifications-section";
import {
  SETTINGS_CORE_SECTIONS,
  SETTINGS_NAV_GROUPS,
  type SettingsCoreSectionId,
  type SettingsSearchResult,
  type SettingsSubTabId,
  buildSettingsSectionForTarget,
  getVisibleSubTabsForSection,
  parseHostSettingsSection,
  resolveActiveSettingsLocation,
  searchSettingsEntries,
} from "@/components/settings/settings-nav";
import {
  HostStatusIndicator,
  hostConnectionLabel,
} from "@/components/settings/settings-primitives";
import { ShortcutsSection } from "@/components/settings/shortcuts-section";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/dialog-tabs";
import { IconButton } from "@/components/ui/icon-button";
import { Input } from "@/components/ui/input";
import { ShortcutKbd } from "@/components/ui/kbd";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { PanelErrorBoundary } from "@/components/ui/panel-error-boundary";
import { ScrollArea } from "@/components/ui/scroll-area";
import { type MessageKey, type TranslationParams, useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import { type HostCardView, isMacDesktop } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import {
  orbitActions,
  threadScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const CORE_SECTION_ICONS: Record<
  SettingsCoreSectionId,
  ComponentType<{ className?: string }>
> = {
  general: Settings2,
  appearance: Palette,
  notifications_sync: Bell,
  hosts: Server,
  agents: Cpu,
  usage: BarChart3,
  instructions_rules: FileText,
  capabilities: Plug,
};

function orderHostsLocalFirst(hosts: HostCardView[]): HostCardView[] {
  const local: HostCardView[] = [];
  const remote: HostCardView[] = [];
  for (const host of hosts) {
    if (isLocalHostCard(host)) {
      local.push(host);
    } else {
      remote.push(host);
    }
  }
  return [...local, ...remote];
}

function formatHostTransportSubtitle(
  host: HostCardView,
  t: (key: MessageKey, params?: TranslationParams) => string,
): string {
  if (isLocalHostCard(host)) return t("common.local");
  if (host.ssh_alias) return t("settings.sshHostSubtitle", { alias: host.ssh_alias });
  if (host.connection_state === "online_relay") return t("common.relay");
  return hostConnectionLabel(host.connection_state);
}

function SettingsRailButton({
  label,
  icon: Icon,
  active,
  badge,
  onClick,
}: {
  label: string;
  icon: ComponentType<{ className?: string }>;
  active: boolean;
  badge?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-[30px] w-full cursor-pointer items-center justify-between gap-2 rounded-[7px] px-2.5 text-left text-[13px] transition-colors",
        active
          ? "bg-accent text-ink-primary font-medium"
          : "text-ink-secondary hover:bg-accent/55 hover:text-ink-primary",
      )}
    >
      <span className="flex min-w-0 items-center gap-2.5">
        <Icon
          className={cn(
            "h-[14px] w-[14px] shrink-0 stroke-[1.75]",
            active ? "text-ink-primary" : "text-ink-muted",
          )}
        />
        <span className="truncate">{label}</span>
      </span>
      {badge}
    </button>
  );
}

function SettingsSubTabBar({
  ariaLabel,
  tabs,
  activeId,
  onSelect,
}: {
  ariaLabel: string;
  tabs: readonly { id: SettingsSubTabId; label: string; count?: number }[];
  activeId: SettingsSubTabId;
  onSelect: (id: SettingsSubTabId) => void;
}) {
  if (tabs.length <= 1) return null;

  return (
    <div className="border-border-subtle shrink-0 border-b py-3 pr-14 pl-8">
      <Tabs
        value={activeId}
        onValueChange={(value) => {
          if (typeof value === "string") {
            onSelect(value as SettingsSubTabId);
          }
        }}
      >
        <TabsList
          aria-label={ariaLabel}
          className="bg-surface-recessed border-border-subtle/70 h-auto min-h-[30px] max-w-full flex-wrap justify-start gap-0.5 rounded-[9px] border p-[3px]"
        >
          {tabs.map((tab) => (
            <TabsTrigger
              key={tab.id}
              value={tab.id}
              className="text-ink-muted hover:text-ink-primary data-[active]:bg-surface-raised data-[active]:text-ink-primary h-[24px] rounded-[6px] px-2.5 py-0 text-[12px] font-medium transition-colors data-[active]:shadow-2xs"
            >
              <span>{tab.label}</span>
              {tab.count !== undefined && tab.count > 0 && (
                <span className="text-ink-faint text-[11px] font-normal tabular-nums">
                  {tab.count}
                </span>
              )}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>
    </div>
  );
}

export function SettingsView({ closing = false }: { closing?: boolean }) {
  const { locale, t } = useI18n();
  const section = useOrbitSelector((s) => s.settingsSection);
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);

  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const [filterQuery, setFilterQuery] = useState("");
  const [searchFocused, setSearchFocused] = useState(false);
  const [selectedSearchIndex, setSelectedSearchIndex] = useState(0);

  const mod = modSymbol();

  // Scope Cmd/Ctrl+F strictly to Settings search while Settings modal is open
  useEffect(() => {
    if (closing) return;
    const onKeyDown = (e: KeyboardEvent) => {
      const hasMod = isMacDesktop() ? e.metaKey : e.ctrlKey;
      if (!hasMod || e.shiftKey || e.altKey || e.key.toLowerCase() !== "f") {
        return;
      }
      const snap = orbitActions.getSnapshot();
      if (snap.activeView !== "settings" || snap.commandPaletteOpen) return;
      if (
        document.querySelector(
          '[data-slot="dialog-content"], [data-slot="confirm-dialog"], [role="alertdialog"]',
        )
      ) {
        return;
      }
      e.preventDefault();
      searchInputRef.current?.focus();
      searchInputRef.current?.select();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [closing]);

  const archivedCount = useMemo(() => {
    const seen = new Set<string>();
    for (const [hostId, detail] of Object.entries(hostDetails)) {
      for (const ag of detail.agents) {
        if (ag.background) continue;
        const tKey = threadScopeKey(hostId, ag.agent_id);
        if (!ag.archived_at_rfc3339 && !archivedThreads[tKey]) continue;
        seen.add(tKey);
      }
    }
    return seen.size;
  }, [hostDetails, archivedThreads]);

  const parsedHost = useMemo(() => parseHostSettingsSection(section), [section]);
  const orderedHosts = useMemo(() => orderHostsLocalFirst(fleet.hosts), [fleet.hosts]);
  const hasRemoteHosts = useMemo(
    () => orderedHosts.some((h) => !isLocalHostCard(h)),
    [orderedHosts],
  );
  const isMultiOrRemoteMode = hasRemoteHosts || orderedHosts.length > 1;

  const [pickedHostId, setPickedHostId] = useState<string | null>(
    () => parsedHost?.hostId ?? selectedHostId ?? orderedHosts[0]?.host_id ?? null,
  );

  useEffect(() => {
    if (parsedHost?.hostId && orderedHosts.some((h) => h.host_id === parsedHost.hostId)) {
      setPickedHostId(parsedHost.hostId);
    }
  }, [parsedHost?.hostId, orderedHosts]);

  const activeHost = useMemo(() => {
    if (parsedHost) {
      const matched = orderedHosts.find((h) => h.host_id === parsedHost.hostId);
      if (matched) return matched;
    }
    if (pickedHostId) {
      const matched = orderedHosts.find((h) => h.host_id === pickedHostId);
      if (matched) return matched;
    }
    if (selectedHostId) {
      const matched = orderedHosts.find((h) => h.host_id === selectedHostId);
      if (matched) return matched;
    }
    return orderedHosts[0] ?? null;
  }, [parsedHost, pickedHostId, selectedHostId, orderedHosts]);

  const resolvedHost = useMemo(() => {
    if (!parsedHost) return null;
    const exists = orderedHosts.some((h) => h.host_id === parsedHost.hostId);
    if (exists) return parsedHost;
    const fallback = orderedHosts[0];
    return fallback ? { hostId: fallback.host_id, tab: parsedHost.tab } : null;
  }, [parsedHost, orderedHosts]);

  const effectiveHostId = resolvedHost?.hostId ?? activeHost?.host_id ?? null;
  const activeLocation = useMemo(() => resolveActiveSettingsLocation(section), [section]);
  const activeCoreSection = activeLocation.coreSection;

  const activeCoreSpec = useMemo(
    () =>
      SETTINGS_CORE_SECTIONS.find((item) => item.id === activeCoreSection) ??
      SETTINGS_CORE_SECTIONS[0],
    [activeCoreSection],
  );

  const visibleSubTabs = useMemo(() => {
    const specs = getVisibleSubTabsForSection(activeCoreSection, Boolean(effectiveHostId));
    return specs.map((spec) => ({
      id: spec.id,
      label: t(spec.labelKey),
      count: spec.id === "archived" ? archivedCount : undefined,
    }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeCoreSection, effectiveHostId, archivedCount, locale]);

  const activeSubTabId: SettingsSubTabId | null = useMemo(() => {
    if (visibleSubTabs.length === 0) return null;
    if (
      activeLocation.subTab &&
      visibleSubTabs.some((tab) => tab.id === activeLocation.subTab)
    ) {
      return activeLocation.subTab;
    }
    return visibleSubTabs[0]?.id ?? null;
  }, [activeLocation.subTab, visibleSubTabs]);

  const activePanelName = useMemo(() => {
    if (activeSubTabId) {
      const tab = visibleSubTabs.find((item) => item.id === activeSubTabId);
      if (tab) return tab.label;
    }
    return t(activeCoreSpec.labelKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSubTabId, visibleSubTabs, activeCoreSpec.labelKey, locale]);

  const normalizedFilter = filterQuery.trim();
  const isSearching = normalizedFilter.length > 0;

  const searchResults = useMemo(
    () =>
      searchSettingsEntries(normalizedFilter, {
        t: (key) => t(key),
        hasHost: Boolean(effectiveHostId),
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [normalizedFilter, effectiveHostId, locale],
  );

  useEffect(() => {
    setSelectedSearchIndex(0);
  }, [normalizedFilter]);

  const clampedSearchIndex =
    searchResults.length > 0
      ? Math.min(selectedSearchIndex, searchResults.length - 1)
      : 0;

  const handleSelectHost = (hostId: string) => {
    setPickedHostId(hostId);
    if (parsedHost) {
      orbitActions.setSettingsSection(`host:${hostId}:${parsedHost.tab}`);
    } else if (section === "add_host") {
      orbitActions.setSettingsSection(`host:${hostId}:connection`);
    }
  };

  const navigateCoreSection = (coreId: SettingsCoreSectionId) => {
    orbitActions.setSettingsSection(
      buildSettingsSectionForTarget(coreId, null, effectiveHostId),
    );
  };

  const handleSelectSubTab = (subTabId: SettingsSubTabId) => {
    orbitActions.setSettingsSection(
      buildSettingsSectionForTarget(activeCoreSection, subTabId, effectiveHostId),
    );
  };

  const handleJumpToSearchResult = (result: SettingsSearchResult) => {
    orbitActions.setSettingsSection(
      buildSettingsSectionForTarget(result.coreSection, result.subTab, effectiveHostId),
    );
  };

  const handleSearchKeyDown = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") {
      if (filterQuery.length > 0) {
        e.preventDefault();
        e.stopPropagation();
        setFilterQuery("");
      } else {
        e.currentTarget.blur();
      }
      return;
    }
    if (!isSearching || searchResults.length === 0) return;

    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedSearchIndex((prev) => (prev + 1) % searchResults.length);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedSearchIndex(
        (prev) => (prev - 1 + searchResults.length) % searchResults.length,
      );
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const target = searchResults[clampedSearchIndex];
      if (target) {
        handleJumpToSearchResult(target);
      }
    }
  };

  const renderCoreBadge = (id: SettingsCoreSectionId) => {
    if (id === "notifications_sync" && archivedCount > 0) {
      return (
        <span className="text-ink-faint shrink-0 font-mono text-[11px] tabular-nums">
          {archivedCount}
        </span>
      );
    }
    return undefined;
  };

  const subTabsAriaLabel = t("settings.subTabsAria", {
    section: t(activeCoreSpec.labelKey),
  });

  const isWidePanel = resolvedHost?.tab === "usage";

  return (
    <div
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) {
          orbitActions.closeSettings();
        }
      }}
      style={{
        animation: closing
          ? "orbit-backdrop-out 155ms var(--ease-exit) forwards"
          : "orbit-backdrop-in 195ms var(--ease-out-quart)",
      }}
      className={cn(
        "fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-5 select-none dark:bg-black/55",
        closing && "pointer-events-none",
      )}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("settings.title")}
        style={{
          animation: closing
            ? "orbit-modal-card-out 155ms var(--ease-exit) forwards"
            : "orbit-modal-card-in 210ms var(--ease-out-expo)",
        }}
        className="border-border-default bg-surface-base text-ink-primary floating-surface relative flex h-[min(760px,88vh)] w-[min(1100px,94vw)] overflow-hidden rounded-[16px] border"
      >
        <IconButton
          size="md"
          tooltip={t("settings.closeSettings")}
          onClick={() => orbitActions.closeSettings()}
          className="absolute top-3 right-3.5 z-10"
        >
          <X className="h-[15px] w-[15px]" />
        </IconButton>

        {/* Left Settings Rail (236px): 3 Domain Groups (App / Host / Workspace) + Search */}
        <aside className="border-border-subtle bg-surface-recessed/70 flex w-[236px] shrink-0 flex-col border-r py-4">
          <div className="px-4 pb-3 text-[14px] font-semibold tracking-[-0.015em]">
            {t("settings.title")}
          </div>

          <div className="mb-3 px-3">
            <div className="group/search relative">
              <Search className="text-ink-faint pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 stroke-[1.8]" />
              <Input
                ref={searchInputRef}
                type="text"
                value={filterQuery}
                onChange={(e) => setFilterQuery(e.target.value)}
                onFocus={() => setSearchFocused(true)}
                onBlur={() => setSearchFocused(false)}
                onKeyDown={handleSearchKeyDown}
                placeholder={t("settings.searchPlaceholder")}
                aria-label={t("settings.searchAria")}
                className="h-[30px] pr-11 pl-7.5 text-[12px] shadow-none"
              />
              {filterQuery ? (
                <button
                  type="button"
                  aria-label={t("settings.clearSearch")}
                  onClick={() => {
                    setFilterQuery("");
                    searchInputRef.current?.focus();
                  }}
                  className="text-ink-muted hover:text-ink-primary absolute top-1/2 right-2 -translate-y-1/2 cursor-pointer rounded p-0.5 transition-colors"
                >
                  <X className="size-3" />
                </button>
              ) : (
                !searchFocused && (
                  <ShortcutKbd
                    keys={`${mod}F`}
                    size="xs"
                    variant="subtle"
                    className="pointer-events-none absolute top-1/2 right-1.5 -translate-y-1/2 opacity-0 transition-opacity group-hover/search:opacity-100"
                  />
                )
              )}
            </div>
          </div>

          <ScrollArea className="min-h-0 flex-1" viewportClassName="px-3 pb-2">
            {isSearching ? (
              <div className="space-y-1">
                {searchResults.length > 0 ? (
                  <>
                    <div className="text-ink-faint px-2.5 pb-1 text-[10.5px] font-semibold tracking-[0.05em] uppercase">
                      {t("settings.searchResultsLabel")}
                    </div>
                    <nav className="space-y-0.5">
                      {searchResults.map((result, idx) => {
                        const Icon = CORE_SECTION_ICONS[result.coreSection];
                        const isActiveDest =
                          activeCoreSection === result.coreSection &&
                          (result.subTab === null || activeSubTabId === result.subTab);
                        const isHighlighted = idx === clampedSearchIndex;

                        return (
                          <button
                            key={result.id}
                            type="button"
                            onMouseEnter={() => setSelectedSearchIndex(idx)}
                            onClick={() => handleJumpToSearchResult(result)}
                            className={cn(
                              "flex w-full cursor-pointer items-center justify-between gap-2 rounded-[7px] px-2.5 py-1.5 text-left transition-colors",
                              isHighlighted || isActiveDest
                                ? "bg-accent text-ink-primary"
                                : "text-ink-secondary hover:bg-accent/55 hover:text-ink-primary",
                            )}
                          >
                            <span className="flex min-w-0 items-center gap-2.5">
                              <Icon className="text-ink-muted h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                              <span className="min-w-0">
                                <span
                                  className={cn(
                                    "block truncate text-[12.5px] leading-snug",
                                    isActiveDest ? "font-medium" : "font-normal",
                                  )}
                                >
                                  {t(result.labelKey)}
                                </span>
                                {result.parentLabelKey && (
                                  <span className="text-ink-faint block truncate text-[11px] leading-tight">
                                    {t(result.parentLabelKey)}
                                  </span>
                                )}
                              </span>
                            </span>
                            {result.subTab === "archived" && archivedCount > 0 && (
                              <span className="text-ink-faint shrink-0 font-mono text-[11px] tabular-nums">
                                {archivedCount}
                              </span>
                            )}
                          </button>
                        );
                      })}
                    </nav>
                  </>
                ) : (
                  <div className="text-ink-muted px-2.5 py-8 text-center text-[12px]">
                    {t("settings.noMatches")}
                  </div>
                )}
              </div>
            ) : (
              <nav className="space-y-4">
                {SETTINGS_NAV_GROUPS.map((group) => {
                  const groupItems = SETTINGS_CORE_SECTIONS.filter(
                    (item) => item.group === group.id,
                  );
                  return (
                    <div key={group.id} className="space-y-1">
                      <div className="text-ink-faint px-2.5 pb-0.5 text-[10.5px] font-semibold tracking-[0.06em] uppercase">
                        {t(group.labelKey)}
                      </div>

                      {/* Multi-host selector lives right at the top of the HOST group */}
                      {group.id === "host" && isMultiOrRemoteMode && activeHost && (
                        <div className="pb-1">
                          <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                              <button
                                type="button"
                                aria-label={t("settings.switchHost")}
                                className="border-border-subtle bg-surface-base hover:bg-accent/50 focus-visible:ring-border-strong flex h-[30px] w-full cursor-pointer items-center justify-between gap-2 rounded-[7px] border px-2.5 text-left transition-colors outline-none focus-visible:ring-1"
                              >
                                <span className="flex min-w-0 flex-1 items-center gap-2">
                                  <HostStatusIndicator state={activeHost.connection_state} />
                                  <span className="text-ink-primary truncate text-[12px] font-medium">
                                    {activeHost.display_name}
                                  </span>
                                  {isLocalHostCard(activeHost) && (
                                    <span className="bg-surface-recessed text-ink-muted shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium">
                                      {t("common.local")}
                                    </span>
                                  )}
                                </span>
                                <ChevronsUpDown className="text-ink-faint h-[12.5px] w-[12.5px] shrink-0" />
                              </button>
                            </DropdownMenuTrigger>

                            <DropdownMenuContent
                              align="start"
                              sideOffset={5}
                              finalFocus={false}
                              className="min-w-[212px] max-w-[260px]"
                            >
                              <DropdownMenuGroup>
                                {orderedHosts.map((host) => {
                                  const selected = host.host_id === activeHost.host_id;
                                  return (
                                    <DropdownMenuItem
                                      key={host.host_id}
                                      onSelect={() => handleSelectHost(host.host_id)}
                                      className="items-center py-1.5"
                                    >
                                      <span className="flex min-w-0 flex-1 items-center gap-2.5">
                                        <HostStatusIndicator state={host.connection_state} />
                                        <span className="min-w-0 flex-1">
                                          <span
                                            className={cn(
                                              "block truncate text-[12.5px]",
                                              selected
                                                ? "text-ink-primary font-medium"
                                                : "text-ink-secondary font-normal",
                                            )}
                                          >
                                            {host.display_name}
                                          </span>
                                          <span className="text-ink-faint block truncate text-[11px]">
                                            {formatHostTransportSubtitle(host, t)}
                                          </span>
                                        </span>
                                      </span>
                                      {selected && (
                                        <Check className="text-ink-primary h-[13px] w-[13px] shrink-0 stroke-[2]" />
                                      )}
                                    </DropdownMenuItem>
                                  );
                                })}
                              </DropdownMenuGroup>
                              <DropdownMenuSeparator />
                              <DropdownMenuGroup>
                                <DropdownMenuItem
                                  onSelect={() => orbitActions.setSettingsSection("add_host")}
                                >
                                  <span className="flex items-center gap-2 whitespace-nowrap">
                                    <Plus className="text-ink-muted size-[13px] stroke-[1.85]" />
                                    <span>{t("settings.connectHost")}</span>
                                  </span>
                                </DropdownMenuItem>
                              </DropdownMenuGroup>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </div>
                      )}

                      <div className="space-y-0.5">
                        {groupItems.map((item) => (
                          <SettingsRailButton
                            key={item.id}
                            label={t(item.labelKey)}
                            icon={CORE_SECTION_ICONS[item.id]}
                            active={activeCoreSection === item.id}
                            badge={renderCoreBadge(item.id)}
                            onClick={() => navigateCoreSection(item.id)}
                          />
                        ))}
                      </div>
                    </div>
                  );
                })}
              </nav>
            )}
          </ScrollArea>
        </aside>

        {/* Right Settings Content Area */}
        <div className="bg-surface-base flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          {activeSubTabId && visibleSubTabs.length > 1 && (
            <SettingsSubTabBar
              ariaLabel={subTabsAriaLabel}
              activeId={activeSubTabId}
              tabs={visibleSubTabs}
              onSelect={handleSelectSubTab}
            />
          )}

          {section === "instructions" ? (
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden px-7 pt-5 pb-5">
              <div key={section} className="orbit-stage-enter min-h-0 flex-1">
                <PanelErrorBoundary panelName={activePanelName}>
                  <InstructionsSection />
                </PanelErrorBoundary>
              </div>
            </div>
          ) : (
            <ScrollArea className="min-h-0 flex-1" viewportClassName="px-8 py-6">
              <div
                key={section}
                className={cn(
                  "orbit-stage-enter mx-auto w-full pb-8",
                  isWidePanel ? "max-w-[840px]" : "max-w-[680px]",
                )}
              >
                <PanelErrorBoundary panelName={activePanelName}>
                  {section === "general" && <GeneralSection />}
                  {section === "shortcuts" && <ShortcutsSection />}
                  {section === "appearance" && <AppearanceSection />}
                  {section === "notifications" && <NotificationsSection />}
                  {section === "cloud_sync" && <CloudSyncSection />}
                  {section === "archived" && <ArchivedSection />}
                  {section === "add_host" && <AddHostSection />}
                  {parsedHost && !resolvedHost && <AddHostSection />}
                  {resolvedHost?.tab === "connection" && (
                    <HostConnectionSection hostId={resolvedHost.hostId} />
                  )}
                  {resolvedHost?.tab === "im_channels" && (
                    <HostImChannelsSection
                      key={resolvedHost.hostId}
                      hostId={resolvedHost.hostId}
                    />
                  )}
                  {resolvedHost?.tab === "agents" && (
                    <HostAgentsSection hostId={resolvedHost.hostId} />
                  )}
                  {resolvedHost?.tab === "router" && (
                    <HostPersonalRouterSection
                      key={resolvedHost.hostId}
                      hostId={resolvedHost.hostId}
                    />
                  )}
                  {resolvedHost?.tab === "acp" && (
                    <HostAcpSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
                  )}
                  {resolvedHost?.tab === "guardian" && (
                    <HostGuardianSection
                      key={resolvedHost.hostId}
                      hostId={resolvedHost.hostId}
                    />
                  )}
                  {resolvedHost?.tab === "usage" && (
                    <HostUsageSection hostId={resolvedHost.hostId} />
                  )}
                  {resolvedHost?.tab === "rules" && (
                    <HostRulesSection
                      key={resolvedHost.hostId}
                      hostId={resolvedHost.hostId}
                    />
                  )}
                  {resolvedHost?.tab === "skills" && (
                    <HostSkillsSection
                      key={resolvedHost.hostId}
                      hostId={resolvedHost.hostId}
                    />
                  )}
                  {resolvedHost?.tab === "mcp" && (
                    <HostMcpSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
                  )}
                  {resolvedHost?.tab === "hooks" && (
                    <HostHooksSection
                      key={resolvedHost.hostId}
                      hostId={resolvedHost.hostId}
                    />
                  )}
                  {resolvedHost?.tab === "lsp" && (
                    <HostLspSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
                  )}
                </PanelErrorBoundary>
              </div>
            </ScrollArea>
          )}
        </div>
      </div>
    </div>
  );
}
