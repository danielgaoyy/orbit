package dev.orbit.android.ui.stage

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.ParagraphStyle
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextIndent
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.orbit.android.ui.theme.OrbitColors
import dev.orbit.android.ui.theme.OrbitCopyIcon
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import dev.orbit.android.ui.theme.OrbitTypographyScale
import dev.orbit.android.ui.theme.parseHexColor
import kotlinx.coroutines.delay
import uniffi.orbit_ffi_uniffi.FfiHighlightedLine
import uniffi.orbit_ffi_uniffi.FfiInlineSpan
import uniffi.orbit_ffi_uniffi.FfiRichBlock

private sealed class MarkdownRenderSegment {
    data class ProseRun(val blocks: List<FfiRichBlock>) : MarkdownRenderSegment()
    data class TaskListItem(val block: FfiRichBlock.ListItem) : MarkdownRenderSegment()
    data class BlockQuote(val block: FfiRichBlock.BlockQuote) : MarkdownRenderSegment()
    data class CodeBlock(val block: FfiRichBlock.CodeBlock) : MarkdownRenderSegment()
    data object HorizontalRule : MarkdownRenderSegment()
}

private fun isProseMergeable(block: FfiRichBlock): Boolean = when (block) {
    is FfiRichBlock.Heading -> true
    is FfiRichBlock.Paragraph -> true
    is FfiRichBlock.ListItem -> block.checked == null
    is FfiRichBlock.BlockQuote,
    is FfiRichBlock.CodeBlock,
    FfiRichBlock.HorizontalRule -> false
}

private fun groupMarkdownBlocks(blocks: List<FfiRichBlock>): List<MarkdownRenderSegment> {
    val result = mutableListOf<MarkdownRenderSegment>()
    val pendingProse = mutableListOf<FfiRichBlock>()

    fun flushProse() {
        if (pendingProse.isNotEmpty()) {
            result += MarkdownRenderSegment.ProseRun(pendingProse.toList())
            pendingProse.clear()
        }
    }

    blocks.forEach { block ->
        if (isProseMergeable(block)) {
            pendingProse += block
        } else {
            flushProse()
            when (block) {
                is FfiRichBlock.ListItem -> result += MarkdownRenderSegment.TaskListItem(block)
                is FfiRichBlock.BlockQuote -> result += MarkdownRenderSegment.BlockQuote(block)
                is FfiRichBlock.CodeBlock -> result += MarkdownRenderSegment.CodeBlock(block)
                FfiRichBlock.HorizontalRule -> result += MarkdownRenderSegment.HorizontalRule
                is FfiRichBlock.Heading,
                is FfiRichBlock.Paragraph -> Unit
            }
        }
    }
    flushProse()
    return result
}

