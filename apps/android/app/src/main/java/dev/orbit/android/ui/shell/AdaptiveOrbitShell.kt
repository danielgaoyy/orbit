package dev.orbit.android.ui.shell

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.MenuOpen
import androidx.compose.material.icons.automirrored.outlined.ViewSidebar
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Difference
import androidx.compose.material.icons.outlined.Menu
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.Text
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitAndroidStore
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.store.OrbitUiState
import dev.orbit.android.ui.composer.ContextMentionSheet
import dev.orbit.android.ui.composer.ModelSelectorSheet
import dev.orbit.android.ui.sheets.DirectoryBrowserSheet
import dev.orbit.android.ui.sheets.GitDiffBottomSheet
import dev.orbit.android.ui.sheets.GitDiffInspectorPane
import dev.orbit.android.ui.sheets.PairQrSheet
import dev.orbit.android.ui.sheets.PtyTerminalSheet
import dev.orbit.android.ui.sheets.SettingsSheet
import dev.orbit.android.ui.stage.StreamStageScreen
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.orbit_ffi_uniffi.FfiConnectionState

private val PERMANENT_DRAWER_WIDTH = 260.dp
private val INLINE_DIFF_PANE_WIDTH = 360.dp
private val TWO_PANE_BREAKPOINT = 700.dp
private val THREE_PANE_BREAKPOINT = 1024.dp
private val SIDE_BY_SIDE_DIFF_STAGE_THRESHOLD = 780.dp

private enum class ActiveModalSheet {
    NONE,
    TERMINAL,
    MODEL_SELECTOR,
    CONTEXT_MENTION,
    PAIR_QR,
    DIRECTORY_BROWSER,
    SETTINGS,
}

