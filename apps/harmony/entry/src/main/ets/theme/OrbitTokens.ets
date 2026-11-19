export class OrbitTokens {
  // Strict 4-Tier Typography Scale (fp)
  public static readonly FONT_TITLE: number = 18;
  public static readonly LH_TITLE: number = 24;

  public static readonly FONT_BODY: number = 15;
  public static readonly LH_BODY: number = 22;

  public static readonly FONT_SUBHEAD: number = 13;
  public static readonly LH_SUBHEAD: number = 18;

  public static readonly FONT_META: number = 12;
  public static readonly LH_META: number = 16;

  public static readonly FAMILY_SANS: string = 'HarmonyOS Sans';
  public static readonly FAMILY_MONO: string = 'HarmonyOS Sans Mono';

  // Strict 4vp Spatial Grid (vp)
  public static readonly SPACE_XS: number = 4;
  public static readonly SPACE_SM: number = 8;
  public static readonly SPACE_MD: number = 12;
  public static readonly SPACE_LG: number = 16;
  public static readonly SPACE_XL: number = 24;
  public static readonly SPACE_XXL: number = 32;

  // Hit Targets, Radii & Hairline
  public static readonly HIT_MIN: number = 44;
  public static readonly TOP_BAR_HEIGHT: number = 52;
  public static readonly TOAST_OFFSET_TOP: number = 56;
  public static readonly COMPOSER_RADIUS: number = 20;
  public static readonly CARD_RADIUS: number = 12;
  public static readonly PILL_RADIUS: number = 8;
  public static readonly DOT_SIZE: number = 6;
  public static readonly HAIRLINE: number = 0.5;

  // Multi-Form-Factor Breakpoints:
  // - Phone (< 700vp): Single-column Stage + Overlay FleetSidebar (260vp)
  // - Dual-Fold / Portrait Tablet (700..1023vp): 2-column (FleetSidebar 260vp + Stage >= 440vp), Git Diff in bindSheet
  // - Mate XT Full 3-Screen Unfolded / Landscape MatePad (>= 1024vp): 3-column (FleetSidebar 260vp + Stage + GitDiffInspector 360vp)
  public static readonly BP_FOLDABLE_MIN: number = 700;
  public static readonly BP_EXPANDED_MIN: number = 1024;
  public static readonly SIDEBAR_FOLD_WIDTH: number = 260;
  public static readonly SIDEBAR_EXPANDED_WIDTH: number = 260;
  public static readonly DIFF_PANE_EXPANDED_WIDTH: number = 360;
}

export interface OrbitThemePalette {
  id: string;
  isDark: boolean;
  canvas: string;
  drawer: string;
  elevated: string;
  code: string;
  subtleSurface: string;
  userBubble: string;
  fgPrimary: string;
  fgSecondary: string;
  fgMuted: string;
  hairline: string;
  accentPrimary: string;
  accentPrimaryFill: string;
  accentPrimarySoft: string;
  onAccent: string;
  statusDirect: string;
  statusDirectSoft: string;
  statusRelay: string;
  statusRelaySoft: string;
  statusWarning: string;
  statusWarningSoft: string;
  statusDanger: string;
  statusDangerSoft: string;
  diffAddedBg: string;
  diffRemovedBg: string;
}

export const LIGHT_PALETTE: OrbitThemePalette = {
  id: 'light',
  isDark: false,
  canvas: '#FFFFFF',
  drawer: '#F6F7F9',
  elevated: '#FFFFFF',
  code: '#F8F9FB',
  subtleSurface: '#EFF1F5',
  userBubble: '#F3F5F8',
  fgPrimary: '#18191D',
  fgSecondary: '#686B75',
  fgMuted: '#9CA0AB',
  hairline: '#14000000',
  accentPrimary: '#2B5CE6',
  accentPrimaryFill: '#2B5CE6',
  accentPrimarySoft: '#142B5CE6',
  onAccent: '#FFFFFF',
  statusDirect: '#16A34A',
  statusDirectSoft: '#1416A34A',
  statusRelay: '#7C3AED',
  statusRelaySoft: '#147C3AED',
  statusWarning: '#D97706',
  statusWarningSoft: '#16D97706',
  statusDanger: '#DC2626',
  statusDangerSoft: '#14DC2626',
  diffAddedBg: '#1616A34A',
  diffRemovedBg: '#16DC2626'
};

export const GRAPHITE_DARK_PALETTE: OrbitThemePalette = {
  id: 'dark',
  isDark: true,
  canvas: '#17181C',
  drawer: '#111216',
  elevated: '#202227',
  code: '#121418',
  subtleSurface: '#262930',
  userBubble: '#22252C',
  fgPrimary: '#EFEFF2',
  fgSecondary: '#969AA6',
  fgMuted: '#636773',
  hairline: '#14FFFFFF',
  accentPrimary: '#6B93F6',
  accentPrimaryFill: '#2B5CE6',
  accentPrimarySoft: '#226B93F6',
  onAccent: '#FFFFFF',
  statusDirect: '#34D399',
  statusDirectSoft: '#1E34D399',
  statusRelay: '#A78BFA',
  statusRelaySoft: '#1EA78BFA',
  statusWarning: '#FBBF24',
  statusWarningSoft: '#1EFBBF24',
  statusDanger: '#F87171',
  statusDangerSoft: '#1EF87171',
  diffAddedBg: '#1F34D399',
  diffRemovedBg: '#1FF87171'
};

