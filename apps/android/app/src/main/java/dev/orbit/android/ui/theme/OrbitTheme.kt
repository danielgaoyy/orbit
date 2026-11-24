package dev.orbit.android.ui.theme

import android.app.Activity
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.InsertDriveFile
import androidx.compose.material.icons.outlined.Code
import androidx.compose.material.icons.outlined.DataObject
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Storage
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.graphics.vector.path
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import uniffi.orbit_ffi_uniffi.FfiProviderKind

enum class OrbitAppearanceMode(
    val label: String,
) {
    SYSTEM("System"),
    LIGHT("Light"),
    DARK("Dark");

    companion object {
        fun fromStorage(raw: String?): OrbitAppearanceMode =
            when (raw?.trim()?.uppercase()) {
                "LIGHT" -> LIGHT
                "DARK", "GRAPHITE", "OLED", "BLACK" -> DARK
                else -> SYSTEM
            }
    }
}

/**
 * STRICT INTEGER TYPOGRAPHY SCALE aligned with `.interface-design/system.md`:
 * - hero:    24sp / 30sp (-0.45sp tracking) — calm overview / empty-state hero
 * - title:   18sp / 24sp (-0.30sp tracking) — sheet headers & section anchors
 * - body:    15sp / 24sp                    — T1 Reading prose & composer input
 * - subhead: 13sp / 19sp                    — T2 Interface controls, rows, traces
 * - meta:    11sp / 15sp (tnum)             — T3 Meta timestamps, badges, eyebrows
 * - code:    12sp / 18sp (Monospace, tnum)  — Code blocks, diffs, PTY terminal
 */
@Immutable
data class OrbitTypographyScale(
    val hero: TextStyle = TextStyle(
        fontSize = 24.sp,
        fontWeight = FontWeight.SemiBold,
        lineHeight = 30.sp,
        letterSpacing = (-0.45).sp,
    ),
    val title: TextStyle = TextStyle(
        fontSize = 18.sp,
        fontWeight = FontWeight.SemiBold,
        lineHeight = 24.sp,
        letterSpacing = (-0.3).sp,
    ),
    val body: TextStyle = TextStyle(
        fontSize = 15.sp,
        fontWeight = FontWeight.Normal,
        lineHeight = 24.sp,
    ),
    val bodyMedium: TextStyle = TextStyle(
        fontSize = 15.sp,
        fontWeight = FontWeight.Medium,
        lineHeight = 24.sp,
    ),
    val bodySemiBold: TextStyle = TextStyle(
        fontSize = 15.sp,
        fontWeight = FontWeight.SemiBold,
        lineHeight = 24.sp,
    ),
    val subhead: TextStyle = TextStyle(
        fontSize = 13.sp,
        fontWeight = FontWeight.Normal,
        lineHeight = 19.sp,
    ),
    val subheadMedium: TextStyle = TextStyle(
        fontSize = 13.sp,
        fontWeight = FontWeight.Medium,
        lineHeight = 19.sp,
    ),
    val subheadSemiBold: TextStyle = TextStyle(
        fontSize = 13.sp,
        fontWeight = FontWeight.SemiBold,
        lineHeight = 19.sp,
    ),
    val subheadTabular: TextStyle = TextStyle(
        fontSize = 13.sp,
        fontWeight = FontWeight.Medium,
        lineHeight = 19.sp,
        fontFeatureSettings = "tnum",
    ),
    val meta: TextStyle = TextStyle(
        fontSize = 11.sp,
        fontWeight = FontWeight.Normal,
        lineHeight = 15.sp,
        fontFeatureSettings = "tnum",
    ),
    val metaMedium: TextStyle = TextStyle(
        fontSize = 11.sp,
        fontWeight = FontWeight.Medium,
        lineHeight = 15.sp,
        fontFeatureSettings = "tnum",
    ),
    val metaSemiBold: TextStyle = TextStyle(
        fontSize = 11.sp,
        fontWeight = FontWeight.SemiBold,
        lineHeight = 15.sp,
        fontFeatureSettings = "tnum",
    ),
    val code: TextStyle = TextStyle(
        fontSize = 12.sp,
        fontWeight = FontWeight.Normal,
        lineHeight = 18.sp,
        fontFamily = FontFamily.Monospace,
        fontFeatureSettings = "tnum",
    ),
    val codeMedium: TextStyle = TextStyle(
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
        lineHeight = 18.sp,
        fontFamily = FontFamily.Monospace,
        fontFeatureSettings = "tnum",
    ),
)

val OrbitTypography = OrbitTypographyScale()

/**
 * STRICT 4dp SPATIAL GRID (4 / 8 / 12 / 16 / 20 / 24 / 32).
 */