@Composable
fun AdaptiveOrbitShell(
    ui: OrbitUiState,
    store: OrbitAndroidStore,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val scope = rememberCoroutineScope()
    val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)

    var activeSheet by rememberSaveable { mutableStateOf(ActiveModalSheet.NONE) }
    var lastToastText by remember { mutableStateOf("") }
    if (ui.toastMessage != null) {
        lastToastText = ui.toastMessage
    }

    LaunchedEffect(ui.toastMessage) {
        if (ui.toastMessage != null) {
            delay(2400)
            store.clearToast()
        }
    }

    BoxWithConstraints(
        modifier = modifier
            .fillMaxSize()
            .background(colors.canvas),
    ) {
        // >= 700.dp supports collapsible 260.dp sidebar; Zen mode collapses sidebar to 0.dp.
        // Side-by-side 360.dp GitDiffInspectorPane activates whenever maxWidth >= 1024.dp OR (maxWidth - effectiveDrawerWidth) >= 780.dp.
        val hasPermanentDrawer = maxWidth >= TWO_PANE_BREAKPOINT
        val effectiveDrawerWidth = if (hasPermanentDrawer && !ui.isPermanentDrawerCollapsed) {
            PERMANENT_DRAWER_WIDTH
        } else {
            0.dp
        }
        val supportsThreePane = maxWidth >= THREE_PANE_BREAKPOINT ||
            (maxWidth - effectiveDrawerWidth) >= SIDE_BY_SIDE_DIFF_STAGE_THRESHOLD

        LaunchedEffect(hasPermanentDrawer) {
            if (hasPermanentDrawer && drawerState.isOpen) {
                drawerState.close()
            }
        }

        // ADB Launch Harness Sheet / Drawer Trigger (~450ms after launch/intent)
        LaunchedEffect(ui.harnessRequestId, ui.harnessSheetTarget) {
            val target = ui.harnessSheetTarget ?: return@LaunchedEffect
            delay(450)
            when (target) {
                "drawer" -> {
                    activeSheet = ActiveModalSheet.NONE
                    store.closeDiffInspector()
                    if (hasPermanentDrawer) {
                        store.setPermanentDrawerCollapsed(false)
                    } else {
                        drawerState.open()
                    }
                }
                "diff", "gitdiff", "git_diff" -> {
                    activeSheet = ActiveModalSheet.NONE
                    if (!hasPermanentDrawer && drawerState.isOpen) drawerState.close()
                    store.openDiffInspector(null)
                }
                "pty", "terminal" -> {
                    store.closeDiffInspector()
                    if (!hasPermanentDrawer && drawerState.isOpen) drawerState.close()
                    activeSheet = ActiveModalSheet.TERMINAL
                }
                "model", "models" -> {
                    store.closeDiffInspector()
                    if (!hasPermanentDrawer && drawerState.isOpen) drawerState.close()
                    activeSheet = ActiveModalSheet.MODEL_SELECTOR
                }
                "context", "mention" -> {
                    store.closeDiffInspector()
                    if (!hasPermanentDrawer && drawerState.isOpen) drawerState.close()
                    activeSheet = ActiveModalSheet.CONTEXT_MENTION
                }
                "pair", "qr" -> {
                    store.closeDiffInspector()
                    if (!hasPermanentDrawer && drawerState.isOpen) drawerState.close()
                    activeSheet = ActiveModalSheet.PAIR_QR
                }
                "settings", "quotas" -> {
                    store.closeDiffInspector()
                    if (!hasPermanentDrawer && drawerState.isOpen) drawerState.close()
                    activeSheet = ActiveModalSheet.SETTINGS
                }
                "none", "stage" -> {
                    activeSheet = ActiveModalSheet.NONE
                    store.closeDiffInspector()
                    if (!hasPermanentDrawer && drawerState.isOpen) drawerState.close()
                }
            }
            store.consumeHarnessSheetTarget()
        }

        // ADB Launch Harness PTY Command Injection (~900ms after PTY sheet opens)
        LaunchedEffect(activeSheet, ui.pendingPtyCommand) {
            if (activeSheet == ActiveModalSheet.TERMINAL && ui.pendingPtyCommand != null) {
                val cmd = store.consumePendingPtyCommand() ?: return@LaunchedEffect
                store.ensureTerminalStarted(ui.activeCwd)
                delay(900)
                store.sendTerminalKeys("${cmd.trimEnd('\r', '\n')}\n")
            }
        }

        // Predictive Back / BackHandler closes active sheets, diff inspector, or compact drawer smoothly
        val shouldInterceptBack = activeSheet != ActiveModalSheet.NONE ||
            ui.isDiffInspectorOpen ||
            (!hasPermanentDrawer && drawerState.isOpen)

        BackHandler(enabled = shouldInterceptBack) {
            when {
                activeSheet != ActiveModalSheet.NONE -> activeSheet = ActiveModalSheet.NONE
                ui.isDiffInspectorOpen -> store.closeDiffInspector()
                !hasPermanentDrawer && drawerState.isOpen -> scope.launch { drawerState.close() }
            }
        }

        val openDiffHandler: (String?) -> Unit = { pathFilter ->
            store.openDiffInspector(pathFilter)
        }

        val drawerBody: @Composable () -> Unit = {
            FleetDrawerContent(
                ui = ui,
                onSearchQueryChange = { store.updateDrawerSearchQuery(it) },
                onSelectHost = { hostId ->
                    store.selectHost(hostId)
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                },
                onRefreshHost = { store.refreshSelectedHost() },
                onSelectProjectDir = { dir ->
                    store.selectWorkspaceCwd(dir)
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                },
                onPinProject = { name, dir -> store.pinProject(name, dir) },
                onUnpinProject = { projId -> store.removeProject(projId) },
                onOpenDirectoryBrowser = {
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                    activeSheet = ActiveModalSheet.DIRECTORY_BROWSER
                },
                onSelectAgent = { agentId ->
                    store.selectAgent(agentId)
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                },
                onArchiveAgent = { agentId -> store.archiveAgent(agentId) },
                onNewSession = {
                    store.startNewSession()
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                },
                onNewSessionInWorkspace = { cwd, provider ->
                    store.startNewSessionInWorkspace(cwd = cwd, provider = provider)
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                },
                onInspectGitDiffAtCwd = { cwd ->
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                    store.inspectGitDiffAtCwd(cwd)
                },
                onOpenTerminalAtCwd = { cwd ->
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                    store.openTerminalAtCwd(cwd)
                    activeSheet = ActiveModalSheet.TERMINAL
                },
                onCopiedPath = {
                    store.showToast("Copied path to clipboard")
                },
                onOpenPairHost = {
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                    activeSheet = ActiveModalSheet.PAIR_QR
                },
                onOpenSettings = {
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                    activeSheet = ActiveModalSheet.SETTINGS
                },
            )
        }

        val mainStageColumn: @Composable (Modifier) -> Unit = { colModifier ->
            Column(modifier = colModifier.fillMaxHeight()) {
                OrbitStageTopBar(
                    ui = ui,
                    isWideViewport = hasPermanentDrawer,
                    onOpenDrawer = { scope.launch { drawerState.open() } },
                    onTogglePermanentDrawer = { store.togglePermanentDrawerCollapsed() },
                    onTitleClick = {
                        if (ui.hosts.isNotEmpty() && ui.selectedHostId != null) {
                            activeSheet = ActiveModalSheet.DIRECTORY_BROWSER
                        }
                    },
                    onToggleDiff = {
                        store.toggleDiffInspector()
                    },
                    onOpenTerminal = { activeSheet = ActiveModalSheet.TERMINAL },
                    onNewSession = { store.startNewSession() },
                    onOpenSettings = { activeSheet = ActiveModalSheet.SETTINGS },
                )
                HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                StreamStageScreen(
                    ui = ui,
                    store = store,
                    onOpenContextPicker = { activeSheet = ActiveModalSheet.CONTEXT_MENTION },
                    onOpenModelSelector = { activeSheet = ActiveModalSheet.MODEL_SELECTOR },
                    onOpenDiffForFile = openDiffHandler,
                    onOpenDirectoryBrowser = { activeSheet = ActiveModalSheet.DIRECTORY_BROWSER },
                    onOpenPairSheet = { activeSheet = ActiveModalSheet.PAIR_QR },
                    modifier = Modifier.weight(1f),
                )
            }
        }

        if (hasPermanentDrawer) {
            Row(modifier = Modifier.fillMaxSize()) {
                AnimatedVisibility(
                    visible = !ui.isPermanentDrawerCollapsed,
                    enter = expandHorizontally() + fadeIn(),
                    exit = shrinkHorizontally() + fadeOut(),
                ) {
                    Row(
                        modifier = Modifier
                            .width(PERMANENT_DRAWER_WIDTH)
                            .fillMaxHeight()
                            .background(colors.drawer),
                    ) {
                        Box(modifier = Modifier.weight(1f)) {
                            drawerBody()
                        }
                        Box(
                            modifier = Modifier
                                .width(OrbitSpacing.hairline)
                                .fillMaxHeight()
                                .background(colors.hairline)
                        )
                    }
                }
                mainStageColumn(Modifier.weight(1f))
                if (supportsThreePane && ui.isDiffInspectorOpen) {
                    GitDiffInspectorPane(
                        cwd = ui.activeCwd,
                        gitReview = ui.gitReview,
                        sessionTurnDiffs = ui.agentStage?.sessionTurnDiffs.orEmpty(),
                        stagedMode = ui.gitStagedMode,
                        selectedPath = ui.gitSelectedPath,
                        isRefreshing = ui.isRefreshingGitDiff,
                        onRefresh = { store.loadGitReview() },
                        onPullRefresh = { store.refreshGitDiff() },
                        onSetStagedMode = { store.setGitStagedMode(it) },
                        onSelectPath = { store.selectGitFileFilter(it) },
                        onQuoteLine = { path, lineLabel, content ->
                            store.addQuotedDiffLine(path, lineLabel, content)
                        },
                        onClose = { store.closeDiffInspector() },
                    )
                }
            }
        } else {
            ModalNavigationDrawer(
                drawerState = drawerState,
                drawerContent = {
                    ModalDrawerSheet(
                        modifier = Modifier.width(304.dp),
                        drawerContainerColor = colors.drawer,
                        drawerContentColor = colors.fgPrimary,
                    ) {
                        drawerBody()
                    }
                }
            ) {
                mainStageColumn(Modifier.fillMaxSize())
            }
        }

        // Non-overlapping transient toast pill positioned strictly below the 52.dp top header bar (padding(top = 56.dp))
        val stageStartInset = effectiveDrawerWidth
        val stageEndInset = if (supportsThreePane && ui.isDiffInspectorOpen) {
            INLINE_DIFF_PANE_WIDTH
        } else {
            0.dp
        }
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = stageStartInset, end = stageEndInset)
                .statusBarsPadding()
                .padding(top = 56.dp, start = OrbitSpacing.lg, end = OrbitSpacing.lg),
            contentAlignment = Alignment.TopCenter,
        ) {
            AnimatedVisibility(
                visible = ui.toastMessage != null,
                enter = fadeIn() + slideInVertically { -it / 2 },
                exit = fadeOut() + slideOutVertically { -it / 2 },
            ) {
                val toastShape = RoundedCornerShape(OrbitSpacing.md)
                Row(
                    modifier = Modifier
                        .widthIn(max = 420.dp)
                        .clip(toastShape)
                        .background(colors.elevated, toastShape)
                        .border(OrbitSpacing.hairline, colors.hairline, toastShape)
                        .clickable { store.clearToast() }
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = lastToastText,
                        style = typography.metaMedium,
                        color = colors.fgPrimary,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }

        // Compact & Foldable/Medium Modal Diff Sheet (< 1024.dp, keeping Stage >= 440.dp on 700..1023.dp)
        if (!supportsThreePane && ui.isDiffInspectorOpen) {
            GitDiffBottomSheet(
                cwd = ui.activeCwd,
                gitReview = ui.gitReview,
                sessionTurnDiffs = ui.agentStage?.sessionTurnDiffs.orEmpty(),
                stagedMode = ui.gitStagedMode,
                selectedPath = ui.gitSelectedPath,
                isRefreshing = ui.isRefreshingGitDiff,
                onRefresh = { store.loadGitReview() },
                onPullRefresh = { store.refreshGitDiff() },
                onSetStagedMode = { store.setGitStagedMode(it) },
                onSelectPath = { store.selectGitFileFilter(it) },
                onQuoteLine = { path, lineLabel, content ->
                    store.addQuotedDiffLine(path, lineLabel, content)
                    store.closeDiffInspector()
                },
                onDismiss = { store.closeDiffInspector() },
            )
        }

        // Modal Bottom Sheets
        when (activeSheet) {
            ActiveModalSheet.NONE -> Unit

            ActiveModalSheet.TERMINAL -> {
                PtyTerminalSheet(
                    cwd = ui.activeCwd,
                    terminals = ui.hostDetail?.terminals.orEmpty(),
                    activeTerminalId = ui.hostDetail?.activeTerminalId,
                    screen = ui.terminalScreen,
                    onEnsureStarted = { store.ensureTerminalStarted(ui.activeCwd) },
                    onSpawnTerminal = { store.spawnNewTerminal(ui.activeCwd) },
                    onAttachTerminal = { store.attachTerminalSession(it) },
                    onKillTerminal = { store.killTerminalSession(it) },
                    onViewportMeasured = { c, r -> store.onTerminalViewportMeasured(c, r) },
                    onSendKeys = { store.sendTerminalKeys(it) },
                    onCopiedOutput = { store.showToast("Copied terminal output") },
                    onQuoteTerminal = {
                        store.quoteTerminalToComposer()
                        activeSheet = ActiveModalSheet.NONE
                    },
                    onDismiss = { activeSheet = ActiveModalSheet.NONE },
                )
            }

            ActiveModalSheet.MODEL_SELECTOR -> {
                ModelSelectorSheet(
                    currentProvider = ui.composer.provider,
                    currentModel = ui.composer.model,
                    currentEffort = ui.composer.effort,
                    hostDetail = ui.hostDetail,
                    onSelect = { prov, mod, eff ->
                        store.selectProviderModelAndEffort(prov, mod, eff)
                    },
                    onRefreshCatalogs = { store.refreshQuotasAndModels(forceModels = true) },
                    onDismiss = { activeSheet = ActiveModalSheet.NONE },
                )
            }

            ActiveModalSheet.CONTEXT_MENTION -> {
                ContextMentionSheet(
                    workspaceFiles = ui.workspaceFiles,
                    mentionedFiles = ui.composer.mentionedFiles,
                    includeGitDiff = ui.composer.includeGitDiff,
                    includeStagedDiff = ui.composer.includeStagedDiff,
                    includeTerminal = ui.composer.includeTerminal,
                    activeSlashCommand = ui.composer.activeSlashCommand,
                    onRequestRefreshFiles = { store.refreshWorkspaceFiles() },
                    onAddFileMention = { store.addFileMention(it) },
                    onToggleGitDiff = { store.toggleGitDiffContext() },
                    onToggleStagedDiff = { store.toggleStagedDiffContext() },
                    onToggleTerminal = { store.toggleTerminalContext() },
                    onSelectSlash = { store.setSlashCommand(it) },
                    onDismiss = { activeSheet = ActiveModalSheet.NONE },
                )
            }

            ActiveModalSheet.PAIR_QR -> {
                PairQrSheet(
                    localDaemonAvailable = ui.localDaemonUri != null,
                    lastError = ui.lastError,
                    onPairUri = { uri ->
                        store.pairHost(uri) {
                            activeSheet = ActiveModalSheet.NONE
                        }
                    },
                    onDiscoverLocal = {
                        store.discoverLocalDaemon()
                        activeSheet = ActiveModalSheet.NONE
                    },
                    onDismiss = { activeSheet = ActiveModalSheet.NONE },
                )
            }

            ActiveModalSheet.DIRECTORY_BROWSER -> {
                DirectoryBrowserSheet(
                    initialPath = ui.activeCwd,
                    listing = ui.hostDetail?.directoryListing,
                    onBrowsePath = { store.browseDirectory(it) },
                    onSelectDirectory = { store.selectWorkspaceCwd(it) },
                    onPinProject = { name, dir -> store.pinProject(name, dir) },
                    onDismiss = { activeSheet = ActiveModalSheet.NONE },
                )
            }

            ActiveModalSheet.SETTINGS -> {
                SettingsSheet(
                    appearanceMode = ui.appearanceMode,
                    selectedHost = ui.selectedHost,
                    quotas = ui.hostDetail?.quotas.orEmpty(),
                    onSelectAppearance = { store.setAppearanceMode(it) },
                    onReconnectHost = { store.reconnectSelectedHost() },
                    onSaveHostConfig = { hostId, customName, routeMode, directEp, relay ->
                        store.updateHostConfiguration(
                            hostId = hostId,
                            customName = customName,
                            routeMode = routeMode,
                            sshAlias = ui.selectedHost?.sshAlias,
                            remoteDaemonPort = ui.selectedHost?.remoteDaemonPort,
                            customDirectEndpoint = directEp,
                            relayUrl = relay,
                            reconnect = true,
                        )
                    },
                    onRemoveHost = { store.removeHost(it) },
                    onRefreshQuotas = { store.refreshQuotasAndModels(forceModels = false) },
                    onDismiss = { activeSheet = ActiveModalSheet.NONE },
                )
            }
        }
    }
}

