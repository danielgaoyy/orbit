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
      ["preferences", "general", "appearance", "workspace", "shortcuts", "notifications", "voice"],
      ["fleet_sync", "hosts", "cloud_sync"],
      ["agents_safety", "agents", "permissions", "usage"],
      ["context_extensions", "context", "mcp", "hooks", "extensions", "channels"],
    ]);
    for (const { id } of SETTINGS_CORE_SECTIONS) {
      const resolved = resolveSettingsSectionTarget(id, hostId);
      assert.equal(resolveActiveSettingsLocation(resolved).coreSection, id);
    }
  });

  it("opens every legacy target at its page and, where applicable, its original resource", () => {
    const cases: [SettingsSectionTarget, SettingsCoreSectionId, SettingsSubTabId | null][] = [
      ["general", "general", null], ["appearance", "appearance", null],
      ["workspace", "workspace", null], ["git_worktrees", "workspace", null],
      ["shortcuts", "shortcuts", null], ["notifications", "notifications", null],
      ["cloud_sync", "cloud_sync", null],
      ["hosts", "hosts", null], ["connection", "hosts", "connection"], ["add_host", "hosts", "add_host"],
      ["archived", "hosts", "archived"], ["data_diagnostics", "hosts", "data_diagnostics"],
      ["agents", "agents", "agents"], ["providers", "agents", "agents"], ["models_agents", "agents", "agents"],
      ["acp", "agents", "acp"], ["personal_router", "agents", "router"], ["router", "agents", "router"],
      ["permissions", "permissions", "permissions"], ["safety", "permissions", "permissions"],
      ["guardian", "permissions", "guardian"], ["usage", "usage", "usage"],
      ["context", "context", "instructions"], ["instructions", "context", "instructions"],
      ["instructions_rules", "context", "instructions"], ["instructions_memory", "context", "instructions"],
      ["rules", "context", "rules"], ["memory", "context", "memory"],
      ["mcp", "mcp", "mcp"], ["mcp_tools", "mcp", "mcp"], ["hooks", "hooks", "hooks"],
      ["lsp", "workspace", "lsp"],
      ["extensions", "extensions", "skills"], ["skills_plugins", "extensions", "skills"],
      ["capabilities", "extensions", "skills"], ["skills", "extensions", "skills"],
      ["plugins", "extensions", "plugins"], ["voice", "voice", "voice"],
      ["channels", "channels", "channels"], ["im_channels", "channels", "im_channels"],
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
      "connection", "agents", "router", "permissions", "safety", "guardian", "channels", "im_channels",
      "skills", "plugins", "voice", "acp", "mcp", "rules", "memory", "hooks", "lsp", "usage",
      "instructions", "data_diagnostics", "archived",
    ] as const) {
      const link = `host:${hostId}:${tab}` as const;
      assert.deepEqual(parseHostSettingsSection(link), { hostId, tab });
      assert.equal(resolveSettingsSectionTarget(link, "some-other-host"), link);
      const loc = resolveActiveSettingsLocation(link);
      assert.equal(loc.hostId, hostId);
      assert.equal(loc.subTab, tab);
      assert.equal(loc.coreSection, resolveActiveSettingsLocation(resolveSettingsSectionTarget(tab, null)).coreSection);
    }
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
      ["排队发送", "settings.activeTurnSendBehavior"],
      ["入力マイク", "voice.microphoneTitle"], ["microphone", "voice.microphoneTitle"],
      ["keep awake", "settings.agentKeepAwake"], ["cloudflare", "settings.advancedRelayTitle"],
      ["CLAUDE.md", "settings.subTabInstructions"], ["飞书", "im.configuredChannels"],
      ["缩放", "settings.windowZoom"], ["文字サイズ", "settings.conversationTextSize"],
      ["monospace", "settings.codeTextSize"], ["oauth-unused-term", undefined], ["   ", undefined],
    ];
    for (const dict of [en, zhCN, ja]) {
      for (const [query, id] of cases) {
        const result = searchSettingsEntries(query, { t: (key) => dict[key] })[0];
        assert.equal(result?.settingId, id, query);
      }
    }
    assert.equal(searchSettingsEntries("Appearance", { t })[0]?.id, "appearance");
    assert.equal(searchSettingsEntries("分支前缀", { t })[0]?.target, "workspace");
    assert.equal(searchSettingsEntries("collapse exploration", { t })[0]?.target, "appearance");
    assert.equal(searchSettingsEntries("项目记忆", { t }).some((hit) => hit.settingId === "memory.title"), true);
    assert.equal(searchSettingsEntries("mobile push", { t }).length, 0);
    assert.equal(searchSettingsEntries("desktop quiet hours", { t })[0]?.settingId, "settings.notifyQuietHours");
  });

  it("omits native-only controls when searching from the web client", () => {
    const hits = searchSettingsEntries("external ide", { t, isDesktop: false });
    assert.ok(hits.every((hit) => hit.settingId !== "settings.externalEditor"));
    assert.ok(searchSettingsEntries("external ide", { t, isDesktop: true }).some((hit) => hit.settingId === "settings.externalEditor"));

    const webZoomHits = searchSettingsEntries("zoom", { t, isDesktop: false });
    assert.ok(webZoomHits.every((hit) => hit.settingId !== "settings.windowZoom"));
    assert.ok(searchSettingsEntries("zoom", { t, isDesktop: true }).some((hit) => hit.settingId === "settings.windowZoom"));
  });

  it("keeps hostless searches relevant while retaining client defaults", () => {
    assert.deepEqual(searchSettingsEntries("lsp", { t, hasHost: false }), []);
    assert.equal(searchSettingsEntries("connect remote host", { t, hasHost: false })[0]?.target, "hosts");
    assert.equal(searchSettingsEntries("hermes", { t, hasHost: false })[0]?.target, "router");
    for (const hasHost of [true, false]) {
      assert.ok(searchSettingsEntries("default provider", { t, hasHost }).every((hit) => hit.id !== "im.defaultProvider"));
    }
  });
});
