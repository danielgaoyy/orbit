package dev.orbit.android.ui.stage

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.FolderOpen
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
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitAndroidStore
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.store.OrbitUiState
import dev.orbit.android.ui.composer.OmniComposer
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StreamStageScreen(
    ui: OrbitUiState,
    store: OrbitAndroidStore,
    onOpenContextPicker: () -> Unit,
    onOpenModelSelector: () -> Unit,
    onOpenDiffForFile: (String?) -> Unit,
    onOpenDirectoryBrowser: () -> Unit,
    onOpenPairSheet: () -> Unit,
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
    val isActiveAgentRunning = !ui.isNewSessionMode && (
        ui.activeAgent?.isRunning == true ||
            turns.any { it.hasActiveItem }
    )
    val listState = rememberLazyListState()

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
                            NewSessionHero(
                                hostName = ui.selectedHost?.displayName ?: "Remote Host",
                                activeCwd = ui.activeCwd,
                                gitBranch = ui.gitReview?.branch,
                                changedCount = ui.gitReview?.changedFiles?.size ?: 0,
                                onBrowseDirectory = onOpenDirectoryBrowser,
                                onStarterPrompt = { text, slash ->
                                    if (slash != null) store.setSlashCommand(slash)
                                    store.updateComposerText(text)
                                },
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
                            items(
                                items = turns,
                                key = { it.key },
                            ) { turn ->
                                TurnItem(
                                    turn = turn,
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

            // Floating 32.dp-tall `↓ Streaming` / `↓ Latest` capsule centered 8.dp above OmniComposer
            androidx.compose.animation.AnimatedVisibility(
                visible = turns.isNotEmpty() && canScrollForward,
                enter = fadeIn(),
                exit = fadeOut(),
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(bottom = OrbitSpacing.sm),
            ) {
                val pillShape = RoundedCornerShape(OrbitSpacing.lg)
                val borderTint = if (isActiveAgentRunning) {
                    colors.primary.copy(alpha = 0.6f)
                } else {
                    colors.hairline
                }
                Surface(
                    shape = pillShape,
                    color = colors.elevated,
                    shadowElevation = OrbitSpacing.xs,
                    modifier = Modifier
                        .height(OrbitSpacing.xxl)
                        .border(OrbitSpacing.hairline, borderTint, pillShape)
                        .clip(pillShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            scope.launch {
                                if (turns.isNotEmpty()) {
                                    listState.animateScrollToItem(turns.lastIndex, scrollOffset = 100_000)
                                }
                            }
                        },
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = OrbitSpacing.md),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    ) {
                        if (isActiveAgentRunning) {
                            BreathingDot(color = colors.primary)
                        }
                        Text(
                            text = if (isActiveAgentRunning) "↓ Streaming" else "↓ Latest",
                            style = typography.metaSemiBold,
                            color = if (isActiveAgentRunning) colors.primary else colors.fgPrimary,
                            maxLines = 1,
                            softWrap = false,
                        )
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
                tint = Color.White,
                modifier = Modifier.size(OrbitSpacing.iconMd),
            )
            Text(
                text = "Pair Host",
                style = typography.bodySemiBold,
                color = Color.White,
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
private fun NewSessionHero(
    hostName: String,
    activeCwd: String,
    gitBranch: String?,
    changedCount: Int,
    onBrowseDirectory: () -> Unit,
    onStarterPrompt: (text: String, slash: String?) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val pillShape = RoundedCornerShape(OrbitSpacing.md)

    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = OrbitSpacing.xl),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
    ) {
        Text(
            text = "New Session",
            style = typography.title,
            color = colors.fgPrimary,
        )

        // Tappable CWD & Git Summary Pill
        Row(
            modifier = Modifier
                .clip(pillShape)
                .background(colors.subtleFill, pillShape)
                .border(OrbitSpacing.hairline, colors.hairline, pillShape)
                .clickable { onBrowseDirectory() }
                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            Icon(
                imageVector = Icons.Outlined.FolderOpen,
                contentDescription = "Choose directory",
                tint = colors.fgSecondary,
                modifier = Modifier.size(OrbitSpacing.iconSm),
            )
            Text(
                text = "$hostName · ${OrbitFormatters.shortPath(activeCwd)}",
                style = typography.codeMedium,
                color = colors.fgPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (!gitBranch.isNullOrBlank()) {
                Text(
                    text = "($gitBranch)",
                    style = typography.meta,
                    color = colors.fgSecondary,
                )
            }
        }

        val gitSummary = if (changedCount == 0) {
            "Working tree clean"
        } else {
            "$changedCount modified ${if (changedCount == 1) "file" else "files"} in working tree"
        }
        Text(
            text = gitSummary,
            style = typography.meta,
            color = colors.fgMuted,
        )

        Spacer(modifier = Modifier.height(OrbitSpacing.xs))

        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            val starters = listOf(
                Triple("Plan feature", "Outline a clean implementation plan for ", "/plan"),
                Triple("Review diff", "Audit uncommitted working tree changes for bugs", "/review"),
                Triple("Run tests", "Run the project test suite and summarize any failures", null),
            )
            starters.forEach { (label, prompt, slash) ->
                val chipShape = RoundedCornerShape(OrbitSpacing.md)
                Box(
                    modifier = Modifier
                        .heightIn(min = 40.dp)
                        .clip(chipShape)
                        .background(colors.subtleFill, chipShape)
                        .border(OrbitSpacing.hairline, colors.hairline, chipShape)
                        .clickable { onStarterPrompt(prompt, slash) }
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = label,
                        style = typography.subheadMedium,
                        color = colors.fgSecondary,
                    )
                }
            }
        }
    }
}
