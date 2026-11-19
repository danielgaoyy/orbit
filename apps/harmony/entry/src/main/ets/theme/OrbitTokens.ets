import { ColorMetrics, CustomColors, CustomTheme } from '@kit.ArkUI';
import { common2D, drawing } from '@kit.ArkGraphics2D';
import { orbitLogWarn } from '../model/OrbitLogger';

class OrbitCustomColors implements CustomColors {
  public brand: ResourceColor = $r('app.color.orbit_brand');
  public fontEmphasize: ResourceColor = $r('app.color.orbit_brand');
  public iconEmphasize: ResourceColor = $r('app.color.orbit_brand');
  public iconSubEmphasize: ResourceColor = $r('app.color.orbit_brand');
  public backgroundEmphasize: ResourceColor = $r('app.color.orbit_brand');
  public compBackgroundEmphasize: ResourceColor = $r('app.color.orbit_brand');
  public compEmphasizeSecondary: ResourceColor = $r('app.color.orbit_accent_soft');
  public compEmphasizeTertiary: ResourceColor = $r('app.color.orbit_accent_soft');
  public fontOnPrimary: ResourceColor = $r('app.color.orbit_on_brand');
  public iconOnPrimary: ResourceColor = $r('app.color.orbit_on_brand');
  public interactiveFocus: ResourceColor = $r('app.color.orbit_brand');
  public interactiveActive: ResourceColor = $r('app.color.orbit_brand');
}

class OrbitBrandTheme implements CustomTheme {
  public colors: CustomColors = new OrbitCustomColors();
}

export const ORBIT_DEFAULT_THEME: CustomTheme = new OrbitBrandTheme();

export function orbitColorMetrics(color: ResourceColor): ColorMetrics | undefined {
  try {
    return ColorMetrics.resourceColor(color);
  } catch (err) {
    orbitLogWarn('OrbitTokens', 'Failed to convert ColorMetrics', err as Object);
    return undefined;
  }
}

export function orbitSegmentSelectedBg(palette: OrbitThemePalette): ColorMetrics | undefined {
  return orbitColorMetrics(palette.isDark ? '#F4F4F6' : '#141416');
}

export function orbitSegmentSelectedFg(palette: OrbitThemePalette): ColorMetrics | undefined {
  return orbitColorMetrics(palette.isDark ? '#141416' : '#FFFFFF');
}

export function orbitTintColorFilter(hexColor: string): drawing.ColorFilter | undefined {
  try {
    const clean = hexColor.startsWith('#') ? hexColor.substring(1) : hexColor;
    let alpha = 255;
    let red = 0;
    let green = 0;
    let blue = 0;
    if (clean.length === 8) {
      alpha = parseInt(clean.substring(0, 2), 16);
      red = parseInt(clean.substring(2, 4), 16);
      green = parseInt(clean.substring(4, 6), 16);
      blue = parseInt(clean.substring(6, 8), 16);
    } else if (clean.length === 6) {
      red = parseInt(clean.substring(0, 2), 16);
      green = parseInt(clean.substring(2, 4), 16);
      blue = parseInt(clean.substring(4, 6), 16);
    } else {
      return undefined;
    }
    const argb: common2D.Color = {
      alpha: isNaN(alpha) ? 255 : alpha,
      red: isNaN(red) ? 0 : red,
      green: isNaN(green) ? 0 : green,
      blue: isNaN(blue) ? 0 : blue,
    };
    return drawing.ColorFilter.createBlendModeColorFilter(argb, drawing.BlendMode.SRC_IN);
  } catch (err) {
    orbitLogWarn('OrbitTokens', 'Failed to create ColorFilter', err as Object);
    return undefined;
  }
}

