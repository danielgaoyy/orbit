import type { MessageKey } from "../../lib/i18n/locales/en.ts";

export type HostSettingsTab =
  | "agents"
  | "guardian"
  | "im_channels"
  | "skills"
  | "acp"
  | "mcp"
  | "rules"
  | "hooks"
  | "lsp"
  | "usage"
  | "connection";

export type SettingsSection =
  | "general"
  | "instructions"
  | "cloud_sync"
  | "appearance"
  | "shortcuts"
  | "notifications"
  | "archived"
  | "add_host"
  | `host:${string}:${HostSettingsTab}`;

export type SettingsNavGroupId = "app" | "host" | "workspace";

export interface SettingsNavGroupSpec {
  id: SettingsNavGroupId;
  labelKey: MessageKey;
}

export const SETTINGS_NAV_GROUPS: readonly SettingsNavGroupSpec[] = [
  { id: "app", labelKey: "settings.navGroupApp" },
  { id: "host", labelKey: "settings.navGroupHost" },
  { id: "workspace", labelKey: "settings.navGroupWorkspace" },
];

export type SettingsCoreSectionId =
  | "general"
  | "appearance"
  | "notifications_sync"
  | "hosts"
  | "agents"
  | "usage"
  | "instructions_rules"
  | "capabilities";

export type SettingsSubTabId =
  | "general"
  | "shortcuts"
  | "connection"
  | "add_host"
  | "im_channels"
  | "agents"
  | "acp"
  | "guardian"
  | "usage"
  | "instructions"
  | "rules"
  | "skills"
  | "mcp"
  | "hooks"
  | "lsp"
  | "notifications"
  | "cloud_sync"
  | "archived";

export type SettingsLegacyAlias =
  | "hosts"
  | "providers"
  | "skills"
  | "models_agents"
  | "context"
  | "sync_data"
  | "channels"
  | "agents"
  | "extensions"
  | "instructions_rules"
  | "capabilities"
  | "notifications_sync"
  | "connection"
  | "acp"
  | "guardian"
  | "usage"
  | "rules"
  | "mcp"
  | "hooks"
  | "lsp"
  | "im_channels";

export type SettingsSectionTarget = SettingsSection | SettingsLegacyAlias;

export interface SettingsSubTabSpec {
  id: SettingsSubTabId;
  labelKey: MessageKey;
  requiresHost?: boolean;
  keywords: readonly string[];
}

export interface SettingsCoreSectionSpec {
  id: SettingsCoreSectionId;
  group: SettingsNavGroupId;
  labelKey: MessageKey;
  requiresHost?: boolean;
  keywords: readonly string[];
  subTabs: readonly SettingsSubTabSpec[];
}

export interface ResolvedSettingsLocation {
  coreSection: SettingsCoreSectionId;
  subTab: SettingsSubTabId | null;
  hostId: string | null;
  section: SettingsSection;
}

export interface SettingsSearchResult {
  id: string;
  coreSection: SettingsCoreSectionId;
  subTab: SettingsSubTabId | null;
  labelKey: MessageKey;
  parentLabelKey: MessageKey | null;
  requiresHost: boolean;
  score: number;
}

const VALID_HOST_TABS = new Set<string>([
  "agents",
  "guardian",
  "im_channels",
  "skills",
  "acp",
  "mcp",
  "rules",
  "hooks",
  "lsp",
  "usage",
  "connection",
]);

const DIRECT_NON_HOST_SECTIONS = new Set<string>([
  "general",
  "instructions",
  "cloud_sync",
  "appearance",
  "shortcuts",
  "notifications",
  "archived",
  "add_host",
]);

