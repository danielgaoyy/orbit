import { en, type MessageKey } from "../../lib/i18n/locales/en.ts";
import { ja } from "../../lib/i18n/locales/ja.ts";
import { zhCN } from "../../lib/i18n/locales/zh-CN.ts";
import { SETTING_ENTRIES } from "./settings-search-catalog.ts";

export const SETTINGS_NAV_GROUPS = [
  { id: "preferences", labelKey: "settings.nav4Preferences" },
  { id: "fleet_sync", labelKey: "settings.nav4FleetSync" },
  { id: "agents_safety", labelKey: "settings.nav4AgentsSafety" },
  { id: "context_extensions", labelKey: "settings.nav4ContextExtensions" },
] as const satisfies readonly { id: string; labelKey: MessageKey }[];
export type SettingsNavGroupId = (typeof SETTINGS_NAV_GROUPS)[number]["id"];

export const SETTINGS_CORE_SECTIONS = [
  { group: "preferences", id: "general", labelKey: "settings.navGeneral" },
  { group: "preferences", id: "appearance", labelKey: "settings.navAppearance" },
  { group: "preferences", id: "git_worktrees", labelKey: "settings.groupGitWorktrees" },
  { group: "preferences", id: "shortcuts", labelKey: "settings.subTabShortcuts" },
  { group: "preferences", id: "notifications", labelKey: "settings.subTabNotifications" },
  { group: "preferences", id: "voice", labelKey: "settings.subTabVoice" },
  { group: "fleet_sync", id: "cloud_sync", labelKey: "settings.subTabCloudSync" },
  { group: "fleet_sync", id: "hosts", labelKey: "settings.nav4Hosts" },
  { group: "agents_safety", id: "agents", labelKey: "settings.navAgents" },
  { group: "agents_safety", id: "personal_router", labelKey: "settings.navPersonalRouter" },
  { group: "agents_safety", id: "safety", labelKey: "settings.nav4Safety" },
  { group: "agents_safety", id: "usage", labelKey: "settings.navUsage" },
  { group: "context_extensions", id: "instructions_memory", labelKey: "settings.nav4Instructions" },
  { group: "context_extensions", id: "mcp_tools", labelKey: "settings.nav4Mcp" },
  { group: "context_extensions", id: "skills_plugins", labelKey: "settings.nav4Skills" },
  { group: "context_extensions", id: "im_channels", labelKey: "settings.subTabImChannels" },
  { group: "context_extensions", id: "data_diagnostics", labelKey: "settings.nav4Data" },
] as const satisfies readonly { group: SettingsNavGroupId; id: string; labelKey: MessageKey }[];
export type SettingsCoreSectionId = (typeof SETTINGS_CORE_SECTIONS)[number]["id"];

// Stable leaf names are also used by command-palette actions and persisted host links.
const HOST_ROUTES = {
  agents: "agents",
  acp: "agents",
  router: "personal_router",
  safety: "safety",
  guardian: "safety",
  im_channels: "im_channels",
  skills: "skills_plugins",
  plugins: "skills_plugins",
  voice: "voice",
  mcp: "mcp_tools",
  hooks: "mcp_tools",
  lsp: "mcp_tools",
  rules: "instructions_memory",
  memory: "instructions_memory",
  instructions: "instructions_memory",
  usage: "usage",
  connection: "hosts",
  data_diagnostics: "data_diagnostics",
  archived: "data_diagnostics",
} as const satisfies Record<string, SettingsCoreSectionId>;
export type HostSettingsTab = keyof typeof HOST_ROUTES;
export type SettingsSubTabId = HostSettingsTab | "add_host";

export const LEGACY_SETTING_ANCHORS: Partial<Record<SettingsSubTabId, MessageKey>> = {
  acp: "settings.subTabAcpRegistry",
  guardian: "guardian.groupPolicy",
  hooks: "hooks.title",
  lsp: "settings.subTabLsp",
  plugins: "plugins.title",
  archived: "dataOps.archive",
};
export type SettingsPage = SettingsCoreSectionId | SettingsSubTabId;
export type SettingsSection = SettingsPage | `host:${string}:${HostSettingsTab}`;

// A page can contain multiple independent resources. Keep their route even when no host
// is connected so an empty state can still offer the appropriate next action.
const ALIASES = {
  providers: "agents",
  models_agents: "agents",
  router: "personal_router",
  channels: "im_channels",
  extensions: "instructions",
  context: "instructions",
  instructions_rules: "instructions",
  instructions_memory: "instructions",
  capabilities: "skills",
  skills_plugins: "skills",
  mcp_tools: "mcp",
  notifications_sync: "notifications",
  sync_data: "cloud_sync",
} as const satisfies Record<string, SettingsPage>;
export type SettingsLegacyAlias = keyof typeof ALIASES;
export type SettingsSectionTarget = SettingsSection | SettingsLegacyAlias;

function isHostTab(value: string): value is HostSettingsTab {
  return Object.hasOwn(HOST_ROUTES, value);
}

export function parseHostSettingsSection(section: string): { hostId: string; tab: HostSettingsTab } | null {
  if (!section.startsWith("host:")) return null;
  const separator = section.lastIndexOf(":");
  // Only the final colon delimits the tab: host IDs may include ports and IPv6 literals.
  if (separator <= 5) return null;
  const tab = section.slice(separator + 1);
  return isHostTab(tab) ? { hostId: section.slice(5, separator), tab } : null;
}

