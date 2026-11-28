import { resolveEffectiveOrbitLanguage } from '../../model/OrbitDeviceBridge';
import {
  ExternalMcpServerRecordDto,
  InstalledPluginDto,
  ProjectMemoryItemDto,
  nonBlankPath
} from '../../model/OrbitTypes';
import { orbitNormalizeThemeMode } from '../../model/OrbitWindowTheme';

export type OrbitSettingsGroupId =
  | 'preferences'
  | 'fleet_sync'
  | 'agents_safety'
  | 'context_extensions';

export type OrbitSettingsDestinationId =
  | 'general'
  | 'appearance'
  | 'notifications'
  | 'hosts'
  | 'host_detail'
  | 'pair_host'
  | 'cloud_sync'
  | 'personal_agent'
  | 'agents_models'
  | 'permissions_guardian'
  | 'usage_quotas'
  | 'mcp'
  | 'skills'
  | 'memory'
  | 'plugins'
  | 'sandbox';

export interface OrbitSettingsNavParam {
  hostId: string;
  focusKey: string;
}

export interface OrbitSettingsGroupSpec {
  id: OrbitSettingsGroupId;
  destinations: OrbitSettingsDestinationId[];
}

export interface OrbitSettingsSearchEntry {
  id: string;
  group: OrbitSettingsGroupId;
  destination: OrbitSettingsDestinationId;
  focusKey: string;
  requiresHost: boolean;
  titleEn: string;
  titleZh: string;
  titleJa: string;
  groupEn: string;
  groupZh: string;
  groupJa: string;
  keywords: string;
}

export interface OrbitSettingsSearchHit {
  entry: OrbitSettingsSearchEntry;
  score: number;
}

export const ORBIT_APPEARANCE_MODES: string[] = ['system', 'light', 'dark'];
export const ORBIT_LANGUAGE_MODES: string[] = ['system', 'en', 'zh_CN', 'ja_JP'];
export const ORBIT_ACTIVE_TURN_SEND_MODES: string[] = ['queue', 'steer', 'interrupt'];
export const ORBIT_ROUTE_MODES: string[] = ['auto', 'direct_only', 'relay_only'];
export const ORBIT_USAGE_PERIOD_DAYS: number[] = [7, 30];

export const ORBIT_SETTINGS_GROUPS: OrbitSettingsGroupSpec[] = [
  {
    id: 'preferences',
    destinations: ['general', 'appearance', 'notifications']
  },
  {
    id: 'fleet_sync',
    destinations: ['hosts', 'cloud_sync']
  },
  {
    id: 'agents_safety',
    destinations: ['personal_agent', 'agents_models', 'permissions_guardian', 'usage_quotas']
  },
  {
    id: 'context_extensions',
    destinations: ['mcp', 'skills', 'memory', 'plugins', 'sandbox']
  }
];

export function resolveOrbitAppearanceIndex(mode: string): number {
  const normalized = orbitNormalizeThemeMode(mode);
  if (normalized === 'light') {
    return 1;
  }
  if (normalized === 'dark') {
    return 2;
  }
  return 0;
}

export function migrateLegacyAppearanceMode(rawMode: string): string {
  return orbitNormalizeThemeMode(rawMode);
}

export function resolveOrbitSettingsInitialDestination(
  rawPath: string | undefined
): OrbitSettingsDestinationId | '' {
  if (rawPath === undefined) {
    return '';
  }
  const key = rawPath.trim().toLowerCase();
  if (key.length === 0) {
    return '';
  }
  if (key === 'general') {
    return 'general';
  }
  if (key === 'appearance' || key === 'theme') {
    return 'appearance';
  }
  if (key === 'notifications' || key === 'push_settings' || key === 'push') {
    return 'notifications';
  }
  if (key === 'hosts' || key === 'connection' || key === 'fleet') {
    return 'hosts';
  }
  if (key === 'host_detail') {
    return 'host_detail';
  }
  if (key === 'pair_host' || key === 'add_host') {
    return 'pair_host';
  }
  if (key === 'cloud_sync' || key === 'sync') {
    return 'cloud_sync';
  }
  if (
    key === 'personal_agent' ||
    key === 'router_mesh' ||
    key === 'router' ||
    key === 'mesh'
  ) {
    return 'personal_agent';
  }
  if (key === 'agents_models' || key === 'agents' || key === 'models' || key === 'acp') {
    return 'agents_models';
  }
  if (
    key === 'permissions_guardian' ||
    key === 'permissions' ||
    key === 'guardian' ||
    key === 'safety'
  ) {
    return 'permissions_guardian';
  }
  if (key === 'usage_quotas' || key === 'usage' || key === 'quotas') {
    return 'usage_quotas';
  }
  if (key === 'mcp') {
    return 'mcp';
  }
  if (key === 'skills') {
    return 'skills';
  }
  if (key === 'memory') {
    return 'memory';
  }
  if (key === 'plugins') {
    return 'plugins';
  }
  if (key === 'sandbox') {
    return 'sandbox';
  }
  return '';
}

/**
 * Canonical trilingual Settings search catalog.
 * Every root destination title matches the exact rendered `$r('app.string.*')` string across
 * EN (`base`), zh_CN, and ja_JP so searching any visible UI title hits its exact destination,
 * while preserving aliases, sub-items, and direct `host_detail` navigation.
 */
