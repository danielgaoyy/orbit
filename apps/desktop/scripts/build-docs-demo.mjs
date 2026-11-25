#!/usr/bin/env node
/**
 * Builds the real Orbit Desktop React + Vite application into
 * `apps/docs/public/desktop-demo/` with a self-contained mock Tauri IPC bridge
 * so the marketing website Hero embeds the pixel-identical live Orbit Desktop UI
 * (with right side panel closed by default and interactive sidebar/diff/approval state).
 */
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { buildShowcaseFixture } from "./capture-showcase-screenshots.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DESKTOP_DIR = path.resolve(__dirname, "..");
const DEMO_OUT_DIR = path.resolve(DESKTOP_DIR, "../docs/public/desktop-demo");

console.log("[build-docs-demo] Building real apps/desktop Vite bundle into apps/docs/public/desktop-demo...");
const buildRes = spawnSync(
  "npx",
  ["vite", "build", "--outDir", DEMO_OUT_DIR, "--emptyOutDir"],
  {
    cwd: DESKTOP_DIR,
    env: { ...process.env, ORBIT_DESKTOP_BASE: "./" },
    stdio: "inherit",
  },
);

if (buildRes.status !== 0) {
  console.error("[build-docs-demo] Vite build failed.");
  process.exit(buildRes.status ?? 1);
}

const ipcScript = buildShowcaseFixture("dark", { interactiveDemo: true });
fs.writeFileSync(path.join(DEMO_OUT_DIR, "showcase-ipc.js"), ipcScript, "utf8");

const indexPath = path.join(DEMO_OUT_DIR, "index.html");
const rawHtml = fs.readFileSync(indexPath, "utf8");
const injectedHtml = rawHtml.replace(
  /<head>/i,
  `<head>\n    <meta name="robots" content="noindex, nofollow" />\n    <script src="./showcase-ipc.js"></script>`,
);

if (injectedHtml === rawHtml) {
  throw new Error("[build-docs-demo] Failed to inject showcase-ipc.js into index.html");
}

fs.writeFileSync(indexPath, injectedHtml, "utf8");
console.log("[build-docs-demo] Ready:", path.relative(path.resolve(DESKTOP_DIR, "../.."), indexPath));
