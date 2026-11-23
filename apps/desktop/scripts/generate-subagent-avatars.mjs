#!/usr/bin/env node
/**
 * Generates crisp 96x96 transparent-background static PNG portraits for every character in
 * `SUBAGENT_AVATAR_ROSTER` directly from `@oneworks/avatar-react`.
 *
 * Why: `@oneworks/avatar-react`'s live `<Avatar>` runs 28^3 3D Marching Tetrahedra + 420x420 CPU
 * software rasterization on mount and caches only 4 species (`createAvatarCompiledRenderCache(4)`).
 * Pre-baking the resting pose eliminates multi-second main-thread freezes & MB-scale inline SVG
 * DOM bloat on sessions with dozens of completed subagents while preserving 100% visual parity
 * with live animated `<Avatar>` instances.
 *
 * Re-run whenever `SUBAGENT_AVATAR_ROSTER` or the resting pose in `src/lib/subagent-avatar.ts` changes:
 *   node scripts/generate-subagent-avatars.mjs
 */

import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// Stub browser globals required during SSR markup extraction of `@oneworks/avatar-react`.
globalThis.window = {
  matchMedia: () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  }),
};
globalThis.document = {
  visibilityState: "visible",
  addEventListener() {},
  removeEventListener() {},
};

const React = (await import("react")).default;
const { renderToStaticMarkup } = await import("react-dom/server");
const { Avatar } = await import("@oneworks/avatar-react/renderer");
const { SUBAGENT_AVATAR_ROSTER, buildSubagentAvatarDefinition } = await import(
  "../src/lib/subagent-avatar.ts"
);

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const desktopDir = path.resolve(__dirname, "..");
const outDir = path.join(desktopDir, "src", "assets", "subagent-avatars");
const portraitsModulePath = path.join(
  desktopDir,
  "src",
  "components",
  "ui",
  "subagent-avatar-portraits.ts",
);
const RASTER_PX = 128;

function findChromeBinary() {
  const envBin = process.env.CHROME_BIN;
  if (envBin && fs.existsSync(envBin)) return envBin;
  const macCandidates = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
  ];
  for (const c of macCandidates) {
    if (fs.existsSync(c)) return c;
  }
  for (const name of ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]) {
    const res = spawnSync("which", [name], { encoding: "utf8" });
    if (res.status === 0) {
      const resolved = res.stdout.trim().split(/\r?\n/)[0];
      if (resolved && fs.existsSync(resolved)) return resolved;
    }
  }
  throw new Error("Chrome/Chromium binary not found; set CHROME_BIN to regenerate avatars.");
}