export const ORBIT_SETTINGS_SEARCH_CATALOG: OrbitSettingsSearchEntry[] = [
  // Group 1: Preferences
  {
    id: 'pref_general',
    group: 'preferences',
    destination: 'general',
    focusKey: 'general',
    requiresHost: false,
    titleEn: 'General',
    titleZh: '通用',
    titleJa: '一般',
    groupEn: 'Preferences',
    groupZh: '偏好设置',
    groupJa: '環境設定',
    keywords: 'general preferences language locale send behavior 通用 偏好 语言 默认发送 一般 言語 送信'
  },
  {
    id: 'pref_language',
    group: 'preferences',
    destination: 'general',
    focusKey: 'language',
    requiresHost: false,
    titleEn: 'Language (English / 中文 / 日本語)',
    titleZh: '界面语言（English / 中文 / 日本語）',
    titleJa: '表示言語（English / 中文 / 日本語）',
    groupEn: 'Preferences',
    groupZh: '偏好设置',
    groupJa: '環境設定',
    keywords: 'language locale i18n english chinese japanese zh_cn ja_jp 语言 简体中文 日语 言語 日本語'
  },
  {
    id: 'pref_send_behavior',
    group: 'preferences',
    destination: 'general',
    focusKey: 'send_behavior',
    requiresHost: false,
    titleEn: 'Default Send Behavior While Running',
    titleZh: '代理运行时的默认发送行为',
    titleJa: '実行中のデフォルト送信動作',
    groupEn: 'Preferences',
    groupZh: '偏好设置',
    groupJa: '環境設定',
    keywords: 'send behavior queue steer interrupt composer running 排队 引导 中断 发送行为 キュー ステア 中断 送信'
  },
  {
    id: 'pref_appearance',
    group: 'preferences',
    destination: 'appearance',
    focusKey: 'appearance',
    requiresHost: false,
    titleEn: 'Appearance',
    titleZh: '外观',
    titleJa: '外観',
    groupEn: 'Preferences',
    groupZh: '偏好设置',
    groupJa: '環境設定',
    keywords: 'appearance theme mode system light dark color scheme reduced motion 外观（跟随系统 / 浅色 / 深色） 主题 深色 浅色 跟随系统 减弱动态 外観（システム / ライト / ダーク） テーマ ダーク ライト'
  },
  {
    id: 'pref_notifications',
    group: 'preferences',
    destination: 'notifications',
    focusKey: 'notifications',
    requiresHost: false,
    titleEn: 'Notifications',
    titleZh: '通知',
    titleJa: '通知',
    groupEn: 'Preferences',
    groupZh: '偏好设置',
    groupJa: '環境設定',
    keywords: 'notifications push kit alert permission turn error goal test notification 通知与推送提醒 推送 测试通知 权限 提醒 通知とプッシュアラート 通知テスト アラート'
  },

  // Group 2: Fleet & Sync
  {
    id: 'fleet_hosts',
    group: 'fleet_sync',
    destination: 'hosts',
    focusKey: 'hosts',
    requiresHost: false,
    titleEn: 'Hosts',
    titleZh: '主机管理',
    titleJa: 'ホスト',
    groupEn: 'Fleet & Sync',
    groupZh: '主机与同步',
    groupJa: 'フリートと同期',
    keywords: 'hosts fleet relay direct route mode 主机、配对二维码与全局中继 主机 配对 中继 直连 ホスト・ペアリングQR・フリートリレー フリート リレー'
  },
  {
    id: 'fleet_host_detail',
    group: 'fleet_sync',
    destination: 'host_detail',
    focusKey: 'host_detail',
    requiresHost: true,
    titleEn: 'Host Details & Pairing QR',
    titleZh: '主机详情与配对二维码',
    titleJa: 'ホスト詳細とペアリングQR',
    groupEn: 'Fleet & Sync',
    groupZh: '主机与同步',
    groupJa: 'フリートと同期',
    keywords: 'host detail share pairing qr uri export daemon upgrade pubkey reconnect direct relay 主机详情 分享配对 二维码 导出 升级 守护进程 公钥 重连 ホスト詳細 ペアリングQR 共有 エクスポート アップグレード デーモン'
  },
  {
    id: 'fleet_pair_host',
    group: 'fleet_sync',
    destination: 'pair_host',
    focusKey: 'pair_host',
    requiresHost: false,
    titleEn: 'Pair New Host',
    titleZh: '配对新主机',
    titleJa: '新しいホストをペアリング',
    groupEn: 'Fleet & Sync',
    groupZh: '主机与同步',
    groupJa: 'フリートと同期',
    keywords: 'pair new host scan qr code paste uri orbit://pair 配对新主机（扫码 / 粘贴 URI） 添加主机 扫码配对 粘贴 新しいホストをペアリング（QR / URI） ホスト追加 スキャン'
  },
  {
    id: 'fleet_cloud_sync',
    group: 'fleet_sync',
    destination: 'cloud_sync',
    focusKey: 'cloud_sync',
    requiresHost: false,
    titleEn: 'Cloud Sync',
    titleZh: '云同步',
    titleJa: 'クラウド同期',
    groupEn: 'Fleet & Sync',
    groupZh: '主机与同步',
    groupJa: 'フリートと同期',
    keywords: 'cloud sync github gist e2ee vault device link qr passphrase auto sync token 云同步（端到端加密库） 密钥 口令 设备关联 クラウド同期（E2EEボールト） パスフレーズ'
  },

  // Group 3: Agents & Safety
  {
    id: 'agents_personal_agent',
    group: 'agents_safety',
    destination: 'personal_agent',
    focusKey: 'personal_agent',
    requiresHost: true,
    titleEn: 'Personal Agent & Mesh',
    titleZh: '个人代理与互联',
    titleJa: 'パーソナルエージェント＆メッシュ',
    groupEn: 'Agents & Safety',
    groupZh: '代理与安全',
    groupJa: 'エージェントと安全',
    keywords: 'personal agent mesh peers routing scope pre review multi host 个人助手与主机互联 个人助手 互联 节点 预审 パーソナルエージェントとメッシュ パーソナルエージェント メッシュ ピア'
  },
  {
    id: 'agents_models',
    group: 'agents_safety',
    destination: 'agents_models',
    focusKey: 'agents_models',
    requiresHost: true,
    titleEn: 'Agents & Models',
    titleZh: '代理与模型',
    titleJa: 'エージェントとモデル',
    groupEn: 'Agents & Safety',
    groupZh: '代理与安全',
    groupJa: 'エージェントと安全',
    keywords: 'agents models providers default effort claude codex antigravity pi acp registry custom test 代理、模型与 ACP 注册表 模型 提供商 推理深度 注册表 エージェント・モデル・ACPレジストリ プロバイダー レジストリ'
  },
  {
    id: 'agents_permissions_guardian',
    group: 'agents_safety',
    destination: 'permissions_guardian',
    focusKey: 'permissions_guardian',
    requiresHost: true,
    titleEn: 'Permissions & Guardian',
    titleZh: '权限与 Guardian',
    titleJa: '権限と Guardian',
    groupEn: 'Agents & Safety',
    groupZh: '代理与安全',
    groupJa: 'エージェントと安全',
    keywords: 'permissions guardian policy auto approve risk threshold rules allow deny safety 权限规则与 Guardian 策略 权限 审批 自动批准 风险阈值 安全策略 権限ルールとGuardianポリシー 権限 ガーディアン 承認 ルール'
  },
  {
    id: 'agents_usage_quotas',
    group: 'agents_safety',
    destination: 'usage_quotas',
    focusKey: 'usage_quotas',
    requiresHost: true,
    titleEn: 'Usage & Quotas',
    titleZh: '用量与配额',
    titleJa: '使用量とクォータ',
    groupEn: 'Agents & Safety',
    groupZh: '代理与安全',
    groupJa: 'エージェントと安全',
    keywords: 'usage insights quotas tokens cost 7d 30d forecast burn rate rate limit 用量洞察与配额 用量 费用 配额 预测 令牌 使用量インサイトとクォータ 使用量 トークン コスト 割当 クォータ'
  },

  // Group 4: Context & Extensions
  {
    id: 'ext_mcp',
    group: 'context_extensions',
    destination: 'mcp',
    focusKey: 'mcp',
    requiresHost: true,
    titleEn: 'MCP Servers',
    titleZh: 'MCP 服务器',
    titleJa: 'MCP サーバー',
    groupEn: 'Context & Extensions',
    groupZh: '上下文与扩展',
    groupJa: 'コンテキストと拡張',
    keywords: 'mcp model context protocol external servers stdio http toggle tools 工具 服务 启停 MCPサーバー サーバー'
  },
  {
    id: 'ext_skills',
    group: 'context_extensions',
    destination: 'skills',
    focusKey: 'skills',
    requiresHost: true,
    titleEn: 'Skills',
    titleZh: '技能',
    titleJa: 'スキル',
    groupEn: 'Context & Extensions',
    groupZh: '上下文与扩展',
    groupJa: 'コンテキストと拡張',
    keywords: 'skills skill.md workflows commands triggers read only 技能（Skills） 技能 扩展 命令 スキル（Skills） スキル コマンド'
  },
  {
    id: 'ext_memory',
    group: 'context_extensions',
    destination: 'memory',
    focusKey: 'memory',
    requiresHost: true,
    titleEn: 'Project Memory',
    titleZh: '项目记忆',
    titleJa: 'プロジェクトメモリ',
    groupEn: 'Context & Extensions',
    groupZh: '上下文与扩展',
    groupJa: 'コンテキストと拡張',
    keywords: 'project memory conventions architecture confirm delete hypothesis 项目记忆 确认 删除 プロジェクトメモリ 確認 削除'
  },
  {
    id: 'ext_plugins',
    group: 'context_extensions',
    destination: 'plugins',
    focusKey: 'plugins',
    requiresHost: true,
    titleEn: 'Plugins',
    titleZh: '插件',
    titleJa: 'プラグイン',
    groupEn: 'Context & Extensions',
    groupZh: '上下文与扩展',
    groupJa: 'コンテキストと拡張',
    keywords: 'plugins wasm extensions enable disable capabilities WASM 插件 插件 扩展 启停 WASMプラグイン プラグイン 有効 無効'
  },
  {
    id: 'ext_sandbox',
    group: 'context_extensions',
    destination: 'sandbox',
    focusKey: 'sandbox',
    requiresHost: true,
    titleEn: 'Sandbox',
    titleZh: '沙箱',
    titleJa: 'サンドボックス',
    groupEn: 'Context & Extensions',
    groupZh: '上下文与扩展',
    groupJa: 'コンテキストと拡張',
    keywords: 'sandbox os policy isolation workspace write strict read only network credential paths 操作系统沙箱策略 沙箱 隔离 只读 网络 凭据路径 OSサンドボックスポリシー サンドボックス 隔離 ネットワーク'
  }
];

