import type { MessageKey } from "../../lib/i18n/locales/en.ts";
import { SHORTCUT_ACTIONS } from "../../lib/shortcut-registry.ts";
import type { SettingsPage } from "./settings-nav.ts";

// The label key is also the data-setting-id of the rendered row. Search reads each label
// in all three locales; the last column covers terms users know from other tools or CLIs.
type SettingEntry = readonly [target: SettingsPage, label: MessageKey, keywords: string];

export const SETTING_ENTRIES = [
  ["general", "settings.language", "locale interface language 言語 语言 日本語 中文 english"],
  ["general", "settings.sendWithEnter", "enter to send composer submit 回车发送 エンターキー 送信"],
  ["general", "settings.collapseExploration", "coalesce exploration steps collapse 折叠探索 探索ステップ"],
  ["general", "settings.externalEditor", "external ide vscode cursor zed 外部编辑器 エディタ"],
  ["general", "settings.isolateWorktrees", "git isolate worktree 隔离 工作树 分離 作業ツリー"],
  ["general", "settings.branchPrefix", "worktree branch prefix git 分支前缀 ブランチ接頭辞"],
  ["general", "settings.appVersion", "check for updates app desktop version 版本升级 バージョン更新"],
  ["general", "settings.setupGuide", "onboarding walkthrough first run 入门向导 セットアップ 初期設定"],

  ["appearance", "settings.groupTheme", "system light dark color scheme 主题 暗色 ダーク ライト テーマ"],
  ["appearance", "settings.diffLayout", "git code review diff layout unified split 统一 分栏 差分 並列"],

  ["shortcuts", "settings.searchShortcutsPlaceholder", "keybindings hotkeys filter 查找快捷键 ショートカット検索"],
  ...SHORTCUT_ACTIONS.map(({ labelKey, defaultChord }): SettingEntry => ["shortcuts", labelKey, defaultChord]),

  ["notifications", "settings.desktopNotificationPermission", "desktop notification permissions 系统通知 通知許可"],
  ["notifications", "settings.notifyApproval", "approval notification 审批通知 承認通知"],
  ["notifications", "settings.notifyTurnComplete", "turn complete notification 回合结束 ターン完了"],
  ["notifications", "settings.notifyAgentError", "agent error notification 出错通知 エラー通知"],
  ["notifications", "settings.notifyGoalFinished", "goal finished notification 目标完成 目標完了"],
  ["notifications", "settings.notificationSoundTitle", "notification sound chime 声音 音"],
  ["notifications", "settings.notifyQuietHours", "desktop quiet hours do not disturb 免打扰 勿扰 おやすみ"],
  ["notifications", "settings.pushEnabled", "mobile background push 移动推送 プッシュ通知"],
  ["notifications", "settings.pushMessagePreview", "push message preview 消息预览 通知プレビュー"],
  ["notifications", "settings.pushQuietHours", "mobile quiet hours 移动免打扰 モバイル静音"],
  ["notifications", "settings.pushRegisteredDevices", "registered phone devices 已注册设备 登録端末"],

  ["voice", "voice.enableTitle", "dictation speech microphone 语音听写 音声入力"],
  ["voice", "voice.languageTitle", "dictation language 识别语言 認識言語"],
  ["voice", "voice.autoSpeakTitle", "read aloud speak responses 朗读 音声読み上げ"],
  ["voice", "voice.sttBackendTitle", "whisper speech to text 语音转文字 音声認識"],
  ["voice", "voice.ttsBackendTitle", "tts text to speech 文本转语音 音声合成"],
  ["voice", "voice.sectionModels", "download voice models 语音模型 音声モデル"],

  ["cloud_sync", "settings.cloudSyncTitle", "encrypted sync status 云同步状态 クラウド同期"],
  ["cloud_sync", "settings.signInGithub", "github authentication sign in 登录 ログイン"],
  ["cloud_sync", "settings.linkDeviceQr", "one scan device link qr code 扫码关联 デバイスリンク"],
  ["cloud_sync", "settings.cloudAutoSync", "automatic sync changes 自动同步 自動同期"],
  ["cloud_sync", "settings.cloudHeadless", "auto register headless server 自动注册无头服务器 ヘッドレス登録"],
  ["cloud_sync", "settings.fleetNetworkTitle", "automatic fleet network 自动组网 自動ネットワーク"],
  ["cloud_sync", "settings.cloudSyncAdvancedTitle", "security passphrase vault recovery 口令 暗号化パスフレーズ"],

  ["hosts", "settings.checkForUpdates", "fleet upgrade sync all daemon versions 集群升级 一括更新"],
  ["hosts", "settings.fleetConnectHost", "add connect remote host ssh pairing 连接远程主机 リモートホストを接続"],
  ["connection", "settings.mobilePairing", "pairing qr phone mobile device 手机配对 端末ペアリング"],
  ["connection", "settings.advancedRelayTitle", "advanced relay ssh cloudflare tunnel 中继 隧道 リレー トンネル"],
  ["connection", "settings.agentMesh", "connected hosts mesh peer network 主机互联 ピアメッシュ"],
  ["connection", "settings.fleetRemoveHost", "forget remove disconnect host 移除主机 ホスト削除"],

  ["agents", "settings.agentInstalledClis", "agent cli models refresh provider installed 已安装 模型刷新 CLI 一覧"],
  ["agents", "im.defaultProvider", "default provider default agent 默认提供商 デフォルトプロバイダ"],
  ["agents", "settings.agentMcpInjector", "built in orbit mcp orchestration 内置编排 オーケストレーション"],
  ["agents", "settings.agentKeepAwake", "keep awake prevent sleep 保持唤醒 スリープ抑止"],
  ["agents", "settings.agentIdleCleanup", "idle runtime cleanup timeout 空闲清理 アイドルクリーンアップ"],
  ["agents", "settings.agentAutoResume", "auto resume daemon startup 自动恢复 自動再開"],
  ["agents", "settings.agentProfiles", "agent profiles presets 配置档 プロファイル"],
  ["acp", "settings.subTabAcpRegistry", "acp registry custom agents 注册表 レジストリ"],

  ["personal_router", "settings.personalAgentChooseHost", "personal agent router hermes host selection 个人代理 主机选择 ホスト選択"],
  ["personal_router", "settings.personalAgentEngineLabel", "personal router engine hermes 引擎 エンジン"],
  ["personal_router", "settings.personalAgentAdvancedOptions", "personal router webhook bridge guardian 高级选项 詳細設定"],

  ["safety", "settings.defaultPermissions", "autonomy preset default permission ask auto guardian accept edits full access 默认权限 自律性"],
  ["guardian", "guardian.groupPolicy", "guardian policy permissions approval 审批策略 ガーディアン ポリシー"],
  ["guardian", "guardian.enableTitle", "enable guardian auto approval 自动审批 自動承認"],
  ["guardian", "guardian.thresholdTitle", "auto approve threshold risk 审批阈值 承認しきい値"],
  ["guardian", "guardian.readOnlyTitle", "read only workspace tools 只读工具 読み取り専用"],
  ["guardian", "guardian.workspaceTestsTitle", "workspace build lint test commands 构建测试 ビルドテスト"],
  ["guardian", "guardian.tier2Title", "secondary review medium risk 中风险复核 中リスク再審査"],
  ["guardian", "guardian.groupCustomRules", "trusted prefixes blocked patterns 自定义规则 カスタムルール"],
  ["guardian", "guardian.groupRecentDecisions", "guardian decisions history 审批历史 最近の判定"],
  ["guardian", "permissionRules.sectionTitle", "tool permission rules allow deny 工具权限 允许 拒绝 ツール権限"],
  ["guardian", "securityOps.sandboxModeTitle", "os sandbox mode isolation 沙箱策略 サンドボックス"],
  ["guardian", "securityOps.auditChainTitle", "verify export cryptographic audit chain 审计链验证 監査ログ"],

  ["usage", "usage.dailyUsage", "7d 30d costs tokens chart activity 用量图表 トークン使用量"],
  ["usage", "usage.quotas", "quota capacity burn pace forecast depletion 额度 消耗预测 割当 予測"],

  ["instructions", "settings.subTabInstructions", "AGENTS.md CLAUDE.md instruction files 规则文件 指示ファイル"],
  ["instructions", "context.workspace", "instructions workspace project cwd 项目选择 ワークスペース選択"],
  ["instructions", "context.copyMerged", "copy merged instructions 复制合并指令 統合した指示をコピー"],
  ["rules", "settings.subTabRules", ".orbit/rules path rules glob .cursor/rules 路径规则 パスルール"],
  ["rules", "context.ruleBudget", "path rules token budget 令牌预算 トークン予算"],
  ["memory", "memory.title", "project memory conventions facts 项目记忆 プロジェクトメモリ"],
  ["memory", "memory.searchPlaceholder", "recall search project memories 检索记忆 メモリ検索"],

  ["mcp", "settings.subTabMcp", "mcp servers model context protocol 服务配置 MCP サーバー"],
  ["mcp", "devTools.scope", "mcp scope global user project .mcp.json 全局 项目 スコープ"],
  ["mcp", "devTools.addServer", "add mcp server stdio http transport 添加服务器 サーバー追加"],
  ["hooks", "hooks.title", "lifecycle hook pretooluse posttooluse 生命周期钩子 ライフサイクルフック"],
  ["hooks", "hooks.addHook", "create lifecycle hook 新增钩子 フック追加"],
  ["hooks", "devTools.recentRuns", "hook execution history logs 执行记录 実行履歴"],
  ["lsp", "settings.subTabLsp", "language server lsp rust-analyzer typescript diagnostics 语言服务器 言語サーバー"],

  ["skills", "settings.agentBundledSkills", "builtin orchestration skills orbit advisor committee 编排技能 内蔵スキル"],
  ["skills", "managedSkills.title", "SKILL.md workflows agent skills 自定义技能 エージェントスキル"],
  ["skills", "managedSkills.create", "create skill 创建技能 スキル作成"],
  ["skills", "managedSkills.import", "import skill install path markdown 导入技能 スキルをインポート"],
  ["plugins", "plugins.title", "wasm plugins extensions 插件 プラグイン"],
  ["plugins", "plugins.installSection", "install wasm plugin 安装插件 プラグインをインストール"],

  ["im_channels", "im.configuredChannels", "channel connections lark feishu telegram slack discord wechat 飞书 渠道 チャネル"],
  ["im_channels", "im.addChannel", "add channel bot webhook 添加机器人 チャネル追加"],

  ["archived", "dataOps.archive", "restore archived threads delete 归档 对话恢复 アーカイブ 復元"],
  ["data_diagnostics", "securityOps.migrationGroup", "external ide migration import settings IDE 迁移 移行"],
  ["data_diagnostics", "securityOps.migrationSourceTitle", "migration source external ide 迁移来源 移行元"],
  ["data_diagnostics", "securityOps.migrationConflictTitle", "migration conflict strategy 冲突策略 競合"],
  ["data_diagnostics", "securityOps.diagTitle", "export diagnostic bundle troubleshooting logs 导出诊断包 診断ログ"],
  ["data_diagnostics", "dataOps.resetPreferences", "reset ui preferences restore defaults 重置偏好 恢复默认 既定値に戻す"],
] as const satisfies readonly SettingEntry[];
