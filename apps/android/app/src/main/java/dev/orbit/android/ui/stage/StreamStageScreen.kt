package dev.orbit.android.ui.stage

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AccountTree
import androidx.compose.material.icons.outlined.ArrowDownward
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material.icons.outlined.QrCodeScanner
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.orbit.android.store.OrbitAndroidStore
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.store.OrbitUiState
import dev.orbit.android.ui.composer.OmniComposer
import dev.orbit.android.ui.theme.OrbitCompactDropdownMenu
import dev.orbit.android.ui.theme.OrbitMenuOptionRow
import dev.orbit.android.ui.theme.OrbitMenuScrollContainer
import dev.orbit.android.ui.theme.OrbitMenuSectionHeader
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import kotlinx.coroutines.launch
import uniffi.orbit_ffi_uniffi.FfiConnectionState

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StreamStageScreen(
    ui: OrbitUiState,
    store: OrbitAndroidStore,
    onOpenContextPicker: () -> Unit,
    onOpenModelSelector: () -> Unit,
    onOpenDiffForFile: (String?) -> Unit,
    onOpenPairSheet: () -> Unit,
    onOpenTerminal: () -> Unit = {},
    onOpenSettings: () -> Unit = {},
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val context = LocalContext.current
    val haptics = LocalHapticFeedback.current
    val scope = rememberCoroutineScope()
    val pullRefreshState = rememberPullToRefreshState()

    val stage = if (ui.isNewSessionMode) null else ui.agentStage
    val turns = stage?.turns.orEmpty()
    val isActiveAgentRunning = ui.isActiveAgentRunning
    val listState = rememberLazyListState()
    val focusManager = LocalFocusManager.current
    val keyboardController = LocalSoftwareKeyboardController.current

    LaunchedEffect(listState.isScrollInProgress) {
        if (listState.isScrollInProgress) {
            focusManager.clearFocus()
            keyboardController?.hide()
        }
    }

    val canScrollForward by remember {
        derivedStateOf { listState.canScrollForward }
    }

    val mergedPermissions = remember(stage?.pendingPermissions, ui.demoPermissions) {
        stage?.pendingPermissions.orEmpty() + ui.demoPermissions
    }
    val mergedQuestions = remember(stage?.pendingQuestions, ui.demoQuestions) {
        stage?.pendingQuestions.orEmpty() + ui.demoQuestions
    }
    val activeDecisionCount = mergedPermissions.size + mergedQuestions.size

    // Track whether the user was anchored at the bottom before viewport height shrinks
    // (e.g. when StickyDecisionBar or IME keyboard opens) so the latest turn stays snug!
    var stageViewportHeightPx by remember { mutableIntStateOf(0) }
    var prevStageViewportHeightPx by remember { mutableIntStateOf(0) }
    var anchoredAtBottom by remember { mutableStateOf(true) }

    LaunchedEffect(canScrollForward, listState.isScrollInProgress) {
        if (listState.isScrollInProgress || stageViewportHeightPx == prevStageViewportHeightPx) {
            anchoredAtBottom = !canScrollForward
        }
    }

    LaunchedEffect(stageViewportHeightPx, activeDecisionCount) {
        val shrank = prevStageViewportHeightPx > 0 &&
            stageViewportHeightPx in 1 until prevStageViewportHeightPx
        prevStageViewportHeightPx = stageViewportHeightPx
        if (turns.isNotEmpty() && anchoredAtBottom && (shrank || activeDecisionCount > 0)) {
            listState.scrollToItem(turns.lastIndex, scrollOffset = 100_000)
        }
    }

    // Smart auto-scroll to bottom when pinned at bottom and stage sequence advances
    val stageSeq = stage?.latestSeq ?: 0uL
    val turnCount = turns.size
    LaunchedEffect(ui.selectedAgentId) {
        anchoredAtBottom = true
        if (turns.isNotEmpty()) {
            listState.scrollToItem(turns.lastIndex, scrollOffset = 100_000)
        }
    }
    LaunchedEffect(stageSeq, turnCount) {
        if (turns.isNotEmpty() && !canScrollForward) {
            listState.animateScrollToItem(turns.lastIndex, scrollOffset = 100_000)
        }
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(colors.canvas),
    ) {
        // Error Banner
        ui.lastError?.let { err ->
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs)
                    .background(colors.diffRemovedBg, RoundedCornerShape(OrbitSpacing.sm))
                    .border(OrbitSpacing.hairline, colors.dangerRemoved.copy(alpha = 0.45f), RoundedCornerShape(OrbitSpacing.sm))
                    .padding(start = OrbitSpacing.md),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    text = err,
                    style = typography.subhead,
                    color = colors.dangerRemoved,
                    modifier = Modifier
                        .weight(1f)
                        .padding(vertical = OrbitSpacing.sm),
                )
                Box(
                    modifier = Modifier
                        .size(44.dp)
                        .clickable { store.dismissError() },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Close,
                        contentDescription = "Dismiss error",
                        tint = colors.dangerRemoved,
                        modifier = Modifier.size(OrbitSpacing.iconMd),
                    )
                }
            }
        }

        // Main Editorial Stage Area with Native Pull-to-Refresh
        Box(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .pointerInput(Unit) {
                    awaitEachGesture {
                        awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                        val up = waitForUpOrCancellation(pass = PointerEventPass.Initial)
                        if (up != null) {
                            focusManager.clearFocus()
                            keyboardController?.hide()
                        }
                    }
                }
                .onSizeChanged { size ->
                    stageViewportHeightPx = size.height
                },
        ) {
            if (ui.hosts.isEmpty()) {
                UnpairedWelcomeHero(
                    localDaemonAvailable = ui.localDaemonUri != null,
                    onOpenPairSheet = onOpenPairSheet,
                    onConnectLocal = { store.discoverLocalDaemon() },
                    modifier = Modifier.align(Alignment.Center),
                )
            } else {
                PullToRefreshBox(
                    isRefreshing = ui.isRefreshingStage,
                    onRefresh = { store.refreshStageAndHost() },
                    state = pullRefreshState,
                    modifier = Modifier.fillMaxSize(),
                    indicator = {
                        PullToRefreshDefaults.Indicator(
                            state = pullRefreshState,
                            isRefreshing = ui.isRefreshingStage,
                            containerColor = colors.elevated,
                            color = colors.primary,
                            modifier = Modifier.align(Alignment.TopCenter),
                        )
                    },
                ) {
                    if (turns.isEmpty()) {
                        Box(
                            modifier = Modifier
                                .fillMaxSize()
                                .verticalScroll(rememberScrollState()),
                            contentAlignment = Alignment.Center,
                        ) {
                            NewSessionZenCanvas(
                                ui = ui,
                                onSelectProject = { path -> store.selectWorkspaceCwd(path) },
                                onSelectHost = { hostId -> store.selectHost(hostId) },
                                onSetDraftUseWorktree = { useWt -> store.setDraftUseWorktree(useWt) },
                            )
                        }
                    } else {
                        LazyColumn(
                            state = listState,
                            modifier = Modifier.fillMaxSize(),
                            contentPadding = PaddingValues(
                                horizontal = OrbitSpacing.lg,
                                vertical = OrbitSpacing.lg,
                            ),
                            // STRICT 24dp vertical spacing between conversation turns
                            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xl),
                        ) {
                            itemsIndexed(
                                items = turns,
                                key = { _, it -> it.key },
                            ) { index, turn ->
                                val isTurnActive = isActiveAgentRunning && index == turns.lastIndex
                                TurnItem(
                                    turn = turn,
                                    isTurnActive = isTurnActive,
                                    activeCwd = ui.activeCwd,
                                    describeStep = { store.describeStep(it) },
                                    onOpenDiffFile = { filePath -> onOpenDiffForFile(filePath) },
                                    onQuoteDiffLine = { path, lineLabel, content ->
                                        store.addQuotedDiffLine(path, lineLabel, content)
                                    },
                                    onCopyAssistantReply = { replyText ->
                                        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                                        cm?.setPrimaryClip(ClipData.newPlainText("assistant_reply", replyText))
                                        store.showToast("Copied reply to clipboard")
                                    },
                                    onQuoteAssistantReply = { replyText ->
                                        store.addQuotedAssistantReply(replyText)
                                    },
                                )
                            }
                        }
                    }
                }
            }

            // Native 36.dp circular scroll-to-bottom button above OmniComposer (standard fade, no bounce)
            androidx.compose.animation.AnimatedVisibility(
                visible = turns.isNotEmpty() && canScrollForward,
                enter = fadeIn(),
                exit = fadeOut(),
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(bottom = OrbitSpacing.sm),
            ) {
                Surface(
                    shape = CircleShape,
                    color = colors.elevated,
                    tonalElevation = 1.dp,
                    shadowElevation = 2.dp,
                    modifier = Modifier
                        .size(36.dp)
                        .border(OrbitSpacing.hairline, colors.hairline, CircleShape)
                        .clip(CircleShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            scope.launch {
                                if (turns.isNotEmpty()) {
                                    listState.animateScrollToItem(turns.lastIndex, scrollOffset = 100_000)
                                }
                            }
                        },
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Icon(
                            imageVector = Icons.Outlined.ArrowDownward,
                            contentDescription = "Scroll to bottom",
                            tint = colors.fgPrimary,
                            modifier = Modifier.size(18.dp),
                        )
                        if (isActiveAgentRunning) {
                            BreathingDot(
                                color = colors.primary,
                                modifier = Modifier
                                    .align(Alignment.TopEnd)
                                    .padding(6.dp),
                            )
                        }
                    }
                }
            }
        }

        val showBottomControls = ui.hosts.isNotEmpty() ||
            activeDecisionCount > 0 ||
            ui.composer.text.isNotEmpty() ||
            ui.composer.hasContextAttachments

        if (showBottomControls) {
            if (activeDecisionCount > 0) {
                StickyDecisionBar(
                    pendingPermissions = mergedPermissions,
                    pendingQuestions = mergedQuestions,
                    onResolvePermission = { reqId, decision -> store.resolvePermission(reqId, decision) },
                    onAnswerQuestion = { reqId, answers -> store.answerQuestion(reqId, answers) },
                )
            }

            OmniComposer(
                draft = ui.composer,
                workspaceFiles = ui.workspaceFiles,
                queuedPrompts = ui.activeQueuedPrompts,
                hostDetail = ui.hostDetail,
                activeCwd = ui.activeCwd,
                isNewSession = ui.isNewSessionMode || ui.selectedAgentId == null,
                isAgentRunning = isActiveAgentRunning,
                hasHost = ui.selectedHostId != null || ui.composer.text.isNotEmpty(),
                onTextChange = { store.updateComposerText(it) },
                onSend = { store.submitComposerPrompt() },
                onQueue = { store.enqueueComposerPrompt() },
                onInterrupt = { store.interruptActiveAgent() },
                onOpenContextPicker = onOpenContextPicker,
                onOpenModelSelector = onOpenModelSelector,
                onSelectMode = { store.selectSessionMode(it) },
                onRequestRefreshFiles = { store.refreshWorkspaceFiles() },
                onAddFileMention = { store.addFileMention(it) },
                onRemoveFileMention = { store.removeFileMention(it) },
                onAttachGitDiff = { store.attachGitDiffContext() },
                onToggleGitDiff = { store.toggleGitDiffContext() },
                onAttachStagedDiff = { store.attachStagedDiffContext() },
                onToggleStagedDiff = { store.toggleStagedDiffContext() },
                onAttachTerminal = { store.attachTerminalContext() },
                onToggleTerminal = { store.toggleTerminalContext() },
                onSelectSlash = { store.setSlashCommand(it) },
                onClearSlash = { store.setSlashCommand(null) },
                onRemoveQuotedSnippet = { store.removeQuotedSnippet(it) },
                onEditQueuedPrompt = { store.editQueuedPrompt(it) },
                onSendQueuedPromptNow = { store.sendQueuedPromptNow(it) },
                onRemoveQueuedPrompt = { store.removeQueuedPrompt(it) },
                onAttachImageBytes = { name, mime, bytes -> store.attachImageBytes(name, mime, bytes) },
                onAttachFileBytes = { name, mime, bytes -> store.attachFileBytes(name, mime, bytes) },
                onRemoveStagedAttachment = { store.removeStagedAttachment(it) },
                onRegisterInlineMention = { token, kind, path, staged, label ->
                    store.registerInlineMention(token, kind, path, staged, label)
                },
                slashCommands = ui.slashCommands,
                onFilterSlashCommands = { store.filterSlashCommands(it) },
                onRankMentions = { store.rankMentionCandidates(it) },
                onExecuteSlashCommand = { cmd ->
                    store.executeSlashCommand(
                        cmd = cmd,
                        onOpenTerminal = onOpenTerminal,
                        onOpenSettings = onOpenSettings,
                    )
                },
            )
        }
    }
}