export class OrbitTokens {
  // HarmonyOS 7 Official Typography Scale ($r('sys.float.*'))
  public static readonly FONT_TITLE_RES: Resource = $r('sys.float.Title_S');       // 20fp
  public static readonly FONT_SUBTITLE_RES: Resource = $r('sys.float.Subtitle_M'); // 16fp
  public static readonly FONT_BODY_RES: Resource = $r('sys.float.Body_L');         // 16fp
  public static readonly FONT_SUBHEAD_RES: Resource = $r('sys.float.Body_M');      // 14fp
  public static readonly FONT_META_RES: Resource = $r('sys.float.Body_S');         // 12fp
  public static readonly FONT_CAPTION_RES: Resource = $r('sys.float.Caption_M');   // 10fp

  // Numeric equivalents strictly for Span / LengthMetrics APIs that require numeric fp
  public static readonly FONT_TITLE: number = 20;
  public static readonly FONT_BODY: number = 16;
  public static readonly FONT_SUBHEAD: number = 14;
  public static readonly FONT_META: number = 12;

  public static readonly MAX_FONT_SCALE_CHROME: number = 1.45;
  public static readonly MAX_FONT_SCALE_BODY: number = 2.0;

  public static readonly FAMILY_MONO: string = 'HarmonyOS Sans Mono';

  // HarmonyOS 7 Spatial Grid (vp)
  public static readonly SPACE_XS: number = 4;
  public static readonly SPACE_SM: number = 8;
  public static readonly SPACE_MD: number = 12;
  public static readonly SPACE_LG: number = 16;
  public static readonly SPACE_XL: number = 24;

  // HarmonyOS 7 Corner Radius Tokens ($r('sys.float.corner_radius_level*'))
  public static readonly RADIUS_XS: Resource = $r('sys.float.corner_radius_level2');  // 4vp
  public static readonly RADIUS_SM: Resource = $r('sys.float.corner_radius_level4');  // 8vp
  public static readonly RADIUS_MD: Resource = $r('sys.float.corner_radius_level6');  // 12vp
  public static readonly RADIUS_LG: Resource = $r('sys.float.corner_radius_level8');  // 16vp
  public static readonly RADIUS_XL: Resource = $r('sys.float.corner_radius_level10'); // 20vp
  public static readonly RADIUS_XXL: Resource = $r('sys.float.corner_radius_level12'); // 24vp

  // Touch Targets & Chrome Metrics (HarmonyOS 7: 48vp primary / 40vp compact)
  public static readonly HIT_MIN: number = 48;
  public static readonly HIT_COMPACT: number = 40;
  public static readonly TOP_BAR_HEIGHT: number = 56;
  public static readonly DOT_SIZE: number = 6;
  public static readonly HAIRLINE: number = 0.5;

  // Multi-Form-Factor Breakpoints
  public static readonly BP_FOLDABLE_MIN: number = 700;
  public static readonly BP_EXPANDED_MIN: number = 1024;
  public static readonly SIDEBAR_FOLD_WIDTH: number = 280;
  public static readonly DIFF_PANE_EXPANDED_WIDTH: number = 360;
}

/**
 * Pure HarmonyOS 7 native semantic palette backed exclusively by `$r('sys.color.*')`
 * and dual `base/` + `dark/` `$r('app.color.*')` resources so `configColorModeChangePerformanceInArkUI`
 * performs zero-JS-rebuild dark/light transitions natively.
 */
