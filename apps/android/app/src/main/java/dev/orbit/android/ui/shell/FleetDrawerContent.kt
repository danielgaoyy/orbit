package dev.orbit.android.ui.shell

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Archive
import androidx.compose.material.icons.outlined.Bookmark
import androidx.compose.material.icons.outlined.BookmarkBorder
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.Difference
import androidx.compose.material.icons.outlined.FolderOpen
import androidx.compose.material.icons.outlined.QrCodeScanner
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.store.OrbitUiState
import dev.orbit.android.ui.stage.BreathingDot
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiAgentCard
import uniffi.orbit_ffi_uniffi.FfiConnectionState
import uniffi.orbit_ffi_uniffi.FfiDiscoveredProject
import uniffi.orbit_ffi_uniffi.FfiHostCard
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiWorkspaceSummary

private fun resolveLiveWorkspaceBranch(
    cwd: String,
    activeCwd: String,
    liveGitBranch: String?,
    discoveredProjects: List<FfiDiscoveredProject>,
    workspaces: List<FfiWorkspaceSummary>,
): String? {
    if (!liveGitBranch.isNullOrBlank() && OrbitFormatters.pathsEquivalent(cwd, activeCwd)) {
        return liveGitBranch
    }
    discoveredProjects.firstOrNull { OrbitFormatters.pathsEquivalent(it.path, cwd) }
        ?.gitBranch?.takeIf { it.isNotBlank() }
        ?.let { return it }
    workspaces.firstOrNull { OrbitFormatters.pathsEquivalent(it.cwd, cwd) }
        ?.gitBranch?.takeIf { it.isNotBlank() }
        ?.let { return it }
    return null
}

private enum class DrawerFilterChip(val label: String) {
    ALL("All"),
    ACTIVE("Active"),
    APPROVALS("Approvals"),
}

