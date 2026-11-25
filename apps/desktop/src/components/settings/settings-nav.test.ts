import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { en, type MessageKey } from "../../lib/i18n/locales/en.ts";
import { ja } from "../../lib/i18n/locales/ja.ts";
import { zhCN } from "../../lib/i18n/locales/zh-CN.ts";
import {
  LEGACY_SETTING_ANCHORS,
  SETTINGS_CORE_SECTIONS,
  SETTINGS_NAV_GROUPS,
  SETTINGS_SEARCH_CATALOG,
  type SettingsCoreSectionId,
  type SettingsSectionTarget,
  type SettingsSubTabId,
  parseHostSettingsSection,
  resolveActiveSettingsLocation,
  resolveSettingsSectionTarget,
  searchSettingsEntries,
} from "./settings-nav.ts";

const t = (key: MessageKey) => en[key];
const hostId = "ipv6:2001:db8::42";

describe("Settings navigation", () => {
  it("contains the sixteen sections in sidebar order within their four domains", () => {
    const grouped = SETTINGS_NAV_GROUPS.map((group) => [
      group.id, ...SETTINGS_CORE_SECTIONS.filter((s) => s.group === group.id).map((s) => s.id),
    ]);
    assert.deepEqual(grouped, [
      ["preferences", "general", "appearance", "shortcuts", "notifications", "voice"],
      ["fleet_sync", "cloud_sync", "hosts"],
      ["agents_safety", "agents", "personal_router", "safety", "usage"],
      ["context_extensions", "instructions_memory", "mcp_tools", "skills_plugins", "im_channels", "data_diagnostics"],
    ]);
    for (const { id } of SETTINGS_CORE_SECTIONS) {
      const resolved = resolveSettingsSectionTarget(id, hostId);
      assert.equal(resolveActiveSettingsLocation(resolved).coreSection, id);
    }
  });

  it("opens every legacy target at its page and, where applicable, its original resource", () => {
    const cases: [SettingsSectionTarget, SettingsCoreSectionId, SettingsSubTabId | null][] = [
      ["general", "general", null], ["appearance", "appearance", null],
      ["shortcuts", "shortcuts", null], ["notifications", "notifications", null],
      ["cloud_sync", "cloud_sync", null], ["archived", "data_diagnostics", "archived"],
      ["personal_router", "personal_router", null], ["router", "personal_router", null],
      ["hosts", "hosts", null], ["connection", "hosts", "connection"], ["add_host", "hosts", "add_host"],
      ["im_channels", "im_channels", "im_channels"], ["channels", "im_channels", "im_channels"],
      ["agents", "agents", "agents"], ["providers", "agents", "agents"], ["models_agents", "agents", "agents"],
      ["acp", "agents", "acp"], ["safety", "safety", "safety"], ["guardian", "safety", "guardian"], ["usage", "usage", "usage"],
      ["instructions", "instructions_memory", "instructions"], ["instructions_rules", "instructions_memory", "instructions"],
      ["extensions", "instructions_memory", "instructions"], ["context", "instructions_memory", "instructions"],
      ["rules", "instructions_memory", "rules"], ["memory", "instructions_memory", "memory"],
      ["capabilities", "skills_plugins", "skills"], ["skills", "skills_plugins", "skills"],
      ["plugins", "skills_plugins", "plugins"], ["voice", "voice", "voice"],
      ["mcp", "mcp_tools", "mcp"], ["hooks", "mcp_tools", "hooks"], ["lsp", "mcp_tools", "lsp"],
      ["notifications_sync", "notifications", null], ["sync_data", "cloud_sync", null],
    ];
    for (const [target, page, leaf] of cases) {
      for (const id of [hostId, null]) {
        const loc = resolveActiveSettingsLocation(resolveSettingsSectionTarget(target, id));
        assert.equal(loc.coreSection, page, target);
        assert.equal(loc.subTab, leaf, target);
        if (leaf && leaf !== "add_host") assert.equal(loc.hostId, id, target);
      }
    }
  });

  it("keeps explicitly linked hosts, even when the default host differs", () => {
    for (const tab of [
      "connection", "agents", "guardian", "im_channels", "skills", "plugins", "voice", "acp",
      "mcp", "rules", "memory", "hooks", "lsp", "usage", "instructions", "data_diagnostics", "archived",
    ] as const) {
      const link = `host:${hostId}:${tab}` as const;
      assert.deepEqual(parseHostSettingsSection(link), { hostId, tab });
      assert.equal(resolveSettingsSectionTarget(link, "some-other-host"), link);
      const loc = resolveActiveSettingsLocation(link);
      assert.equal(loc.hostId, hostId);
      assert.equal(loc.subTab, tab);
      assert.equal(loc.coreSection, resolveActiveSettingsLocation(resolveSettingsSectionTarget(tab, null)).coreSection);
    }
    assert.equal(resolveSettingsSectionTarget(`host:${hostId}:router`, "local"), "personal_router");
  });

  it("preserves the supplied fallback for malformed, missing, or unknown deep links", () => {
    for (const link of [undefined, "", "__proto__", "toString", "unknown", "host::mcp", "host:machine:unknown", "host:machine:"]) {
      assert.equal(resolveSettingsSectionTarget(link, hostId, "shortcuts"), "shortcuts");
    }
    assert.equal(parseHostSettingsSection("host:machine:constructor"), null);
  });
});

