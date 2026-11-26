package dev.orbit.android.ui.composer

import android.content.Context
import android.graphics.BitmapFactory
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.Image
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.CallSplit
import androidx.compose.material.icons.automirrored.outlined.PlaylistAdd
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Archive
import androidx.compose.material.icons.outlined.ArrowUpward
import androidx.compose.material.icons.outlined.AttachFile
import androidx.compose.material.icons.outlined.AutoAwesome
import androidx.compose.material.icons.outlined.Bolt
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Checklist
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.ContentCut
import androidx.compose.material.icons.outlined.DeleteOutline
import androidx.compose.material.icons.outlined.Difference
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.EditNote
import androidx.compose.material.icons.outlined.FindInPage
import androidx.compose.material.icons.outlined.FormatQuote
import androidx.compose.material.icons.outlined.Forum
import androidx.compose.material.icons.outlined.GppMaybe
import androidx.compose.material.icons.outlined.IosShare
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material.icons.outlined.Layers
import androidx.compose.material.icons.outlined.PhotoCamera
import androidx.compose.material.icons.outlined.PhotoLibrary
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Shield
import androidx.compose.material.icons.outlined.Stop
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material.icons.outlined.VerifiedUser
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import dev.orbit.android.store.ComposerDraftState
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.store.QueuedPromptItem
import dev.orbit.android.store.StagedAttachmentItem
import dev.orbit.android.ui.theme.OrbitCompactDropdownMenu
import dev.orbit.android.ui.theme.OrbitMenuOptionRow
import dev.orbit.android.ui.theme.OrbitMenuSectionHeader
import dev.orbit.android.ui.theme.OrbitProviderIcon
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import dev.orbit.android.ui.theme.orbitSemanticFileIcon
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.orbit_ffi_uniffi.FfiHostDetail
import uniffi.orbit_ffi_uniffi.FfiMentionCandidate
import uniffi.orbit_ffi_uniffi.FfiSessionMode
import uniffi.orbit_ffi_uniffi.FfiSlashCommand
import java.io.File

private sealed class AutocompleteTrigger {
    data class Slash(val range: IntRange, val query: String) : AutocompleteTrigger()
    data class Mention(val range: IntRange, val query: String) : AutocompleteTrigger()
}

private val SLASH_TRIGGER_REGEX = Regex("""^\s*(/[^\s/]*)$""")
private val MENTION_TRIGGER_REGEX = Regex("""(?:^|\s)(@[^\s]*)$""")

private fun detectAutocompleteTrigger(
    text: String,
    hasActiveSlashDirective: Boolean,
): AutocompleteTrigger? {
    val atMatch = MENTION_TRIGGER_REGEX.find(text)
    val slashMatch = if (!hasActiveSlashDirective) SLASH_TRIGGER_REGEX.find(text) else null

    val atGroup = atMatch?.groups?.get(1)
    val slashGroup = slashMatch?.groups?.get(1)

    return when {
        atGroup != null && slashGroup != null -> {
            if (atGroup.range.first >= slashGroup.range.first) {
                AutocompleteTrigger.Mention(atGroup.range, atGroup.value.drop(1))
            } else {
                AutocompleteTrigger.Slash(slashGroup.range, slashGroup.value.drop(1))
            }
        }
        atGroup != null -> AutocompleteTrigger.Mention(atGroup.range, atGroup.value.drop(1))
        slashGroup != null -> AutocompleteTrigger.Slash(slashGroup.range, slashGroup.value.drop(1))
        else -> null
    }
}

private fun slashCommandIcon(id: String): ImageVector = when (id) {
    "plan" -> Icons.Outlined.Checklist
    "review" -> Icons.Outlined.FindInPage
    "diff" -> Icons.Outlined.Difference
    "compact" -> Icons.Outlined.ContentCut
    "btw" -> Icons.Outlined.Forum
    "fast" -> Icons.Outlined.Bolt
    "fork" -> Icons.AutoMirrored.Outlined.CallSplit
    "auto" -> Icons.Outlined.AutoAwesome
    "automations" -> Icons.Outlined.Schedule
    "board" -> Icons.Outlined.Layers
    "archived" -> Icons.Outlined.Archive
    "export" -> Icons.Outlined.IosShare
    "supervised", "ask" -> Icons.Outlined.Shield
    "edits" -> Icons.Outlined.VerifiedUser
    "full" -> Icons.Outlined.GppMaybe
    "terminal" -> Icons.Outlined.Terminal
    "new" -> Icons.Outlined.EditNote
    "settings" -> Icons.Outlined.Settings
    else -> Icons.Outlined.Terminal
}