@Composable
fun MarkdownBlocks(
    blocks: List<FfiRichBlock>,
    fallbackText: String = "",
    textColor: Color = OrbitTheme.colors.fgPrimary,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography

    if (blocks.isEmpty()) {
        if (fallbackText.isNotBlank()) {
            SelectionContainer {
                Text(
                    text = fallbackText,
                    style = typography.body,
                    color = textColor,
                    modifier = modifier,
                )
            }
        }
        return
    }

    val segments = remember(blocks) { groupMarkdownBlocks(blocks) }

    SelectionContainer {
        Column(
            modifier = modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            segments.forEach { seg ->
                when (seg) {
                    is MarkdownRenderSegment.ProseRun -> {
                        val combined = remember(seg.blocks, textColor, colors, typography) {
                            buildCombinedProseAnnotatedString(
                                blocks = seg.blocks,
                                baseColor = textColor,
                                colors = colors,
                                typography = typography,
                            )
                        }
                        Text(
                            text = combined,
                            style = typography.body,
                            color = textColor,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }

                    is MarkdownRenderSegment.TaskListItem -> {
                        val item = seg.block
                        val checked = item.checked == true
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                            verticalAlignment = Alignment.Top,
                        ) {
                            val boxShape = RoundedCornerShape(4.dp)
                            Box(
                                modifier = Modifier
                                    .padding(top = 3.dp)
                                    .size(16.dp)
                                    .clip(boxShape)
                                    .background(
                                        if (checked) colors.primary else colors.subtleFill,
                                        boxShape,
                                    )
                                    .border(
                                        OrbitSpacing.hairline,
                                        if (checked) colors.primary else colors.hairline,
                                        boxShape,
                                    ),
                                contentAlignment = Alignment.Center,
                            ) {
                                if (checked) {
                                    Icon(
                                        imageVector = Icons.Outlined.Check,
                                        contentDescription = "Checked",
                                        tint = colors.onPrimary,
                                        modifier = Modifier.size(11.dp),
                                    )
                                }
                            }
                            RichSpanText(
                                spans = item.spans,
                                textColor = if (checked) colors.fgMuted else textColor,
                                modifier = Modifier.weight(1f),
                            )
                        }
                    }

                    is MarkdownRenderSegment.BlockQuote -> {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(IntrinsicSize.Min),
                        ) {
                            Box(
                                modifier = Modifier
                                    .width(2.dp)
                                    .fillMaxHeight()
                                    .background(colors.hairline, RoundedCornerShape(1.dp))
                            )
                            Spacer(modifier = Modifier.width(OrbitSpacing.md))
                            RichSpanText(
                                spans = seg.block.spans,
                                textColor = colors.fgSecondary,
                                modifier = Modifier
                                    .weight(1f)
                                    .padding(vertical = 2.dp),
                            )
                        }
                    }

                    is MarkdownRenderSegment.CodeBlock -> {
                        CodeBlockSurface(block = seg.block)
                    }

                    MarkdownRenderSegment.HorizontalRule -> {
                        HorizontalDivider(
                            thickness = OrbitSpacing.hairline,
                            color = colors.hairline,
                            modifier = Modifier.padding(vertical = OrbitSpacing.xs),
                        )
                    }
                }
            }
        }
    }
}

private fun buildCombinedProseAnnotatedString(
    blocks: List<FfiRichBlock>,
    baseColor: Color,
    colors: OrbitColors,
    typography: OrbitTypographyScale,
): AnnotatedString = buildAnnotatedString {
    blocks.forEachIndexed { idx, block ->
        val prev = blocks.getOrNull(idx - 1)
        if (idx > 0) {
            val tightListPair = prev is FfiRichBlock.ListItem && block is FfiRichBlock.ListItem
            append(if (tightListPair) "\n" else "\n\n")
        }

        when (block) {
            is FfiRichBlock.Heading -> {
                val (fontSize, lineHeight) = when {
                    block.level <= 1u -> 18.sp to 25.sp
                    block.level == 2u.toUByte() -> 16.5.sp to 23.sp
                    else -> 15.sp to 21.sp
                }
                withStyle(ParagraphStyle(lineHeight = lineHeight)) {
                    withStyle(
                        SpanStyle(
                            color = baseColor,
                            fontSize = fontSize,
                            fontWeight = FontWeight.SemiBold,
                        )
                    ) {
                        appendStyledSpans(
                            spans = block.spans,
                            baseColor = baseColor,
                            colors = colors,
                            typography = typography,
                        )
                    }
                }
            }

            is FfiRichBlock.Paragraph -> {
                withStyle(ParagraphStyle(lineHeight = typography.body.lineHeight)) {
                    appendStyledSpans(
                        spans = block.spans,
                        baseColor = baseColor,
                        colors = colors,
                        typography = typography,
                    )
                }
            }

            is FfiRichBlock.ListItem -> {
                val prefix = if (block.ordered) "${block.index}.  " else "•  "
                val indentStyle = ParagraphStyle(
                    lineHeight = typography.body.lineHeight,
                    textIndent = TextIndent(firstLine = 0.sp, restLine = 16.sp),
                )
                withStyle(indentStyle) {
                    withStyle(SpanStyle(color = colors.fgSecondary, fontWeight = FontWeight.Medium)) {
                        append(prefix)
                    }
                    appendStyledSpans(
                        spans = block.spans,
                        baseColor = baseColor,
                        colors = colors,
                        typography = typography,
                    )
                }
            }

            is FfiRichBlock.BlockQuote,
            is FfiRichBlock.CodeBlock,
            FfiRichBlock.HorizontalRule -> Unit
        }
    }
}

