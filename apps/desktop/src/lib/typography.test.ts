import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it } from "node:test";
import {
  CODE_SIZES,
  DEFAULT_CODE_SIZE,
  DEFAULT_READ_SIZE,
  ORBIT_CODE_DEFAULT_STACK,
  ORBIT_TERMINAL_DEFAULT_STACK,
  ORBIT_UI_DEFAULT_STACK,
  READ_SIZES,
  ZOOM_DEFAULT_PERCENT,
  ZOOM_MAX_PERCENT,
  ZOOM_MIN_PERCENT,
  ZOOM_STEP_PERCENT,
  ZOOM_STEPS,
  matchDesktopZoomAction,
  resolveTypography,
  sanitizeFontName,
} from "./typography.ts";

const DESKTOP_ROOT = resolve(import.meta.dirname, "..", "..");

function parseRootCssVars(cssText: string): Map<string, string> {
  const vars = new Map<string, string>();
  const rootBlockRe = /:root\s*\{([^}]*)\}/g;
  let match: RegExpExecArray | null;
  while ((match = rootBlockRe.exec(cssText)) !== null) {
    const body = match[1];
    const declRe = /(--orbit-[a-z0-9-]+)\s*:\s*([^;]+);/g;
    let decl: RegExpExecArray | null;
    while ((decl = declRe.exec(body)) !== null) {
      vars.set(decl[1].trim(), decl[2].replace(/\s+/g, " ").trim());
    }
  }
  return vars;
}

