package dev.orbit.android.ui.sheets

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
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardReturn
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.FormatQuote
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
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
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import dev.orbit.android.ui.theme.parseHexColor
import uniffi.orbit_ffi_uniffi.FfiTerminalRow
import uniffi.orbit_ffi_uniffi.FfiTerminalScreen
import uniffi.orbit_ffi_uniffi.FfiTerminalSummary

private data class DevKeySpec(
    val label: String,
    val sequence: String? = null,
    val isStickyCtrl: Boolean = false,
    val isStickyAlt: Boolean = false,
)

private val DeveloperKeys = listOf(
    DevKeySpec("ESC", "\u001B"),
    DevKeySpec("TAB", "\t"),
    DevKeySpec("CTRL", isStickyCtrl = true),
    DevKeySpec("ALT", isStickyAlt = true),
    DevKeySpec("^C", "\u0003"),
    DevKeySpec("^D", "\u0004"),
    DevKeySpec("^L", "\u000C"),
    DevKeySpec("↑", "\u001B[A"),
    DevKeySpec("↓", "\u001B[B"),
    DevKeySpec("←", "\u001B[D"),
    DevKeySpec("→", "\u001B[C"),
    DevKeySpec("|", "|"),
    DevKeySpec("/", "/"),
    DevKeySpec("-", "-"),
    DevKeySpec("~", "~"),
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PtyTerminalSheet(
    cwd: String,
    terminals: List<FfiTerminalSummary>,
    activeTerminalId: String?,
    screen: FfiTerminalScreen?,
    onEnsureStarted: () -> Unit,
    onSpawnTerminal: () -> Unit,
    onAttachTerminal: (String) -> Unit,
    onKillTerminal: (String) -> Unit,
    onViewportMeasured: (cols: Int, rows: Int) -> Unit,
    onSendKeys: (String) -> Unit,
    onCopiedOutput: () -> Unit,
    onQuoteTerminal: () -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val context = LocalContext.current
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    val textMeasurer = rememberTextMeasurer()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    var stickyCtrl by remember { mutableStateOf(false) }
    var stickyAlt by remember { mutableStateOf(false) }
    var inputLine by remember { mutableStateOf("") }

    val visibleSnapshotText = remember(screen) {
        screen?.rowsData.orEmpty()
            .map { row -> row.spans.joinToString("") { it.text }.trimEnd() }
            .dropLastWhile { it.isBlank() }
            .joinToString("\n")
    }

    LaunchedEffect(cwd) {
        onEnsureStarted()
    }

    val codeStyle = typography.code
    val cellMetrics = remember(codeStyle, density) {
        val res = textMeasurer.measure(
            text = AnnotatedString("MMMMMMMMMM"),
            style = codeStyle,
        )
        val charW = (res.size.width / 10f).coerceAtLeast(6f)
        val rowH = res.size.height.toFloat().coerceAtLeast(12f)
        charW to rowH
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = colors.elevated,
        contentColor = colors.fgPrimary,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.88f)
                .imePadding()
                .navigationBarsPadding(),
        ) {
            // Terminal Header Bar: Scrollable Session Tabs + Pinned Copy & Quote Actions
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Row(
                    modifier = Modifier
                        .weight(1f)
                        .horizontalScroll(rememberScrollState()),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    if (terminals.isEmpty()) {
                        Text(
                            text = "PTY · ${OrbitFormatters.shortPath(cwd)}",
                            style = typography.subheadSemiBold,
                            color = colors.fgPrimary,
                        )
                    } else {
                        terminals.forEachIndexed { idx, term ->
                            val isActive = term.terminalId == activeTerminalId ||
                                (activeTerminalId == null && idx == 0)
                            val tabShape = RoundedCornerShape(OrbitSpacing.sm)
                            Row(
                                modifier = Modifier
                                    .heightIn(min = 44.dp)
                                    .clip(tabShape)
                                    .background(if (isActive) colors.code else colors.subtleFill, tabShape)
                                    .border(
                                        OrbitSpacing.hairline,
                                        if (isActive) colors.primary else colors.hairline,
                                        tabShape,
                                    )
                                    .clickable {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        onAttachTerminal(term.terminalId)
                                    }
                                    .padding(start = OrbitSpacing.sm),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                            ) {
                                Text(
                                    text = term.title.ifBlank { OrbitFormatters.shortPath(term.cwd) },
                                    style = typography.codeMedium,
                                    color = if (isActive) colors.primary else colors.fgSecondary,
                                )
                                Box(
                                    modifier = Modifier
                                        .size(44.dp)
                                        .clickable {
                                            haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                            onKillTerminal(term.terminalId)
                                        },
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Icon(
                                        imageVector = Icons.Outlined.Close,
                                        contentDescription = "Kill terminal session",
                                        tint = colors.fgMuted,
                                        modifier = Modifier.size(OrbitSpacing.iconSm),
                                    )
                                }
                            }
                        }
                    }

                    // + Spawn New Terminal Tab (>= 44.dp touch target)
                    Box(
                        modifier = Modifier
                            .size(44.dp)
                            .clip(RoundedCornerShape(OrbitSpacing.sm))
                            .background(colors.subtleFill, RoundedCornerShape(OrbitSpacing.sm))
                            .border(OrbitSpacing.hairline, colors.hairline, RoundedCornerShape(OrbitSpacing.sm))
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onSpawnTerminal()
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Add,
                            contentDescription = "New terminal tab",
                            tint = colors.fgPrimary,
                            modifier = Modifier.size(OrbitSpacing.iconSm),
                        )
                    }

                    if (screen != null) {
                        Text(
                            text = "${screen.cols}×${screen.rows}",
                            style = typography.meta,
                            color = colors.fgMuted,
                            modifier = Modifier.padding(start = OrbitSpacing.xs),
                        )
                    }
                }

                // Copy & Quote Header Micro-Actions
                val actionShape = RoundedCornerShape(OrbitSpacing.sm)
                Row(
                    modifier = Modifier
                        .heightIn(min = 36.dp)
                        .clip(actionShape)
                        .background(colors.subtleFill, actionShape)
                        .border(OrbitSpacing.hairline, colors.hairline, actionShape)
                        .clickable {
                            val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                            cm?.setPrimaryClip(ClipData.newPlainText("terminal_output", visibleSnapshotText))
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onCopiedOutput()
                        }
                        .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.ContentCopy,
                        contentDescription = "Copy terminal output",
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                    Text(
                        text = "Copy",
                        style = typography.metaMedium,
                        color = colors.fgPrimary,
                        maxLines = 1,
                        softWrap = false,
                    )
                }

                Row(
                    modifier = Modifier
                        .heightIn(min = 36.dp)
                        .clip(actionShape)
                        .background(colors.subtleFill, actionShape)
                        .border(OrbitSpacing.hairline, colors.warning.copy(alpha = 0.55f), actionShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onQuoteTerminal()
                        }
                        .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.FormatQuote,
                        contentDescription = "Quote terminal into composer",
                        tint = colors.warning,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                    Text(
                        text = "Quote",
                        style = typography.metaSemiBold,
                        color = colors.warning,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            // Dynamic Pixel-Measured PTY Screen Canvas
            val termBg = if (colors.isDark) colors.code else Color(0xFF0E1015)
            val defaultTermFg = Color(0xFFE6E8EE)
            val padPx = with(density) { (OrbitSpacing.md * 2).toPx() }

            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.sm)
                    .clip(RoundedCornerShape(OrbitSpacing.md))
                    .background(termBg, RoundedCornerShape(OrbitSpacing.md))
                    .border(OrbitSpacing.hairline, colors.hairline, RoundedCornerShape(OrbitSpacing.md))
                    .onSizeChanged { size ->
                        val usableW = (size.width - padPx).coerceAtLeast(120f)
                        val usableH = (size.height - padPx).coerceAtLeast(80f)
                        val (charW, rowH) = cellMetrics
                        val cols = (usableW / charW).toInt()
                        val rows = (usableH / rowH).toInt()
                        onViewportMeasured(cols, rows)
                    }
                    .padding(OrbitSpacing.md),
            ) {
                val rowsData: List<FfiTerminalRow> = screen?.rowsData.orEmpty()
                if (rowsData.isEmpty()) {
                    Text(
                        text = "Connecting to remote shim PTY...",
                        style = typography.code,
                        color = Color(0xFF8E929E),
                    )
                } else {
                    LazyColumn(
                        modifier = Modifier
                            .fillMaxSize()
                            .horizontalScroll(rememberScrollState()),
                    ) {
                        items(rowsData, key = { it.rowIndex.toInt() }) { row ->
                            val isCursorRow = screen != null && row.rowIndex == screen.cursorRow
                            val cursorCol = screen?.cursorCol?.toInt() ?: -1
                            Text(
                                text = buildTerminalRowString(
                                    row = row,
                                    defaultFg = defaultTermFg,
                                    defaultBg = termBg,
                                    cursorCol = if (isCursorRow) cursorCol else -1,
                                    cursorAccent = colors.primary,
                                ),
                                style = typography.code,
                            )
                        }
                    }
                }
            }

            // Developer Keybar (ESC, TAB, sticky CTRL, sticky ALT, ^C, ^D, arrows, |, /, -, ~)
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState())
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs),
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                DeveloperKeys.forEach { key ->
                    val latched = (key.isStickyCtrl && stickyCtrl) || (key.isStickyAlt && stickyAlt)
                    val keyShape = RoundedCornerShape(OrbitSpacing.sm)
                    Box(
                        modifier = Modifier
                            .heightIn(min = 38.dp)
                            .clip(keyShape)
                            .background(
                                if (latched) colors.primary else colors.subtleFill,
                                keyShape
                            )
                            .border(OrbitSpacing.hairline, colors.hairline, keyShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                when {
                                    key.isStickyCtrl -> stickyCtrl = !stickyCtrl
                                    key.isStickyAlt -> stickyAlt = !stickyAlt
                                    key.sequence != null -> {
                                        onSendKeys(key.sequence)
                                        stickyCtrl = false
                                        stickyAlt = false
                                    }
                                }
                            }
                            .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = key.label,
                            style = typography.codeMedium,
                            color = if (latched) Color.White else colors.fgPrimary,
                        )
                    }
                }
            }

            // Direct Shell Input Row
            val inputShape = RoundedCornerShape(OrbitSpacing.md)
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(
                        start = OrbitSpacing.lg,
                        end = OrbitSpacing.lg,
                        top = OrbitSpacing.xs,
                        bottom = OrbitSpacing.sm,
                    ),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 44.dp)
                        .background(colors.code, inputShape)
                        .border(OrbitSpacing.hairline, colors.hairline, inputShape)
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    if (inputLine.isEmpty()) {
                        val hint = when {
                            stickyCtrl -> "CTRL + key..."
                            stickyAlt -> "ALT + key..."
                            else -> "Type shell command..."
                        }
                        Text(
                            text = hint,
                            style = typography.code,
                            color = if (stickyCtrl || stickyAlt) colors.primary else colors.fgMuted,
                        )
                    }
                    BasicTextField(
                        value = inputLine,
                        onValueChange = { next ->
                            if ((stickyCtrl || stickyAlt) && next.length == 1 && inputLine.isEmpty()) {
                                val ch = next[0]
                                val seq = when {
                                    stickyCtrl && ch.uppercaseChar() in 'A'..'Z' -> {
                                        ((ch.uppercaseChar().code - 64).toChar()).toString()
                                    }
                                    stickyAlt -> "\u001B$ch"
                                    else -> next
                                }
                                onSendKeys(seq)
                                stickyCtrl = false
                                stickyAlt = false
                                inputLine = ""
                            } else {
                                inputLine = next
                            }
                        },
                        textStyle = typography.code.copy(color = colors.fgPrimary),
                        cursorBrush = SolidColor(colors.primary),
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(
                            capitalization = KeyboardCapitalization.None,
                            autoCorrectEnabled = false,
                            keyboardType = KeyboardType.Ascii,
                            imeAction = ImeAction.Send,
                        ),
                        keyboardActions = KeyboardActions(
                            onSend = {
                                onSendKeys("$inputLine\r")
                                inputLine = ""
                            }
                        ),
                        modifier = Modifier.fillMaxWidth(),
                    )
                }

                Box(
                    modifier = Modifier
                        .size(OrbitSpacing.touchTarget)
                        .clip(inputShape)
                        .background(colors.primary, inputShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onSendKeys("$inputLine\r")
                            inputLine = ""
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Outlined.KeyboardReturn,
                        contentDescription = "Send Enter",
                        tint = Color.White,
                        modifier = Modifier.size(OrbitSpacing.iconMd),
                    )
                }
            }
        }
    }
}