object OrbitSpacing {
    val hairline = 0.5.dp
    val xs = 4.dp
    val sm = 8.dp
    val md = 12.dp
    val lg = 16.dp
    val section = 20.dp
    val xl = 24.dp
    val xxl = 32.dp
    val touchTarget = 48.dp
    val topBarHeight = 52.dp
    val iconSm = 16.dp
    val iconMd = 18.dp
}

/**
 * Concentric radius scale matching Desktop (`4 / 6 / 8 / 12 / 16 / 20`).
 */
object OrbitRadius {
    val xs = 4.dp
    val sm = 6.dp
    val md = 8.dp
    val lg = 12.dp
    val xl = 16.dp
    val composer = 20.dp
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
            onPrimary = palette.onPrimary,
            primaryContainer = palette.subtleFill,
            onPrimaryContainer = palette.fgPrimary,
            secondary = palette.fgSecondary,
            onSecondary = palette.canvas,
            secondaryContainer = palette.subtleFill,
            onSecondaryContainer = palette.fgPrimary,
            tertiary = palette.fgSecondary,
            onTertiary = palette.canvas,
            surfaceTint = palette.fgPrimary,
            surfaceVariant = palette.surfaceRecessed,
            background = palette.canvas,
            onBackground = palette.fgPrimary,
            surface = palette.elevated,
            surfaceContainerLowest = palette.surfaceRecessed,
            surfaceContainerLow = palette.canvas,
            surfaceContainer = palette.elevated,
            surfaceContainerHigh = palette.surfaceOverlay,
            surfaceContainerHighest = palette.surfaceOverlay,
            onSurface = palette.fgPrimary,
            onSurfaceVariant = palette.fgSecondary,
            outline = palette.borderDefault,
            outlineVariant = palette.hairline,
            error = palette.dangerRemoved,
        )
    } else {
        lightColorScheme(
            primary = palette.primary,
            onPrimary = palette.onPrimary,
            primaryContainer = palette.subtleFill,
            onPrimaryContainer = palette.fgPrimary,
            secondary = palette.fgSecondary,
            onSecondary = palette.canvas,
            secondaryContainer = palette.subtleFill,
            onSecondaryContainer = palette.fgPrimary,
            tertiary = palette.fgSecondary,
            onTertiary = palette.canvas,
            surfaceTint = palette.fgPrimary,
            surfaceVariant = palette.surfaceRecessed,
            background = palette.canvas,
            onBackground = palette.fgPrimary,
            surface = palette.elevated,
            surfaceContainerLowest = palette.surfaceRecessed,
            surfaceContainerLow = palette.canvas,
            surfaceContainer = palette.elevated,
            surfaceContainerHigh = palette.surfaceOverlay,
            surfaceContainerHighest = palette.surfaceOverlay,
            onSurface = palette.fgPrimary,
            onSurfaceVariant = palette.fgSecondary,
            outline = palette.borderDefault,
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

/**
 * Minimalist two-square geometric Copy icon matching Desktop Lucide `Copy` & iOS `square.on.square`.
 */
val OrbitCopyIcon: ImageVector by lazy {
    ImageVector.Builder(
        name = "OrbitCopy",
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 24f,
        viewportHeight = 24f,
    ).apply {
        path(
            fill = null,
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.9f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        ) {
            moveTo(10f, 8f)
            lineTo(20f, 8f)
            reflectiveCurveToRelative(2f, 0f, 2f, 2f)
            lineTo(22f, 20f)
            reflectiveCurveToRelative(0f, 2f, -2f, 2f)
            lineTo(10f, 22f)
            reflectiveCurveToRelative(-2f, 0f, -2f, -2f)
            lineTo(8f, 10f)
            reflectiveCurveToRelative(0f, -2f, 2f, -2f)
            close()
        }
        path(
            fill = null,
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.9f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        ) {
            moveTo(4f, 16f)
            curveToRelative(-1.1f, 0f, -2f, -0.9f, -2f, -2f)
            verticalLineTo(4f)
            curveToRelative(0f, -1.1f, 0.9f, -2f, 2f, -2f)
            horizontalLineTo(14f)
            curveToRelative(1.1f, 0f, 2f, 0.9f, 2f, 2f)
        }
    }.build()
}

/**
 * Geometric left-sidebar icon aligned with Desktop Lucide `PanelLeft` & iOS `sidebar.left`.
 */
val OrbitSidebarIcon: ImageVector by lazy {
    ImageVector.Builder(
        name = "OrbitSidebar",
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 24f,
        viewportHeight = 24f,
    ).apply {
        path(
            fill = null,
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.9f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        ) {
            moveTo(5f, 3f)
            horizontalLineTo(19f)
            curveTo(20.1f, 3f, 21f, 3.9f, 21f, 5f)
            verticalLineTo(19f)
            curveTo(21f, 20.1f, 20.1f, 21f, 19f, 21f)
            horizontalLineTo(5f)
            curveTo(3.9f, 21f, 3f, 20.1f, 3f, 19f)
            verticalLineTo(5f)
            curveTo(3f, 3.9f, 3.9f, 3f, 5f, 3f)
            close()
        }
        path(
            fill = null,
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.9f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        ) {
            moveTo(9f, 3f)
            verticalLineTo(21f)
        }
    }.build()
}

/**
 * Geometric new-conversation compose icon aligned with Desktop Lucide `SquarePen` & iOS `square.and.pencil`.
 */
val OrbitComposeIcon: ImageVector by lazy {
    ImageVector.Builder(
        name = "OrbitCompose",
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 24f,
        viewportHeight = 24f,
    ).apply {
        path(
            fill = null,
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.9f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        ) {
            moveTo(12f, 3f)
            horizontalLineTo(5f)
            curveTo(3.9f, 3f, 3f, 3.9f, 3f, 5f)
            verticalLineTo(19f)
            curveTo(3f, 20.1f, 3.9f, 21f, 5f, 21f)
            horizontalLineTo(19f)
            curveTo(20.1f, 21f, 21f, 20.1f, 21f, 19f)
            verticalLineTo(12f)
        }
        path(
            fill = null,
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.9f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        ) {
            moveTo(18.375f, 2.625f)
            curveTo(19.2f, 1.8f, 20.55f, 1.8f, 21.375f, 2.625f)
            curveTo(22.2f, 3.45f, 22.2f, 4.8f, 21.375f, 5.625f)
            lineTo(12.36f, 14.64f)
            curveTo(12.05f, 14.95f, 11.67f, 15.17f, 11.24f, 15.27f)
            lineTo(8.37f, 15.99f)
            curveTo(8.11f, 16.05f, 7.95f, 15.89f, 8.01f, 15.63f)
            lineTo(8.73f, 12.76f)
            curveTo(8.83f, 12.33f, 9.05f, 11.95f, 9.36f, 11.64f)
            close()
        }
    }.build()
}

private val OrbitLockfileNames = setOf(
    "cargo.lock", "package-lock.json", "pnpm-lock.yaml", "yarn.lock",
    "bun.lock", "bun.lockb", "flake.lock", "poetry.lock", "uv.lock",
)

private val OrbitCodeExtensions = setOf(
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "swift",
    "kt", "kts", "java", "c", "h", "cc", "cpp", "hpp", "cs", "rb",
    "php", "lua", "zig", "nix", "proto", "graphql", "gql", "css",
    "scss", "less", "html", "vue", "svelte", "astro",
)

/**
 * Semantic file icon aligned with Desktop & iOS.
 */
fun orbitSemanticFileIcon(basename: String): ImageVector {
    val lower = basename.lowercase()
    if (lower in OrbitLockfileNames || lower.endsWith(".lock")) {
        return Icons.Outlined.Lock
    }
    if (lower == "dockerfile" || lower.startsWith("dockerfile.") ||
        lower.startsWith(".env") || ".config." in lower
    ) {
        return Icons.Outlined.Settings
    }
    if (lower == "makefile" || lower == "justfile") {
        return Icons.Outlined.Terminal
    }
    if (lower == "license" || lower.startsWith("readme") || lower.startsWith("changelog")) {
        return Icons.Outlined.Description
    }
    return when (lower.substringAfterLast('.', "")) {
        "json", "jsonc", "jsonl" -> Icons.Outlined.DataObject
        "toml", "yaml", "yml", "ini", "conf", "plist" -> Icons.Outlined.Settings
        "sh", "zsh", "bash", "fish", "ps1" -> Icons.Outlined.Terminal
        "md", "mdx", "txt", "rst", "adoc" -> Icons.Outlined.Description
        "png", "jpg", "jpeg", "webp", "gif", "svg", "ico", "icns", "bmp", "heic" -> Icons.Outlined.Image
        "sql", "sqlite", "db", "prisma" -> Icons.Outlined.Storage
        in OrbitCodeExtensions -> Icons.Outlined.Code
        else -> Icons.AutoMirrored.Outlined.InsertDriveFile
    }
}

private val CodexComposePath: Path by lazy {
    PathParser().parsePathString(
        "M8.086.457a6.105 6.105 0 0 1 3.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 0 0 .107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 0 1-.18 1.631.167.167 0 0 0 .04.155 5.982 5.982 0 0 1 1.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 0 1-2.934 1.851.162.162 0 0 0-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 0 0-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 0 1-2.595-.622 6.058 6.058 0 0 1-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 0 1-.495-1.283 6.11 6.11 0 0 1-.017-3.064.166.166 0 0 0 .008-.074.115.115 0 0 0-.037-.064 5.958 5.958 0 0 1-1.38-2.202 5.196 5.196 0 0 1-.333-1.589 6.915 6.915 0 0 1 .188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 0 0 .087-.087A6.016 6.016 0 0 1 5.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 0 0-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 0 0 1.46.864l1.94-3.272a.849.849 0 0 0 .007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 0 0 0 1.695h4.848a.849.849 0 0 0 0-1.696h-4.848z"
    ).toPath().apply { fillType = PathFillType.EvenOdd }
}

private val ClaudeComposePath: Path by lazy {
    PathParser().parsePathString(
        "M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 0 1-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z"
    ).toPath()
}

private val AntigravityComposePath: Path by lazy {
    PathParser().parsePathString(
        "M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z"
    ).toPath()
}

/**
 * Vector provider brand mark aligned 1:1 with Orbit Desktop (`provider-icons.tsx`) and iOS.
 */
@Composable
fun OrbitProviderIcon(
    provider: FfiProviderKind,
    size: Dp = 16.dp,
    modifier: Modifier = Modifier,
) {
    Canvas(modifier = modifier.size(size)) {
        val scale = this.size.minDimension / 24f
        val dx = (this.size.width - 24f * scale) * 0.5f
        val dy = (this.size.height - 24f * scale) * 0.5f

        withTransform({
            translate(dx, dy)
            scale(scale, scale, pivot = Offset.Zero)
        }) {
            when (provider) {
                FfiProviderKind.CODEX -> {
                    drawPath(
                        path = CodexComposePath,
                        brush = Brush.verticalGradient(
                            0.0f to Color(0xFF9E94FF),
                            0.5f to Color(0xFF6B8CFF),
                            1.0f to Color(0xFF3B43FF),
                            startY = 0f,
                            endY = 24f,
                        ),
                    )
                }
                FfiProviderKind.CLAUDE_CODE -> {
                    drawPath(
                        path = ClaudeComposePath,
                        color = Color(0xFFD97757),
                    )
                }
                FfiProviderKind.ANTIGRAVITY -> {
                    clipPath(AntigravityComposePath) {
                        val rectSize = Size(24f, 24f)
                        drawRect(color = Color(0xFF3186FF), size = rectSize)
                        drawRect(
                            brush = Brush.radialGradient(
                                0.0f to Color(0xFF00B95C),
                                0.58f to Color(0xD100B95C),
                                1.0f to Color(0x0000B95C),
                                center = Offset(2f, 11f),
                                radius = 14f,
                            ),
                            size = rectSize,
                        )
                        drawRect(
                            brush = Brush.radialGradient(
                                0.0f to Color(0xFFFC413D),
                                0.55f to Color(0xC7FC413D),
                                1.0f to Color(0x00FC413D),
                                center = Offset(20f, 6f),
                                radius = 13f,
                            ),
                            size = rectSize,
                        )
                        drawRect(
                            brush = Brush.radialGradient(
                                0.0f to Color(0xFFFFE432),
                                0.55f to Color(0xE6FBBC04),
                                1.0f to Color(0x00FBBC04),
                                center = Offset(10.5f, 2f),
                                radius = 10.5f,
                            ),
                            size = rectSize,
                        )
                    }
                }
                else -> {
                    val badgeColor = when (provider) {
                        FfiProviderKind.GEMINI_CLI -> Color(0xFF4285F4)
                        FfiProviderKind.CURSOR_AGENT -> Color(0xFF10B981)
                        FfiProviderKind.COPILOT -> Color(0xFF6366F1)
                        FfiProviderKind.OPEN_CODE -> Color(0xFF0EA5E9)
                        FfiProviderKind.GOOSE -> Color(0xFFF59E0B)
                        FfiProviderKind.PI -> Color(0xFFEC4899)
                        FfiProviderKind.KIMI -> Color(0xFF14B8A6)
                        FfiProviderKind.CLINE -> Color(0xFF8B5CF6)
                        FfiProviderKind.MISTRAL_VIBE -> Color(0xFFF97316)
                        FfiProviderKind.AUGMENT -> Color(0xFF3B82F6)
                        FfiProviderKind.ACP -> Color(0xFF64748B)
                        else -> Color(0xFF64748B)
                    }
                    drawCircle(
                        color = badgeColor,
                        radius = 10f,
                        center = Offset(12f, 12f),
                    )
                    drawCircle(
                        color = Color.White.copy(alpha = 0.88f),
                        radius = 3.5f,
                        center = Offset(12f, 12f),
                    )
                }
            }
        }
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