export interface OrbitThemePalette {
  id: string;
  isDark: boolean;
  canvas: ResourceColor;
  drawer: ResourceColor;
  elevated: ResourceColor;
  listCard: ResourceColor;
  code: ResourceColor;
  subtleSurface: ResourceColor;
  userBubble: ResourceColor;
  fgPrimary: ResourceColor;
  fgSecondary: ResourceColor;
  fgMuted: ResourceColor;
  iconPrimary: ResourceColor;
  iconSecondary: ResourceColor;
  hairline: ResourceColor;
  accentPrimary: ResourceColor;
  accentPrimaryFill: ResourceColor;
  accentPrimarySoft: ResourceColor;
  onAccent: ResourceColor;
  statusDirect: ResourceColor;
  statusDirectSoft: ResourceColor;
  statusRelay: ResourceColor;
  statusRelaySoft: ResourceColor;
  statusWarning: ResourceColor;
  statusWarningSoft: ResourceColor;
  statusDanger: ResourceColor;
  statusDangerSoft: ResourceColor;
  diffAddedBg: ResourceColor;
  diffRemovedBg: ResourceColor;
  terminalBg: ResourceColor;
  terminalFg: ResourceColor;
  unreadBadge: ResourceColor;
  scrim: ResourceColor;
  overlayPillBg: ResourceColor;
  // Raw hex strings strictly for non-Resource APIs (drawing.ColorFilter)
  tintPrimaryHex: string;
  tintMutedHex: string;
  tintDirectHex: string;
  tintWarningHex: string;
  tintDangerHex: string;
}

const SHARED_NATIVE_COLORS_LIGHT: OrbitThemePalette = {
  id: 'light',
  isDark: false,
  canvas: $r('sys.color.background_primary'),
  drawer: $r('sys.color.background_secondary'),
  elevated: $r('sys.color.comp_background_primary'),
  listCard: $r('sys.color.comp_background_list_card'),
  code: $r('app.color.orbit_code_bg'),
  subtleSurface: $r('sys.color.comp_background_tertiary'),
  userBubble: $r('sys.color.comp_background_list_card'),
  fgPrimary: $r('sys.color.font_primary'),
  fgSecondary: $r('sys.color.font_secondary'),
  fgMuted: $r('sys.color.font_tertiary'),
  iconPrimary: $r('sys.color.icon_primary'),
  iconSecondary: $r('sys.color.icon_secondary'),
  hairline: $r('sys.color.comp_divider'),
  accentPrimary: $r('app.color.orbit_brand'),
  accentPrimaryFill: $r('app.color.orbit_brand'),
  accentPrimarySoft: $r('app.color.orbit_accent_soft'),
  onAccent: $r('app.color.orbit_on_brand'),
  statusDirect: $r('sys.color.confirm'),
  statusDirectSoft: $r('app.color.orbit_status_direct_soft'),
  statusRelay: $r('app.color.orbit_status_relay'),
  statusRelaySoft: $r('app.color.orbit_status_relay_soft'),
  statusWarning: $r('sys.color.warning'),
  statusWarningSoft: $r('app.color.orbit_status_warning_soft'),
  statusDanger: $r('sys.color.alert'),
  statusDangerSoft: $r('app.color.orbit_status_danger_soft'),
  diffAddedBg: $r('app.color.orbit_diff_added_bg'),
  diffRemovedBg: $r('app.color.orbit_diff_removed_bg'),
  terminalBg: $r('app.color.orbit_terminal_bg'),
  terminalFg: $r('app.color.orbit_terminal_fg'),
  unreadBadge: $r('app.color.orbit_badge_unread'),
  scrim: $r('app.color.orbit_scrim'),
  overlayPillBg: $r('app.color.orbit_overlay_pill_bg'),
  tintPrimaryHex: '#E6000000',
  tintMutedHex: '#99000000',
  tintDirectHex: '#FF64BB5C',
  tintWarningHex: '#FFED6F21',
  tintDangerHex: '#FFE84026'
};

