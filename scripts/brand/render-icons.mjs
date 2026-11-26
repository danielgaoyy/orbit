#!/usr/bin/env node
// Rasterize docs/assets/brand/*.svg into every platform icon slot.
// Usage (needs `sharp`, already a dependency of apps/docs):
//   cd apps/docs && npm ci && cd ../.. && node scripts/brand/render-icons.mjs
// Desktop .icns/.ico are produced afterwards by `npx tauri icon` (see docs/assets/brand/README.md).
import { createRequire } from "node:module";
import { readFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const sharp = createRequire(join(root, "apps/docs/package.json"))("sharp");
const brand = (f) => readFileSync(join(root, "docs/assets/brand", f));

const macIcon = brand("orbit-app-icon.svg");
const fullBleed = brand("orbit-app-icon-fullbleed.svg");
const tray = brand("orbit-tray-template.svg");

async function png(svg, size, out, { opaque = false, height } = {}) {
  // Square art rasterizes at its native 1024px then downsamples; the cropped tray art is
  // tiny (53×36) so it is rendered at 4× density before being reduced to 18/36px tall.
  let img = sharp(svg, { density: height ? 288 : 72 }).resize({ width: height ? null : size, height });
  if (opaque) img = img.flatten({ background: "#121318" }).removeAlpha();
  const file = join(root, out);
  mkdirSync(dirname(file), { recursive: true });
  await img.png({ compressionLevel: 9 }).toFile(file);
  console.log("wrote", out);
}

const jobs = [
  // Desktop (Tauri) PNG slots + source for `tauri icon`
  [macIcon, 1024, "apps/desktop/src-tauri/icons/app-icon-1024.png"],
  [macIcon, 512, "apps/desktop/src-tauri/icons/icon.png"],
  [macIcon, 32, "apps/desktop/src-tauri/icons/32x32.png"],
  [macIcon, 64, "apps/desktop/src-tauri/icons/64x64.png"],
  [macIcon, 128, "apps/desktop/src-tauri/icons/128x128.png"],
  [macIcon, 256, "apps/desktop/src-tauri/icons/128x128@2x.png"],
  [macIcon, 256, "apps/desktop/public/orbit-logo.png"],
  // Windows Store tiles
  ...[30, 44, 71, 89, 107, 142, 150, 284, 310].map((s) => [macIcon, s, `apps/desktop/src-tauri/icons/Square${s}x${s}Logo.png`]),
  [macIcon, 50, "apps/desktop/src-tauri/icons/StoreLogo.png"],
  // Android legacy launcher (OS adds no mask for legacy icons → keep squircle with alpha)
  ...[["mdpi", 48], ["hdpi", 72], ["xhdpi", 96], ["xxhdpi", 144], ["xxxhdpi", 192]].map(([d, s]) => [macIcon, s, `apps/android/app/src/main/res/mipmap-${d}/ic_launcher.png`]),
  // HarmonyOS (system applies its own mask)
  [fullBleed, 512, "apps/harmony/AppScope/resources/base/media/app_icon.png"],
  [fullBleed, 512, "apps/harmony/entry/src/main/resources/base/media/startIcon.png"],
];
for (const [svg, size, out] of jobs) await png(svg, size, out);

// iOS: opaque full-bleed, sizes from Contents.json
const iosDir = "apps/ios/Sources/Resources/Assets.xcassets/AppIcon.appiconset";
const { images } = JSON.parse(readFileSync(join(root, iosDir, "Contents.json"), "utf8"));
const seen = new Set();
for (const { size, scale, filename } of images) {
  if (seen.has(filename)) continue;
  seen.add(filename);
  const px = Math.round(parseFloat(size) * parseInt(scale, 10));
  await png(fullBleed, px, `${iosDir}/${filename}`, { opaque: true });
}

// macOS menu bar template: 36px tall (18pt @2x); tray-icon scales it to 18pt.
await png(tray, 0, "apps/desktop/src-tauri/icons/tray-template@2x.png", { height: 36 });
