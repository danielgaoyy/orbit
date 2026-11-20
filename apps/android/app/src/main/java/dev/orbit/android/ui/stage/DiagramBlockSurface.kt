package dev.orbit.android.ui.stage

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Color as AndroidColor
import android.view.ViewGroup
import android.webkit.WebView
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AccountTree
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.OpenInFull
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import dev.orbit.android.ui.theme.OrbitCopyIcon
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import kotlinx.coroutines.delay
import uniffi.orbit_ffi_uniffi.FfiDiagramKind
import uniffi.orbit_ffi_uniffi.FfiRenderDiagramView
import uniffi.orbit_ffi_uniffi.FfiRichBlock

val LocalDiagramRenderer =
    staticCompositionLocalOf<(suspend (FfiDiagramKind, String, Boolean) -> FfiRenderDiagramView?)?> {
        null
    }

private fun buildDiagramHtml(svg: String, isDark: Boolean, zoomable: Boolean): String {
    val fgHex = if (isDark) "#F4F4F5" else "#18181B"
    val viewport = if (zoomable) {
        """<meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=5.0, user-scalable=yes">"""
    } else {
        """<meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">"""
    }
    return """
        <!doctype html>
        <html>
        <head>
        <meta charset="utf-8">
        $viewport
        <style>
          html, body {
            margin: 0;
            padding: 0;
            width: 100%;
            height: 100%;
            background: transparent;
            color: $fgHex;
            font-family: sans-serif;
          }
          .wrap {
            width: 100%;
            min-height: 100%;
            display: flex;
            align-items: center;
            justify-content: center;
            box-sizing: border-box;
            padding: ${if (zoomable) "16px" else "8px"};
          }
          svg {
            display: block;
            max-width: 100%;
            max-height: 100%;
            width: auto;
            height: auto;
          }
        </style>
        </head>
        <body><div class="wrap">$svg</div></body>
        </html>
    """.trimIndent()
}

@Composable
private fun SvgWebCanvas(
    svg: String,
    isDark: Boolean,
    zoomable: Boolean,
    modifier: Modifier = Modifier,
) {
    val htmlTag = remember(svg, isDark, zoomable) { buildDiagramHtml(svg, isDark, zoomable) }
    AndroidView(
        modifier = modifier,
        factory = { ctx ->
            WebView(ctx).apply {
                layoutParams = ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT,
                )
                setBackgroundColor(AndroidColor.TRANSPARENT)
                settings.javaScriptEnabled = false
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                settings.setSupportZoom(zoomable)
                settings.builtInZoomControls = zoomable
                settings.displayZoomControls = false
                settings.loadWithOverviewMode = zoomable
                settings.useWideViewPort = zoomable
                isVerticalScrollBarEnabled = zoomable
                isHorizontalScrollBarEnabled = zoomable
                tag = htmlTag
                loadDataWithBaseURL(null, htmlTag, "text/html", "UTF-8", null)
            }
        },
        update = { web ->
            if (web.tag != htmlTag) {
                web.tag = htmlTag
                web.loadDataWithBaseURL(null, htmlTag, "text/html", "UTF-8", null)
            }
        },
        onRelease = { web ->
            web.stopLoading()
            web.loadUrl("about:blank")
            web.removeAllViews()
            web.destroy()
        },
    )
}

