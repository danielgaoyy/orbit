package dev.orbit.android.ui.theme

import android.app.Activity
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat

enum class OrbitAppearanceMode(
    val label: String,
    val subtitle: String,
) {
    SYSTEM("System", "Match system"),
    LIGHT("Light", "Daylight paper"),
    DARK("Graphite", "Quiet graphite"),
    OLED("OLED", "True pitch black");

    companion object {
        fun fromStorage(raw: String?): OrbitAppearanceMode =
            entries.firstOrNull { it.name == raw } ?: SYSTEM
    }
}

@Immutable
data class OrbitColors(
    val isDark: Boolean,
    val canvas: Color,
    val drawer: Color,
    val elevated: Color,
    val code: Color,
    val fgPrimary: Color,
    val fgSecondary: Color,
    val fgMuted: Color,
    val hairline: Color,
    val primary: Color,
    val directAdded: Color,
    val relay: Color,
    val warning: Color,
    val dangerRemoved: Color,
    val userBubble: Color,
    val subtleFill: Color,
    val diffAddedBg: Color,
    val diffRemovedBg: Color,
    val diffHunkBg: Color,
)

val LightOrbitColors = OrbitColors(
    isDark = false,
    canvas = Color(0xFFFFFFFF),
    drawer = Color(0xFFF6F7F9),
    elevated = Color(0xFFFFFFFF),
    code = Color(0xFFF8F9FB),
    fgPrimary = Color(0xFF18191D),
    fgSecondary = Color(0xFF686B75),
    fgMuted = Color(0xFF9CA0AB),
    hairline = Color(0x13000000),
    primary = Color(0xFF2B5CE6),
    directAdded = Color(0xFF16A34A),
    relay = Color(0xFF7C3AED),
    warning = Color(0xFFD97706),
    dangerRemoved = Color(0xFFDC2626),
    userBubble = Color(0xFFF4F5F8),
    subtleFill = Color(0x0D000000),
    diffAddedBg = Color(0x1816A34A),
    diffRemovedBg = Color(0x18DC2626),
    diffHunkBg = Color(0x142B5CE6),
)

val GraphiteDarkOrbitColors = OrbitColors(
    isDark = true,
    canvas = Color(0xFF17181C),
    drawer = Color(0xFF111216),
    elevated = Color(0xFF202227),
    code = Color(0xFF121418),
    fgPrimary = Color(0xFFEFEFF2),
    fgSecondary = Color(0xFF969AA6),
    fgMuted = Color(0xFF636773),
    hairline = Color(0x14FFFFFF),
    primary = Color(0xFF6B93F6),
    directAdded = Color(0xFF34D399),
    relay = Color(0xFFA78BFA),
    warning = Color(0xFFFBBF24),
    dangerRemoved = Color(0xFFF87171),
    userBubble = Color(0xFF22252C),
    subtleFill = Color(0x12FFFFFF),
    diffAddedBg = Color(0x2234D399),
    diffRemovedBg = Color(0x22F87171),
    diffHunkBg = Color(0x206B93F6),
)

val OledPureOrbitColors = OrbitColors(
    isDark = true,
    canvas = Color(0xFF000000),
    drawer = Color(0xFF08090B),
    elevated = Color(0xFF121317),
    code = Color(0xFF0A0B0E),
    fgPrimary = Color(0xFFF4F4F6),
    fgSecondary = Color(0xFF8E929E),
    fgMuted = Color(0xFF5C606B),
    hairline = Color(0x1CFFFFFF),
    primary = Color(0xFF6B93F6),
    directAdded = Color(0xFF34D399),
    relay = Color(0xFFA78BFA),
    warning = Color(0xFFFBBF24),
    dangerRemoved = Color(0xFFF87171),
    userBubble = Color(0xFF14161B),
    subtleFill = Color(0x16FFFFFF),
    diffAddedBg = Color(0x2434D399),
    diffRemovedBg = Color(0x24F87171),
    diffHunkBg = Color(0x226B93F6),
)

/**
 * STRICT 4-TIER TYPOGRAPHY SCALE (18sp / 15sp / 13sp / 12sp ONLY).
 * All composables MUST consume styles from [OrbitTypography] and never set inline fontSize.
 */
@Immutable
data class OrbitTypographyScale(
    val title: TextStyle = TextStyle(
        fontSize = 18.sp,
        fontWeight = FontWeight.SemiBold,
        lineHeight = 24.sp,
    ),
    val body: TextStyle = TextStyle(
        fontSize = 15.sp,
        fontWeight = FontWeight.Normal,
        lineHeight = 22.sp,
    ),
    val bodyMedium: TextStyle = TextStyle(
        fontSize = 15.sp,
        fontWeight = FontWeight.Medium,
        lineHeight = 22.sp,
    ),
    val bodySemiBold: TextStyle = TextStyle(
        fontSize = 15.sp,
        fontWeight = FontWeight.SemiBold,
        lineHeight = 22.sp,
    ),
    val subhead: TextStyle = TextStyle(
        fontSize = 13.sp,
        fontWeight = FontWeight.Normal,
        lineHeight = 18.sp,
    ),
    val subheadMedium: TextStyle = TextStyle(
        fontSize = 13.sp,
        fontWeight = FontWeight.Medium,
        lineHeight = 18.sp,
    ),
    val subheadSemiBold: TextStyle = TextStyle(
        fontSize = 13.sp,
        fontWeight = FontWeight.SemiBold,
        lineHeight = 18.sp,
    ),
    val subheadTabular: TextStyle = TextStyle(
        fontSize = 13.sp,
        fontWeight = FontWeight.Medium,
        lineHeight = 18.sp,
        fontFeatureSettings = "tnum",
    ),
    val meta: TextStyle = TextStyle(
        fontSize = 12.sp,
        fontWeight = FontWeight.Normal,
        lineHeight = 16.sp,
        fontFeatureSettings = "tnum",
    ),
    val metaMedium: TextStyle = TextStyle(
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
        lineHeight = 16.sp,
        fontFeatureSettings = "tnum",
    ),
    val metaSemiBold: TextStyle = TextStyle(
        fontSize = 12.sp,
        fontWeight = FontWeight.SemiBold,
        lineHeight = 16.sp,
        fontFeatureSettings = "tnum",
    ),
    val code: TextStyle = TextStyle(
        fontSize = 12.sp,
        fontWeight = FontWeight.Normal,
        lineHeight = 16.sp,
        fontFamily = FontFamily.Monospace,
        fontFeatureSettings = "tnum",
    ),
    val codeMedium: TextStyle = TextStyle(
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
        lineHeight = 16.sp,
        fontFamily = FontFamily.Monospace,
        fontFeatureSettings = "tnum",
    ),
)