export const SETTINGS_CORE_SECTIONS: readonly SettingsCoreSectionSpec[] = [
  // ── APP ────────────────────────────────────────────────────────────────────
  {
    id: "general",
    group: "app",
    labelKey: "settings.navGeneral",
    keywords: [
      "general",
      "preferences",
      "workspace",
      "enter",
      "editor",
      "worktree",
      "branch",
      "update",
      "version",
      "fleet",
      "setup",
      "guide",
      "onboarding",
      "shortcuts",
      "keybindings",
      "hotkeys",
      "通用",
      "常规",
      "更新",
      "向导",
      "快捷键",
      "一般",
      "ガイド",
      "ショートカット",
    ],
    subTabs: [
      {
        id: "general",
        labelKey: "settings.subTabGeneral",
        keywords: [
          "general",
          "preferences",
          "composer",
          "enter",
          "permissions",
          "editor",
          "worktree",
          "git",
          "branch",
          "update",
          "version",
          "fleet",
          "setup",
          "guide",
          "onboarding",
          "通用",
          "权限",
          "编辑器",
          "分支",
          "更新",
          "版本",
          "向导",
          "一般",
          "アップデート",
          "ガイド",
        ],
      },
      {
        id: "shortcuts",
        labelKey: "settings.subTabShortcuts",
        keywords: [
          "shortcuts",
          "keyboard",
          "keybindings",
          "hotkeys",
          "cmd",
          "ctrl",
          "快捷键",
          "键盘",
          "ショートカット",
          "キーボード",
        ],
      },
    ],
  },
  {
    id: "appearance",
    group: "app",
    labelKey: "settings.navAppearance",
    keywords: [
      "appearance",
      "theme",
      "dark",
      "light",
      "system",
      "language",
      "locale",
      "i18n",
      "english",
      "chinese",
      "japanese",
      "font",
      "typography",
      "density",
      "diff",
      "split",
      "unified",
      "terminal",
      "palette",
      "contrast",
      "cursor",
      "scrollback",
      "外观",
      "主题",
      "深色",
      "浅色",
      "语言",
      "字号",
      "终端",
      "配色",
      "外観",
      "テーマ",
      "言語",
      "ターミナル",
    ],
    subTabs: [],
  },
  {
    id: "notifications_sync",
    group: "app",
    labelKey: "settings.navNotificationsSync",
    keywords: [
      "notifications",
      "push",
      "apns",
      "fcm",
      "harmony",
      "quiet hours",
      "sound",
      "alert",
      "cloud",
      "sync",
      "github",
      "gist",
      "backup",
      "archived",
      "archive",
      "history",
      "data",
      "通知",
      "推送",
      "提示音",
      "云同步",
      "同步",
      "归档",
      "アーカイブ",
      "同期",
    ],
    subTabs: [
      {
        id: "notifications",
        labelKey: "settings.subTabNotifications",
        keywords: [
          "notifications",
          "push",
          "desktop",
          "mobile",
          "apns",
          "fcm",
          "harmony",
          "quiet hours",
          "sound",
          "chime",
          "alert",
          "通知",
          "推送",
          "免打扰",
          "提示音",
          "プッシュ",
          "サウンド",
        ],
      },
      {
        id: "cloud_sync",
        labelKey: "settings.subTabCloudSync",
        keywords: [
          "cloud",
          "sync",
          "github",
          "gist",
          "oauth",
          "pat",
          "e2ee",
          "backup",
          "passphrase",
          "云同步",
          "同步",
          "备份",
          "クラウド同期",
          "同期",
        ],
      },
      {
        id: "archived",
        labelKey: "settings.subTabArchived",
        keywords: [
          "archived",
          "archive",
          "threads",
          "conversations",
          "restore",
          "delete",
          "history",
          "已归档",
          "归档",
          "恢复",
          "历史",
          "アーカイブ",
          "復元",
        ],
      },
    ],
  },

  // ── HOST ───────────────────────────────────────────────────────────────────
  {
    id: "hosts",
    group: "host",
    labelKey: "settings.navHosts",
    keywords: [
      "hosts",
      "pairing",
      "connection",
      "remote",
      "ssh",
      "relay",
      "daemon",
      "qr",
      "mobile",
      "add host",
      "im",
      "channels",
      "lark",
      "feishu",
      "slack",
      "telegram",
      "discord",
      "bot",
      "主机",
      "配对",
      "连接",
      "远程",
      "二维码",
      "中继",
      "频道",
      "飞书",
      "ホスト",
      "ペアリング",
      "接続",
      "チャンネル",
    ],
    subTabs: [
      {
        id: "connection",
        labelKey: "settings.subTabConnectionPairing",
        requiresHost: true,
        keywords: [
          "connection",
          "pairing",
          "host",
          "hosts",
          "status",
          "qr",
          "mobile",
          "uri",
          "relay",
          "e2ee",
          "noise",
          "daemon",
          "service",
          "restart",
          "连接",
          "配对",
          "主机",
          "二维码",
          "中继",
          "服务",
          "接続",
          "ペアリング",
          "リレー",
        ],
      },
      {
        id: "add_host",
        labelKey: "settings.subTabConnectRemoteHost",
        keywords: [
          "add host",
          "connect remote host",
          "remote",
          "ssh",
          "pair",
          "server",
          "workstation",
          "uri",
          "添加主机",
          "连接远程主机",
          "远程",
          "服务器",
          "リモートホスト",
          "ホストを追加",
        ],
      },
      {
        id: "im_channels",
        labelKey: "settings.subTabImChannels",
        requiresHost: true,
        keywords: [
          "im",
          "channels",
          "chat",
          "lark",
          "feishu",
          "slack",
          "telegram",
          "discord",
          "bot",
          "webhook",
          "approval card",
          "频道",
          "飞书",
          "机器人",
          "チャンネル",
          "ボット",
        ],
      },
    ],
  },
  {
    id: "agents",
    group: "host",
    labelKey: "settings.navAgents",
    requiresHost: true,
    keywords: [
      "agents",
      "models",
      "providers",
      "runtime",
      "claude",
      "codex",
      "antigravity",
      "gemini",
      "cursor",
      "copilot",
      "acp",
      "registry",
      "profiles",
      "fast",
      "keep awake",
      "guardian",
      "auto",
      "approval",
      "safety",
      "risk",
      "policy",
      "代理",
      "模型",
      "提供商",
      "审批",
      "风控",
      "エージェント",
      "モデル",
      "プロバイダ",
    ],
    subTabs: [
      {
        id: "agents",
        labelKey: "settings.subTabProvidersRuntime",
        requiresHost: true,
        keywords: [
          "providers",
          "runtime",
          "models",
          "agents",
          "claude",
          "codex",
          "antigravity",
          "gemini",
          "profiles",
          "orchestrator",
          "keep awake",
          "idle",
          "fast",
          "提供商",
          "运行时",
          "模型",
          "代理",
          "预设",
          "プロバイダ",
          "ランタイム",
          "モデル",
        ],
      },
      {
        id: "acp",
        labelKey: "settings.subTabAcpRegistry",
        requiresHost: true,
        keywords: [
          "acp",
          "registry",
          "agent client protocol",
          "custom agent",
          "external agent",
          "注册表",
          "レジストリ",
        ],
      },
      {
        id: "guardian",
        labelKey: "settings.subTabGuardian",
        requiresHost: true,
        keywords: [
          "guardian",
          "auto",
          "approval",
          "safety",
          "risk",
          "policy",
          "security",
          "blocked",
          "trusted",
          "自动审批",
          "风险",
          "安全",
          "策略",
          "自動承認",
          "リスク",
          "セキュリティ",
        ],
      },
    ],
  },
  {
    id: "usage",
    group: "host",
    labelKey: "settings.navUsage",
    requiresHost: true,
    keywords: [
      "usage",
      "tokens",
      "cost",
      "spend",
      "quotas",
      "insights",
      "analytics",
      "forecast",
      "cache",
      "billing",
      "budget",
      "burn rate",
      "用量",
      "配额",
      "统计",
      "费用",
      "使用量",
      "トークン",
      "クォータ",
    ],
    subTabs: [],
  },

  // ── WORKSPACE ──────────────────────────────────────────────────────────────
  {
    id: "instructions_rules",
    group: "workspace",
    labelKey: "settings.navInstructionsRules",
    keywords: [
      "workspace",
      "context",
      "instructions",
      "claude.md",
      "agents.md",
      "rules",
      "glob",
      "path",
      "memory",
      "prompt",
      "工作区",
      "上下文",
      "项目指令",
      "指令",
      "规则",
      "路径规则",
      "ワークスペース",
      "指示",
      "ルール",
    ],
    subTabs: [
      {
        id: "instructions",
        labelKey: "settings.subTabInstructions",
        keywords: [
          "instructions",
          "claude.md",
          "agents.md",
          "imports",
          "memory",
          "context",
          "prompt",
          "项目指令",
          "指令",
          "上下文",
          "指示",
          "コンテキスト",
        ],
      },
      {
        id: "rules",
        labelKey: "settings.subTabRules",
        requiresHost: true,
        keywords: [
          "rules",
          "glob",
          "path",
          "scoped",
          "context",
          "规则",
          "路径规则",
          "ルール",
        ],
      },
    ],
  },
  {
    id: "capabilities",
    group: "workspace",
    labelKey: "settings.navCapabilities",
    requiresHost: true,
    keywords: [
      "capabilities",
      "extensions",
      "skills",
      "skill.md",
      "plugins",
      "mcp",
      "servers",
      "hooks",
      "lifecycle",
      "lsp",
      "language servers",
      "扩展",
      "能力",
      "技能",
      "钩子",
      "语言服务器",
      "拡張",
      "スキル",
      "フック",
      "言語サーバー",
    ],
    subTabs: [
      {
        id: "skills",
        labelKey: "settings.subTabSkills",
        requiresHost: true,
        keywords: [
          "skills",
          "skill.md",
          "capabilities",
          "plugins",
          "技能",
          "スキル",
        ],
      },
      {
        id: "mcp",
        labelKey: "settings.subTabMcp",
        requiresHost: true,
        keywords: [
          "mcp",
          "mcp servers",
          "model context protocol",
          "tools",
          "servers",
          "extensions",
          "服务",
          "MCP 服务",
          "MCP サーバー",
        ],
      },
      {
        id: "hooks",
        labelKey: "settings.subTabHooks",
        requiresHost: true,
        keywords: [
          "hooks",
          "lifecycle",
          "pretooluse",
          "posttooluse",
          "automation",
          "scripts",
          "钩子",
          "生命周期钩子",
          "フック",
        ],
      },
      {
        id: "lsp",
        labelKey: "settings.subTabLsp",
        requiresHost: true,
        keywords: [
          "lsp",
          "language servers",
          "diagnostics",
          "symbols",
          "definition",
          "rust-analyzer",
          "typescript",
          "语言服务器",
          "診断",
          "言語サーバー",
        ],
      },
    ],
  },
];

