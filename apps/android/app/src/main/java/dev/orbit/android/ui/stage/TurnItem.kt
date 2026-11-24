package dev.orbit.android.ui.stage

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.SystemClock
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
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
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AutoAwesome
import androidx.compose.material.icons.outlined.Build
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material.icons.outlined.ContentCut
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Difference
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.ErrorOutline
import androidx.compose.material.icons.outlined.FormatQuote
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.Language
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitCopyIcon
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import dev.orbit.android.ui.theme.orbitSemanticFileIcon
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
import uniffi.orbit_ffi_uniffi.FfiToolStepKind
import uniffi.orbit_ffi_uniffi.FfiTurnFileDiffSummary
import uniffi.orbit_ffi_uniffi.FfiTurnLiveSegment

@Composable
fun TurnItem(
    turn: FfiStageTurn,
    isTurnActive: Boolean,
    activeCwd: String,
    describeStep: suspend (FfiTimelineItem) -> FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
    onCopyAssistantReply: (String) -> Unit,
    onQuoteAssistantReply: (String) -> Unit = {},
    onInterruptDeliver: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val active = isTurnActive
    val userPrompt = turn.userPrompt as? FfiTimelineItem.UserPrompt
    val isCompactionOnly = turn.isCompactionCommand ||
        (userPrompt != null && isBareCompactionPrompt(userPrompt))

    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
    ) {
        (turn.agentMessage as? FfiTimelineItem.AgentMessage)?.let {
            AgentMessageBody(it, onInterruptDeliver = onInterruptDeliver)
        }
        // 1. User Prompt Bubble (or compact-only divider suppression)
        if (isCompactionOnly) {
            CompactionDividerRow(label = "Context compacted")
        } else if (userPrompt != null) {
            UserPromptBubble(prompt = userPrompt)
        }

        // 2. Live Running Trajectory OR Settled Collapsible Trajectory
        if (active || (turn.liveSegments.isNotEmpty() && turn.foldedSteps.isEmpty())) {
            LiveTurnSegments(
                turn = turn,
                isTurnActive = active,
                activeCwd = activeCwd,
                describeStep = describeStep,
                onOpenDiffFile = onOpenDiffFile,
                onQuoteDiffLine = onQuoteDiffLine,
                onInterruptDeliver = onInterruptDeliver,
            )
        } else {
            if (turn.foldedSteps.isNotEmpty()) {
                SettledWorkTrajectory(
                    turn = turn,
                    activeCwd = activeCwd,
                    describeStep = describeStep,
                    onOpenDiffFile = onOpenDiffFile,
                    onQuoteDiffLine = onQuoteDiffLine,
                    onInterruptDeliver = onInterruptDeliver,
                )
            }

            turn.notices.filterIsInstance<FfiTimelineItem.Notice>().forEach { notice ->
                NoticeBannerRow(notice = notice)
            }

            (turn.terminalReply as? FfiTimelineItem.AssistantMessage)?.let { reply ->
                MarkdownBlocks(
                    blocks = reply.blocks,
                    fallbackText = reply.text,
                )
            }
        }

        // 3. Single TurnChangesBar (per-turn changed file summary)
        if (!active && turn.turnDiffs.isNotEmpty()) {
            TurnChangesBar(
                diffs = turn.turnDiffs,
                activeCwd = activeCwd,
                onOpenDiffFile = onOpenDiffFile,
            )
        }

        // 4. Restrained TurnFooter (Hidden while active; icon-only Copy + relative time when settled)
        val terminalReply = (turn.terminalReply ?: turn.lastAssistantReply) as? FfiTimelineItem.AssistantMessage
        if (!active && !isCompactionOnly && (terminalReply != null || turn.foldedSteps.isNotEmpty() || turn.completedAtMs != null)) {
            AssistantTurnFooter(
                turn = turn,
                replyText = terminalReply?.let { extractAssistantReplyText(it) },
                onCopy = onCopyAssistantReply,
            )
        }
    }
}

private fun isBareCompactionPrompt(prompt: FfiTimelineItem.UserPrompt): Boolean {
    val cleaned = cleanLegacyPromptText(prompt.text).first
    val hasCompactBadge = prompt.badges.any { it.kind == "slash" && it.label.equals("/compact", ignoreCase = true) }
    if (hasCompactBadge && cleaned.isEmpty()) return true
    return prompt.text.trim().equals("/compact", ignoreCase = true)
}

private val LEGACY_WORKFLOW_BLOCK_REGEX = Regex(
    """<WORKFLOW_INSTRUCTION[^>]*>[\s\S]*?</WORKFLOW_INSTRUCTION>\s*""",
    RegexOption.IGNORE_CASE,
)
private val LEADING_SLASH_REGEX = Regex("""^/(plan|review|compact)(?:\s+([\s\S]*))?$""", RegexOption.IGNORE_CASE)

