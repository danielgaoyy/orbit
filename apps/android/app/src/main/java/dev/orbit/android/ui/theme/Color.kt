package dev.orbit.android.ui.theme

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color

/**
 * Canonical OKLCH 258° neutral axis translated faithfully to sRGB for
 * **Orbital Quiet Precision** (aligned 1:1 with `.interface-design/system.md`):
 *
 * - Light: *Alabaster Paper* (`#F5F6F8` recessed -> `#FCFCFE` base -> `#FFFFFF` raised/overlay)
 * - Dark:  *Obsidian Graphite* (`#121318` recessed -> `#17181E` base -> `#20222B` raised -> `#272A35` overlay)
 * - Color is reserved strictly for Semantic Signals (`success`, `warning`, `danger`, `info`)
 *   and Provider brand marks; all chrome and primary actions remain monochrome high-contrast.
 */
object OrbitPalette {
    // --- Light Mode · Alabaster Paper (OKLCH 258°) ---
    val LightSurfaceRecessed = Color(0xFFF5F6F8)
    val LightSurfaceBase = Color(0xFFFCFCFE)
    val LightSurfaceRaised = Color(0xFFFFFFFF)
    val LightSurfaceOverlay = Color(0xFFFFFFFF)

    val LightInkPrimary = Color(0xFF14161D)
    val LightInkSecondary = Color(0xFF4A5060)
    val LightInkTertiary = Color(0xFF6E7487)
    val LightInkFaint = Color(0xFF969CAE)

    val LightBorderSubtle = Color(0x1114161D)   // ~6.5% cool-slate alpha
    val LightBorderDefault = Color(0x1C14161D)  // ~11% cool-slate alpha
    val LightBorderStrong = Color(0x3814161D)   // ~22% focus / active ring

    val LightSubtleFill = Color(0x0B14161D)     // whisper-quiet inset fill
    val LightActionPrimaryBg = Color(0xFF14161D)
    val LightActionPrimaryFg = Color(0xFFFCFCFE)

    val LightSuccess = Color(0xFF0F7B55)
    val LightWarning = Color(0xFFB45309)
    val LightDanger = Color(0xFFC8283B)
    val LightInfo = Color(0xFF2563EB)

    // --- Dark Mode · Obsidian Graphite (OKLCH 258°) ---
    val DarkSurfaceRecessed = Color(0xFF121318)
    val DarkSurfaceBase = Color(0xFF17181E)
    val DarkSurfaceRaised = Color(0xFF20222B)
    val DarkSurfaceOverlay = Color(0xFF272A35)

    val DarkInkPrimary = Color(0xFFF1F2F7)
    val DarkInkSecondary = Color(0xFFB2B6C7)
    val DarkInkTertiary = Color(0xFF83889C)
    val DarkInkFaint = Color(0xFF5F6477)

    val DarkBorderSubtle = Color(0x11F1F2F7)    // ~6.5% cool-slate hairline
    val DarkBorderDefault = Color(0x1DF1F2F7)   // ~11.5% control outline
    val DarkBorderStrong = Color(0x38F1F2F7)    // ~22% focus / active ring

    val DarkSubtleFill = Color(0x12F1F2F7)
    val DarkActionPrimaryBg = Color(0xFFF1F2F7)
    val DarkActionPrimaryFg = Color(0xFF14161D)

    val DarkSuccess = Color(0xFF3ED598)
    val DarkWarning = Color(0xFFF5B749)
    val DarkDanger = Color(0xFFF87171)
    val DarkInfo = Color(0xFF6FA8FF)
}