export function parseHostSettingsSection(
  section: string,
): { hostId: string; tab: HostSettingsTab } | null {
  if (!section.startsWith("host:")) return null;
  const lastColon = section.lastIndexOf(":");
  if (lastColon <= 5) return null;
  const hostId = section.slice(5, lastColon);
  if (!hostId) return null;
  const tab = section.slice(lastColon + 1);
  if (!VALID_HOST_TABS.has(tab)) return null;
  return { hostId, tab: tab as HostSettingsTab };
}

export function resolveSettingsSectionTarget(
  target: SettingsSectionTarget | string | undefined,
  defaultHostId: string | null,
  fallbackSection: SettingsSection = "general",
): SettingsSection {
  if (!target) return fallbackSection;

  if (DIRECT_NON_HOST_SECTIONS.has(target)) {
    return target as SettingsSection;
  }

  const parsedHost = parseHostSettingsSection(target);
  if (parsedHost) {
    return `host:${parsedHost.hostId}:${parsedHost.tab}`;
  }

  switch (target) {
    case "hosts":
    case "connection":
      return defaultHostId ? `host:${defaultHostId}:connection` : "add_host";
    case "channels":
    case "im_channels":
      return defaultHostId ? `host:${defaultHostId}:im_channels` : "add_host";
    case "agents":
    case "providers":
    case "models_agents":
      return defaultHostId ? `host:${defaultHostId}:agents` : "add_host";
    case "acp":
      return defaultHostId ? `host:${defaultHostId}:acp` : "add_host";
    case "guardian":
      return defaultHostId ? `host:${defaultHostId}:guardian` : "add_host";
    case "usage":
      return defaultHostId ? `host:${defaultHostId}:usage` : "add_host";
    case "extensions":
    case "context":
    case "instructions_rules":
      return "instructions";
    case "capabilities":
      return defaultHostId ? `host:${defaultHostId}:skills` : "instructions";
    case "rules":
      return defaultHostId ? `host:${defaultHostId}:rules` : "instructions";
    case "skills":
      return defaultHostId ? `host:${defaultHostId}:skills` : "instructions";
    case "mcp":
      return defaultHostId ? `host:${defaultHostId}:mcp` : "instructions";
    case "hooks":
      return defaultHostId ? `host:${defaultHostId}:hooks` : "instructions";
    case "lsp":
      return defaultHostId ? `host:${defaultHostId}:lsp` : "instructions";
    case "notifications_sync":
      return "notifications";
    case "sync_data":
      return "cloud_sync";
    default:
      return fallbackSection;
  }
}

