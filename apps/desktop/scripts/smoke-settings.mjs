import assert from "node:assert/strict";

// Runs in the page via CDP. Find controls by their visible name and drive pointer
// and keyboard events through Chrome so focus, portals, and dismissal are real.
function findControl(selector, name) {
  return [...document.querySelectorAll(selector)].find((node) =>
    !name || [node.getAttribute("aria-label"), node.innerText].some(
      (label) => label?.trim().startsWith(name),
    ),
  );
}

async function settingsDriver(cdp) {
  await cdp.send("Runtime.evaluate", { expression: `window.smokeFindControl = ${findControl.toString()}` });
  async function evaluate(fn, ...args) {
    const result = await cdp.send("Runtime.evaluate", {
      awaitPromise: true,
      expression: `(async () => {
        const value = await (${fn.toString()})(${args.map((arg) => JSON.stringify(arg)).join(", ")});
        return value instanceof Node ? true : value;
      })()`,
      returnByValue: true,
    });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    }
    return result.result.value;
  }

  async function waitFor(description, predicate, ...args) {
    const deadline = Date.now() + 4000;
    do {
      const value = await evaluate(predicate, ...args);
      if (value) return value;
      await new Promise((resolve) => setTimeout(resolve, 40));
    } while (Date.now() < deadline);
    const body = await evaluate(() => document.body.innerText.slice(0, 1600));
    throw new Error(`Timed out waiting for ${description}. Page text: ${body}`);
  }

  async function click(selector, name) {
    const point = await waitFor(name || selector, async (selector, name) => {
      const el = window.smokeFindControl(selector, name);
      if (!el || el.disabled) return null;
      el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
      // A search jump or opening drawer may still be moving on the compositor.
      // Click only once the same point hits this control across consecutive frames.
      const rect = el.getBoundingClientRect();
      if (!rect.width || !rect.height) return null;
      const x = rect.left + rect.width / 2;
      const y = rect.top + rect.height / 2;
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const current = el.getBoundingClientRect();
      if (Math.abs(rect.top - current.top) > 0.5 || Math.abs(rect.left - current.left) > 0.5) return null;
      return el.contains(document.elementFromPoint(x, y)) ? { x, y } : null;
    }, selector, name ?? null);
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", ...point });
    for (const type of ["mousePressed", "mouseReleased"]) {
      await cdp.send("Input.dispatchMouseEvent", { type, ...point, button: "left", clickCount: 1 });
    }
  }

  async function key(key, code, modifiers = 0) {
    for (const type of ["rawKeyDown", "keyUp"]) {
      await cdp.send("Input.dispatchKeyEvent", { type, key, code, modifiers });
    }
  }

  async function open() {
    await key(",", "Comma", 4); // The settings passes emulate macOS.
    await waitFor("Settings rail", () => document.querySelectorAll("[data-settings-section]").length === 16);
  }

  async function go(section) {
    await click(`[data-settings-section="${section}"]`);
    await waitFor(`active ${section}`, (section) =>
      document.querySelector(`[data-settings-section="${section}"]`)?.getAttribute("aria-current") === "page", section);
  }

  return { evaluate, waitFor, click, key, open, go };
}