const SHARED_NATIVE_COLORS_DARK: OrbitThemePalette = {
  id: 'dark',
  isDark: true,
  canvas: $r('sys.color.background_primary'),
  drawer: $r('sys.color.background_secondary'),
  elevated: $r('sys.color.comp_background_primary'),
  listCard: $r('sys.color.comp_background_list_card'),
  code: $r('app.color.orbit_code_bg'),
  subtleSurface: $r('sys.color.comp_background_tertiary'),
  userBubble: $r('sys.color.comp_background_list_card'),
  fgPrimary: $r('sys.color.font_primary'),
  fgSecondary: $r('sys.color.font_secondary'),
  fgMuted: $r('sys.color.font_tertiary'),
  iconPrimary: $r('sys.color.icon_primary'),
  iconSecondary: $r('sys.color.icon_secondary'),
  hairline: $r('sys.color.comp_divider'),
  accentPrimary: $r('app.color.orbit_brand'),
  accentPrimaryFill: $r('app.color.orbit_brand'),
  accentPrimarySoft: $r('app.color.orbit_accent_soft'),
  onAccent: $r('app.color.orbit_on_brand'),
  statusDirect: $r('sys.color.confirm'),
  statusDirectSoft: $r('app.color.orbit_status_direct_soft'),
  statusRelay: $r('app.color.orbit_status_relay'),
  statusRelaySoft: $r('app.color.orbit_status_relay_soft'),
  statusWarning: $r('sys.color.warning'),
  statusWarningSoft: $r('app.color.orbit_status_warning_soft'),
  statusDanger: $r('sys.color.alert'),
  statusDangerSoft: $r('app.color.orbit_status_danger_soft'),
  diffAddedBg: $r('app.color.orbit_diff_added_bg'),
  diffRemovedBg: $r('app.color.orbit_diff_removed_bg'),
  terminalBg: $r('app.color.orbit_terminal_bg'),
  terminalFg: $r('app.color.orbit_terminal_fg'),
  unreadBadge: $r('app.color.orbit_badge_unread'),
  scrim: $r('app.color.orbit_scrim'),
  overlayPillBg: $r('app.color.orbit_overlay_pill_bg'),
  tintPrimaryHex: '#DBFFFFFF',
  tintMutedHex: '#99FFFFFF',
  tintDirectHex: '#FF64BB5C',
  tintWarningHex: '#FFED6F21',
  tintDangerHex: '#FFE84026'
};

export const LIGHT_PALETTE: OrbitThemePalette = SHARED_NATIVE_COLORS_LIGHT;
export const DARK_PALETTE: OrbitThemePalette = SHARED_NATIVE_COLORS_DARK;

export function resolveOrbitPalette(themeMode: string, systemIsDark: boolean): OrbitThemePalette {
  if (themeMode === 'light') {
    return LIGHT_PALETTE;
  }
  if (themeMode === 'dark') {
    return DARK_PALETTE;
  }
  return systemIsDark ? DARK_PALETTE : LIGHT_PALETTE;
}

// Centralized ArkUI V2 AttributeModifiers using HarmonyOS 7 `$r('sys.float.*')` & scalable `maxFontScale`

export class OrbitTextModifier implements AttributeModifier<TextAttribute> {
  private readonly sizeRes: Resource;
  private readonly color: ResourceColor;
  private readonly weight: FontWeight;
  private readonly mono: boolean;
  private readonly maxScale: number;

  constructor(
    sizeRes: Resource,
    color: ResourceColor,
    weight: FontWeight,
    mono: boolean = false,
    maxScale: number = OrbitTokens.MAX_FONT_SCALE_BODY
  ) {
    this.sizeRes = sizeRes;
    this.color = color;
    this.weight = weight;
    this.mono = mono;
    this.maxScale = maxScale;
  }

  applyNormalAttribute(instance: TextAttribute): void {
    instance.fontSize(this.sizeRes);
    instance.fontWeight(this.weight);
    instance.fontColor(this.color);
    instance.maxFontScale(this.maxScale);
    if (this.mono) {
      instance.fontFamily(OrbitTokens.FAMILY_MONO);
    }
  }
}

export class OrbitButtonModifier implements AttributeModifier<ButtonAttribute> {
  private readonly sizeRes: Resource;
  private readonly fgColor: ResourceColor;
  private readonly bgColor: ResourceColor;

  constructor(sizeRes: Resource, fgColor: ResourceColor, bgColor: ResourceColor) {
    this.sizeRes = sizeRes;
    this.fgColor = fgColor;
    this.bgColor = bgColor;
  }

