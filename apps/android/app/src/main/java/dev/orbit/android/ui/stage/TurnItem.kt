package dev.orbit.android.ui.stage

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.FormatQuote
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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import kotlinx.coroutines.delay
import uniffi.orbit_ffi_uniffi.FfiDiffFile
import uniffi.orbit_ffi_uniffi.FfiDiffLineKind
import uniffi.orbit_ffi_uniffi.FfiPromptBadge
import uniffi.orbit_ffi_uniffi.FfiRichBlock
import uniffi.orbit_ffi_uniffi.FfiStageTurn
import uniffi.orbit_ffi_uniffi.FfiTimelineItem
import uniffi.orbit_ffi_uniffi.FfiToolCallDetail
import uniffi.orbit_ffi_uniffi.FfiToolStatus
import uniffi.orbit_ffi_uniffi.FfiToolStepDescriptor
import uniffi.orbit_ffi_uniffi.FfiTurnFileDiffSummary
import uniffi.orbit_ffi_uniffi.FfiTurnLiveSegment

@Composable
fun TurnItem(
    turn: FfiStageTurn,
    describeStep: (FfiTimelineItem) -> FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
    onCopyAssistantReply: (String) -> Unit,
    onQuoteAssistantReply: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors

    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
    ) {
        // 1. User Prompt Bubble (right-aligned, elevated surface, 18.dp radius, 0.5.dp hairline)
        (turn.userPrompt as? FfiTimelineItem.UserPrompt)?.let { prompt ->
            UserPromptBubble(prompt = prompt)
        }

        if (turn.isCompactionCommand) {
            CompactionDividerRow(label = "Session context compacted")
        }

        // 2. Live Running Trajectory OR Settled Collapsible Trajectory
        if (turn.hasActiveItem) {
            LiveTurnSegments(
                turn = turn,
                describeStep = describeStep,
                onOpenDiffFile = onOpenDiffFile,
                onQuoteDiffLine = onQuoteDiffLine,
            )
        } else {
            if (turn.foldedSteps.isNotEmpty()) {
                SettledWorkTrajectory(
                    turn = turn,
                    describeStep = describeStep,
                    onOpenDiffFile = onOpenDiffFile,
                    onQuoteDiffLine = onQuoteDiffLine,
                )
            }

            // 3. De-Carded Editorial Assistant Reply directly on bg.canvas + subtle Copy / Quote row
            (turn.terminalReply as? FfiTimelineItem.AssistantMessage)?.let { reply ->
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    MarkdownBlocks(
                        blocks = reply.blocks,
                        fallbackText = reply.text,
                        textColor = colors.fgPrimary,
                    )
                    val fullPlainText = remember(reply.text, reply.blocks) {
                        extractAssistantReplyText(reply)
                    }
                    if (fullPlainText.isNotBlank()) {
                        AssistantTurnMicroActions(
                            onCopy = { onCopyAssistantReply(fullPlainText) },
                            onQuote = { onQuoteAssistantReply(fullPlainText) },
                        )
                    }
                }
            }

            // 4. Turn File Diff Summary Chips
            if (turn.turnDiffs.isNotEmpty()) {
                TurnFileDiffStrip(
                    diffs = turn.turnDiffs,
                    onOpenDiffFile = onOpenDiffFile,
                )
            }

        // 5. Turn Notices
            turn.notices.filterIsInstance<FfiTimelineItem.Notice>().forEach { notice ->
                NoticeBannerRow(notice = notice)
            }
        }
    }
}

