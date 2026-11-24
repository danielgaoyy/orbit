import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { en, type MessageDictionary, type MessageKey } from "../../lib/i18n/locales/en.ts";
import { ja } from "../../lib/i18n/locales/ja.ts";
import { zhCN } from "../../lib/i18n/locales/zh-CN.ts";
import {
  SETTINGS_CORE_SECTIONS,
  SETTINGS_NAV_GROUPS,
  type SettingsSection,
  buildSettingsSectionForTarget,
  getVisibleSubTabsForSection,
  parseHostSettingsSection,
  resolveActiveSettingsLocation,
  resolveSettingsSectionTarget,
  searchSettingsEntries,
} from "./settings-nav.ts";

function makeTranslator(dict: MessageDictionary): (key: MessageKey) => string {
  return (key) => dict[key];
}

describe("Settings IA (3 Domain Groups · 9 Core Sections · 20 Leaf Views)", () => {
  it("organizes all 20 settings views cleanly across APP, HOST, and WORKSPACE groups", () => {
    assert.deepEqual(
      SETTINGS_NAV_GROUPS.map((g) => g.id),
      ["app", "host", "workspace"],
    );

    assert.deepEqual(
      SETTINGS_CORE_SECTIONS.map((s) => `${s.group}:${s.id}`),
      [
        "app:general",
        "app:appearance",
        "app:notifications_sync",
        "app:personal_router",
        "host:hosts",
        "host:agents",
        "host:usage",
        "workspace:instructions_rules",
        "workspace:capabilities",
      ],
    );

    const totalLeafViews = SETTINGS_CORE_SECTIONS.reduce(
      (sum, section) => sum + Math.max(1, section.subTabs.length),
      0,
    );
    assert.equal(totalLeafViews, 20);

    assert.deepEqual(
      getVisibleSubTabsForSection("general", true).map((t) => t.id),
      ["general", "shortcuts"],
    );
    assert.deepEqual(getVisibleSubTabsForSection("appearance", true), []);
    assert.deepEqual(
      getVisibleSubTabsForSection("notifications_sync", true).map((t) => t.id),
      ["notifications", "cloud_sync", "archived"],
    );
    assert.deepEqual(getVisibleSubTabsForSection("personal_router", true), []);
    assert.deepEqual(
      getVisibleSubTabsForSection("hosts", true).map((t) => t.id),
      ["connection", "add_host", "im_channels"],
    );
    assert.deepEqual(
      getVisibleSubTabsForSection("agents", true).map((t) => t.id),
      ["agents", "acp", "guardian"],
    );
    assert.deepEqual(getVisibleSubTabsForSection("usage", true), []);
    assert.deepEqual(
      getVisibleSubTabsForSection("instructions_rules", true).map((t) => t.id),
      ["instructions", "rules"],
    );
    assert.deepEqual(
      getVisibleSubTabsForSection("capabilities", true).map((t) => t.id),
      ["skills", "mcp", "hooks", "lsp"],
    );

    // Without a host, host-only sub-tabs gracefully filter out while non-host views stay accessible
    assert.deepEqual(
      getVisibleSubTabsForSection("hosts", false).map((t) => t.id),
      ["add_host"],
    );
    assert.deepEqual(
      getVisibleSubTabsForSection("instructions_rules", false).map((t) => t.id),
      ["instructions"],
    );
    assert.deepEqual(getVisibleSubTabsForSection("capabilities", false), []);
  });

  it("resolves every legacy section ID and deep-link to the exact grouped section and sub-tab", () => {
    const hostId = "local-mac";

    const cases: Array<{
      target: string;
      expectedSection: SettingsSection;
      expectedCore: string;
      expectedSubTab: string | null;
    }> = [
      { target: "general", expectedSection: "general", expectedCore: "general", expectedSubTab: "general" },
      { target: "shortcuts", expectedSection: "shortcuts", expectedCore: "general", expectedSubTab: "shortcuts" },
      { target: "appearance", expectedSection: "appearance", expectedCore: "appearance", expectedSubTab: null },
      { target: "hosts", expectedSection: `host:${hostId}:connection`, expectedCore: "hosts", expectedSubTab: "connection" },
      { target: "connection", expectedSection: `host:${hostId}:connection`, expectedCore: "hosts", expectedSubTab: "connection" },
      { target: "add_host", expectedSection: "add_host", expectedCore: "hosts", expectedSubTab: "add_host" },
      { target: "channels", expectedSection: `host:${hostId}:im_channels`, expectedCore: "hosts", expectedSubTab: "im_channels" },
      { target: "im_channels", expectedSection: `host:${hostId}:im_channels`, expectedCore: "hosts", expectedSubTab: "im_channels" },
      { target: "providers", expectedSection: `host:${hostId}:agents`, expectedCore: "agents", expectedSubTab: "agents" },
      { target: "models_agents", expectedSection: `host:${hostId}:agents`, expectedCore: "agents", expectedSubTab: "agents" },
      { target: "agents", expectedSection: `host:${hostId}:agents`, expectedCore: "agents", expectedSubTab: "agents" },
      { target: "router", expectedSection: "personal_router", expectedCore: "personal_router", expectedSubTab: null },
      { target: "host:devbox-1:router", expectedSection: "personal_router", expectedCore: "personal_router", expectedSubTab: null },
      { target: "personal_router", expectedSection: "personal_router", expectedCore: "personal_router", expectedSubTab: null },
      { target: "acp", expectedSection: `host:${hostId}:acp`, expectedCore: "agents", expectedSubTab: "acp" },
      { target: "guardian", expectedSection: `host:${hostId}:guardian`, expectedCore: "agents", expectedSubTab: "guardian" },
      { target: "usage", expectedSection: `host:${hostId}:usage`, expectedCore: "usage", expectedSubTab: null },
      { target: "extensions", expectedSection: "instructions", expectedCore: "instructions_rules", expectedSubTab: "instructions" },
      { target: "context", expectedSection: "instructions", expectedCore: "instructions_rules", expectedSubTab: "instructions" },
      { target: "instructions_rules", expectedSection: "instructions", expectedCore: "instructions_rules", expectedSubTab: "instructions" },
      { target: "instructions", expectedSection: "instructions", expectedCore: "instructions_rules", expectedSubTab: "instructions" },
      { target: "rules", expectedSection: `host:${hostId}:rules`, expectedCore: "instructions_rules", expectedSubTab: "rules" },
      { target: "capabilities", expectedSection: `host:${hostId}:skills`, expectedCore: "capabilities", expectedSubTab: "skills" },
      { target: "skills", expectedSection: `host:${hostId}:skills`, expectedCore: "capabilities", expectedSubTab: "skills" },
      { target: "mcp", expectedSection: `host:${hostId}:mcp`, expectedCore: "capabilities", expectedSubTab: "mcp" },
      { target: "hooks", expectedSection: `host:${hostId}:hooks`, expectedCore: "capabilities", expectedSubTab: "hooks" },
      { target: "lsp", expectedSection: `host:${hostId}:lsp`, expectedCore: "capabilities", expectedSubTab: "lsp" },
      { target: "notifications", expectedSection: "notifications", expectedCore: "notifications_sync", expectedSubTab: "notifications" },
      { target: "notifications_sync", expectedSection: "notifications", expectedCore: "notifications_sync", expectedSubTab: "notifications" },
      { target: "sync_data", expectedSection: "cloud_sync", expectedCore: "notifications_sync", expectedSubTab: "cloud_sync" },
      { target: "cloud_sync", expectedSection: "cloud_sync", expectedCore: "notifications_sync", expectedSubTab: "cloud_sync" },
      { target: "archived", expectedSection: "archived", expectedCore: "notifications_sync", expectedSubTab: "archived" },
      { target: "host:devbox-1:acp", expectedSection: "host:devbox-1:acp", expectedCore: "agents", expectedSubTab: "acp" },
      { target: "host:devbox-1:usage", expectedSection: "host:devbox-1:usage", expectedCore: "usage", expectedSubTab: null },
      { target: "host:devbox-1:mcp", expectedSection: "host:devbox-1:mcp", expectedCore: "capabilities", expectedSubTab: "mcp" },
      { target: "host:devbox-1:im_channels", expectedSection: "host:devbox-1:im_channels", expectedCore: "hosts", expectedSubTab: "im_channels" },
    ];

    for (const c of cases) {
      const resolved = resolveSettingsSectionTarget(c.target, hostId, "general");
      assert.equal(resolved, c.expectedSection, `target=${c.target}`);
      const loc = resolveActiveSettingsLocation(resolved);
      assert.equal(loc.coreSection, c.expectedCore, `core for target=${c.target}`);
      assert.equal(loc.subTab, c.expectedSubTab, `subTab for target=${c.target}`);
    }
  });

  it("handles hostless fallbacks, host IDs containing colons, and unknown targets safely", () => {
    assert.equal(resolveSettingsSectionTarget("hosts", null, "general"), "add_host");
    assert.equal(resolveSettingsSectionTarget("providers", null, "general"), "add_host");
    assert.equal(resolveSettingsSectionTarget("usage", null, "general"), "add_host");
    assert.equal(resolveSettingsSectionTarget("skills", null, "general"), "instructions");
    assert.equal(resolveSettingsSectionTarget("mcp", null, "general"), "instructions");
    assert.equal(resolveSettingsSectionTarget("nonexistent_legacy_id", "h1", "appearance"), "appearance");
    assert.equal(resolveSettingsSectionTarget(undefined, "h1", "shortcuts"), "shortcuts");

    const colonHost = parseHostSettingsSection("host:ipv6:2001:db8::1:mcp");
    assert.deepEqual(colonHost, { hostId: "ipv6:2001:db8::1", tab: "mcp" });
    assert.equal(parseHostSettingsSection("host:h1:invalid_tab"), null);
    assert.equal(parseHostSettingsSection("host::mcp"), null);

    assert.equal(buildSettingsSectionForTarget("hosts", "im_channels", "h2"), "host:h2:im_channels");
    assert.equal(buildSettingsSectionForTarget("hosts", "im_channels", null), "add_host");
    assert.equal(buildSettingsSectionForTarget("usage", null, "h2"), "host:h2:usage");
    assert.equal(buildSettingsSectionForTarget("usage", null, null), "add_host");
    assert.equal(buildSettingsSectionForTarget("capabilities", "lsp", null), "instructions");
    assert.equal(buildSettingsSectionForTarget("capabilities", null, "h2"), "host:h2:skills");
    assert.equal(buildSettingsSectionForTarget("notifications_sync", null, "h2"), "notifications");
  });

  it("filters direct-jump results accurately across en, zh-CN, and ja localized labels and keywords", () => {
    const tEn = makeTranslator(en);
    const tZh = makeTranslator(zhCN);
    const tJa = makeTranslator(ja);

    // Empty / whitespace query returns no search overlay items
    assert.deepEqual(searchSettingsEntries("   ", { t: tEn }), []);

    // Exact / primary sub-tab hits rank #1
    const mcpHits = searchSettingsEntries("mcp", { t: tEn });
    assert.equal(mcpHits[0]?.coreSection, "capabilities");
    assert.equal(mcpHits[0]?.subTab, "mcp");
    assert.equal(mcpHits[0]?.parentLabelKey, "settings.navCapabilities");

    const shortcutsHits = searchSettingsEntries("shortcuts", { t: tEn });
    assert.equal(shortcutsHits[0]?.coreSection, "general");
    assert.equal(shortcutsHits[0]?.subTab, "shortcuts");

    // Cross-keyword matches (Lark -> IM Channels inside Hosts & Pairing; CLAUDE.md -> Instructions)
    const larkHits = searchSettingsEntries("lark", { t: tEn });
    assert.equal(larkHits[0]?.coreSection, "hosts");
    assert.equal(larkHits[0]?.subTab, "im_channels");

    const claudeMdHits = searchSettingsEntries("CLAUDE.md", { t: tEn });
    assert.equal(claudeMdHits[0]?.coreSection, "instructions_rules");
    assert.equal(claudeMdHits[0]?.subTab, "instructions");

    // Promoted top-level Usage & Quotas
    const usageHits = searchSettingsEntries("quotas", { t: tEn });
    assert.equal(usageHits[0]?.coreSection, "usage");
    assert.equal(usageHits[0]?.subTab, null);
    assert.equal(usageHits[0]?.parentLabelKey, null);

    // Chinese localized labels & keywords
    const zhShortcuts = searchSettingsEntries("快捷键", { t: tZh });
    assert.equal(zhShortcuts[0]?.coreSection, "general");
    assert.equal(zhShortcuts[0]?.subTab, "shortcuts");

    const zhArchived = searchSettingsEntries("归档", { t: tZh });
    assert.equal(zhArchived[0]?.coreSection, "notifications_sync");
    assert.equal(zhArchived[0]?.subTab, "archived");

    const zhLark = searchSettingsEntries("飞书", { t: tZh });
    assert.equal(zhLark[0]?.coreSection, "hosts");
    assert.equal(zhLark[0]?.subTab, "im_channels");

    // Japanese localized labels & keywords
    const jaAppearance = searchSettingsEntries("外観", { t: tJa });
    assert.equal(jaAppearance[0]?.coreSection, "appearance");
    assert.equal(jaAppearance[0]?.subTab, null);
    assert.equal(jaAppearance[0]?.parentLabelKey, null);

    const jaShortcuts = searchSettingsEntries("ショートカット", { t: tJa });
    assert.equal(jaShortcuts[0]?.coreSection, "general");
    assert.equal(jaShortcuts[0]?.subTab, "shortcuts");

    const jaUsage = searchSettingsEntries("使用量", { t: tJa });
    assert.equal(jaUsage[0]?.coreSection, "usage");
    assert.equal(jaUsage[0]?.subTab, null);

    // Hostless mode omits host-only sections & sub-tabs cleanly while keeping APP-level Personal Agent accessible
    assert.deepEqual(searchSettingsEntries("mcp", { t: tEn, hasHost: false }), []);
    assert.deepEqual(searchSettingsEntries("quotas", { t: tEn, hasHost: false }), []);
    const hostlessRouter = searchSettingsEntries("hermes", { t: tEn, hasHost: false });
    assert.equal(hostlessRouter[0]?.coreSection, "personal_router");
    assert.equal(hostlessRouter[0]?.subTab, null);
    const hostlessAdd = searchSettingsEntries("ssh", { t: tEn, hasHost: false });
    assert.equal(hostlessAdd[0]?.coreSection, "hosts");
    assert.equal(hostlessAdd[0]?.subTab, "add_host");

    // Non-matching query returns empty list
    assert.deepEqual(searchSettingsEntries("zzzz-no-such-setting", { t: tEn }), []);
  });
});