export function localizeSettingsSearchTitle(
  entry: OrbitSettingsSearchEntry,
  appLanguage: string,
  systemLanguageOverride?: string
): string {
  const effective = resolveEffectiveOrbitLanguage(appLanguage, systemLanguageOverride);
  if (effective === 'zh_CN') {
    return entry.titleZh;
  }
  if (effective === 'ja_JP') {
    return entry.titleJa;
  }
  return entry.titleEn;
}

export function localizeSettingsSearchGroup(
  entry: OrbitSettingsSearchEntry,
  appLanguage: string,
  systemLanguageOverride?: string
): string {
  const effective = resolveEffectiveOrbitLanguage(appLanguage, systemLanguageOverride);
  if (effective === 'zh_CN') {
    return entry.groupZh;
  }
  if (effective === 'ja_JP') {
    return entry.groupJa;
  }
  return entry.groupEn;
}

export function searchOrbitSettings(
  query: string,
  hasHost: boolean = true,
  includeHostOnlyWhenNoHost: boolean = true
): OrbitSettingsSearchHit[] {
  const clean = query.trim().toLowerCase();
  if (clean.length === 0) {
    return [];
  }
  const tokens = clean.split(/\s+/).filter((tok: string): boolean => tok.length > 0);
  if (tokens.length === 0) {
    return [];
  }
  const hits: OrbitSettingsSearchHit[] = [];
  for (let i = 0; i < ORBIT_SETTINGS_SEARCH_CATALOG.length; i++) {
    const entry = ORBIT_SETTINGS_SEARCH_CATALOG[i];
    if (!hasHost && entry.requiresHost && !includeHostOnlyWhenNoHost) {
      continue;
    }
    const tEn = entry.titleEn.toLowerCase();
    const tZh = entry.titleZh.toLowerCase();
    const tJa = entry.titleJa.toLowerCase();
    const titles = `${tEn} ${tZh} ${tJa}`;
    const groups = `${entry.groupEn} ${entry.groupZh} ${entry.groupJa}`.toLowerCase();
    const haystack = `${titles} ${groups} ${entry.keywords.toLowerCase()} ${entry.destination}`;
    let matchedAll = true;
    let score = 0;
    for (let t = 0; t < tokens.length; t++) {
      const tok = tokens[t];
      if (!haystack.includes(tok)) {
        matchedAll = false;
        break;
      }
      if (titles.includes(tok)) {
        score += 30;
      } else if (groups.includes(tok)) {
        score += 15;
      } else {
        score += 8;
      }
    }
    if (matchedAll) {
      if (tEn === clean || tZh === clean || tJa === clean) {
        score += 50;
      } else if (tEn.startsWith(clean) || tZh.startsWith(clean) || tJa.startsWith(clean)) {
        score += 20;
      }
      hits.push({ entry: entry, score: score });
    }
  }
  hits.sort((a: OrbitSettingsSearchHit, b: OrbitSettingsSearchHit): number => b.score - a.score);
  return hits;
}