@Composable
private fun UnpairedWelcomeHero(
    localDaemonAvailable: Boolean,
    onOpenPairSheet: () -> Unit,
    onConnectLocal: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val btnShape = RoundedCornerShape(OrbitSpacing.md)

    Column(
        modifier = modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(horizontal = OrbitSpacing.xl),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
    ) {
        Text(
            text = "Orbit Fleet",
            style = typography.title,
            color = colors.fgPrimary,
        )
        Text(
            text = "End-to-end encrypted multi-host control plane for Claude Code, Codex, and Antigravity.",
            style = typography.body,
            color = colors.fgSecondary,
        )
        Spacer(modifier = Modifier.height(OrbitSpacing.xs))
        Row(
            modifier = Modifier
                .heightIn(min = OrbitSpacing.touchTarget)
                .clip(btnShape)
                .background(colors.primary, btnShape)
                .clickable { onOpenPairSheet() }
                .padding(horizontal = OrbitSpacing.xl, vertical = OrbitSpacing.md),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            Icon(
                imageVector = Icons.Outlined.QrCodeScanner,
                contentDescription = null,
                tint = colors.onPrimary,
                modifier = Modifier.size(OrbitSpacing.iconMd),
            )
            Text(
                text = "Pair Host",
                style = typography.bodySemiBold,
                color = colors.onPrimary,
            )
        }
        if (localDaemonAvailable) {
            Box(
                modifier = Modifier
                    .heightIn(min = OrbitSpacing.touchTarget)
                    .clip(btnShape)
                    .background(colors.subtleFill, btnShape)
                    .border(OrbitSpacing.hairline, colors.hairline, btnShape)
                    .clickable { onConnectLocal() }
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.sm),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = "Connect Local Daemon",
                    style = typography.subheadMedium,
                    color = colors.fgPrimary,
                )
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun NewSessionZenCanvas(
    ui: OrbitUiState,
    onSelectProject: (String) -> Unit,
    onSelectHost: (String) -> Unit,
    onSetDraftUseWorktree: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current

    val projects = remember(ui.selectedHostId, ui.sidebarView) {
        ui.launchpadProjects
    }
    val activeProjectLabel = remember(projects, ui.activeCwd) {
        projects.firstOrNull { OrbitFormatters.pathsEquivalent(it.path, ui.activeCwd) }?.name
            ?: OrbitFormatters.shortPath(ui.activeCwd)
    }
    val activeHost = ui.selectedHost
    val isHostOnline = activeHost?.connectionState == FfiConnectionState.ONLINE_DIRECT ||
        activeHost?.connectionState == FfiConnectionState.ONLINE_RELAY
    val activeBranch = ui.activeGitBranch
    val isDraftSession = ui.isNewSessionMode || ui.selectedAgentId == null

    var projectMenuExpanded by remember { mutableStateOf(false) }
    var hostMenuExpanded by remember { mutableStateOf(false) }
    var environmentMenuExpanded by remember { mutableStateOf(false) }

    fun connectionDotColor(state: FfiConnectionState): Color = when (state) {
        FfiConnectionState.ONLINE_DIRECT -> colors.directAdded
        FfiConnectionState.ONLINE_RELAY -> colors.relay
        FfiConnectionState.CONNECTING_DIRECT, FfiConnectionState.CONNECTING_RELAY -> colors.warning
        FfiConnectionState.DISCONNECTED -> colors.dangerRemoved
    }

    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = OrbitSpacing.lg)
            .padding(bottom = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            text = "What would you like to build?",
            style = typography.title.copy(
                fontSize = 23.sp,
                fontWeight = FontWeight.Medium,
                letterSpacing = (-0.35).sp,
            ),
            color = colors.fgPrimary,
            textAlign = TextAlign.Center,
            maxLines = 2,
        )

        Spacer(modifier = Modifier.height(14.dp))

        // Compact Contextual Readiness Pills: [Project ▾] [Host ▾] [Local / Worktree ▾]
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm, Alignment.CenterHorizontally),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            // 1. Project Picker Pill
            Box {
                LaunchpadPillTrigger(
                    onClick = {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        projectMenuExpanded = true
                    },
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Folder,
                        contentDescription = null,
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(14.dp),
                    )
                    Text(
                        text = activeProjectLabel,
                        style = typography.subheadMedium,
                        color = colors.fgPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.widthIn(max = 160.dp),
                    )
                    Icon(
                        imageVector = Icons.Outlined.KeyboardArrowDown,
                        contentDescription = null,
                        tint = colors.fgMuted,
                        modifier = Modifier.size(14.dp),
                    )
                }

                OrbitCompactDropdownMenu(
                    expanded = projectMenuExpanded,
                    onDismissRequest = { projectMenuExpanded = false },
                    width = 252.dp,
                ) {
                    OrbitMenuSectionHeader(title = "Projects")
                    OrbitMenuScrollContainer(maxHeight = 240.dp) {
                        if (projects.isEmpty()) {
                            Text(
                                text = "No projects",
                                style = typography.meta,
                                color = colors.fgMuted,
                                modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                            )
                        } else {
                            projects.forEach { proj ->
                                val isSelected = OrbitFormatters.pathsEquivalent(proj.path, ui.activeCwd)
                                OrbitMenuOptionRow(
                                    title = proj.name,
                                    icon = Icons.Outlined.Folder,
                                    selected = isSelected,
                                    onClick = {
                                        projectMenuExpanded = false
                                        onSelectProject(proj.path)
                                    },
                                )
                            }
                        }
                    }
                }
            }

            // 2. Host Picker Pill (multi-host only)
            if (ui.hosts.size > 1 && activeHost != null) {
                Box {
                    LaunchpadPillTrigger(
                        onClick = {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            hostMenuExpanded = true
                        },
                    ) {
                        Box(
                            modifier = Modifier
                                .size(6.dp)
                                .clip(CircleShape)
                                .background(connectionDotColor(activeHost.connectionState)),
                        )
                        Text(
                            text = activeHost.displayName,
                            style = typography.subheadMedium,
                            color = colors.fgPrimary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.widthIn(max = 140.dp),
                        )
                        Icon(
                            imageVector = Icons.Outlined.KeyboardArrowDown,
                            contentDescription = null,
                            tint = colors.fgMuted,
                            modifier = Modifier.size(14.dp),
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
                                                .background(connectionDotColor(host.connectionState)),
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
                    }
                }
            }

            // 3. Execution Environment Pill (Local / Worktree)
            if (isDraftSession) {
                Box {
                    LaunchpadPillTrigger(
                        onClick = {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            environmentMenuExpanded = true
                        },
                    ) {
                        Icon(
                            imageVector = if (ui.draftUseWorktree) {
                                Icons.Outlined.AccountTree
                            } else {
                                Icons.Outlined.Computer
                            },
                            contentDescription = null,
                            tint = colors.fgSecondary,
                            modifier = Modifier.size(14.dp),
                        )
                        Text(
                            text = if (ui.draftUseWorktree) "Worktree" else "Local",
                            style = typography.subheadMedium,
                            color = colors.fgPrimary,
                            maxLines = 1,
                        )
                        Icon(
                            imageVector = Icons.Outlined.KeyboardArrowDown,
                            contentDescription = null,
                            tint = colors.fgMuted,
                            modifier = Modifier.size(14.dp),
                        )
                    }

                    OrbitCompactDropdownMenu(
                        expanded = environmentMenuExpanded,
                        onDismissRequest = { environmentMenuExpanded = false },
                        width = 288.dp,
                    ) {
                        OrbitMenuSectionHeader(
                            title = "Execution",
                            trailing = activeBranch,
                        )
                        OrbitMenuOptionRow(
                            title = "Local",
                            subtitle = "Work directly in project folder",
                            icon = Icons.Outlined.Computer,
                            selected = !ui.draftUseWorktree,
                            onClick = {
                                environmentMenuExpanded = false
                                onSetDraftUseWorktree(false)
                            },
                        )
                        OrbitMenuOptionRow(
                            title = "Worktree",
                            subtitle = "Isolated Git worktree for parallel tasks",
                            icon = Icons.Outlined.AccountTree,
                            selected = ui.draftUseWorktree,
                            onClick = {
                                environmentMenuExpanded = false
                                onSetDraftUseWorktree(true)
                            },
                        )
                    }
                }
            }
        }

        // Quiet single-line Git branch or host connection caption
        if (activeHost != null && !isHostOnline) {
            Spacer(modifier = Modifier.height(10.dp))
            Row(
                horizontalArrangement = Arrangement.spacedBy(5.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    modifier = Modifier
                        .size(6.dp)
                        .clip(CircleShape)
                        .background(connectionDotColor(activeHost.connectionState)),
                )
                Text(
                    text = "${OrbitFormatters.topBarConnectionLabel(activeHost.connectionState, activeHost.viaSshTunnel)} · ${activeHost.displayName}",
                    style = typography.meta,
                    color = colors.fgMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        } else if (!activeBranch.isNullOrBlank()) {
            Spacer(modifier = Modifier.height(10.dp))
            Row(
                horizontalArrangement = Arrangement.spacedBy(4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    imageVector = Icons.Outlined.AccountTree,
                    contentDescription = null,
                    tint = colors.fgMuted,
                    modifier = Modifier.size(12.dp),
                )
                Text(
                    text = if (ui.draftUseWorktree) "Worktree from $activeBranch" else activeBranch,
                    style = typography.meta,
                    color = colors.fgMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun LaunchpadPillTrigger(
    onClick: () -> Unit,
    content: @Composable () -> Unit,
) {
    val colors = OrbitTheme.colors
    Row(
        modifier = Modifier
            .height(34.dp)
            .clip(CircleShape)
            .background(colors.subtleFill, CircleShape)
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(5.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        content()
    }
}