export function resolveSettingsSectionTarget(
  target: string | undefined,
  defaultHostId: string | null,
  fallbackSection: SettingsSection = "general",
): SettingsSection {
  if (!target) return fallbackSection;
  const explicit = parseHostSettingsSection(target);
  if (explicit) {
    if (explicit.tab === "router") return "personal_router";
    return `host:${explicit.hostId}:${explicit.tab}`;
  }
  const page = Object.hasOwn(ALIASES, target) ? ALIASES[target as SettingsLegacyAlias] : target;
  if (isHostTab(page)) return defaultHostId ? `host:${defaultHostId}:${page}` : page;
  if (page === "add_host") return page;
  return SETTINGS_CORE_SECTIONS.find(({ id }) => id === page)?.id ?? fallbackSection;
}

export interface ResolvedSettingsLocation {
  coreSection: SettingsCoreSectionId;
  subTab: SettingsSubTabId | null;
  hostId: string | null;
  section: SettingsSection;
}

export function resolveActiveSettingsLocation(section: SettingsSection): ResolvedSettingsLocation {
  const host = parseHostSettingsSection(section);
  const page = host?.tab ?? section;
  if (page === "add_host") return { section, coreSection: "hosts", subTab: page, hostId: null };
  if (isHostTab(page)) {
    return { section, coreSection: HOST_ROUTES[page], subTab: page, hostId: host?.hostId ?? null };
  }
  const coreSection = SETTINGS_CORE_SECTIONS.find(({ id }) => id === page)?.id ?? "general";
  const subTab = Object.hasOwn(ALIASES, coreSection) ? ALIASES[coreSection as SettingsLegacyAlias] : null;
  return { section, coreSection, subTab: subTab && isHostTab(subTab) ? subTab : null, hostId: null };
}

interface SearchEntry {
  coreSection: SettingsCoreSectionId;
  labelKey: MessageKey;
  target: SettingsPage;
  settingId?: MessageKey;
  keywords?: string;
}

export interface SettingsSearchResult extends SearchEntry {
  id: string;
  parentLabelKey: MessageKey | null;
  score: number;
}

const hostRequiredTargets = new Set<SettingsPage>([
  "agents", "acp", "guardian", "connection", "usage", "voice", "rules", "memory", "im_channels",
  "mcp_tools", "mcp", "hooks", "lsp", "skills_plugins", "skills", "plugins",
]);
export const SETTINGS_SEARCH_CATALOG: readonly SearchEntry[] = [
  ...SETTINGS_CORE_SECTIONS.map(({ id, labelKey }) => ({ coreSection: id, target: id, labelKey })),
  ...SETTING_ENTRIES.map(([target, labelKey, keywords]) => ({
    coreSection: resolveActiveSettingsLocation(target).coreSection,
    target,
    labelKey,
    settingId: labelKey,
    keywords,
  })),
];

const sectionLabels = new Map<SettingsCoreSectionId, MessageKey>(SETTINGS_CORE_SECTIONS.map(({ id, labelKey }) => [id, labelKey]));

function matchScore(phrase: string, currentLabel: string, labels: string[], keywords: string) {
  if (!phrase) return 0;
  if (currentLabel === phrase) return 100;
  if (labels.includes(phrase)) return 95;
  if (labels.some((label) => label.startsWith(phrase))) return 80;
  if (labels.some((label) => label.includes(phrase))) return 70;
  if (keywords.includes(phrase)) return 60;
  return 30;
}

function normalize(text: string): string {
  return text.normalize("NFKC").toLowerCase().replace(/[_–-]+/gu, " ").trim();
}

// Build the locale-independent index once. Only the query changes while typing.
const index = SETTINGS_SEARCH_CATALOG.map((entry) => {
  const labels = [en, zhCN, ja].map((dict) => normalize(dict[entry.labelKey] ?? entry.labelKey));
  const keywords = normalize(entry.keywords ?? "");
  const parentLabelKey = entry.settingId ? sectionLabels.get(entry.coreSection) ?? null : null;
  const fields = [...labels, keywords, normalize(entry.target)];
  if (parentLabelKey) fields.push(...[en, zhCN, ja].map((dict) => normalize(dict[parentLabelKey] ?? parentLabelKey)));
  return { entry, fields, labels, keywords, parentLabelKey };
});

export function searchSettingsEntries(
  rawQuery: string,
  options: { t: (key: MessageKey) => string; hasHost?: boolean; isDesktop?: boolean },
): SettingsSearchResult[] {
  const terms = normalize(rawQuery).split(/\s+/u).filter(Boolean);
  if (!terms.length) return [];
  const results: SettingsSearchResult[] = [];
  const phrase = terms.join(" ");
  for (const { entry, fields, labels, keywords, parentLabelKey } of index) {
    if (options.isDesktop === false && (entry.settingId === "settings.appVersion" || entry.settingId === "settings.externalEditor")) continue;
    if (options.hasHost === false && hostRequiredTargets.has(entry.target) && entry.settingId !== "im.defaultProvider") continue;

    if (!terms.every((term) => fields.some((field) => field.includes(term)))) continue;
    const score = matchScore(phrase, normalize(options.t(entry.labelKey) ?? entry.labelKey), labels, keywords);
    results.push({ ...entry, id: entry.settingId ?? entry.coreSection, parentLabelKey, score });
  }
  return results.sort((a, b) => b.score - a.score);
}