// --- Cross-Host, Global-Epoch, Visibility & Row-Level Concurrent Write Guard ---

let nextSettingsGuardInstanceId: number = 1;

export interface OrbitSettingsRequestToken {
  epoch: number;
  scopeEpoch: number;
  hostId: string;
  scopeKey: string;
  globalHostEpoch: number;
  rootEpoch: number;
  pageInstanceId: number;
}

export interface OrbitSettingsItemWriteToken {
  scopeEpoch: number;
  hostId: string;
  scopeKey: string;
  itemId: string;
  itemKey: string;
  itemSeq: number;
  globalHostEpoch: number;
  rootEpoch: number;
  pageInstanceId: number;
}

export interface OrbitSettingsDeleteConfirmSnapshot {
  pageInstanceId: number;
  hostId: string;
  scopeKey: string;
  cwd: string;
  itemId: string;
  globalHostEpoch: number;
  rootEpoch: number;
  scopeEpoch: number;
  dialogSeq: number;
}

export class OrbitSettingsRequestGuard {
  public readonly pageInstanceId: number;
  private loadEpoch: number = 0;
  private scopeEpoch: number = 0;
  private dialogSeq: number = 0;
  private activeHostId: string = '';
  private activeScopeKey: string = '';
  private mounted: boolean = true;
  private visible: boolean = true;
  private visibilityEpoch: number = 1;
  private readonly itemWriteSeqByKey: Map<string, number> = new Map<string, number>();

  constructor() {
    this.pageInstanceId = nextSettingsGuardInstanceId;
    nextSettingsGuardInstanceId += 1;
  }

  private encodeItemKey(scopeKey: string, itemId: string): string {
    return `${scopeKey}\u0000${itemId}`;
  }

  /**
   * Starts a list/snapshot load for `(hostId, scopeKey)` without invalidating concurrent
   * row-level writes in the same scope (or resets scope if `(hostId, scopeKey)` changed).
   */
  public beginLoad(
    hostId: string,
    scopeKey: string = '',
    globalHostEpoch: number = 0,
    rootEpoch: number = 0
  ): OrbitSettingsRequestToken {
    const cleanHost = hostId.trim();
    if (cleanHost !== this.activeHostId || scopeKey !== this.activeScopeKey) {
      this.scopeEpoch += 1;
      this.dialogSeq += 1;
      this.itemWriteSeqByKey.clear();
    }
    this.loadEpoch += 1;
    this.activeHostId = cleanHost;
    this.activeScopeKey = scopeKey;
    return {
      epoch: this.loadEpoch,
      scopeEpoch: this.scopeEpoch,
      hostId: this.activeHostId,
      scopeKey: this.activeScopeKey,
      globalHostEpoch: globalHostEpoch,
      rootEpoch: rootEpoch,
      pageInstanceId: this.pageInstanceId
    };
  }

  /** Backward-compatible alias for `beginLoad`. */
  public begin(
    hostId: string,
    scopeKey: string = '',
    globalHostEpoch: number = 0,
    rootEpoch: number = 0
  ): OrbitSettingsRequestToken {
    return this.beginLoad(hostId, scopeKey, globalHostEpoch, rootEpoch);
  }

  /**
   * Starts an independent row-level mutation keyed by `(scopeKey, itemId)`.
   * Concurrent writes on distinct items (A and B) maintain separate monotonic sequences so
   * finishing B never blocks A from settling its busy flag or applying its own outcome.
   */
  public beginItemWrite(
    hostId: string,
    scopeKey: string,
    itemId: string,
    globalHostEpoch: number = 0,
    rootEpoch: number = 0
  ): OrbitSettingsItemWriteToken {
    const cleanHost = hostId.trim();
    if (cleanHost !== this.activeHostId || scopeKey !== this.activeScopeKey) {
      this.scopeEpoch += 1;
      this.dialogSeq += 1;
      this.itemWriteSeqByKey.clear();
      this.activeHostId = cleanHost;
      this.activeScopeKey = scopeKey;
    }
    const itemKey = this.encodeItemKey(scopeKey, itemId);
    const nextSeq = (this.itemWriteSeqByKey.get(itemKey) ?? 0) + 1;
    this.itemWriteSeqByKey.set(itemKey, nextSeq);
    return {
      scopeEpoch: this.scopeEpoch,
      hostId: this.activeHostId,
      scopeKey: this.activeScopeKey,
      itemId: itemId,
      itemKey: itemKey,
      itemSeq: nextSeq,
      globalHostEpoch: globalHostEpoch,
      rootEpoch: rootEpoch,
      pageInstanceId: this.pageInstanceId
    };
  }

  public isItemWriteCurrent(
    token: OrbitSettingsItemWriteToken,
    currentGlobalHostEpoch: number = token.globalHostEpoch,
    currentRootEpoch: number = token.rootEpoch
  ): boolean {
    return (
      this.mounted &&
      this.visible &&
      token.pageInstanceId === this.pageInstanceId &&
      token.scopeEpoch === this.scopeEpoch &&
      token.hostId === this.activeHostId &&
      token.scopeKey === this.activeScopeKey &&
      token.globalHostEpoch === currentGlobalHostEpoch &&
      token.rootEpoch === currentRootEpoch &&
      this.itemWriteSeqByKey.get(token.itemKey) === token.itemSeq
    );
  }

