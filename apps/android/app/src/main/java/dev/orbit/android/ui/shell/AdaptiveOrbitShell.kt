package dev.orbit.android.ui.shell

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
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
import androidx.compose.material.icons.automirrored.outlined.CompareArrows
import androidx.compose.material.icons.outlined.Archive
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material.icons.outlined.MoreHoriz
import androidx.compose.material.icons.outlined.OpenInFull
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
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
import dev.orbit.android.ui.theme.OrbitComposeIcon
import dev.orbit.android.ui.theme.OrbitCopyIcon
import dev.orbit.android.ui.theme.OrbitSidebarIcon
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
    FULL_DIFF,
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
    val context = LocalContext.current
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
                onSetProjectSort = { store.setSidebarProjectSort(it) },
                onSetThreadFilter = { store.setSidebarThreadFilter(it) },
                onToggleProjectCollapsed = { store.toggleProjectCollapsed(it) },
                onTogglePinThread = { hId, aId -> store.togglePinThread(hId, aId) },
                onSelectHost = { hostId ->
                    store.selectHost(hostId)
                    if (!hasPermanentDrawer) scope.launch { drawerState.close() }
                },
                onConnectLocalDaemon = { store.discoverLocalDaemon() },
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
                    isInlineDiffOpen = supportsThreePane && ui.isDiffInspectorOpen,
                    onOpenDrawer = { scope.launch { drawerState.open() } },
                    onTogglePermanentDrawer = { store.togglePermanentDrawerCollapsed() },
                    onOpenDirectoryBrowser = { activeSheet = ActiveModalSheet.DIRECTORY_BROWSER },
                    onCopyPath = { path ->
                        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                        cm?.setPrimaryClip(ClipData.newPlainText("workspace_path", path))
                        store.showToast("Copied path")
                    },
                    onOpenPairSheet = { activeSheet = ActiveModalSheet.PAIR_QR },
                    onToggleDiff = { store.toggleDiffInspector() },
                    onOpenFullDiff = {
                        store.loadGitReview()
                        activeSheet = ActiveModalSheet.FULL_DIFF
                    },
                    onOpenTerminal = { activeSheet = ActiveModalSheet.TERMINAL },
                    onNewSession = { store.startNewSession() },
                    onArchiveSession = { agentId -> store.archiveAgent(agentId) },
                    onOpenSettings = { activeSheet = ActiveModalSheet.SETTINGS },
                )
                val showTopBarDivider = ui.hosts.isEmpty() || (!ui.isNewSessionMode && ui.activeAgent != null)
                if (showTopBarDivider) {
                    HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                }
                StreamStageScreen(
                    ui = ui,
                    store = store,
                    onOpenContextPicker = { activeSheet = ActiveModalSheet.CONTEXT_MENTION },
                    onOpenModelSelector = { activeSheet = ActiveModalSheet.MODEL_SELECTOR },
                    onOpenDiffForFile = openDiffHandler,
                    onOpenPairSheet = { activeSheet = ActiveModalSheet.PAIR_QR },
                    onOpenTerminal = { activeSheet = ActiveModalSheet.TERMINAL },
                    onOpenSettings = { activeSheet = ActiveModalSheet.SETTINGS },
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
                        modifier = Modifier.width(320.dp),
                        drawerShape = RoundedCornerShape(topEnd = 16.dp, bottomEnd = 16.dp),
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

            ActiveModalSheet.FULL_DIFF -> {
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
                        activeSheet = ActiveModalSheet.NONE
                    },
                    onDismiss = { activeSheet = ActiveModalSheet.NONE },
                )
            }

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
                    isNewSession = ui.isNewSessionMode || ui.selectedAgentId == null,
                    onSwitchProvider = { store.switchDraftProvider(it) },
                    onSelectEffort = { store.selectEffort(it) },
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
                val hostPinned = remember(ui.projects, ui.selectedHostId) {
                    ui.projects.filter { it.hostId == ui.selectedHostId }
                }
                DirectoryBrowserSheet(
                    initialPath = ui.activeCwd,
                    hostDisplayName = ui.selectedHost?.displayName,
                    isMultiHost = ui.hosts.size > 1,
                    listing = ui.hostDetail?.directoryListing,
                    pinnedProjects = hostPinned,
                    discoveredProjects = ui.hostDetail?.discoveredProjects.orEmpty(),
                    onBrowsePath = { store.browseDirectory(it) },
                    onSelectDirectory = { store.selectWorkspaceCwd(it) },
                    onPinProject = { name, dir -> store.pinProject(name, dir) },
                    onUnpinProject = { projId -> store.removeProject(projId) },
                    onDismiss = { activeSheet = ActiveModalSheet.NONE },
                )
            }

            ActiveModalSheet.SETTINGS -> {
                SettingsSheet(
                    appearanceMode = ui.appearanceMode,
                    hosts = ui.hosts,
                    selectedHostId = ui.selectedHostId,
                    quotas = ui.hostDetail?.quotas.orEmpty(),
                    onSelectAppearance = { store.setAppearanceMode(it) },
                    onSelectHost = { store.selectHost(it) },
                    onOpenPairHost = { activeSheet = ActiveModalSheet.PAIR_QR },
                    onReconnectHost = { hostId -> store.reconnectHost(hostId) },
                    onSaveHostConfig = { hostId, customName, routeMode, directEp, relay ->
                        val target = ui.hosts.firstOrNull { it.hostId == hostId }
                        store.updateHostConfiguration(
                            hostId = hostId,
                            customName = customName,
                            routeMode = routeMode,
                            sshAlias = target?.sshAlias,
                            remoteDaemonPort = target?.remoteDaemonPort,
                            customDirectEndpoint = directEp,
                            relayUrl = relay,
                            reconnect = true,
                        )
                    },
                    onRemoveHost = { store.removeHost(it) },
                    onRefreshQuotas = { store.refreshQuotasAndModels(forceModels = false) },
                    onShowToast = { store.showToast(it) },
                    onDismiss = { activeSheet = ActiveModalSheet.NONE },
                )
            }
        }
    }
}