private fun extractAssistantReplyText(reply: FfiTimelineItem.AssistantMessage): String {
    if (reply.text.isNotBlank()) return reply.text.trim()
    return buildString {
        reply.blocks.forEach { block ->
            when (block) {
                is FfiRichBlock.Heading -> appendLine(block.spans.joinToString("") { it.text })
                is FfiRichBlock.Paragraph -> appendLine(block.spans.joinToString("") { it.text })
                is FfiRichBlock.ListItem -> appendLine("• " + block.spans.joinToString("") { it.text })
                is FfiRichBlock.BlockQuote -> appendLine("> " + block.spans.joinToString("") { it.text })
                is FfiRichBlock.CodeBlock -> {
                    appendLine("```${block.language}")
                    appendLine(block.rawCode)
                    appendLine("```")
                }
                is FfiRichBlock.Table -> {
                    if (block.headers.isNotEmpty()) {
                        appendLine(block.headers.joinToString(" | ") { cell -> cell.spans.joinToString("") { it.text } })
                    }
                    block.rows.forEach { row ->
                        appendLine(row.cells.joinToString(" | ") { cell -> cell.spans.joinToString("") { it.text } })
                    }
                }
                FfiRichBlock.HorizontalRule -> appendLine("---")
            }
        }
    }.trim()
}