  public freezeDeleteConfirm(
    hostId: string,
    scopeKey: string,
    cwd: string,
    itemId: string,
    globalHostEpoch: number = 0,
    rootEpoch: number = 0
  ): OrbitSettingsDeleteConfirmSnapshot {
    const cleanHost = hostId.trim();
    if (cleanHost !== this.activeHostId || scopeKey !== this.activeScopeKey) {
      this.scopeEpoch += 1;
      this.itemWriteSeqByKey.clear();
      this.activeHostId = cleanHost;
      this.activeScopeKey = scopeKey;
    }
    this.dialogSeq += 1;
    return {
      pageInstanceId: this.pageInstanceId,
      hostId: this.activeHostId,
      scopeKey: this.activeScopeKey,
      cwd: cwd,
      itemId: itemId,
      globalHostEpoch: globalHostEpoch,
      rootEpoch: rootEpoch,
      scopeEpoch: this.scopeEpoch,
      dialogSeq: this.dialogSeq
    };
  }

  public cancelDeleteConfirm(): void {
    this.dialogSeq += 1;
  }

  public isDeleteConfirmValid(
    snap: OrbitSettingsDeleteConfirmSnapshot,
    currentHostId: string,
    currentScopeKey: string,
    currentCwd: string,
    currentGlobalHostEpoch: number,
    currentRootEpoch: number
  ): boolean {
    return (
      this.mounted &&
      this.visible &&
      snap.pageInstanceId === this.pageInstanceId &&
      snap.dialogSeq === this.dialogSeq &&
      snap.scopeEpoch === this.scopeEpoch &&
      snap.hostId === currentHostId.trim() &&
      snap.hostId === this.activeHostId &&
      snap.scopeKey === currentScopeKey &&
      snap.scopeKey === this.activeScopeKey &&
      snap.cwd === currentCwd &&
      snap.globalHostEpoch === currentGlobalHostEpoch &&
      snap.rootEpoch === currentRootEpoch
    );
  }

  public invalidate(nextHostId?: string, nextScopeKey?: string): void {
    this.loadEpoch += 1;
    this.scopeEpoch += 1;
    this.dialogSeq += 1;
    this.itemWriteSeqByKey.clear();
    if (nextHostId !== undefined) {
      this.activeHostId = nextHostId.trim();
    }
    if (nextScopeKey !== undefined) {
      this.activeScopeKey = nextScopeKey;
    }
  }

  public setVisible(nextVisible: boolean): void {
    if (this.visible === nextVisible) {
      return;
    }
    this.visible = nextVisible;
    this.visibilityEpoch += 1;
    if (!nextVisible) {
      this.loadEpoch += 1;
      this.scopeEpoch += 1;
      this.dialogSeq += 1;
      this.itemWriteSeqByKey.clear();
    }
  }

  public unmount(): void {
    this.mounted = false;
    this.visible = false;
    this.visibilityEpoch += 1;
    this.loadEpoch += 1;
    this.scopeEpoch += 1;
    this.dialogSeq += 1;
    this.itemWriteSeqByKey.clear();
  }

  public isCurrent(
    token: OrbitSettingsRequestToken,
    currentGlobalHostEpoch: number = token.globalHostEpoch,
    currentRootEpoch: number = token.rootEpoch
  ): boolean {
    return (
      this.mounted &&
      this.visible &&
      token.pageInstanceId === this.pageInstanceId &&
      token.epoch === this.loadEpoch &&
      token.scopeEpoch === this.scopeEpoch &&
      token.hostId === this.activeHostId &&
      token.scopeKey === this.activeScopeKey &&
      token.globalHostEpoch === currentGlobalHostEpoch &&
      token.rootEpoch === currentRootEpoch
    );
  }

  public get currentHostId(): string {
    return this.activeHostId;
  }

  public get isMounted(): boolean {
    return this.mounted;
  }

  public get isVisible(): boolean {
    return this.visible;
  }

  public get isInteractive(): boolean {
    return this.mounted && this.visible;
  }

  public get visibleEpoch(): number {
    return this.visibilityEpoch;
  }
}

/**
 * Immutable render-time control context binding a single action button/toggle to the exact
 * `(hostId, hostSelectionEpoch, rootEpoch, visibleEpoch, scopeKey)` active when rendered.
 */
export interface OrbitSettingsControlBinding {
  renderKey: string;
  hostId: string;
  hostSelectionEpoch: number;
  rootEpoch: number;
  visibleEpoch: number;
  scopeKey: string;
}

export function createSettingsControlBinding(
  hostId: string,
  hostSelectionEpoch: number,
  rootEpoch: number,
  visibleEpoch: number,
  scopeKey: string = ''
): OrbitSettingsControlBinding {
  const cleanHost = hostId.trim();
  return {
    renderKey: `${cleanHost}\u0000${hostSelectionEpoch}\u0000${rootEpoch}\u0000${visibleEpoch}\u0000${scopeKey}`,
    hostId: cleanHost,
    hostSelectionEpoch: hostSelectionEpoch,
    rootEpoch: rootEpoch,
    visibleEpoch: visibleEpoch,
    scopeKey: scopeKey
  };
}

export function isSettingsControlBindingActive(
  binding: OrbitSettingsControlBinding,
  pageMounted: boolean,
  pageVisible: boolean,
  liveVisibleEpoch: number,
  liveHostId: string,
  liveHostSelectionEpoch: number,
  liveRootEpoch: number,
  liveScopeKey: string = ''
): boolean {
  if (!pageMounted || !pageVisible) {
    return false;
  }
  return (
    binding.visibleEpoch > 0 &&
    binding.visibleEpoch === liveVisibleEpoch &&
    binding.hostId === liveHostId.trim() &&
    binding.hostSelectionEpoch === liveHostSelectionEpoch &&
    binding.rootEpoch === liveRootEpoch &&
    binding.scopeKey === liveScopeKey
  );
}

/**
 * Resolves the exact (untrimmed) active Memory directory for the current host:
 * preserves `currentSelectedCwd` if and only if it matches an entry in `availableCwds`
 * byte-for-byte; otherwise picks `availableCwds[0]` (or `''` when no directory is registered).
 */
export function resolveEffectiveMemoryCwd(
  currentSelectedCwd: string,
  availableCwds: string[]
): string {
  if (currentSelectedCwd.length > 0 && availableCwds.includes(currentSelectedCwd)) {
    return currentSelectedCwd;
  }
  return availableCwds.length > 0 ? availableCwds[0] : '';
}

/**
 * Immutable render-time binding for a Memory workspace directory chip button.
 * Preserves `rawCwd` byte-for-byte (never trims spaces, slashes, or POSIX backslashes)
 * and pairs it with the exact `(hostId, hostSelectionEpoch, rootEpoch, visibleEpoch)` active
 * when rendered.
 */
