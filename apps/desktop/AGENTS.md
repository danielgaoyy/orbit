# `apps/desktop/` 局部工程规范

在遵守 [`../AGENTS.md`](../AGENTS.md) 的基础上，改动 Desktop 前端或 Tauri 后端时必须满足：

1. **React 19 `useSyncExternalStore` 稳定性红线（严防 `#185` 崩溃）**：
   - `useOrbitSelector` 选择器每次返回的值必须在引用/结构上稳定；**严禁**在 selector 内部直接 `.map()` / `.filter()` / `{ ... }` 每次返回新对象或新数组引用导致无限重渲染循环。
2. **三语 i18n 与文案约束**：
   - 默认文案使用克制英文（`src/lib/i18n/locales/en.ts`），并在同一改动内同步更新 `zh-CN.ts` 与 `ja.ts` 的全部 Key 与占位符（由 `i18n.test.ts` 强制校验）。
3. **Tauri IPC 双向同步**：
   - 修改 `src-tauri/src/` 下的 `#[tauri::command]`、事件枚举或版本号回退时，必须同步更新 `src/lib/orbit-ipc.ts` 与 `scripts/smoke-render.mjs` 中的 mock。
4. **技术栈、shadcn/ui 优先与 Token 复用**：
   - 遵循 Tailwind CSS v4 + shadcn/ui（Base UI `@base-ui/react`）+ 既有语义 CSS Token；动手前先读 [`.agents/skills/shadcn/SKILL.md`](../../.agents/skills/shadcn/SKILL.md) 并检查 `src/components/ui/`（如 `Button`、`Input`、`FieldGroup`/`Field`、`Dialog`、`DropdownMenu`、`IconButton`、`OrbitSpinner`）与 `settings-primitives.tsx`，优先复用并组合官方组件，禁止重复手写原生基础控件或硬编码打破深浅色切换的颜色字面量。
   - **根字号换算注意**：Desktop `html { font-size: 13px }`，Tailwind `rem` 类（如 `text-xs` = `9.75px`、`size-8.5` = `27.625px`）会按 `13px` 缩放；凡设计系统规定了精确像素（主对话区三档整数 `15/13/11px`、按钮高宽、行对齐间距），一律使用显式 `px` 令牌或校准后的语义类。
5. **全局同源与容器联动自检（改动 Stage / 侧栏 / 滚动 / 行组件必做）**：
   - **平行视图同源**：凡修改回合间距、折叠锚定、子代理/消息行、滚动吸底或字阶，必须同步检查 `StreamStage`、`SubagentTab` / `ConversationViewport` 以及所有复用 `TurnStageSection` / `PeerMessageRow` 的调用点，用同一共享原语/工具函数驱动，禁止在旁路容器里硬编码第二套间距或 padding。
   - **相邻容器联动**：凡修改底部停靠区（`DecisionDock`、`ComposerPlanShelf`、`ComposerQueueStrip`、`OmniComposer`）的显隐或展开收起，必须核验对上方 `ScrollArea` 的 `clientHeight` 变化、吸底锁与 `scrollTop` 钳位的影响；展开收起型组件需核验点击外部判定是否误绑到全宽外层容器。
   - **子代理交付必复核**：多子代理并行收口后，主 Agent 必须逐项复查生命周期边界（如完成态是否自动退场、跨轮残留状态、新数据到达是否恢复展示）与全流视觉一致性，发现同类隐患主动提出方案征询用户是否一并优化。
6. **提交前必跑检查**：
   ```bash
   cd apps/desktop
   npx tsc -p tsconfig.app.json --noEmit
   node --experimental-strip-types --test "src/**/*.test.ts"
   npm run build
   npm run test:smoke   # 5 项 Chrome CDP 场景全过且 consoleErrors=0
   ```
   ```bash
   cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
   cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
   cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
   ```
