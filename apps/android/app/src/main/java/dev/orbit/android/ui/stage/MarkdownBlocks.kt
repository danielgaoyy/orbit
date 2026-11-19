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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.DisableSelection
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
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
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import dev.orbit.android.ui.theme.OrbitColors
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import dev.orbit.android.ui.theme.parseHexColor
import kotlinx.coroutines.delay
import uniffi.orbit_ffi_uniffi.FfiHighlightedLine
import uniffi.orbit_ffi_uniffi.FfiInlineSpan
import uniffi.orbit_ffi_uniffi.FfiRichBlock

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

    SelectionContainer {
        Column(
            modifier = modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            blocks.forEach { block ->
                when (block) {
                    is FfiRichBlock.Heading -> {
                        val headingStyle = if (block.level <= 2u) {
                            typography.title
                        } else {
                            typography.bodySemiBold
                        }
                        RichSpanText(
                            spans = block.spans,
                            style = headingStyle,
                            defaultColor = colors.fgPrimary,
                            colors = colors,
                        )
                    }
                    is FfiRichBlock.Paragraph -> {
                        RichSpanText(
                            spans = block.spans,
                            style = typography.body,
                            defaultColor = textColor,
                            colors = colors,
                        )
                    }
                    is FfiRichBlock.ListItem -> {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.Top,
                        ) {
                            val marker = when {
                                block.checked == true -> "☑"
                                block.checked == false -> "☐"
                                block.ordered -> "${block.index}."
                                else -> "•"
                            }
                            Text(
                                text = marker,
                                style = typography.bodyMedium,
                                color = colors.fgSecondary,
                                modifier = Modifier.width(OrbitSpacing.xl),
                            )
                            RichSpanText(
                                spans = block.spans,
                                style = typography.body,
                                defaultColor = textColor,
                                colors = colors,
                                modifier = Modifier.weight(1f),
                            )
                        }
                    }
                    is FfiRichBlock.BlockQuote -> {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = OrbitSpacing.xs),
                            verticalAlignment = Alignment.Top,
                        ) {
                            Box(
                                modifier = Modifier
                                    .width(OrbitSpacing.xs / 2)
                                    .height(OrbitSpacing.xl)
                                    .background(colors.fgMuted, RoundedCornerShape(OrbitSpacing.xs))
                            )
                            Spacer(modifier = Modifier.width(OrbitSpacing.md))
                            RichSpanText(
                                spans = block.spans,
                                style = typography.subhead,
                                defaultColor = colors.fgSecondary,
                                colors = colors,
                                modifier = Modifier.weight(1f),
                            )
                        }
                    }
                    is FfiRichBlock.CodeBlock -> {
                        CodeBlockSurface(block = block)
                    }
                    FfiRichBlock.HorizontalRule -> {
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

@Composable
private fun RichSpanText(
    spans: List<FfiInlineSpan>,
    style: TextStyle,
    defaultColor: Color,
    colors: OrbitColors,
    modifier: Modifier = Modifier,
) {
    val annotated = remember(spans, defaultColor, colors) {
        buildStyledSpans(spans, defaultColor, colors)
    }

    Text(
        text = annotated,
        style = style,
        color = defaultColor,
        modifier = modifier,
    )
}

private fun buildStyledSpans(
    spans: List<FfiInlineSpan>,
    defaultColor: Color,
    colors: OrbitColors,
): AnnotatedString = buildAnnotatedString {
    spans.forEach { sp ->
        val link = sp.linkUrl?.takeIf { it.isNotBlank() }
        val fg = when {
            link != null -> colors.primary
            sp.inlineCode -> colors.fgPrimary
            else -> defaultColor
        }
        val bg = if (sp.inlineCode) colors.subtleFill else Color.Transparent
        val decoration = when {
            link != null && sp.strikethrough -> TextDecoration.combine(
                listOf(TextDecoration.Underline, TextDecoration.LineThrough)
            )
            link != null -> TextDecoration.Underline
            sp.strikethrough -> TextDecoration.LineThrough
            else -> TextDecoration.None
        }
        val spanStyle = SpanStyle(
            color = fg,
            background = bg,
            fontWeight = when {
                sp.bold -> FontWeight.SemiBold
                sp.inlineCode -> FontWeight.Medium
                else -> null
            },
            fontStyle = if (sp.italic) FontStyle.Italic else FontStyle.Normal,
            fontFamily = if (sp.inlineCode) FontFamily.Monospace else null,
            textDecoration = decoration,
        )
        val content = if (sp.inlineCode) "\u00A0${sp.text}\u00A0" else sp.text
        if (link != null) {
            withLink(LinkAnnotation.Url(link, TextLinkStyles(style = spanStyle))) {
                append(content)
            }
        } else {
            withStyle(spanStyle) {
                append(content)
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
    var copied by remember { mutableStateOf(false) }
    var showLineNumbers by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(copied) {
        if (copied) {
            delay(1600)
            copied = false
        }
    }

    val fallbackLines = remember(block.rawCode) {
        block.rawCode.trimEnd('\n', '\r').split('\n')
    }
    val lineCount = if (block.lines.isNotEmpty()) block.lines.size else fallbackLines.size
    val gutterDigits = lineCount.toString().length.coerceAtLeast(2)

    val shape = RoundedCornerShape(OrbitSpacing.md)
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.code, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape)
    ) {
        DisableSelection {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = block.language.ifBlank { "code" },
                    style = typography.metaMedium,
                    color = colors.fgMuted,
                )
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    val toggleShape = RoundedCornerShape(OrbitSpacing.sm)
                    Box(
                        modifier = Modifier
                            .clip(toggleShape)
                            .background(
                                if (showLineNumbers) colors.subtleFill else Color.Transparent,
                                toggleShape,
                            )
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                showLineNumbers = !showLineNumbers
                            }
                            .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = "#",
                            style = typography.codeMedium,
                            color = if (showLineNumbers) colors.primary else colors.fgSecondary,
                        )
                    }

                    Row(
                        modifier = Modifier
                            .clip(RoundedCornerShape(OrbitSpacing.sm))
                            .clickable {
                                val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                                cm?.setPrimaryClip(ClipData.newPlainText("code", block.rawCode))
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                copied = true
                            }
                            .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    ) {
                        Icon(
                            imageVector = if (copied) Icons.Outlined.Check else Icons.Outlined.ContentCopy,
                            contentDescription = "Copy code",
                            tint = if (copied) colors.directAdded else colors.fgSecondary,
                            modifier = Modifier.size(OrbitSpacing.iconSm),
                        )
                        Text(
                            text = if (copied) "Copied" else "Copy",
                            style = typography.metaMedium,
                            color = if (copied) colors.directAdded else colors.fgSecondary,
                        )
                    }
                }
            }
        }
        HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(IntrinsicSize.Min)
                .padding(OrbitSpacing.md),
        ) {
            if (showLineNumbers) {
                DisableSelection {
                    Row(modifier = Modifier.fillMaxHeight()) {
                        Column(
                            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs / 2),
                        ) {
                            for (lineNum in 1..lineCount) {
                                Text(
                                    text = lineNum.toString().padStart(gutterDigits, ' '),
                                    style = typography.code,
                                    color = colors.fgMuted,
                                    maxLines = 1,
                                    softWrap = false,
                                )
                            }
                        }
                        Spacer(modifier = Modifier.width(OrbitSpacing.sm))
                        Box(
                            modifier = Modifier
                                .width(OrbitSpacing.hairline)
                                .fillMaxHeight()
                                .background(colors.hairline)
                        )
                        Spacer(modifier = Modifier.width(OrbitSpacing.sm))
                    }
                }
            }
            Column(
                modifier = Modifier
                    .weight(1f)
                    .horizontalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs / 2),
            ) {
                if (block.lines.isNotEmpty()) {
                    block.lines.forEach { ln ->
                        Text(
                            text = buildHighlightedLine(ln, colors.fgPrimary),
                            style = typography.code,
                            maxLines = 1,
                            softWrap = false,
                        )
                    }
                } else {
                    fallbackLines.forEach { ln ->
                        Text(
                            text = ln,
                            style = typography.code,
                            color = colors.fgPrimary,
                            maxLines = 1,
                            softWrap = false,
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
            withStyle(
                SpanStyle(
                    color = parseHexColor(tok.fgHex, fallbackFg),
                    fontWeight = if (tok.bold) FontWeight.SemiBold else FontWeight.Normal,
                    fontStyle = if (tok.italic) FontStyle.Italic else FontStyle.Normal,
                )
            ) {
                append(tok.text)
            }
        }
    }