export const OLED_PURE_PALETTE: OrbitThemePalette = {
  id: 'oled',
  isDark: true,
  canvas: '#000000',
  drawer: '#08090B',
  elevated: '#121317',
  code: '#0A0B0E',
  subtleSurface: '#17191F',
  userBubble: '#14161B',
  fgPrimary: '#F4F4F6',
  fgSecondary: '#8E929E',
  fgMuted: '#5C606B',
  hairline: '#1CFFFFFF',
  accentPrimary: '#6B93F6',
  accentPrimaryFill: '#2B5CE6',
  accentPrimarySoft: '#246B93F6',
  onAccent: '#FFFFFF',
  statusDirect: '#34D399',
  statusDirectSoft: '#2034D399',
  statusRelay: '#A78BFA',
  statusRelaySoft: '#20A78BFA',
  statusWarning: '#FBBF24',
  statusWarningSoft: '#20FBBF24',
  statusDanger: '#F87171',
  statusDangerSoft: '#20F87171',
  diffAddedBg: '#2234D399',
  diffRemovedBg: '#22F87171'
};

export function resolveOrbitPalette(themeMode: string, systemIsDark: boolean): OrbitThemePalette {
  if (themeMode === 'light') {
    return LIGHT_PALETTE;
  }
  if (themeMode === 'oled') {
    return OLED_PURE_PALETTE;
  }
  if (themeMode === 'system') {
    return systemIsDark ? GRAPHITE_DARK_PALETTE : LIGHT_PALETTE;
  }
  return GRAPHITE_DARK_PALETTE;
}

// Centralized ArkUI V2 AttributeModifiers & Span Font Presets — ALL .fontSize() calls live strictly here.

export class OrbitTextModifier implements AttributeModifier<TextAttribute> {
  private readonly sizeFp: number;
  private readonly lineHeightVp: number;
  private readonly color: ResourceColor;
  private readonly weight: FontWeight;
  private readonly family: string;

  constructor(
    sizeFp: number,
    lineHeightVp: number,
    color: ResourceColor,
    weight: FontWeight,
    family: string
  ) {
    this.sizeFp = sizeFp;
    this.lineHeightVp = lineHeightVp;
    this.color = color;
    this.weight = weight;
    this.family = family;
  }

  applyNormalAttribute(instance: TextAttribute): void {
    instance.fontSize(this.sizeFp);
    instance.lineHeight(this.lineHeightVp);
    instance.fontWeight(this.weight);
    instance.fontFamily(this.family);
    instance.fontColor(this.color);
  }
}

export class OrbitButtonModifier implements AttributeModifier<ButtonAttribute> {
  private readonly sizeFp: number;
  private readonly fgColor: ResourceColor;
  private readonly bgColor: ResourceColor;
  private readonly family: string;

  constructor(sizeFp: number, fgColor: ResourceColor, bgColor: ResourceColor, family: string) {
    this.sizeFp = sizeFp;
    this.fgColor = fgColor;
    this.bgColor = bgColor;
    this.family = family;
  }

  applyNormalAttribute(instance: ButtonAttribute): void {
    instance.fontSize(this.sizeFp);
    instance.fontWeight(FontWeight.Medium);
    instance.fontFamily(this.family);
    instance.fontColor(this.fgColor);
    instance.backgroundColor(this.bgColor);
  }
}

export class OrbitInputModifier implements AttributeModifier<TextInputAttribute> {
  private readonly sizeFp: number;
  private readonly fgColor: ResourceColor;
  private readonly placeholderColor: ResourceColor;
  private readonly bgColor: ResourceColor;
  private readonly family: string;

  constructor(
    sizeFp: number,
    fgColor: ResourceColor,
    placeholderColor: ResourceColor,
    bgColor: ResourceColor,
    family: string
  ) {
    this.sizeFp = sizeFp;
    this.fgColor = fgColor;
    this.placeholderColor = placeholderColor;
    this.bgColor = bgColor;
    this.family = family;
  }

  applyNormalAttribute(instance: TextInputAttribute): void {
    instance.fontSize(this.sizeFp);
    instance.fontFamily(this.family);
    instance.fontColor(this.fgColor);
    instance.placeholderColor(this.placeholderColor);
    instance.placeholderFont({ size: this.sizeFp, family: this.family });
    instance.backgroundColor(this.bgColor);
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
    instance.fontSize(OrbitTokens.FONT_BODY);
    instance.lineHeight(OrbitTokens.LH_BODY);
    instance.fontFamily(OrbitTokens.FAMILY_SANS);
    instance.fontColor(this.fgColor);
    instance.placeholderColor(this.placeholderColor);
    instance.placeholderFont({
      size: OrbitTokens.FONT_BODY,
      family: OrbitTokens.FAMILY_SANS
    });
    instance.backgroundColor(Color.Transparent);
  }
}