describe("Settings search", () => {
  it("indexes all page titles and concrete, uniquely addressable settings", () => {
    const ids = SETTINGS_SEARCH_CATALOG.map((item) => item.settingId ?? item.coreSection);
    assert.equal(new Set(ids).size, ids.length);
    for (const { id } of SETTINGS_CORE_SECTIONS) {
      assert.ok(SETTINGS_SEARCH_CATALOG.some((entry) => entry.coreSection === id && !entry.settingId), id);
      assert.ok(SETTINGS_SEARCH_CATALOG.some((entry) => entry.coreSection === id && entry.settingId), id);
    }
    assert.ok(SETTINGS_SEARCH_CATALOG.filter((item) => item.settingId).length >= 60);
    for (const result of SETTINGS_SEARCH_CATALOG) {
      const loc = resolveActiveSettingsLocation(resolveSettingsSectionTarget(result.target, hostId));
      assert.equal(loc.coreSection, result.coreSection, result.labelKey);
      for (const dict of [en, zhCN, ja]) assert.ok(dict[result.labelKey], result.labelKey);
    }
    for (const [leaf, anchor] of Object.entries(LEGACY_SETTING_ANCHORS)) {
      assert.ok(SETTINGS_SEARCH_CATALOG.some((entry) => entry.target === leaf && entry.settingId === anchor), leaf);
    }
  });

  it("matches individual settings, localized labels and keywords across all language choices", () => {
    const cases: [string, string | undefined][] = [
      ["diff layout", "settings.diffLayout"], ["分支前缀", "settings.branchPrefix"],
      ["入力マイク", "voice.microphoneTitle"], ["microphone", "voice.microphoneTitle"],
      ["keep awake", "settings.agentKeepAwake"], ["cloudflare", "settings.advancedRelayTitle"],
      ["CLAUDE.md", "settings.subTabInstructions"], ["飞书", "im.configuredChannels"],
      ["oauth-unused-term", undefined], ["   ", undefined],
    ];
    for (const dict of [en, zhCN, ja]) {
      for (const [query, id] of cases) {
        const result = searchSettingsEntries(query, { t: (key) => dict[key] })[0];
        assert.equal(result?.settingId, id, query);
      }
    }
    assert.equal(searchSettingsEntries("Appearance", { t })[0]?.id, "appearance");
    assert.equal(searchSettingsEntries("项目记忆", { t }).some((hit) => hit.settingId === "memory.title"), true);
  });

  it("filters modified rows using current values and combines the filter with text", () => {
    const modifiedIds = new Set(["settings.branchPrefix", "settings.diffLayout", "settings.defaultPermissions"]);
    assert.deepEqual(new Set(searchSettingsEntries("@modified", { t, modifiedIds }).map((hit) => hit.id)), modifiedIds);
    const [result] = searchSettingsEntries("diff @MODIFIED", { t, modifiedIds });
    assert.equal(result.settingId, "settings.diffLayout");
    assert.equal(result.target, "appearance");
    modifiedIds.delete("settings.diffLayout");
    assert.deepEqual(searchSettingsEntries("@modified diff", { t, modifiedIds }), []);
    assert.deepEqual(searchSettingsEntries("@modified", { t }), []);
  });

  it("omits native-only controls when searching from the web client", () => {
    const hits = searchSettingsEntries("external ide", { t, isDesktop: false });
    assert.ok(hits.every((hit) => hit.settingId !== "settings.externalEditor"));
    assert.ok(searchSettingsEntries("external ide", { t, isDesktop: true }).some((hit) => hit.settingId === "settings.externalEditor"));
  });

  it("keeps hostless searches relevant while retaining modified client defaults", () => {
    assert.deepEqual(searchSettingsEntries("lsp", { t, hasHost: false }), []);
    assert.equal(searchSettingsEntries("connect remote host", { t, hasHost: false })[0]?.target, "hosts");
    assert.equal(searchSettingsEntries("hermes", { t, hasHost: false })[0]?.target, "personal_router");
    assert.equal(searchSettingsEntries("@modified", { t, hasHost: false, modifiedIds: new Set(["im.defaultProvider"]) })[0]?.id, "im.defaultProvider");
  });
});
