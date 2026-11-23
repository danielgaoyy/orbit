package dev.orbit.android.ui.sheets

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.outlined.MergeType
import androidx.compose.material.icons.outlined.AccountTree
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.Forum
import androidx.compose.material.icons.outlined.Group
import androidx.compose.material.icons.outlined.Layers
import androidx.compose.material.icons.outlined.PersonAdd
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.TrackChanges
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
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
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.stage.MarkdownBlocks
import dev.orbit.android.ui.theme.OrbitProviderIcon
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiAgentCard
import uniffi.orbit_ffi_uniffi.FfiWorkspaceBoardSnapshot
import uniffi.orbit_ffi_uniffi.FfiWorkspaceEvent
import uniffi.orbit_ffi_uniffi.FfiWorkspaceEventKind
import uniffi.orbit_ffi_uniffi.FfiWorkspaceSummary
import uniffi.orbit_ffi_uniffi.parseMarkdownPreview

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WorkspaceBoardSheet(
    workspaces: List<FfiWorkspaceSummary>,
    initialWorkspaceId: String?,
    allAgents: List<FfiAgentCard>,
    selectedAgentId: String?,
    boardsByWorkspaceId: Map<String, FfiWorkspaceBoardSnapshot>,
    eventsByWorkspaceId: Map<String, List<FfiWorkspaceEvent>>,
    onFetchBoardAndEvents: (workspaceId: String) -> Unit,
    onUpdateGoal: (workspaceId: String, goal: String) -> Unit,
    onUpdateBoard: (workspaceId: String, content: String) -> Unit,
    onSelectPeer: (agentId: String) -> Unit,
    onSpawnPeer: (workspaceId: String, handle: String, prompt: String, worktree: Boolean) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val groupedBg = if (colors.isDark) colors.canvas else colors.drawer
    val cardShape = RoundedCornerShape(14.dp)

    var selectedWorkspaceId by remember(initialWorkspaceId) {
        mutableStateOf(
            initialWorkspaceId?.takeIf { id -> workspaces.any { it.workspaceId == id } }
                ?: workspaces.firstOrNull()?.workspaceId
        )
    }

    LaunchedEffect(workspaces) {
        val current = selectedWorkspaceId
        if (current == null || workspaces.none { it.workspaceId == current }) {
            selectedWorkspaceId = initialWorkspaceId?.takeIf { id -> workspaces.any { it.workspaceId == id } }
                ?: workspaces.firstOrNull()?.workspaceId
        }
    }

    val workspace = remember(workspaces, selectedWorkspaceId) {
        workspaces.firstOrNull { it.workspaceId == selectedWorkspaceId }
    }
    val board = remember(boardsByWorkspaceId, selectedWorkspaceId) {
        selectedWorkspaceId?.let { boardsByWorkspaceId[it] }
    }
    val events = remember(eventsByWorkspaceId, selectedWorkspaceId) {
        selectedWorkspaceId?.let { eventsByWorkspaceId[it] }.orEmpty()
    }
    val peers = remember(allAgents, selectedWorkspaceId) {
        if (selectedWorkspaceId == null) {
            emptyList()
        } else {
            allAgents.filter { ag ->
                ag.workspaceId == selectedWorkspaceId &&
                    ag.archivedAtRfc3339 == null &&
                    !(ag.parentAgentId != null && ag.parentHostId == null)
            }
        }
    }
    val conflictEvents = remember(events) {
        events.filter { it.kind == FfiWorkspaceEventKind.CONFLICT_WARNING }
    }

    var isEditingGoal by remember(selectedWorkspaceId) { mutableStateOf(false) }
    var goalDraft by remember(selectedWorkspaceId) {
        mutableStateOf(workspace?.missionGoal.orEmpty())
    }
    var isEditingBoard by remember(selectedWorkspaceId) { mutableStateOf(false) }
    var boardDraft by remember(selectedWorkspaceId) {
        mutableStateOf(board?.content.orEmpty())
    }
    var isTimelineExpanded by remember { mutableStateOf(false) }
    var showAddPeerForm by remember(selectedWorkspaceId) { mutableStateOf(false) }
    var peerHandleDraft by remember { mutableStateOf("") }
    var peerPromptDraft by remember { mutableStateOf("") }
    var peerWorktree by remember { mutableStateOf(false) }

    LaunchedEffect(selectedWorkspaceId) {
        selectedWorkspaceId?.let { onFetchBoardAndEvents(it) }
    }

    LaunchedEffect(workspace?.missionGoal, isEditingGoal) {
        if (!isEditingGoal) {
            goalDraft = workspace?.missionGoal.orEmpty()
        }
    }

    LaunchedEffect(board?.revision, board?.content, isEditingBoard) {
        if (!isEditingBoard) {
            boardDraft = board?.content.orEmpty()
        }
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
                .fillMaxHeight(0.88f)
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
                    imageVector = Icons.Outlined.Layers,
                    contentDescription = null,
                    tint = colors.relay,
                    modifier = Modifier.size(18.dp),
                )
                Text(
                    text = "Mission Board & Peers",
                    style = typography.title,
                    color = colors.fgPrimary,
                    modifier = Modifier.weight(1f),
                )
                if (selectedWorkspaceId != null) {
                    Box(
                        modifier = Modifier
                            .size(36.dp)
                            .clip(CircleShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                selectedWorkspaceId?.let { onFetchBoardAndEvents(it) }
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Refresh,
                            contentDescription = "Refresh board",
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

            // Optional Workspace selector strip when >1 workspace exists
            if (workspaces.size > 1) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .horizontalScroll(rememberScrollState())
                        .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs),
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    workspaces.forEach { ws ->
                        val active = ws.workspaceId == selectedWorkspaceId
                        Box(
                            modifier = Modifier
                                .clip(CircleShape)
                                .background(if (active) colors.primary else colors.subtleFill, CircleShape)
                                .clickable { selectedWorkspaceId = ws.workspaceId }
                                .padding(horizontal = 12.dp, vertical = 6.dp),
                        ) {
                            Text(
                                text = ws.title.ifBlank { OrbitFormatters.shortPath(ws.cwd) },
                                style = typography.metaSemiBold,
                                color = if (active) colors.onPrimary else colors.fgSecondary,
                                maxLines = 1,
                            )
                        }
                    }
                }
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            if (workspace == null) {
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
                            imageVector = Icons.Outlined.Layers,
                            contentDescription = null,
                            tint = colors.fgMuted,
                            modifier = Modifier.size(28.dp),
                        )
                        Text(
                            text = "No Active Mission Workspace",
                            style = typography.bodySemiBold,
                            color = colors.fgPrimary,
                        )
                        Text(
                            text = "Start a conversation to create a workspace with a shared BOARD.md and sibling peer roster.",
                            style = typography.subhead,
                            color = colors.fgSecondary,
                            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                        )
                    }
                }
            } else {
                Column(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth()
                        .verticalScroll(rememberScrollState())
                        .padding(OrbitSpacing.lg),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                ) {
                    // 1. Workspace Header Card
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(cardShape)
                            .background(colors.elevated, cardShape)
                            .border(OrbitSpacing.hairline, colors.hairline, cardShape)
                            .padding(OrbitSpacing.md),
                        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                    ) {
                        Text(
                            text = workspace.title.ifBlank { OrbitFormatters.shortPath(workspace.cwd) },
                            style = typography.bodySemiBold,
                            color = colors.fgPrimary,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Row(
                            modifier = Modifier.horizontalScroll(rememberScrollState()),
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                        ) {
                            BoardStatChip(
                                icon = Icons.Outlined.Group,
                                label = "${peers.size} ${if (peers.size == 1) "peer" else "peers"}",
                            )
                            BoardStatChip(
                                icon = Icons.Outlined.Description,
                                label = "v${board?.revision ?: workspace.boardRevision}",
                            )
                            workspace.gitBranch?.takeIf { it.isNotBlank() }?.let { branch ->
                                BoardStatChip(icon = Icons.Outlined.AccountTree, label = branch)
                            }
                        }
                    }

                    // 2. Editable Mission Goal Card
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
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.TrackChanges,
                                contentDescription = null,
                                tint = colors.fgMuted,
                                modifier = Modifier.size(14.dp),
                            )
                            Spacer(modifier = Modifier.size(6.dp))
                            Text(
                                text = "Mission Goal",
                                style = typography.metaSemiBold,
                                color = colors.fgMuted,
                                modifier = Modifier.weight(1f),
                            )
                            Text(
                                text = if (isEditingGoal) "Save" else "Edit",
                                style = typography.metaSemiBold,
                                color = colors.primary,
                                modifier = Modifier
                                    .clip(RoundedCornerShape(8.dp))
                                    .clickable {
                                        if (isEditingGoal) {
                                            onUpdateGoal(workspace.workspaceId, goalDraft.trim())
                                            isEditingGoal = false
                                        } else {
                                            goalDraft = workspace.missionGoal.orEmpty()
                                            isEditingGoal = true
                                        }
                                    }
                                    .padding(horizontal = 6.dp, vertical = 2.dp),
                            )
                        }

                        if (isEditingGoal) {
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(10.dp))
                                    .background(colors.subtleFill)
                                    .padding(10.dp),
                            ) {
                                if (goalDraft.isEmpty()) {
                                    Text(
                                        text = "Define the shared mission objective…",
                                        style = typography.subhead,
                                        color = colors.fgMuted,
                                    )
                                }
                                BasicTextField(
                                    value = goalDraft,
                                    onValueChange = { goalDraft = it },
                                    textStyle = typography.subhead.copy(color = colors.fgPrimary),
                                    cursorBrush = SolidColor(colors.primary),
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            }
                        } else {
                            val goalText = workspace.missionGoal?.trim().orEmpty()
                            Text(
                                text = goalText.ifEmpty { "No mission goal set yet. Tap Edit to align all peers." },
                                style = typography.subhead,
                                color = if (goalText.isEmpty()) colors.fgMuted else colors.fgPrimary,
                            )
                        }
                    }

                    // 3. File Conflict Radar (conditional)
                    if (conflictEvents.isNotEmpty()) {
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(cardShape)
                                .background(colors.warning.copy(alpha = if (colors.isDark) 0.14f else 0.08f), cardShape)
                                .border(OrbitSpacing.hairline, colors.warning.copy(alpha = 0.4f), cardShape)
                                .padding(OrbitSpacing.md),
                            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                        ) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                Icon(
                                    imageVector = Icons.Outlined.WarningAmber,
                                    contentDescription = null,
                                    tint = colors.warning,
                                    modifier = Modifier.size(15.dp),
                                )
                                Text(
                                    text = "File Conflict Radar (${conflictEvents.size})",
                                    style = typography.subheadSemiBold,
                                    color = colors.warning,
                                )
                            }
                            conflictEvents.takeLast(5).forEach { ev ->
                                Text(
                                    text = ev.summary,
                                    style = typography.meta,
                                    color = colors.fgPrimary,
                                )
                            }
                        }
                    }

                    // 4. Sibling Peers Roster Card
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
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                text = "Peers (${peers.size})",
                                style = typography.metaSemiBold,
                                color = colors.fgMuted,
                                modifier = Modifier.weight(1f),
                            )
                            Row(
                                modifier = Modifier
                                    .clip(RoundedCornerShape(8.dp))
                                    .clickable { showAddPeerForm = !showAddPeerForm }
                                    .padding(horizontal = 6.dp, vertical = 2.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(3.dp),
                            ) {
                                Icon(
                                    imageVector = if (showAddPeerForm) Icons.Outlined.Close else Icons.Outlined.Add,
                                    contentDescription = null,
                                    tint = colors.primary,
                                    modifier = Modifier.size(14.dp),
                                )
                                Text(
                                    text = if (showAddPeerForm) "Cancel" else "Add Peer",
                                    style = typography.metaSemiBold,
                                    color = colors.primary,
                                )
                            }
                        }

                        AnimatedVisibility(visible = showAddPeerForm) {
                            Column(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(10.dp))
                                    .background(colors.subtleFill)
                                    .padding(OrbitSpacing.sm),
                                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                            ) {
                                BasicTextField(
                                    value = peerHandleDraft,
                                    onValueChange = { peerHandleDraft = it },
                                    singleLine = true,
                                    textStyle = typography.code.copy(color = colors.fgPrimary),
                                    cursorBrush = SolidColor(colors.primary),
                                    modifier = Modifier.fillMaxWidth(),
                                    decorationBox = { inner ->
                                        if (peerHandleDraft.isEmpty()) {
                                            Text("@handle (e.g. reviewer, tester)", style = typography.code, color = colors.fgMuted)
                                        }
                                        inner()
                                    },
                                )
                                HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                                BasicTextField(
                                    value = peerPromptDraft,
                                    onValueChange = { peerPromptDraft = it },
                                    textStyle = typography.subhead.copy(color = colors.fgPrimary),
                                    cursorBrush = SolidColor(colors.primary),
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .heightIn(min = 56.dp),
                                    decorationBox = { inner ->
                                        if (peerPromptDraft.isEmpty()) {
                                            Text("Initial task for this peer…", style = typography.subhead, color = colors.fgMuted)
                                        }
                                        inner()
                                    },
                                )
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Text(
                                        text = "Isolated Git Worktree",
                                        style = typography.meta,
                                        color = colors.fgSecondary,
                                        modifier = Modifier.weight(1f),
                                    )
                                    Switch(
                                        checked = peerWorktree,
                                        onCheckedChange = { peerWorktree = it },
                                        colors = SwitchDefaults.colors(checkedTrackColor = colors.directAdded),
                                    )
                                }
                                val canSpawn = peerPromptDraft.isNotBlank()
                                Box(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .heightIn(min = 38.dp)
                                        .clip(RoundedCornerShape(8.dp))
                                        .background(if (canSpawn) colors.primary else colors.fgMuted.copy(alpha = 0.25f))
                                        .clickable(enabled = canSpawn) {
                                            onSpawnPeer(
                                                workspace.workspaceId,
                                                peerHandleDraft.trim().removePrefix("@"),
                                                peerPromptDraft.trim(),
                                                peerWorktree,
                                            )
                                            peerHandleDraft = ""
                                            peerPromptDraft = ""
                                            showAddPeerForm = false
                                        },
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Text("Spawn Peer", style = typography.metaSemiBold, color = colors.onPrimary)
                                }
                            }
                        }

                        if (peers.isEmpty()) {
                            Text(
                                text = "No active peers in this workspace yet.",
                                style = typography.subhead,
                                color = colors.fgMuted,
                            )
                        } else {
                            peers.forEachIndexed { idx, peer ->
                                if (idx > 0) {
                                    HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                                }
                                val isCurrent = peer.agentId == selectedAgentId
                                val statusDot = when {
                                    peer.isAwaitingPermission -> colors.warning
                                    peer.isRunning -> colors.directAdded
                                    else -> colors.fgMuted
                                }
                                Row(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .clip(RoundedCornerShape(10.dp))
                                        .clickable {
                                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                            onSelectPeer(peer.agentId)
                                            onDismiss()
                                        }
                                        .padding(vertical = 8.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                                ) {
                                    Box(modifier = Modifier.size(18.dp), contentAlignment = Alignment.Center) {
                                        when {
                                            peer.isRunning -> CircularProgressIndicator(
                                                modifier = Modifier.size(13.dp),
                                                strokeWidth = 1.8.dp,
                                                color = colors.primary,
                                            )
                                            peer.isAwaitingPermission -> Box(
                                                modifier = Modifier
                                                    .size(8.dp)
                                                    .clip(CircleShape)
                                                    .background(colors.warning),
                                            )
                                            else -> OrbitProviderIcon(provider = peer.provider, size = 15.dp)
                                        }
                                    }
                                    Column(
                                        modifier = Modifier.weight(1f),
                                        verticalArrangement = Arrangement.spacedBy(1.dp),
                                    ) {
                                        Row(
                                            verticalAlignment = Alignment.CenterVertically,
                                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                                        ) {
                                            val displayName = peer.handle?.takeIf { it.isNotBlank() }?.let { "@$it" }
                                                ?: peer.title.ifBlank { OrbitFormatters.providerLabel(peer.provider) }
                                            Text(
                                                text = displayName,
                                                style = typography.subheadSemiBold,
                                                color = colors.fgPrimary,
                                                maxLines = 1,
                                                overflow = TextOverflow.Ellipsis,
                                                modifier = Modifier.weight(1f, fill = false),
                                            )
                                            Box(
                                                modifier = Modifier
                                                    .size(6.dp)
                                                    .clip(CircleShape)
                                                    .background(statusDot),
                                            )
                                            if (isCurrent) {
                                                Box(
                                                    modifier = Modifier
                                                        .clip(CircleShape)
                                                        .background(colors.relay.copy(alpha = 0.16f), CircleShape)
                                                        .padding(horizontal = 6.dp, vertical = 1.dp),
                                                ) {
                                                    Text(text = "Current", style = typography.metaSemiBold, color = colors.relay)
                                                }
                                            }
                                        }
                                        Text(
                                            text = peer.taskSummary?.takeIf { it.isNotBlank() } ?: peer.statusLabel,
                                            style = typography.meta,
                                            color = colors.fgMuted,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                    Icon(
                                        imageVector = Icons.AutoMirrored.Outlined.KeyboardArrowRight,
                                        contentDescription = null,
                                        tint = colors.fgMuted,
                                        modifier = Modifier.size(16.dp),
                                    )
                                }
                            }
                        }
                    }

                    // 5. Shared Blackboard (`BOARD.md`) Viewer + Inline Editor
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
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Description,
                                contentDescription = null,
                                tint = colors.fgMuted,
                                modifier = Modifier.size(14.dp),
                            )
                            Text(
                                text = "Shared Board (BOARD.md)",
                                style = typography.metaSemiBold,
                                color = colors.fgMuted,
                                modifier = Modifier.weight(1f),
                            )
                            if (isEditingBoard) {
                                Text(
                                    text = "Cancel",
                                    style = typography.metaMedium,
                                    color = colors.fgMuted,
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(8.dp))
                                        .clickable {
                                            isEditingBoard = false
                                            boardDraft = board?.content.orEmpty()
                                        }
                                        .padding(horizontal = 6.dp, vertical = 2.dp),
                                )
                                Text(
                                    text = "Save",
                                    style = typography.metaSemiBold,
                                    color = colors.primary,
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(8.dp))
                                        .clickable {
                                            onUpdateBoard(workspace.workspaceId, boardDraft)
                                            isEditingBoard = false
                                        }
                                        .padding(horizontal = 6.dp, vertical = 2.dp),
                                )
                            } else {
                                Text(
                                    text = "Edit Markdown",
                                    style = typography.metaSemiBold,
                                    color = colors.primary,
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(8.dp))
                                        .clickable {
                                            boardDraft = board?.content.orEmpty()
                                            isEditingBoard = true
                                        }
                                        .padding(horizontal = 6.dp, vertical = 2.dp),
                                )
                            }
                        }

                        if (isEditingBoard) {
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .heightIn(min = 200.dp)
                                    .clip(RoundedCornerShape(10.dp))
                                    .background(colors.code)
                                    .border(OrbitSpacing.hairline, colors.hairline, RoundedCornerShape(10.dp))
                                    .padding(10.dp),
                            ) {
                                BasicTextField(
                                    value = boardDraft,
                                    onValueChange = { boardDraft = it },
                                    textStyle = typography.code.copy(color = colors.fgPrimary),
                                    cursorBrush = SolidColor(colors.primary),
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            }
                        } else {
                            val raw = board?.content?.trim().orEmpty()
                            if (raw.isEmpty()) {
                                Text(
                                    text = "No shared notes recorded yet. Agents and users can post decisions, contracts, and findings here.",
                                    style = typography.subhead,
                                    color = colors.fgMuted,
                                )
                            } else {
                                val blocks = remember(raw) {
                                    runCatching { parseMarkdownPreview(raw, false) }.getOrDefault(emptyList())
                                }
                                MarkdownBlocks(blocks = blocks, fallbackText = raw)
                            }
                        }
                    }

                    // 6. Collapsible Activity Timeline
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
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { isTimelineExpanded = !isTimelineExpanded },
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                text = "Activity Timeline (${events.size})",
                                style = typography.metaSemiBold,
                                color = colors.fgMuted,
                                modifier = Modifier.weight(1f),
                            )
                            Text(
                                text = if (isTimelineExpanded) "Hide" else "Show",
                                style = typography.metaMedium,
                                color = colors.fgSecondary,
                            )
                        }

                        AnimatedVisibility(visible = isTimelineExpanded) {
                            if (events.isEmpty()) {
                                Text(
                                    text = "No workspace events recorded yet.",
                                    style = typography.meta,
                                    color = colors.fgMuted,
                                )
                            } else {
                                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                    events.asReversed().take(25).forEach { ev ->
                                        Row(
                                            verticalAlignment = Alignment.Top,
                                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                                        ) {
                                            Icon(
                                                imageVector = workspaceEventIcon(ev.kind),
                                                contentDescription = null,
                                                tint = if (ev.kind == FfiWorkspaceEventKind.CONFLICT_WARNING) {
                                                    colors.warning
                                                } else {
                                                    colors.fgMuted
                                                },
                                                modifier = Modifier
                                                    .padding(top = 2.dp)
                                                    .size(14.dp),
                                            )
                                            Column(modifier = Modifier.weight(1f)) {
                                                Text(
                                                    text = ev.summary,
                                                    style = typography.meta,
                                                    color = colors.fgPrimary,
                                                )
                                                Text(
                                                    text = OrbitFormatters.compactRelativeTime(ev.createdAtRfc3339),
                                                    style = typography.meta,
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
            }
        }
    }
}

@Composable
private fun BoardStatChip(icon: ImageVector, label: String) {
    val colors = OrbitTheme.colors
    Row(
        modifier = Modifier
            .clip(CircleShape)
            .background(colors.subtleFill, CircleShape)
            .padding(horizontal = 8.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            tint = colors.fgSecondary,
            modifier = Modifier.size(12.dp),
        )
        Text(
            text = label,
            style = OrbitTheme.typography.metaMedium,
            color = colors.fgSecondary,
            maxLines = 1,
        )
    }
}

private fun workspaceEventIcon(kind: FfiWorkspaceEventKind): ImageVector = when (kind) {
    FfiWorkspaceEventKind.PEER_JOINED -> Icons.Outlined.PersonAdd
    FfiWorkspaceEventKind.PEER_FINISHED -> Icons.Outlined.CheckCircle
    FfiWorkspaceEventKind.BOARD_UPDATED -> Icons.Outlined.Description
    FfiWorkspaceEventKind.PEER_MESSAGE -> Icons.Outlined.Forum
    FfiWorkspaceEventKind.FILE_EDITED -> Icons.Outlined.Edit
    FfiWorkspaceEventKind.CONFLICT_WARNING -> Icons.Outlined.WarningAmber
    FfiWorkspaceEventKind.WORKSPACE_MERGED -> Icons.AutoMirrored.Outlined.MergeType
}