@Composable
private fun OrbitStageTopBar(
    ui: OrbitUiState,
    isWideViewport: Boolean,
    onOpenDrawer: () -> Unit,
    onTogglePermanentDrawer: () -> Unit,
    onTitleClick: () -> Unit,
    onToggleDiff: () -> Unit,
    onOpenTerminal: () -> Unit,
    onNewSession: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current

    val hasHosts = ui.hosts.isNotEmpty()
    val host = if (hasHosts) ui.selectedHost else null
    val statusDotColor = when (host?.connectionState) {
        FfiConnectionState.ONLINE_DIRECT -> colors.directAdded
        FfiConnectionState.ONLINE_RELAY -> colors.relay
        FfiConnectionState.CONNECTING_DIRECT, FfiConnectionState.CONNECTING_RELAY -> colors.warning
        else -> colors.fgMuted
    }

    val primaryTitle = when {
        host == null -> "Orbit"
        ui.isNewSessionMode || ui.activeAgent == null -> "New Session"
        else -> ui.activeAgent?.title?.ifBlank { "New Session" } ?: "New Session"
    }

    // Compact top-bar-only connection route ("Direct" / "SSH" / "Relay" / "Connecting" / "Offline")
    val subtitle: String? = if (host == null) {
        null
    } else {
        val compactRoute = OrbitFormatters.topBarConnectionLabel(host.connectionState, host.viaSshTunnel)
        "$compactRoute · ${OrbitFormatters.shortPath(ui.activeCwd)}"
    }

    val diffFiles = ui.gitReview?.diffFiles?.takeIf { it.isNotEmpty() }
        ?: ui.agentStage?.sessionTurnDiffs.orEmpty()
    val totalAdditions = remember(diffFiles) { diffFiles.sumOf { it.additions.toLong() } }
    val totalDeletions = remember(diffFiles) { diffFiles.sumOf { it.deletions.toLong() } }
    val hasActiveTerminals = ui.hostDetail?.terminals?.isNotEmpty() == true

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(colors.canvas)
            .statusBarsPadding()
            .height(OrbitSpacing.topBarHeight)
            .padding(horizontal = OrbitSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // 1. Compact Menu Trigger (< 700.dp) OR Collapsible Sidebar Zen Toggle (>= 700.dp)
        if (!isWideViewport) {
            Box(
                modifier = Modifier
                    .size(OrbitSpacing.touchTarget)
                    .clip(CircleShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onOpenDrawer()
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Menu,
                    contentDescription = "Open fleet drawer",
                    tint = colors.fgPrimary,
                    modifier = Modifier.size(OrbitSpacing.iconMd),
                )
                if (ui.totalPendingApprovals > 0u) {
                    Box(
                        modifier = Modifier
                            .align(Alignment.TopEnd)
                            .padding(top = OrbitSpacing.md, end = OrbitSpacing.md)
                            .size(6.dp)
                            .background(colors.warning, CircleShape)
                    )
                }
            }
        } else {
            Box(
                modifier = Modifier
                    .size(OrbitSpacing.touchTarget)
                    .clip(CircleShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onTogglePermanentDrawer()
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = if (ui.isPermanentDrawerCollapsed) {
                        Icons.AutoMirrored.Outlined.ViewSidebar
                    } else {
                        Icons.AutoMirrored.Outlined.MenuOpen
                    },
                    contentDescription = if (ui.isPermanentDrawerCollapsed) {
                        "Expand fleet sidebar"
                    } else {
                        "Collapse fleet sidebar"
                    },
                    tint = colors.fgSecondary,
                    modifier = Modifier.size(OrbitSpacing.iconMd),
                )
                if (ui.isPermanentDrawerCollapsed && ui.totalPendingApprovals > 0u) {
                    Box(
                        modifier = Modifier
                            .align(Alignment.TopEnd)
                            .padding(top = OrbitSpacing.md, end = OrbitSpacing.md)
                            .size(6.dp)
                            .background(colors.warning, CircleShape)
                    )
                }
            }
        }

        // 2. Session Title + Zero-Truncation "● Direct · repo" Subtitle
        Column(
            modifier = Modifier
                .weight(1f)
                .clip(RoundedCornerShape(OrbitSpacing.sm))
                .clickable(enabled = host != null) { onTitleClick() }
                .padding(horizontal = OrbitSpacing.xs, vertical = OrbitSpacing.xs / 2),
            verticalArrangement = Arrangement.Center,
        ) {
            Text(
                text = primaryTitle,
                style = typography.subheadSemiBold,
                color = colors.fgPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (subtitle != null) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    Box(
                        modifier = Modifier
                            .size(6.dp)
                            .background(statusDotColor, CircleShape)
                    )
                    Text(
                        text = subtitle,
                        style = typography.meta,
                        color = colors.fgSecondary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }

        if (host != null) {
            // 3. `+add -del` Diff Pill (>= 44.dp touch target + explicit TalkBack label)
            val pillShape = RoundedCornerShape(OrbitSpacing.sm)
            val diffSemanticsDesc = if (totalAdditions > 0L || totalDeletions > 0L) {
                "Inspect git diff, $totalAdditions additions and $totalDeletions deletions"
            } else {
                "Inspect git diff"
            }
            Box(
                modifier = Modifier
                    .height(OrbitSpacing.touchTarget)
                    .widthIn(min = 44.dp)
                    .clip(pillShape)
                    .semantics { contentDescription = diffSemanticsDesc }
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onToggleDiff()
                    }
                    .padding(horizontal = OrbitSpacing.sm),
                contentAlignment = Alignment.Center,
            ) {
                if (totalAdditions > 0L || totalDeletions > 0L) {
                    Row(
                        modifier = Modifier
                            .background(colors.subtleFill, pillShape)
                            .border(OrbitSpacing.hairline, colors.hairline, pillShape)
                            .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = "+$totalAdditions",
                            style = typography.metaMedium,
                            color = colors.directAdded,
                            maxLines = 1,
                            softWrap = false,
                        )
                        Text(
                            text = "-$totalDeletions",
                            style = typography.metaMedium,
                            color = colors.dangerRemoved,
                            maxLines = 1,
                            softWrap = false,
                        )
                    }
                } else {
                    Icon(
                        imageVector = Icons.Outlined.Difference,
                        contentDescription = "Inspect git diff",
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconMd),
                    )
                }
            }

            // 4. `>_` PTY Terminal Button (48dp touch target)
            Box(
                modifier = Modifier
                    .size(OrbitSpacing.touchTarget)
                    .clip(CircleShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onOpenTerminal()
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Terminal,
                    contentDescription = "Open interactive terminal",
                    tint = colors.fgSecondary,
                    modifier = Modifier.size(OrbitSpacing.iconMd),
                )
                if (hasActiveTerminals) {
                    Box(
                        modifier = Modifier
                            .align(Alignment.TopEnd)
                            .padding(top = 12.dp, end = 12.dp)
                            .size(6.dp)
                            .background(colors.directAdded, CircleShape)
                    )
                }
            }

            // 5. `+` New Session Icon Button (48dp touch target)
            Box(
                modifier = Modifier
                    .size(OrbitSpacing.touchTarget)
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
        } else {
            // Unpaired Chrome: only a quiet Settings icon button on the right
            Box(
                modifier = Modifier
                    .size(OrbitSpacing.touchTarget)
                    .clip(CircleShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onOpenSettings()
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Settings,
                    contentDescription = "Settings",
                    tint = colors.fgSecondary,
                    modifier = Modifier.size(OrbitSpacing.iconMd),
                )
            }
        }
    }
}
