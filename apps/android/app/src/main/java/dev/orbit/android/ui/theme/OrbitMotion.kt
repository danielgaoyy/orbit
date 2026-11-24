package dev.orbit.android.ui.theme

import android.provider.Settings
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.Easing
import androidx.compose.animation.core.TweenSpec
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext

/**
 * Canonical *Orbital Quiet Precision* motion tokens matching `.interface-design/system.md`
 * and `docs/design/README.md` (fast ease-out, compositor-only, reduced-motion compliant).
 */
object OrbitMotion {
    const val DUR_FAST = 110
    const val DUR_UI = 150
    const val DUR_OVERLAY = 190
    const val DUR_PANEL = 240
    const val DUR_EXIT = 130

    /** Snappy entrance & tactile press: cubic-bezier(0.23, 1, 0.32, 1). */
    val EaseOutExpo: Easing = CubicBezierEasing(0.23f, 1.0f, 0.32f, 1.0f)

    /** Subtle opacity fades & disclosures: cubic-bezier(0.25, 1, 0.5, 1). */
    val EaseOutQuart: Easing = CubicBezierEasing(0.25f, 1.0f, 0.5f, 1.0f)

    /** Structural pane slides & foldable transitions: cubic-bezier(0.32, 0.72, 0, 1). */
    val EasePanel: Easing = CubicBezierEasing(0.32f, 0.72f, 0.0f, 1.0f)

    fun <T> uiSpec(reducedMotion: Boolean = false): TweenSpec<T> = tween(
        durationMillis = if (reducedMotion) 0 else DUR_UI,
        easing = EaseOutExpo,
    )

    fun <T> overlaySpec(reducedMotion: Boolean = false): TweenSpec<T> = tween(
        durationMillis = if (reducedMotion) 0 else DUR_OVERLAY,
        easing = EaseOutExpo,
    )

    fun <T> panelSpec(reducedMotion: Boolean = false): TweenSpec<T> = tween(
        durationMillis = if (reducedMotion) 0 else DUR_PANEL,
        easing = EasePanel,
    )

    fun <T> exitSpec(reducedMotion: Boolean = false): TweenSpec<T> = tween(
        durationMillis = if (reducedMotion) 0 else DUR_EXIT,
        easing = EaseOutQuart,
    )
}

@Composable
fun rememberReducedMotion(): Boolean {
    val context = LocalContext.current
    return remember(context) {
        runCatching {
            val scale = Settings.Global.getFloat(
                context.contentResolver,
                Settings.Global.ANIMATOR_DURATION_SCALE,
                1.0f,
            )
            scale == 0f
        }.getOrDefault(false)
    }
}
