package dev.orbit.android.ui.composer

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Code
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Terminal
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
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme

private data class SlashWorkflowSpec(
    val command: String,
    val description: String,
)

private val BuiltinSlashWorkflows = listOf(
    SlashWorkflowSpec("/plan", "Draft step-by-step plan first"),
    SlashWorkflowSpec("/review", "Audit working tree changes"),
    SlashWorkflowSpec("/compact", "Compress session context"),
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContextMentionSheet(
    workspaceFiles: List<String>,
    mentionedFiles: List<String>,
    includeGitDiff: Boolean,
    includeStagedDiff: Boolean,
    includeTerminal: Boolean,
    activeSlashCommand: String?,
    onRequestRefreshFiles: () -> Unit,
    onAddFileMention: (String) -> Unit,
    onToggleGitDiff: () -> Unit,
    onToggleStagedDiff: () -> Unit,
    onToggleTerminal: () -> Unit,
    onSelectSlash: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    // Keep ContextMentionSheet compact/adaptive
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false)
    var query by remember { mutableStateOf("") }

    LaunchedEffect(Unit) {
        onRequestRefreshFiles()
    }

    val filteredFiles = remember(workspaceFiles, query) {
        val q = query.trim().removePrefix("@").lowercase()
        if (q.isEmpty()) {
            workspaceFiles.take(40)
        } else {
            workspaceFiles.filter { it.lowercase().contains(q) }.take(60)
        }
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
                .navigationBarsPadding()
                .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
        ) {
            Text(
                text = "Context & Workflows",
                style = typography.title,
                color = colors.fgPrimary,
            )

            // Fuzzy search bar
            val searchShape = RoundedCornerShape(OrbitSpacing.md)
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 44.dp)
                    .background(colors.code, searchShape)
                    .border(OrbitSpacing.hairline, colors.hairline, searchShape)
                    .padding(horizontal = OrbitSpacing.md),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Icon(
                    imageVector = Icons.Outlined.Search,
                    contentDescription = null,
                    tint = colors.fgMuted,
                    modifier = Modifier.size(OrbitSpacing.iconMd),
                )
                Box(modifier = Modifier.weight(1f)) {
                    if (query.isEmpty()) {
                        Text(
                            text = "Filter @files or /workflows...",
                            style = typography.body,
                            color = colors.fgMuted,
                        )
                    }
                    BasicTextField(
                        value = query,
                        onValueChange = { query = it },
                        textStyle = typography.body.copy(color = colors.fgPrimary),
                        cursorBrush = SolidColor(colors.primary),
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
            }

            LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(max = 360.dp),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                // @diff / @staged / @terminal quick attachments
                item(key = "git_diff_toggle") {
                    val rowShape = RoundedCornerShape(OrbitSpacing.md)
                    Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs)) {
                        listOf(
                            Triple("@diff", "Include uncommitted working tree diff", includeGitDiff to onToggleGitDiff),
                            Triple("@staged", "Include staged git index changes", includeStagedDiff to onToggleStagedDiff),
                            Triple("@terminal", "Include active PTY terminal output", includeTerminal to onToggleTerminal),
                        ).forEach { (tag, subtitle, stateAndAction) ->
                            val (isIncluded, onToggle) = stateAndAction
                            val isTerm = tag == "@terminal"
                            val accentColor = if (isTerm) colors.warning else colors.directAdded
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .heightIn(min = OrbitSpacing.touchTarget)
                                    .clip(rowShape)
                                    .background(if (isIncluded) colors.subtleFill else Color.Transparent, rowShape)
                                    .border(OrbitSpacing.hairline, colors.hairline, rowShape)
                                    .clickable {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        onToggle()
                                        onDismiss()
                                    }
                                    .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                            ) {
                                Icon(
                                    imageVector = if (isTerm) Icons.Outlined.Terminal else Icons.Outlined.Code,
                                    contentDescription = null,
                                    tint = accentColor,
                                    modifier = Modifier.size(OrbitSpacing.iconMd),
                                )
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(
                                        text = tag,
                                        style = typography.codeMedium,
                                        color = colors.fgPrimary,
                                    )
                                    Text(
                                        text = subtitle,
                                        style = typography.meta,
                                        color = colors.fgSecondary,
                                        maxLines = 1,
                                    )
                                }
                                if (isIncluded) {
                                    Icon(
                                        imageVector = Icons.Outlined.Check,
                                        contentDescription = "Included",
                                        tint = accentColor,
                                        modifier = Modifier.size(OrbitSpacing.iconMd),
                                    )
                                }
                            }
                        }
                    }
                }

                // Slash workflows
                item(key = "slash_header") {
                    Spacer(modifier = Modifier.height(OrbitSpacing.xs))
                    Text(
                        text = "WORKFLOWS",
                        style = typography.metaSemiBold,
                        color = colors.fgMuted,
                        modifier = Modifier.padding(vertical = OrbitSpacing.xs),
                    )
                }

                items(BuiltinSlashWorkflows, key = { it.command }) { wf ->
                    val isSelected = activeSlashCommand == wf.command
                    val rowShape = RoundedCornerShape(OrbitSpacing.md)
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(min = OrbitSpacing.touchTarget)
                            .clip(rowShape)
                            .background(if (isSelected) colors.subtleFill else Color.Transparent, rowShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onSelectSlash(wf.command)
                                onDismiss()
                            }
                            .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Terminal,
                            contentDescription = null,
                            tint = colors.primary,
                            modifier = Modifier.size(OrbitSpacing.iconMd),
                        )
                        Column(modifier = Modifier.weight(1f)) {
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
                                softWrap = false,
                            )
                        }
                        if (isSelected) {
                            Icon(
                                imageVector = Icons.Outlined.Check,
                                contentDescription = "Selected",
                                tint = colors.primary,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                        }
                    }
                }

                // Workspace @files
                item(key = "files_header") {
                    HorizontalDivider(
                        thickness = OrbitSpacing.hairline,
                        color = colors.hairline,
                        modifier = Modifier.padding(vertical = OrbitSpacing.xs),
                    )
                    Text(
                        text = "WORKSPACE FILES (${workspaceFiles.size})",
                        style = typography.metaSemiBold,
                        color = colors.fgMuted,
                        modifier = Modifier.padding(vertical = OrbitSpacing.xs),
                    )
                }

                if (filteredFiles.isEmpty()) {
                    item(key = "empty_files") {
                        Text(
                            text = if (workspaceFiles.isEmpty()) "No indexed files in current directory." else "No matching files.",
                            style = typography.subhead,
                            color = colors.fgMuted,
                            modifier = Modifier.padding(vertical = OrbitSpacing.sm),
                        )
                    }
                } else {
                    items(filteredFiles, key = { it }) { path ->
                        val added = path in mentionedFiles
                        val rowShape = RoundedCornerShape(OrbitSpacing.sm)
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 44.dp)
                                .clip(rowShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onAddFileMention(path)
                                    onDismiss()
                                }
                                .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Description,
                                contentDescription = null,
                                tint = colors.fgMuted,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                            Text(
                                text = path,
                                style = typography.code,
                                color = if (added) colors.primary else colors.fgPrimary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            if (added) {
                                Icon(
                                    imageVector = Icons.Outlined.Check,
                                    contentDescription = "Referenced",
                                    tint = colors.primary,
                                    modifier = Modifier.size(OrbitSpacing.iconSm),
                                )
                            }
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(OrbitSpacing.xs))
        }
    }
}