export interface OrbitMemoryCwdOptionBinding {
  renderKey: string;
  hostId: string;
  hostSelectionEpoch: number;
  rootEpoch: number;
  visibleEpoch: number;
  rawCwd: string;
}

export function createMemoryCwdOptionBinding(
  hostId: string,
  hostSelectionEpoch: number,
  rootEpoch: number,
  visibleEpoch: number,
  rawCwd: string
): OrbitMemoryCwdOptionBinding {
  const cleanHost = hostId.trim();
  return {
    renderKey: `${cleanHost}\u0000${hostSelectionEpoch}\u0000${rootEpoch}\u0000${visibleEpoch}\u0000${rawCwd}`,
    hostId: cleanHost,
    hostSelectionEpoch: hostSelectionEpoch,
    rootEpoch: rootEpoch,
    visibleEpoch: visibleEpoch,
    rawCwd: rawCwd
  };
}

export function isMemoryCwdOptionBindingActive(
  binding: OrbitMemoryCwdOptionBinding,
  pageMounted: boolean,
  pageVisible: boolean,
  liveVisibleEpoch: number,
  liveHostId: string,
  liveHostSelectionEpoch: number,
  liveRootEpoch: number,
  liveAvailableCwds: string[]
): boolean {
  if (!pageMounted || !pageVisible) {
    return false;
  }
  const cleanLiveHost = liveHostId.trim();
  return (
    binding.visibleEpoch > 0 &&
    binding.visibleEpoch === liveVisibleEpoch &&
    binding.hostId.length > 0 &&
    binding.rawCwd.length > 0 &&
    binding.hostId === cleanLiveHost &&
    binding.hostSelectionEpoch === liveHostSelectionEpoch &&
    binding.rootEpoch === liveRootEpoch &&
    liveAvailableCwds.includes(binding.rawCwd)
  );
}

/**
 * Resolves the exact (untrimmed) project `cwd` strictly scoped to `targetHost` for MCP & Skills.
 * When `targetHost` differs from `selectedHostId`, NEVER returns `activeCwd` from another host:
 * uses only `targetHostCwds` or degrades to `null` (host-only query).
 */
export function resolveHostScopedExtensionCwdPure(
  targetHost: string,
  selectedHostId: string,
  activeCwd: string,
  targetHostCwds: string[],
  includeProjectScope: boolean,
  explicitSelection: string
): string | null {
  const cleanTarget = targetHost.trim();
  if (!includeProjectScope || cleanTarget.length === 0) {
    return null;
  }
  const exactExplicit = nonBlankPath(explicitSelection);
  if (exactExplicit.length > 0 && targetHostCwds.includes(exactExplicit)) {
    return exactExplicit;
  }
  if (cleanTarget === selectedHostId.trim()) {
    const active = nonBlankPath(activeCwd);
    if (active.length > 0 && active !== '~') {
      return active;
    }
  }
  return targetHostCwds.length > 0 ? targetHostCwds[0] : null;
}

/**
 * Immutable row binding tying each rendered Memory item to the exact successful load batch
 * (`loadSeq`, `hostId`, `hostSelectionEpoch`, `rootEpoch`, `visibleEpoch`, `scopeKey`, `requestCwd`)
 * that produced it.
 *
 * Eliminates client-side path string guessing (never strips POSIX `\` or guesses remote symlinks
 * like `/tmp/...` vs `/private/tmp/...`): dialogs and write RPCs read BOTH UI title and target
 * `(hostId, item.id, requestCwd)` strictly from the same verified binding.
 */
export interface OrbitMemoryRowBinding {
  loadSeq: number;
  hostId: string;
  hostSelectionEpoch: number;
  rootEpoch: number;
  visibleEpoch: number;
  scopeKey: string;
  requestCwd: string;
  item: ProjectMemoryItemDto;
}

export function isMemoryRowBindingActive(
  binding: OrbitMemoryRowBinding,
  pageMounted: boolean,
  pageVisible: boolean,
  liveVisibleEpoch: number,
  loadedListSeq: number,
  liveHostId: string,
  liveHostSelectionEpoch: number,
  liveRootEpoch: number,
  liveScopeKey: string,
  liveRequestCwd: string,
  liveBindings: OrbitMemoryRowBinding[]
): boolean {
  if (
    !pageMounted ||
    !pageVisible ||
    binding.visibleEpoch <= 0 ||
    binding.visibleEpoch !== liveVisibleEpoch ||
    binding.loadSeq <= 0 ||
    binding.loadSeq !== loadedListSeq
  ) {
    return false;
  }
  const cleanLiveHost = liveHostId.trim();
  if (
    binding.hostId.length === 0 ||
    binding.requestCwd.length === 0 ||
    binding.hostId !== cleanLiveHost ||
    binding.hostSelectionEpoch !== liveHostSelectionEpoch ||
    binding.rootEpoch !== liveRootEpoch ||
    binding.scopeKey !== liveScopeKey ||
    binding.requestCwd !== liveRequestCwd
  ) {
    return false;
  }
  for (let i = 0; i < liveBindings.length; i++) {
    const cur = liveBindings[i];
    if (
      cur.loadSeq === binding.loadSeq &&
      cur.visibleEpoch === binding.visibleEpoch &&
      cur.item.id === binding.item.id
    ) {
      return true;
    }
  }
  return false;
}

/**
 * Checks whether an ALREADY-DISPATCHED Memory write RPC may settle its confirmed/deleted result
 * into the live list. Unlike pre-dispatch `isMemoryRowBindingActive`, this allows `loadedListSeq`
 * to have advanced via a same-scope refresh (`loadedListSeq >= binding.loadSeq`) while still
 * strictly requiring the exact same mounted/visible page, `visibleEpoch`, `hostId`,
 * `hostSelectionEpoch`, `rootEpoch`, `scopeKey`, and `requestCwd` (paired with `guard.isItemWriteCurrent`).
 */