async function waitForDevToolsPort(port, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/list`);
      if (res.ok) {
        const list = await res.json();
        const page = list.find((item) => item.type === "page" && item.webSocketDebuggerUrl);
        if (page) return page.webSocketDebuggerUrl;
      }
    } catch {
      // retry
    }
    await new Promise((r) => setTimeout(r, 120));
  }
  throw new Error(`Timed out waiting for Chrome CDP on port ${port}`);
}

class CdpClient {
  constructor(ws) {
    this.ws = ws;
    this.nextId = 1;
    this.pending = new Map();
    ws.addEventListener("message", (event) => {
      const msg = JSON.parse(String(event.data));
      if (msg.id && this.pending.has(msg.id)) {
        const { resolve, reject } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        if (msg.error) reject(new Error(msg.error.message || JSON.stringify(msg.error)));
        else resolve(msg.result);
      }
    });
  }
  send(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
  close() {
    this.ws.close();
  }
}

async function main() {
  fs.mkdirSync(outDir, { recursive: true });

  // 1. Extract exact resting-pose transparent SVG markup from `@oneworks/avatar-react`
  const svgs = [];
  for (const spec of SUBAGENT_AVATAR_ROSTER) {
    const def = buildSubagentAvatarDefinition(spec, "light");
    const html = renderToStaticMarkup(
      React.createElement(Avatar, {
        definition: def,
        interactive: false,
        animation: null,
        autoplay: false,
        theme: "light",
      }),
    );
    const svgStart = html.indexOf("<svg");
    const svgEnd = html.lastIndexOf("</svg>");
    if (svgStart < 0 || svgEnd < 0) {
      throw new Error(`Failed to extract <svg> for character ${spec.id}`);
    }
    let svg = html.slice(svgStart, svgEnd + 6);
    if (!svg.includes('xmlns="http://www.w3.org/2000/svg"')) {
      svg = svg.replace("<svg ", '<svg xmlns="http://www.w3.org/2000/svg" ');
    }
    svgs.push({ id: spec.id, svg });
  }

  // 2. Rasterize each SVG cleanly at 96x96 transparent PNG via headless Chrome
  const chromeBin = findChromeBinary();
  const userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), "orbit-avatar-gen-"));
  const port = 19220 + Math.floor(Math.random() * 500);
  const chrome = spawn(
    chromeBin,
    [
      "--headless=new",
      "--disable-gpu",
      "--no-sandbox",
      "--disable-dev-shm-usage",
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${userDataDir}`,
      "about:blank",
    ],
    { stdio: "ignore" },
  );

  let totalBytes = 0;
  try {
    const wsUrl = await waitForDevToolsPort(port);
    const ws = new WebSocket(wsUrl);
    await new Promise((resolve, reject) => {
      ws.addEventListener("open", resolve, { once: true });
      ws.addEventListener("error", reject, { once: true });
    });
    const cdp = new CdpClient(ws);
    await cdp.send("Runtime.enable");

    for (const entry of svgs) {
      const expr = `(async () => {
        const svg = ${JSON.stringify(entry.svg)};
        const blob = new Blob([svg], { type: "image/svg+xml;charset=utf-8" });
        const url = URL.createObjectURL(blob);
        try {
          const img = new Image();
          img.decoding = "sync";
          await new Promise((resolve, reject) => {
            img.onload = resolve;
            img.onerror = reject;
            img.src = url;
          });
          const canvas = document.createElement("canvas");
          canvas.width = ${RASTER_PX};
          canvas.height = ${RASTER_PX};
          const ctx = canvas.getContext("2d");
          ctx.clearRect(0, 0, ${RASTER_PX}, ${RASTER_PX});
          ctx.imageSmoothingEnabled = true;
          ctx.imageSmoothingQuality = "high";
          ctx.drawImage(img, 0, 0, ${RASTER_PX}, ${RASTER_PX});
          return canvas.toDataURL("image/png");
        } finally {
          URL.revokeObjectURL(url);
        }
      })()`;

      const evalRes = await cdp.send("Runtime.evaluate", {
        expression: expr,
        awaitPromise: true,
        returnByValue: true,
      });
      if (evalRes.exceptionDetails) {
        throw new Error(
          `Chrome rasterize failed for ${entry.id}: ${JSON.stringify(evalRes.exceptionDetails)}`,
        );
      }
      const dataUrl = String(evalRes.result?.value || "");
      const prefix = "data:image/png;base64,";
      if (!dataUrl.startsWith(prefix)) {
        throw new Error(`Unexpected PNG data URL for ${entry.id}`);
      }
      const buf = Buffer.from(dataUrl.slice(prefix.length), "base64");
      totalBytes += buf.length;
      fs.writeFileSync(path.join(outDir, `${entry.id}.png`), buf);
    }

    cdp.close();
  } finally {
    chrome.kill("SIGKILL");
    fs.rmSync(userDataDir, { recursive: true, force: true });
  }

  // 3. Emit typed Vite static import map
  const importLines = [];
  const mapLines = [];
  for (const spec of SUBAGENT_AVATAR_ROSTER) {
    const varName =
      "avatar_" +
      spec.id.replace(/[^a-zA-Z0-9]+(.)/g, (_, ch) => ch.toUpperCase()).replace(/[^a-zA-Z0-9]/g, "");
    importLines.push(`import ${varName} from "@/assets/subagent-avatars/${spec.id}.png";`);
    mapLines.push(`  "${spec.id}": ${varName},`);
  }
  const moduleSource = `// Auto-generated by scripts/generate-subagent-avatars.mjs — do not edit by hand.
${importLines.join("\n")}

export const SUBAGENT_AVATAR_PORTRAITS: Readonly<Record<string, string>> = {
${mapLines.join("\n")}
};
`;
  fs.writeFileSync(portraitsModulePath, moduleSource, "utf8");

  console.log(
    `[generate-subagent-avatars] Generated ${svgs.length} transparent ${RASTER_PX}x${RASTER_PX} PNGs (${(
      totalBytes / 1024
    ).toFixed(1)} KB total) + ${path.relative(desktopDir, portraitsModulePath)}`,
  );
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