private data class ReadUriResult(
    val filename: String,
    val mimeType: String?,
    val bytes: ByteArray,
)

private fun readAttachmentFromUri(
    context: Context,
    uri: Uri,
    fallbackName: String,
): ReadUriResult? {
    val cr = context.contentResolver
    var displayName: String? = null
    runCatching {
        cr.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                val idx = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (idx >= 0) {
                    displayName = cursor.getString(idx)
                }
            }
        }
    }
    val bytes = runCatching {
        cr.openInputStream(uri)?.use { it.readBytes() }
    }.getOrNull() ?: return null
    val resolvedName = displayName?.takeIf { it.isNotBlank() }
        ?: uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() }
        ?: fallbackName
    val mime = cr.getType(uri)
    return ReadUriResult(filename = resolvedName, mimeType = mime, bytes = bytes)
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
    onVoiceNavigate: (VoiceNavigation) -> Unit,
    onSend: () -> Unit,
    onQueue: () -> Unit,
    onInterrupt: () -> Unit,
    onOpenContextPicker: () -> Unit,
    onOpenModelSelector: () -> Unit,
    onSelectMode: (FfiSessionMode) -> Unit,
    onToggleFastMode: () -> Unit = {},
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
    onAttachImageBytes: (filename: String, mimeType: String?, bytes: ByteArray) -> Unit = { _, _, _ -> },
    onAttachFileBytes: (filename: String, mimeType: String?, bytes: ByteArray) -> Unit = { _, _, _ -> },
    onRemoveStagedAttachment: (String) -> Unit = {},
    onRegisterInlineMention: (token: String, kind: String, path: String?, staged: Boolean, label: String) -> Unit = { _, _, _, _, _ -> },
    slashCommands: List<FfiSlashCommand> = emptyList(),
    onFilterSlashCommands: suspend (String) -> List<FfiSlashCommand> = { emptyList() },
    onRankMentions: suspend (String) -> List<FfiMentionCandidate> = { emptyList() },
    onExecuteSlashCommand: (FfiSlashCommand) -> Unit = {},
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var listening by remember { mutableStateOf(false) }
    var plusMenuExpanded by remember { mutableStateOf(false) }
    var modeMenuExpanded by remember { mutableStateOf(false) }
    var pendingCameraFile by remember { mutableStateOf<File?>(null) }

    val photoPickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.PickVisualMedia(),
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val res = withContext(Dispatchers.IO) {
                readAttachmentFromUri(context, uri, "photo.jpg")
            }
            if (res != null) {
                onAttachImageBytes(res.filename, res.mimeType, res.bytes)
            }
        }
    }

    val cameraLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.TakePicture(),
    ) { saved ->
        val file = pendingCameraFile
        pendingCameraFile = null
        if (!saved || file == null || !file.exists()) {
            file?.delete()
            return@rememberLauncherForActivityResult
        }
        scope.launch {
            val bytes = withContext(Dispatchers.IO) {
                runCatching { file.readBytes() }.also { file.delete() }.getOrNull()
            }
            if (bytes != null && bytes.isNotEmpty()) {
                onAttachImageBytes(file.name, "image/jpeg", bytes)
            }
        }
    }

    val filePickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val res = withContext(Dispatchers.IO) {
                readAttachmentFromUri(context, uri, "attachment.txt")
            }
            if (res != null) {
                val isImage = res.mimeType?.startsWith("image/") == true
                if (isImage) {
                    onAttachImageBytes(res.filename, res.mimeType, res.bytes)
                } else {
                    onAttachFileBytes(res.filename, res.mimeType, res.bytes)
                }
            }
        }
    }

    val trigger = remember(draft.text, draft.activeSlashCommand) {
        detectAutocompleteTrigger(
            text = draft.text,
            hasActiveSlashDirective = draft.activeSlashCommand != null,
        )
    }

    var filteredSlashCommands by remember { mutableStateOf<List<FfiSlashCommand>>(emptyList()) }
    LaunchedEffect(trigger, slashCommands) {
        if (trigger is AutocompleteTrigger.Slash) {
            if (trigger.query.isEmpty()) {
                filteredSlashCommands = slashCommands
            } else {
                delay(40)
                filteredSlashCommands = onFilterSlashCommands(trigger.query)
            }
        } else {
            filteredSlashCommands = emptyList()
        }
    }

    var rankedMentionCandidates by remember { mutableStateOf<List<FfiMentionCandidate>>(emptyList()) }
    LaunchedEffect(trigger, workspaceFiles, activeCwd) {
        if (trigger is AutocompleteTrigger.Mention) {
            if (trigger.query.isNotEmpty()) {
                delay(50)
            }
            rankedMentionCandidates = onRankMentions(trigger.query)
        } else {
            rankedMentionCandidates = emptyList()
        }
    }

    val catalog = remember(hostDetail, draft.provider) {
        OrbitFormatters.catalogFor(hostDetail, draft.provider)
    }
    val modelLabel = remember(catalog, draft.model) {
        val raw = OrbitFormatters.modelLabel(catalog, draft.model)
        if (draft.provider.name == "CLAUDE_CODE") {
            raw.removePrefix("Claude ").trim().ifEmpty { raw }
        } else {
            raw
        }
    }
    val effortTag = draft.effort?.let { OrbitFormatters.compactEffortLabel(it) }
    val normalizedMode = OrbitFormatters.normalizeSessionMode(draft.mode)

    val containerShape = RoundedCornerShape(24.dp)

    Column(
        modifier = modifier
            .fillMaxWidth()
            .background(colors.canvas)
            .navigationBarsPadding()
            .imePadding()
            .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
    ) {
        // Queued Prompts Dock
        if (queuedPrompts.isNotEmpty()) {
            val queueShape = RoundedCornerShape(14.dp)
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(queueShape)
                    .background(colors.elevated, queueShape)
                    .border(OrbitSpacing.hairline, colors.hairline, queueShape)
                    .padding(OrbitSpacing.sm),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                Text(
                    text = "Queued (${queuedPrompts.size})",
                    style = typography.metaSemiBold,
                    color = colors.fgSecondary,
                    modifier = Modifier.padding(horizontal = OrbitSpacing.xs),
                )
                queuedPrompts.forEach { item ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(10.dp))
                            .background(colors.subtleFill, RoundedCornerShape(10.dp))
                            .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    ) {
                        Text(
                            text = item.previewText,
                            style = typography.meta,
                            color = colors.fgPrimary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        Icon(
                            imageVector = Icons.Outlined.Edit,
                            contentDescription = "Edit queued prompt",
                            tint = colors.fgSecondary,
                            modifier = Modifier
                                .size(22.dp)
                                .clip(CircleShape)
                                .clickable { onEditQueuedPrompt(item.id) }
                                .padding(3.dp),
                        )
                        Icon(
                            imageVector = Icons.Outlined.PlayArrow,
                            contentDescription = "Send queued prompt now",
                            tint = colors.primary,
                            modifier = Modifier
                                .size(22.dp)
                                .clip(CircleShape)
                                .clickable { onSendQueuedPromptNow(item.id) }
                                .padding(3.dp),
                        )
                        Icon(
                            imageVector = Icons.Outlined.DeleteOutline,
                            contentDescription = "Remove queued prompt",
                            tint = colors.fgMuted,
                            modifier = Modifier
                                .size(22.dp)
                                .clip(CircleShape)
                                .clickable { onRemoveQueuedPrompt(item.id) }
                                .padding(3.dp),
                        )
                    }
                }
            }
        }

        // Native Floating Autocomplete Panel (`/` commands or `@` context & files)
        val hasAutocompleteItems = when (trigger) {
            is AutocompleteTrigger.Slash -> filteredSlashCommands.isNotEmpty()
            is AutocompleteTrigger.Mention -> true
            null -> false
        }

        AnimatedVisibility(
            visible = trigger != null && hasAutocompleteItems,
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            val panelShape = RoundedCornerShape(14.dp)
            Surface(
                shape = panelShape,
                color = colors.elevated,
                tonalElevation = 2.dp,
                shadowElevation = 4.dp,
                modifier = Modifier
                    .fillMaxWidth()
                    .border(OrbitSpacing.hairline, colors.hairline, panelShape),
            ) {
                LazyColumn(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(max = 240.dp),
                    contentPadding = PaddingValues(vertical = OrbitSpacing.xs),
                ) {
                    when (trigger) {
                        is AutocompleteTrigger.Slash -> {
                            items(
                                items = filteredSlashCommands,
                                key = { "slash:${it.id}" },
                            ) { cmd ->
                                Row(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .clickable {
                                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                            val updated = draft.text
                                                .removeRange(trigger.range)
                                                .trimStart()
                                            onTextChange(updated)
                                            onExecuteSlashCommand(cmd)
                                        }
                                        .padding(horizontal = OrbitSpacing.md, vertical = 9.dp),
                                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Icon(
                                        imageVector = slashCommandIcon(cmd.id),
                                        contentDescription = null,
                                        tint = colors.fgSecondary,
                                        modifier = Modifier.size(OrbitSpacing.iconSm),
                                    )
                                    Text(
                                        text = cmd.command,
                                        style = typography.codeMedium,
                                        color = colors.fgPrimary,
                                    )
                                    Text(
                                        text = cmd.description,
                                        style = typography.meta,
                                        color = colors.fgMuted,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                        modifier = Modifier.weight(1f),
                                    )
                                }
                            }
                        }

                        is AutocompleteTrigger.Mention -> {
                            items(
                                items = rankedMentionCandidates,
                                key = { it.key },
                            ) { candidate ->
                                val icon = when (candidate.kind) {
                                    "git_diff", "staged" -> Icons.Outlined.Difference
                                    "terminal" -> Icons.Outlined.Terminal
                                    else -> orbitSemanticFileIcon(candidate.title)
                                }
                                val displayTitle = if (candidate.kind == "file") {
                                    candidate.title
                                } else {
                                    candidate.tokenLabel
                                }
                                val displaySubtitle = if (candidate.kind == "file") {
                                    if (candidate.gitStatus.isNotEmpty()) {
                                        if (candidate.subtitle.isNotEmpty()) "${candidate.subtitle} · ${candidate.gitStatus}" else candidate.gitStatus
                                    } else {
                                        candidate.subtitle
                                    }
                                } else {
                                    candidate.subtitle
                                }
                                MentionAutocompleteRow(
                                    icon = icon,
                                    title = displayTitle,
                                    subtitle = displaySubtitle,
                                    onClick = {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        val token = candidate.tokenLabel.ifEmpty { "@${candidate.title}" }
                                        val replaced = draft.text.replaceRange(trigger.range, "$token ")
                                        onTextChange(replaced)
                                        when (candidate.kind) {
                                            "git_diff" -> onRegisterInlineMention(
                                                token,
                                                "git_diff",
                                                null,
                                                false,
                                                candidate.title.ifEmpty { "Working Tree Diff" },
                                            )
                                            "staged" -> onRegisterInlineMention(
                                                token,
                                                "git_diff",
                                                null,
                                                true,
                                                candidate.title.ifEmpty { "Staged Diff" },
                                            )
                                            "terminal" -> onRegisterInlineMention(
                                                token,
                                                "terminal",
                                                null,
                                                false,
                                                candidate.title.ifEmpty { "Terminal" },
                                            )
                                            else -> {
                                                val rawPath = candidate.filePath.ifEmpty { candidate.key }
                                                onRegisterInlineMention(token, "file", rawPath, false, candidate.title)
                                            }
                                        }
                                    },
                                )
                            }
                            if (rankedMentionCandidates.isEmpty()) {
                                item(key = "mention:empty") {
                                    Text(
                                        text = "No matching workspace files",
                                        style = typography.meta,
                                        color = colors.fgMuted,
                                        modifier = Modifier.padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                                    )
                                }
                            }
                        }

                        null -> Unit
                    }
                }
            }
        }

        // Main Composer Surface Card
        Surface(
            shape = containerShape,
            color = colors.elevated,
            tonalElevation = 1.dp,
            shadowElevation = 2.dp,
            modifier = Modifier
                .fillMaxWidth()
                .border(OrbitSpacing.hairline, colors.hairline, containerShape),
        ) {
            Column(
                modifier = Modifier.fillMaxWidth(),
            ) {
                // Non-slash Context & Staged Attachment Strip above input
                val showContextStrip = draft.stagedAttachments.isNotEmpty() ||
                    draft.isStagingAttachment ||
                    draft.mentionedFiles.isNotEmpty() ||
                    draft.includeGitDiff ||
                    draft.includeStagedDiff ||
                    draft.includeTerminal ||
                    draft.quotedSnippets.isNotEmpty()

                if (showContextStrip) {
                    LazyRow(
                        modifier = Modifier.padding(
                            start = OrbitSpacing.md,
                            end = OrbitSpacing.md,
                            top = OrbitSpacing.sm,
                        ),
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (draft.isStagingAttachment) {
                            item(key = "staging_spinner") {
                                Box(
                                    modifier = Modifier
                                        .size(32.dp)
                                        .clip(RoundedCornerShape(8.dp))
                                        .background(colors.subtleFill, RoundedCornerShape(8.dp)),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    CircularProgressIndicator(
                                        modifier = Modifier.size(14.dp),
                                        strokeWidth = 2.dp,
                                        color = colors.fgSecondary,
                                    )
                                }
                            }
                        }

                        items(
                            items = draft.stagedAttachments,
                            key = { "staged:${it.id}" },
                        ) { att ->
                            if (att.isImage && att.previewBytes != null) {
                                StagedImageThumbnailChip(
                                    item = att,
                                    onRemove = { onRemoveStagedAttachment(att.id) },
                                )
                            } else {
                                RemovableComposerChip(
                                    icon = orbitSemanticFileIcon(att.filename),
                                    label = att.filename,
                                    onRemove = { onRemoveStagedAttachment(att.id) },
                                )
                            }
                        }

                        items(
                            items = draft.mentionedFiles,
                            key = { "ref:$it" },
                        ) { path ->
                            val short = OrbitFormatters.shortPath(path)
                            RemovableComposerChip(
                                icon = orbitSemanticFileIcon(short),
                                label = short,
                                onRemove = { onRemoveFileMention(path) },
                            )
                        }

                        if (draft.includeGitDiff) {
                            item(key = "ctx:diff") {
                                RemovableComposerChip(
                                    icon = Icons.Outlined.Difference,
                                    label = "Working Diff",
                                    onRemove = onToggleGitDiff,
                                )
                            }
                        }

                        if (draft.includeStagedDiff) {
                            item(key = "ctx:staged") {
                                RemovableComposerChip(
                                    icon = Icons.Outlined.Difference,
                                    label = "Staged Diff",
                                    onRemove = onToggleStagedDiff,
                                )
                            }
                        }

                        if (draft.includeTerminal) {
                            item(key = "ctx:term") {
                                RemovableComposerChip(
                                    icon = Icons.Outlined.Terminal,
                                    label = "Terminal",
                                    onRemove = onToggleTerminal,
                                )
                            }
                        }

                        items(
                            items = draft.quotedSnippets,
                            key = { "quote:${it.id}" },
                        ) { snippet ->
                            RemovableComposerChip(
                                icon = Icons.Outlined.FormatQuote,
                                label = snippet.badgeLabel,
                                onRemove = { onRemoveQuotedSnippet(snippet.id) },
                            )
                        }
                    }
                }

                // Inline Input Row (`/` directive prefix lives INSIDE the input row!)
                val placeholderText = "Ask anything, @ to mention, / for actions"

                if (listening) {
                    Text(
                        text = androidx.compose.ui.res.stringResource(dev.orbit.android.R.string.composer_voice_listening),
                        style = typography.meta,
                        color = colors.fgSecondary,
                        modifier = Modifier.padding(horizontal = OrbitSpacing.lg, vertical = 2.dp),
                    )
                }
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 34.dp, max = 132.dp)
                        .padding(
                            start = OrbitSpacing.lg,
                            end = OrbitSpacing.lg,
                            top = if (showContextStrip) 6.dp else 10.dp,
                            bottom = 2.dp,
                        ),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    if (!draft.activeSlashCommand.isNullOrBlank()) {
                        val slashChipShape = RoundedCornerShape(8.dp)
                        Row(
                            modifier = Modifier
                                .clip(slashChipShape)
                                .background(colors.subtleFill, slashChipShape)
                                .border(OrbitSpacing.hairline, colors.hairline, slashChipShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onClearSlash()
                                }
                                .padding(horizontal = OrbitSpacing.sm, vertical = 3.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(4.dp),
                        ) {
                            Text(
                                text = draft.activeSlashCommand,
                                style = typography.codeMedium,
                                color = colors.fgPrimary,
                            )
                        }
                    }

                    Box(
                        modifier = Modifier.weight(1f),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        if (draft.activeSlashCommand.isNullOrBlank() && draft.text.isEmpty()) {
                            Text(
                                text = placeholderText,
                                style = typography.body,
                                color = colors.fgMuted.copy(alpha = 0.6f),
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
                            maxLines = 5,
                            keyboardOptions = KeyboardOptions(
                                capitalization = KeyboardCapitalization.Sentences,
                                imeAction = ImeAction.Default,
                            ),
                            keyboardActions = KeyboardActions(
                                onSend = {
                                    if (draft.canSend) {
                                        if (isAgentRunning) onQueue() else onSend()
                                    }
                                }
                            ),
                            modifier = Modifier
                                .fillMaxWidth()
                                .onPreviewKeyEvent { event ->
                                    if (event.type == KeyEventType.KeyDown &&
                                        event.key == Key.Backspace &&
                                        draft.text.isEmpty() &&
                                        draft.activeSlashCommand != null
                                    ) {
                                        onClearSlash()
                                        true
                                    } else {
                                        false
                                    }
                                },
                        )
                    }
                }

                // Bottom Toolbar (crisp 32-34.dp visual controls centered inside 48.dp touch hitboxes)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(start = 4.dp, end = 4.dp, bottom = 2.dp)
                        .height(48.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    // Left Cluster: `+` Attachment Menu, Model Pill, Fast Pill, Permission Shield
                    Row(
                        modifier = Modifier.weight(1f).horizontalScroll(rememberScrollState()),
                        horizontalArrangement = Arrangement.spacedBy(2.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        // `+` Attachment Menu Button (ONLY Photos, Camera, Files)
                        Box {
                            Box(
                                modifier = Modifier
                                    .minimumInteractiveComponentSize()
                                    .size(48.dp)
                                    .clip(CircleShape)
                                    .clickable(enabled = hasHost) {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        plusMenuExpanded = true
                                    },
                                contentAlignment = Alignment.Center,
                            ) {
                                Box(
                                    modifier = Modifier
                                        .size(34.dp)
                                        .clip(CircleShape)
                                        .background(colors.subtleFill, CircleShape)
                                        .border(OrbitSpacing.hairline, colors.hairline, CircleShape),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Icon(
                                        imageVector = Icons.Outlined.Add,
                                        contentDescription = "Add attachment",
                                        tint = if (hasHost) colors.fgPrimary else colors.fgMuted,
                                        modifier = Modifier.size(OrbitSpacing.iconMd),
                                    )
                                }
                            }

                            DropdownMenu(
                                expanded = plusMenuExpanded,
                                onDismissRequest = { plusMenuExpanded = false },
                                containerColor = colors.elevated,
                            ) {
                                DropdownMenuItem(
                                    text = {
                                        Text(
                                            text = "Photos",
                                            style = typography.subheadMedium,
                                            color = colors.fgPrimary,
                                        )
                                    },
                                    leadingIcon = {
                                        Icon(
                                            imageVector = Icons.Outlined.PhotoLibrary,
                                            contentDescription = null,
                                            tint = colors.fgSecondary,
                                            modifier = Modifier.size(OrbitSpacing.iconSm),
                                        )
                                    },
                                    onClick = {
                                        plusMenuExpanded = false
                                        photoPickerLauncher.launch(
                                            PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)
                                        )
                                    },
                                )
                                DropdownMenuItem(
                                    text = {
                                        Text(
                                            text = "Camera",
                                            style = typography.subheadMedium,
                                            color = colors.fgPrimary,
                                        )
                                    },
                                    leadingIcon = {
                                        Icon(
                                            imageVector = Icons.Outlined.PhotoCamera,
                                            contentDescription = null,
                                            tint = colors.fgSecondary,
                                            modifier = Modifier.size(OrbitSpacing.iconSm),
                                        )
                                    },
                                    onClick = {
                                        plusMenuExpanded = false
                                        runCatching {
                                            val camDir = File(context.cacheDir, "orbit_camera").apply { mkdirs() }
                                            val outFile = File(camDir, "orbit_cam_${System.currentTimeMillis()}.jpg")
                                            val uri = FileProvider.getUriForFile(
                                                context,
                                                "${context.packageName}.fileprovider",
                                                outFile,
                                            )
                                            pendingCameraFile = outFile
                                            cameraLauncher.launch(uri)
                                        }
                                    },
                                )
                                DropdownMenuItem(
                                    text = {
                                        Text(
                                            text = "Files",
                                            style = typography.subheadMedium,
                                            color = colors.fgPrimary,
                                        )
                                    },
                                    leadingIcon = {
                                        Icon(
                                            imageVector = Icons.Outlined.AttachFile,
                                            contentDescription = null,
                                            tint = colors.fgSecondary,
                                            modifier = Modifier.size(OrbitSpacing.iconSm),
                                        )
                                    },
                                    onClick = {
                                        plusMenuExpanded = false
                                        filePickerLauncher.launch(arrayOf("*/*"))
                                    },
                                )
                            }
                        }

                        // Model Pill with OrbitProviderIcon (34.dp visual pill inside 48.dp touch target)
                        val pillShape = RoundedCornerShape(10.dp)
                        Box(
                            modifier = Modifier
                                .minimumInteractiveComponentSize()
                                .height(48.dp)
                                .clip(pillShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onOpenModelSelector()
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            Row(
                                modifier = Modifier
                                    .height(34.dp)
                                    .widthIn(min = 48.dp, max = 224.dp)
                                    .clip(pillShape)
                                    .background(colors.subtleFill, pillShape)
                                    .border(OrbitSpacing.hairline, colors.hairline, pillShape)
                                    .padding(horizontal = OrbitSpacing.sm),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(5.dp),
                            ) {
                                OrbitProviderIcon(
                                    provider = draft.provider,
                                    size = 14.dp,
                                )
                                Text(
                                    text = modelLabel,
                                    style = typography.metaMedium,
                                    color = colors.fgPrimary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier.weight(1f, fill = false),
                                )
                                if (effortTag != null) {
                                    Text(
                                        text = effortTag,
                                        style = typography.meta,
                                        color = colors.fgMuted,
                                        maxLines = 1,
                                    )
                                }
                                Icon(
                                    imageVector = Icons.Outlined.KeyboardArrowDown,
                                    contentDescription = null,
                                    tint = colors.fgMuted,
                                    modifier = Modifier.size(14.dp),
                                )
                            }
                        }

                        // Fast Mode Toggle Pill (32.dp visual pill inside 48.dp touch target)
                        val fastPillShape = RoundedCornerShape(10.dp)
                        Box(
                            modifier = Modifier
                                .minimumInteractiveComponentSize()
                                .height(48.dp)
                                .widthIn(min = 48.dp)
                                .clip(fastPillShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onToggleFastMode()
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            Row(
                                modifier = Modifier
                                    .height(32.dp)
                                    .clip(fastPillShape)
                                    .background(
                                        if (draft.fastMode) colors.warning.copy(alpha = 0.16f) else Color.Transparent,
                                        fastPillShape,
                                    )
                                    .border(
                                        OrbitSpacing.hairline,
                                        if (draft.fastMode) colors.warning.copy(alpha = 0.45f) else Color.Transparent,
                                        fastPillShape,
                                    )
                                    .padding(horizontal = if (draft.fastMode) 8.dp else 7.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(2.dp),
                            ) {
                                Icon(
                                    imageVector = Icons.Outlined.Bolt,
                                    contentDescription = if (draft.fastMode) "Fast mode on" else "Fast mode off",
                                    tint = if (draft.fastMode) colors.warning else colors.fgMuted,
                                    modifier = Modifier.size(OrbitSpacing.iconSm),
                                )
                                if (draft.fastMode) {
                                    Text(
                                        text = "Fast",
                                        style = typography.metaSemiBold,
                                        color = colors.warning,
                                        maxLines = 1,
                                    )
                                }
                            }
                        }

                        // Compact Access Permissions Shield Button + Unified Menu (32.dp circle inside 48.dp touch target)
                        Box {
                            val modeIcon = when (normalizedMode) {
                                FfiSessionMode.Auto -> Icons.Outlined.AutoAwesome
                                FfiSessionMode.AcceptEdits -> Icons.Outlined.VerifiedUser
                                FfiSessionMode.FullAccess -> Icons.Outlined.GppMaybe
                                else -> Icons.Outlined.Shield
                            }
                            Box(
                                modifier = Modifier
                                    .minimumInteractiveComponentSize()
                                    .size(48.dp)
                                    .clip(CircleShape)
                                    .clickable {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        modeMenuExpanded = true
                                    },
                                contentAlignment = Alignment.Center,
                            ) {
                                Box(
                                    modifier = Modifier
                                        .size(32.dp)
                                        .clip(CircleShape),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Icon(
                                        imageVector = modeIcon,
                                        contentDescription = "Access Permissions: ${OrbitFormatters.sessionModeLabel(normalizedMode)}",
                                        tint = if (normalizedMode == FfiSessionMode.Auto) colors.relay else colors.fgSecondary,
                                        modifier = Modifier.size(OrbitSpacing.iconSm),
                                    )
                                }
                            }

                            OrbitCompactDropdownMenu(
                                expanded = modeMenuExpanded,
                                onDismissRequest = { modeMenuExpanded = false },
                                width = 296.dp,
                            ) {
                                OrbitMenuSectionHeader(title = "Access Permissions")
                                OrbitMenuOptionRow(
                                    title = "Always Ask",
                                    subtitle = "Require approval before file edits and commands",
                                    icon = Icons.Outlined.Shield,
                                    selected = normalizedMode == FfiSessionMode.Default,
                                    onClick = {
                                        modeMenuExpanded = false
                                        onSelectMode(FfiSessionMode.Default)
                                    },
                                )
                                OrbitMenuOptionRow(
                                    title = "Auto (Guardian)",
                                    subtitle = "Auto-approve safe actions, prompt on risky ones",
                                    icon = Icons.Outlined.AutoAwesome,
                                    selected = normalizedMode == FfiSessionMode.Auto,
                                    onClick = {
                                        modeMenuExpanded = false
                                        onSelectMode(FfiSessionMode.Auto)
                                    },
                                )
                                OrbitMenuOptionRow(
                                    title = "Accept Edits",
                                    subtitle = "Auto-approve file edits, ask before commands",
                                    icon = Icons.Outlined.VerifiedUser,
                                    selected = normalizedMode == FfiSessionMode.AcceptEdits,
                                    onClick = {
                                        modeMenuExpanded = false
                                        onSelectMode(FfiSessionMode.AcceptEdits)
                                    },
                                )
                                OrbitMenuOptionRow(
                                    title = "Full Access",
                                    subtitle = "Edit files and run commands without prompts",
                                    icon = Icons.Outlined.GppMaybe,
                                    selected = normalizedMode == FfiSessionMode.FullAccess,
                                    onClick = {
                                        modeMenuExpanded = false
                                        onSelectMode(FfiSessionMode.FullAccess)
                                    },
                                )
                            }
                        }
                    }
                    ComposerAction(
                        hasDraft = draft.canSend,
                        running = isAgentRunning,
                        enabled = hasHost,
                        onDictated = { onTextChange(listOf(draft.text, it).filter(String::isNotBlank).joinToString(" ")) },
                        onListening = { listening = it },
                        onSubmit = if (isAgentRunning) onQueue else onSend,
                        onInterrupt = onInterrupt,
                        onNavigate = onVoiceNavigate,
                    )
                }
            }
        }
    }
}

@Composable
private fun MentionAutocompleteRow(
    icon: ImageVector,
    title: String,
    subtitle: String,
    onClick: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = OrbitSpacing.md, vertical = 9.dp),
        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            tint = colors.fgSecondary,
            modifier = Modifier.size(OrbitSpacing.iconSm),
        )
        Text(
            text = title,
            style = typography.codeMedium,
            color = colors.fgPrimary,
            maxLines = 1,
        )
        Text(
            text = subtitle,
            style = typography.meta,
            color = colors.fgMuted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun StagedImageThumbnailChip(
    item: StagedAttachmentItem,
    onRemove: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val bitmap = remember(item.id, item.previewBytes) {
        item.previewBytes?.let { bytes ->
            runCatching { BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap() }.getOrNull()
        }
    }
    val thumbShape = RoundedCornerShape(10.dp)
    Box(modifier = Modifier.size(48.dp)) {
        if (bitmap != null) {
            Image(
                bitmap = bitmap,
                contentDescription = item.filename,
                contentScale = ContentScale.Crop,
                modifier = Modifier
                    .fillMaxSize()
                    .clip(thumbShape)
                    .border(OrbitSpacing.hairline, colors.hairline, thumbShape),
            )
        } else {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .clip(thumbShape)
                    .background(colors.subtleFill, thumbShape)
                    .border(OrbitSpacing.hairline, colors.hairline, thumbShape),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.PhotoLibrary,
                    contentDescription = item.filename,
                    tint = colors.fgSecondary,
                    modifier = Modifier.size(OrbitSpacing.iconMd),
                )
            }
        }
        Box(
            modifier = Modifier
                .align(Alignment.TopEnd)
                .padding(2.dp)
                .size(16.dp)
                .clip(CircleShape)
                .background(Color.Black.copy(alpha = 0.68f), CircleShape)
                .clickable(onClick = onRemove),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = Icons.Outlined.Close,
                contentDescription = "Remove ${item.filename}",
                tint = Color.White,
                modifier = Modifier.size(11.dp),
            )
        }
    }
}

@Composable
private fun RemovableComposerChip(
    icon: ImageVector,
    label: String,
    onRemove: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val shape = RoundedCornerShape(8.dp)
    Row(
        modifier = Modifier
            .heightIn(min = 28.dp)
            .clip(shape)
            .background(colors.subtleFill, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape)
            .padding(start = OrbitSpacing.sm, end = 6.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            tint = colors.fgSecondary,
            modifier = Modifier.size(13.dp),
        )
        Text(
            text = label,
            style = typography.metaMedium,
            color = colors.fgPrimary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.widthIn(max = 140.dp),
        )
        Spacer(modifier = Modifier.size(1.dp))
        Icon(
            imageVector = Icons.Outlined.Close,
            contentDescription = "Remove $label",
            tint = colors.fgMuted,
            modifier = Modifier
                .size(14.dp)
                .clip(CircleShape)
                .clickable(onClick = onRemove),
        )
    }
}
