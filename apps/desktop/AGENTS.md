# `apps/desktop/` 局部工程规范

在遵守 [`../AGENTS.md`](../AGENTS.md) 的基础上，改动 Desktop 前端或 Tauri 后端时必须满足：

1. **React 19 `useSyncExternalStore` 稳定性红线（严防 `#185` 崩溃）**：
   - `useOrbitSelector` 选择器每次返回的值必须在引用/结构上稳定；**严禁**在 selector 内部直接 `.map()` / `.filter()` / `{ ... }` 每次返回新对象或新数组引用导致无限重渲染循环。
2. **三语 i18n 与文案约束**：
   - 默认文案使用克制英文（`src/lib/i18n/locales/en.ts`），并在同一改动内同步更新 `zh-CN.ts` 与 `ja.ts` 的全部 Key 与占位符（由 `i18n.test.ts` 强制校验）。
3. **Tauri IPC 双向同步**：
   - 修改 `src-tauri/src/` 下的 `#[tauri::command]`、事件枚举或版本号回退时，必须同步更新 `src/lib/orbit-ipc.ts` 与 `scripts/smoke-render.mjs` 中的 mock。
4. **技术栈与 Token 复用**：
   - 遵循 Tailwind CSS v4 + Base UI（`@base-ui/react`）+ 既有语义 CSS Token；复用 `src/components/ui/` 与 `settings-primitives.tsx`，禁止硬编码打破深浅色切换的颜色字面量。
5. **提交前必跑检查**：
   ```bash
   cd apps/desktop
   npx tsc --noEmit
   node --experimental-strip-types --test "src/**/*.test.ts"
   npm run build
   npm run test:smoke   # 3 项 Chrome CDP 场景全过且 consoleErrors=0
   ```
   ```bash
   cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
   cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
   cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
   ```