@Composable
private fun AssistantTurnMicroActions(
    onCopy: () -> Unit,
    onQuote: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    var copied by remember { mutableStateOf(false) }

    LaunchedEffect(copied) {
        if (copied) {
            delay(1600)
            copied = false
        }
    }

    val actionShape = RoundedCornerShape(OrbitSpacing.xs)
    Row(
        modifier = Modifier.padding(top = OrbitSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
    ) {
        Row(
            modifier = Modifier
                .clip(actionShape)
                .clickable {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    copied = true
                    onCopy()
                }
                .padding(horizontal = OrbitSpacing.xs, vertical = OrbitSpacing.xs),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
        ) {
            Icon(
                imageVector = if (copied) Icons.Outlined.Check else Icons.Outlined.ContentCopy,
                contentDescription = "Copy assistant reply",
                tint = if (copied) colors.directAdded else colors.fgMuted,
                modifier = Modifier.size(OrbitSpacing.iconSm),
            )
            Text(
                text = if (copied) "Copied" else "Copy",
                style = typography.metaMedium,
                color = if (copied) colors.directAdded else colors.fgMuted,
            )
        }

        Row(
            modifier = Modifier
                .clip(actionShape)
                .clickable {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    onQuote()
                }
                .padding(horizontal = OrbitSpacing.xs, vertical = OrbitSpacing.xs),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
        ) {
            Icon(
                imageVector = Icons.Outlined.FormatQuote,
                contentDescription = "Quote assistant reply",
                tint = colors.fgMuted,
                modifier = Modifier.size(OrbitSpacing.iconSm),
            )
            Text(
                text = "Quote",
                style = typography.metaMedium,
                color = colors.fgMuted,
            )
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun UserPromptBubble(prompt: FfiTimelineItem.UserPrompt) {
    val colors = OrbitTheme.colors
    val shape = RoundedCornerShape(18.dp)

    Box(
        modifier = Modifier.fillMaxWidth(),
        contentAlignment = Alignment.CenterEnd,
    ) {
        Column(
            modifier = Modifier
                .widthIn(max = 560.dp)
                .clip(shape)
                .background(colors.userBubble, shape)
                .border(OrbitSpacing.hairline, colors.hairline, shape)
                .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.md),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            if (prompt.badges.isNotEmpty()) {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    prompt.badges.forEach { badge ->
                        PromptBadgePill(badge = badge)
                    }
                }
            }
            MarkdownBlocks(
                blocks = prompt.blocks,
                fallbackText = prompt.text,
                textColor = colors.fgPrimary,
            )
        }
    }
}

@Composable
private fun PromptBadgePill(badge: FfiPromptBadge) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val accent = when (badge.kind) {
        "git" -> colors.directAdded
        "slash" -> colors.primary
        "quote" -> colors.relay
        "terminal" -> colors.warning
        else -> colors.fgSecondary
    }
    val pillShape = RoundedCornerShape(OrbitSpacing.sm)
    Box(
        modifier = Modifier
            .background(colors.subtleFill, pillShape)
            .border(OrbitSpacing.hairline, colors.hairline, pillShape)
            .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs / 2)
    ) {
        Text(
            text = badge.label,
            style = typography.metaMedium,
            color = accent,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun SettledWorkTrajectory(
    turn: FfiStageTurn,
    describeStep: (FfiTimelineItem) -> FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    var expanded by rememberSaveable(turn.key) { mutableStateOf(false) }

    val durationSec = ((turn.turnDurationMs ?: turn.thoughtDurationMs) ?: 0L) / 1000L
    val baseSummary = turn.workSummaryLabel.ifBlank {
        if (durationSec > 0L) "Thought for ${durationSec}s" else "Completed ${turn.foldedSteps.size} steps"
    }
    val summaryWithDuration = if (durationSec > 0L && !baseSummary.contains("${durationSec}s")) {
        "$baseSummary · ${durationSec}s"
    } else {
        baseSummary
    }

    Column(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = OrbitSpacing.xl + OrbitSpacing.md)
                .clip(RoundedCornerShape(OrbitSpacing.sm))
                .clickable {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    expanded = !expanded
                }
                .padding(vertical = OrbitSpacing.xs),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
        ) {
            Icon(
                imageVector = if (expanded) Icons.Outlined.ExpandMore else Icons.Outlined.ChevronRight,
                contentDescription = if (expanded) "Collapse steps" else "Expand steps",
                tint = colors.fgMuted,
                modifier = Modifier.size(OrbitSpacing.iconSm),
            )
            Text(
                text = summaryWithDuration,
                style = typography.subheadMedium,
                color = if (turn.hasUnrecoveredError) colors.dangerRemoved else colors.fgSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
            if (turn.workCounts.additions > 0u || turn.workCounts.deletions > 0u) {
                Spacer(modifier = Modifier.width(OrbitSpacing.xs))
                if (turn.workCounts.additions > 0u) {
                    Text(
                        text = "+${turn.workCounts.additions}",
                        style = typography.metaMedium,
                        color = colors.directAdded,
                    )
                }
                if (turn.workCounts.deletions > 0u) {
                    Text(
                        text = "-${turn.workCounts.deletions}",
                        style = typography.metaMedium,
                        color = colors.dangerRemoved,
                    )
                }
            }
        }

        AnimatedVisibility(
            visible = expanded,
            enter = expandVertically() + fadeIn(),
            exit = shrinkVertically() + fadeOut(),
        ) {
            HairlineStepRail(
                items = turn.foldedSteps,
                describeStep = describeStep,
                onOpenDiffFile = onOpenDiffFile,
                onQuoteDiffLine = onQuoteDiffLine,
            )
        }
    }
}

@Composable
private fun LiveTurnSegments(
    turn: FfiStageTurn,
    describeStep: (FfiTimelineItem) -> FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography

    if (turn.liveSegments.isEmpty()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(vertical = OrbitSpacing.xs),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            BreathingDot(color = colors.primary)
            Text(
                text = turn.workSummaryLiveLabel.ifBlank { "Thinking..." },
                style = typography.subheadMedium,
                color = colors.fgSecondary,
            )
        }
        return
    }

    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
    ) {
        turn.liveSegments.forEach { segment ->
            when (segment) {
                is FfiTurnLiveSegment.StepGroup -> {
                    var expanded by rememberSaveable(segment.key) { mutableStateOf(segment.hasRunningStep) }
                    val label = if (segment.hasRunningStep) {
                        segment.summary.liveLabel.ifBlank { segment.summary.label.ifBlank { "Working..." } }
                    } else {
                        segment.summary.label.ifBlank { "Completed steps" }
                    }
                    Column(modifier = Modifier.fillMaxWidth()) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(OrbitSpacing.sm))
                                .clickable { expanded = !expanded }
                                .padding(vertical = OrbitSpacing.xs),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                        ) {
                            if (segment.hasRunningStep) {
                                BreathingDot(color = colors.primary)
                            } else {
                                Icon(
                                    imageVector = if (expanded) Icons.Outlined.ExpandMore else Icons.Outlined.ChevronRight,
                                    contentDescription = null,
                                    tint = colors.fgMuted,
                                    modifier = Modifier.size(OrbitSpacing.iconSm),
                                )
                            }
                            Text(
                                text = label,
                                style = typography.subheadMedium,
                                color = colors.fgSecondary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        AnimatedVisibility(visible = expanded) {
                            HairlineStepRail(
                                items = segment.items,
                                describeStep = describeStep,
                                onOpenDiffFile = onOpenDiffFile,
                                onQuoteDiffLine = onQuoteDiffLine,
                            )
                        }
                    }
                }
                is FfiTurnLiveSegment.AssistantMessage -> {
                    (segment.item as? FfiTimelineItem.AssistantMessage)?.let { msg ->
                        MarkdownBlocks(
                            blocks = msg.blocks,
                            fallbackText = msg.text,
                            textColor = colors.fgPrimary,
                        )
                    }
                }
                is FfiTurnLiveSegment.Notice -> {
                    (segment.item as? FfiTimelineItem.Notice)?.let { NoticeBannerRow(it) }
                }
                is FfiTurnLiveSegment.Compaction -> {
                    CompactionDividerRow(label = segment.label)
                }
            }
        }
    }
}