describe("typography engine & CSS contract (F2)", () => {
  it("matches every default cssVars entry to :root in src/index.css", () => {
    const cssText = readFileSync(resolve(DESKTOP_ROOT, "src/index.css"), "utf8");
    const rootVars = parseRootCssVars(cssText);
    const resolved = resolveTypography();

    assert.equal(resolved.isDefault, true);
    assert.equal(resolved.readSizePx, DEFAULT_READ_SIZE);
    assert.equal(resolved.codeFontPx, DEFAULT_CODE_SIZE);
    assert.equal(resolved.codeLinePx, 20);
    assert.equal(resolved.readScale, 1);

    assert.equal(
      Object.prototype.hasOwnProperty.call(resolved.cssVars, "--orbit-font-code-surface"),
      false,
      "Default state must omit --orbit-font-code-surface so :root var(--font-mono) applies",
    );
    assert.equal(rootVars.get("--orbit-font-code-surface"), "var(--font-mono)");
    assert.equal(rootVars.get("--orbit-code-pane-size"), "12px");

    for (const [key, value] of Object.entries(resolved.cssVars)) {
      assert.equal(
        rootVars.get(key),
        value,
        `CSS :root mismatch for ${key}: expected ${JSON.stringify(value)}, got ${JSON.stringify(rootVars.get(key))}`,
      );
    }

    const customCode = resolveTypography({ codeFontId: "fira-code", codeFontSize: 16 });
    assert.equal(customCode.cssVars["--orbit-font-code-surface"], customCode.cssVars["--orbit-font-code"]);
    assert.equal(customCode.cssVars["--orbit-code-pane-size"], `${Math.round((12 * 16) / 13)}px`);
  });

  it("keeps default font stacks character-for-character identical to v0.11.7 baselines", () => {
    assert.equal(
      ORBIT_UI_DEFAULT_STACK,
      '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Hiragino Sans GB", "Inter", "Segoe UI", Roboto, sans-serif',
    );
    assert.equal(
      ORBIT_CODE_DEFAULT_STACK,
      '"JetBrains Mono", "SF Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
    );
    assert.equal(
      ORBIT_TERMINAL_DEFAULT_STACK,
      '"JetBrains Mono", "SF Mono", Menlo, Monaco, Consolas, monospace',
    );
  });

  it("derives strictly integer px values across all read and code size steps", () => {
    const pxVars = [
      "--orbit-read-body",
      "--orbit-read-compact",
      "--orbit-read-h1",
      "--orbit-read-h2",
      "--orbit-read-h3",
      "--orbit-read-table",
      "--orbit-read-th",
      "--orbit-composer-size",
      "--orbit-composer-leading",
      "--orbit-composer-max-h",
      "--orbit-code-size",
      "--orbit-code-pane-size",
      "--orbit-code-line",
    ] as const;

    for (const readSize of READ_SIZES) {
      for (const codeSize of CODE_SIZES) {
        const resolved = resolveTypography({
          conversationFontSize: readSize,
          codeFontSize: codeSize,
        });
        assert.equal(Number.isInteger(resolved.codeLinePx), true);
        for (const key of pxVars) {
          assert.match(
            resolved.cssVars[key],
            /^\d+px$/,
            `${key} at read=${readSize}, code=${codeSize} must be integer px, got ${resolved.cssVars[key]}`,
          );
        }
        assert.equal(resolved.cssVars["--orbit-read-leading"], "1.65");
        assert.equal(resolved.cssVars["--orbit-read-compact-leading"], "1.55");
        assert.equal(resolved.cssVars["--orbit-code-block-leading"], "1.6");
      }
    }
  });

  it("keeps size and zoom steps strictly monotonic", () => {
    for (const steps of [READ_SIZES, CODE_SIZES, ZOOM_STEPS]) {
      for (let i = 1; i < steps.length; i += 1) {
        assert.ok(steps[i] > steps[i - 1], `expected strictly increasing step at index ${i}`);
      }
    }
    assert.equal(READ_SIZES.includes(DEFAULT_READ_SIZE), true);
    assert.equal(CODE_SIZES.includes(DEFAULT_CODE_SIZE), true);
    assert.equal(ZOOM_STEPS[0], ZOOM_MIN_PERCENT);
    assert.equal(ZOOM_STEPS[ZOOM_STEPS.length - 1], ZOOM_MAX_PERCENT);
  });

  it("sanitizes custom font names against CSS injection, control chars, and excessive length", () => {
    const injected = sanitizeFontName('a";}body{color:red');
    assert.ok(injected !== null);
    assert.doesNotMatch(injected, /["'\\;{}<>]/);
    assert.equal(injected, "abodycolor:red");

    assert.equal(sanitizeFontName("\u0000\u0007\u2028\u2029   "), null);
    assert.equal(sanitizeFontName("  Fira\u0000Code\u2028Pro  "), "Fira Code Pro");

    const overlong = sanitizeFontName("A".repeat(200));
    assert.ok(overlong !== null);
    assert.equal(overlong.length, 64);

    assert.equal(sanitizeFontName('"\';{}<>'), null);
    assert.equal(sanitizeFontName(null), null);
  });

  it("matches desktop zoom shortcuts across macOS and non-macOS while rejecting Alt, Shift+Minus, and IME", () => {
    const mac = (overrides: Partial<Parameters<typeof matchDesktopZoomAction>[0]>) =>
      matchDesktopZoomAction(
        {
          key: "",
          code: "",
          metaKey: true,
          ctrlKey: false,
          altKey: false,
          shiftKey: false,
          isComposing: false,
          ...overrides,
        },
        true,
      );
    const win = (overrides: Partial<Parameters<typeof matchDesktopZoomAction>[0]>) =>
      matchDesktopZoomAction(
        {
          key: "",
          code: "",
          metaKey: false,
          ctrlKey: true,
          altKey: false,
          shiftKey: false,
          isComposing: false,
          ...overrides,
        },
        false,
      );

    // Zoom in
    assert.equal(mac({ key: "=", code: "Equal" }), "in");
    assert.equal(mac({ key: "+", code: "Equal", shiftKey: true }), "in");
    assert.equal(mac({ key: "+", code: "NumpadAdd" }), "in");
    assert.equal(win({ key: "=", code: "Equal" }), "in");
    assert.equal(win({ key: "+", code: "Equal", shiftKey: true }), "in");
    assert.equal(win({ key: "+", code: "NumpadAdd" }), "in");

    // Zoom out
    assert.equal(mac({ key: "-", code: "Minus" }), "out");
    assert.equal(mac({ key: "-", code: "NumpadSubtract" }), "out");
    assert.equal(win({ key: "-", code: "Minus" }), "out");
    assert.equal(win({ key: "-", code: "NumpadSubtract" }), "out");

    // Reset (Actual size)
    assert.equal(mac({ key: "0", code: "Digit0" }), "reset");
    assert.equal(mac({ key: "0", code: "Numpad0" }), "reset");
    assert.equal(win({ key: "0", code: "Digit0" }), "reset");
    assert.equal(win({ key: "0", code: "Numpad0" }), "reset");

    // Rejections
    assert.equal(mac({ key: "=", code: "Equal", altKey: true }), null);
    assert.equal(win({ key: "-", code: "Minus", altKey: true }), null);
    assert.equal(mac({ key: "_", code: "Minus", shiftKey: true }), null);
    assert.equal(win({ key: "-", code: "Minus", shiftKey: true }), null);
    assert.equal(mac({ key: ")", code: "Digit0", shiftKey: true }), null);
    assert.equal(mac({ key: "=", code: "Equal", isComposing: true }), null);
    assert.equal(mac({ key: "=", code: "Equal", metaKey: false, ctrlKey: true }), null);
    assert.equal(win({ key: "=", code: "Equal", metaKey: true, ctrlKey: false }), null);
    assert.equal(mac({ key: "=", code: "Equal", metaKey: true, ctrlKey: true }), null);
  });

  it("locks zoom constants in sync with Rust src-tauri/src/zoom.rs", () => {
    const zoomRsPath = resolve(DESKTOP_ROOT, "src-tauri/src/zoom.rs");
    assert.ok(
      existsSync(zoomRsPath),
      `Expected Rust zoom module at ${zoomRsPath} (created by backend track)`,
    );
    const rustSource = readFileSync(zoomRsPath, "utf8");
    const extract = (name: string): number => {
      const m = rustSource.match(new RegExp(`pub\\s+const\\s+${name}\\s*:\\s*u16\\s*=\\s*(\\d+)\\s*;`));
      assert.ok(m, `Expected pub const ${name}: u16 in src-tauri/src/zoom.rs`);
      return Number.parseInt(m[1], 10);
    };

    const min = extract("ZOOM_MIN_PERCENT");
    const max = extract("ZOOM_MAX_PERCENT");
    const step = extract("ZOOM_STEP_PERCENT");
    const def = extract("ZOOM_DEFAULT_PERCENT");

    assert.equal(min, ZOOM_MIN_PERCENT);
    assert.equal(max, ZOOM_MAX_PERCENT);
    assert.equal(step, ZOOM_STEP_PERCENT);
    assert.equal(def, ZOOM_DEFAULT_PERCENT);

    const expectedSteps: number[] = [];
    for (let p = min; p <= max; p += step) {
      expectedSteps.push(p);
    }
    assert.deepEqual([...ZOOM_STEPS], expectedSteps);
  });
});
