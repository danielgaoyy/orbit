package dev.orbit.android.ui.stage

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiQuestionItem
import uniffi.orbit_ffi_uniffi.FfiTimelineItem

private sealed interface QueuedDecisionItem {
    val seq: ULong
    val requestId: String

    data class Permission(val item: FfiTimelineItem.PermissionRequest) : QueuedDecisionItem {
        override val seq: ULong = item.seq
        override val requestId: String = item.requestId
    }

    data class Question(val item: FfiTimelineItem.QuestionRequest) : QueuedDecisionItem {
        override val seq: ULong = item.seq
        override val requestId: String = item.requestId
    }
}

/**
 * Unclipped Single-Focus Decision Queue:
 * Combines pending permissions and pending questions into a unified `seq`-sorted queue
 * and renders ONLY ONE active card at a time at its full natural unclipped height,
 * with a subtle `1 of N` + `<` `>` stepper when `N > 1`.
 */
@Composable
fun StickyDecisionBar(
    pendingPermissions: List<FfiTimelineItem>,
    pendingQuestions: List<FfiTimelineItem>,
    onResolvePermission: (requestId: String, decision: String) -> Unit,
    onAnswerQuestion: (requestId: String, answers: List<List<String>>) -> Unit,
    modifier: Modifier = Modifier,
) {
    val queue = remember(pendingPermissions, pendingQuestions) {
        buildList {
            pendingPermissions
                .filterIsInstance<FfiTimelineItem.PermissionRequest>()
                .filter { it.resolvedDecision == null }
                .forEach { add(QueuedDecisionItem.Permission(it)) }

            pendingQuestions
                .filterIsInstance<FfiTimelineItem.QuestionRequest>()
                .filter { it.resolvedAnswers == null }
                .forEach { add(QueuedDecisionItem.Question(it)) }
        }
            .distinctBy { it.requestId }
            .sortedBy { it.seq }
    }

    if (queue.isEmpty()) return

    var focusedRequestId by rememberSaveable { mutableStateOf<String?>(null) }
    var fallbackSlotIndex by rememberSaveable { mutableIntStateOf(0) }

    val activeIndex = remember(queue, focusedRequestId, fallbackSlotIndex) {
        val byId = focusedRequestId?.let { targetId ->
            queue.indexOfFirst { it.requestId == targetId }
        } ?: -1
        if (byId >= 0) {
            byId
        } else {
            fallbackSlotIndex.coerceIn(0, queue.lastIndex)
        }
    }

    val activeEntry = queue[activeIndex]
    if (focusedRequestId != activeEntry.requestId) {
        focusedRequestId = activeEntry.requestId
        fallbackSlotIndex = activeIndex
    }

    val onStepPrev: () -> Unit = {
        if (activeIndex > 0) {
            val prevIdx = activeIndex - 1
            fallbackSlotIndex = prevIdx
            focusedRequestId = queue[prevIdx].requestId
        }
    }
    val onStepNext: () -> Unit = {
        if (activeIndex < queue.lastIndex) {
            val nextIdx = activeIndex + 1
            fallbackSlotIndex = nextIdx
            focusedRequestId = queue[nextIdx].requestId
        }
    }

    Box(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs),
    ) {
        when (activeEntry) {
            is QueuedDecisionItem.Permission -> {
                PermissionDecisionCard(
                    permission = activeEntry.item,
                    activeIndex = activeIndex,
                    totalCount = queue.size,
                    onStepPrev = onStepPrev,
                    onStepNext = onStepNext,
                    onResolve = onResolvePermission,
                )
            }
            is QueuedDecisionItem.Question -> {
                QuestionDecisionCard(
                    request = activeEntry.item,
                    activeIndex = activeIndex,
                    totalCount = queue.size,
                    onStepPrev = onStepPrev,
                    onStepNext = onStepNext,
                    onSubmit = onAnswerQuestion,
                )
            }
        }
    }
}

@Composable
private fun DecisionQueueStepperPill(
    activeIndex: Int,
    totalCount: Int,
    accentColor: Color,
    onStepPrev: () -> Unit,
    onStepNext: () -> Unit,
) {
    if (totalCount <= 1) return

    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val pillShape = RoundedCornerShape(OrbitSpacing.sm)
    val canPrev = activeIndex > 0
    val canNext = activeIndex < totalCount - 1

    Row(
        modifier = Modifier
            .clip(pillShape)
            .background(colors.subtleFill, pillShape)
            .border(OrbitSpacing.hairline, colors.hairline, pillShape),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(44.dp)
                .clip(CircleShape)
                .clickable(enabled = canPrev) {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    onStepPrev()
                },
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = Icons.AutoMirrored.Outlined.KeyboardArrowLeft,
                contentDescription = "Previous decision",
                tint = if (canPrev) colors.fgPrimary else colors.fgMuted,
                modifier = Modifier.size(OrbitSpacing.iconSm),
            )
        }
        Text(
            text = "${activeIndex + 1} of $totalCount",
            style = typography.metaMedium,
            color = accentColor,
            maxLines = 1,
            softWrap = false,
        )
        Box(
            modifier = Modifier
                .size(44.dp)
                .clip(CircleShape)
                .clickable(enabled = canNext) {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    onStepNext()
                },
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = Icons.AutoMirrored.Outlined.KeyboardArrowRight,
                contentDescription = "Next decision",
                tint = if (canNext) colors.fgPrimary else colors.fgMuted,
                modifier = Modifier.size(OrbitSpacing.iconSm),
            )
        }
    }
}