@Composable
private fun HairlineStepRail(
    items: List<FfiTimelineItem>,
    describeStep: (FfiTimelineItem) -> FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
) {
    val colors = OrbitTheme.colors

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .padding(start = OrbitSpacing.sm, top = OrbitSpacing.xs, bottom = OrbitSpacing.xs),
    ) {
        Box(
            modifier = Modifier
                .width(1.dp)
                .fillMaxHeight()
                .background(colors.hairline)
        )
        Spacer(modifier = Modifier.width(OrbitSpacing.md))
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            items.forEach { item ->
                StepRow(
                    item = item,
                    descriptor = describeStep(item),
                    onOpenDiffFile = onOpenDiffFile,
                    onQuoteDiffLine = onQuoteDiffLine,
                )
            }
        }
    }
}

@Composable
private fun StepRow(
    item: FfiTimelineItem,
    descriptor: FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography

    when (item) {
        is FfiTimelineItem.AssistantThinking -> {
            var open by remember { mutableStateOf(false) }
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(OrbitSpacing.xs))
                    .clickable(enabled = item.text.isNotBlank()) { open = !open }
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    Text(
                        text = item.headline.ifBlank { "Thought" },
                        style = typography.subhead,
                        color = colors.fgMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    if (item.text.isNotBlank()) {
                        Icon(
                            imageVector = if (open) Icons.Outlined.ExpandMore else Icons.Outlined.ChevronRight,
                            contentDescription = null,
                            tint = colors.fgMuted,
                            modifier = Modifier.size(OrbitSpacing.iconSm),
                        )
                    }
                }
                AnimatedVisibility(visible = open && item.text.isNotBlank()) {
                    Text(
                        text = item.text.trim(),
                        style = typography.subhead,
                        color = colors.fgSecondary,
                        modifier = Modifier.padding(top = OrbitSpacing.xs),
                    )
                }
            }
        }
        is FfiTimelineItem.ToolCall -> {
            val haptics = LocalHapticFeedback.current
            var expanded by rememberSaveable(item.callId) { mutableStateOf(false) }

            val dotColor = when (item.status) {
                FfiToolStatus.RUNNING -> colors.primary
                FfiToolStatus.COMPLETED -> colors.directAdded
                FfiToolStatus.FAILED -> colors.dangerRemoved
                FfiToolStatus.DENIED -> colors.warning
            }

            Column(modifier = Modifier.fillMaxWidth()) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 36.dp)
                        .clip(RoundedCornerShape(OrbitSpacing.xs))
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            expanded = !expanded
                        }
                        .padding(vertical = OrbitSpacing.xs),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    if (item.status == FfiToolStatus.RUNNING) {
                        BreathingDot(color = dotColor)
                    } else {
                        Box(
                            modifier = Modifier
                                .size(6.dp)
                                .background(dotColor, CircleShape)
                        )
                    }

                    val verb = descriptor?.verb?.takeIf { it.isNotBlank() }
                    val target = descriptor?.target?.takeIf { it.isNotBlank() } ?: item.summaryTitle

                    if (verb != null) {
                        Text(
                            text = verb,
                            style = typography.subheadMedium,
                            color = colors.fgSecondary,
                        )
                    }
                    Text(
                        text = target,
                        style = typography.code,
                        color = colors.fgPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )

                    if (descriptor != null && (descriptor.additions > 0u || descriptor.deletions > 0u)) {
                        if (descriptor.additions > 0u) {
                            Text(
                                text = "+${descriptor.additions}",
                                style = typography.metaMedium,
                                color = colors.directAdded,
                            )
                        }
                        if (descriptor.deletions > 0u) {
                            Text(
                                text = "-${descriptor.deletions}",
                                style = typography.metaMedium,
                                color = colors.dangerRemoved,
                            )
                        }
                    }

                    descriptor?.exitCode?.takeIf { it != 0 }?.let { code ->
                        Text(
                            text = "exit $code",
                            style = typography.metaMedium,
                            color = colors.dangerRemoved,
                        )
                    }

                    Icon(
                        imageVector = if (expanded) Icons.Outlined.ExpandMore else Icons.Outlined.ChevronRight,
                        contentDescription = if (expanded) "Collapse step detail" else "Expand step detail",
                        tint = colors.fgMuted,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                }

                AnimatedVisibility(
                    visible = expanded,
                    enter = expandVertically() + fadeIn(),
                    exit = shrinkVertically() + fadeOut(),
                ) {
                    ToolStepDetailPanel(
                        tool = item,
                        descriptor = descriptor,
                        onOpenDiffFile = onOpenDiffFile,
                        onQuoteDiffLine = onQuoteDiffLine,
                    )
                }
            }
        }
        is FfiTimelineItem.AssistantMessage -> {
            MarkdownBlocks(
                blocks = item.blocks,
                fallbackText = item.text,
                textColor = colors.fgSecondary,
            )
        }
        is FfiTimelineItem.PermissionRequest -> {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Box(
                    modifier = Modifier
                        .size(6.dp)
                        .background(colors.warning, CircleShape)
                )
                Text(
                    text = "${item.title}: ${item.commandOrPath}",
                    style = typography.meta,
                    color = colors.fgSecondary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        is FfiTimelineItem.Notice -> NoticeBannerRow(item)
        else -> Unit
    }
}