export function canSettleMemoryRowWrite(
  binding: OrbitMemoryRowBinding,
  pageMounted: boolean,
  pageVisible: boolean,
  liveVisibleEpoch: number,
  loadedListSeq: number,
  loadedHostId: string,
  loadedScopeKey: string,
  loadedRequestCwd: string,
  liveHostId: string,
  liveHostSelectionEpoch: number,
  liveRootEpoch: number,
  liveScopeKey: string,
  liveRequestCwd: string
): boolean {
  if (
    !pageMounted ||
    !pageVisible ||
    binding.visibleEpoch <= 0 ||
    binding.visibleEpoch !== liveVisibleEpoch ||
    binding.loadSeq <= 0 ||
    loadedListSeq < binding.loadSeq
  ) {
    return false;
  }
  const cleanLiveHost = liveHostId.trim();
  return (
    binding.hostId.length > 0 &&
    binding.requestCwd.length > 0 &&
    binding.hostId === cleanLiveHost &&
    binding.hostId === loadedHostId &&
    binding.hostSelectionEpoch === liveHostSelectionEpoch &&
    binding.rootEpoch === liveRootEpoch &&
    binding.scopeKey === liveScopeKey &&
    binding.scopeKey === loadedScopeKey &&
    binding.requestCwd === liveRequestCwd &&
    binding.requestCwd === loadedRequestCwd
  );
}

export function applySettledMemoryConfirm(
  liveBindings: OrbitMemoryRowBinding[],
  itemId: string,
  updatedAtMs: number
): OrbitMemoryRowBinding[] {
  const next: OrbitMemoryRowBinding[] = [];
  for (let i = 0; i < liveBindings.length; i++) {
    const curBinding = liveBindings[i];
    const cur = curBinding.item;
    if (cur.id === itemId) {
      next.push({
        loadSeq: curBinding.loadSeq,
        hostId: curBinding.hostId,
        hostSelectionEpoch: curBinding.hostSelectionEpoch,
        rootEpoch: curBinding.rootEpoch,
        visibleEpoch: curBinding.visibleEpoch,
        scopeKey: curBinding.scopeKey,
        requestCwd: curBinding.requestCwd,
        item: {
          id: cur.id,
          workspace_root: cur.workspace_root,
          title: cur.title,
          content: cur.content,
          category: cur.category,
          status: 'confirmed',
          confidence: cur.confidence,
          file_paths: cur.file_paths,
          tags: cur.tags,
          source_provider: cur.source_provider,
          source_agent_id: cur.source_agent_id,
          source_path: cur.source_path,
          access_count: cur.access_count,
          created_at_ms: cur.created_at_ms,
          updated_at_ms: updatedAtMs,
          last_accessed_ms: cur.last_accessed_ms
        }
      });
    } else {
      next.push(curBinding);
    }
  }
  return next;
}

export function applySettledMemoryDelete(
  liveBindings: OrbitMemoryRowBinding[],
  itemId: string
): OrbitMemoryRowBinding[] {
  const next: OrbitMemoryRowBinding[] = [];
  for (let i = 0; i < liveBindings.length; i++) {
    if (liveBindings[i].item.id !== itemId) {
      next.push(liveBindings[i]);
    }
  }
  return next;
}

/**
 * Immutable row binding for rendered MCP servers (`OrbitSettingsMcpPage`).
 */
export interface OrbitMcpRowBinding {
  loadSeq: number;
  hostId: string;
  hostSelectionEpoch: number;
  rootEpoch: number;
  visibleEpoch: number;
  scopeKey: string;
  requestCwd: string | null;
  server: ExternalMcpServerRecordDto;
}

export function isMcpRowBindingActive(
  binding: OrbitMcpRowBinding,
  pageMounted: boolean,
  pageVisible: boolean,
  liveVisibleEpoch: number,
  loadedListSeq: number,
  liveHostId: string,
  liveHostSelectionEpoch: number,
  liveRootEpoch: number,
  liveScopeKey: string,
  liveBindings: OrbitMcpRowBinding[]
): boolean {
  if (
    !pageMounted ||
    !pageVisible ||
    binding.visibleEpoch <= 0 ||
    binding.visibleEpoch !== liveVisibleEpoch ||
    binding.loadSeq <= 0 ||
    binding.loadSeq !== loadedListSeq
  ) {
    return false;
  }
  const cleanLiveHost = liveHostId.trim();
  if (
    binding.hostId.length === 0 ||
    binding.hostId !== cleanLiveHost ||
    binding.hostSelectionEpoch !== liveHostSelectionEpoch ||
    binding.rootEpoch !== liveRootEpoch ||
    binding.scopeKey !== liveScopeKey
  ) {
    return false;
  }
  for (let i = 0; i < liveBindings.length; i++) {
    const cur = liveBindings[i];
    if (
      cur.loadSeq === binding.loadSeq &&
      cur.visibleEpoch === binding.visibleEpoch &&
      cur.server.id === binding.server.id
    ) {
      return true;
    }
  }
  return false;
}

export function canSettleMcpRowWrite(
  binding: OrbitMcpRowBinding,
  pageMounted: boolean,
  pageVisible: boolean,
  liveVisibleEpoch: number,
  loadedListSeq: number,
  loadedHostId: string,
  loadedScopeKey: string,
  liveHostId: string,
  liveHostSelectionEpoch: number,
  liveRootEpoch: number,
  liveScopeKey: string
): boolean {
  if (
    !pageMounted ||
    !pageVisible ||
    binding.visibleEpoch <= 0 ||
    binding.visibleEpoch !== liveVisibleEpoch ||
    binding.loadSeq <= 0 ||
    loadedListSeq < binding.loadSeq
  ) {
    return false;
  }
  const cleanLiveHost = liveHostId.trim();
  return (
    binding.hostId.length > 0 &&
    binding.hostId === cleanLiveHost &&
    binding.hostId === loadedHostId &&
    binding.hostSelectionEpoch === liveHostSelectionEpoch &&
    binding.rootEpoch === liveRootEpoch &&
    binding.scopeKey === liveScopeKey &&
    binding.scopeKey === loadedScopeKey
  );
}

