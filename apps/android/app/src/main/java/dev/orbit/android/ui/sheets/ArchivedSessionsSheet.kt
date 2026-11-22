package dev.orbit.android.ui.sheets

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
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Archive
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.DeleteOutline
import androidx.compose.material.icons.outlined.Restore
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitProviderIcon
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiAgentCard

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ArchivedSessionsSheet(
    archivedAgents: List<FfiAgentCard>,
    onUnarchive: (String) -> Unit,
    onDeletePermanently: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val groupedBg = if (colors.isDark) colors.canvas else colors.drawer

    var searchQuery by remember { mutableStateOf("") }
    var confirmDeleteAgent by remember { mutableStateOf<FfiAgentCard?>(null) }

    val filtered = remember(archivedAgents, searchQuery) {
        val q = searchQuery.trim().lowercase()
        if (q.isEmpty()) {
            archivedAgents
        } else {
            archivedAgents.filter { ag ->
                ag.title.lowercase().contains(q) ||
                    ag.cwd.lowercase().contains(q) ||
                    (ag.handle?.lowercase()?.contains(q) == true)
            }
        }
    }

    confirmDeleteAgent?.let { agent ->
        AlertDialog(
            onDismissRequest = { confirmDeleteAgent = null },
            containerColor = colors.elevated,
            titleContentColor = colors.fgPrimary,
            textContentColor = colors.fgSecondary,
            title = { Text("Delete permanently?", style = typography.title) },
            text = {
                Text(
                    text = "This permanently removes \"${agent.title.ifBlank { "Untitled Session" }}\" from the host daemon and cannot be undone.",
                    style = typography.subhead,
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        onDeletePermanently(agent.agentId)
                        confirmDeleteAgent = null
                    },
                ) {
                    Text("Delete Forever", color = colors.dangerRemoved, style = typography.subheadSemiBold)
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmDeleteAgent = null }) {
                    Text("Cancel", color = colors.fgSecondary, style = typography.subheadMedium)
                }
            },
        )
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = groupedBg,
        contentColor = colors.fgPrimary,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.84f)
                .navigationBarsPadding(),
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg)
                    .padding(bottom = OrbitSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Archived Sessions",
                    style = typography.title,
                    color = colors.fgPrimary,
                    modifier = Modifier.weight(1f),
                )
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

            // Search bar
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs)
                    .heightIn(min = 40.dp)
                    .clip(CircleShape)
                    .background(colors.subtleFill, CircleShape)
                    .padding(horizontal = OrbitSpacing.md),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Icon(
                    imageVector = Icons.Outlined.Search,
                    contentDescription = null,
                    tint = colors.fgMuted,
                    modifier = Modifier.size(16.dp),
                )
                Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                    if (searchQuery.isEmpty()) {
                        Text(
                            text = "Search archived sessions…",
                            style = typography.subhead,
                            color = colors.fgMuted,
                        )
                    }
                    BasicTextField(
                        value = searchQuery,
                        onValueChange = { searchQuery = it },
                        singleLine = true,
                        textStyle = typography.subhead.copy(color = colors.fgPrimary),
                        cursorBrush = SolidColor(colors.primary),
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
                if (searchQuery.isNotEmpty()) {
                    Icon(
                        imageVector = Icons.Outlined.Close,
                        contentDescription = "Clear search",
                        tint = colors.fgMuted,
                        modifier = Modifier
                            .size(16.dp)
                            .clickable { searchQuery = "" },
                    )
                }
            }

            HorizontalDivider(
                thickness = OrbitSpacing.hairline,
                color = colors.hairline,
                modifier = Modifier.padding(top = OrbitSpacing.sm),
            )

            if (filtered.isEmpty()) {
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth()
                        .padding(OrbitSpacing.xl),
                    contentAlignment = Alignment.Center,
                ) {
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Archive,
                            contentDescription = null,
                            tint = colors.fgMuted,
                            modifier = Modifier.size(28.dp),
                        )
                        Text(
                            text = if (searchQuery.isEmpty()) "No Archived Sessions" else "No Matching Sessions",
                            style = typography.subheadMedium,
                            color = colors.fgSecondary,
                        )
                    }
                }
            } else {
                LazyColumn(
                    modifier = Modifier.weight(1f),
                    contentPadding = PaddingValues(OrbitSpacing.lg),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    items(filtered, key = { it.agentId }) { agent ->
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
                                OrbitProviderIcon(
                                    provider = agent.provider,
                                    size = 15.dp,
                                    modifier = Modifier.padding(top = 2.dp),
                                )
                                Column(
                                    modifier = Modifier.weight(1f),
                                    verticalArrangement = Arrangement.spacedBy(2.dp),
                                ) {
                                    Text(
                                        text = agent.title.ifBlank { "Untitled Session" },
                                        style = typography.subheadSemiBold,
                                        color = colors.fgPrimary,
                                        maxLines = 2,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    val timeLabel = OrbitFormatters.compactRelativeTime(
                                        agent.archivedAtRfc3339 ?: agent.updatedAtRfc3339
                                    )
                                    Text(
                                        text = listOf(OrbitFormatters.shortPath(agent.cwd), timeLabel)
                                            .filter { it.isNotBlank() }
                                            .joinToString(" · "),
                                        style = typography.meta,
                                        color = colors.fgMuted,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                            }

                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Row(
                                    modifier = Modifier
                                        .height(30.dp)
                                        .clip(CircleShape)
                                        .background(colors.primary, CircleShape)
                                        .clickable {
                                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                            onUnarchive(agent.agentId)
                                            onDismiss()
                                        }
                                        .padding(horizontal = 12.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                                ) {
                                    Icon(
                                        imageVector = Icons.Outlined.Restore,
                                        contentDescription = null,
                                        tint = colors.onPrimary,
                                        modifier = Modifier.size(14.dp),
                                    )
                                    Text(
                                        text = "Restore",
                                        style = typography.metaSemiBold,
                                        color = colors.onPrimary,
                                    )
                                }

                                Spacer(modifier = Modifier.weight(1f))

                                Row(
                                    modifier = Modifier
                                        .height(30.dp)
                                        .clip(CircleShape)
                                        .background(colors.dangerRemoved.copy(alpha = 0.10f), CircleShape)
                                        .clickable { confirmDeleteAgent = agent }
                                        .padding(horizontal = 10.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                                ) {
                                    Icon(
                                        imageVector = Icons.Outlined.DeleteOutline,
                                        contentDescription = null,
                                        tint = colors.dangerRemoved,
                                        modifier = Modifier.size(14.dp),
                                    )
                                    Text(
                                        text = "Delete",
                                        style = typography.metaMedium,
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
}