private fun cleanLegacyPromptText(rawText: String): Pair<String, String?> {
    var working = rawText.replace(LEGACY_WORKFLOW_BLOCK_REGEX, "").trim()

    val markers = listOf(
        "Attached image file:",
        "Referenced workspace files:",
        "Working Tree Diff:",
        "Staged Diff:",
        "Active terminal output:",
        "Reference `",
        "Quoted diff lines:",
        "Quoted assistant context:",
    )
    if (markers.any { working.startsWith(it) } && working.contains("\n\n")) {
        // Legacy serialized wrappers placed metadata blocks before the final user message
        val userTagStart = working.indexOf("<USER_REQUEST>")
        working = if (userTagStart >= 0) {
            working.substring(userTagStart)
                .removePrefix("<USER_REQUEST>")
                .removeSuffix("</USER_REQUEST>")
                .trim()
        } else {
            val segments = working.split(Regex("""\n\n(?![`>-])"""))
            segments.lastOrNull { seg ->
                val s = seg.trim()
                s.isNotEmpty() && markers.none { s.startsWith(it) } && !s.startsWith("```")
            }?.trim().orEmpty()
        }
    }

    val match = LEADING_SLASH_REGEX.matchEntire(working)
    return if (match != null) {
        val slash = "/${match.groupValues[1].lowercase()}"
        val body = match.groupValues[2].trim()
        body to slash
    } else {
        working to null
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
        HorizontalDivider(
            modifier = Modifier.weight(1f),
            thickness = OrbitSpacing.hairline,
            color = colors.hairline,
        )
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(5.dp),
        ) {
            Icon(
                imageVector = Icons.Outlined.ContentCut,
                contentDescription = null,
                tint = colors.fgMuted,
                modifier = Modifier.size(13.dp),
            )
            Text(
                text = label,
                style = typography.meta,
                color = colors.fgMuted,
            )
        }
        HorizontalDivider(
            modifier = Modifier.weight(1f),
            thickness = OrbitSpacing.hairline,
            color = colors.hairline,
        )
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun UserPromptBubble(prompt: FfiTimelineItem.UserPrompt) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography

    val (cleanBody, inferredSlash) = remember(prompt.text) { cleanLegacyPromptText(prompt.text) }
    val slashLabel = remember(prompt.badges, inferredSlash) {
        prompt.badges.firstOrNull { it.kind == "slash" }?.label ?: inferredSlash
    }
    val contextBadges = remember(prompt.badges) {
        prompt.badges.filterNot { it.kind == "slash" }
    }

    var expanded by rememberSaveable(prompt.messageId) { mutableStateOf(false) }
    var exceedsMaxLines by remember(prompt.messageId, cleanBody) {
        mutableStateOf(cleanBody.lines().size > 8)
    }

    val bubbleShape = RoundedCornerShape(20.dp)
    val hairlineColor = if (colors.isDark) {
        Color.White.copy(alpha = 0.09f)
    } else {
        Color.Black.copy(alpha = 0.075f)
    }

    Box(
        modifier = Modifier.fillMaxWidth(),
        contentAlignment = Alignment.CenterEnd,
    ) {
        Surface(
            shape = bubbleShape,
            color = colors.elevated,
            tonalElevation = 0.dp,
            shadowElevation = 1.dp,
            modifier = Modifier
                .widthIn(max = 320.dp)
                .border(OrbitSpacing.hairline, hairlineColor, bubbleShape),
        ) {
            Column(
                modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                if (contextBadges.isNotEmpty()) {
                    FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    ) {
                        contextBadges.forEach { badge ->
                            PromptBadgePill(badge = badge)
                        }
                    }
                }

                if (cleanBody.isNotEmpty() || slashLabel != null) {
                    Box {
                        SelectionContainer {
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                                verticalAlignment = Alignment.Top,
                            ) {
                                if (slashLabel != null) {
                                    Text(
                                        text = slashLabel,
                                        style = typography.codeMedium,
                                        color = colors.fgPrimary,
                                        modifier = Modifier
                                            .padding(top = 1.dp)
                                            .background(colors.subtleFill, RoundedCornerShape(6.dp))
                                            .border(OrbitSpacing.hairline, colors.hairline, RoundedCornerShape(6.dp))
                                            .padding(horizontal = 6.dp, vertical = 1.dp),
                                    )
                                }
                                if (cleanBody.isNotEmpty()) {
                                    Text(
                                        text = cleanBody,
                                        style = typography.body,
                                        color = colors.fgPrimary,
                                        maxLines = if (expanded) Int.MAX_VALUE else 8,
                                        overflow = TextOverflow.Clip,
                                        onTextLayout = { result ->
                                            if (!expanded && (result.hasVisualOverflow || result.lineCount > 8)) {
                                                exceedsMaxLines = true
                                            }
                                        },
                                    )
                                }
                            }
                        }

                        if (exceedsMaxLines && !expanded) {
                            Box(
                                modifier = Modifier
                                    .align(Alignment.BottomCenter)
                                    .fillMaxWidth()
                                    .height(22.dp)
                                    .background(
                                        Brush.verticalGradient(
                                            listOf(Color.Transparent, colors.elevated)
                                        )
                                    )
                            )
                        }
                    }
                }

                if (exceedsMaxLines) {
                    Text(
                        text = if (expanded) "Show less" else "Show more",
                        style = typography.metaMedium,
                        color = colors.fgSecondary,
                        modifier = Modifier
                            .clip(RoundedCornerShape(6.dp))
                            .clickable { expanded = !expanded }
                            .padding(vertical = 2.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun PromptBadgePill(badge: FfiPromptBadge) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val icon: ImageVector = when (badge.kind) {
        "image" -> Icons.Outlined.Image
        "git_diff", "git" -> Icons.Outlined.Difference
        "terminal" -> Icons.Outlined.Terminal
        "quote" -> Icons.Outlined.FormatQuote
        "file" -> orbitSemanticFileIcon(badge.label.removePrefix("@"))
        else -> Icons.Outlined.Description
    }
    val shape = RoundedCornerShape(7.dp)
    Row(
        modifier = Modifier
            .background(colors.subtleFill, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape)
            .padding(horizontal = 6.dp, vertical = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            tint = colors.fgSecondary,
            modifier = Modifier.size(12.dp),
        )
        Text(
            text = badge.label,
            style = typography.metaMedium,
            color = colors.fgSecondary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun AssistantTurnFooter(
    turn: FfiStageTurn,
    replyText: String?,
    onCopy: (String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    var copied by remember(turn.key) { mutableStateOf(false) }

    LaunchedEffect(copied) {
        if (copied) {
            delay(1500L)
            copied = false
        }
    }

    val timestampSecs = ((turn.completedAtMs ?: turn.startedAtMs ?: 0L) / 1000L)
    val relTime = remember(timestampSecs) {
        if (timestampSecs > 0L) OrbitFormatters.relativeTimeLabel(timestampSecs) else ""
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (!replyText.isNullOrBlank()) {
            Box(
                modifier = Modifier
                    .size(28.dp)
                    .clip(CircleShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onCopy(replyText)
                        copied = true
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = if (copied) Icons.Outlined.Check else OrbitCopyIcon,
                    contentDescription = "Copy response",
                    tint = if (copied) colors.directAdded else colors.fgMuted,
                    modifier = Modifier.size(14.dp),
                )
            }
        }
        if (relTime.isNotEmpty()) {
            Text(
                text = relTime,
                style = typography.meta,
                color = colors.fgMuted,
            )
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
                is FfiRichBlock.MathBlock -> {
                    appendLine("$$")
                    appendLine(block.tex)
                    appendLine("$$")
                }
                FfiRichBlock.HorizontalRule -> appendLine("---")
            }
        }
    }.trim()
}

private fun stepStableKey(item: FfiTimelineItem): String = when (item) {
    is FfiTimelineItem.UserPrompt -> "user:${item.messageId}"
    is FfiTimelineItem.AssistantThinking -> "think:${item.itemId}:${item.seq}"
    is FfiTimelineItem.AssistantMessage -> "msg:${item.itemId}:${item.seq}"
    is FfiTimelineItem.ToolCall -> "tool:${item.callId}:${item.seq}"
    is FfiTimelineItem.PermissionRequest -> "perm:${item.requestId}"
    is FfiTimelineItem.QuestionRequest -> "q:${item.requestId}"
    is FfiTimelineItem.Notice -> "notice:${item.seq}"
    is FfiTimelineItem.AgentMessage -> "agent-msg:${item.messageId}:${item.seq}"
}

@Composable
private fun SettledWorkTrajectory(
    turn: FfiStageTurn,
    activeCwd: String,
    describeStep: suspend (FfiTimelineItem) -> FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
    onInterruptDeliver: (() -> Unit)? = null,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    var expanded by rememberSaveable(turn.key) { mutableStateOf(false) }

    val chevronRotation by animateFloatAsState(
        targetValue = if (expanded) 90f else 0f,
        animationSpec = tween(durationMillis = 180),
        label = "settled_chevron",
    )

    val summaryText = buildString {
        append(turn.workSummaryLabel.ifBlank { "${turn.foldedSteps.size} steps" })
        turn.thoughtDurationMs?.takeIf { it >= 1000L }?.let { ms ->
            append(" · Thought ${ms / 1000L}s")
        }
    }

    Column(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier
                .clip(RoundedCornerShape(8.dp))
                .clickable {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    expanded = !expanded
                }
                .padding(vertical = 4.dp, horizontal = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
        ) {
            Icon(
                imageVector = Icons.Outlined.ChevronRight,
                contentDescription = if (expanded) "Collapse steps" else "Expand steps",
                tint = if (turn.workSummaryIncludesFailure) colors.dangerRemoved else colors.fgMuted,
                modifier = Modifier
                    .size(OrbitSpacing.iconSm)
                    .rotate(chevronRotation),
            )
            Text(
                text = summaryText,
                style = typography.subheadMedium,
                color = if (turn.workSummaryIncludesFailure) colors.dangerRemoved else colors.fgSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }

        AnimatedVisibility(
            visible = expanded,
            enter = expandVertically(expandFrom = Alignment.Top) + fadeIn(tween(150)),
            exit = shrinkVertically(shrinkTowards = Alignment.Top) + fadeOut(tween(120)),
        ) {
            HairlineStepRail(
                steps = turn.foldedSteps,
                activeCwd = activeCwd,
                describeStep = describeStep,
                onOpenDiffFile = onOpenDiffFile,
                onQuoteDiffLine = onQuoteDiffLine,
                onInterruptDeliver = onInterruptDeliver,
                modifier = Modifier.padding(top = OrbitSpacing.xs),
            )
        }
    }
}

private fun isHiddenWakeupLabel(label: String): Boolean {
    val lower = label.trim().lowercase()
    return lower.isEmpty()
        || lower.startsWith("waiting for ")
        || lower.startsWith("resumed from background task")
}

@Composable
private fun LiveTurnSegments(
    turn: FfiStageTurn,
    isTurnActive: Boolean,
    activeCwd: String,
    describeStep: suspend (FfiTimelineItem) -> FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
    onInterruptDeliver: (() -> Unit)? = null,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val visibleSegments = remember(turn.liveSegments) {
        buildList {
            for (seg in turn.liveSegments) {
                if (seg is FfiTurnLiveSegment.Wakeup) {
                    val trimmed = seg.label.trim()
                    if (isHiddenWakeupLabel(trimmed)) continue
                    val prev = lastOrNull() as? FfiTurnLiveSegment.Wakeup
                    if (prev != null && prev.label.trim().equals(trimmed, ignoreCase = true)) continue
                }
                add(seg)
            }
        }
    }

    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
    ) {
        visibleSegments.forEachIndexed { segIdx, seg ->
            when (seg) {
                is FfiTurnLiveSegment.StepGroup -> {
                    val isLastVisible = segIdx == visibleSegments.lastIndex
                    val isTrailingFailed = !seg.hasRunningStep && seg.summary.counts.unrecoveredFailureCount > 0u
                    val isSegLiveActive = isTurnActive && !seg.hasFollowingContent && (seg.hasRunningStep || (isLastVisible && !isTrailingFailed))
                    val rollUpThreshold = 5
                    val shouldRollUp = isSegLiveActive && seg.items.size >= rollUpThreshold
                    var groupExpanded by rememberSaveable("${turn.key}:${seg.key}:${isSegLiveActive}") {
                        mutableStateOf(isSegLiveActive && !shouldRollUp)
                    }
                    val chevronRotation by animateFloatAsState(
                        targetValue = if (groupExpanded) 90f else 0f,
                        animationSpec = tween(durationMillis = 180),
                        label = "live_chevron",
                    )

                    Column(modifier = Modifier.fillMaxWidth()) {
                        Row(
                            modifier = Modifier
                                .clip(RoundedCornerShape(8.dp))
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    groupExpanded = !groupExpanded
                                }
                                .padding(vertical = 4.dp, horizontal = 2.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                        ) {
                            if (isSegLiveActive) {
                                BreathingDot(color = colors.primary)
                            } else {
                                Icon(
                                    imageVector = Icons.Outlined.ChevronRight,
                                    contentDescription = null,
                                    tint = colors.fgMuted,
                                    modifier = Modifier
                                        .size(OrbitSpacing.iconSm)
                                        .rotate(chevronRotation),
                                )
                            }
                            Text(
                                text = if (isSegLiveActive) {
                                    seg.summary.liveLabel.ifBlank { seg.summary.label }
                                } else {
                                    seg.summary.label.ifBlank { "Completed ${seg.items.size} ${if (seg.items.size == 1) "step" else "steps"}" }
                                },
                                style = typography.subheadMedium,
                                color = if (seg.summary.labelIncludesFailure) colors.dangerRemoved else colors.fgSecondary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }

                        AnimatedVisibility(
                            visible = groupExpanded,
                            enter = expandVertically(expandFrom = Alignment.Top) + fadeIn(tween(150)),
                            exit = shrinkVertically(shrinkTowards = Alignment.Top) + fadeOut(tween(120)),
                        ) {
                            val displayedItems = if (!groupExpanded && shouldRollUp) {
                                seg.items.takeLast(2)
                            } else {
                                seg.items
                            }
                            HairlineStepRail(
                                steps = displayedItems,
                                activeCwd = activeCwd,
                                describeStep = describeStep,
                                onOpenDiffFile = onOpenDiffFile,
                                onQuoteDiffLine = onQuoteDiffLine,
                                onInterruptDeliver = onInterruptDeliver,
                                modifier = Modifier.padding(top = OrbitSpacing.xs),
                            )
                        }

                        // When rolled up on >= 5 steps and collapsed while live, keep newest step visible below header
                        if (shouldRollUp && !groupExpanded && seg.items.isNotEmpty()) {
                            HairlineStepRail(
                                steps = listOf(seg.items.last()),
                                activeCwd = activeCwd,
                                describeStep = describeStep,
                                onOpenDiffFile = onOpenDiffFile,
                                onQuoteDiffLine = onQuoteDiffLine,
                                onInterruptDeliver = onInterruptDeliver,
                                modifier = Modifier.padding(top = OrbitSpacing.xs),
                            )
                        }
                    }
                }

                is FfiTurnLiveSegment.AssistantMessage -> {
                    (seg.item as? FfiTimelineItem.AssistantMessage)?.let { msg ->
                        MarkdownBlocks(
                            blocks = msg.blocks,
                            fallbackText = msg.text,
                        )
                    }
                }

                is FfiTurnLiveSegment.AgentMessage -> {
                    (seg.item as? FfiTimelineItem.AgentMessage)?.let { msg ->
                        AgentMessageBody(msg, onInterruptDeliver = onInterruptDeliver)
                    }
                }

                is FfiTurnLiveSegment.Notice -> {
                    (seg.item as? FfiTimelineItem.Notice)?.let { notice ->
                        NoticeBannerRow(notice = notice)
                    }
                }

                is FfiTurnLiveSegment.Compaction -> {
                    CompactionDividerRow(label = seg.label.ifBlank { "Context compacted" })
                }

                is FfiTurnLiveSegment.Wakeup -> {
                    CompactionDividerRow(label = seg.label.ifBlank { "Resumed" })
                }
            }
        }
    }
}

@Composable
private fun HairlineStepRail(
    steps: List<FfiTimelineItem>,
    activeCwd: String,
    describeStep: suspend (FfiTimelineItem) -> FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
    onInterruptDeliver: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    Row(
        modifier = modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min),
    ) {
        Spacer(modifier = Modifier.width(6.dp))
        Box(
            modifier = Modifier
                .width(1.dp)
                .fillMaxHeight()
                .background(colors.hairline)
        )
        Spacer(modifier = Modifier.width(OrbitSpacing.md))
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
        ) {
            steps.forEach { step ->
                val stableKey = stepStableKey(step)
                androidx.compose.runtime.key(stableKey) {
                    StepRow(
                        item = step,
                        stableKey = stableKey,
                        activeCwd = activeCwd,
                        describeStep = describeStep,
                        onOpenDiffFile = onOpenDiffFile,
                        onQuoteDiffLine = onQuoteDiffLine,
                        onInterruptDeliver = onInterruptDeliver,
                    )
                }
            }
        }
    }
}

private fun semanticStepIcon(
    kind: FfiToolStepKind?,
    target: String,
): ImageVector = when (kind) {
    FfiToolStepKind.FILE_READ -> orbitSemanticFileIcon(OrbitFormatters.shortPath(target))
    FfiToolStepKind.FILE_CREATE,
    FfiToolStepKind.FILE_EDIT -> Icons.Outlined.Edit
    FfiToolStepKind.SHELL -> Icons.Outlined.Terminal
    FfiToolStepKind.SEARCH -> {
        val lower = target.lowercase()
        if (lower.startsWith("http://") || lower.startsWith("https://") || lower.contains("web")) {
            Icons.Outlined.Language
        } else {
            Icons.Outlined.Search
        }
    }
    FfiToolStepKind.SUBAGENT -> Icons.Outlined.Hub
    FfiToolStepKind.GENERIC, null -> Icons.Outlined.Build
}

@Composable
private fun StepRow(
    item: FfiTimelineItem,
    stableKey: String,
    activeCwd: String,
    describeStep: suspend (FfiTimelineItem) -> FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
    onInterruptDeliver: (() -> Unit)? = null,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current

    when (item) {
        is FfiTimelineItem.AssistantThinking -> {
            var expanded by rememberSaveable(stableKey) { mutableStateOf(false) }
            val hasBody = item.text.isNotBlank()
            Column(modifier = Modifier.fillMaxWidth()) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(6.dp))
                        .clickable(enabled = hasBody) {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            expanded = !expanded
                        }
                        .padding(vertical = 3.dp),
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        imageVector = Icons.Outlined.AutoAwesome,
                        contentDescription = null,
                        tint = colors.fgMuted,
                        modifier = Modifier.size(14.dp),
                    )
                    Text(
                        text = item.headline.ifBlank { "Thought" },
                        style = typography.metaMedium,
                        color = colors.fgSecondary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    if (hasBody) {
                        val rot by animateFloatAsState(
                            targetValue = if (expanded) 90f else 0f,
                            animationSpec = tween(160),
                            label = "think_chevron",
                        )
                        Icon(
                            imageVector = Icons.Outlined.ChevronRight,
                            contentDescription = null,
                            tint = colors.fgMuted,
                            modifier = Modifier
                                .size(14.dp)
                                .rotate(rot),
                        )
                    }
                }

                if (hasBody) {
                    AnimatedVisibility(
                        visible = expanded,
                        enter = expandVertically(expandFrom = Alignment.Top) + fadeIn(tween(140)),
                        exit = shrinkVertically(shrinkTowards = Alignment.Top) + fadeOut(tween(110)),
                    ) {
                        SelectionContainer {
                            Text(
                                text = item.text.trim(),
                                style = typography.meta,
                                color = colors.fgSecondary,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(start = OrbitSpacing.lg, top = 2.dp, bottom = OrbitSpacing.xs),
                            )
                        }
                    }
                }
            }
        }

        is FfiTimelineItem.ToolCall -> {
            val descriptorState by produceState<FfiToolStepDescriptor?>(
                initialValue = item.stepDescriptor,
                key1 = item,
                key2 = activeCwd,
            ) {
                if (value == null) {
                    value = describeStep(item)
                }
            }
            val descriptor = descriptorState
            val stepKind = descriptor?.kind
            val verb = descriptor?.verb?.takeIf { it.isNotBlank() } ?: "Run"
            val targetRaw = descriptor?.target?.takeIf { it.isNotBlank() } ?: item.summaryTitle
            val targetDisplay = remember(targetRaw, activeCwd) {
                OrbitFormatters.stripWorkspacePrefix(targetRaw, activeCwd)
            }
            val failed = item.status == FfiToolStatus.FAILED || item.status == FfiToolStatus.DENIED
            val running = item.status == FfiToolStatus.RUNNING

            var expanded by rememberSaveable(stableKey) { mutableStateOf(false) }
            val hasDetails = item.diffFiles.isNotEmpty() ||
                !item.previewBody.isNullOrBlank() ||
                item.detail !is FfiToolCallDetail.FileRead

            Column(modifier = Modifier.fillMaxWidth()) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(6.dp))
                        .clickable(enabled = hasDetails) {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            expanded = !expanded
                        }
                        .padding(vertical = 3.dp),
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (running) {
                        BreathingDot(color = colors.fgSecondary)
                    } else {
                        Icon(
                            imageVector = semanticStepIcon(stepKind, targetDisplay),
                            contentDescription = null,
                            tint = if (failed) colors.dangerRemoved else colors.fgMuted,
                            modifier = Modifier.size(14.dp),
                        )
                    }

                    Text(
                        text = verb,
                        style = typography.metaMedium,
                        color = if (failed) colors.dangerRemoved else colors.fgSecondary,
                        maxLines = 1,
                    )

                    Text(
                        text = targetDisplay,
                        style = typography.code,
                        color = if (failed) colors.dangerRemoved else colors.fgPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )

                    if (descriptor != null && (descriptor.additions > 0u || descriptor.deletions > 0u)) {
                        if (descriptor.additions > 0u) {
                            Text(
                                text = "+${descriptor.additions}",
                                style = typography.meta,
                                color = colors.directAdded,
                            )
                        }
                        if (descriptor.deletions > 0u) {
                            Text(
                                text = "-${descriptor.deletions}",
                                style = typography.meta,
                                color = colors.dangerRemoved,
                            )
                        }
                    }

                    descriptor?.secondaryMeta?.takeIf { it.isNotBlank() }?.let { meta ->
                        Text(
                            text = meta,
                            style = typography.meta,
                            color = colors.fgMuted,
                            maxLines = 1,
                        )
                    }

                    if (hasDetails) {
                        val rot by animateFloatAsState(
                            targetValue = if (expanded) 90f else 0f,
                            animationSpec = tween(160),
                            label = "tool_chevron",
                        )
                        Icon(
                            imageVector = Icons.Outlined.ChevronRight,
                            contentDescription = null,
                            tint = colors.fgMuted,
                            modifier = Modifier
                                .size(14.dp)
                                .rotate(rot),
                        )
                    }
                }

                if (hasDetails) {
                    AnimatedVisibility(
                        visible = expanded,
                        enter = expandVertically(expandFrom = Alignment.Top) + fadeIn(tween(150)),
                        exit = shrinkVertically(shrinkTowards = Alignment.Top) + fadeOut(tween(120)),
                    ) {
                        ToolStepDetailPanel(
                            item = item,
                            descriptor = descriptor,
                            onOpenDiffFile = onOpenDiffFile,
                            onQuoteDiffLine = onQuoteDiffLine,
                            modifier = Modifier.padding(top = OrbitSpacing.xs, bottom = OrbitSpacing.xs),
                        )
                    }
                }
            }
        }

        is FfiTimelineItem.Notice -> NoticeBannerRow(notice = item)
        is FfiTimelineItem.AssistantMessage -> {
            MarkdownBlocks(
                blocks = item.blocks,
                fallbackText = item.text,
            )
        }
        is FfiTimelineItem.AgentMessage -> {
            AgentMessageBody(item, onInterruptDeliver = onInterruptDeliver)
        }
        is FfiTimelineItem.PermissionRequest,
        is FfiTimelineItem.QuestionRequest,
        is FfiTimelineItem.UserPrompt -> Unit
    }
}

