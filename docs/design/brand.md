# Orbit 品牌标识与图标资产

Orbit 品牌标识为 **"Sliced O"**：一枚粗环 "O" 被 `-28°` 倾斜轨道穿过，轨道前段遮挡圆环，右上角一颗 success 绿（`#3ED598` / 浅色 `#0F7B55`）卫星。配色遵循 [`.interface-design/system.md`](../../.interface-design/system.md) 的单色 + 语义色约束。

## 1. 唯一源文件（`docs/assets/brand/`）

| 文件 | 用途 |
| --- | --- |
| `orbit-app-icon.svg` | macOS / Windows / Linux 应用图标（1024 画布内 824px 圆角方形，含投影与高光描边） |
| `orbit-app-icon-fullbleed.svg` | 满版不透明方形，供 iOS / Android / HarmonyOS 由系统自行裁切 |
| `orbit-mark.svg` | `currentColor` 平面标识（文档站 `OrbitMark.astro` 与 favicon 同源） |
|  `orbit-mark-dark.svg` / `orbit-mark-light.svg` | 深色 / 浅色背景定色标识 |
| `orbit-favicon.svg` | 深色小方块 Favicon（`apps/docs/public/favicon.svg`），在深浅标签栏均可见 |
| `orbit-tray-template.svg` | macOS 菜单栏 Template 图像：纯黑 + Alpha，由 AppKit 按明暗菜单栏与高亮态自动着色 |

上述 SVG 由 [`scripts/brand/generate-brand-svgs.py`](../../scripts/brand/generate-brand-svgs.py) 参数化生成，**禁止手改单个平台 PNG**。

## 2. 重新生成全部平台图标

```bash
python3 scripts/brand/generate-brand-svgs.py docs/assets/brand
(cd apps/docs && npm ci) && node scripts/brand/render-icons.mjs
# Desktop .icns / .ico（tauri icon 输出到临时目录后只取这两个文件）
(cd apps/desktop && npx tauri icon src-tauri/icons/app-icon-1024.png -o /tmp/orbit-tauri-icons)
cp /tmp/orbit-tauri-icons/icon.{icns,ico} apps/desktop/src-tauri/icons/
```

`render-icons.mjs` 覆盖：Tauri PNG / Windows Store Tile、`apps/desktop/public/orbit-logo.png`（Web PWA 复用）、Android `mipmap-*`、HarmonyOS `app_icon` / `startIcon`、iOS `AppIcon.appiconset`（按 `Contents.json` 尺寸，去 Alpha），以及 `apps/desktop/src-tauri/icons/tray-template@2x.png`。

## 3. 菜单栏图标约束（macOS）

- `tray.rs` 在 macOS 上使用 `include_image!("./icons/tray-template@2x.png")` 并 `icon_as_template(true)`；其他平台继续使用彩色窗口图标。
- Template 图像只能包含黑色与透明度，高度 36px（18pt @2x），已裁切到标识外框，使视觉高度与系统状态图标一致。
- 卫星处在 Template 中以镂空描边与圆环分离，保证单色下仍可辨识。
