package dev.orbit.android.ui.composer

import androidx.compose.animation.AnimatedVisibility
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
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.ArrowUpward
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Code
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Shield
import androidx.compose.material.icons.outlined.Stop
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Surface
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
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.ComposerDraftState
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.store.QueuedPromptItem
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiHostDetail
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiSessionMode

private val MentionTailRegex = Regex("(?:^|\\s)@([^\\s@]*)$")

private data class InlineSlashOption(
    val command: String,
    val description: String,
)

private val InlineSlashOptions = listOf(
    InlineSlashOption("/plan", "Draft step-by-step plan first"),
    InlineSlashOption("/review", "Audit working tree changes"),
    InlineSlashOption("/compact", "Compress session context"),
)

private sealed interface InlineMentionCandidate {
    val key: String
    val label: String
    val subtitle: String?

    data class Builtin(
        override val key: String,
        override val label: String,
        override val subtitle: String,
        val icon: ImageVector,
    ) : InlineMentionCandidate

    data class WorkspaceFile(
        val path: String,
    ) : InlineMentionCandidate {
        override val key: String = "file:$path"
        override val label: String = "@${OrbitFormatters.shortPath(path)}"
        override val subtitle: String = path
    }
}

private fun stripTrailingMentionToken(text: String): String {
    val atIdx = text.lastIndexOf('@')
    return if (atIdx < 0) text else text.substring(0, atIdx)
}

