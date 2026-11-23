export type OrbitLocale = "en" | "zh-CN" | "ja";

export interface LocaleMeta {
  readonly id: OrbitLocale;
  readonly nativeName: string;
  readonly englishName: string;
  readonly bcp47: string;
}

export type TranslationParams = Record<string, string | number>;