@Composable
fun FleetDrawerContent(
    ui: OrbitUiState,
    onSearchQueryChange: (String) -> Unit,
    onSelectHost: (String) -> Unit,
    onRefreshHost: () -> Unit,
    onSelectProjectDir: (String) -> Unit,
    onPinProject: (name: String, directory: String) -> Unit,
    onUnpinProject: (String) -> Unit,
    onOpenDirectoryBrowser: () -> Unit,
    onSelectAgent: (String) -> Unit,
    onArchiveAgent: (String) -> Unit,
    onNewSession: () -> Unit,
    onNewSessionInWorkspace: (cwd: String, provider: FfiProviderKind?) -> Unit,
    onInspectGitDiffAtCwd: (cwd: String) -> Unit,
    onOpenTerminalAtCwd: (cwd: String) -> Unit,
    onCopiedPath: (path: String) -> Unit,
    onOpenPairHost: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val context = LocalContext.current
    val haptics = LocalHapticFeedback.current

    val copyPathToClipboard: (String) -> Unit = { rawPath ->
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        cm?.setPrimaryClip(ClipData.newPlainText("workspace_path", rawPath))
        onCopiedPath(rawPath)
    }

    val searchQuery = ui.drawerSearchQuery
    var activeFilter by rememberSaveable(ui.selectedHostId) { mutableStateOf(DrawerFilterChip.ALL) }
    val normalizedQuery = remember(searchQuery) { searchQuery.trim().lowercase() }

    val agents = ui.hostDetail?.agents.orEmpty()
    val totalApprovalsCount = remember(agents) { agents.count { it.isAwaitingPermission } }

    val queryFilteredAgents = remember(agents, normalizedQuery) {
        if (normalizedQuery.isEmpty()) {
            agents
        } else {
            agents.filter { ag ->
                ag.title.lowercase().contains(normalizedQuery) ||
                    ag.cwd.lowercase().contains(normalizedQuery) ||
                    ag.providerLabel.lowercase().contains(normalizedQuery) ||
                    OrbitFormatters.providerLabel(ag.provider).lowercase().contains(normalizedQuery)
            }
        }
    }

    val needsApprovalAgents = remember(queryFilteredAgents, activeFilter) {
        when (activeFilter) {
            DrawerFilterChip.ALL, DrawerFilterChip.APPROVALS ->
                queryFilteredAgents.filter { it.isAwaitingPermission }
            DrawerFilterChip.ACTIVE -> emptyList()
        }
    }
    val activeAgents = remember(queryFilteredAgents, activeFilter) {
        when (activeFilter) {
            DrawerFilterChip.ALL -> queryFilteredAgents.filter { it.isRunning && !it.isAwaitingPermission }
            DrawerFilterChip.ACTIVE -> queryFilteredAgents.filter { it.isRunning }
            DrawerFilterChip.APPROVALS -> emptyList()
        }
    }
    val recentAgents = remember(queryFilteredAgents, activeFilter) {
        when (activeFilter) {
            DrawerFilterChip.ALL -> queryFilteredAgents.filter { !it.isRunning && !it.isAwaitingPermission }
            DrawerFilterChip.ACTIVE, DrawerFilterChip.APPROVALS -> emptyList()
        }
    }

    val hostProjects = remember(ui.projects, ui.selectedHostId) {
        ui.projects.filter { it.hostId == ui.selectedHostId }
    }
    val pinnedPaths = remember(hostProjects) {
        hostProjects.flatMap { it.directories }.toSet()
    }
    val allDiscovered = ui.hostDetail?.discoveredProjects.orEmpty()
    val allWorkspaces = ui.hostDetail?.workspaces.orEmpty()
    val liveActiveBranch = ui.gitReview?.branch

    val filteredHostProjects = remember(
        hostProjects,
        normalizedQuery,
        ui.activeCwd,
        liveActiveBranch,
        allDiscovered,
        allWorkspaces,
    ) {
        if (normalizedQuery.isEmpty()) {
            hostProjects
        } else {
            hostProjects.filter { proj ->
                proj.name.lowercase().contains(normalizedQuery) ||
                    proj.directories.any { dir ->
                        val resolvedBr = resolveLiveWorkspaceBranch(
                            cwd = dir,
                            activeCwd = ui.activeCwd,
                            liveGitBranch = liveActiveBranch,
                            discoveredProjects = allDiscovered,
                            workspaces = allWorkspaces,
                        )
                        dir.lowercase().contains(normalizedQuery) ||
                            (resolvedBr?.lowercase()?.contains(normalizedQuery) == true)
                    }
            }
        }
    }
    val extraWorkspaces = remember(allWorkspaces, pinnedPaths, allDiscovered) {
        allWorkspaces.filter { ws ->
            pinnedPaths.none { OrbitFormatters.pathsEquivalent(it, ws.cwd) } &&
                allDiscovered.none { OrbitFormatters.pathsEquivalent(it.path, ws.cwd) }
        }
    }
    val filteredExtraWorkspaces = remember(
        extraWorkspaces,
        normalizedQuery,
        ui.activeCwd,
        liveActiveBranch,
        allDiscovered,
        allWorkspaces,
    ) {
        if (normalizedQuery.isEmpty()) {
            extraWorkspaces
        } else {
            extraWorkspaces.filter { ws ->
                val resolvedBr = resolveLiveWorkspaceBranch(
                    cwd = ws.cwd,
                    activeCwd = ui.activeCwd,
                    liveGitBranch = liveActiveBranch,
                    discoveredProjects = allDiscovered,
                    workspaces = allWorkspaces,
                )
                ws.title.lowercase().contains(normalizedQuery) ||
                    ws.cwd.lowercase().contains(normalizedQuery) ||
                    (resolvedBr?.lowercase()?.contains(normalizedQuery) == true)
            }
        }
    }
    val discoveredRepos = remember(
        allDiscovered,
        pinnedPaths,
        normalizedQuery,
        ui.activeCwd,
        liveActiveBranch,
        allWorkspaces,
    ) {
        allDiscovered
            .filterNot { repo -> pinnedPaths.any { OrbitFormatters.pathsEquivalent(it, repo.path) } }
            .filter { repo ->
                val resolvedBr = resolveLiveWorkspaceBranch(
                    cwd = repo.path,
                    activeCwd = ui.activeCwd,
                    liveGitBranch = liveActiveBranch,
                    discoveredProjects = allDiscovered,
                    workspaces = allWorkspaces,
                )
                normalizedQuery.isEmpty() ||
                    repo.name.lowercase().contains(normalizedQuery) ||
                    repo.path.lowercase().contains(normalizedQuery) ||
                    (resolvedBr?.lowercase()?.contains(normalizedQuery) == true)
            }
            .take(12)
    }
    val showProjectsSection = activeFilter == DrawerFilterChip.ALL ||
        (normalizedQuery.isNotEmpty() && (
            filteredHostProjects.isNotEmpty() ||
                filteredExtraWorkspaces.isNotEmpty() ||
                discoveredRepos.isNotEmpty()
            ))

    Column(
        modifier = modifier
            .fillMaxHeight()
            .background(colors.drawer)
            .statusBarsPadding()
            .navigationBarsPadding(),
    ) {
        // Drawer Top Header
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = OrbitSpacing.topBarHeight)
                .padding(horizontal = OrbitSpacing.lg),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                text = "Orbit",
                style = typography.title,
                color = colors.fgPrimary,
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (ui.selectedHostId != null) {
                    Box(
                        modifier = Modifier
                            .size(44.dp)
                            .clip(CircleShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onRefreshHost()
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Refresh,
                            contentDescription = "Refresh host",
                            tint = colors.fgSecondary,
                            modifier = Modifier.size(OrbitSpacing.iconSm),
                        )
                    }
                    Box(
                        modifier = Modifier
                            .size(44.dp)
                            .clip(CircleShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onNewSession()
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Add,
                            contentDescription = "New Session",
                            tint = colors.fgPrimary,
                            modifier = Modifier.size(OrbitSpacing.iconMd),
                        )
                    }
                }
            }
        }

        HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

        // Scrollable Fleet, Workspaces, and Grouped Sessions
        LazyColumn(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth(),
            contentPadding = PaddingValues(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.md),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
        ) {
            // 1. Multi-Host Fleet Switcher
            item(key = "fleet_section_header") {
                DrawerSectionLabel(title = "Hosts (${ui.hosts.size})")
            }

            if (ui.hosts.isEmpty()) {
                item(key = "fleet_empty") {
                    Text(
                        text = "No hosts paired yet.",
                        style = typography.subhead,
                        color = colors.fgMuted,
                        modifier = Modifier.padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                    )
                }
            } else {
                items(ui.hosts, key = { it.hostId }) { host ->
                    HostSwitcherCard(
                        host = host,
                        selected = host.hostId == ui.selectedHostId,
                        onClick = {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onSelectHost(host.hostId)
                        },
                    )
                }
            }

            // 2. Search & Filter Strip + Projects & Grouped Sessions
            if (ui.selectedHostId != null) {
                item(key = "drawer_search_and_filters") {
                    Spacer(modifier = Modifier.height(OrbitSpacing.sm))
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                    ) {
                        val searchShape = RoundedCornerShape(OrbitSpacing.sm)
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 44.dp)
                                .clip(searchShape)
                                .background(colors.subtleFill, searchShape)
                                .border(OrbitSpacing.hairline, colors.hairline, searchShape)
                                .padding(start = OrbitSpacing.sm),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Search,
                                contentDescription = null,
                                tint = colors.fgMuted,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                            Box(modifier = Modifier.weight(1f)) {
                                if (searchQuery.isEmpty()) {
                                    Text(
                                        text = "Search sessions or repos…",
                                        style = typography.subhead,
                                        color = colors.fgMuted,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                                BasicTextField(
                                    value = searchQuery,
                                    onValueChange = { onSearchQueryChange(it) },
                                    singleLine = true,
                                    textStyle = typography.subhead.copy(color = colors.fgPrimary),
                                    cursorBrush = SolidColor(colors.primary),
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            }
                            if (searchQuery.isNotEmpty()) {
                                Box(
                                    modifier = Modifier
                                        .size(44.dp)
                                        .clip(CircleShape)
                                        .clickable {
                                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                            onSearchQueryChange("")
                                        },
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Icon(
                                        imageVector = Icons.Outlined.Close,
                                        contentDescription = "Clear search",
                                        tint = colors.fgMuted,
                                        modifier = Modifier.size(OrbitSpacing.iconSm),
                                    )
                                }
                            } else {
                                Spacer(modifier = Modifier.width(OrbitSpacing.sm))
                            }
                        }

                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            DrawerFilterChip.entries.forEach { chip ->
                                val selected = activeFilter == chip
                                val pillShape = RoundedCornerShape(OrbitSpacing.sm)
                                val hasWarningDot = chip == DrawerFilterChip.APPROVALS && totalApprovalsCount > 0
                                val borderColor = when {
                                    selected && hasWarningDot -> colors.warning.copy(alpha = 0.7f)
                                    selected -> colors.primary.copy(alpha = 0.65f)
                                    else -> colors.hairline
                                }
                                val labelColor = when {
                                    selected && hasWarningDot -> colors.warning
                                    selected -> colors.primary
                                    else -> colors.fgSecondary
                                }
                                Row(
                                    modifier = Modifier
                                        .heightIn(min = OrbitSpacing.xxl)
                                        .clip(pillShape)
                                        .background(if (selected) colors.elevated else Color.Transparent, pillShape)
                                        .border(OrbitSpacing.hairline, borderColor, pillShape)
                                        .clickable {
                                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                            activeFilter = chip
                                        }
                                        .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                                ) {
                                    if (hasWarningDot) {
                                        Box(
                                            modifier = Modifier
                                                .size(6.dp)
                                                .background(colors.warning, CircleShape)
                                        )
                                    }
                                    Text(
                                        text = chip.label,
                                        style = if (selected) typography.metaSemiBold else typography.metaMedium,
                                        color = labelColor,
                                        maxLines = 1,
                                        softWrap = false,
                                    )
                                }
                            }
                        }
                    }
                }

                if (showProjectsSection) {
                    item(key = "projects_header") {
                        Spacer(modifier = Modifier.height(OrbitSpacing.sm))
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                text = "Projects",
                                style = typography.metaSemiBold,
                                color = colors.fgMuted,
                            )
                            Row(
                                modifier = Modifier
                                    .clip(RoundedCornerShape(OrbitSpacing.xs))
                                    .clickable { onOpenDirectoryBrowser() }
                                    .padding(horizontal = OrbitSpacing.xs, vertical = OrbitSpacing.xs),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                            ) {
                                Icon(
                                    imageVector = Icons.Outlined.FolderOpen,
                                    contentDescription = null,
                                    tint = colors.primary,
                                    modifier = Modifier.size(OrbitSpacing.iconSm),
                                )
                                Text(
                                    text = "Browse",
                                    style = typography.metaMedium,
                                    color = colors.primary,
                                )
                            }
                        }
                    }

                    // Pinned Projects
                    items(filteredHostProjects, key = { "pin_${it.projectId}" }) { proj ->
                        val primaryDir = proj.directories.firstOrNull() ?: "~"
                        val isCurrent = OrbitFormatters.pathsEquivalent(primaryDir, ui.activeCwd)
                        val liveBranch = resolveLiveWorkspaceBranch(
                            cwd = primaryDir,
                            activeCwd = ui.activeCwd,
                            liveGitBranch = liveActiveBranch,
                            discoveredProjects = allDiscovered,
                            workspaces = allWorkspaces,
                        )
                        WorkspaceDrawerRow(
                            title = proj.name,
                            subtitle = liveBranch?.let { "$it · ${OrbitFormatters.shortPath(primaryDir)}" }
                                ?: primaryDir,
                            useCodeSubtitle = true,
                            isCurrent = isCurrent,
                            isPinned = true,
                            onSelect = { onSelectProjectDir(primaryDir) },
                            onNewSessionHere = { onNewSessionInWorkspace(primaryDir, null) },
                            onTogglePin = { onUnpinProject(proj.projectId) },
                            onInspectGitDiff = { onInspectGitDiffAtCwd(primaryDir) },
                            onOpenTerminalHere = { onOpenTerminalAtCwd(primaryDir) },
                            onCopyPath = { copyPathToClipboard(primaryDir) },
                        )
                    }

                    // Active Workspaces / Worktrees not in pinned or discovered
                    items(filteredExtraWorkspaces, key = { "ws_${it.workspaceId}" }) { ws ->
                        val wsTitle = ws.title.ifBlank { OrbitFormatters.shortPath(ws.cwd) }
                        val isCurrent = OrbitFormatters.pathsEquivalent(ws.cwd, ui.activeCwd)
                        val liveBranch = resolveLiveWorkspaceBranch(
                            cwd = ws.cwd,
                            activeCwd = ui.activeCwd,
                            liveGitBranch = liveActiveBranch,
                            discoveredProjects = allDiscovered,
                            workspaces = allWorkspaces,
                        )
                        WorkspaceDrawerRow(
                            title = wsTitle,
                            subtitle = liveBranch,
                            useCodeSubtitle = false,
                            isCurrent = isCurrent,
                            isPinned = false,
                            onSelect = { onSelectProjectDir(ws.cwd) },
                            onNewSessionHere = { onNewSessionInWorkspace(ws.cwd, null) },
                            onTogglePin = { onPinProject(wsTitle, ws.cwd) },
                            onInspectGitDiff = { onInspectGitDiffAtCwd(ws.cwd) },
                            onOpenTerminalHere = { onOpenTerminalAtCwd(ws.cwd) },
                            onCopyPath = { copyPathToClipboard(ws.cwd) },
                        )
                    }

                    // Discovered Git Repos
                    items(discoveredRepos, key = { "disc_${it.path}" }) { repo ->
                        val isCurrent = OrbitFormatters.pathsEquivalent(repo.path, ui.activeCwd)
                        val liveBranch = resolveLiveWorkspaceBranch(
                            cwd = repo.path,
                            activeCwd = ui.activeCwd,
                            liveGitBranch = liveActiveBranch,
                            discoveredProjects = allDiscovered,
                            workspaces = allWorkspaces,
                        )
                        WorkspaceDrawerRow(
                            title = repo.name,
                            subtitle = liveBranch,
                            useCodeSubtitle = false,
                            isCurrent = isCurrent,
                            isPinned = false,
                            onSelect = { onSelectProjectDir(repo.path) },
                            onNewSessionHere = { onNewSessionInWorkspace(repo.path, null) },
                            onTogglePin = { onPinProject(repo.name, repo.path) },
                            onInspectGitDiff = { onInspectGitDiffAtCwd(repo.path) },
                            onOpenTerminalHere = { onOpenTerminalAtCwd(repo.path) },
                            onCopyPath = { copyPathToClipboard(repo.path) },
                        )
                    }
                }

                // 3. Sessions: Needs Approval / Active / Recent
                if (needsApprovalAgents.isNotEmpty()) {
                    item(key = "sec_needs_approval") {
                        Spacer(modifier = Modifier.height(OrbitSpacing.sm))
                        DrawerSectionLabel(
                            title = "Needs Approval (${needsApprovalAgents.size})",
                            accent = colors.warning,
                        )
                    }
                    items(needsApprovalAgents, key = { "appr_${it.agentId}" }) { ag ->
                        AgentSessionDrawerRow(
                            agent = ag,
                            selected = !ui.isNewSessionMode && ag.agentId == ui.selectedAgentId,
                            onSelect = { onSelectAgent(ag.agentId) },
                            onNewSessionInWorkspace = { onNewSessionInWorkspace(ag.cwd, ag.provider) },
                            onOpenTerminalHere = { onOpenTerminalAtCwd(ag.cwd) },
                            onCopyWorkspacePath = { copyPathToClipboard(ag.cwd) },
                            onArchive = { onArchiveAgent(ag.agentId) },
                        )
                    }
                }

                if (activeAgents.isNotEmpty()) {
                    item(key = "sec_active") {
                        Spacer(modifier = Modifier.height(OrbitSpacing.sm))
                        DrawerSectionLabel(title = "Active (${activeAgents.size})")
                    }
                    items(activeAgents, key = { "act_${it.agentId}" }) { ag ->
                        AgentSessionDrawerRow(
                            agent = ag,
                            selected = !ui.isNewSessionMode && ag.agentId == ui.selectedAgentId,
                            onSelect = { onSelectAgent(ag.agentId) },
                            onNewSessionInWorkspace = { onNewSessionInWorkspace(ag.cwd, ag.provider) },
                            onOpenTerminalHere = { onOpenTerminalAtCwd(ag.cwd) },
                            onCopyWorkspacePath = { copyPathToClipboard(ag.cwd) },
                            onArchive = { onArchiveAgent(ag.agentId) },
                        )
                    }
                }

                if (activeFilter == DrawerFilterChip.ALL) {
                    item(key = "sec_recent") {
                        Spacer(modifier = Modifier.height(OrbitSpacing.sm))
                        DrawerSectionLabel(title = "Recent (${recentAgents.size})")
                    }
                }

                val noMatchingSessions = recentAgents.isEmpty() &&
                    activeAgents.isEmpty() &&
                    needsApprovalAgents.isEmpty()

                if (noMatchingSessions) {
                    item(key = "empty_sessions") {
                        val isFiltered = normalizedQuery.isNotEmpty() || activeFilter != DrawerFilterChip.ALL
                        val emptyHint = when {
                            normalizedQuery.isNotEmpty() -> "No matching sessions."
                            activeFilter == DrawerFilterChip.ACTIVE -> "No active sessions right now."
                            activeFilter == DrawerFilterChip.APPROVALS -> "No pending approvals."
                            else -> "No sessions yet. Start one from the composer."
                        }
                        Column(
                            modifier = Modifier.padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                        ) {
                            Text(
                                text = emptyHint,
                                style = typography.meta,
                                color = colors.fgMuted,
                            )
                            if (isFiltered) {
                                Text(
                                    text = "Reset filters",
                                    style = typography.metaMedium,
                                    color = colors.primary,
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(OrbitSpacing.xs))
                                        .clickable {
                                            onSearchQueryChange("")
                                            activeFilter = DrawerFilterChip.ALL
                                        }
                                        .padding(vertical = OrbitSpacing.xs),
                                )
                            }
                        }
                    }
                } else {
                    items(recentAgents, key = { "rec_${it.agentId}" }) { ag ->
                        AgentSessionDrawerRow(
                            agent = ag,
                            selected = !ui.isNewSessionMode && ag.agentId == ui.selectedAgentId,
                            onSelect = { onSelectAgent(ag.agentId) },
                            onNewSessionInWorkspace = { onNewSessionInWorkspace(ag.cwd, ag.provider) },
                            onOpenTerminalHere = { onOpenTerminalAtCwd(ag.cwd) },
                            onCopyWorkspacePath = { copyPathToClipboard(ag.cwd) },
                            onArchive = { onArchiveAgent(ag.agentId) },
                        )
                    }
                }
            }
        }

        HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

        // Bottom Actions: Pair Host + Settings (concise, non-truncating)
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            val btnShape = RoundedCornerShape(OrbitSpacing.md)
            Row(
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = OrbitSpacing.touchTarget)
                    .clip(btnShape)
                    .background(colors.subtleFill, btnShape)
                    .border(OrbitSpacing.hairline, colors.hairline, btnShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onOpenPairHost()
                    }
                    .padding(horizontal = OrbitSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.QrCodeScanner,
                    contentDescription = null,
                    tint = colors.fgPrimary,
                    modifier = Modifier.size(OrbitSpacing.iconSm),
                )
                Spacer(modifier = Modifier.width(OrbitSpacing.xs))
                Text(
                    text = "Pair Host",
                    style = typography.subheadMedium,
                    color = colors.fgPrimary,
                    maxLines = 1,
                    softWrap = false,
                )
            }

            Row(
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = OrbitSpacing.touchTarget)
                    .clip(btnShape)
                    .background(colors.subtleFill, btnShape)
                    .border(OrbitSpacing.hairline, colors.hairline, btnShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onOpenSettings()
                    }
                    .padding(horizontal = OrbitSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Settings,
                    contentDescription = null,
                    tint = colors.fgPrimary,
                    modifier = Modifier.size(OrbitSpacing.iconSm),
                )
                Spacer(modifier = Modifier.width(OrbitSpacing.xs))
                Text(
                    text = "Settings",
                    style = typography.subheadMedium,
                    color = colors.fgPrimary,
                    maxLines = 1,
                    softWrap = false,
                )
            }
        }
    }
}

