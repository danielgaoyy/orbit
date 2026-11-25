import { useSyncExternalStore } from "react";
import { en, type MessageDictionary, type MessageKey } from "./locales/en.ts";
import { zhCN } from "./locales/zh-CN.ts";
import { ja } from "./locales/ja.ts";
import type { LocaleMeta, OrbitLocale, TranslationParams } from "./types.ts";

export type { LocaleMeta, MessageDictionary, MessageKey, OrbitLocale, TranslationParams };

export const DEFAULT_LOCALE: OrbitLocale = "en";
export const LOCALE_STORAGE_KEY = "orbit.desktop.locale.v1";

/**
 * Extensible locale registry. Adding a future locale (e.g. `ko`, `de`, `fr`)
 * only requires adding a dictionary under `./locales/` and registering one entry
 * in `LOCALES` and `MESSAGES`.
 */
export const LOCALES: readonly LocaleMeta[] = [
  { id: "en", nativeName: "English", englishName: "English", bcp47: "en" },
  { id: "zh-CN", nativeName: "简体中文", englishName: "Simplified Chinese", bcp47: "zh-CN" },
  { id: "ja", nativeName: "日本語", englishName: "Japanese", bcp47: "ja" },
];

export const MESSAGES: Record<OrbitLocale, MessageDictionary> = {
  en,
  "zh-CN": zhCN,
  ja,
};

export function resolveLocale(raw: unknown): OrbitLocale {
  if (typeof raw !== "string") return DEFAULT_LOCALE;
  const trimmed = raw.trim();
  if (trimmed === "en" || trimmed === "zh-CN" || trimmed === "ja") {
    return trimmed;
  }
  const lower = trimmed.toLowerCase();
  if (lower === "zh" || lower.startsWith("zh-cn") || lower.startsWith("zh-hans")) {
    return "zh-CN";
  }
  if (lower === "ja" || lower.startsWith("ja-")) {
    return "ja";
  }
  return DEFAULT_LOCALE;
}

function readStoredLocale(): OrbitLocale {
  if (typeof window === "undefined") return DEFAULT_LOCALE;
  try {
    const direct = window.localStorage.getItem(LOCALE_STORAGE_KEY);
    if (direct) return resolveLocale(direct);
    const prefsRaw = window.localStorage.getItem("orbit.desktop.prefs.v3");
    if (prefsRaw) {
      const parsed = JSON.parse(prefsRaw) as { locale?: unknown };
      if (parsed && typeof parsed.locale === "string") {
        return resolveLocale(parsed.locale);
      }
    }
  } catch {
    // Ignore unavailable storage
  }
  return DEFAULT_LOCALE;
}

function applyDocumentLang(locale: OrbitLocale): void {
  if (typeof document === "undefined") return;
  const meta = LOCALES.find((l) => l.id === locale);
  document.documentElement.lang = meta?.bcp47 ?? "en";
}

let currentLocale: OrbitLocale = readStoredLocale();
applyDocumentLang(currentLocale);

const listeners = new Set<() => void>();

export function getLocale(): OrbitLocale {
  return currentLocale;
}

export function setLocale(next: OrbitLocale): void {
  const resolved = resolveLocale(next);
  if (resolved === currentLocale) return;
  currentLocale = resolved;
  applyDocumentLang(resolved);
  if (typeof window !== "undefined") {
    try {
      window.localStorage.setItem(LOCALE_STORAGE_KEY, resolved);
    } catch {
      // Ignore storage quota errors
    }
  }
  for (const listener of listeners) {
    listener();
  }
}

export function subscribeLocale(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function interpolate(template: string, params?: TranslationParams): string {
  if (!params) return template;
  return template.replace(/\{([a-zA-Z0-9_]+)\}/g, (match, token: string) => {
    const value = params[token];
    return value === undefined ? match : String(value);
  });
}

export function translate(
  locale: OrbitLocale,
  key: MessageKey,
  params?: TranslationParams,
): string {
  const dict = MESSAGES[locale] ?? MESSAGES[DEFAULT_LOCALE];
  const raw = dict[key] ?? MESSAGES[DEFAULT_LOCALE][key] ?? key;
  return interpolate(raw, params);
}

export function t(key: MessageKey, params?: TranslationParams): string {
  return translate(currentLocale, key, params);
}

const TRANSLATORS: Record<
  OrbitLocale,
  (key: MessageKey, params?: TranslationParams) => string
> = {
  en: (key, params) => translate("en", key, params),
  "zh-CN": (key, params) => translate("zh-CN", key, params),
  ja: (key, params) => translate("ja", key, params),
};

const I18N_HANDLES: Record<
  OrbitLocale,
  {
    locale: OrbitLocale;
    setLocale: (next: OrbitLocale) => void;
    t: (key: MessageKey, params?: TranslationParams) => string;
  }
> = {
  en: { locale: "en", setLocale, t: TRANSLATORS.en },
  "zh-CN": { locale: "zh-CN", setLocale, t: TRANSLATORS["zh-CN"] },
  ja: { locale: "ja", setLocale, t: TRANSLATORS.ja },
};

export function useLocale(): OrbitLocale {
  return useSyncExternalStore(subscribeLocale, getLocale, () => DEFAULT_LOCALE);
}

export function getI18nHandle(locale: OrbitLocale) {
  return I18N_HANDLES[locale] ?? I18N_HANDLES[DEFAULT_LOCALE];
}

export function useI18n(): {
  locale: OrbitLocale;
  setLocale: (next: OrbitLocale) => void;
  t: (key: MessageKey, params?: TranslationParams) => string;
} {
  const locale = useLocale();
  return getI18nHandle(locale);
}
