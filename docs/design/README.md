# Orbit UI 与交互设计总入口

本目录是 `apps/` 下桌面端（Desktop）与原生移动端（iOS / Android / HarmonyOS）视觉、布局、样式、状态反馈与交互细节的设计总入口。

---

## 1. 动手改 UI 前的强制前置清单

凡修改 `apps/` 下任何页面、组件、弹窗、设置项、导航、动效或样式 Token 前，**必须先完成以下步骤**：

1. **完整读取 [`.agents/skills/interface-design/SKILL.md`](../../.agents/skills/interface-design/SKILL.md)**（PR #40 引入的官方 Craft-First 界面设计 Skill）。
   - 严禁依赖通用模板默认风格；每次构建新组件前，需明确意图（Intent）、唯一视觉焦点（Hierarchy）、配色逻辑（Palette）、深度策略（Depth）、表面层级（Surfaces）、字阶/字重（Typography）与栅格密度（Spacing）。
2. **检查设计记忆文件 `.interface-design/system.md`**：
   - 若仓库中已存在 `.interface-design/system.md`，必须一并读取并严格复用其中已确定的间距基准、色板 Token、圆角/阴影层级与组件模式，防止风格漂移。
3. **跨端组件对齐**：
   - 同时阅读 [`docs/CLIENT_BRIDGE_ARCHITECTURE.md`](../CLIENT_BRIDGE_ARCHITECTURE.md)，确保业务数据（Sidebar、Stage、模型列表、Slash/Mention、Diagram 渲染等）一律由 `orbit-client-core` 计算下发，客户端只负责薄层原生渲染与平台手势/动效。

---

## 2. 技术栈与端内差异基准

- **Desktop (`apps/desktop`)**：React 19 + TypeScript + Tailwind CSS v4 + Base UI（`@base-ui/react` 语义原语）+ Lucide Icons + `@xterm/xterm` WebGL。优先复用 `src/components/ui/`、`settings-primitives.tsx` 等现有组件与语义 Token，禁止在各页面裸写重复长 Tailwind 串或硬编码色值（确保 Light/Dark 模式完整一致）。
- **Desktop 多语言文案（i18n）铁律**：默认 UI 文案采用克制、精准的英文（`apps/desktop/src/lib/i18n/locales/en.ts`），并在同一次修改中保持 `zh-CN.ts` 与 `ja.ts` 的 Key 与插值占位符 100% 对齐（由 `i18n.test.ts` 自动检查）。
- **原生移动端（`apps/ios`、`apps/android`、`apps/harmony`）**：遵循各自平台原生交互范式（SwiftUI / Jetpack Compose M3 / ArkUI V2），仅在原生生命周期、选择器、触感反馈与原生导航动画处做平台原生增强，保持与 Desktop 核心信息架构一致。

---

## 3. UI 改动提交前验证

- **Desktop**：`npx tsc --noEmit` + `node --experimental-strip-types --test "src/**/*.test.ts"` + `npm run build` + `npm run test:smoke`（无头 Chrome CDP 3 项场景必须全部 `PASS` 且 `consoleErrors=0`）。
- **iOS**：`xcodegen generate`（如有结构调整）+ `OrbitMobileLogicTests` 全绿。
- **Android / HarmonyOS**：按对应目录 `AGENTS.md` 执行构建校验。