private fun resolveHeaderGitBranch(ui: OrbitUiState): String? {
    ui.gitReview?.branch?.takeIf { it.isNotBlank() }?.let { return it.trim() }
    val detail = ui.hostDetail ?: return null
    detail.discoveredProjects
        .firstOrNull { OrbitFormatters.pathsEquivalent(it.path, ui.activeCwd) }
        ?.gitBranch?.takeIf { it.isNotBlank() }
        ?.let { return it.trim() }
    detail.workspaces
        .firstOrNull { OrbitFormatters.pathsEquivalent(it.cwd, ui.activeCwd) }
        ?.gitBranch?.takeIf { it.isNotBlank() }
        ?.let { return it.trim() }
    return null
}

@Composable
private fun OrbitStageTopBar(
    ui: OrbitUiState,
    isWideViewport: Boolean,
    isInlineDiffOpen: Boolean,
    onOpenDrawer: () -> Unit,
    onTogglePermanentDrawer: () -> Unit,
    onOpenDirectoryBrowser: () -> Unit,
    onCopyPath: (String) -> Unit,
    onOpenPairSheet: () -> Unit,
    onToggleDiff: () -> Unit,
    onOpenFullDiff: () -> Unit,
    onOpenTerminal: () -> Unit,
    onNewSession: () -> Unit,
    onArchiveSession: (String) -> Unit,
    onOpenSettings: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current

    val hasHosts = ui.hosts.isNotEmpty()
    val host = if (hasHosts) ui.selectedHost else null
    val activeAgent = if (host != null && !ui.isNewSessionMode) ui.activeAgent else null
    val isDraftTitle = host != null && activeAgent == null
    val isMultiHost = ui.hosts.size > 1

    val statusDotColor = when (host?.connectionState) {
        FfiConnectionState.ONLINE_DIRECT -> colors.directAdded
        FfiConnectionState.ONLINE_RELAY -> colors.relay
        FfiConnectionState.CONNECTING_DIRECT, FfiConnectionState.CONNECTING_RELAY -> colors.warning
        else -> colors.fgMuted
    }

    val primaryTitle = when {
        host == null -> "Orbit"
        activeAgent != null && activeAgent.title.isNotBlank() -> activeAgent.title.trim()
        else -> "New Conversation"
    }

    val subtitleText: String? = if (host == null) {
        null
    } else {
        val repo = OrbitFormatters.shortPath(ui.activeCwd)
        val hostName = host.displayName.trim().ifEmpty { host.hostId }
        val branch = resolveHeaderGitBranch(ui)
        val isConnectionHealthy = host.connectionState == FfiConnectionState.ONLINE_DIRECT ||
            host.connectionState == FfiConnectionState.ONLINE_RELAY
        when {
            !isConnectionHealthy -> {
                val status = OrbitFormatters.topBarConnectionLabel(host.connectionState, host.viaSshTunnel)
                val target = if (isMultiHost) hostName else repo
                "$status · $target"
            }
            isMultiHost -> "$hostName / $repo"
            !branch.isNullOrBlank() -> "$repo · $branch"
            else -> repo
        }
    }

    val diffFiles = ui.gitReview?.diffFiles?.takeIf { it.isNotEmpty() }
        ?: ui.agentStage?.sessionTurnDiffs.orEmpty()
    val diffFileCount = diffFiles.size
    val diffSubtitle = if (diffFileCount > 0) {
        "$diffFileCount ${if (diffFileCount == 1) "file" else "files"}"
    } else {
        null
    }
    val terminalCount = ui.hostDetail?.terminals?.size ?: 0
    val terminalSubtitle = if (terminalCount > 0) "$terminalCount active" else null

    var titleMenuExpanded by remember { mutableStateOf(false) }
    var moreMenuExpanded by remember { mutableStateOf(false) }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(colors.canvas)
            .statusBarsPadding()
            .height(OrbitSpacing.topBarHeight)
            .padding(horizontal = OrbitSpacing.lg),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
    ) {
        // 1. Leading Sidebar Toggle Circle (aligned with Desktop & iOS sidebar.left)
        Box(
            modifier = Modifier
                .size(40.dp)
                .clip(CircleShape)
                .background(colors.subtleFill, CircleShape)
                .border(OrbitSpacing.hairline, colors.hairline, CircleShape)
                .clickable {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    if (!isWideViewport) onOpenDrawer() else onTogglePermanentDrawer()
                },
            contentAlignment = Alignment.Center,
        ) {
            val showCollapsedTint = isWideViewport && ui.isPermanentDrawerCollapsed
            Icon(
                imageVector = OrbitSidebarIcon,
                contentDescription = if (!isWideViewport) {
                    "Open fleet drawer"
                } else if (ui.isPermanentDrawerCollapsed) {
                    "Expand fleet sidebar"
                } else {
                    "Collapse fleet sidebar"
                },
                tint = if (showCollapsedTint) colors.fgSecondary else colors.fgPrimary,
                modifier = Modifier.size(OrbitSpacing.iconMd),
            )
            val showApprovalBadge = ui.totalPendingApprovals > 0u &&
                (!isWideViewport || ui.isPermanentDrawerCollapsed)
            if (showApprovalBadge) {
                Box(
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .padding(top = 8.dp, end = 8.dp)
                        .size(6.dp)
                        .background(colors.warning, CircleShape)
                )
            }
        }

        // 2. Center Title + Smart Subtitle (hidden on Minimalist Zen New-Session Canvas to avoid duplicating Launchpad pills)
        Box(modifier = Modifier.weight(1f)) {
            if (host != null && !isDraftTitle) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(OrbitSpacing.sm))
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            titleMenuExpanded = true
                        }
                        .padding(horizontal = OrbitSpacing.xs, vertical = 2.dp),
                    verticalArrangement = Arrangement.Center,
                ) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        Text(
                            text = primaryTitle,
                            style = typography.subheadSemiBold,
                            color = colors.fgPrimary.copy(alpha = if (isDraftTitle) 0.76f else 1f),
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false),
                        )
                        Icon(
                            imageVector = Icons.Outlined.KeyboardArrowDown,
                            contentDescription = null,
                            tint = colors.fgMuted,
                            modifier = Modifier.size(14.dp),
                        )
                    }
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
                            text = subtitleText.orEmpty(),
                            style = typography.meta,
                            color = colors.fgSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }

                DropdownMenu(
                    expanded = titleMenuExpanded,
                    onDismissRequest = { titleMenuExpanded = false },
                    containerColor = colors.elevated,
                ) {
                    DropdownMenuItem(
                        text = {
                            Text(
                                text = "Open Folder",
                                style = typography.subheadMedium,
                                color = colors.fgPrimary,
                            )
                        },
                        leadingIcon = {
                            Icon(
                                imageVector = Icons.Outlined.Folder,
                                contentDescription = null,
                                tint = colors.fgPrimary,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                        },
                        onClick = {
                            titleMenuExpanded = false
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onOpenDirectoryBrowser()
                        },
                    )
                    DropdownMenuItem(
                        text = {
                            Text(
                                text = "Copy Path",
                                style = typography.subheadMedium,
                                color = colors.fgPrimary,
                            )
                        },
                        leadingIcon = {
                            Icon(
                                imageVector = OrbitCopyIcon,
                                contentDescription = null,
                                tint = colors.fgPrimary,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                        },
                        onClick = {
                            titleMenuExpanded = false
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onCopyPath(ui.activeCwd)
                        },
                    )
                }
            } else if (host == null) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(OrbitSpacing.sm))
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onOpenPairSheet()
                        }
                        .padding(horizontal = OrbitSpacing.xs, vertical = 2.dp),
                    verticalArrangement = Arrangement.Center,
                ) {
                    Text(
                        text = "Orbit",
                        style = typography.subheadSemiBold,
                        color = colors.fgPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = "No Host Paired",
                        style = typography.meta,
                        color = colors.fgMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }

        // 3. Trailing Controls: [ •••   ✎ ] when active, [ ••• ] on draft, [ ⚙ ] when unpaired
        if (host != null) {
            val capsuleShape = RoundedCornerShape(20.dp)
            Row(
                modifier = Modifier
                    .height(40.dp)
                    .clip(capsuleShape)
                    .background(colors.subtleFill, capsuleShape)
                    .border(OrbitSpacing.hairline, colors.hairline, capsuleShape),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box {
                    Box(
                        modifier = Modifier
                            .width(40.dp)
                            .fillMaxHeight()
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                moreMenuExpanded = true
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.MoreHoriz,
                            contentDescription = "More actions",
                            tint = colors.fgPrimary,
                            modifier = Modifier.size(OrbitSpacing.iconMd),
                        )
                    }

                    DropdownMenu(
                        expanded = moreMenuExpanded,
                        onDismissRequest = { moreMenuExpanded = false },
                        containerColor = colors.elevated,
                    ) {
                        DropdownMenuItem(
                            text = {
                                Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
                                    Text(
                                        text = if (isInlineDiffOpen) "Hide Diff Review" else "Diff Review",
                                        style = typography.subheadMedium,
                                        color = colors.fgPrimary,
                                    )
                                    if (diffSubtitle != null) {
                                        Text(
                                            text = diffSubtitle,
                                            style = typography.meta,
                                            color = colors.fgMuted,
                                        )
                                    }
                                }
                            },
                            leadingIcon = {
                                Icon(
                                    imageVector = Icons.AutoMirrored.Outlined.CompareArrows,
                                    contentDescription = null,
                                    tint = colors.fgPrimary,
                                    modifier = Modifier.size(OrbitSpacing.iconSm),
                                )
                            },
                            onClick = {
                                moreMenuExpanded = false
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onToggleDiff()
                            },
                        )

                        if (isInlineDiffOpen) {
                            DropdownMenuItem(
                                text = {
                                    Text(
                                        text = "Full Diff",
                                        style = typography.subheadMedium,
                                        color = colors.fgPrimary,
                                    )
                                },
                                leadingIcon = {
                                    Icon(
                                        imageVector = Icons.Outlined.OpenInFull,
                                        contentDescription = null,
                                        tint = colors.fgPrimary,
                                        modifier = Modifier.size(OrbitSpacing.iconSm),
                                    )
                                },
                                onClick = {
                                    moreMenuExpanded = false
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onOpenFullDiff()
                                },
                            )
                        }

                        DropdownMenuItem(
                            text = {
                                Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
                                    Text(
                                        text = "Terminal",
                                        style = typography.subheadMedium,
                                        color = colors.fgPrimary,
                                    )
                                    if (terminalSubtitle != null) {
                                        Text(
                                            text = terminalSubtitle,
                                            style = typography.meta,
                                            color = colors.fgMuted,
                                        )
                                    }
                                }
                            },
                            leadingIcon = {
                                Icon(
                                    imageVector = Icons.Outlined.Terminal,
                                    contentDescription = null,
                                    tint = colors.fgPrimary,
                                    modifier = Modifier.size(OrbitSpacing.iconSm),
                                )
                            },
                            onClick = {
                                moreMenuExpanded = false
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onOpenTerminal()
                            },
                        )

                        if (activeAgent != null) {
                            HorizontalDivider(
                                thickness = OrbitSpacing.hairline,
                                color = colors.hairline,
                            )
                            DropdownMenuItem(
                                text = {
                                    Text(
                                        text = "Archive",
                                        style = typography.subheadMedium,
                                        color = colors.dangerRemoved,
                                    )
                                },
                                leadingIcon = {
                                    Icon(
                                        imageVector = Icons.Outlined.Archive,
                                        contentDescription = null,
                                        tint = colors.dangerRemoved,
                                        modifier = Modifier.size(OrbitSpacing.iconSm),
                                    )
                                },
                                onClick = {
                                    moreMenuExpanded = false
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onArchiveSession(activeAgent.agentId)
                                },
                            )
                        }
                    }
                }

                if (activeAgent != null) {
                    Box(
                        modifier = Modifier
                            .width(40.dp)
                            .fillMaxHeight()
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onNewSession()
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = OrbitComposeIcon,
                            contentDescription = "New Conversation",
                            tint = colors.fgPrimary,
                            modifier = Modifier.size(OrbitSpacing.iconMd),
                        )
                    }
                }
            }
        } else {
            Box(
                modifier = Modifier
                    .size(40.dp)
                    .clip(CircleShape)
                    .background(colors.subtleFill, CircleShape)
                    .border(OrbitSpacing.hairline, colors.hairline, CircleShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onOpenSettings()
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Settings,
                    contentDescription = "Settings",
                    tint = colors.fgPrimary,
                    modifier = Modifier.size(OrbitSpacing.iconMd),
                )
            }
        }
    }
}