private fun buildToolSnippetAndCopyPayload(
    item: FfiTimelineItem.ToolCall,
    descriptor: FfiToolStepDescriptor?,
): Pair<String, String> = when (val d = item.detail) {
    is FfiToolCallDetail.Shell -> {
        val header = buildString {
            append("$ ${d.command}")
            d.exitCode?.let { append("  (exit $it)") }
        }
        val body = d.outputPreview?.trim().orEmpty().ifEmpty { item.previewBody?.trim().orEmpty() }
        val combined = if (body.isNotEmpty()) "$header\n$body" else header
        combined to combined
    }
    is FfiToolCallDetail.FileRead -> {
        val loc = buildString {
            append(d.path)
            if (d.startLine != null && d.endLine != null) {
                append(":${d.startLine}-${d.endLine}")
            }
        }
        val body = item.previewBody?.trim().orEmpty()
        val display = if (body.isNotEmpty()) "$loc\n$body" else loc
        display to (body.ifEmpty { loc })
    }
    is FfiToolCallDetail.FileEdit -> {
        val patch = d.unifiedDiff?.trim().orEmpty().ifEmpty { item.previewBody?.trim().orEmpty() }
        patch to (patch.ifEmpty { d.path })
    }
    is FfiToolCallDetail.Search -> {
        val q = if (d.scope != null) "${d.query} (${d.scope})" else d.query
        val body = item.previewBody?.trim().orEmpty()
        val combined = if (body.isNotEmpty()) "$q\n$body" else q
        combined to combined
    }
    is FfiToolCallDetail.Subagent -> {
        val body = buildString {
            if (d.role.isNotBlank()) appendLine(d.role)
            if (d.promptPreview.isNotBlank()) appendLine(d.promptPreview)
            item.previewBody?.trim()?.takeIf { it.isNotEmpty() }?.let { append(it) }
        }.trim()
        body to body
    }
    is FfiToolCallDetail.Generic -> {
        val preview = item.previewBody?.trim().orEmpty().ifEmpty { d.argumentsJson.trim() }
        preview to preview
    }
}

