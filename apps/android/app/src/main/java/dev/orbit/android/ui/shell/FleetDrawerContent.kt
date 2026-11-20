package dev.orbit.android.ui.shell

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Archive
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.CreateNewFolder
import androidx.compose.material.icons.outlined.Difference
import androidx.compose.material.icons.outlined.DragHandle
import androidx.compose.material.icons.outlined.FilterList
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.FolderOpen
import androidx.compose.material.icons.outlined.Forum
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material.icons.outlined.MarkChatUnread
import androidx.compose.material.icons.outlined.PushPin
import androidx.compose.material.icons.outlined.QrCodeScanner
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.SortByAlpha
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material.icons.outlined.UnfoldMore
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.store.OrbitSidebarProjectSort
import dev.orbit.android.store.OrbitSidebarThreadFilter
import dev.orbit.android.store.OrbitUiState
import dev.orbit.android.ui.theme.OrbitCompactDropdownMenu
import dev.orbit.android.ui.theme.OrbitComposeIcon
import dev.orbit.android.ui.theme.OrbitCopyIcon
import dev.orbit.android.ui.theme.OrbitMenuActionRow
import dev.orbit.android.ui.theme.OrbitMenuDivider
import dev.orbit.android.ui.theme.OrbitMenuOptionRow
import dev.orbit.android.ui.theme.OrbitMenuScrollContainer
import dev.orbit.android.ui.theme.OrbitMenuSectionHeader
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiAgentCard
import uniffi.orbit_ffi_uniffi.FfiConnectionState
import uniffi.orbit_ffi_uniffi.FfiHostCard
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiSidebarConversation
import uniffi.orbit_ffi_uniffi.FfiSidebarProjectGroup

private const val MAX_VISIBLE_THREADS_PER_PROJECT = 5

private fun hostStatusColor(
    state: FfiConnectionState,
    colors: dev.orbit.android.ui.theme.OrbitColors,
): Color = when (state) {
    FfiConnectionState.ONLINE_DIRECT -> colors.directAdded
    FfiConnectionState.ONLINE_RELAY -> colors.relay
    FfiConnectionState.CONNECTING_DIRECT, FfiConnectionState.CONNECTING_RELAY -> colors.warning
    FfiConnectionState.DISCONNECTED -> colors.dangerRemoved
}