@Composable
internal fun DiagramBlockSurface(
    kind: FfiDiagramKind,
    block: FfiRichBlock.CodeBlock,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val context = LocalContext.current
    val haptics = LocalHapticFeedback.current
    val renderer = LocalDiagramRenderer.current

    var rendered by remember(kind, block.rawCode, colors.isDark) {
        mutableStateOf<FfiRenderDiagramView?>(null)
    }
    var isLoading by remember(kind, block.rawCode, colors.isDark) {
        mutableStateOf(renderer != null)
    }
    var showCode by remember(block.rawCode) { mutableStateOf(false) }
    var showFullscreen by remember { mutableStateOf(false) }
    var copied by remember(block.rawCode) { mutableStateOf(false) }

    LaunchedEffect(copied) {
        if (copied) {
            delay(1500L)
            copied = false
        }
    }

    LaunchedEffect(kind, block.rawCode, colors.isDark, renderer) {
        if (renderer == null) {
            isLoading = false
            return@LaunchedEffect
        }
        val hadRendered = rendered != null
        isLoading = true
        if (hadRendered) {
            delay(180L)
        }
        rendered = renderer(kind, block.rawCode, colors.isDark)
        isLoading = false
    }

    val baseTitle = when (kind) {
        FfiDiagramKind.MERMAID -> "Mermaid"
        FfiDiagramKind.GRAPHVIZ -> "Graphviz"
        FfiDiagramKind.SVG -> "SVG"
    }
    val headerTitle = rendered?.diagramTypeLabel
        ?.takeIf { it.isNotBlank() && it != baseTitle }
        ?.let { "$baseTitle · $it" }
        ?: baseTitle
    val svg = rendered?.svg?.takeIf { rendered?.error == null }

    val shape = RoundedCornerShape(12.dp)
    Column(
        modifier = modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.code, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = OrbitSpacing.md, end = OrbitSpacing.xs, top = 6.dp, bottom = 6.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(
                modifier = Modifier.weight(1f),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Icon(
                    imageVector = Icons.Outlined.AccountTree,
                    contentDescription = null,
                    tint = colors.fgMuted,
                    modifier = Modifier.size(14.dp),
                )
                Text(
                    text = headerTitle,
                    style = typography.metaMedium,
                    color = colors.fgMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }

            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (svg != null) {
                    Row(
                        modifier = Modifier
                            .clip(RoundedCornerShape(7.dp))
                            .background(colors.subtleFill)
                            .padding(2.dp),
                        horizontalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        DiagramModeTab(label = "Diagram", active = !showCode) { showCode = false }
                        DiagramModeTab(label = "Code", active = showCode) { showCode = true }
                    }

                    Box(
                        modifier = Modifier
                            .size(28.dp)
                            .clip(CircleShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                showFullscreen = true
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.OpenInFull,
                            contentDescription = "Expand diagram",
                            tint = colors.fgMuted,
                            modifier = Modifier.size(14.dp),
                        )
                    }
                }

                Box(
                    modifier = Modifier
                        .size(28.dp)
                        .clip(CircleShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                            cm?.setPrimaryClip(ClipData.newPlainText("Diagram source", block.rawCode))
                            copied = true
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = if (copied) Icons.Outlined.Check else OrbitCopyIcon,
                        contentDescription = "Copy diagram source",
                        tint = if (copied) colors.directAdded else colors.fgMuted,
                        modifier = Modifier.size(14.dp),
                    )
                }
            }
        }
        HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

        if (!showCode && svg != null && rendered != null) {
            val view = rendered!!
            val aspectRatio = (view.width / view.height.coerceAtLeast(1f)).coerceIn(0.35f, 3.0f)
            BoxWithConstraints(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { showFullscreen = true }
                    .padding(OrbitSpacing.xs),
            ) {
                val targetHeight = (maxWidth / aspectRatio).coerceIn(130.dp, 400.dp)
                SvgWebCanvas(
                    svg = svg,
                    isDark = colors.isDark,
                    zoomable = false,
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(targetHeight),
                )
            }
        } else if (isLoading && !showCode) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(96.dp),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                    color = colors.primary,
                )
            }
        } else {
            CodeBlockSurface(block = block, modifier = Modifier.padding(OrbitSpacing.xs))
        }
    }

    if (showFullscreen && svg != null) {
        Dialog(
            onDismissRequest = { showFullscreen = false },
            properties = DialogProperties(usePlatformDefaultWidth = false),
        ) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .background(colors.canvas),
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .statusBarsPadding()
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = headerTitle,
                        style = typography.subheadSemiBold,
                        color = colors.fgPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    Box(
                        modifier = Modifier
                            .size(36.dp)
                            .clip(CircleShape)
                            .background(colors.subtleFill)
                            .clickable { showFullscreen = false },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Close,
                            contentDescription = "Close fullscreen diagram",
                            tint = colors.fgPrimary,
                            modifier = Modifier.size(18.dp),
                        )
                    }
                }
                HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                SvgWebCanvas(
                    svg = svg,
                    isDark = colors.isDark,
                    zoomable = true,
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f),
                )
            }
        }
    }
}

@Composable
private fun DiagramModeTab(
    label: String,
    active: Boolean,
    onClick: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val bg = if (active) colors.elevated else colors.subtleFill
    Box(
        modifier = Modifier
            .clip(RoundedCornerShape(5.dp))
            .background(bg)
            .clickable(onClick = onClick)
            .padding(horizontal = 7.dp, vertical = 3.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = label,
            style = typography.metaMedium,
            color = if (active) colors.fgPrimary else colors.fgMuted,
        )
    }
}