val OrbitTypography = OrbitTypographyScale()

/**
 * STRICT 4dp SPATIAL GRID (4 / 8 / 12 / 16 / 24 / 32).
 */
object OrbitSpacing {
    val hairline = 0.5.dp
    val xs = 4.dp
    val sm = 8.dp
    val md = 12.dp
    val lg = 16.dp
    val xl = 24.dp
    val xxl = 32.dp
    val touchTarget = 48.dp
    val topBarHeight = 52.dp
    val iconSm = 16.dp
    val iconMd = 18.dp
}

val LocalOrbitColors = staticCompositionLocalOf { GraphiteDarkOrbitColors }
val LocalOrbitTypography = staticCompositionLocalOf { OrbitTypography }

object OrbitTheme {
    val colors: OrbitColors
        @Composable get() = LocalOrbitColors.current

    val typography: OrbitTypographyScale
        @Composable get() = LocalOrbitTypography.current
}

private val OrbitMaterialTypography = Typography(
    headlineSmall = OrbitTypography.title,
    titleLarge = OrbitTypography.title,
    titleMedium = OrbitTypography.bodySemiBold,
    titleSmall = OrbitTypography.subheadSemiBold,
    bodyLarge = OrbitTypography.body,
    bodyMedium = OrbitTypography.subhead,
    bodySmall = OrbitTypography.meta,
    labelLarge = OrbitTypography.subheadMedium,
    labelMedium = OrbitTypography.metaMedium,
    labelSmall = OrbitTypography.meta,
)

@Composable
fun OrbitTheme(
    appearanceMode: OrbitAppearanceMode,
    content: @Composable () -> Unit,
) {
    val systemDark = isSystemInDarkTheme()
    val palette = when (appearanceMode) {
        OrbitAppearanceMode.LIGHT -> LightOrbitColors
        OrbitAppearanceMode.DARK -> GraphiteDarkOrbitColors
        OrbitAppearanceMode.OLED -> OledPureOrbitColors
        OrbitAppearanceMode.SYSTEM -> if (systemDark) GraphiteDarkOrbitColors else LightOrbitColors
    }

    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val activity = view.context as? Activity ?: return@SideEffect
            val window = activity.window
            val controller = WindowCompat.getInsetsController(window, view)
            controller.isAppearanceLightStatusBars = !palette.isDark
            controller.isAppearanceLightNavigationBars = !palette.isDark
        }
    }

    val m3Scheme = if (palette.isDark) {
        darkColorScheme(
            primary = palette.primary,
            onPrimary = Color.White,
            background = palette.canvas,
            onBackground = palette.fgPrimary,
            surface = palette.elevated,
            surfaceContainer = palette.elevated,
            surfaceContainerLow = palette.drawer,
            surfaceContainerHigh = palette.elevated,
            onSurface = palette.fgPrimary,
            onSurfaceVariant = palette.fgSecondary,
            outline = palette.hairline,
            outlineVariant = palette.hairline,
            error = palette.dangerRemoved,
        )
    } else {
        lightColorScheme(
            primary = palette.primary,
            onPrimary = Color.White,
            background = palette.canvas,
            onBackground = palette.fgPrimary,
            surface = palette.elevated,
            surfaceContainer = palette.elevated,
            surfaceContainerLow = palette.drawer,
            surfaceContainerHigh = palette.elevated,
            onSurface = palette.fgPrimary,
            onSurfaceVariant = palette.fgSecondary,
            outline = palette.hairline,
            outlineVariant = palette.hairline,
            error = palette.dangerRemoved,
        )
    }

    CompositionLocalProvider(
        LocalOrbitColors provides palette,
        LocalOrbitTypography provides OrbitTypography,
    ) {
        MaterialTheme(
            colorScheme = m3Scheme,
            typography = OrbitMaterialTypography,
            content = content,
        )
    }
}

fun parseHexColor(hex: String?, fallback: Color): Color {
    if (hex.isNullOrBlank()) return fallback
    val clean = hex.trim().removePrefix("#")
    if (clean.length != 6 && clean.length != 8) return fallback
    return runCatching {
        val parsed = clean.toLong(16)
        if (clean.length == 6) {
            Color(parsed or 0xFF000000L)
        } else {
            Color(parsed)
        }
    }.getOrDefault(fallback)
}