export function resolveActiveSettingsLocation(
  section: SettingsSection,
): ResolvedSettingsLocation {
  const parsedHost = parseHostSettingsSection(section);
  if (parsedHost) {
    switch (parsedHost.tab) {
      case "connection":
        return {
          coreSection: "hosts",
          subTab: "connection",
          hostId: parsedHost.hostId,
          section,
        };
      case "im_channels":
        return {
          coreSection: "hosts",
          subTab: "im_channels",
          hostId: parsedHost.hostId,
          section,
        };
      case "agents":
      case "acp":
      case "guardian":
        return {
          coreSection: "agents",
          subTab: parsedHost.tab,
          hostId: parsedHost.hostId,
          section,
        };
      case "usage":
        return {
          coreSection: "usage",
          subTab: null,
          hostId: parsedHost.hostId,
          section,
        };
      case "rules":
        return {
          coreSection: "instructions_rules",
          subTab: "rules",
          hostId: parsedHost.hostId,
          section,
        };
      case "skills":
      case "mcp":
      case "hooks":
      case "lsp":
        return {
          coreSection: "capabilities",
          subTab: parsedHost.tab,
          hostId: parsedHost.hostId,
          section,
        };
    }
  }

  switch (section) {
    case "general":
      return { coreSection: "general", subTab: "general", hostId: null, section };
    case "shortcuts":
      return { coreSection: "general", subTab: "shortcuts", hostId: null, section };
    case "appearance":
      return { coreSection: "appearance", subTab: null, hostId: null, section };
    case "add_host":
      return { coreSection: "hosts", subTab: "add_host", hostId: null, section };
    case "instructions":
      return {
        coreSection: "instructions_rules",
        subTab: "instructions",
        hostId: null,
        section,
      };
    case "notifications":
      return {
        coreSection: "notifications_sync",
        subTab: "notifications",
        hostId: null,
        section,
      };
    case "cloud_sync":
      return {
        coreSection: "notifications_sync",
        subTab: "cloud_sync",
        hostId: null,
        section,
      };
    case "archived":
      return {
        coreSection: "notifications_sync",
        subTab: "archived",
        hostId: null,
        section,
      };
    default:
      return { coreSection: "general", subTab: "general", hostId: null, section: "general" };
  }
}