@Composable
private fun PermissionDecisionCard(
    permission: FfiTimelineItem.PermissionRequest,
    activeIndex: Int,
    totalCount: Int,
    onStepPrev: () -> Unit,
    onStepNext: () -> Unit,
    onResolve: (requestId: String, decision: String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val shape = RoundedCornerShape(OrbitSpacing.lg)

    val decisions = permission.allowedDecisions.ifEmpty {
        listOf("allow", "allow_session", "deny")
    }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.elevated, shape)
            .border(OrbitSpacing.hairline, colors.warning.copy(alpha = 0.55f), shape)
            .padding(OrbitSpacing.md),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Row(
                modifier = Modifier.weight(1f),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Box(
                    modifier = Modifier
                        .size(6.dp)
                        .background(colors.warning, CircleShape)
                )
                Text(
                    text = permission.title.ifBlank { "Approval Required" },
                    style = typography.subheadSemiBold,
                    color = colors.fgPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            DecisionQueueStepperPill(
                activeIndex = activeIndex,
                totalCount = totalCount,
                accentColor = colors.warning,
                onStepPrev = onStepPrev,
                onStepNext = onStepNext,
            )
        }

        if (permission.commandOrPath.isNotBlank()) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(colors.code, RoundedCornerShape(OrbitSpacing.sm))
                    .border(OrbitSpacing.hairline, colors.hairline, RoundedCornerShape(OrbitSpacing.sm))
                    .horizontalScroll(rememberScrollState())
                    .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs)
            ) {
                Text(
                    text = permission.commandOrPath,
                    style = typography.code,
                    color = colors.fgPrimary,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            decisions.forEach { rawDecision ->
                val isDeny = rawDecision.contains("deny", ignoreCase = true) ||
                    rawDecision.contains("reject", ignoreCase = true)
                val isAlways = rawDecision.contains("session", ignoreCase = true) ||
                    rawDecision.contains("always", ignoreCase = true)

                val buttonLabel = formatDecisionLabel(rawDecision)
                val bgColor = when {
                    isDeny -> colors.subtleFill
                    isAlways -> colors.subtleFill
                    else -> colors.primary
                }
                val fgColor = when {
                    isDeny -> colors.dangerRemoved
                    isAlways -> colors.fgPrimary
                    else -> Color.White
                }
                val btnShape = RoundedCornerShape(OrbitSpacing.md)

                Box(
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = OrbitSpacing.touchTarget)
                        .clip(btnShape)
                        .background(bgColor, btnShape)
                        .border(OrbitSpacing.hairline, colors.hairline, btnShape)
                        .clickable {
                            haptics.performHapticFeedback(
                                if (isDeny) HapticFeedbackType.LongPress else HapticFeedbackType.TextHandleMove
                            )
                            onResolve(permission.requestId, rawDecision)
                        }
                        .padding(horizontal = OrbitSpacing.sm),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = buttonLabel,
                        style = typography.subheadSemiBold,
                        color = fgColor,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

private fun formatDecisionLabel(raw: String): String = when (raw.lowercase()) {
    "allow", "allow_once", "approve", "once" -> "Allow Once"
    "allow_session", "allow_for_session", "always", "allow_always" -> "Allow Session"
    "deny", "reject", "cancel" -> "Deny"
    else -> raw.replace('_', ' ').replaceFirstChar { it.uppercase() }
}

@Composable
private fun QuestionDecisionCard(
    request: FfiTimelineItem.QuestionRequest,
    activeIndex: Int,
    totalCount: Int,
    onStepPrev: () -> Unit,
    onStepNext: () -> Unit,
    onSubmit: (requestId: String, answers: List<List<String>>) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val shape = RoundedCornerShape(OrbitSpacing.lg)

    val selections = remember(request.requestId) {
        mutableStateMapOf<Int, Set<String>>()
    }
    var customAnswer by remember(request.requestId) { mutableStateOf("") }

    val totalOptionCount = remember(request.questions) {
        request.questions.sumOf { it.options.size }
    }
    val topHeaderLabel = request.questions.firstOrNull()?.header?.takeIf { it.isNotBlank() }
        ?: "Input Requested"

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.elevated, shape)
            .border(OrbitSpacing.hairline, colors.primary.copy(alpha = 0.55f), shape)
            .padding(OrbitSpacing.md),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Row(
                modifier = Modifier.weight(1f),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Box(
                    modifier = Modifier
                        .size(6.dp)
                        .background(colors.primary, CircleShape)
                )
                Text(
                    text = topHeaderLabel,
                    style = typography.metaSemiBold,
                    color = colors.primary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            DecisionQueueStepperPill(
                activeIndex = activeIndex,
                totalCount = totalCount,
                accentColor = colors.primary,
                onStepPrev = onStepPrev,
                onStepNext = onStepNext,
            )
        }

        // Natural-height Column (zero collapsed nested scroll for <= 6 options)
        val questionsBodyModifier = if (totalOptionCount <= 6) {
            Modifier.fillMaxWidth()
        } else {
            Modifier
                .fillMaxWidth()
                .heightIn(max = 260.dp)
                .verticalScroll(rememberScrollState())
        }

        Column(
            modifier = questionsBodyModifier,
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            request.questions.forEachIndexed { qIdx, q: FfiQuestionItem ->
                if (qIdx > 0) {
                    q.header?.takeIf { it.isNotBlank() }?.let { hdr ->
                        Text(
                            text = hdr,
                            style = typography.metaSemiBold,
                            color = colors.primary,
                        )
                    }
                }
                Text(
                    text = q.question,
                    style = typography.subheadSemiBold,
                    color = colors.fgPrimary,
                )

                val selectedSet = selections[qIdx].orEmpty()
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    q.options.forEach { opt ->
                        val isSelected = opt in selectedSet
                        val rowShape = RoundedCornerShape(OrbitSpacing.md)
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 40.dp)
                                .clip(rowShape)
                                .background(
                                    if (isSelected) colors.subtleFill else colors.code,
                                    rowShape,
                                )
                                .border(
                                    OrbitSpacing.hairline,
                                    if (isSelected) colors.primary else colors.hairline,
                                    rowShape,
                                )
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    val next = if (q.isMultiSelect) {
                                        if (isSelected) selectedSet - opt else selectedSet + opt
                                    } else {
                                        if (isSelected) emptySet() else setOf(opt)
                                    }
                                    selections[qIdx] = next
                                }
                                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                        ) {
                            Box(
                                modifier = Modifier
                                    .size(OrbitSpacing.iconSm)
                                    .clip(CircleShape)
                                    .background(
                                        if (isSelected) colors.primary else Color.Transparent,
                                        CircleShape,
                                    )
                                    .border(
                                        OrbitSpacing.hairline,
                                        if (isSelected) colors.primary else colors.fgMuted,
                                        CircleShape,
                                    ),
                                contentAlignment = Alignment.Center,
                            ) {
                                if (isSelected) {
                                    Icon(
                                        imageVector = Icons.Outlined.Check,
                                        contentDescription = null,
                                        tint = Color.White,
                                        modifier = Modifier.size(OrbitSpacing.md),
                                    )
                                }
                            }
                            Text(
                                text = opt,
                                style = if (isSelected) typography.subheadSemiBold else typography.subheadMedium,
                                color = if (isSelected) colors.primary else colors.fgPrimary,
                                modifier = Modifier.weight(1f),
                            )
                        }
                    }
                }

                if (qIdx < request.questions.lastIndex) {
                    HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                }
            }
        }

        // Optional "Custom answer…" text field row + Submit CTA matching Desktop
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            Box(
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 40.dp)
                    .background(colors.code, RoundedCornerShape(OrbitSpacing.md))
                    .border(OrbitSpacing.hairline, colors.hairline, RoundedCornerShape(OrbitSpacing.md))
                    .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                contentAlignment = Alignment.CenterStart,
            ) {
                if (customAnswer.isEmpty()) {
                    Text(
                        text = "Custom answer…",
                        style = typography.subhead,
                        color = colors.fgMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                BasicTextField(
                    value = customAnswer,
                    onValueChange = { customAnswer = it },
                    textStyle = typography.subhead.copy(color = colors.fgPrimary),
                    cursorBrush = SolidColor(colors.primary),
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            }

            val canSubmit = customAnswer.isNotBlank() || selections.values.any { it.isNotEmpty() }
            val btnShape = RoundedCornerShape(OrbitSpacing.md)
            Box(
                modifier = Modifier
                    .heightIn(min = 40.dp)
                    .clip(btnShape)
                    .background(if (canSubmit) colors.primary else colors.subtleFill, btnShape)
                    .clickable(enabled = canSubmit) {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        val compiled = request.questions.indices.map { idx ->
                            val picked = selections[idx]?.toList().orEmpty()
                            if (idx == 0 && customAnswer.isNotBlank()) {
                                picked + customAnswer.trim()
                            } else {
                                picked
                            }
                        }
                        onSubmit(request.requestId, compiled)
                    }
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.sm),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = "Submit",
                    style = typography.subheadSemiBold,
                    color = if (canSubmit) Color.White else colors.fgMuted,
                )
            }
        }
    }
}