export function orbitTextTitle(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Bold
): OrbitTextModifier {
  return new OrbitTextModifier(
    OrbitTokens.FONT_TITLE,
    OrbitTokens.LH_TITLE,
    color,
    weight,
    OrbitTokens.FAMILY_SANS
  );
}

export function orbitTextBody(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Normal
): OrbitTextModifier {
  return new OrbitTextModifier(
    OrbitTokens.FONT_BODY,
    OrbitTokens.LH_BODY,
    color,
    weight,
    OrbitTokens.FAMILY_SANS
  );
}

export function orbitTextSubhead(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Medium
): OrbitTextModifier {
  return new OrbitTextModifier(
    OrbitTokens.FONT_SUBHEAD,
    OrbitTokens.LH_SUBHEAD,
    color,
    weight,
    OrbitTokens.FAMILY_SANS
  );
}

export function orbitTextMeta(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Normal
): OrbitTextModifier {
  return new OrbitTextModifier(
    OrbitTokens.FONT_META,
    OrbitTokens.LH_META,
    color,
    weight,
    OrbitTokens.FAMILY_SANS
  );
}

export function orbitTextMono(
  color: ResourceColor,
  weight: FontWeight = FontWeight.Normal
): OrbitTextModifier {
  return new OrbitTextModifier(
    OrbitTokens.FONT_META,
    OrbitTokens.LH_META,
    color,
    weight,
    OrbitTokens.FAMILY_MONO
  );
}

export function orbitSpanTitleFont(weight: FontWeight = FontWeight.Bold): Font {
  const f: Font = {
    size: OrbitTokens.FONT_TITLE,
    weight: weight,
    family: OrbitTokens.FAMILY_SANS
  };
  return f;
}

export function orbitSpanBodyFont(
  weight: FontWeight = FontWeight.Normal,
  italic: boolean = false
): Font {
  const f: Font = {
    size: OrbitTokens.FONT_BODY,
    weight: weight,
    style: italic ? FontStyle.Italic : FontStyle.Normal,
    family: OrbitTokens.FAMILY_SANS
  };
  return f;
}

export function orbitSpanSubheadFont(weight: FontWeight = FontWeight.Medium): Font {
  const f: Font = {
    size: OrbitTokens.FONT_SUBHEAD,
    weight: weight,
    family: OrbitTokens.FAMILY_SANS
  };
  return f;
}

export function orbitSpanMetaFont(weight: FontWeight = FontWeight.Normal): Font {
  const f: Font = {
    size: OrbitTokens.FONT_META,
    weight: weight,
    family: OrbitTokens.FAMILY_SANS
  };
  return f;
}

export function orbitSpanMonoFont(
  weight: FontWeight = FontWeight.Normal,
  italic: boolean = false
): Font {
  const f: Font = {
    size: OrbitTokens.FONT_META,
    weight: weight,
    style: italic ? FontStyle.Italic : FontStyle.Normal,
    family: OrbitTokens.FAMILY_MONO
  };
  return f;
}

export function orbitButtonSubhead(
  fgColor: ResourceColor,
  bgColor: ResourceColor
): OrbitButtonModifier {
  return new OrbitButtonModifier(
    OrbitTokens.FONT_SUBHEAD,
    fgColor,
    bgColor,
    OrbitTokens.FAMILY_SANS
  );
}

export function orbitButtonMeta(
  fgColor: ResourceColor,
  bgColor: ResourceColor
): OrbitButtonModifier {
  return new OrbitButtonModifier(
    OrbitTokens.FONT_META,
    fgColor,
    bgColor,
    OrbitTokens.FAMILY_SANS
  );
}

export function orbitButtonMono(
  fgColor: ResourceColor,
  bgColor: ResourceColor
): OrbitButtonModifier {
  return new OrbitButtonModifier(
    OrbitTokens.FONT_META,
    fgColor,
    bgColor,
    OrbitTokens.FAMILY_MONO
  );
}

export function orbitInputSubhead(
  fgColor: ResourceColor,
  placeholderColor: ResourceColor,
  bgColor: ResourceColor
): OrbitInputModifier {
  return new OrbitInputModifier(
    OrbitTokens.FONT_SUBHEAD,
    fgColor,
    placeholderColor,
    bgColor,
    OrbitTokens.FAMILY_SANS
  );
}

export function orbitInputMono(
  fgColor: ResourceColor,
  placeholderColor: ResourceColor,
  bgColor: ResourceColor
): OrbitInputModifier {
  return new OrbitInputModifier(
    OrbitTokens.FONT_META,
    fgColor,
    placeholderColor,
    bgColor,
    OrbitTokens.FAMILY_MONO
  );
}

export function orbitComposerArea(
  fgColor: ResourceColor,
  placeholderColor: ResourceColor
): OrbitComposerAreaModifier {
  return new OrbitComposerAreaModifier(fgColor, placeholderColor);
}