@Composable
private fun ToolStepDetailPanel(
    item: FfiTimelineItem.ToolCall,
    descriptor: FfiToolStepDescriptor?,
    onOpenDiffFile: (String) -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val context = LocalContext.current
    val haptics = LocalHapticFeedback.current
    val panelShape = RoundedCornerShape(10.dp)
    val (snippetText, copyPayload) = remember(item, descriptor) {
        buildToolSnippetAndCopyPayload(item, descriptor)
    }
    var copied by remember(item.callId) { mutableStateOf(false) }

    LaunchedEffect(copied) {
        if (copied) {
            delay(1500L)
            copied = false
        }
    }

    Column(
        modifier = modifier
            .fillMaxWidth()
            .clip(panelShape)
            .background(colors.code, panelShape)
            .border(OrbitSpacing.hairline, colors.hairline, panelShape)
            .padding(OrbitSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
    ) {
        if (snippetText.isNotEmpty()) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.Top,
            ) {
                SelectionContainer(
                    modifier = Modifier
                        .weight(1f)
                        .horizontalScroll(rememberScrollState()),
                ) {
                    Text(
                        text = snippetText,
                        style = typography.code,
                        color = if (colors.isDark) Color(0xFFE6E8EE) else colors.fgPrimary,
                    )
                }
                if (copyPayload.isNotEmpty()) {
                    Box(
                        modifier = Modifier
                            .size(26.dp)
                            .clip(CircleShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                                cm?.setPrimaryClip(ClipData.newPlainText("step_output", copyPayload))
                                copied = true
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = if (copied) Icons.Outlined.Check else OrbitCopyIcon,
                            contentDescription = "Copy output",
                            tint = if (copied) colors.directAdded else colors.fgMuted,
                            modifier = Modifier.size(13.dp),
                        )
                    }
                }
            }
        }

        item.diffFiles.forEach { diffFile ->
            InlineDiffFileBlock(
                diffFile = diffFile,
                onOpenDiffFile = { onOpenDiffFile(diffFile.path) },
                onQuoteDiffLine = onQuoteDiffLine,
            )
        }
    }
}