  applyNormalAttribute(instance: ButtonAttribute): void {
    instance.fontSize(this.sizeRes);
    instance.fontWeight(FontWeight.Medium);
    instance.fontColor(this.fgColor);
    instance.backgroundColor(this.bgColor);
    instance.stateEffect(true);
  }
}

export class OrbitInputModifier implements AttributeModifier<TextInputAttribute> {
  private readonly sizeRes: Resource;
  private readonly fgColor: ResourceColor;
  private readonly placeholderColor: ResourceColor;
  private readonly bgColor: ResourceColor;
  private readonly mono: boolean;

  constructor(
    sizeRes: Resource,
    fgColor: ResourceColor,
    placeholderColor: ResourceColor,
    bgColor: ResourceColor,
    mono: boolean = false
  ) {
    this.sizeRes = sizeRes;
    this.fgColor = fgColor;
    this.placeholderColor = placeholderColor;
    this.bgColor = bgColor;
    this.mono = mono;
  }

  applyNormalAttribute(instance: TextInputAttribute): void {
    instance.fontSize(this.sizeRes);
    instance.fontColor(this.fgColor);
    instance.placeholderColor(this.placeholderColor);
    instance.placeholderFont({ size: this.sizeRes });
    instance.backgroundColor(this.bgColor);
    instance.maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME);
    if (this.mono) {
      instance.fontFamily(OrbitTokens.FAMILY_MONO);
    }
  }
}

export class OrbitComposerAreaModifier implements AttributeModifier<TextAreaAttribute> {
  private readonly fgColor: ResourceColor;
  private readonly placeholderColor: ResourceColor;

  constructor(fgColor: ResourceColor, placeholderColor: ResourceColor) {
    this.fgColor = fgColor;
    this.placeholderColor = placeholderColor;
  }

  applyNormalAttribute(instance: TextAreaAttribute): void {
    instance.fontSize(OrbitTokens.FONT_BODY_RES);
    instance.fontColor(this.fgColor);
    instance.placeholderColor(this.placeholderColor);
    instance.placeholderFont({ size: OrbitTokens.FONT_BODY_RES });
    instance.backgroundColor(Color.Transparent);
    instance.maxFontScale(OrbitTokens.MAX_FONT_SCALE_BODY);
  }
}

export class OrbitSurfaceModifier implements AttributeModifier<CommonAttribute> {
  private readonly bg: ResourceColor;
  private readonly borderCol: ResourceColor;
  private readonly borderWidth: number;
  private readonly shadowStyle?: ShadowStyle;

  constructor(
    bg: ResourceColor,
    borderCol: ResourceColor,
    borderWidth: number = OrbitTokens.HAIRLINE,
    shadowStyle?: ShadowStyle
  ) {
    this.bg = bg;
    this.borderCol = borderCol;
    this.borderWidth = borderWidth;
    this.shadowStyle = shadowStyle;
  }

  applyNormalAttribute(instance: CommonAttribute): void {
    instance.backgroundColor(this.bg);
    if (this.borderWidth > 0) {
      instance.border({ width: this.borderWidth, color: this.borderCol });
    }
    if (this.shadowStyle !== undefined) {
      instance.shadow(this.shadowStyle);
    }
  }
}

// Modifier caches to avoid per-render heap allocations for common presets
const textModifierCache: Map<string, OrbitTextModifier> = new Map<string, OrbitTextModifier>();

function cachedTextMod(
  tierKey: string,
  sizeRes: Resource,
  color: ResourceColor,
  weight: FontWeight,
  mono: boolean,
  maxScale: number
): OrbitTextModifier {
  const colorKey = typeof color === 'string'
    ? color
    : `${(color as Resource).id ?? 0}_${(color as Resource).bundleName ?? ''}`;
  const key = `${tierKey}|${colorKey}|${weight}|${mono ? 1 : 0}|${maxScale}`;
  const existing = textModifierCache.get(key);
  if (existing !== undefined) {
    return existing;
  }
  const mod = new OrbitTextModifier(sizeRes, color, weight, mono, maxScale);
  textModifierCache.set(key, mod);
  return mod;
}

