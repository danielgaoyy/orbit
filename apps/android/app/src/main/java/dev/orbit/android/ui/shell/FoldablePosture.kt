package dev.orbit.android.ui.shell

import androidx.activity.compose.LocalActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowInfoTracker
import androidx.window.layout.WindowLayoutInfo

/**
 * Plain geometry snapshot extracted from Android Jetpack WindowManager `FoldingFeature`
 * so hinge math can be unit-tested cleanly on the JVM.
 */
data class OrbitHingeGeometry(
    val isVerticalAxis: Boolean,
    val isSeparatingSurface: Boolean,
    val isHalfFolded: Boolean,
    val leftPx: Int,
    val topPx: Int,
    val rightPx: Int,
    val bottomPx: Int,
) {
    val spanWidthPx: Int get() = (rightPx - leftPx).coerceAtLeast(0)
    val spanHeightPx: Int get() = (bottomPx - topPx).coerceAtLeast(0)
}

/**
 * Hinge-aware layout posture for Orbit Android foldables & tablets.
 */
sealed interface OrbitFoldablePosture {
    data object FlatOrStandard : OrbitFoldablePosture

    data class BookDualPane(
        val masterWidthDp: Dp,
        val hingeGapDp: Dp,
    ) : OrbitFoldablePosture

    data class TabletopSplit(
        val upperStageHeightDp: Dp,
        val hingeGapDp: Dp,
    ) : OrbitFoldablePosture
}

object OrbitFoldablePostureMapper {
    private const val MIN_SIDE_PANE_DP = 220f
    private const val MIN_LOWER_DECK_DP = 160f

    fun mapGeometry(
        hinge: OrbitHingeGeometry?,
        viewportWidthPx: Int,
        viewportHeightPx: Int,
        density: Float,
    ): OrbitFoldablePosture {
        if (hinge == null || density <= 0f || viewportWidthPx <= 0 || viewportHeightPx <= 0) {
            return OrbitFoldablePosture.FlatOrStandard
        }

        if (hinge.isVerticalAxis && (hinge.isSeparatingSurface || hinge.isHalfFolded)) {
            val leftWidthDp = hinge.leftPx / density
            val gapDp = hinge.spanWidthPx / density
            val rightWidthDp = (viewportWidthPx - hinge.rightPx) / density
            if (leftWidthDp >= MIN_SIDE_PANE_DP && rightWidthDp >= MIN_SIDE_PANE_DP) {
                return OrbitFoldablePosture.BookDualPane(
                    masterWidthDp = leftWidthDp.dp,
                    hingeGapDp = gapDp.dp,
                )
            }
        }

        if (!hinge.isVerticalAxis && hinge.isHalfFolded) {
            val topHeightDp = hinge.topPx / density
            val gapDp = hinge.spanHeightPx / density
            val bottomHeightDp = (viewportHeightPx - hinge.bottomPx) / density
            if (topHeightDp >= MIN_SIDE_PANE_DP && bottomHeightDp >= MIN_LOWER_DECK_DP) {
                return OrbitFoldablePosture.TabletopSplit(
                    upperStageHeightDp = topHeightDp.dp,
                    hingeGapDp = gapDp.dp,
                )
            }
        }

        return OrbitFoldablePosture.FlatOrStandard
    }

    fun fromLayoutInfo(
        layoutInfo: WindowLayoutInfo?,
        viewportWidthPx: Int,
        viewportHeightPx: Int,
        density: Float,
    ): OrbitFoldablePosture {
        val folding = layoutInfo
            ?.displayFeatures
            ?.filterIsInstance<FoldingFeature>()
            ?.firstOrNull()
            ?: return OrbitFoldablePosture.FlatOrStandard

        return mapGeometry(
            hinge = OrbitHingeGeometry(
                isVerticalAxis = folding.orientation == FoldingFeature.Orientation.VERTICAL,
                isSeparatingSurface = folding.isSeparating,
                isHalfFolded = folding.state == FoldingFeature.State.HALF_OPENED,
                leftPx = folding.bounds.left,
                topPx = folding.bounds.top,
                rightPx = folding.bounds.right,
                bottomPx = folding.bounds.bottom,
            ),
            viewportWidthPx = viewportWidthPx,
            viewportHeightPx = viewportHeightPx,
            density = density,
        )
    }
}

@Composable
fun rememberOrbitFoldablePosture(
    viewportWidthPx: Int,
    viewportHeightPx: Int,
): OrbitFoldablePosture {
    val activity = LocalActivity.current
    val density = LocalDensity.current.density

    val layoutInfo by produceState<WindowLayoutInfo?>(initialValue = null, key1 = activity) {
        if (activity == null) {
            value = null
            return@produceState
        }
        runCatching {
            WindowInfoTracker.getOrCreate(activity)
                .windowLayoutInfo(activity)
                .collect { next -> value = next }
        }
    }

    return OrbitFoldablePostureMapper.fromLayoutInfo(
        layoutInfo = layoutInfo,
        viewportWidthPx = viewportWidthPx,
        viewportHeightPx = viewportHeightPx,
        density = density,
    )
}