@Composable
fun OmniComposer(
    draft: ComposerDraftState,
    workspaceFiles: List<String>,
    queuedPrompts: List<QueuedPromptItem>,
    hostDetail: FfiHostDetail?,
    activeCwd: String,
    isNewSession: Boolean,
    isAgentRunning: Boolean,
    hasHost: Boolean,
    onTextChange: (String) -> Unit,
    onSend: () -> Unit,
    onQueue: () -> Unit,
    onInterrupt: () -> Unit,
    onOpenContextPicker: () -> Unit,
    onOpenModelSelector: () -> Unit,
    onSelectMode: (FfiSessionMode) -> Unit,
    onRequestRefreshFiles: () -> Unit,
    onAddFileMention: (String) -> Unit,
    onRemoveFileMention: (String) -> Unit,
    onAttachGitDiff: () -> Unit,
    onToggleGitDiff: () -> Unit,
    onAttachStagedDiff: () -> Unit,
    onToggleStagedDiff: () -> Unit,
    onAttachTerminal: () -> Unit,
    onToggleTerminal: () -> Unit,
    onSelectSlash: (String) -> Unit,
    onClearSlash: () -> Unit,
    onRemoveQuotedSnippet: (String) -> Unit,
    onEditQueuedPrompt: (String) -> Unit,
    onSendQueuedPromptNow: (String) -> Unit,
    onRemoveQueuedPrompt: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    var modeMenuExpanded by remember { mutableStateOf(false) }

    // 1. Detect `/` prefix (`/<query>`) while typing
    val slashQuery: String? = remember(draft.text, hasHost) {
        if (!hasHost) {
            null
        } else if (draft.text.startsWith("/") && !draft.text.contains(' ') && !draft.text.contains('\n')) {
            draft.text.removePrefix("/").lowercase()
        } else {
            null
        }
    }

    val matchingSlashOptions = remember(slashQuery) {
        if (slashQuery == null) {
            emptyList()
        } else {
            InlineSlashOptions.filter { opt ->
                val bare = opt.command.removePrefix("/")
                bare.startsWith(slashQuery) || bare.contains(slashQuery)
            }
        }
    }

    // 2. Detect trailing `@<query>` token while typing
    val mentionQuery: String? = remember(draft.text, hasHost) {
        if (!hasHost || slashQuery != null) {
            null
        } else {
            MentionTailRegex.find(draft.text)?.groupValues?.getOrNull(1)?.lowercase()
        }
    }

    LaunchedEffect(mentionQuery != null) {
        if (mentionQuery != null && workspaceFiles.isEmpty()) {
            onRequestRefreshFiles()
        }
    }

    val matchingMentionCandidates = remember(mentionQuery, workspaceFiles) {
        val q = mentionQuery ?: return@remember emptyList<InlineMentionCandidate>()
        val builtins = buildList {
            if (q.isEmpty() || "diff".contains(q) || "git-diff".contains(q)) {
                add(
                    InlineMentionCandidate.Builtin(
                        key = "builtin:diff",
                        label = "@diff",
                        subtitle = "Uncommitted working tree diff",
                        icon = Icons.Outlined.Code,
                    )
                )
            }
            if (q.isEmpty() || "staged".contains(q) || "index".contains(q)) {
                add(
                    InlineMentionCandidate.Builtin(
                        key = "builtin:staged",
                        label = "@staged",
                        subtitle = "Staged git index changes",
                        icon = Icons.Outlined.Code,
                    )
                )
            }
            if (q.isEmpty() || "terminal".contains(q) || "pty".contains(q)) {
                add(
                    InlineMentionCandidate.Builtin(
                        key = "builtin:terminal",
                        label = "@terminal",
                        subtitle = "Active PTY terminal output",
                        icon = Icons.Outlined.Terminal,
                    )
                )
            }
        }
        val remainingSlots = (6 - builtins.size).coerceAtLeast(0)
        val matchedFiles = if (remainingSlots == 0) {
            emptyList()
        } else if (q.isEmpty()) {
            workspaceFiles.take(remainingSlots)
        } else {
            workspaceFiles
                .asSequence()
                .filter { it.lowercase().contains(q) }
                .sortedWith(
                    compareBy<String> { path ->
                        val base = OrbitFormatters.shortPath(path).lowercase()
                        when {
                            base.startsWith(q) -> 0
                            base.contains(q) -> 1
                            else -> 2
                        }
                    }.thenBy { it.length }
                )
                .take(remainingSlots)
                .toList()
        }.map { InlineMentionCandidate.WorkspaceFile(it) }

        (builtins + matchedFiles).take(6)
    }

    val catalog = remember(hostDetail, draft.provider) {
        OrbitFormatters.catalogFor(hostDetail, draft.provider)
    }
    val modelShort = remember(catalog, draft.model) {
        OrbitFormatters.modelLabel(catalog, draft.model)
    }
    val compactEffort = remember(draft.effort) {
        draft.effort?.let { OrbitFormatters.compactEffortLabel(it) }
    }
    val modelPillText = buildString {
        append(modelShort)
        if (!compactEffort.isNullOrBlank()) {
            append(" · ")
            append(compactEffort)
        }
    }
    val providerDotColor = when (draft.provider) {
        FfiProviderKind.ANTIGRAVITY -> colors.primary
        FfiProviderKind.CODEX -> colors.directAdded
        FfiProviderKind.CLAUDE_CODE -> colors.warning
    }

    val modeLabel = OrbitFormatters.sessionModeLabel(draft.mode)
    val modeTint = when (draft.mode) {
        FfiSessionMode.FullAccess -> colors.warning
        FfiSessionMode.Plan -> colors.primary
        FfiSessionMode.AcceptEdits -> colors.directAdded
        else -> colors.fgSecondary
    }

    val capsuleShape = RoundedCornerShape(20.dp)
    val showSlashPopover = matchingSlashOptions.isNotEmpty()
    val showMentionPopover = !showSlashPopover && matchingMentionCandidates.isNotEmpty()

    Column(
        modifier = modifier
            .fillMaxWidth()
            .imePadding()
            .navigationBarsPadding()
            .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
    ) {
        // Floating `/` Slash Directive Autocomplete Card
        AnimatedVisibility(
            visible = showSlashPopover,
            enter = fadeIn() + expandVertically(),
            exit = fadeOut() + shrinkVertically(),
        ) {
            val popoverShape = RoundedCornerShape(OrbitSpacing.lg)
            Surface(
                shape = popoverShape,
                color = colors.elevated,
                shadowElevation = OrbitSpacing.sm,
                modifier = Modifier
                    .fillMaxWidth()
                    .border(OrbitSpacing.hairline, colors.hairline, popoverShape),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(OrbitSpacing.xs),
                ) {
                    matchingSlashOptions.forEach { wf ->
                        val itemShape = RoundedCornerShape(OrbitSpacing.md)
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 44.dp)
                                .clip(itemShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onSelectSlash(wf.command)
                                    onTextChange("")
                                }
                                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Terminal,
                                contentDescription = null,
                                tint = colors.primary,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                            Text(
                                text = wf.command,
                                style = typography.codeMedium,
                                color = colors.primary,
                                maxLines = 1,
                                softWrap = false,
                            )
                            Text(
                                text = wf.description,
                                style = typography.meta,
                                color = colors.fgSecondary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                        }
                    }
                }
            }
        }

        // Floating `@` Mention Autocomplete Card (Top 6 matches from @diff, @staged, @terminal, workspaceFiles)
        AnimatedVisibility(
            visible = showMentionPopover,
            enter = fadeIn() + expandVertically(),
            exit = fadeOut() + shrinkVertically(),
        ) {
            val popoverShape = RoundedCornerShape(OrbitSpacing.lg)
            Surface(
                shape = popoverShape,
                color = colors.elevated,
                shadowElevation = OrbitSpacing.sm,
                modifier = Modifier
                    .fillMaxWidth()
                    .border(OrbitSpacing.hairline, colors.hairline, popoverShape),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(OrbitSpacing.xs),
                ) {
                    matchingMentionCandidates.forEach { cand ->
                        val itemShape = RoundedCornerShape(OrbitSpacing.md)
                        val iconVec = when (cand) {
                            is InlineMentionCandidate.Builtin -> cand.icon
                            is InlineMentionCandidate.WorkspaceFile -> Icons.Outlined.Description
                        }
                        val iconColor = when (cand) {
                            is InlineMentionCandidate.Builtin -> if (cand.label == "@terminal") {
                                colors.warning
                            } else {
                                colors.directAdded
                            }
                            is InlineMentionCandidate.WorkspaceFile -> colors.fgMuted
                        }
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 40.dp)
                                .clip(itemShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    val stripped = stripTrailingMentionToken(draft.text)
                                    onTextChange(stripped)
                                    when (cand) {
                                        is InlineMentionCandidate.Builtin -> when (cand.label) {
                                            "@diff" -> onAttachGitDiff()
                                            "@staged" -> onAttachStagedDiff()
                                            "@terminal" -> onAttachTerminal()
                                        }
                                        is InlineMentionCandidate.WorkspaceFile -> onAddFileMention(cand.path)
                                    }
                                }
                                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                        ) {
                            Icon(
                                imageVector = iconVec,
                                contentDescription = null,
                                tint = iconColor,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                            Text(
                                text = cand.label,
                                style = typography.codeMedium,
                                color = colors.fgPrimary,
                                maxLines = 1,
                                softWrap = false,
                            )
                            cand.subtitle?.let { sub ->
                                Text(
                                    text = sub,
                                    style = typography.meta,
                                    color = colors.fgMuted,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier.weight(1f),
                                )
                            }
                        }
                    }
                }
            }
        }

        // Compact Queued Prompts Tray (Edit, Send Now, × remove)
        AnimatedVisibility(
            visible = queuedPrompts.isNotEmpty(),
            enter = fadeIn() + expandVertically(),
            exit = fadeOut() + shrinkVertically(),
        ) {
            val trayShape = RoundedCornerShape(OrbitSpacing.lg)
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(trayShape)
                    .background(colors.elevated, trayShape)
                    .border(OrbitSpacing.hairline, colors.hairline, trayShape)
                    .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                Text(
                    text = "Queued (${queuedPrompts.size})",
                    style = typography.metaSemiBold,
                    color = colors.fgMuted,
                )
                queuedPrompts.forEachIndexed { idx, item ->
                    if (idx > 0) {
                        HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                    }
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(min = 44.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                    ) {
                        Text(
                            text = item.previewText,
                            style = typography.subhead,
                            color = colors.fgPrimary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        Text(
                            text = "Edit",
                            style = typography.metaMedium,
                            color = colors.fgSecondary,
                            modifier = Modifier
                                .clip(RoundedCornerShape(OrbitSpacing.xs))
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onEditQueuedPrompt(item.id)
                                }
                                .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.sm),
                        )
                        Text(
                            text = "Send Now",
                            style = typography.metaSemiBold,
                            color = colors.primary,
                            maxLines = 1,
                            softWrap = false,
                            modifier = Modifier
                                .clip(RoundedCornerShape(OrbitSpacing.xs))
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onSendQueuedPromptNow(item.id)
                                }
                                .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.sm),
                        )
                        Box(
                            modifier = Modifier
                                .size(44.dp)
                                .clip(CircleShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onRemoveQueuedPrompt(item.id)
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Close,
                                contentDescription = "Remove queued prompt",
                                tint = colors.fgMuted,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                        }
                    }
                }
            }
        }

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clip(capsuleShape)
                .background(colors.elevated, capsuleShape)
                .border(OrbitSpacing.hairline, colors.hairline, capsuleShape)
                .padding(top = OrbitSpacing.md, bottom = OrbitSpacing.xs),
        ) {
            // Context Tray (chips for @file, @diff, @staged, @terminal, /slash, quotes)
            if (draft.hasContextAttachments) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .horizontalScroll(rememberScrollState())
                        .padding(
                            start = OrbitSpacing.lg,
                            end = OrbitSpacing.lg,
                            bottom = OrbitSpacing.sm,
                        ),
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    draft.activeSlashCommand?.let { cmd ->
                        RemovableContextChip(
                            label = cmd,
                            accent = colors.primary,
                            onRemove = onClearSlash,
                        )
                    }
                    if (draft.includeGitDiff) {
                        RemovableContextChip(
                            label = "@diff",
                            accent = colors.directAdded,
                            onRemove = onToggleGitDiff,
                        )
                    }
                    if (draft.includeStagedDiff) {
                        RemovableContextChip(
                            label = "@staged",
                            accent = colors.directAdded,
                            onRemove = onToggleStagedDiff,
                        )
                    }
                    if (draft.includeTerminal) {
                        RemovableContextChip(
                            label = "@terminal",
                            accent = colors.warning,
                            onRemove = onToggleTerminal,
                        )
                    }
                    draft.mentionedFiles.forEach { file ->
                        RemovableContextChip(
                            label = "@${OrbitFormatters.shortPath(file)}",
                            accent = colors.fgPrimary,
                            onRemove = { onRemoveFileMention(file) },
                        )
                    }
                    draft.quotedSnippets.forEach { q ->
                        RemovableContextChip(
                            label = q.badgeLabel,
                            accent = colors.relay,
                            onRemove = { onRemoveQuotedSnippet(q.id) },
                        )
                    }
                }
            }

            // Multi-line auto-growing BasicTextField (maxLines = 6)
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg),
            ) {
                if (draft.text.isEmpty()) {
                    val placeholder = when {
                        !hasHost -> "Pair a host to start coding..."
                        isNewSession -> "New session in ${OrbitFormatters.shortPath(activeCwd)}..."
                        isAgentRunning -> "Queue follow-up or reference @file..."
                        else -> "Reply, type /workflow, or @file..."
                    }
                    Text(
                        text = placeholder,
                        style = typography.body,
                        color = colors.fgMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                BasicTextField(
                    value = draft.text,
                    onValueChange = onTextChange,
                    enabled = hasHost,
                    textStyle = typography.body.copy(color = colors.fgPrimary),
                    cursorBrush = SolidColor(colors.primary),
                    maxLines = 6,
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = OrbitSpacing.xl),
                )
            }

            // Bottom Control Row ([+]  [Provider · Model · Effort]  [Mode]  ... [Send/Stop])
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.xs),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Row(
                    modifier = Modifier.weight(1f),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    // [+] Context / Slash Picker (48dp touch target)
                    Box(
                        modifier = Modifier
                            .size(OrbitSpacing.touchTarget)
                            .clip(CircleShape)
                            .clickable(enabled = hasHost) {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onOpenContextPicker()
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Add,
                            contentDescription = "Attach context or slash workflow",
                            tint = colors.fgSecondary,
                            modifier = Modifier.size(OrbitSpacing.iconMd),
                        )
                    }

                    // [● Model · Effort] Compact Zero-Truncation Pill
                    Row(
                        modifier = Modifier
                            .weight(1f, fill = false)
                            .heightIn(min = OrbitSpacing.touchTarget)
                            .clip(RoundedCornerShape(OrbitSpacing.md))
                            .clickable(enabled = hasHost) {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onOpenModelSelector()
                            }
                            .padding(horizontal = OrbitSpacing.sm),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                    ) {
                        Box(
                            modifier = Modifier
                                .size(6.dp)
                                .background(providerDotColor, CircleShape)
                        )
                        Text(
                            text = modelPillText,
                            style = typography.metaMedium,
                            color = colors.fgSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }

                    // [Mode] Shield Selector Pill
                    Box {
                        Row(
                            modifier = Modifier
                                .heightIn(min = OrbitSpacing.touchTarget)
                                .clip(RoundedCornerShape(OrbitSpacing.md))
                                .clickable(enabled = hasHost) {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    modeMenuExpanded = true
                                }
                                .padding(horizontal = OrbitSpacing.sm),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Shield,
                                contentDescription = "Session permission mode",
                                tint = modeTint,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                            Text(
                                text = modeLabel,
                                style = typography.metaMedium,
                                color = modeTint,
                                maxLines = 1,
                                softWrap = false,
                            )
                        }

                        DropdownMenu(
                            expanded = modeMenuExpanded,
                            onDismissRequest = { modeMenuExpanded = false },
                            containerColor = colors.elevated,
                        ) {
                            val modes = listOf(
                                FfiSessionMode.Default to "Ask before commands and edits",
                                FfiSessionMode.Plan to "Read-only research and planning",
                                FfiSessionMode.AcceptEdits to "Auto-apply file edits, ask for shell",
                                FfiSessionMode.FullAccess to "Execute tools without prompts",
                            )
                            modes.forEach { (m, desc) ->
                                val selected = m == draft.mode
                                DropdownMenuItem(
                                    text = {
                                        Column {
                                            Text(
                                                text = OrbitFormatters.sessionModeLabel(m),
                                                style = typography.subheadSemiBold,
                                                color = if (selected) colors.primary else colors.fgPrimary,
                                            )
                                            Text(
                                                text = desc,
                                                style = typography.meta,
                                                color = colors.fgSecondary,
                                            )
                                        }
                                    },
                                    onClick = {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        onSelectMode(m)
                                        modeMenuExpanded = false
                                    }
                                )
                            }
                        }
                    }

                    // Compact subtle Stop button next to Mode shield when running & draft is non-empty
                    if (isAgentRunning && draft.canSend) {
                        val stopPillShape = RoundedCornerShape(OrbitSpacing.md)
                        Row(
                            modifier = Modifier
                                .heightIn(min = OrbitSpacing.xxl)
                                .clip(stopPillShape)
                                .background(colors.diffRemovedBg, stopPillShape)
                                .border(
                                    OrbitSpacing.hairline,
                                    colors.dangerRemoved.copy(alpha = 0.45f),
                                    stopPillShape,
                                )
                                .clickable(enabled = hasHost) {
                                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                    onInterrupt()
                                }
                                .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Stop,
                                contentDescription = "Interrupt session",
                                tint = colors.dangerRemoved,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                            Text(
                                text = "Stop",
                                style = typography.metaMedium,
                                color = colors.dangerRemoved,
                                maxLines = 1,
                                softWrap = false,
                            )
                        }
                    }
                }

                // Primary Action Button:
                // - Running + empty draft -> Stop (■)
                // - Running + non-empty draft -> Queue
                // - Idle -> Send (↑)
                val showStop = isAgentRunning && !draft.canSend
                val showQueue = isAgentRunning && draft.canSend
                val enabled = hasHost && (showStop || draft.canSend)

                if (showQueue) {
                    val queueShape = RoundedCornerShape(18.dp)
                    Box(
                        modifier = Modifier
                            .height(OrbitSpacing.touchTarget)
                            .padding(end = OrbitSpacing.xs)
                            .clip(queueShape)
                            .clickable(enabled = enabled) {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onQueue()
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Box(
                            modifier = Modifier
                                .height(36.dp)
                                .background(colors.primary, queueShape)
                                .padding(horizontal = OrbitSpacing.md),
                            contentAlignment = Alignment.Center,
                        ) {
                            Text(
                                text = "Queue",
                                style = typography.metaSemiBold,
                                color = Color.White,
                                maxLines = 1,
                                softWrap = false,
                            )
                        }
                    }
                } else {
                    val circleFill = when {
                        showStop -> colors.dangerRemoved
                        enabled -> colors.primary
                        else -> colors.subtleFill
                    }
                    val iconTint = if (showStop || enabled) Color.White else colors.fgMuted

                    Box(
                        modifier = Modifier
                            .size(OrbitSpacing.touchTarget)
                            .clip(CircleShape)
                            .clickable(enabled = enabled) {
                                if (showStop) {
                                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                    onInterrupt()
                                } else {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onSend()
                                }
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Box(
                            modifier = Modifier
                                .size(36.dp)
                                .background(circleFill, CircleShape),
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                imageVector = if (showStop) Icons.Outlined.Stop else Icons.Outlined.ArrowUpward,
                                contentDescription = if (showStop) "Interrupt session" else "Send prompt",
                                tint = iconTint,
                                modifier = Modifier.size(OrbitSpacing.iconMd),
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun RemovableContextChip(
    label: String,
    accent: Color,
    onRemove: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val shape = RoundedCornerShape(OrbitSpacing.sm)

    Row(
        modifier = Modifier
            .clip(shape)
            .background(colors.subtleFill, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape)
            .clickable { onRemove() }
            .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
    ) {
        Text(
            text = label,
            style = typography.metaMedium,
            color = accent,
            maxLines = 1,
        )
        Icon(
            imageVector = Icons.Outlined.Close,
            contentDescription = "Remove $label",
            tint = colors.fgMuted,
            modifier = Modifier.size(12.dp),
        )
    }
}
