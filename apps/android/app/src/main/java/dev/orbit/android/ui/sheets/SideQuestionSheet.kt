package dev.orbit.android.ui.sheets

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Reply
import androidx.compose.material.icons.outlined.ArrowUpward
import androidx.compose.material.icons.outlined.AutoAwesome
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material.icons.outlined.Forum
import androidx.compose.material.icons.outlined.NorthEast
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.unit.dp
import dev.orbit.android.ui.stage.MarkdownBlocks
import dev.orbit.android.ui.theme.OrbitCopyIcon
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.parseMarkdownPreview

@Immutable
data class SideQuestionEntry(
    val id: String,
    val question: String,
    val answer: String = "",
    val isStreaming: Boolean = true,
    val error: String? = null,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SideQuestionSheet(
    entries: List<SideQuestionEntry>,
    draft: String,
    onDraftChange: (String) -> Unit,
    onAsk: (String) -> Unit,
    onClear: () -> Unit,
    onPromoteToComposer: (SideQuestionEntry) -> Unit,
    onCopiedAnswer: () -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val context = LocalContext.current
    val haptics = LocalHapticFeedback.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val listState = rememberLazyListState()

    val quickPrompts = remember {
        listOf(
            "Summarize the current task state and next steps",
            "Explain the latest error or failure concisely",
            "What edge cases or regressions should we watch for?",
            "Summarize the uncommitted git changes",
        )
    }

    LaunchedEffect(entries.size) {
        if (entries.isNotEmpty()) {
            listState.animateScrollToItem(entries.lastIndex)
        }
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = colors.canvas,
        contentColor = colors.fgPrimary,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.86f)
                .imePadding()
                .navigationBarsPadding(),
        ) {
            // Header
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg)
                    .padding(bottom = OrbitSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                Icon(
                    imageVector = Icons.Outlined.Forum,
                    contentDescription = null,
                    tint = colors.fgPrimary,
                    modifier = Modifier.size(18.dp),
                )
                Text(
                    text = "Side Question (/btw)",
                    style = typography.title,
                    color = colors.fgPrimary,
                    modifier = Modifier.weight(1f),
                )
                if (entries.isNotEmpty()) {
                    Box(
                        modifier = Modifier
                            .size(36.dp)
                            .clip(CircleShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onClear()
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.DeleteSweep,
                            contentDescription = "Clear side questions",
                            tint = colors.fgSecondary,
                            modifier = Modifier.size(18.dp),
                        )
                    }
                }
                Box(
                    modifier = Modifier
                        .size(36.dp)
                        .clip(CircleShape)
                        .clickable { onDismiss() },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Close,
                        contentDescription = "Close",
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(18.dp),
                    )
                }
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            LazyColumn(
                state = listState,
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth(),
                contentPadding = PaddingValues(OrbitSpacing.lg),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
            ) {
                if (entries.isEmpty()) {
                    item(key = "side_q_empty") {
                        Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md)) {
                            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                Text(
                                    text = "Ask without interrupting",
                                    style = typography.bodySemiBold,
                                    color = colors.fgPrimary,
                                )
                                Text(
                                    text = "Read-only side thread with full session context. Does not pollute or block your main agent run.",
                                    style = typography.subhead,
                                    color = colors.fgSecondary,
                                )
                            }

                            Text(
                                text = "Quick Prompts",
                                style = typography.metaMedium,
                                color = colors.fgMuted,
                            )

                            Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm)) {
                                quickPrompts.forEach { prompt ->
                                    val shape = RoundedCornerShape(12.dp)
                                    Row(
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .clip(shape)
                                            .background(colors.elevated, shape)
                                            .border(OrbitSpacing.hairline, colors.hairline, shape)
                                            .clickable {
                                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                                onAsk(prompt)
                                            }
                                            .padding(horizontal = OrbitSpacing.md, vertical = 10.dp),
                                        verticalAlignment = Alignment.CenterVertically,
                                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                                    ) {
                                        Icon(
                                            imageVector = Icons.Outlined.AutoAwesome,
                                            contentDescription = null,
                                            tint = colors.fgSecondary,
                                            modifier = Modifier.size(15.dp),
                                        )
                                        Text(
                                            text = prompt,
                                            style = typography.subhead,
                                            color = colors.fgPrimary,
                                            modifier = Modifier.weight(1f),
                                        )
                                        Icon(
                                            imageVector = Icons.Outlined.NorthEast,
                                            contentDescription = null,
                                            tint = colors.fgMuted,
                                            modifier = Modifier.size(14.dp),
                                        )
                                    }
                                }
                            }
                        }
                    }
                } else {
                    items(entries, key = { it.id }) { entry ->
                        SideQuestionCard(
                            entry = entry,
                            onCopy = {
                                val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                                cm?.setPrimaryClip(ClipData.newPlainText("side_question_answer", entry.answer))
                                onCopiedAnswer()
                            },
                            onPromote = {
                                onPromoteToComposer(entry)
                                onDismiss()
                            },
                        )
                    }
                }
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            // Input bar
            val canSend = draft.trim().isNotEmpty()
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(colors.canvas)
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 40.dp, max = 108.dp)
                        .clip(RoundedCornerShape(20.dp))
                        .background(colors.subtleFill)
                        .padding(horizontal = 14.dp, vertical = 9.dp),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    if (draft.isEmpty()) {
                        Text(
                            text = "Ask a read-only side question…",
                            style = typography.body,
                            color = colors.fgMuted,
                        )
                    }
                    BasicTextField(
                        value = draft,
                        onValueChange = onDraftChange,
                        textStyle = typography.body.copy(color = colors.fgPrimary),
                        cursorBrush = SolidColor(colors.primary),
                        maxLines = 4,
                        modifier = Modifier.fillMaxWidth(),
                    )
                }

                Box(
                    modifier = Modifier
                        .size(36.dp)
                        .clip(CircleShape)
                        .background(if (canSend) colors.primary else colors.subtleFill)
                        .clickable(enabled = canSend) {
                            val q = draft.trim()
                            if (q.isNotEmpty()) {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onDraftChange("")
                                onAsk(q)
                            }
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = Icons.Outlined.ArrowUpward,
                        contentDescription = "Ask side question",
                        tint = if (canSend) colors.onPrimary else colors.fgMuted,
                        modifier = Modifier.size(18.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun SideQuestionCard(
    entry: SideQuestionEntry,
    onCopy: () -> Unit,
    onPromote: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val cardShape = RoundedCornerShape(14.dp)

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(cardShape)
            .background(colors.elevated, cardShape)
            .border(OrbitSpacing.hairline, colors.hairline, cardShape)
            .padding(OrbitSpacing.md),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
    ) {
        Row(
            verticalAlignment = Alignment.Top,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Box(
                modifier = Modifier
                    .size(20.dp)
                    .clip(CircleShape)
                    .background(colors.subtleFill),
                contentAlignment = Alignment.Center,
            ) {
                Text(text = "Q", style = typography.metaSemiBold, color = colors.fgPrimary)
            }
            Text(
                text = entry.question,
                style = typography.subheadSemiBold,
                color = colors.fgPrimary,
                modifier = Modifier.weight(1f),
            )
        }

        HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

        when {
            !entry.error.isNullOrBlank() -> {
                Row(
                    verticalAlignment = Alignment.Top,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.WarningAmber,
                        contentDescription = null,
                        tint = colors.dangerRemoved,
                        modifier = Modifier
                            .padding(top = 1.dp)
                            .size(15.dp),
                    )
                    Text(
                        text = entry.error,
                        style = typography.meta,
                        color = colors.dangerRemoved,
                    )
                }
            }
            entry.answer.isEmpty() && entry.isStreaming -> {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.padding(vertical = 4.dp),
                ) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(13.dp),
                        strokeWidth = 1.8.dp,
                        color = colors.fgSecondary,
                    )
                    Text(text = "Thinking…", style = typography.meta, color = colors.fgMuted)
                }
            }
            else -> {
                val parsed = remember(entry.answer, entry.isStreaming) {
                    runCatching { parseMarkdownPreview(entry.answer, entry.isStreaming) }
                        .getOrDefault(emptyList())
                }
                MarkdownBlocks(
                    blocks = parsed,
                    fallbackText = entry.answer,
                )
                if (entry.isStreaming) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(11.dp),
                            strokeWidth = 1.6.dp,
                            color = colors.fgMuted,
                        )
                        Text(text = "Streaming…", style = typography.meta, color = colors.fgMuted)
                    }
                }
            }
        }

        if (!entry.isStreaming && entry.answer.isNotBlank()) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 2.dp),
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(
                    modifier = Modifier
                        .clip(RoundedCornerShape(8.dp))
                        .clickable(onClick = onCopy)
                        .padding(horizontal = 6.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Icon(
                        imageVector = OrbitCopyIcon,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(13.dp),
                    )
                    Text(text = "Copy", style = typography.metaMedium, color = colors.fgSecondary)
                }

                Row(
                    modifier = Modifier
                        .clip(RoundedCornerShape(8.dp))
                        .clickable(onClick = onPromote)
                        .padding(horizontal = 6.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Outlined.Reply,
                        contentDescription = null,
                        tint = colors.primary,
                        modifier = Modifier.size(14.dp),
                    )
                    Text(text = "Send to Main Chat", style = typography.metaSemiBold, color = colors.primary)
                }

                Spacer(modifier = Modifier.weight(1f))
            }
        }
    }
}