export function buildSettingsSectionForTarget(
  coreSection: SettingsCoreSectionId,
  subTab: SettingsSubTabId | null,
  hostId: string | null,
): SettingsSection {
  if (subTab) {
    switch (subTab) {
      case "general":
        return "general";
      case "shortcuts":
        return "shortcuts";
      case "add_host":
        return "add_host";
      case "instructions":
        return "instructions";
      case "notifications":
        return "notifications";
      case "cloud_sync":
        return "cloud_sync";
      case "archived":
        return "archived";
      case "connection":
      case "im_channels":
      case "agents":
      case "acp":
      case "guardian":
      case "usage":
        return hostId ? `host:${hostId}:${subTab}` : "add_host";
      case "rules":
      case "skills":
      case "mcp":
      case "hooks":
      case "lsp":
        return hostId ? `host:${hostId}:${subTab}` : "instructions";
    }
  }

  switch (coreSection) {
    case "general":
      return "general";
    case "appearance":
      return "appearance";
    case "notifications_sync":
      return "notifications";
    case "hosts":
      return hostId ? `host:${hostId}:connection` : "add_host";
    case "agents":
      return hostId ? `host:${hostId}:agents` : "add_host";
    case "usage":
      return hostId ? `host:${hostId}:usage` : "add_host";
    case "instructions_rules":
      return "instructions";
    case "capabilities":
      return hostId ? `host:${hostId}:skills` : "instructions";
  }
}