@Composable
private fun RichSpanText(
    spans: List<FfiInlineSpan>,
    textColor: Color,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val styled = remember(spans, textColor, colors, typography) {
        buildAnnotatedString {
            appendStyledSpans(
                spans = spans,
                baseColor = textColor,
                colors = colors,
                typography = typography,
            )
        }
    }
    Text(
        text = styled,
        style = typography.body,
        color = textColor,
        modifier = modifier,
    )
}

private fun AnnotatedString.Builder.appendStyledSpans(
    spans: List<FfiInlineSpan>,
    baseColor: Color,
    colors: OrbitColors,
    typography: OrbitTypographyScale,
) {
    spans.forEach { sp ->
        val decoration = when {
            sp.strikethrough && sp.linkUrl != null -> TextDecoration.combine(
                listOf(TextDecoration.LineThrough, TextDecoration.Underline)
            )
            sp.strikethrough -> TextDecoration.LineThrough
            sp.linkUrl != null -> TextDecoration.Underline
            else -> TextDecoration.None
        }
        val style = SpanStyle(
            color = if (sp.linkUrl != null) colors.primary else baseColor,
            fontWeight = if (sp.bold) FontWeight.SemiBold else null,
            fontStyle = if (sp.italic) FontStyle.Italic else null,
            fontFamily = if (sp.inlineCode) FontFamily.Monospace else null,
            fontSize = if (sp.inlineCode) 13.sp else typography.body.fontSize,
            background = if (sp.inlineCode) colors.subtleFill else Color.Transparent,
            textDecoration = decoration,
        )
        val url = sp.linkUrl?.takeIf { it.isNotBlank() }
        if (url != null) {
            withLink(
                LinkAnnotation.Url(
                    url = url,
                    styles = TextLinkStyles(style = style),
                )
            ) {
                append(sp.text)
            }
        } else {
            withStyle(style) {
                append(sp.text)
            }
        }
    }
}

@Composable
private fun CodeBlockSurface(block: FfiRichBlock.CodeBlock) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val context = LocalContext.current
    val haptics = LocalHapticFeedback.current
    var copied by remember(block.rawCode) { mutableStateOf(false) }

    LaunchedEffect(copied) {
        if (copied) {
            delay(1500L)
            copied = false
        }
    }

    val shape = RoundedCornerShape(12.dp)
    val defaultCodeFg = if (colors.isDark) Color(0xFFE6E8EE) else colors.fgPrimary

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.code, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = OrbitSpacing.md, end = OrbitSpacing.sm, top = 6.dp, bottom = 6.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = block.language.ifBlank { "text" },
                style = typography.metaMedium,
                color = colors.fgMuted,
            )
            Box(
                modifier = Modifier
                    .size(28.dp)
                    .clip(CircleShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                        cm?.setPrimaryClip(ClipData.newPlainText("Code block", block.rawCode))
                        copied = true
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = if (copied) Icons.Outlined.Check else OrbitCopyIcon,
                    contentDescription = "Copy code",
                    tint = if (copied) colors.directAdded else colors.fgMuted,
                    modifier = Modifier.size(14.dp),
                )
            }
        }
        HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

        SelectionContainer {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState())
                    .padding(OrbitSpacing.md),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                if (block.lines.isEmpty()) {
                    Text(
                        text = block.rawCode,
                        style = typography.code,
                        color = defaultCodeFg,
                    )
                } else {
                    block.lines.forEach { line ->
                        Text(
                            text = buildHighlightedLine(line, defaultCodeFg),
                            style = typography.code,
                        )
                    }
                }
            }
        }
    }
}

private fun buildHighlightedLine(line: FfiHighlightedLine, fallbackFg: Color): AnnotatedString =
    buildAnnotatedString {
        line.tokens.forEach { tok ->
            pushStyle(
                SpanStyle(
                    color = parseHexColor(tok.fgHex, fallbackFg),
                    fontWeight = if (tok.bold) FontWeight.SemiBold else FontWeight.Normal,
                    fontStyle = if (tok.italic) FontStyle.Italic else FontStyle.Normal,
                )
            )
            append(tok.text)
            pop()
        }
    }
