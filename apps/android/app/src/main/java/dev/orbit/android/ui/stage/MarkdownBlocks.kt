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
import androidx.compose.ui.text.TextStyle
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
import uniffi.orbit_ffi_uniffi.FfiTableAlignment
import uniffi.orbit_ffi_uniffi.FfiTableCell
import uniffi.orbit_ffi_uniffi.detectDiagramKind

private sealed class MarkdownRenderSegment {
    data class ProseRun(val blocks: List<FfiRichBlock>) : MarkdownRenderSegment()
    data class TaskListItem(val block: FfiRichBlock.ListItem) : MarkdownRenderSegment()
    data class BlockQuote(val block: FfiRichBlock.BlockQuote) : MarkdownRenderSegment()
    data class CodeBlock(val block: FfiRichBlock.CodeBlock) : MarkdownRenderSegment()
    data class Table(val block: FfiRichBlock.Table) : MarkdownRenderSegment()
    data class MathBlock(val block: FfiRichBlock.MathBlock) : MarkdownRenderSegment()
    data object HorizontalRule : MarkdownRenderSegment()
}

private fun isProseMergeable(block: FfiRichBlock): Boolean = when (block) {
    is FfiRichBlock.Heading -> true
    is FfiRichBlock.Paragraph -> true
    is FfiRichBlock.ListItem -> block.checked == null
    is FfiRichBlock.BlockQuote,
    is FfiRichBlock.CodeBlock,
    is FfiRichBlock.Table,
    is FfiRichBlock.MathBlock,
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
                is FfiRichBlock.Table -> result += MarkdownRenderSegment.Table(block)
                is FfiRichBlock.MathBlock -> result += MarkdownRenderSegment.MathBlock(block)
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
                        val nestedIndentDp = (item.depth.toInt().coerceAtMost(6) * 16).dp
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(start = nestedIndentDp),
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
                        val diagramKind = detectDiagramKind(seg.block.language, seg.block.rawCode)
                        if (diagramKind != null && LocalDiagramRenderer.current != null) {
                            DiagramBlockSurface(kind = diagramKind, block = seg.block)
                        } else {
                            CodeBlockSurface(block = seg.block)
                        }
                    }

                    is MarkdownRenderSegment.Table -> {
                        MarkdownTableSurface(block = seg.block, textColor = textColor)
                    }

                    is MarkdownRenderSegment.MathBlock -> {
                        val mathShape = RoundedCornerShape(10.dp)
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(mathShape)
                                .background(colors.code, mathShape)
                                .border(OrbitSpacing.hairline, colors.hairline, mathShape)
                                .horizontalScroll(rememberScrollState())
                                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                        ) {
                            Text(
                                text = seg.block.tex,
                                style = typography.code,
                                color = textColor,
                            )
                        }
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
                val depth = block.depth.toInt().coerceAtMost(6)
                val bullet = when (depth % 3) {
                    1 -> "◦"
                    2 -> "▪"
                    else -> "•"
                }
                val prefix = if (block.ordered) "${block.index}.  " else "$bullet  "
                val baseIndentSp = (depth * 16).sp
                val indentStyle = ParagraphStyle(
                    lineHeight = typography.body.lineHeight,
                    textIndent = TextIndent(firstLine = baseIndentSp, restLine = (depth * 16 + 16).sp),
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
            is FfiRichBlock.Table,
            is FfiRichBlock.MathBlock,
            FfiRichBlock.HorizontalRule -> Unit
        }
    }
}

@Composable
private fun RichSpanText(
    spans: List<FfiInlineSpan>,
    textColor: Color,
    modifier: Modifier = Modifier,
    style: TextStyle = OrbitTheme.typography.body,
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
        style = style,
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
internal fun CodeBlockSurface(
    block: FfiRichBlock.CodeBlock,
    modifier: Modifier = Modifier,
) {
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
        modifier = modifier
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

@Composable
private fun MarkdownTableSurface(
    block: FfiRichBlock.Table,
    textColor: Color,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val colCount = maxOf(
        block.headers.size,
        block.rows.maxOfOrNull { it.cells.size } ?: 0,
        1,
    )
    val shape = RoundedCornerShape(OrbitSpacing.md)

    @Composable
    fun TableCellBox(
        cell: FfiTableCell?,
        isHeader: Boolean,
        colIndex: Int,
    ) {
        val align = block.alignments.getOrNull(colIndex) ?: FfiTableAlignment.NONE
        val boxAlign = when (align) {
            FfiTableAlignment.CENTER -> Alignment.Center
            FfiTableAlignment.RIGHT -> Alignment.CenterEnd
            else -> Alignment.CenterStart
        }
        Box(
            modifier = Modifier
                .width(OrbitSpacing.xl * 7)
                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
            contentAlignment = boxAlign,
        ) {
            RichSpanText(
                spans = cell?.spans ?: emptyList(),
                textColor = if (isHeader) colors.fgPrimary else textColor,
                style = if (isHeader) typography.subheadSemiBold else typography.subhead,
            )
        }
    }

    Box(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.code, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape)
            .horizontalScroll(rememberScrollState()),
    ) {
        Column(
            modifier = Modifier.width(IntrinsicSize.Max),
        ) {
            if (block.headers.isNotEmpty()) {
                Row(
                    modifier = Modifier
                        .background(colors.subtleFill)
                        .height(IntrinsicSize.Min),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    for (colIdx in 0 until colCount) {
                        if (colIdx > 0) {
                            Box(
                                modifier = Modifier
                                    .width(OrbitSpacing.hairline)
                                    .fillMaxHeight()
                                    .background(colors.hairline)
                            )
                        }
                        TableCellBox(
                            cell = block.headers.getOrNull(colIdx),
                            isHeader = true,
                            colIndex = colIdx,
                        )
                    }
                }
            }
            block.rows.forEachIndexed { rowIdx, row ->
                HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                Row(
                    modifier = Modifier
                        .background(if (rowIdx % 2 == 1) colors.subtleFill.copy(alpha = 0.45f) else Color.Transparent)
                        .height(IntrinsicSize.Min),
                    verticalAlignment = Alignment.Top,
                ) {
                    for (colIdx in 0 until colCount) {
                        if (colIdx > 0) {
                            Box(
                                modifier = Modifier
                                    .width(OrbitSpacing.hairline)
                                    .fillMaxHeight()
                                    .background(colors.hairline)
                            )
                        }
                        TableCellBox(
                            cell = row.cells.getOrNull(colIdx),
                            isHeader = false,
                            colIndex = colIdx,
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