export function orbitTextTitle(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Bold
): OrbitTextModifier {
  return cachedTextMod('title', OrbitTokens.FONT_TITLE_RES, color, weight, false, OrbitTokens.MAX_FONT_SCALE_CHROME);
}

export function orbitTextSubtitle(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Medium
): OrbitTextModifier {
  return cachedTextMod('subtitle', OrbitTokens.FONT_SUBTITLE_RES, color, weight, false, OrbitTokens.MAX_FONT_SCALE_CHROME);
}

export function orbitTextBody(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Normal
): OrbitTextModifier {
  return cachedTextMod('body', OrbitTokens.FONT_BODY_RES, color, weight, false, OrbitTokens.MAX_FONT_SCALE_BODY);
}

export function orbitTextSubhead(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Normal
): OrbitTextModifier {
  return cachedTextMod('subhead', OrbitTokens.FONT_SUBHEAD_RES, color, weight, false, OrbitTokens.MAX_FONT_SCALE_CHROME);
}

export function orbitTextMeta(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Normal
): OrbitTextModifier {
  return cachedTextMod('meta', OrbitTokens.FONT_META_RES, color, weight, false, OrbitTokens.MAX_FONT_SCALE_CHROME);
}

export function orbitTextCaption(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Normal
): OrbitTextModifier {
  return cachedTextMod('caption', OrbitTokens.FONT_CAPTION_RES, color, weight, false, OrbitTokens.MAX_FONT_SCALE_CHROME);
}

export function orbitTextMono(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Normal
): OrbitTextModifier {
  return cachedTextMod('mono', OrbitTokens.FONT_META_RES, color, weight, true, OrbitTokens.MAX_FONT_SCALE_BODY);
}

export function orbitSpanHeadingFont(level: number): Font {
  const size = level <= 1 ? OrbitTokens.FONT_TITLE : (level === 2 ? 18 : OrbitTokens.FONT_BODY);
  return {
    size: size,
    weight: FontWeight.Bold
  };
}

export function orbitSpanBodyFont(
  weight: FontWeight = FontWeight.Normal,
  italic: boolean = false
): Font {
  return {
    size: OrbitTokens.FONT_BODY,
    weight: weight,
    style: italic ? FontStyle.Italic : FontStyle.Normal
  };
}

export function orbitSpanSubheadFont(
  weight: FontWeight = FontWeight.Normal,
  italic: boolean = false
): Font {
  return {
    size: OrbitTokens.FONT_SUBHEAD,
    weight: weight,
    style: italic ? FontStyle.Italic : FontStyle.Normal
  };
}

export function orbitSpanMetaFont(
  weight: FontWeight = FontWeight.Normal,
  italic: boolean = false
): Font {
  return {
    size: OrbitTokens.FONT_META,
    weight: weight,
    style: italic ? FontStyle.Italic : FontStyle.Normal
  };
}

export function orbitSpanMonoFont(
  weight: FontWeight = FontWeight.Normal,
  italic: boolean = false
): Font {
  return {
    size: OrbitTokens.FONT_META,
    weight: weight,
    family: OrbitTokens.FAMILY_MONO,
    style: italic ? FontStyle.Italic : FontStyle.Normal
  };
}

export function orbitButtonSubhead(
  fgColor: ResourceColor,
  bgColor: ResourceColor
): OrbitButtonModifier {
  return new OrbitButtonModifier(OrbitTokens.FONT_SUBHEAD_RES, fgColor, bgColor);
}

export function orbitButtonMeta(
  fgColor: ResourceColor,
  bgColor: ResourceColor
): OrbitButtonModifier {
  return new OrbitButtonModifier(OrbitTokens.FONT_META_RES, fgColor, bgColor);
}