export async function exerciseLocalSettings(cdp) {
  const { click, evaluate, key, open, waitFor, go } = await settingsDriver(cdp);
  const draft = "A draft to keep while configuring Orbit";
  await click("textarea");
  await cdp.send("Input.insertText", { text: draft });
  await evaluate(() => { window.smokeComposer = document.activeElement; });
  const scrollPoint = await waitFor("scrollable conversation history", () => {
    const el = document.querySelector('[data-orbit-stage] [data-slot="scroll-area-viewport"]');
    if (!el || el.scrollTop < 300) return false;
    window.smokeStage = el;
    el.addEventListener("scrollend", () => { window.smokeStageSettled = true; }, { once: true });
    const rect = el.getBoundingClientRect();
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseWheel", deltaX: 0, deltaY: -240, ...scrollPoint });
  const stageOffset = await waitFor("conversation scrolled away from latest", () => {
    const el = window.smokeStage;
    return window.smokeStageSettled && el.scrollHeight - el.clientHeight - el.scrollTop > 100 && el.scrollTop;
  });
  await open();

  await waitFor("full-window Settings and macOS titlebar clearance", () => {
    const shell = document.querySelector('[data-slot="settings-view"]');
    const rect = shell?.getBoundingClientRect();
    const rail = shell?.querySelector("aside");
    const drag = shell?.querySelector("[data-tauri-drag-region]")?.getBoundingClientRect();
    const back = window.smokeFindControl("button", "Back to app")?.getBoundingClientRect();
    return rect?.left === 0 && rect.top === 0 && rect.width === innerWidth &&
      rect.height === innerHeight && rail?.getBoundingClientRect().width === 240 &&
      drag?.top === 0 && drag.height >= 40 && back && (back.top >= 40 || back.left >= 76);
  });
  const railFits = await evaluate(() => [...document.querySelectorAll("[data-settings-section]")].every((btn) => {
    const box = btn.getBoundingClientRect();
    return box.top >= 0 && box.bottom <= innerHeight;
  }));
  assert.ok(railFits, "All 16 destinations should fit in a 14-inch viewport");
  assert.equal(await evaluate(() => document.documentElement.classList.contains("dark")), false);
  const destinations = await evaluate(() =>
    [...document.querySelectorAll("[data-settings-section]")].map((button) => button.dataset.settingsSection));
  for (const section of destinations) {
    await go(section);
    await waitFor(`content header for ${section}`, () => Boolean(document.querySelector("[data-settings-canvas] h1")));
  }
  await go("hosts");
  await waitFor("inline local pairing QR", () =>
    document.querySelector('[data-setting-id="settings.mobilePairing"] img')?.naturalWidth > 0);
  await click('[data-setting-id="settings.fleetConnectHost"]', "Connect remote host");
  await waitFor("host connection drawer", () => Boolean(document.querySelector('[data-slot="setting-drawer"]')));
  await key("Escape", "Escape");
  await waitFor("host overview retained", () => !document.querySelector('[data-slot="setting-drawer"]') &&
    document.querySelector('[data-settings-section="hosts"][aria-current="page"]'));
  await go("voice");
  assert.equal(await evaluate(() => Boolean(document.querySelector('[data-slot="settings-scope-bar"]'))), false);

  await go("general");
  const toggle = '[data-setting-id="settings.sendWithEnter"] [role="switch"]';
  await click(toggle);
  await waitFor("changed Send with Enter preference", (selector) =>
    document.querySelector(selector)?.getAttribute("aria-checked") === "false", toggle);
  await go("data_diagnostics");
  await click('[data-setting-id="dataOps.resetPreferences"] button');
  await waitFor("Reset UI Preferences confirmation", () => Boolean(document.querySelector('[role="alertdialog"]')));
  await key("Escape", "Escape");
  await waitFor("confirmation cancelled without leaving Data & Diagnostics", () =>
    !document.querySelector('[role="alertdialog"]') &&
    document.querySelector('[data-settings-section="data_diagnostics"][aria-current="page"]'));
  await go("appearance");
  await key("f", "KeyF", 4);
  const search = 'input[aria-label="Search settings"]';
  await waitFor("keyboard focus in Settings search", (selector) => document.activeElement?.matches(selector), search);
  await cdp.send("Input.insertText", { text: "@modified" });
  await waitFor("modified Send with Enter result", () =>
    document.querySelector('[data-slot="settings-view"]')?.innerText.includes("Send with Enter"));
  await key("Enter", "Enter");
  await waitFor("jump to modified row", () =>
    document.querySelector('[data-setting-id="settings.sendWithEnter"][data-settings-highlight="true"]') !== null);
  await click('[data-setting-id="settings.sendWithEnter"] button', "Reset");
  await waitFor("restored default", (selector) => document.querySelector(selector)?.getAttribute("aria-checked") === "true", toggle);

  await key("f", "KeyF", 4);
  await cdp.send("Input.insertText", { text: "scrollback" });
  await key("Escape", "Escape");
  await waitFor("Escape clears the query", (selector) => document.querySelector(selector)?.value === "", search);
  await key("f", "KeyF", 4);
  await cdp.send("Input.insertText", { text: "scrollback" });
  await key("Enter", "Enter");
  await waitFor("scrollback row highlighted in view", () => {
    const row = document.querySelector('[data-setting-id="settings.scrollback"][data-settings-highlight="true"]');
    if (!row) return false;
    const rect = row.getBoundingClientRect();
    return rect.top >= 0 && rect.bottom <= innerHeight;
  });
  await click('[data-setting-id="settings.fontSize"] button', "Increase terminal font size");
  await waitFor("terminal preview updates to 14px across four lines", () => {
    const screen = document.querySelector('[data-setting-id="settings.terminalPreview"] > div');
    return screen?.children.length === 4 && getComputedStyle(screen).fontSize === "14px";
  });

  await go("im_channels");
  await click('[data-slot="settings-view"] button', "Add Channel");
  await waitFor("480px channel drawer", () => {
    const box = document.querySelector('[data-slot="setting-drawer"]')?.getBoundingClientRect();
    return box?.width === 480 && Math.abs(box.right - innerWidth) < 1;
  });
  await key("Escape", "Escape");
  await waitFor("drawer closes, Settings stays open", () =>
    !document.querySelector('[data-slot="setting-drawer"]') && document.querySelector('[data-settings-section="im_channels"][aria-current="page"]'));
  await key("Escape", "Escape");
  await waitFor("workspace focus and draft restored", (draft) =>
    !document.querySelector('[data-slot="settings-view"]') && window.smokeComposer === document.activeElement &&
    window.smokeComposer?.value === draft, draft);
  assert.equal(await evaluate(() => window.smokeStage.scrollTop), stageOffset, "Stage should retain its reading position");
}

export async function exerciseMultiHostSettings(cdp) {
  const { click, evaluate, key, open, waitFor, go } = await settingsDriver(cdp);
  await open();
  assert.equal(await evaluate(() => document.documentElement.classList.contains("dark")), true);
  await go("voice");
  const scope = '[data-slot="settings-scope-bar"] button';
  await click(scope);
  await waitFor("remote host available", () =>
    window.smokeFindControl('[role="menuitem"],[role="option"]', "Remote Build Host") !== undefined);
  await key("Escape", "Escape");
  await waitFor("host menu dismisses without exiting Settings", () =>
    !document.querySelector('[role="menu"],[role="listbox"]') && document.querySelector('[data-settings-section="voice"][aria-current="page"]'));
  await click(scope);
  await click('[role="menuitem"],[role="option"]', "Remote Build Host");
  await waitFor("Voice retained while remote host preferences load", () =>
    document.querySelector('[data-settings-section="voice"][aria-current="page"]') &&
    document.querySelector('[data-slot="settings-scope-bar"]')?.innerText.includes("Remote Build Host") &&
    window.smokeVoiceHost === "host:ipv6:2001:db8::2");
  await go("mcp_tools");
  await click('[data-setting-id="devTools.addServer"]');
  await click("#mcp-server-name");
  await cdp.send("Input.insertText", { text: "smoke-server" });
  await click("#mcp-server-command");
  await cdp.send("Input.insertText", { text: "smoke-fixture" });
  await click('[data-slot="setting-drawer"] button[type="submit"]');
  await waitFor("new MCP server displayed", () =>
    !document.querySelector('[data-slot="setting-drawer"]') &&
    document.querySelector('[data-setting-id="settings.subTabMcp"]')?.innerText.includes("smoke-server"));
  await click(scope);
  await click('[role="menuitem"],[role="option"]', "Smoke Host");
  await waitFor("remote MCP server absent from the local catalog", () =>
    document.querySelector('[data-slot="settings-scope-bar"]')?.innerText.includes("Smoke Host") &&
    document.querySelector('[data-setting-id="settings.subTabMcp"]') &&
    !document.querySelector('[data-setting-id="settings.subTabMcp"]').innerText.includes("smoke-server"));
  await go("general");
  assert.equal(await evaluate(() => Boolean(document.querySelector('[data-slot="settings-scope-bar"]'))), false);
  await go("hosts");
  await click("[data-settings-canvas] button", "Remote Build Host");
  await waitFor("remote host detail", () => document.querySelector("[data-settings-canvas] h2")?.innerText === "Remote Build Host");
  const relay = '[data-setting-id="settings.advancedRelayTitle"] [aria-expanded]';
  assert.equal(await evaluate((selector) => document.querySelector(selector)?.getAttribute("aria-expanded"), relay), "false");
  await click(relay);
  await waitFor("advanced relay expanded", (selector) => document.querySelector(selector)?.getAttribute("aria-expanded") === "true", relay);
  await click("[data-settings-canvas] header button", "Hosts & Fleet");
  await waitFor("fleet overview", () => document.querySelector("[data-settings-canvas] h1")?.innerText === "Hosts & Fleet");
}
