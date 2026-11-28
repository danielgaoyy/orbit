import {
  DARK_PALETTE,
  LIGHT_PALETTE,
  ORBIT_DEFAULT_THEME,
  OrbitSurfaceModifier,
  OrbitThemePalette,
  OrbitTokens,
  orbitSurface,
  resolveOrbitPalette
} from './OrbitTokens';

export {
  DARK_PALETTE,
  LIGHT_PALETTE,
  ORBIT_DEFAULT_THEME,
  OrbitThemePalette,
  OrbitTokens,
  resolveOrbitPalette
};

export interface OrbitMotionConfig {
  duration: number;
  curve: Curve;
}

export class OrbitTheme {
  public static readonly MOTION_FAST: OrbitMotionConfig = {
    duration: OrbitTokens.DUR_FAST,
    curve: Curve.FastOutSlowIn
  };

  public static readonly MOTION_UI: OrbitMotionConfig = {
    duration: OrbitTokens.DUR_UI,
    curve: Curve.FastOutSlowIn
  };

  public static readonly MOTION_OVERLAY: OrbitMotionConfig = {
    duration: OrbitTokens.DUR_OVERLAY,
    curve: Curve.FastOutSlowIn
  };

  public static readonly MOTION_PANEL: OrbitMotionConfig = {
    duration: OrbitTokens.DUR_PANEL,
    curve: Curve.FastOutSlowIn
  };

  public static readonly MOTION_ZERO: OrbitMotionConfig = {
    duration: 0,
    curve: Curve.Linear
  };

  public static readonly MOTION_EXIT: OrbitMotionConfig = {
    duration: OrbitTokens.DUR_EXIT,
    curve: Curve.EaseOut
  };

  public static resolveMotion(base: OrbitMotionConfig, reducedMotion: boolean): OrbitMotionConfig {
    if (reducedMotion) {
      return OrbitTheme.MOTION_ZERO;
    }
    return base;
  }

  public static resolveDuration(durationMs: number, reducedMotion: boolean): number {
    if (reducedMotion) {
      return 0;
    }
    return durationMs;
  }

  public static raisedCard(palette: OrbitThemePalette): OrbitSurfaceModifier {
    return orbitSurface(
      palette.listCard,
      palette.hairline,
      OrbitTokens.HAIRLINE,
      palette.isDark ? undefined : ShadowStyle.OUTER_DEFAULT_XS
    );
  }

  public static dockedCard(palette: OrbitThemePalette): OrbitSurfaceModifier {
    return orbitSurface(
      palette.listCard,
      palette.borderDefault,
      OrbitTokens.HAIRLINE,
      palette.isDark ? undefined : ShadowStyle.OUTER_DEFAULT_SM
    );
  }

  public static recessedWell(palette: OrbitThemePalette): OrbitSurfaceModifier {
    return orbitSurface(
      palette.recessed,
      palette.hairline,
      OrbitTokens.HAIRLINE
    );
  }

  public static overlayPopover(palette: OrbitThemePalette): OrbitSurfaceModifier {
    return orbitSurface(
      palette.overlay,
      palette.borderDefault,
      OrbitTokens.HAIRLINE,
      ShadowStyle.OUTER_DEFAULT_MD
    );
  }
}