export function orbitInputSubhead(
  fgColor: ResourceColor,
  placeholderColor: ResourceColor,
  bgColor: ResourceColor
): OrbitInputModifier {
  return new OrbitInputModifier(OrbitTokens.FONT_SUBHEAD_RES, fgColor, placeholderColor, bgColor, false);
}

export function orbitInputMono(
  fgColor: ResourceColor,
  placeholderColor: ResourceColor,
  bgColor: ResourceColor
): OrbitInputModifier {
  return new OrbitInputModifier(OrbitTokens.FONT_META_RES, fgColor, placeholderColor, bgColor, true);
}

export function orbitComposerArea(
  fgColor: ResourceColor,
  placeholderColor: ResourceColor
): OrbitComposerAreaModifier {
  return new OrbitComposerAreaModifier(fgColor, placeholderColor);
}

export function orbitSurface(
  bg: ResourceColor,
  borderCol: ResourceColor = $r('sys.color.comp_divider'),
  borderWidth: number = OrbitTokens.HAIRLINE,
  shadowStyle?: ShadowStyle
): OrbitSurfaceModifier {
  return new OrbitSurfaceModifier(bg, borderCol, borderWidth, shadowStyle);
}

const ORBIT_LOCKFILE_NAMES: string[] = [
  'cargo.lock', 'package-lock.json', 'pnpm-lock.yaml', 'yarn.lock',
  'bun.lock', 'bun.lockb', 'flake.lock', 'poetry.lock', 'uv.lock', 'oh-package-lock.json5'
];

const ORBIT_CODE_EXTS: string[] = [
  'rs', 'ts', 'tsx', 'ets', 'js', 'jsx', 'mjs', 'cjs', 'py', 'go', 'swift',
  'kt', 'kts', 'java', 'c', 'h', 'cc', 'cpp', 'hpp', 'cs', 'rb', 'php', 'lua',
  'zig', 'nix', 'proto', 'graphql', 'gql', 'css', 'scss', 'less', 'html', 'vue', 'svelte'
];

export function resolveSemanticFileSymbol(basename: string): Resource {
  const lower = basename.trim().toLowerCase();
  if (ORBIT_LOCKFILE_NAMES.includes(lower) || lower.endsWith('.lock')) {
    return $r('sys.symbol.lock');
  }
  if (
    lower === 'dockerfile' ||
    lower.startsWith('dockerfile.') ||
    lower.startsWith('.env') ||
    lower.includes('.config.')
  ) {
    return $r('sys.symbol.gearshape');
  }
  if (lower === 'makefile' || lower === 'justfile') {
    return $r('sys.symbol.code_square');
  }
  if (lower === 'license' || lower.startsWith('readme') || lower.startsWith('changelog')) {
    return $r('sys.symbol.doc_text');
  }
  const dotIdx = lower.lastIndexOf('.');
  const ext = dotIdx >= 0 ? lower.substring(dotIdx + 1) : '';
  if (ext === 'toml' || ext === 'yaml' || ext === 'yml' || ext === 'ini' || ext === 'conf' || ext === 'json5') {
    return $r('sys.symbol.gearshape');
  }
  if (ext === 'sh' || ext === 'zsh' || ext === 'bash' || ext === 'fish' || ext === 'ps1') {
    return $r('sys.symbol.code_square');
  }
  if (ext === 'md' || ext === 'mdx' || ext === 'txt' || ext === 'rst' || ext === 'json' || ext === 'jsonc') {
    return $r('sys.symbol.doc_text');
  }
  if (
    ext === 'png' || ext === 'jpg' || ext === 'jpeg' || ext === 'webp' ||
    ext === 'gif' || ext === 'svg' || ext === 'ico' || ext === 'bmp' || ext === 'heic'
  ) {
    return $r('sys.symbol.picture');
  }
  if (ext === 'sql' || ext === 'sqlite' || ext === 'db') {
    return $r('sys.symbol.archivebox');
  }
  if (ORBIT_CODE_EXTS.includes(ext)) {
    return $r('sys.symbol.code');
  }
  return $r('sys.symbol.doc');
}