private fun buildTerminalRowString(
    row: FfiTerminalRow,
    defaultFg: Color,
    defaultBg: Color,
    cursorCol: Int,
    cursorAccent: Color,
): AnnotatedString = buildAnnotatedString {
    var colOffset = 0
    row.spans.forEach { sp ->
        val rawFg = parseHexColor(sp.fgHex, defaultFg)
        val rawBg = sp.bgHex?.let { parseHexColor(it, defaultBg) } ?: Color.Transparent
        val fg = if (sp.inverse) (if (rawBg == Color.Transparent) defaultBg else rawBg) else rawFg
        val bg = if (sp.inverse) rawFg else rawBg
        val baseStyle = SpanStyle(
            color = fg,
            background = bg,
            fontWeight = if (sp.bold) FontWeight.Bold else FontWeight.Normal,
            textDecoration = if (sp.underline) TextDecoration.Underline else TextDecoration.None,
        )

        val endCol = colOffset + sp.text.length
        if (cursorCol in colOffset until endCol && sp.text.isNotEmpty()) {
            val localIdx = cursorCol - colOffset
            if (localIdx > 0) {
                pushStyle(baseStyle)
                append(sp.text.substring(0, localIdx))
                pop()
            }
            pushStyle(
                baseStyle.copy(
                    color = Color.White,
                    background = cursorAccent,
                )
            )
            append(sp.text[localIdx])
            pop()
            if (localIdx + 1 < sp.text.length) {
                pushStyle(baseStyle)
                append(sp.text.substring(localIdx + 1))
                pop()
            }
        } else {
            pushStyle(baseStyle)
            append(sp.text)
            pop()
        }
        colOffset = endCol
    }
    if (cursorCol >= colOffset && cursorCol >= 0) {
        pushStyle(SpanStyle(color = Color.White, background = cursorAccent))
        append(" ")
        pop()
    }
}
