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
    await waitFor("Settings rail", () => document.querySelectorAll("[data-settings-section]").length === 17);
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

  await waitFor("centered floating Settings modal with 240px rail and close button", () => {
    const shell = document.querySelector('[data-slot="settings-view"]');
    const rect = shell?.getBoundingClientRect();
    const rail = shell?.querySelector("aside");
    const close = window.smokeFindControl('[data-slot="settings-view"] button', "Close settings");
    return shell?.getAttribute("role") === "dialog" &&
      shell.getAttribute("aria-modal") === "true" &&
      document.activeElement === shell &&
      document.activeElement !== close &&
      rect && rect.left > 0 && rect.top > 0 &&
      rect.right < innerWidth && rect.bottom < innerHeight &&
      rail?.getBoundingClientRect().width === 240 && Boolean(close);
  });
  const buttonFocusSafe = await evaluate(() => {
    const styles = getComputedStyle(document.documentElement);
    const ring = styles.getPropertyValue("--ring").trim();
    const signalInfo = styles.getPropertyValue("--signal-info").trim();
    const close = window.smokeFindControl('[data-slot="settings-view"] button', "Close settings");
    const closeOutline = close ? getComputedStyle(close).outlineStyle : null;
    return Boolean(ring) && ring !== signalInfo && closeOutline === "none";
  });
  assert.ok(buttonFocusSafe, "Buttons should never render a native browser outline or blue signal-info ring");
  const railFits = await evaluate(() => {
    const shellRect = document.querySelector('[data-slot="settings-view"]')?.getBoundingClientRect();
    if (!shellRect) return false;
    return [...document.querySelectorAll("[data-settings-section]")].every((btn) => {
      const box = btn.getBoundingClientRect();
      return box.top >= shellRect.top && box.bottom <= shellRect.bottom;
    });
  });
  assert.ok(railFits, "All 17 destinations should fit inside the Settings modal rail at a 14-inch viewport");
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
  await waitFor("host connection drawer without focusing close button", () => {
    const drawer = document.querySelector('[data-slot="setting-drawer"]');
    return Boolean(drawer) && document.activeElement?.getAttribute("aria-label") !== "Close";
  });
  await key("Escape", "Escape");
  await waitFor("host overview retained", () => !document.querySelector('[data-slot="setting-drawer"]') &&
    document.querySelector('[data-settings-section="hosts"][aria-current="page"]'));
  await go("voice");
  assert.equal(await evaluate(() => Boolean(document.querySelector('[data-slot="settings-scope-bar"]'))), false);
  const voiceDisabledCheck = await evaluate(async () => {
    const sw = document.querySelector('[data-setting-id="voice.autoSpeakTitle"] [role="switch"]');
    const langTrigger = document.querySelector('[data-setting-id="voice.languageTitle"] [data-slot="select-trigger"]');
    if (!sw || !langTrigger) return { ok: false, reason: "missing voice controls" };
    const before = sw.getAttribute("aria-checked");
    const hasDataDisabled = sw.hasAttribute("data-disabled");
    const opacity = Number.parseFloat(getComputedStyle(sw).opacity);
    const langDisabled = Boolean(langTrigger.disabled || langTrigger.hasAttribute("data-disabled"));
    const rect = sw.getBoundingClientRect();
    return { ok: true, before, hasDataDisabled, opacity, langDisabled, x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  });
  assert.ok(voiceDisabledCheck.ok, `Voice disabled check setup failed: ${voiceDisabledCheck.reason}`);
  assert.equal(voiceDisabledCheck.hasDataDisabled, true, "Disabled Voice switch must carry data-disabled");
  assert.ok(voiceDisabledCheck.opacity < 0.6, `Disabled Switch must visually dim via data-[disabled] (got opacity=${voiceDisabledCheck.opacity})`);
  assert.equal(voiceDisabledCheck.langDisabled, true, "Recognition language select must be disabled when Voice is off");
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: voiceDisabledCheck.x, y: voiceDisabledCheck.y });
  for (const type of ["mousePressed", "mouseReleased"]) {
    await cdp.send("Input.dispatchMouseEvent", { type, x: voiceDisabledCheck.x, y: voiceDisabledCheck.y, button: "left", clickCount: 1 });
  }
  assert.equal(
    await evaluate(() => document.querySelector('[data-setting-id="voice.autoSpeakTitle"] [role="switch"]')?.getAttribute("aria-checked")),
    voiceDisabledCheck.before,
    "Clicking a disabled Switch must never toggle its checked state",
  );

  await go("notifications");
  const notifDisabledCheck = await evaluate(() => {
    const permission = typeof Notification !== "undefined" ? Notification.permission : "unsupported";
    const sw = document.querySelector('[data-setting-id="settings.notifyApproval"] [role="switch"]');
    const soundBtn = document.querySelector('[data-setting-id="settings.notificationSoundTitle"] button');
    const mobilePreviewSw = document.querySelector('[data-setting-id="settings.pushMessagePreview"] [role="switch"]');
    if (!sw || !soundBtn || !mobilePreviewSw) return { ok: false, reason: "missing notifications controls" };
    const swRect = sw.getBoundingClientRect();
    const mRect = mobilePreviewSw.getBoundingClientRect();
    return {
      ok: true,
      permission,
      desktopDisabled: sw.hasAttribute("data-disabled"),
      desktopOpacity: Number.parseFloat(getComputedStyle(sw).opacity),
      desktopBefore: sw.getAttribute("aria-checked"),
      soundBtnDisabled: Boolean(soundBtn.disabled),
      desktopPoint: { x: swRect.left + swRect.width / 2, y: swRect.top + swRect.height / 2 },
      mobilePreviewDisabled: mobilePreviewSw.hasAttribute("data-disabled"),
      mobilePreviewOpacity: Number.parseFloat(getComputedStyle(mobilePreviewSw).opacity),
      mobilePreviewBefore: mobilePreviewSw.getAttribute("aria-checked"),
      mobilePoint: { x: mRect.left + mRect.width / 2, y: mRect.top + mRect.height / 2 },
    };
  });
  assert.ok(notifDisabledCheck.ok, `Notifications disabled check setup failed: ${notifDisabledCheck.reason}`);
  if (notifDisabledCheck.permission === "denied" || notifDisabledCheck.permission === "unsupported") {
    assert.equal(notifDisabledCheck.desktopDisabled, true, "Blocked desktop notification switches must be disabled");
    assert.ok(notifDisabledCheck.desktopOpacity < 0.6, `Blocked desktop notification switch must dim (got opacity=${notifDisabledCheck.desktopOpacity})`);
    assert.equal(notifDisabledCheck.soundBtnDisabled, true, "Blocked desktop notification Test Sound button must be disabled");
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", ...notifDisabledCheck.desktopPoint });
    for (const type of ["mousePressed", "mouseReleased"]) {
      await cdp.send("Input.dispatchMouseEvent", { type, ...notifDisabledCheck.desktopPoint, button: "left", clickCount: 1 });
    }
    assert.equal(
      await evaluate(() => document.querySelector('[data-setting-id="settings.notifyApproval"] [role="switch"]')?.getAttribute("aria-checked")),
      notifDisabledCheck.desktopBefore,
      "Clicking a disabled Notifications desktop switch must never toggle its state",
    );
  }
  if (notifDisabledCheck.mobilePreviewDisabled) {
    assert.ok(notifDisabledCheck.mobilePreviewOpacity < 0.6, `Disabled mobile push switch must dim (got opacity=${notifDisabledCheck.mobilePreviewOpacity})`);
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", ...notifDisabledCheck.mobilePoint });
    for (const type of ["mousePressed", "mouseReleased"]) {
      await cdp.send("Input.dispatchMouseEvent", { type, ...notifDisabledCheck.mobilePoint, button: "left", clickCount: 1 });
    }
    assert.equal(
      await evaluate(() => document.querySelector('[data-setting-id="settings.pushMessagePreview"] [role="switch"]')?.getAttribute("aria-checked")),
      notifDisabledCheck.mobilePreviewBefore,
      "Clicking a disabled mobile push switch must never toggle its state",
    );
  }

  const assertNoResetButtons = async (context) => {
    const found = await evaluate(() => ({
      resetButtons: [...document.querySelectorAll('[data-slot="settings-view"] button')]
        .filter((btn) =>
          /\breset\b/i.test(
            `${btn.getAttribute("aria-label") ?? ""} ${btn.getAttribute("title") ?? ""} ${btn.innerText ?? ""}`,
          ),
        )
        .map((btn) => btn.getAttribute("aria-label") || btn.getAttribute("title") || btn.innerText),
      modifiedAttrs: document.querySelectorAll('[data-slot="settings-view"] [data-modified]').length,
    }));
    assert.deepEqual(
      found,
      { resetButtons: [], modifiedAttrs: 0 },
      `Expected no reset-settings controls on ${context}`,
    );
  };

  await go("general");
  assert.equal(
    await evaluate(() => Boolean(
      document.querySelector('[data-setting-id="settings.language"]') &&
      document.querySelector('[data-setting-id="settings.setupGuide"]') &&
      document.querySelector('[data-setting-id="settings.sendWithEnter"]') &&
      document.querySelector('[data-setting-id="settings.activeTurnSendBehavior"]') &&
      document.querySelector('[data-setting-id="settings.appVersion"]') &&
      !document.querySelector('[data-setting-id="settings.isolateWorktrees"]') &&
      !document.querySelector('[data-setting-id="settings.collapseExploration"]') &&
      !document.querySelector('[data-setting-id="settings.externalEditor"]'),
    )),
    true,
    "General should contain only Language, Setup guide, Send shortcut, While agent is running, and App version",
  );
  const sendTrigger = '[data-setting-id="settings.sendWithEnter"] [data-slot="select-trigger"]';
  await click(sendTrigger);
  await click('[data-slot="select-item"]', "⌘ + Enter");
  await waitFor("changed Send shortcut preference", (selector) =>
    document.querySelector(selector)?.innerText.includes("⌘ + Enter"), sendTrigger);
  await assertNoResetButtons("General (after changing Send shortcut)");

  const activeTurnTrigger = '[data-setting-id="settings.activeTurnSendBehavior"] [data-slot="select-trigger"]';
  await click(activeTurnTrigger);
  await click('[data-slot="select-item"]', "Steer");
  await waitFor("changed active-turn send behavior to Steer", (selector) =>
    document.querySelector(selector)?.innerText.includes("Steer"), activeTurnTrigger);
  await click(activeTurnTrigger);
  await click('[data-slot="select-item"]', "Queue");
  await waitFor("restored active-turn send behavior to Queue", (selector) =>
    document.querySelector(selector)?.innerText.includes("Queue"), activeTurnTrigger);

  await go("git_worktrees");
  assert.equal(
    await evaluate(() => Boolean(
      document.querySelector('[data-setting-id="settings.isolateWorktrees"]') &&
      document.querySelector('[data-setting-id="settings.branchPrefix"]') &&
      document.querySelector('[data-setting-id="settings.externalEditor"]'),
    )),
    true,
    "Git & Worktrees should contain Isolate worktrees, Branch prefix, and External editor",
  );

  await go("shortcuts");
  const cmdPaletteRecorder = '[data-setting-id="settings.scCommandPalette"] button';
  await click(cmdPaletteRecorder);
  await waitFor("shortcut recorder active", (selector) =>
    document.querySelector(selector)?.getAttribute("data-orbit-shortcut-recorder") === "true", cmdPaletteRecorder);
  await key("K", "KeyK", 9); // Alt+Shift+K
  await waitFor("custom shortcut badge", () =>
    document.querySelector('[data-setting-id="settings.scCommandPalette"]')?.innerText.includes("Custom"));
  await assertNoResetButtons("Shortcuts (with custom shortcut)");
  await click(cmdPaletteRecorder);
  await waitFor("shortcut recorder re-opened", (selector) =>
    document.querySelector(selector)?.getAttribute("data-orbit-shortcut-recorder") === "true", cmdPaletteRecorder);
  await key("k", "KeyK", 4); // Mod+K back to default
  await waitFor("custom shortcut cleared", () =>
    !document.querySelector('[data-setting-id="settings.scCommandPalette"]')?.innerText.includes("Custom"));

  await go("data_diagnostics");
  assert.equal(
    await evaluate(() => Boolean(document.querySelector('[data-setting-id="dataOps.resetPreferences"]'))),
    false,
    "Data & Diagnostics should not render Reset UI preferences",
  );
  await assertNoResetButtons("Data & Diagnostics");

  await go("appearance");
  assert.equal(
    await evaluate(() => Boolean(document.querySelector('[data-setting-id="settings.collapseExploration"]'))),
    true,
    "Appearance should contain Collapse thinking and tool calls",
  );
  await key("f", "KeyF", 4);
  const search = 'input[aria-label="Search settings"]';
  await waitFor("keyboard focus in Settings search", (selector) => document.activeElement?.matches(selector), search);
  await cdp.send("Input.insertText", { text: "Send shortcut" });
  await waitFor("Send shortcut search result", () =>
    document.querySelector("#settings-results")?.innerText.includes("Send shortcut"));
  await assertNoResetButtons("search results");
  await key("Enter", "Enter");
  await waitFor("jump to Send shortcut row", () =>
    document.querySelector('[data-setting-id="settings.sendWithEnter"][data-settings-highlight="true"]') !== null);
  await click(sendTrigger);
  await click('[data-slot="select-item"]', "Enter");
  await waitFor("restored default Send shortcut", (selector) =>
    document.querySelector(selector)?.innerText.trim() === "Enter", sendTrigger);

  await key("f", "KeyF", 4);
  await cdp.send("Input.insertText", { text: "diff layout" });
  await key("Escape", "Escape");
  await waitFor("Escape clears the query", (selector) => document.querySelector(selector)?.value === "", search);
  await key("f", "KeyF", 4);
  await cdp.send("Input.insertText", { text: "diff layout" });
  await key("Enter", "Enter");
  await waitFor("diff layout row highlighted in view", () => {
    const row = document.querySelector('[data-setting-id="settings.diffLayout"][data-settings-highlight="true"]');
    if (!row) return false;
    const rect = row.getBoundingClientRect();
    return rect.top >= 0 && rect.bottom <= innerHeight;
  });
  assert.equal(
    await evaluate(() => Boolean(document.querySelector('[data-setting-id="settings.terminalTheme"], [data-setting-id="settings.fontSize"], [data-setting-id="settings.cursorStyle"], [data-setting-id="settings.scrollback"], [data-setting-id="settings.terminalPreview"]'))),
    false,
    "Terminal settings should not be exposed in Appearance",
  );

  await go("im_channels");
  await click('[data-slot="settings-view"] button', "Add Channel");
  await waitFor("480px channel drawer contained inside Settings dialog", () => {
    const shell = document.querySelector('[data-slot="settings-view"]')?.getBoundingClientRect();
    const box = document.querySelector('[data-slot="setting-drawer"]')?.getBoundingClientRect();
    return Boolean(shell && box) && box.width === 480 &&
      Math.abs(box.right - shell.right) < 2 &&
      Math.abs(box.top - shell.top) < 2 &&
      Math.abs(box.bottom - shell.bottom) < 2;
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