@Immutable
data class OrbitColors(
    val isDark: Boolean,
    // 4-step neutral surface hierarchy
    val surfaceRecessed: Color,
    val canvas: Color,          // surface-base
    val drawer: Color,          // aligned with canvas/recessed rail per system.md
    val elevated: Color,        // surface-raised
    val surfaceOverlay: Color,  // popovers / dialogs / menus
    val code: Color,            // recessed code & terminal well
    // 4-tier ink hierarchy
    val fgPrimary: Color,
    val fgSecondary: Color,
    val fgTertiary: Color,
    val fgMuted: Color,         // faint/muted tier
    // 3-tier hairline hierarchy
    val hairline: Color,        // border-subtle
    val borderDefault: Color,   // border-default
    val borderStrong: Color,    // border-strong
    // Monochrome high-contrast primary action
    val primary: Color,
    val onPrimary: Color,
    // WCAG AA semantic signals
    val directAdded: Color,     // success
    val relay: Color,           // info / relay accent
    val info: Color,            // info
    val warning: Color,         // warning / decision waiting
    val dangerRemoved: Color,   // danger / error
    // Interactive & diff fills
    val userBubble: Color,
    val subtleFill: Color,
    val diffAddedBg: Color,
    val diffRemovedBg: Color,
    val diffHunkBg: Color,
)

val LightOrbitColors = OrbitColors(
    isDark = false,
    surfaceRecessed = OrbitPalette.LightSurfaceRecessed,
    canvas = OrbitPalette.LightSurfaceBase,
    drawer = OrbitPalette.LightSurfaceBase,
    elevated = OrbitPalette.LightSurfaceRaised,
    surfaceOverlay = OrbitPalette.LightSurfaceOverlay,
    code = OrbitPalette.LightSurfaceRecessed,
    fgPrimary = OrbitPalette.LightInkPrimary,
    fgSecondary = OrbitPalette.LightInkSecondary,
    fgTertiary = OrbitPalette.LightInkTertiary,
    fgMuted = OrbitPalette.LightInkFaint,
    hairline = OrbitPalette.LightBorderSubtle,
    borderDefault = OrbitPalette.LightBorderDefault,
    borderStrong = OrbitPalette.LightBorderStrong,
    primary = OrbitPalette.LightActionPrimaryBg,
    onPrimary = OrbitPalette.LightActionPrimaryFg,
    directAdded = OrbitPalette.LightSuccess,
    relay = OrbitPalette.LightInfo,
    info = OrbitPalette.LightInfo,
    warning = OrbitPalette.LightWarning,
    dangerRemoved = OrbitPalette.LightDanger,
    userBubble = OrbitPalette.LightSurfaceRaised,
    subtleFill = OrbitPalette.LightSubtleFill,
    diffAddedBg = Color(0x140F7B55),
    diffRemovedBg = Color(0x14C8283B),
    diffHunkBg = Color(0x0E2563EB),
)

val GraphiteDarkOrbitColors = OrbitColors(
    isDark = true,
    surfaceRecessed = OrbitPalette.DarkSurfaceRecessed,
    canvas = OrbitPalette.DarkSurfaceBase,
    drawer = OrbitPalette.DarkSurfaceBase,
    elevated = OrbitPalette.DarkSurfaceRaised,
    surfaceOverlay = OrbitPalette.DarkSurfaceOverlay,
    code = OrbitPalette.DarkSurfaceRecessed,
    fgPrimary = OrbitPalette.DarkInkPrimary,
    fgSecondary = OrbitPalette.DarkInkSecondary,
    fgTertiary = OrbitPalette.DarkInkTertiary,
    fgMuted = OrbitPalette.DarkInkFaint,
    hairline = OrbitPalette.DarkBorderSubtle,
    borderDefault = OrbitPalette.DarkBorderDefault,
    borderStrong = OrbitPalette.DarkBorderStrong,
    primary = OrbitPalette.DarkActionPrimaryBg,
    onPrimary = OrbitPalette.DarkActionPrimaryFg,
    directAdded = OrbitPalette.DarkSuccess,
    relay = OrbitPalette.DarkInfo,
    info = OrbitPalette.DarkInfo,
    warning = OrbitPalette.DarkWarning,
    dangerRemoved = OrbitPalette.DarkDanger,
    userBubble = OrbitPalette.DarkSurfaceRaised,
    subtleFill = OrbitPalette.DarkSubtleFill,
    diffAddedBg = Color(0x1F3ED598),
    diffRemovedBg = Color(0x1FF87171),
    diffHunkBg = Color(0x1A6FA8FF),
)