export function applySettledMcpToggle(
  liveBindings: OrbitMcpRowBinding[],
  serverId: string,
  nextEnabled: boolean
): OrbitMcpRowBinding[] {
  const next: OrbitMcpRowBinding[] = [];
  for (let i = 0; i < liveBindings.length; i++) {
    const curBinding = liveBindings[i];
    const item = curBinding.server;
    if (item.id === serverId) {
      next.push({
        loadSeq: curBinding.loadSeq,
        hostId: curBinding.hostId,
        hostSelectionEpoch: curBinding.hostSelectionEpoch,
        rootEpoch: curBinding.rootEpoch,
        visibleEpoch: curBinding.visibleEpoch,
        scopeKey: curBinding.scopeKey,
        requestCwd: curBinding.requestCwd,
        server: {
          id: item.id,
          name: item.name,
          scope: item.scope,
          transport: item.transport,
          env_keys: item.env_keys,
          masked_env: item.masked_env,
          masked_headers: item.masked_headers,
          enabled: nextEnabled,
          editable: item.editable,
          source_path: item.source_path,
          last_probe: item.last_probe
        }
      });
    } else {
      next.push(curBinding);
    }
  }
  return next;
}

/**
 * Immutable row binding for rendered Plugins (`OrbitSettingsPluginsPage`).
 */
export interface OrbitPluginRowBinding {
  loadSeq: number;
  hostId: string;
  hostSelectionEpoch: number;
  rootEpoch: number;
  visibleEpoch: number;
  plugin: InstalledPluginDto;
}

export function isPluginRowBindingActive(
  binding: OrbitPluginRowBinding,
  pageMounted: boolean,
  pageVisible: boolean,
  liveVisibleEpoch: number,
  loadedListSeq: number,
  liveHostId: string,
  liveHostSelectionEpoch: number,
  liveRootEpoch: number,
  liveBindings: OrbitPluginRowBinding[]
): boolean {
  if (
    !pageMounted ||
    !pageVisible ||
    binding.visibleEpoch <= 0 ||
    binding.visibleEpoch !== liveVisibleEpoch ||
    binding.loadSeq <= 0 ||
    binding.loadSeq !== loadedListSeq
  ) {
    return false;
  }
  const cleanLiveHost = liveHostId.trim();
  if (
    binding.hostId.length === 0 ||
    binding.hostId !== cleanLiveHost ||
    binding.hostSelectionEpoch !== liveHostSelectionEpoch ||
    binding.rootEpoch !== liveRootEpoch
  ) {
    return false;
  }
  for (let i = 0; i < liveBindings.length; i++) {
    const cur = liveBindings[i];
    if (
      cur.loadSeq === binding.loadSeq &&
      cur.visibleEpoch === binding.visibleEpoch &&
      cur.plugin.manifest.id === binding.plugin.manifest.id
    ) {
      return true;
    }
  }
  return false;
}

export function canSettlePluginRowWrite(
  binding: OrbitPluginRowBinding,
  pageMounted: boolean,
  pageVisible: boolean,
  liveVisibleEpoch: number,
  loadedListSeq: number,
  loadedHostId: string,
  liveHostId: string,
  liveHostSelectionEpoch: number,
  liveRootEpoch: number
): boolean {
  if (
    !pageMounted ||
    !pageVisible ||
    binding.visibleEpoch <= 0 ||
    binding.visibleEpoch !== liveVisibleEpoch ||
    binding.loadSeq <= 0 ||
    loadedListSeq < binding.loadSeq
  ) {
    return false;
  }
  const cleanLiveHost = liveHostId.trim();
  return (
    binding.hostId.length > 0 &&
    binding.hostId === cleanLiveHost &&
    binding.hostId === loadedHostId &&
    binding.hostSelectionEpoch === liveHostSelectionEpoch &&
    binding.rootEpoch === liveRootEpoch
  );
}

export function applySettledPluginToggle(
  liveBindings: OrbitPluginRowBinding[],
  pluginId: string,
  nextEnabled: boolean
): OrbitPluginRowBinding[] {
  const next: OrbitPluginRowBinding[] = [];
  for (let i = 0; i < liveBindings.length; i++) {
    const curBinding = liveBindings[i];
    const item = curBinding.plugin;
    if (item.manifest.id === pluginId) {
      next.push({
        loadSeq: curBinding.loadSeq,
        hostId: curBinding.hostId,
        hostSelectionEpoch: curBinding.hostSelectionEpoch,
        rootEpoch: curBinding.rootEpoch,
        visibleEpoch: curBinding.visibleEpoch,
        plugin: {
          manifest: item.manifest,
          enabled: nextEnabled
        }
      });
    } else {
      next.push(curBinding);
    }
  }
  return next;
}

// --- Pull-to-Refresh State Controller (Guarantees Reliable Reset on Error / Host Switch / Unmount) ---

export class OrbitPullRefreshController {
  private epoch: number = 0;
  private scopeKey: string = '';
  private mounted: boolean = true;
  private refreshing: boolean = false;

  public get isRefreshing(): boolean {
    return this.refreshing;
  }

  public begin(scopeKey: string): number {
    if (!this.mounted) {
      return 0;
    }
    const cleanScope = scopeKey.trim();
    if (this.refreshing && this.scopeKey === cleanScope) {
      return 0;
    }
    this.epoch += 1;
    this.scopeKey = cleanScope;
    this.refreshing = true;
    return this.epoch;
  }

  public settle(token: number): boolean {
    if (token <= 0 || token !== this.epoch) {
      return false;
    }
    this.refreshing = false;
    return true;
  }

  public onScopeChanged(nextScopeKey: string): boolean {
    const cleanScope = nextScopeKey.trim();
    if (cleanScope === this.scopeKey && !this.refreshing) {
      return false;
    }
    this.scopeKey = cleanScope;
    this.epoch += 1;
    const wasRefreshing = this.refreshing;
    this.refreshing = false;
    return wasRefreshing;
  }

  public unmount(): void {
    this.mounted = false;
    this.epoch += 1;
    this.refreshing = false;
  }

  public async run(
    scopeKey: string,
    work: () => Promise<void>,
    onRefreshingChanged: (refreshing: boolean) => void
  ): Promise<boolean> {
    const token = this.begin(scopeKey);
    if (token === 0) {
      return false;
    }
    onRefreshingChanged(true);
    let succeeded = false;
    try {
      await work();
      succeeded = true;
    } catch (_err) {
      succeeded = false;
    } finally {
      if (this.settle(token) && this.mounted) {
        onRefreshingChanged(false);
      }
    }
    return succeeded;
  }
}