private const val INLINE_STEP_DIFF_PREVIEW_LIMIT = 16
private const val INLINE_STEP_SNIPPET_LINE_LIMIT = 14

private fun buildToolStepSnippetAndCopyPayload(
    tool: FfiTimelineItem.ToolCall,
    descriptor: FfiToolStepDescriptor?,
): Pair<List<String>, String> {
    val lines = mutableListOf<String>()
    val copyText: String
    when (val det = tool.detail) {
        is FfiToolCallDetail.Shell -> {
            val cmdLine = "$ ${det.command.trim()}"
            lines += cmdLine
            det.cwd?.takeIf { it.isNotBlank() }?.let { lines += "cwd: $it" }
            val out = (det.outputPreview ?: tool.previewBody)?.trimEnd().orEmpty()
            if (out.isNotBlank()) {
                out.lines().forEach { lines += it }
            }
            copyText = if (out.isNotBlank()) "${det.command.trim()}\n$out" else det.command.trim()
        }
        is FfiToolCallDetail.FileRead -> {
            val rangeSuffix = when {
                det.startLine != null && det.endLine != null -> ":L${det.startLine}-L${det.endLine}"
                det.startLine != null -> ":L${det.startLine}+"
                else -> ""
            }
            val fullTarget = "${det.path}$rangeSuffix"
            lines += fullTarget
            val preview = tool.previewBody?.trimEnd().orEmpty()
            if (preview.isNotBlank()) {
                preview.lines().forEach { lines += it }
            }
            copyText = if (preview.isNotBlank()) "$fullTarget\n$preview" else det.path
        }
        is FfiToolCallDetail.Search -> {
            lines += "query: ${det.query}"
            det.scope?.takeIf { it.isNotBlank() }?.let { lines += "scope: $it" }
            val preview = tool.previewBody?.trimEnd().orEmpty()
            if (preview.isNotBlank()) {
                preview.lines().forEach { lines += it }
            }
            copyText = if (preview.isNotBlank()) "${det.query}\n$preview" else det.query
        }
        is FfiToolCallDetail.FileEdit -> {
            lines += det.path
            val patch = (det.unifiedDiff ?: tool.previewBody)?.trimEnd().orEmpty()
            if (patch.isNotBlank()) {
                patch.lines().forEach { lines += it }
            }
            copyText = if (patch.isNotBlank()) patch else det.path
        }
        is FfiToolCallDetail.Subagent -> {
            lines += "role: ${det.role}"
            det.promptPreview.trimEnd().takeIf { it.isNotBlank() }?.lines()?.forEach { lines += it }
            tool.previewBody?.trimEnd()?.takeIf { it.isNotBlank() }?.lines()?.forEach { lines += it }
            copyText = lines.joinToString("\n")
        }
        is FfiToolCallDetail.Generic -> {
            lines += det.toolName.ifBlank { tool.summaryTitle }
            det.argumentsJson.trim().takeIf { it.isNotBlank() && it != "{}" }?.lines()?.forEach { lines += it }
            tool.previewBody?.trimEnd()?.takeIf { it.isNotBlank() }?.lines()?.forEach { lines += it }
            copyText = lines.joinToString("\n")
        }
    }

    if (lines.isEmpty()) {
        val fallback = descriptor?.openPath?.takeIf { it.isNotBlank() }
            ?: descriptor?.target?.takeIf { it.isNotBlank() }
            ?: tool.summaryTitle
        lines += fallback
        return lines to fallback
    }

    return lines to copyText
}