@Composable
fun FleetDrawerContent(
    ui: OrbitUiState,
    onSearchQueryChange: (String) -> Unit,
    onSetProjectSort: (OrbitSidebarProjectSort) -> Unit,
    onSetThreadFilter: (OrbitSidebarThreadFilter) -> Unit,
    onToggleProjectCollapsed: (String) -> Unit,
    onTogglePinThread: (String, String) -> Unit,
    onMarkAgentUnread: (String, String) -> Unit = { _, _ -> },
    onSelectHost: (String) -> Unit,
    onConnectLocalDaemon: () -> Unit,
    onSelectProjectDir: (String) -> Unit,
    onPinProject: (String, String) -> Unit,
    onUnpinProject: (String) -> Unit,
    onOpenDirectoryBrowser: () -> Unit,
    onSelectAgent: (String) -> Unit,
    onArchiveAgent: (String) -> Unit,
    onNewSession: () -> Unit,
    onNewSessionInWorkspace: (String, FfiProviderKind?) -> Unit,
    onInspectGitDiffAtCwd: (String) -> Unit,
    onOpenTerminalAtCwd: (String) -> Unit,
    onCopiedPath: () -> Unit,
    onOpenPairHost: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val context = LocalContext.current

    fun copyPathToClipboard(path: String) {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        clipboard?.setPrimaryClip(ClipData.newPlainText("Workspace Path", path))
        onCopiedPath()
    }

    val hostId = ui.selectedHostId
    val searchQuery = ui.drawerSearchQuery
    val normalizedQuery = remember(searchQuery) { searchQuery.trim().lowercase() }

    var expandedMoreProjectIds by remember(hostId) { mutableStateOf(emptySet<String>()) }
    var filterMenuExpanded by remember { mutableStateOf(false) }
    var hostMenuExpanded by remember { mutableStateOf(false) }

    val projectGroups = remember(ui.sidebarView, hostId) {
        if (hostId == null) {
            emptyList()
        } else {
            ui.sidebarView?.projects.orEmpty().filter { it.hostId == hostId }
        }
    }

    val needsAttentionItems = remember(ui.sidebarView, hostId, normalizedQuery) {
        if (hostId == null || normalizedQuery.isNotEmpty()) {
            emptyList()
        } else {
            ui.sidebarView?.needsAttention.orEmpty().filter { it.hostId == hostId }
        }
    }

    val pinnedItems = remember(ui.sidebarView, hostId, normalizedQuery) {
        if (hostId == null || normalizedQuery.isNotEmpty()) {
            emptyList()
        } else {
            ui.sidebarView?.pinnedConversations.orEmpty().filter { it.hostId == hostId }
        }
    }

    val hasActiveFilterOrCustomSort =
        ui.sidebarProjectSort != OrbitSidebarProjectSort.RECENT ||
            ui.sidebarThreadFilter != OrbitSidebarThreadFilter.ALL

    Column(
        modifier = modifier
            .fillMaxHeight()
            .background(colors.drawer)
            .statusBarsPadding()
            .navigationBarsPadding(),
    ) {
        // 1. Single-Row M3 Top Bar: [ Search Pill ] + [ Quick Compose Circle ]
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            Row(
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 44.dp)
                    .clip(RoundedCornerShape(22.dp))
                    .background(colors.subtleFill)
                    .padding(start = OrbitSpacing.md, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Icon(
                    imageVector = Icons.Outlined.Search,
                    contentDescription = null,
                    tint = colors.fgMuted,
                    modifier = Modifier.size(18.dp),
                )
                Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                    if (searchQuery.isEmpty()) {
                        Text(
                            text = "Search",
                            style = typography.subhead,
                            color = colors.fgMuted,
                            maxLines = 1,
                        )
                    }
                    BasicTextField(
                        value = searchQuery,
                        onValueChange = onSearchQueryChange,
                        singleLine = true,
                        textStyle = typography.subhead.copy(color = colors.fgPrimary),
                        cursorBrush = SolidColor(colors.primary),
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
                if (searchQuery.isNotEmpty()) {
                    Box(
                        modifier = Modifier
                            .size(40.dp)
                            .clip(CircleShape)
                            .clickable { onSearchQueryChange("") },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Close,
                            contentDescription = "Clear search",
                            tint = colors.fgMuted,
                            modifier = Modifier.size(16.dp),
                        )
                    }
                }
            }

            val canCompose = ui.selectedHostId != null
            Box(
                modifier = Modifier
                    .size(44.dp)
                    .clip(CircleShape)
                    .background(colors.subtleFill)
                    .alpha(if (canCompose) 1f else 0.45f)
                    .clickable(enabled = canCompose) {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onNewSession()
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = OrbitComposeIcon,
                    contentDescription = "New session",
                    tint = colors.fgPrimary,
                    modifier = Modifier.size(OrbitSpacing.iconSm),
                )
            }
        }

        // 2. Unified Tree Scrollable Area
        if (ui.hosts.isEmpty()) {
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(OrbitSpacing.lg),
                contentAlignment = Alignment.Center,
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(16.dp))
                        .background(colors.elevated)
                        .padding(OrbitSpacing.lg),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                ) {
                    Text(
                        text = "No Hosts Paired",
                        style = typography.bodySemiBold,
                        color = colors.fgPrimary,
                    )
                    Text(
                        text = "Pair a remote Orbit daemon via QR code or connect a local daemon to browse repositories and sessions.",
                        style = typography.subhead,
                        color = colors.fgSecondary,
                    )
                    if (ui.localDaemonUri != null) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 44.dp)
                                .clip(RoundedCornerShape(10.dp))
                                .background(colors.primary)
                                .clickable { onConnectLocalDaemon() }
                                .padding(horizontal = OrbitSpacing.md),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.Center,
                        ) {
                            Text(
                                text = "Connect Local Daemon",
                                style = typography.subheadSemiBold,
                                color = colors.onPrimary,
                            )
                        }
                    }
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(min = 44.dp)
                            .clip(RoundedCornerShape(10.dp))
                            .background(colors.subtleFill)
                            .clickable { onOpenPairHost() }
                            .padding(horizontal = OrbitSpacing.md),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.Center,
                    ) {
                        Text(
                            text = "Pair Remote Host (QR / URI)",
                            style = typography.subheadSemiBold,
                            color = colors.fgPrimary,
                        )
                    }
                }
            }
        } else {
            LazyColumn(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth(),
                contentPadding = PaddingValues(
                    top = OrbitSpacing.xs,
                    bottom = OrbitSpacing.md,
                ),
            ) {
                // Conditional Needs Attention
                if (needsAttentionItems.isNotEmpty() && hostId != null) {
                    item(key = "sec_needs_attention") {
                        DrawerSectionHeaderRow(
                            title = "Needs Attention",
                            accent = colors.warning,
                        )
                    }
                    items(needsAttentionItems, key = { "attn_${it.agent.agentId}" }) { conv ->
                        val agent = conv.agent
                        val isSelected = !ui.isNewSessionMode && agent.agentId == ui.selectedAgentId
                        ConversationTreeRow(
                            agent = agent,
                            selected = isSelected,
                            isUnread = conv.isUnread && !isSelected && !agent.isRunning,
                            isPinned = conv.isPinned,
                            showWorkspaceHint = true,
                            indentStart = 12.dp,
                            onSelect = { onSelectAgent(agent.agentId) },
                            onTogglePin = { onTogglePinThread(hostId, agent.agentId) },
                            onMarkUnread = if (!conv.isUnread && !isSelected) {
                                { onMarkAgentUnread(hostId, agent.agentId) }
                            } else {
                                null
                            },
                            onNewSessionInWorkspace = { onNewSessionInWorkspace(agent.cwd, agent.provider) },
                            onInspectGitDiff = { onInspectGitDiffAtCwd(agent.cwd) },
                            onOpenTerminalHere = { onOpenTerminalAtCwd(agent.cwd) },
                            onCopyWorkspacePath = { copyPathToClipboard(agent.cwd) },
                            onArchive = { onArchiveAgent(agent.agentId) },
                        )
                    }
                }

                // Conditional Pinned Conversations
                if (pinnedItems.isNotEmpty() && hostId != null) {
                    item(key = "sec_pinned") {
                        Spacer(modifier = Modifier.height(OrbitSpacing.sm))
                        DrawerSectionHeaderRow(title = "Pinned")
                    }
                    items(pinnedItems, key = { "pin_${it.agent.agentId}" }) { conv ->
                        val agent = conv.agent
                        val isSelected = !ui.isNewSessionMode && agent.agentId == ui.selectedAgentId
                        ConversationTreeRow(
                            agent = agent,
                            selected = isSelected,
                            isUnread = conv.isUnread && !isSelected && !agent.isRunning,
                            isPinned = true,
                            showWorkspaceHint = true,
                            indentStart = 12.dp,
                            onSelect = { onSelectAgent(agent.agentId) },
                            onTogglePin = { onTogglePinThread(hostId, agent.agentId) },
                            onMarkUnread = if (!conv.isUnread && !isSelected) {
                                { onMarkAgentUnread(hostId, agent.agentId) }
                            } else {
                                null
                            },
                            onNewSessionInWorkspace = { onNewSessionInWorkspace(agent.cwd, agent.provider) },
                            onInspectGitDiff = { onInspectGitDiffAtCwd(agent.cwd) },
                            onOpenTerminalHere = { onOpenTerminalAtCwd(agent.cwd) },
                            onCopyWorkspacePath = { copyPathToClipboard(agent.cwd) },
                            onArchive = { onArchiveAgent(agent.agentId) },
                        )
                    }
                }

                // Projects Header Row: "Projects" + [ Filter/Sort ] + [ Open Folder ]
                item(key = "sec_projects_header") {
                    if (needsAttentionItems.isNotEmpty() || pinnedItems.isNotEmpty()) {
                        Spacer(modifier = Modifier.height(OrbitSpacing.sm))
                    }
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(start = OrbitSpacing.lg, end = OrbitSpacing.xs),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = "Projects",
                            style = typography.metaMedium,
                            color = colors.fgMuted,
                            modifier = Modifier.weight(1f),
                        )

                        Box {
                            Box(
                                modifier = Modifier
                                    .size(44.dp)
                                    .clip(CircleShape)
                                    .clickable { filterMenuExpanded = true },
                                contentAlignment = Alignment.Center,
                            ) {
                                Icon(
                                    imageVector = Icons.Outlined.FilterList,
                                    contentDescription = "Sort and filter conversations",
                                    tint = if (hasActiveFilterOrCustomSort) colors.primary else colors.fgSecondary,
                                    modifier = Modifier.size(OrbitSpacing.iconSm),
                                )
                            }
                            OrbitCompactDropdownMenu(
                                expanded = filterMenuExpanded,
                                onDismissRequest = { filterMenuExpanded = false },
                                width = 232.dp,
                            ) {
                                OrbitMenuSectionHeader(title = "Sort by")
                                OrbitSidebarProjectSort.entries.forEach { sort ->
                                    OrbitMenuOptionRow(
                                        title = sort.label,
                                        icon = when (sort) {
                                            OrbitSidebarProjectSort.RECENT -> Icons.Outlined.Schedule
                                            OrbitSidebarProjectSort.NAME -> Icons.Outlined.SortByAlpha
                                            OrbitSidebarProjectSort.MANUAL -> Icons.Outlined.DragHandle
                                        },
                                        selected = ui.sidebarProjectSort == sort,
                                        onClick = {
                                            filterMenuExpanded = false
                                            onSetProjectSort(sort)
                                        },
                                    )
                                }
                                OrbitMenuDivider()
                                OrbitMenuSectionHeader(title = "Show")
                                OrbitSidebarThreadFilter.entries.forEach { filter ->
                                    OrbitMenuOptionRow(
                                        title = filter.label,
                                        icon = when (filter) {
                                            OrbitSidebarThreadFilter.ALL -> Icons.Outlined.Forum
                                            OrbitSidebarThreadFilter.ACTIVE_AND_UNREAD -> Icons.Outlined.MarkChatUnread
                                        },
                                        selected = ui.sidebarThreadFilter == filter,
                                        onClick = {
                                            filterMenuExpanded = false
                                            onSetThreadFilter(filter)
                                        },
                                    )
                                }
                            }
                        }

                        Box(
                            modifier = Modifier
                                .size(44.dp)
                                .clip(CircleShape)
                                .alpha(if (hostId != null) 1f else 0.4f)
                                .clickable(enabled = hostId != null) { onOpenDirectoryBrowser() },
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.CreateNewFolder,
                                contentDescription = "Open remote folder",
                                tint = colors.fgSecondary,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                        }
                    }
                }

                // Projects → Conversations Tree
                if (projectGroups.isEmpty()) {
                    item(key = "empty_projects") {
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.md),
                            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                        ) {
                            val emptyMessage = when {
                                normalizedQuery.isNotEmpty() -> "No matching projects or conversations."
                                ui.sidebarThreadFilter == OrbitSidebarThreadFilter.ACTIVE_AND_UNREAD ->
                                    "No active or unread conversations."
                                else -> "No repositories discovered yet."
                            }
                            Text(
                                text = emptyMessage,
                                style = typography.subhead,
                                color = colors.fgMuted,
                            )
                            if (normalizedQuery.isNotEmpty()) {
                                Text(
                                    text = "Clear search",
                                    style = typography.subheadSemiBold,
                                    color = colors.primary,
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(8.dp))
                                        .clickable { onSearchQueryChange("") }
                                        .padding(vertical = 6.dp),
                                )
                            } else if (ui.sidebarThreadFilter != OrbitSidebarThreadFilter.ALL) {
                                Text(
                                    text = "Show all conversations",
                                    style = typography.subheadSemiBold,
                                    color = colors.primary,
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(8.dp))
                                        .clickable { onSetThreadFilter(OrbitSidebarThreadFilter.ALL) }
                                        .padding(vertical = 6.dp),
                                )
                            }
                        }
                    }
                } else if (hostId != null) {
                    projectGroups.forEach { group ->
                        val isCollapsed = normalizedQuery.isEmpty() &&
                            ui.collapsedProjectIds.contains(group.id)
                        val isActiveProject = OrbitFormatters.pathsEquivalent(group.primaryCwd, ui.activeCwd)

                        item(key = "proj_${group.id}") {
                            ProjectTreeHeaderRow(
                                group = group,
                                isCollapsed = isCollapsed,
                                isActiveWorkspace = isActiveProject && ui.isNewSessionMode,
                                onToggleCollapse = { onToggleProjectCollapsed(group.id) },
                                onNewSessionHere = { onNewSessionInWorkspace(group.primaryCwd, null) },
                                onSelectWorkspace = { onSelectProjectDir(group.primaryCwd) },
                                onTogglePin = {
                                    val pinnedId = group.pinnedProjectId
                                    if (pinnedId != null) {
                                        onUnpinProject(pinnedId)
                                    } else {
                                        onPinProject(group.name, group.primaryCwd)
                                    }
                                },
                                onInspectGitDiff = { onInspectGitDiffAtCwd(group.primaryCwd) },
                                onOpenTerminalHere = { onOpenTerminalAtCwd(group.primaryCwd) },
                                onCopyPath = { copyPathToClipboard(group.primaryCwd) },
                            )
                        }

                        if (!isCollapsed) {
                            val isMoreExpanded = normalizedQuery.isNotEmpty() ||
                                expandedMoreProjectIds.contains(group.id)
                            val visibleThreads = if (isMoreExpanded) {
                                group.conversations
                            } else {
                                group.conversations.take(MAX_VISIBLE_THREADS_PER_PROJECT)
                            }
                            val hiddenCount = (group.conversations.size - MAX_VISIBLE_THREADS_PER_PROJECT).coerceAtLeast(0)

                            items(visibleThreads, key = { "thread_${group.id}_${it.agent.agentId}" }) { conv ->
                                val agent = conv.agent
                                val isSelected = !ui.isNewSessionMode && agent.agentId == ui.selectedAgentId
                                ConversationTreeRow(
                                    agent = agent,
                                    selected = isSelected,
                                    isUnread = conv.isUnread && !isSelected && !agent.isRunning,
                                    isPinned = conv.isPinned,
                                    showWorkspaceHint = false,
                                    indentStart = 20.dp,
                                    onSelect = { onSelectAgent(agent.agentId) },
                                    onTogglePin = { onTogglePinThread(hostId, agent.agentId) },
                                    onMarkUnread = if (!conv.isUnread && !isSelected) {
                                        { onMarkAgentUnread(hostId, agent.agentId) }
                                    } else {
                                        null
                                    },
                                    onNewSessionInWorkspace = { onNewSessionInWorkspace(agent.cwd, agent.provider) },
                                    onInspectGitDiff = { onInspectGitDiffAtCwd(agent.cwd) },
                                    onOpenTerminalHere = { onOpenTerminalAtCwd(agent.cwd) },
                                    onCopyWorkspacePath = { copyPathToClipboard(agent.cwd) },
                                    onArchive = { onArchiveAgent(agent.agentId) },
                                )
                            }

                            if (hiddenCount > 0) {
                                item(key = "more_${group.id}") {
                                    Row(
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .padding(horizontal = OrbitSpacing.sm)
                                            .heightIn(min = 44.dp)
                                            .clip(RoundedCornerShape(10.dp))
                                            .clickable {
                                                expandedMoreProjectIds = if (isMoreExpanded) {
                                                    expandedMoreProjectIds - group.id
                                                } else {
                                                    expandedMoreProjectIds + group.id
                                                }
                                            }
                                            .padding(start = 42.dp, end = OrbitSpacing.md),
                                        verticalAlignment = Alignment.CenterVertically,
                                    ) {
                                        Text(
                                            text = if (isMoreExpanded) "Show less" else "Show $hiddenCount more…",
                                            style = typography.metaMedium,
                                            color = colors.fgMuted,
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. Thumb-Zone Bottom Utility Bar: [ Host Switcher Pill ] + [ Settings Circle ]
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            Box(modifier = Modifier.weight(1f)) {
                val activeHost = ui.selectedHost
                val dotColor = if (activeHost != null) {
                    hostStatusColor(activeHost.connectionState, colors)
                } else {
                    colors.fgMuted
                }
                val statusSummary = if (activeHost != null) {
                    val connLabel = OrbitFormatters.topBarConnectionLabel(
                        activeHost.connectionState,
                        activeHost.viaSshTunnel,
                    )
                    val ping = activeHost.lastPingMs?.let { " · ${it}ms" }.orEmpty()
                    "$connLabel$ping"
                } else {
                    "Tap to pair host"
                }

                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 44.dp)
                        .clip(RoundedCornerShape(22.dp))
                        .background(colors.subtleFill)
                        .clickable {
                            if (ui.hosts.isEmpty()) {
                                onOpenPairHost()
                            } else {
                                hostMenuExpanded = true
                            }
                        }
                        .padding(horizontal = OrbitSpacing.md, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    Box(
                        modifier = Modifier
                            .size(8.dp)
                            .clip(CircleShape)
                            .background(dotColor),
                    )
                    Text(
                        text = activeHost?.displayName ?: "No Host",
                        style = typography.subheadSemiBold,
                        color = colors.fgPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = statusSummary,
                        style = typography.meta,
                        color = colors.fgMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    if (ui.otherHostsPendingApprovals > 0u) {
                        Box(
                            modifier = Modifier
                                .clip(RoundedCornerShape(10.dp))
                                .background(colors.warning.copy(alpha = 0.18f))
                                .padding(horizontal = 6.dp, vertical = 2.dp),
                        ) {
                            Text(
                                text = ui.otherHostsPendingApprovals.toString(),
                                style = typography.metaSemiBold,
                                color = colors.warning,
                            )
                        }
                    }
                    Icon(
                        imageVector = Icons.Outlined.UnfoldMore,
                        contentDescription = "Switch host",
                        tint = colors.fgMuted,
                        modifier = Modifier.size(16.dp),
                    )
                }

                OrbitCompactDropdownMenu(
                    expanded = hostMenuExpanded,
                    onDismissRequest = { hostMenuExpanded = false },
                    width = 264.dp,
                ) {
                    OrbitMenuSectionHeader(title = "Hosts")
                    OrbitMenuScrollContainer(maxHeight = 220.dp) {
                        ui.hosts.forEach { host ->
                            val isSelected = host.hostId == ui.selectedHostId
                            val hostDot = hostStatusColor(host.connectionState, colors)
                            val hostSub = buildString {
                                append(OrbitFormatters.topBarConnectionLabel(host.connectionState, host.viaSshTunnel))
                                host.lastPingMs?.let { append(" · ${it}ms") }
                            }
                            OrbitMenuOptionRow(
                                title = host.displayName,
                                subtitle = hostSub,
                                selected = isSelected,
                                leading = {
                                    Box(
                                        modifier = Modifier
                                            .size(7.dp)
                                            .clip(CircleShape)
                                            .background(hostDot),
                                    )
                                },
                                trailingBadge = if (host.pendingApprovalCount > 0u) {
                                    {
                                        Box(
                                            modifier = Modifier
                                                .clip(CircleShape)
                                                .background(colors.warning.copy(alpha = 0.16f))
                                                .padding(horizontal = 6.dp, vertical = 1.dp),
                                        ) {
                                            Text(
                                                text = host.pendingApprovalCount.toString(),
                                                style = typography.metaSemiBold,
                                                color = colors.warning,
                                            )
                                        }
                                    }
                                } else {
                                    null
                                },
                                onClick = {
                                    hostMenuExpanded = false
                                    onSelectHost(host.hostId)
                                },
                            )
                        }
                    }
                    OrbitMenuDivider()
                    OrbitMenuActionRow(
                        title = "Pair New Host",
                        icon = Icons.Outlined.QrCodeScanner,
                        onClick = {
                            hostMenuExpanded = false
                            onOpenPairHost()
                        },
                    )
                    if (ui.localDaemonUri != null) {
                        OrbitMenuActionRow(
                            title = "Connect Local Host",
                            icon = Icons.Outlined.Add,
                            onClick = {
                                hostMenuExpanded = false
                                onConnectLocalDaemon()
                            },
                        )
                    }
                }
            }

            Box(
                modifier = Modifier
                    .size(44.dp)
                    .clip(CircleShape)
                    .background(colors.subtleFill)
                    .clickable { onOpenSettings() },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Settings,
                    contentDescription = "Settings and hosts",
                    tint = colors.fgPrimary,
                    modifier = Modifier.size(19.dp),
                )
            }
        }
    }
}

@Composable
private fun DrawerSectionHeaderRow(
    title: String,
    accent: Color? = null,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = OrbitSpacing.lg, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = title,
            style = typography.metaMedium,
            color = accent ?: colors.fgMuted,
        )
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ProjectTreeHeaderRow(
    group: FfiSidebarProjectGroup,
    isCollapsed: Boolean,
    isActiveWorkspace: Boolean,
    onToggleCollapse: () -> Unit,
    onNewSessionHere: () -> Unit,
    onSelectWorkspace: () -> Unit,
    onTogglePin: () -> Unit,
    onInspectGitDiff: () -> Unit,
    onOpenTerminalHere: () -> Unit,
    onCopyPath: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    var menuExpanded by remember { mutableStateOf(false) }

    Box(modifier = Modifier.padding(horizontal = OrbitSpacing.sm, vertical = 1.dp)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 44.dp)
                .clip(RoundedCornerShape(10.dp))
                .background(if (isActiveWorkspace) colors.subtleFill else Color.Transparent)
                .combinedClickable(
                    onClick = onToggleCollapse,
                    onLongClick = {
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        menuExpanded = true
                    },
                )
                .padding(start = 6.dp, end = 0.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(
                imageVector = if (isCollapsed) {
                    Icons.AutoMirrored.Outlined.KeyboardArrowRight
                } else {
                    Icons.Outlined.KeyboardArrowDown
                },
                contentDescription = if (isCollapsed) "Expand project" else "Collapse project",
                tint = colors.fgMuted,
                modifier = Modifier.size(16.dp),
            )
            Icon(
                imageVector = if (isCollapsed) Icons.Outlined.Folder else Icons.Outlined.FolderOpen,
                contentDescription = null,
                tint = if (isActiveWorkspace) colors.primary else colors.fgSecondary,
                modifier = Modifier.size(16.dp),
            )
            Text(
                text = group.name,
                style = if (isActiveWorkspace) typography.subheadSemiBold else typography.subheadMedium,
                color = colors.fgPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Box(
                modifier = Modifier
                    .size(44.dp)
                    .clip(CircleShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onNewSessionHere()
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Add,
                    contentDescription = "New session in ${group.name}",
                    tint = colors.fgSecondary,
                    modifier = Modifier.size(18.dp),
                )
            }
        }

        DropdownMenu(
            expanded = menuExpanded,
            onDismissRequest = { menuExpanded = false },
            containerColor = colors.elevated,
        ) {
            DropdownMenuItem(
                text = { Text("New Session Here", style = typography.subheadMedium, color = colors.fgPrimary) },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.Add,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onNewSessionHere()
                },
            )
            DropdownMenuItem(
                text = { Text("Select Workspace", style = typography.subheadMedium, color = colors.fgPrimary) },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.FolderOpen,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onSelectWorkspace()
                },
            )
            DropdownMenuItem(
                text = { Text("Inspect Git Diff", style = typography.subheadMedium, color = colors.fgPrimary) },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.Difference,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onInspectGitDiff()
                },
            )
            DropdownMenuItem(
                text = { Text("Open Terminal", style = typography.subheadMedium, color = colors.fgPrimary) },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.Terminal,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onOpenTerminalHere()
                },
            )
            DropdownMenuItem(
                text = {
                    Text(
                        text = if (group.pinnedProjectId != null) "Unpin Project" else "Pin Project",
                        style = typography.subheadMedium,
                        color = colors.fgPrimary,
                    )
                },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.PushPin,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onTogglePin()
                },
            )
            DropdownMenuItem(
                text = { Text("Copy Path", style = typography.subheadMedium, color = colors.fgPrimary) },
                leadingIcon = {
                    Icon(
                        imageVector = OrbitCopyIcon,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onCopyPath()
                },
            )
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ConversationTreeRow(
    agent: FfiAgentCard,
    selected: Boolean,
    isUnread: Boolean,
    isPinned: Boolean,
    showWorkspaceHint: Boolean,
    indentStart: androidx.compose.ui.unit.Dp,
    onSelect: () -> Unit,
    onTogglePin: () -> Unit,
    onMarkUnread: (() -> Unit)? = null,
    onNewSessionInWorkspace: () -> Unit,
    onInspectGitDiff: () -> Unit,
    onOpenTerminalHere: () -> Unit,
    onCopyWorkspacePath: () -> Unit,
    onArchive: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    var menuExpanded by remember { mutableStateOf(false) }

    val unreadBlue = if (colors.isDark) Color(0xFF60A5FA) else Color(0xFF2563EB)

    Box(modifier = Modifier.padding(horizontal = OrbitSpacing.sm, vertical = 1.dp)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 44.dp)
                .clip(RoundedCornerShape(10.dp))
                .background(if (selected) colors.subtleFill else Color.Transparent)
                .combinedClickable(
                    onClick = {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onSelect()
                    },
                    onLongClick = {
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        menuExpanded = true
                    },
                )
                .padding(start = indentStart, end = OrbitSpacing.md, top = 7.dp, bottom = 7.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            // 4-state left gutter: Waiting Approval | Running | Unread Blue Dot | Empty (zero idle dots)
            Box(
                modifier = Modifier.width(16.dp),
                contentAlignment = Alignment.Center,
            ) {
                when {
                    agent.isAwaitingPermission -> {
                        Icon(
                            imageVector = Icons.Outlined.WarningAmber,
                            contentDescription = "Awaiting permission",
                            tint = colors.warning,
                            modifier = Modifier.size(14.dp),
                        )
                    }

                    agent.isRunning -> {
                        CircularProgressIndicator(
                            modifier = Modifier.size(13.dp),
                            strokeWidth = 1.75.dp,
                            color = colors.fgSecondary,
                        )
                    }

                    isUnread -> {
                        Box(
                            modifier = Modifier
                                .size(7.dp)
                                .clip(CircleShape)
                                .background(unreadBlue),
                        )
                    }

                    else -> Spacer(modifier = Modifier.size(1.dp))
                }
            }

            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = agent.title.ifBlank { OrbitFormatters.providerLabel(agent.provider) },
                    style = if (selected) typography.subheadSemiBold else typography.subhead,
                    color = colors.fgPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (showWorkspaceHint) {
                    Text(
                        text = OrbitFormatters.shortPath(agent.cwd),
                        style = typography.meta,
                        color = colors.fgMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }

            val compactTime = remember(agent.updatedAtRfc3339) {
                OrbitFormatters.compactRelativeTime(agent.updatedAtRfc3339)
            }
            if (compactTime.isNotEmpty()) {
                Text(
                    text = compactTime,
                    style = typography.meta,
                    color = colors.fgMuted,
                    maxLines = 1,
                )
            }
        }

        DropdownMenu(
            expanded = menuExpanded,
            onDismissRequest = { menuExpanded = false },
            containerColor = colors.elevated,
        ) {
            DropdownMenuItem(
                text = {
                    Text(
                        text = if (isPinned) "Unpin Conversation" else "Pin Conversation",
                        style = typography.subheadMedium,
                        color = colors.fgPrimary,
                    )
                },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.PushPin,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onTogglePin()
                },
            )
            if (onMarkUnread != null) {
                DropdownMenuItem(
                    text = {
                        Text(
                            text = "Mark Unread",
                            style = typography.subheadMedium,
                            color = colors.fgPrimary,
                        )
                    },
                    leadingIcon = {
                        Icon(
                            imageVector = Icons.Outlined.MarkChatUnread,
                            contentDescription = null,
                            tint = colors.fgSecondary,
                            modifier = Modifier.size(OrbitSpacing.iconSm),
                        )
                    },
                    onClick = {
                        menuExpanded = false
                        onMarkUnread()
                    },
                )
            }
            DropdownMenuItem(
                text = { Text("New Session in Workspace", style = typography.subheadMedium, color = colors.fgPrimary) },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.Add,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onNewSessionInWorkspace()
                },
            )
            DropdownMenuItem(
                text = { Text("Inspect Git Diff", style = typography.subheadMedium, color = colors.fgPrimary) },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.Difference,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onInspectGitDiff()
                },
            )
            DropdownMenuItem(
                text = { Text("Open Terminal", style = typography.subheadMedium, color = colors.fgPrimary) },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.Terminal,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onOpenTerminalHere()
                },
            )
            DropdownMenuItem(
                text = { Text("Copy Workspace Path", style = typography.subheadMedium, color = colors.fgPrimary) },
                leadingIcon = {
                    Icon(
                        imageVector = OrbitCopyIcon,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onCopyWorkspacePath()
                },
            )
            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
            DropdownMenuItem(
                text = { Text("Archive Session", style = typography.subheadSemiBold, color = colors.dangerRemoved) },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.Archive,
                        contentDescription = null,
                        tint = colors.dangerRemoved,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                },
                onClick = {
                    menuExpanded = false
                    onArchive()
                },
            )
        }
    }
}