@Composable
private fun DrawerSectionLabel(
    title: String,
    accent: Color = OrbitTheme.colors.fgMuted,
) {
    Text(
        text = title,
        style = OrbitTheme.typography.metaSemiBold,
        color = accent,
        modifier = Modifier.padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
    )
}

@Composable
private fun HostSwitcherCard(
    host: FfiHostCard,
    selected: Boolean,
    onClick: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val shape = RoundedCornerShape(OrbitSpacing.md)

    val statusColor = when (host.connectionState) {
        FfiConnectionState.ONLINE_DIRECT -> colors.directAdded
        FfiConnectionState.ONLINE_RELAY -> colors.relay
        FfiConnectionState.CONNECTING_DIRECT, FfiConnectionState.CONNECTING_RELAY -> colors.warning
        FfiConnectionState.DISCONNECTED -> colors.fgMuted
    }
    val routeBadge = OrbitFormatters.connectionLabel(host.connectionState, host.viaSshTunnel)

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(if (selected) colors.elevated else Color.Transparent, shape)
            .border(
                OrbitSpacing.hairline,
                if (selected) colors.primary.copy(alpha = 0.65f) else colors.hairline,
                shape
            )
            .clickable { onClick() }
            .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                modifier = Modifier.weight(1f),
            ) {
                Box(
                    modifier = Modifier
                        .size(6.dp)
                        .background(statusColor, CircleShape)
                )
                Text(
                    text = host.displayName,
                    style = typography.subheadSemiBold,
                    color = colors.fgPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }

            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                host.lastPingMs?.let { rtt ->
                    Text(
                        text = "${rtt}ms",
                        style = typography.meta,
                        color = colors.fgMuted,
                    )
                }
                Text(
                    text = routeBadge,
                    style = typography.metaMedium,
                    color = statusColor,
                )
            }
        }

        if (host.pendingApprovalCount > 0u || host.agentCount > 0u) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "${host.agentCount} ${if (host.agentCount == 1u) "session" else "sessions"}",
                    style = typography.meta,
                    color = colors.fgMuted,
                )
                if (host.pendingApprovalCount > 0u) {
                    Text(
                        text = "${host.pendingApprovalCount} awaiting approval",
                        style = typography.metaMedium,
                        color = colors.warning,
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun WorkspaceDrawerRow(
    title: String,
    subtitle: String?,
    useCodeSubtitle: Boolean,
    isCurrent: Boolean,
    isPinned: Boolean,
    onSelect: () -> Unit,
    onNewSessionHere: () -> Unit,
    onTogglePin: () -> Unit,
    onInspectGitDiff: () -> Unit,
    onOpenTerminalHere: () -> Unit,
    onCopyPath: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val rowShape = RoundedCornerShape(OrbitSpacing.sm)
    var menuExpanded by remember { mutableStateOf(false) }

    Box(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 44.dp)
                .clip(rowShape)
                .background(if (isCurrent) colors.subtleFill else Color.Transparent, rowShape)
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
                .padding(start = OrbitSpacing.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            if (isPinned) {
                Icon(
                    imageVector = Icons.Outlined.Bookmark,
                    contentDescription = null,
                    tint = colors.primary,
                    modifier = Modifier.size(OrbitSpacing.iconSm),
                )
            }
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(vertical = OrbitSpacing.xs),
            ) {
                Text(
                    text = title,
                    style = typography.subheadMedium,
                    color = colors.fgPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                subtitle?.takeIf { it.isNotBlank() }?.let { sub ->
                    Text(
                        text = sub,
                        style = if (useCodeSubtitle) typography.code else typography.meta,
                        color = colors.fgMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            Box(
                modifier = Modifier
                    .size(44.dp)
                    .clip(CircleShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onTogglePin()
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = if (isPinned) Icons.Outlined.Close else Icons.Outlined.BookmarkBorder,
                    contentDescription = if (isPinned) "Unpin workspace" else "Pin workspace",
                    tint = colors.fgMuted,
                    modifier = Modifier.size(OrbitSpacing.iconSm),
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
                text = {
                    Text(
                        text = if (isPinned) "Unpin Workspace" else "Pin Workspace",
                        style = typography.subheadMedium,
                        color = colors.fgPrimary,
                    )
                },
                leadingIcon = {
                    Icon(
                        imageVector = if (isPinned) Icons.Outlined.Close else Icons.Outlined.BookmarkBorder,
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
                text = { Text("Open Terminal Here", style = typography.subheadMedium, color = colors.fgPrimary) },
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
                text = { Text("Copy Path", style = typography.subheadMedium, color = colors.fgPrimary) },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Outlined.ContentCopy,
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
private fun AgentSessionDrawerRow(
    agent: FfiAgentCard,
    selected: Boolean,
    onSelect: () -> Unit,
    onNewSessionInWorkspace: () -> Unit,
    onOpenTerminalHere: () -> Unit,
    onCopyWorkspacePath: () -> Unit,
    onArchive: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val shape = RoundedCornerShape(OrbitSpacing.sm)
    var menuExpanded by remember { mutableStateOf(false) }

    Box(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = OrbitSpacing.touchTarget)
                .clip(shape)
                .background(if (selected) colors.elevated else Color.Transparent, shape)
                .then(
                    if (selected) Modifier.border(OrbitSpacing.hairline, colors.hairline, shape)
                    else Modifier
                )
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
                .padding(start = OrbitSpacing.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            when {
                agent.isAwaitingPermission -> {
                    Box(
                        modifier = Modifier
                            .size(6.dp)
                            .background(colors.warning, CircleShape)
                    )
                }
                agent.isRunning -> {
                    BreathingDot(color = colors.primary)
                }
            }

            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(vertical = OrbitSpacing.xs),
            ) {
                Text(
                    text = agent.title.ifBlank { "Untitled Session" },
                    style = if (selected) typography.subheadSemiBold else typography.subheadMedium,
                    color = colors.fgPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = "${OrbitFormatters.shortPath(agent.cwd)} · ${agent.providerLabel}",
                    style = typography.meta,
                    color = colors.fgMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }

            Box(
                modifier = Modifier
                    .size(44.dp)
                    .clip(CircleShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        onArchive()
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Archive,
                    contentDescription = "Archive session",
                    tint = colors.fgMuted,
                    modifier = Modifier.size(OrbitSpacing.iconSm),
                )
            }
        }

        DropdownMenu(
            expanded = menuExpanded,
            onDismissRequest = { menuExpanded = false },
            containerColor = colors.elevated,
        ) {
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
                text = { Text("Open Terminal Here", style = typography.subheadMedium, color = colors.fgPrimary) },
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
                        imageVector = Icons.Outlined.ContentCopy,
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