@Composable
private fun ToolStepDetailPanel(
    tool: FfiTimelineItem.ToolCall,
    descriptor: FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val context = LocalContext.current
    val haptics = LocalHapticFeedback.current
    val shape = RoundedCornerShape(OrbitSpacing.sm)

    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) {
        if (copied) {
            delay(1500)
            copied = false
        }
    }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = OrbitSpacing.xs)
            .clip(shape)
            .background(colors.subtleFill, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape)
            .padding(OrbitSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
    ) {
        if (tool.diffFiles.isNotEmpty()) {
            tool.diffFiles.forEach { df ->
                InlineDiffFileBlock(
                    file = df,
                    onInspectFullDiff = {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onOpenDiffFile(df.path)
                    },
                    onQuoteLine = { lineLabel, content ->
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        onQuoteDiffLine(df.path, lineLabel, content)
                    },
                )
            }
        } else {
            val (allLines, copyPayload) = remember(tool, descriptor) {
                buildToolStepSnippetAndCopyPayload(tool, descriptor)
            }
            val displayedLines = remember(allLines) {
                allLines.take(INLINE_STEP_SNIPPET_LINE_LIMIT)
            }
            val omittedCount = (allLines.size - displayedLines.size).coerceAtLeast(0)

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = descriptor?.secondaryMeta?.takeIf { it.isNotBlank() }
                        ?: when (tool.detail) {
                            is FfiToolCallDetail.Shell -> "Shell command"
                            is FfiToolCallDetail.FileRead -> "File path"
                            is FfiToolCallDetail.Search -> "Search query"
                            is FfiToolCallDetail.FileEdit -> "Edited file"
                            is FfiToolCallDetail.Subagent -> "Subagent"
                            is FfiToolCallDetail.Generic -> "Tool detail"
                        },
                    style = typography.metaMedium,
                    color = colors.fgMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )

                val copyBtnShape = RoundedCornerShape(OrbitSpacing.xs)
                Row(
                    modifier = Modifier
                        .clip(copyBtnShape)
                        .clickable {
                            val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                            cm?.setPrimaryClip(ClipData.newPlainText("step_snippet", copyPayload))
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            copied = true
                        }
                        .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    Icon(
                        imageVector = if (copied) Icons.Outlined.Check else Icons.Outlined.ContentCopy,
                        contentDescription = "Copy step snippet",
                        tint = if (copied) colors.directAdded else colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                    Text(
                        text = if (copied) "Copied" else "Copy",
                        style = typography.metaMedium,
                        color = if (copied) colors.directAdded else colors.fgSecondary,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
            }

            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs / 2),
            ) {
                displayedLines.forEachIndexed { idx, ln ->
                    Text(
                        text = ln,
                        style = if (idx == 0) typography.codeMedium else typography.code,
                        color = if (idx == 0) colors.fgPrimary else colors.fgSecondary,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
            }

            if (omittedCount > 0) {
                Text(
                    text = "… $omittedCount more ${if (omittedCount == 1) "line" else "lines"}",
                    style = typography.meta,
                    color = colors.fgMuted,
                )
            }

            (tool.detail as? FfiToolCallDetail.FileEdit)?.path?.takeIf { it.isNotBlank() }?.let { editPath ->
                val inspectShape = RoundedCornerShape(OrbitSpacing.sm)
                Box(
                    modifier = Modifier
                        .heightIn(min = 36.dp)
                        .clip(inspectShape)
                        .background(colors.elevated, inspectShape)
                        .border(OrbitSpacing.hairline, colors.primary.copy(alpha = 0.55f), inspectShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onOpenDiffFile(editPath)
                        }
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "Inspect Full Diff",
                        style = typography.metaSemiBold,
                        color = colors.primary,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
            }
        }
    }
}

@Composable
private fun InlineDiffFileBlock(
    file: FfiDiffFile,
    onInspectFullDiff: () -> Unit,
    onQuoteLine: (lineLabel: String, content: String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val previewLines = remember(file.lines) { file.lines.take(INLINE_STEP_DIFF_PREVIEW_LIMIT) }
    val hiddenLines = (file.lines.size - previewLines.size).coerceAtLeast(0)

    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = file.path,
                style = typography.codeMedium,
                color = colors.fgPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs)) {
                if (file.additions > 0u) {
                    Text("+${file.additions}", style = typography.metaMedium, color = colors.directAdded)
                }
                if (file.deletions > 0u) {
                    Text("-${file.deletions}", style = typography.metaMedium, color = colors.dangerRemoved)
                }
            }
        }
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(OrbitSpacing.xs))
                .background(colors.code, RoundedCornerShape(OrbitSpacing.xs))
                .border(OrbitSpacing.hairline, colors.hairline, RoundedCornerShape(OrbitSpacing.xs))
                .horizontalScroll(rememberScrollState()),
        ) {
            previewLines.forEach { ln ->
                val bg = when (ln.kind) {
                    FfiDiffLineKind.ADDED -> colors.diffAddedBg
                    FfiDiffLineKind.REMOVED -> colors.diffRemovedBg
                    FfiDiffLineKind.HUNK_HEADER -> colors.diffHunkBg
                    else -> Color.Transparent
                }
                val fg = when (ln.kind) {
                    FfiDiffLineKind.ADDED -> colors.directAdded
                    FfiDiffLineKind.REMOVED -> colors.dangerRemoved
                    FfiDiffLineKind.HUNK_HEADER -> colors.primary
                    FfiDiffLineKind.META -> colors.fgMuted
                    FfiDiffLineKind.CONTEXT -> colors.fgSecondary
                }
                val lineNum = ln.newLine ?: ln.oldLine
                val gutterPrefix = when (ln.kind) {
                    FfiDiffLineKind.ADDED -> "+ "
                    FfiDiffLineKind.REMOVED -> "- "
                    FfiDiffLineKind.CONTEXT -> "  "
                    FfiDiffLineKind.HUNK_HEADER, FfiDiffLineKind.META -> ""
                }
                Text(
                    text = "$gutterPrefix${ln.content}",
                    style = typography.code,
                    color = fg,
                    maxLines = 1,
                    softWrap = false,
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(bg)
                        .clickable(enabled = ln.kind != FfiDiffLineKind.META) {
                            val label = lineNum?.let { "L$it" } ?: "hunk"
                            onQuoteLine(label, ln.content)
                        }
                        .padding(horizontal = OrbitSpacing.xs, vertical = OrbitSpacing.xs / 4),
                )
            }
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (hiddenLines > 0) {
                Text(
                    text = "… $hiddenLines more ${if (hiddenLines == 1) "line" else "lines"}",
                    style = typography.meta,
                    color = colors.fgMuted,
                )
            } else {
                Spacer(modifier = Modifier.width(OrbitSpacing.xs))
            }

            val inspectShape = RoundedCornerShape(OrbitSpacing.sm)
            Box(
                modifier = Modifier
                    .heightIn(min = 36.dp)
                    .clip(inspectShape)
                    .background(colors.elevated, inspectShape)
                    .border(OrbitSpacing.hairline, colors.primary.copy(alpha = 0.55f), inspectShape)
                    .clickable { onInspectFullDiff() }
                    .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = "Inspect Full Diff",
                    style = typography.metaSemiBold,
                    color = colors.primary,
                    maxLines = 1,
                    softWrap = false,
                )
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun TurnFileDiffStrip(
    diffs: List<FfiTurnFileDiffSummary>,
    onOpenDiffFile: (String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography

    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
        modifier = Modifier.padding(top = OrbitSpacing.xs),
    ) {
        diffs.forEach { d ->
            val shape = RoundedCornerShape(OrbitSpacing.sm)
            Row(
                modifier = Modifier
                    .clip(shape)
                    .background(colors.subtleFill, shape)
                    .border(OrbitSpacing.hairline, colors.hairline, shape)
                    .clickable { onOpenDiffFile(d.path) }
                    .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                Text(
                    text = OrbitFormatters.shortPath(d.path),
                    style = typography.code,
                    color = colors.fgPrimary,
                )
                if (d.additions > 0u) {
                    Text("+${d.additions}", style = typography.metaMedium, color = colors.directAdded)
                }
                if (d.deletions > 0u) {
                    Text("-${d.deletions}", style = typography.metaMedium, color = colors.dangerRemoved)
                }
            }
        }
    }
}

@Composable
private fun NoticeBannerRow(notice: FfiTimelineItem.Notice) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val isError = notice.level.equals("error", ignoreCase = true)
    val isWarn = notice.level.equals("warn", ignoreCase = true) || notice.level.equals("warning", ignoreCase = true)
    val fg = when {
        isError -> colors.dangerRemoved
        isWarn -> colors.warning
        else -> colors.fgSecondary
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(colors.subtleFill, RoundedCornerShape(OrbitSpacing.sm))
            .border(OrbitSpacing.hairline, colors.hairline, RoundedCornerShape(OrbitSpacing.sm))
            .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = notice.message,
            style = typography.subhead,
            color = fg,
        )
    }
}

@Composable
private fun CompactionDividerRow(label: String) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = OrbitSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
    ) {
        HorizontalDivider(modifier = Modifier.weight(1f), thickness = OrbitSpacing.hairline, color = colors.hairline)
        Text(
            text = label,
            style = typography.meta,
            color = colors.fgMuted,
        )
        HorizontalDivider(modifier = Modifier.weight(1f), thickness = OrbitSpacing.hairline, color = colors.hairline)
    }
}

@Composable
fun BreathingDot(
    color: Color,
    modifier: Modifier = Modifier,
) {
    val transition = rememberInfiniteTransition(label = "orbit_breathing")
    val alpha by transition.animateFloat(
        initialValue = 0.35f,
        targetValue = 1.0f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 850),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "dot_alpha",
    )
    Box(
        modifier = modifier
            .size(6.dp)
            .alpha(alpha)
            .background(color, CircleShape)
    )
}