export function getVisibleSubTabsForSection(
  coreSection: SettingsCoreSectionId,
  hasHost: boolean,
): readonly SettingsSubTabSpec[] {
  const spec = SETTINGS_CORE_SECTIONS.find((s) => s.id === coreSection);
  if (!spec || spec.subTabs.length === 0) return [];
  return spec.subTabs.filter((tab) => !tab.requiresHost || hasHost);
}

function scoreMatch(
  query: string,
  primaryLabel: string,
  parentLabel: string | null,
  keywords: readonly string[],
): number {
  const primary = primaryLabel.toLowerCase();
  if (primary === query) return 100;
  if (primary.startsWith(query)) return 85;
  if (primary.includes(query)) return 70;

  let bestKeyword = 0;
  for (const kw of keywords) {
    const lower = kw.toLowerCase();
    if (lower === query) {
      bestKeyword = Math.max(bestKeyword, 65);
    } else if (lower.startsWith(query)) {
      bestKeyword = Math.max(bestKeyword, 55);
    } else if (lower.includes(query)) {
      bestKeyword = Math.max(bestKeyword, 40);
    }
  }

  let parentScore = 0;
  if (parentLabel) {
    const parent = parentLabel.toLowerCase();
    if (parent === query) parentScore = 50;
    else if (parent.startsWith(query)) parentScore = 45;
    else if (parent.includes(query)) parentScore = 30;
  }

  return Math.max(bestKeyword, parentScore);
}

export function searchSettingsEntries(
  rawQuery: string,
  options: {
    t: (key: MessageKey) => string;
    hasHost?: boolean;
  },
): SettingsSearchResult[] {
  const query = rawQuery.trim().toLowerCase();
  if (!query) return [];

  const hasHost = options.hasHost ?? true;
  const results: SettingsSearchResult[] = [];

  for (const section of SETTINGS_CORE_SECTIONS) {
    const sectionLabel = options.t(section.labelKey);

    if (section.subTabs.length === 0) {
      if (section.requiresHost && !hasHost) continue;
      const score = scoreMatch(query, sectionLabel, null, section.keywords);
      if (score > 0) {
        results.push({
          id: section.id,
          coreSection: section.id,
          subTab: null,
          labelKey: section.labelKey,
          parentLabelKey: null,
          requiresHost: Boolean(section.requiresHost),
          score,
        });
      }
      continue;
    }

    for (const tab of section.subTabs) {
      const requiresHost = Boolean(tab.requiresHost || section.requiresHost);
      if (requiresHost && !hasHost) continue;

      const tabLabel = options.t(tab.labelKey);
      const showParent = tabLabel.toLowerCase() !== sectionLabel.toLowerCase();
      const score = scoreMatch(
        query,
        tabLabel,
        showParent ? sectionLabel : null,
        tab.keywords,
      );
      if (score > 0) {
        results.push({
          id: `${section.id}:${tab.id}`,
          coreSection: section.id,
          subTab: tab.id,
          labelKey: tab.labelKey,
          parentLabelKey: showParent ? section.labelKey : null,
          requiresHost,
          score,
        });
      }
    }
  }

  results.sort((a, b) => b.score - a.score);
  return results;
}
