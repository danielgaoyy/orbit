import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  DEFAULT_LOCALE,
  LOCALES,
  MESSAGES,
  getLocale,
  interpolate,
  resolveLocale,
  setLocale,
  t,
  translate,
} from "./index.ts";
import { en, type MessageKey } from "./locales/en.ts";

function extractPlaceholders(template: string): string[] {
  const found = new Set<string>();
  const re = /\{([a-zA-Z0-9_]+)\}/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(template)) !== null) {
    found.add(match[1]);
  }
  return Array.from(found).sort();
}

describe("Orbit Desktop i18n", () => {
  it("defaults to restrained English and registers en, zh-CN, and ja", () => {
    assert.equal(DEFAULT_LOCALE, "en");
    assert.deepEqual(
      LOCALES.map((l) => l.id),
      ["en", "zh-CN", "ja"],
    );
    assert.equal(resolveLocale(undefined), "en");
    assert.equal(resolveLocale("fr-FR"), "en");
    assert.equal(resolveLocale("zh-CN"), "zh-CN");
    assert.equal(resolveLocale("zh-Hans"), "zh-CN");
    assert.equal(resolveLocale("ja-JP"), "ja");
  });

  it("maintains 100% key parity and zero empty values across en, zh-CN, and ja", () => {
    const canonicalKeys = Object.keys(en).sort() as MessageKey[];
    assert.ok(canonicalKeys.length > 100, "expected a comprehensive UI message catalog");

    for (const meta of LOCALES) {
      const dict = MESSAGES[meta.id];
      const keys = Object.keys(dict).sort();
      assert.deepEqual(keys, canonicalKeys, `key mismatch in locale ${meta.id}`);

      for (const key of canonicalKeys) {
        const value = dict[key];
        assert.equal(typeof value, "string", `${meta.id}.${key} must be a string`);
        assert.ok(value.trim().length > 0, `${meta.id}.${key} must not be empty`);
      }
    }
  });

  it("maintains 100% placeholder parity across en, zh-CN, and ja", () => {
    const canonicalKeys = Object.keys(en) as MessageKey[];

    for (const key of canonicalKeys) {
      const expected = extractPlaceholders(en[key]);
      for (const meta of LOCALES) {
        const actual = extractPlaceholders(MESSAGES[meta.id][key]);
        assert.deepEqual(
          actual,
          expected,
          `placeholder mismatch for ${meta.id}.${key}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
        );
      }
    }
  });

  it("interpolates placeholders safely and switches locales reactively", () => {
    assert.equal(interpolate("Count: {n}", { n: 0 }), "Count: 0");
    assert.equal(
      interpolate("{a} + {b} = {a}", { a: 1, b: 2 }),
      "1 + 2 = 1",
    );
    assert.equal(interpolate("Keep {unknown}", { other: "x" }), "Keep {unknown}");

    assert.equal(translate("en", "settings.title"), "Settings");
    assert.equal(translate("zh-CN", "settings.title"), "设置");
    assert.equal(translate("ja", "settings.title"), "設定");
    assert.equal(
      translate("zh-CN", "common.showMore", { count: 4 }),
      "展开其余 4 条…",
    );
    assert.equal(
      translate("ja", "common.showMore", { count: 4 }),
      "さらに 4 件表示…",
    );

    const original = getLocale();
    try {
      setLocale("zh-CN");
      assert.equal(getLocale(), "zh-CN");
      assert.equal(t("common.newConversation"), "新建对话");

      setLocale("ja");
      assert.equal(getLocale(), "ja");
      assert.equal(t("common.newConversation"), "新しい会話");

      setLocale("en");
      assert.equal(getLocale(), "en");
      assert.equal(t("common.newConversation"), "New Conversation");
    } finally {
      setLocale(original);
    }
  });
});