@Composable
private fun InlineDiffFileBlock(
    diffFile: FfiDiffFile,
    onOpenDiffFile: () -> Unit,
    onQuoteDiffLine: (path: String, lineLabel: String, content: String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val shape = RoundedCornerShape(8.dp)

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.canvas, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onOpenDiffFile)
                .padding(horizontal = OrbitSpacing.sm, vertical = 6.dp),
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = orbitSemanticFileIcon(OrbitFormatters.shortPath(diffFile.path)),
                contentDescription = null,
                tint = colors.fgSecondary,
                modifier = Modifier.size(13.dp),
            )
            Text(
                text = OrbitFormatters.shortPath(diffFile.path),
                style = typography.codeMedium,
                color = colors.fgPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (diffFile.additions > 0u) {
                Text(
                    text = "+${diffFile.additions}",
                    style = typography.meta,
                    color = colors.directAdded,
                )
            }
            if (diffFile.deletions > 0u) {
                Text(
                    text = "-${diffFile.deletions}",
                    style = typography.meta,
                    color = colors.dangerRemoved,
                )
            }
        }

        if (diffFile.lines.isNotEmpty()) {
            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
            SelectionContainer {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .horizontalScroll(rememberScrollState())
                        .padding(vertical = 4.dp),
                ) {
                    diffFile.lines.take(30).forEach { line ->
                        val bg = when (line.kind) {
                            FfiDiffLineKind.ADDED -> colors.diffAddedBg
                            FfiDiffLineKind.REMOVED -> colors.diffRemovedBg
                            else -> Color.Transparent
                        }
                        val fg = when (line.kind) {
                            FfiDiffLineKind.ADDED -> colors.directAdded
                            FfiDiffLineKind.REMOVED -> colors.dangerRemoved
                            FfiDiffLineKind.HUNK_HEADER,
                            FfiDiffLineKind.META -> colors.fgMuted
                            FfiDiffLineKind.CONTEXT -> colors.fgSecondary
                        }
                        val prefix = when (line.kind) {
                            FfiDiffLineKind.ADDED -> "+"
                            FfiDiffLineKind.REMOVED -> "-"
                            else -> " "
                        }
                        val lineNo = (line.newLine ?: line.oldLine)?.toString() ?: ""
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .background(bg)
                                .clickable(
                                    enabled = line.kind == FfiDiffLineKind.ADDED || line.kind == FfiDiffLineKind.REMOVED
                                ) {
                                    onQuoteDiffLine(diffFile.path, lineNo, line.content)
                                }
                                .padding(horizontal = OrbitSpacing.sm, vertical = 1.dp),
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                        ) {
                            Text(
                                text = lineNo.padStart(4),
                                style = typography.code,
                                color = colors.fgMuted,
                            )
                            Text(
                                text = "$prefix${line.content}",
                                style = typography.code,
                                color = fg,
                            )
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun TurnChangesBar(
    diffs: List<FfiTurnFileDiffSummary>,
    activeCwd: String,
    onOpenDiffFile: (String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    var expanded by remember { mutableStateOf(false) }

    val totalAdditions = remember(diffs) { diffs.sumOf { it.additions.toLong() } }
    val totalDeletions = remember(diffs) { diffs.sumOf { it.deletions.toLong() } }
    val shape = RoundedCornerShape(10.dp)

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.subtleFill, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape)
            .padding(horizontal = OrbitSpacing.sm, vertical = 6.dp),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    if (diffs.size == 1) {
                        onOpenDiffFile(diffs.first().path)
                    } else {
                        expanded = !expanded
                    }
                },
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = Icons.Outlined.Difference,
                contentDescription = null,
                tint = colors.fgSecondary,
                modifier = Modifier.size(14.dp),
            )
            Text(
                text = if (diffs.size == 1) {
                    OrbitFormatters.stripWorkspacePrefix(diffs.first().path, activeCwd)
                } else {
                    "${diffs.size} files changed"
                },
                style = typography.metaMedium,
                color = colors.fgPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (totalAdditions > 0L) {
                Text(
                    text = "+$totalAdditions",
                    style = typography.meta,
                    color = colors.directAdded,
                )
            }
            if (totalDeletions > 0L) {
                Text(
                    text = "-$totalDeletions",
                    style = typography.meta,
                    color = colors.dangerRemoved,
                )
            }
            if (diffs.size > 1) {
                val rot by animateFloatAsState(
                    targetValue = if (expanded) 90f else 0f,
                    animationSpec = tween(160),
                    label = "changes_chevron",
                )
                Icon(
                    imageVector = Icons.Outlined.ChevronRight,
                    contentDescription = null,
                    tint = colors.fgMuted,
                    modifier = Modifier
                        .size(14.dp)
                        .rotate(rot),
                )
            }
        }

        if (diffs.size > 1) {
            AnimatedVisibility(
                visible = expanded,
                enter = expandVertically(expandFrom = Alignment.Top) + fadeIn(tween(140)),
                exit = shrinkVertically(shrinkTowards = Alignment.Top) + fadeOut(tween(110)),
            ) {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    modifier = Modifier.padding(top = 2.dp),
                ) {
                    diffs.forEach { d ->
                        val pillShape = RoundedCornerShape(7.dp)
                        Row(
                            modifier = Modifier
                                .clip(pillShape)
                                .background(colors.canvas, pillShape)
                                .border(OrbitSpacing.hairline, colors.hairline, pillShape)
                                .clickable { onOpenDiffFile(d.path) }
                                .padding(horizontal = 7.dp, vertical = 3.dp),
                            horizontalArrangement = Arrangement.spacedBy(4.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                text = OrbitFormatters.shortPath(d.path),
                                style = typography.code,
                                color = colors.fgPrimary,
                            )
                            if (d.additions > 0u) {
                                Text(
                                    text = "+${d.additions}",
                                    style = typography.meta,
                                    color = colors.directAdded,
                                )
                            }
                            if (d.deletions > 0u) {
                                Text(
                                    text = "-${d.deletions}",
                                    style = typography.meta,
                                    color = colors.dangerRemoved,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun NoticeBannerRow(notice: FfiTimelineItem.Notice) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val isErr = notice.level.equals("error", ignoreCase = true)
    val shape = RoundedCornerShape(10.dp)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(if (isErr) colors.diffRemovedBg else colors.subtleFill, shape)
            .border(
                OrbitSpacing.hairline,
                if (isErr) colors.dangerRemoved.copy(alpha = 0.45f) else colors.hairline,
                shape,
            )
            .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = Icons.Outlined.ErrorOutline,
            contentDescription = null,
            tint = if (isErr) colors.dangerRemoved else colors.fgSecondary,
            modifier = Modifier.size(15.dp),
        )
        SelectionContainer {
            Text(
                text = notice.message,
                style = typography.meta,
                color = if (isErr) colors.dangerRemoved else colors.fgSecondary,
            )
        }
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
