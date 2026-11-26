package dev.orbit.android.ui.sheets

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.AccountTree
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.Bolt
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.DeleteOutline
import androidx.compose.material.icons.outlined.FavoriteBorder
import androidx.compose.material.icons.outlined.Flag
import androidx.compose.material.icons.outlined.Pause
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.RadioButtonUnchecked
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material.icons.outlined.Stop
import androidx.compose.material.icons.outlined.Timer
import androidx.compose.material.icons.outlined.Visibility
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import dev.orbit.android.R
import dev.orbit.android.store.OrbitUiState
import dev.orbit.android.store.OrbitAndroidStore
import androidx.compose.ui.res.stringResource
import androidx.compose.material3.OutlinedTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlinx.coroutines.delay
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiConditionalTimerRecord
import uniffi.orbit_ffi_uniffi.FfiConditionalTimerState
import uniffi.orbit_ffi_uniffi.FfiGoalControlAction
import uniffi.orbit_ffi_uniffi.FfiGoalRunRecord
import uniffi.orbit_ffi_uniffi.FfiGoalStatus
import uniffi.orbit_ffi_uniffi.FfiMonitorKind
import uniffi.orbit_ffi_uniffi.FfiMonitorStatus
import uniffi.orbit_ffi_uniffi.FfiMonitorWatchRecord
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiScheduleKind
import uniffi.orbit_ffi_uniffi.FfiScheduleRunRecord
import uniffi.orbit_ffi_uniffi.FfiScheduleRunStatus
import uniffi.orbit_ffi_uniffi.FfiScheduleSpec
import uniffi.orbit_ffi_uniffi.FfiScheduleStatus
import uniffi.orbit_ffi_uniffi.FfiSessionMode
import uniffi.orbit_ffi_uniffi.FfiWorkflowKind
import uniffi.orbit_ffi_uniffi.FfiWorkflowRunRecord
import uniffi.orbit_ffi_uniffi.FfiWorkflowRunStatus
import uniffi.orbit_ffi_uniffi.FfiWorkflowStepStatus
import uniffi.orbit_ffi_uniffi.PermissionRuleDecision
import uniffi.orbit_ffi_uniffi.PermissionRulePatternKind
import uniffi.orbit_ffi_uniffi.PermissionRuleScopeKind
import uniffi.orbit_ffi_uniffi.PermissionRuleSource
import uniffi.orbit_ffi_uniffi.PermissionRuleToolKind
import uniffi.orbit_ffi_uniffi.UpsertPermissionRuleInput

private enum class AutomationTab(val label: Int) {
    GOALS(R.string.auto_tab_goals), SCHEDULES(R.string.auto_tab_schedules),
    MONITORS(R.string.auto_tab_monitors), TRIGGERS(R.string.triggers),
    WORKFLOWS(R.string.auto_tab_workflows), HOOKS(R.string.hooks),
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AutomationsSheet(
    ui: OrbitUiState,
    store: OrbitAndroidStore,
    schedules: List<FfiScheduleSpec>,
    scheduleRunsById: Map<String, List<FfiScheduleRunRecord>>,
    conditionalTimers: List<FfiConditionalTimerRecord>,
    goalRuns: List<FfiGoalRunRecord>,
    monitorWatches: List<FfiMonitorWatchRecord>,
    workflowRuns: List<FfiWorkflowRunRecord>,
    activeCwd: String,
    defaultProvider: FfiProviderKind,
    onRefreshAll: () -> Unit,
    onCreateSchedule: (
        name: String,
        cronExpr: String,
        prompt: String,
        cwd: String,
        provider: FfiProviderKind,
        mode: FfiSessionMode,
        worktree: Boolean,
        archiveOnFinish: Boolean,
    ) -> Unit,
    onPauseSchedule: (String) -> Unit,
    onResumeSchedule: (String) -> Unit,
    onRunScheduleOnce: (String) -> Unit,
    onDeleteSchedule: (String) -> Unit,
    onFetchScheduleRuns: (String) -> Unit,
    onCancelTimer: (String) -> Unit,
    onFireTimerNow: (String) -> Unit,
    onControlGoal: (goalId: String, action: FfiGoalControlAction) -> Unit,
    onStopMonitor: (String) -> Unit,
    onCancelWorkflow: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val groupedBg = if (colors.isDark) colors.canvas else colors.drawer

    var activeTab by remember { mutableStateOf(AutomationTab.SCHEDULES) }
    var isCreatingSchedule by remember { mutableStateOf(false) }
    var expandedScheduleIds by remember { mutableStateOf(emptySet<String>()) }
    var confirmDeleteSchedule by remember { mutableStateOf<FfiScheduleSpec?>(null) }

    LaunchedEffect(Unit) {
        onRefreshAll()
    }

    LaunchedEffect(activeTab, ui.selectedHostId, activeCwd) {
        if (activeTab == AutomationTab.HOOKS) {
            store.refreshPermissionRulesAndGuardian()
        }
    }

    BackHandler(enabled = isCreatingSchedule) {
        isCreatingSchedule = false
    }

    confirmDeleteSchedule?.let { sched ->
        AlertDialog(
            onDismissRequest = { confirmDeleteSchedule = null },
            containerColor = colors.elevated,
            titleContentColor = colors.fgPrimary,
            textContentColor = colors.fgSecondary,
            title = { Text("Delete automation?", style = typography.title) },
            text = {
                Text(
                    text = "Remove \"${sched.name}\" from this host daemon?",
                    style = typography.subhead,
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        onDeleteSchedule(sched.scheduleId)
                        confirmDeleteSchedule = null
                    },
                ) {
                    Text("Delete", color = colors.dangerRemoved, style = typography.subheadSemiBold)
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmDeleteSchedule = null }) {
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
                if (isCreatingSchedule) {
                    Box(
                        modifier = Modifier
                            .size(36.dp)
                            .clip(CircleShape)
                            .clickable { isCreatingSchedule = false },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Outlined.ArrowBack,
                            contentDescription = "Back",
                            tint = colors.fgPrimary,
                            modifier = Modifier.size(20.dp),
                        )
                    }
                }
                Text(
                    text = if (isCreatingSchedule) "New Automation" else "Automations",
                    style = typography.title,
                    color = colors.fgPrimary,
                    modifier = Modifier.weight(1f),
                )
                if (!isCreatingSchedule) {
                    if (activeTab == AutomationTab.SCHEDULES) {
                        Box(
                            modifier = Modifier
                                .size(36.dp)
                                .clip(CircleShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    isCreatingSchedule = true
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Add,
                                contentDescription = "Create automation",
                                tint = colors.primary,
                                modifier = Modifier.size(20.dp),
                            )
                        }
                    }
                    Box(
                        modifier = Modifier
                            .size(36.dp)
                            .clip(CircleShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onRefreshAll()
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Refresh,
                            contentDescription = "Refresh automations",
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

            if (isCreatingSchedule) {
                HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                CreateScheduleForm(
                    activeCwd = activeCwd,
                    defaultProvider = defaultProvider,
                    onSave = { name, cronExpr, prompt, worktree, archiveOnFinish ->
                        onCreateSchedule(
                            name,
                            cronExpr,
                            prompt,
                            activeCwd,
                            defaultProvider,
                            FfiSessionMode.AcceptEdits,
                            worktree,
                            archiveOnFinish,
                        )
                        isCreatingSchedule = false
                    },
                    modifier = Modifier.weight(1f),
                )
            } else {
                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
                    AutomationTab.entries.forEach { tab ->
                        TextButton(onClick = { activeTab = tab }) {
                            Text(stringResource(tab.label), color = if (activeTab == tab) colors.fgPrimary else colors.fgMuted,
                                style = if (activeTab == tab) typography.subheadSemiBold else typography.subhead)
                        }
                    }
                }
                if (activeTab !in listOf(AutomationTab.SCHEDULES, AutomationTab.HOOKS)) {
                    NewAutomationAction(activeTab, ui, store)
                }
                HorizontalDivider(
                    thickness = OrbitSpacing.hairline,
                    color = colors.hairline,
                    modifier = Modifier.padding(top = OrbitSpacing.xs),
                )

                when (activeTab) {
                    AutomationTab.SCHEDULES -> {
                        if (schedules.isEmpty()) {
                            AutomationEmptyState(
                                icon = Icons.Outlined.Schedule,
                                title = "No Automations Yet",
                                subtitle = "Run recurring checks, nightly reviews, or scheduled agent runs on your host daemon.",
                                actionLabel = "Create Automation",
                                onAction = { isCreatingSchedule = true },
                                modifier = Modifier.weight(1f),
                            )
                        } else {
                            LazyColumn(
                                modifier = Modifier.weight(1f),
                                contentPadding = PaddingValues(OrbitSpacing.lg),
                                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                            ) {
                                items(schedules, key = { it.scheduleId }) { sched ->
                                    val expanded = expandedScheduleIds.contains(sched.scheduleId)
                                    ScheduleCard(
                                        schedule = sched,
                                        runs = scheduleRunsById[sched.scheduleId].orEmpty(),
                                        isExpanded = expanded,
                                        onTogglePause = {
                                            if (sched.status == FfiScheduleStatus.ACTIVE) {
                                                onPauseSchedule(sched.scheduleId)
                                            } else {
                                                onResumeSchedule(sched.scheduleId)
                                            }
                                        },
                                        onRunNow = { onRunScheduleOnce(sched.scheduleId) },
                                        onToggleHistory = {
                                            if (expanded) {
                                                expandedScheduleIds = expandedScheduleIds - sched.scheduleId
                                            } else {
                                                expandedScheduleIds = expandedScheduleIds + sched.scheduleId
                                                onFetchScheduleRuns(sched.scheduleId)
                                            }
                                        },
                                        onDelete = { confirmDeleteSchedule = sched },
                                    )
                                }
                            }
                        }
                    }

                    AutomationTab.TRIGGERS -> {
                        if (conditionalTimers.isEmpty()) {
                            AutomationEmptyState(
                                icon = Icons.Outlined.Timer,
                                title = "No Active Timers",
                                subtitle = "One-shot conditional timers armed by agents will appear here for live inspection or cancellation.",
                                modifier = Modifier.weight(1f),
                            )
                        } else {
                            LazyColumn(
                                modifier = Modifier.weight(1f),
                                contentPadding = PaddingValues(OrbitSpacing.lg),
                                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                            ) {
                                items(conditionalTimers, key = { it.timerId }) { timer ->
                                    ConditionalTimerCard(
                                        timer = timer,
                                        onFireNow = { onFireTimerNow(timer.timerId) },
                                        onCancel = { onCancelTimer(timer.timerId) },
                                    )
                                }
                            }
                        }
                    }

                    AutomationTab.GOALS -> {
                        if (goalRuns.isEmpty()) {
                            AutomationEmptyState(
                                icon = Icons.Outlined.Flag,
                                title = stringResource(R.string.auto_empty_goals_title),
                                subtitle = stringResource(R.string.auto_empty_goals_subtitle),
                                modifier = Modifier.weight(1f),
                            )
                        } else {
                            LazyColumn(
                                modifier = Modifier.weight(1f),
                                contentPadding = PaddingValues(OrbitSpacing.lg),
                                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                            ) {
                                items(goalRuns, key = { it.goalId }) { goal ->
                                    GoalRunCard(goal) { onControlGoal(goal.goalId, it) }
                                }
                            }
                        }
                    }

                    AutomationTab.MONITORS -> {
                        if (monitorWatches.isEmpty()) {
                            AutomationEmptyState(
                                icon = Icons.Outlined.Visibility,
                                title = stringResource(R.string.auto_empty_monitors_title),
                                subtitle = stringResource(R.string.auto_empty_monitors_subtitle),
                                modifier = Modifier.weight(1f),
                            )
                        } else {
                            LazyColumn(
                                modifier = Modifier.weight(1f),
                                contentPadding = PaddingValues(OrbitSpacing.lg),
                                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                            ) {
                                items(monitorWatches, key = { it.monitorId }) { watch ->
                                    MonitorWatchCard(watch) { onStopMonitor(watch.monitorId) }
                                }
                            }
                        }
                    }

                    AutomationTab.HOOKS -> {
                        HooksRulesAndGuardianTab(
                            ui = ui,
                            activeCwd = activeCwd,
                            store = store,
                            modifier = Modifier.weight(1f),
                        )
                    }

                    AutomationTab.WORKFLOWS -> {
                        if (workflowRuns.isEmpty()) {
                            AutomationEmptyState(
                                icon = Icons.Outlined.AccountTree,
                                title = "No Workflow Runs",
                                subtitle = "Batch CSV fan-outs, DAG pipelines, and Best-of-N arena runs appear here.",
                                modifier = Modifier.weight(1f),
                            )
                        } else {
                            LazyColumn(
                                modifier = Modifier.weight(1f),
                                contentPadding = PaddingValues(OrbitSpacing.lg),
                                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                            ) {
                                items(workflowRuns, key = { it.runId }) { run ->
                                    WorkflowRunCard(
                                        run = run,
                                        onCancel = { onCancelWorkflow(run.runId) },
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

@Composable
private fun ScheduleCard(
    schedule: FfiScheduleSpec,
    runs: List<FfiScheduleRunRecord>,
    isExpanded: Boolean,
    onTogglePause: () -> Unit,
    onRunNow: () -> Unit,
    onToggleHistory: () -> Unit,
    onDelete: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val cardShape = RoundedCornerShape(14.dp)
    val isActive = schedule.status == FfiScheduleStatus.ACTIVE
    val isHeartbeat = schedule.kind == FfiScheduleKind.HEARTBEAT
    val statusColor = when (schedule.status) {
        FfiScheduleStatus.ACTIVE -> colors.directAdded
        FfiScheduleStatus.PAUSED -> colors.warning
        FfiScheduleStatus.COMPLETED -> colors.fgMuted
    }
    val statusText = when (schedule.status) {
        FfiScheduleStatus.ACTIVE -> "Active"
        FfiScheduleStatus.PAUSED -> "Paused"
        FfiScheduleStatus.COMPLETED -> "Completed"
    }

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
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            Icon(
                imageVector = if (isHeartbeat) Icons.Outlined.FavoriteBorder else Icons.Outlined.Schedule,
                contentDescription = null,
                tint = if (isActive) colors.directAdded else colors.fgMuted,
                modifier = Modifier
                    .padding(top = 2.dp)
                    .size(16.dp),
            )
            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Text(
                        text = schedule.name,
                        style = typography.subheadSemiBold,
                        color = colors.fgPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    StatusPill(label = statusText, color = statusColor)
                }
                Text(
                    text = "${schedule.cronExpr} · ${schedule.runCount} ${if (schedule.runCount == 1u) "run" else "runs"}",
                    style = typography.code,
                    color = colors.fgMuted,
                )
                Text(
                    text = schedule.prompt,
                    style = typography.meta,
                    color = colors.fgSecondary,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            CompactActionPill(
                icon = if (isActive) Icons.Outlined.Pause else Icons.Outlined.PlayArrow,
                label = if (isActive) "Pause" else "Resume",
                tint = colors.fgPrimary,
                bg = colors.subtleFill,
                onClick = onTogglePause,
            )
            CompactActionPill(
                icon = Icons.Outlined.Bolt,
                label = "Run Now",
                tint = colors.primary,
                bg = colors.subtleFill,
                onClick = onRunNow,
            )
            Spacer(modifier = Modifier.weight(1f))
            Text(
                text = if (isExpanded) "Hide Runs" else "History",
                style = typography.metaMedium,
                color = colors.fgSecondary,
                modifier = Modifier
                    .clip(RoundedCornerShape(8.dp))
                    .clickable(onClick = onToggleHistory)
                    .padding(horizontal = 6.dp, vertical = 4.dp),
            )
            Box(
                modifier = Modifier
                    .size(28.dp)
                    .clip(CircleShape)
                    .clickable(onClick = onDelete),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.DeleteOutline,
                    contentDescription = "Delete automation",
                    tint = colors.dangerRemoved,
                    modifier = Modifier.size(16.dp),
                )
            }
        }

        AnimatedVisibility(visible = isExpanded) {
            Column(
                modifier = Modifier.fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                if (runs.isEmpty()) {
                    Text(
                        text = "No execution history yet.",
                        style = typography.meta,
                        color = colors.fgMuted,
                    )
                } else {
                    runs.take(8).forEach { run ->
                        val dotColor = when (run.status) {
                            FfiScheduleRunStatus.RUNNING -> colors.warning
                            FfiScheduleRunStatus.SUCCEEDED -> colors.directAdded
                            FfiScheduleRunStatus.FAILED -> colors.dangerRemoved
                            FfiScheduleRunStatus.SKIPPED -> colors.fgMuted
                        }
                        Row(
                            verticalAlignment = Alignment.Top,
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Box(
                                modifier = Modifier
                                    .padding(top = 5.dp)
                                    .size(6.dp)
                                    .clip(CircleShape)
                                    .background(dotColor),
                            )
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    text = run.summary ?: run.error ?: run.status.name.lowercase(),
                                    style = typography.meta,
                                    color = colors.fgPrimary,
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Text(
                                    text = OrbitFormatters.compactRelativeTime(run.startedAtRfc3339),
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

@Composable
private fun ConditionalTimerCard(
    timer: FfiConditionalTimerRecord,
    onFireNow: () -> Unit,
    onCancel: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val cardShape = RoundedCornerShape(14.dp)
    val isArmed = timer.state == FfiConditionalTimerState.ARMED
    val statusColor = when (timer.state) {
        FfiConditionalTimerState.ARMED -> colors.warning
        FfiConditionalTimerState.FIRED -> colors.directAdded
        FfiConditionalTimerState.CANCELED_EARLY -> colors.relay
        FfiConditionalTimerState.CANCELED_MANUAL -> colors.fgMuted
    }
    val statusLabel = when (timer.state) {
        FfiConditionalTimerState.ARMED -> "Armed"
        FfiConditionalTimerState.FIRED -> "Fired"
        FfiConditionalTimerState.CANCELED_EARLY -> "Woken Early"
        FfiConditionalTimerState.CANCELED_MANUAL -> "Canceled"
    }
    var nowMs by remember(timer.timerId, isArmed) { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(timer.timerId, isArmed) {
        if (!isArmed) return@LaunchedEffect
        while (true) {
            nowMs = System.currentTimeMillis()
            if (nowMs >= timer.fireAtMs.toLong()) break
            delay(1_000L)
        }
    }
    val remainingLabel = remember(timer.fireAtMs, timer.state, nowMs) {
        if (!isArmed) {
            timer.resolvedReason ?: "${timer.durationSeconds}s"
        } else {
            val leftSecs = ((timer.fireAtMs.toLong() - nowMs) / 1000L).coerceAtLeast(0L)
            when {
                leftSecs < 60L -> "${leftSecs}s remaining"
                leftSecs < 3600L -> "${leftSecs / 60L}m ${leftSecs % 60L}s left"
                else -> "${leftSecs / 3600L}h ${(leftSecs % 3600L) / 60L}m left"
            }
        }
    }

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
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(
                imageVector = Icons.Outlined.Timer,
                contentDescription = null,
                tint = statusColor,
                modifier = Modifier.size(16.dp),
            )
            Text(
                text = timer.conditionLabel,
                style = typography.subheadSemiBold,
                color = colors.fgPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            StatusPill(label = statusLabel, color = statusColor)
        }
        Text(
            text = remainingLabel,
            style = typography.code,
            color = colors.fgMuted,
        )
        Text(
            text = timer.prompt,
            style = typography.meta,
            color = colors.fgSecondary,
            maxLines = 3,
            overflow = TextOverflow.Ellipsis,
        )
        if (isArmed) {
            Row(horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm)) {
                CompactActionPill(
                    icon = Icons.Outlined.Bolt,
                    label = "Fire Now",
                    tint = colors.primary,
                    bg = colors.subtleFill,
                    onClick = onFireNow,
                )
                CompactActionPill(
                    icon = Icons.Outlined.Close,
                    label = "Cancel",
                    tint = colors.dangerRemoved,
                    bg = colors.dangerRemoved.copy(alpha = 0.12f),
                    onClick = onCancel,
                )
            }
        }
    }
}

@Composable
private fun GoalRunCard(
    goal: FfiGoalRunRecord,
    onControl: (FfiGoalControlAction) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val cardShape = RoundedCornerShape(14.dp)
    val isActive = goal.status == FfiGoalStatus.RUNNING || goal.status == FfiGoalStatus.VERIFYING
    val isPaused = goal.status == FfiGoalStatus.PAUSED
    val statusColor = when (goal.status) {
        FfiGoalStatus.RUNNING, FfiGoalStatus.VERIFYING -> colors.directAdded
        FfiGoalStatus.PAUSED -> colors.warning
        FfiGoalStatus.SUCCEEDED -> colors.directAdded
        FfiGoalStatus.FAILED -> colors.dangerRemoved
        FfiGoalStatus.CANCELED -> colors.fgMuted
    }

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
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(
                imageVector = Icons.Outlined.Flag,
                contentDescription = null,
                tint = statusColor,
                modifier = Modifier
                    .padding(top = 2.dp)
                    .size(16.dp),
            )
            Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    text = goal.objective,
                    style = typography.subheadSemiBold,
                    color = colors.fgPrimary,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = "Iteration ${goal.currentIteration}/${goal.maxIterations}",
                    style = typography.code,
                    color = colors.fgMuted,
                )
                if (isPaused && !goal.pauseReason.isNullOrBlank()) {
                    Text(
                        text = goal.pauseReason!!,
                        style = typography.meta,
                        color = colors.warning,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            StatusPill(label = goal.status.name.lowercase().replaceFirstChar { it.uppercase() }, color = statusColor)
        }

        if (goal.criteria.isNotEmpty()) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                goal.criteria.take(4).forEach { criterion ->
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Icon(
                            imageVector = if (criterion.passed) Icons.Outlined.CheckCircle else Icons.Outlined.RadioButtonUnchecked,
                            contentDescription = null,
                            tint = if (criterion.passed) colors.directAdded else colors.fgMuted,
                            modifier = Modifier.size(13.dp),
                        )
                        Text(
                            text = criterion.label,
                            style = typography.meta,
                            color = colors.fgSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        }

        if (isActive || isPaused) {
            Row(horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm)) {
                CompactActionPill(
                    icon = if (isPaused) Icons.Outlined.PlayArrow else Icons.Outlined.Pause,
                    label = if (isPaused) "Resume" else "Pause",
                    tint = colors.fgPrimary,
                    bg = colors.subtleFill,
                    onClick = {
                        onControl(if (isPaused) FfiGoalControlAction.RESUME else FfiGoalControlAction.PAUSE)
                    },
                )
                CompactActionPill(
                    icon = Icons.Outlined.Stop,
                    label = "Stop",
                    tint = colors.dangerRemoved,
                    bg = colors.dangerRemoved.copy(alpha = 0.12f),
                    onClick = { onControl(FfiGoalControlAction.CANCEL) },
                )
            }
        }
    }
}

@Composable
private fun MonitorWatchCard(
    watch: FfiMonitorWatchRecord,
    onStop: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val cardShape = RoundedCornerShape(14.dp)
    val isRunning = watch.status == FfiMonitorStatus.ACTIVE || watch.status == FfiMonitorStatus.TRIGGERED
    val statusColor = when (watch.status) {
        FfiMonitorStatus.ACTIVE -> colors.directAdded
        FfiMonitorStatus.TRIGGERED -> colors.warning
        FfiMonitorStatus.ERROR -> colors.dangerRemoved
        else -> colors.fgMuted
    }
    val kindDetail = when (val k = watch.kind) {
        is FfiMonitorKind.CommandStream -> k.command
        is FfiMonitorKind.LogFileTail -> k.path
        is FfiMonitorKind.HttpHealth -> k.url
    }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(cardShape)
            .background(colors.elevated, cardShape)
            .border(OrbitSpacing.hairline, colors.hairline, cardShape)
            .padding(OrbitSpacing.md),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(
                imageVector = Icons.Outlined.Visibility,
                contentDescription = null,
                tint = statusColor,
                modifier = Modifier.size(16.dp),
            )
            Text(
                text = watch.name,
                style = typography.subheadSemiBold,
                color = colors.fgPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            StatusPill(label = watch.status.name.lowercase().replaceFirstChar { it.uppercase() }, color = statusColor)
        }
        Text(
            text = "$kindDetail · ${watch.matchCount} matches",
            style = typography.code,
            color = colors.fgMuted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        watch.lastEventPreview?.takeIf { it.isNotBlank() }?.let { preview ->
            Text(
                text = preview,
                style = typography.meta,
                color = colors.fgSecondary,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (isRunning) {
            Spacer(modifier = Modifier.height(2.dp))
            CompactActionPill(
                icon = Icons.Outlined.Stop,
                label = "Stop Monitor",
                tint = colors.dangerRemoved,
                bg = colors.dangerRemoved.copy(alpha = 0.12f),
                onClick = onStop,
            )
        }
    }
}

@Composable
private fun WorkflowRunCard(
    run: FfiWorkflowRunRecord,
    onCancel: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val cardShape = RoundedCornerShape(14.dp)
    val isActive = run.status == FfiWorkflowRunStatus.PENDING || run.status == FfiWorkflowRunStatus.RUNNING
    val statusColor = when (run.status) {
        FfiWorkflowRunStatus.RUNNING, FfiWorkflowRunStatus.PENDING -> colors.warning
        FfiWorkflowRunStatus.SUCCEEDED -> colors.directAdded
        FfiWorkflowRunStatus.PARTIAL_FAILURE -> colors.warning
        FfiWorkflowRunStatus.FAILED -> colors.dangerRemoved
        FfiWorkflowRunStatus.CANCELED -> colors.fgMuted
    }
    val kindLabel = when (run.kind) {
        FfiWorkflowKind.BATCH_CSV -> "Batch CSV"
        FfiWorkflowKind.DAG -> "DAG"
        FfiWorkflowKind.BEST_OF_N -> "Best-of-N"
    }
    val completedCount = run.steps.count {
        it.status == FfiWorkflowStepStatus.SUCCEEDED
    }

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
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(
                imageVector = Icons.Outlined.AccountTree,
                contentDescription = null,
                tint = colors.fgSecondary,
                modifier = Modifier.size(16.dp),
            )
            Text(
                text = run.name,
                style = typography.subheadSemiBold,
                color = colors.fgPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            StatusPill(label = kindLabel, color = colors.relay)
            StatusPill(label = run.status.name.lowercase().replace('_', ' ').replaceFirstChar { it.uppercase() }, color = statusColor)
        }

        Text(
            text = "$completedCount/${run.steps.size} steps succeeded · ${OrbitFormatters.shortPath(run.workspaceCwd)}",
            style = typography.code,
            color = colors.fgMuted,
        )

        if (run.steps.isNotEmpty()) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                run.steps.take(6).forEach { step ->
                    val stepDot = when (step.status) {
                        FfiWorkflowStepStatus.RUNNING, FfiWorkflowStepStatus.VERIFYING -> colors.warning
                        FfiWorkflowStepStatus.SUCCEEDED -> colors.directAdded
                        FfiWorkflowStepStatus.FAILED -> colors.dangerRemoved
                        else -> colors.fgMuted
                    }
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Box(
                            modifier = Modifier
                                .size(6.dp)
                                .clip(CircleShape)
                                .background(stepDot),
                        )
                        Text(
                            text = step.name,
                            style = typography.metaMedium,
                            color = colors.fgPrimary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        if (step.isWinner) {
                            StatusPill(label = "Winner", color = colors.directAdded)
                        } else if (step.diffFiles > 0u) {
                            Text(
                                text = "+${step.diffAdditions} -${step.diffDeletions}",
                                style = typography.code,
                                color = colors.fgMuted,
                            )
                        }
                    }
                }
            }
        }

        if (isActive) {
            CompactActionPill(
                icon = Icons.Outlined.Stop,
                label = "Cancel Run",
                tint = colors.dangerRemoved,
                bg = colors.dangerRemoved.copy(alpha = 0.12f),
                onClick = onCancel,
            )
        }
    }
}

@Composable
private fun CreateScheduleForm(
    activeCwd: String,
    defaultProvider: FfiProviderKind,
    onSave: (name: String, cronExpr: String, prompt: String, worktree: Boolean, archiveOnFinish: Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val cardShape = RoundedCornerShape(14.dp)

    var name by remember { mutableStateOf("") }
    var cronExpr by remember { mutableStateOf("daily 09:00") }
    var prompt by remember { mutableStateOf("") }
    var useWorktree by remember { mutableStateOf(true) }
    var archiveOnFinish by remember { mutableStateOf(true) }

    val presets = remember { listOf("15m", "1h", "daily 09:00", "weekdays 09:00") }
    val templates = remember {
        listOf(
            Triple("Daily Digest", "daily 09:00", "Review yesterday's commits, open branches, and TODOs. Summarize risks and priorities."),
            Triple("Hourly Tests", "1h", "Run the test suite and summarize any regressions or flaky tests."),
            Triple("Dep Audit", "weekdays 09:00", "Check for security advisories and outdated dependencies in this workspace."),
        )
    }

    val canSave = name.isNotBlank() && cronExpr.isNotBlank() && prompt.isNotBlank()

    Column(
        modifier = modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(OrbitSpacing.lg),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
    ) {
        // Quick templates
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            templates.forEach { (tName, tCron, tPrompt) ->
                Box(
                    modifier = Modifier
                        .clip(CircleShape)
                        .background(colors.elevated, CircleShape)
                        .border(OrbitSpacing.hairline, colors.hairline, CircleShape)
                        .clickable {
                            name = tName
                            cronExpr = tCron
                            prompt = tPrompt
                        }
                        .padding(horizontal = 12.dp, vertical = 6.dp),
                ) {
                    Text(
                        text = tName,
                        style = typography.metaMedium,
                        color = colors.fgPrimary,
                    )
                }
            }
        }

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clip(cardShape)
                .background(colors.elevated, cardShape)
                .border(OrbitSpacing.hairline, colors.hairline, cardShape),
        ) {
            Column(
                modifier = Modifier.padding(OrbitSpacing.md),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(text = "Name", style = typography.metaMedium, color = colors.fgMuted)
                BasicTextField(
                    value = name,
                    onValueChange = { name = it },
                    singleLine = true,
                    textStyle = typography.subhead.copy(color = colors.fgPrimary),
                    cursorBrush = SolidColor(colors.primary),
                    modifier = Modifier.fillMaxWidth(),
                    decorationBox = { inner ->
                        if (name.isEmpty()) {
                            Text("e.g. Morning Repo Digest", style = typography.subhead, color = colors.fgMuted)
                        }
                        inner()
                    },
                )
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            Column(
                modifier = Modifier.padding(OrbitSpacing.md),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Text(text = "Schedule (Cron or Preset)", style = typography.metaMedium, color = colors.fgMuted)
                BasicTextField(
                    value = cronExpr,
                    onValueChange = { cronExpr = it },
                    singleLine = true,
                    textStyle = typography.code.copy(color = colors.fgPrimary),
                    cursorBrush = SolidColor(colors.primary),
                    modifier = Modifier.fillMaxWidth(),
                )
                Row(
                    modifier = Modifier.horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    presets.forEach { preset ->
                        val active = cronExpr == preset
                        Box(
                            modifier = Modifier
                                .clip(CircleShape)
                                .background(if (active) colors.primary else colors.subtleFill, CircleShape)
                                .clickable { cronExpr = preset }
                                .padding(horizontal = 10.dp, vertical = 4.dp),
                        ) {
                            Text(
                                text = preset,
                                style = typography.code,
                                color = if (active) colors.onPrimary else colors.fgSecondary,
                            )
                        }
                    }
                }
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.md, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Run in isolated Git Worktree",
                    style = typography.subhead,
                    color = colors.fgPrimary,
                    modifier = Modifier.weight(1f),
                )
                Switch(
                    checked = useWorktree,
                    onCheckedChange = { useWorktree = it },
                    colors = SwitchDefaults.colors(checkedTrackColor = colors.directAdded),
                )
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.md, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Archive session on finish",
                    style = typography.subhead,
                    color = colors.fgPrimary,
                    modifier = Modifier.weight(1f),
                )
                Switch(
                    checked = archiveOnFinish,
                    onCheckedChange = { archiveOnFinish = it },
                    colors = SwitchDefaults.colors(checkedTrackColor = colors.directAdded),
                )
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            Column(
                modifier = Modifier.padding(OrbitSpacing.md),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Text(
                    text = "Prompt (${OrbitFormatters.providerShortLabel(defaultProvider)} · ${OrbitFormatters.shortPath(activeCwd)})",
                    style = typography.metaMedium,
                    color = colors.fgMuted,
                )
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 96.dp)
                        .clip(RoundedCornerShape(10.dp))
                        .background(colors.subtleFill)
                        .padding(10.dp),
                ) {
                    if (prompt.isEmpty()) {
                        Text(
                            text = "Describe what this automation should do on each run…",
                            style = typography.subhead,
                            color = colors.fgMuted,
                        )
                    }
                    BasicTextField(
                        value = prompt,
                        onValueChange = { prompt = it },
                        textStyle = typography.subhead.copy(color = colors.fgPrimary),
                        cursorBrush = SolidColor(colors.primary),
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
            }
        }

        Box(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = OrbitSpacing.touchTarget)
                .clip(RoundedCornerShape(12.dp))
                .background(if (canSave) colors.primary else colors.fgMuted.copy(alpha = 0.25f))
                .clickable(enabled = canSave) {
                    onSave(name.trim(), cronExpr.trim(), prompt.trim(), useWorktree, archiveOnFinish)
                },
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = "Create Automation",
                style = typography.subheadSemiBold,
                color = colors.onPrimary,
            )
        }
    }
}

@Composable
private fun AutomationEmptyState(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    subtitle: String,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    Box(
        modifier = modifier
            .fillMaxWidth()
            .padding(OrbitSpacing.xl),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = colors.fgMuted,
                modifier = Modifier.size(28.dp),
            )
            Text(text = title, style = typography.bodySemiBold, color = colors.fgPrimary)
            Text(
                text = subtitle,
                style = typography.subhead,
                color = colors.fgSecondary,
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            )
            if (actionLabel != null && onAction != null) {
                Spacer(modifier = Modifier.height(OrbitSpacing.xs))
                Row(
                    modifier = Modifier
                        .clip(CircleShape)
                        .background(colors.primary, CircleShape)
                        .clickable(onClick = onAction)
                        .padding(horizontal = 18.dp, vertical = 10.dp),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Add,
                        contentDescription = null,
                        tint = colors.onPrimary,
                        modifier = Modifier.size(16.dp),
                    )
                    Text(
                        text = actionLabel,
                        style = typography.subheadSemiBold,
                        color = colors.onPrimary,
                    )
                }
            }
        }
    }
}

@Composable
private fun StatusPill(label: String, color: Color) {
    Box(
        modifier = Modifier
            .clip(CircleShape)
            .background(color.copy(alpha = 0.14f), CircleShape)
            .padding(horizontal = 7.dp, vertical = 2.dp),
    ) {
        Text(
            text = label,
            style = OrbitTheme.typography.metaSemiBold,
            color = color,
            maxLines = 1,
        )
    }
}

@Composable
private fun CompactActionPill(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    tint: Color,
    bg: Color,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .height(28.dp)
            .clip(CircleShape)
            .background(bg, CircleShape)
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Icon(
            imageVector = icon,
            contentDescription = null,
            tint = tint,
            modifier = Modifier.size(13.dp),
        )
        Text(
            text = label,
            style = OrbitTheme.typography.metaSemiBold,
            color = tint,
            maxLines = 1,
        )
    }
}

@Composable
private fun HooksRulesAndGuardianTab(
    ui: OrbitUiState,
    activeCwd: String,
    store: OrbitAndroidStore,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val cardShape = RoundedCornerShape(14.dp)
    var showAddRule by remember { mutableStateOf(false) }

    if (showAddRule) {
        var pattern by remember { mutableStateOf("") }
        var allow by remember { mutableStateOf(true) }
        var projectScope by remember { mutableStateOf(true) }
        AlertDialog(
            onDismissRequest = { showAddRule = false },
            containerColor = colors.elevated,
            title = { Text(stringResource(R.string.auto_new_rule), style = typography.title) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(
                        value = pattern,
                        onValueChange = { pattern = it },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                        label = { Text(stringResource(R.string.auto_rule_pattern_label)) },
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TextButton(onClick = { allow = true }) {
                            Text(
                                stringResource(R.string.auto_rule_allow),
                                color = if (allow) colors.directAdded else colors.fgMuted,
                                style = if (allow) typography.subheadSemiBold else typography.subhead,
                            )
                        }
                        TextButton(onClick = { allow = false }) {
                            Text(
                                stringResource(R.string.auto_rule_deny),
                                color = if (!allow) colors.dangerRemoved else colors.fgMuted,
                                style = if (!allow) typography.subheadSemiBold else typography.subhead,
                            )
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TextButton(onClick = { projectScope = true }) {
                            Text(
                                stringResource(R.string.auto_rule_scope_project),
                                color = if (projectScope) colors.fgPrimary else colors.fgMuted,
                                style = if (projectScope) typography.subheadSemiBold else typography.subhead,
                            )
                        }
                        TextButton(onClick = { projectScope = false }) {
                            Text(
                                stringResource(R.string.auto_rule_scope_host),
                                color = if (!projectScope) colors.fgPrimary else colors.fgMuted,
                                style = if (!projectScope) typography.subheadSemiBold else typography.subhead,
                            )
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(
                    enabled = pattern.isNotBlank() && ui.selectedHostId != null,
                    onClick = {
                        store.upsertPermissionRule(
                            UpsertPermissionRuleInput(
                                id = null,
                                scopeKind = if (projectScope) PermissionRuleScopeKind.PROJECT else PermissionRuleScopeKind.HOST,
                                projectCwd = if (projectScope) activeCwd else null,
                                provider = null,
                                toolKind = PermissionRuleToolKind.SHELL,
                                patternKind = PermissionRulePatternKind.COMMAND_PREFIX,
                                pattern = pattern.trim(),
                                decision = if (allow) PermissionRuleDecision.ALLOW else PermissionRuleDecision.DENY,
                                source = PermissionRuleSource.MANUAL,
                                note = null,
                            )
                        )
                        showAddRule = false
                    },
                ) {
                    Text(stringResource(R.string.nav_save))
                }
            },
            dismissButton = {
                TextButton(onClick = { showAddRule = false }) {
                    Text(stringResource(R.string.nav_cancel))
                }
            },
        )
    }

    LazyColumn(
        modifier = modifier.fillMaxWidth(),
        contentPadding = PaddingValues(OrbitSpacing.lg),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
    ) {
        ui.guardianSnapshot?.let { snapshot ->
            item(key = "guardian_policy_card") {
                val cfg = snapshot.config
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(cardShape)
                        .background(colors.elevated, cardShape)
                        .border(OrbitSpacing.hairline, colors.hairline, cardShape)
                        .padding(OrbitSpacing.md),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text(
                            text = stringResource(R.string.auto_guardian_title),
                            style = typography.subheadSemiBold,
                            color = colors.fgPrimary,
                        )
                        StatusPill(
                            label = stringResource(
                                R.string.auto_guardian_max_risk,
                                cfg.autoApproveMaxRisk.name.lowercase().replaceFirstChar { it.uppercase() },
                            ),
                            color = colors.relay,
                        )
                    }
                    ToggleRow(R.string.auto_guardian_enabled, cfg.enabled) {
                        store.updateGuardianConfig(cfg.copy(enabled = it))
                    }
                    ToggleRow(R.string.auto_guardian_read_only, cfg.allowReadOnlyTools) {
                        store.updateGuardianConfig(cfg.copy(allowReadOnlyTools = it))
                    }
                    ToggleRow(R.string.auto_guardian_tests, cfg.allowWorkspaceTests) {
                        store.updateGuardianConfig(cfg.copy(allowWorkspaceTests = it))
                    }
                }
            }
        }

        item(key = "permission_rules_header") {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    text = stringResource(R.string.auto_tab_hooks),
                    style = typography.subheadSemiBold,
                    color = colors.fgPrimary,
                )
                TextButton(
                    enabled = ui.selectedHostId != null,
                    onClick = { showAddRule = true },
                ) {
                    Text(stringResource(R.string.auto_new_rule))
                }
            }
        }

        if (ui.permissionRules.isEmpty()) {
            item(key = "permission_rules_empty") {
                Text(
                    text = stringResource(R.string.auto_rules_empty),
                    style = typography.meta,
                    color = colors.fgMuted,
                )
            }
        } else {
            items(ui.permissionRules, key = { it.id }) { rule ->
                val isAllow = rule.decision == PermissionRuleDecision.ALLOW
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(cardShape)
                        .background(colors.elevated, cardShape)
                        .border(OrbitSpacing.hairline, colors.hairline, cardShape)
                        .padding(OrbitSpacing.md),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(
                            text = rule.pattern,
                            style = typography.codeMedium,
                            color = colors.fgPrimary,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Text(
                            text = "${rule.toolKind.name.lowercase()} · ${rule.scopeKind.name.lowercase()} · ${rule.matchCount}×",
                            style = typography.meta,
                            color = colors.fgSecondary,
                        )
                    }
                    StatusPill(
                        label = stringResource(if (isAllow) R.string.auto_rule_allow else R.string.auto_rule_deny),
                        color = if (isAllow) colors.directAdded else colors.dangerRemoved,
                    )
                    Box(
                        modifier = Modifier
                            .size(36.dp)
                            .clip(CircleShape)
                            .clickable { store.deletePermissionRule(rule.id) },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.DeleteOutline,
                            contentDescription = stringResource(R.string.nav_delete),
                            tint = colors.dangerRemoved,
                            modifier = Modifier.size(18.dp),
                        )
                    }
                }
            }
        }

        ui.selectedHostId?.let { host ->
            item(key = "hooks_json_section") {
                Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs)) {
                    HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                    Text(".orbit/hooks.json", style = typography.code, color = colors.fgSecondary)
                    WorkspaceFileContent(host, activeCwd, ".orbit/hooks.json", store)
                }
            }
        }
    }
}

@Composable
private fun NewAutomationAction(tab: AutomationTab, ui: OrbitUiState, store: OrbitAndroidStore) {
    val label = when (tab) {
        AutomationTab.GOALS -> R.string.auto_start_goal
        AutomationTab.MONITORS -> R.string.auto_start_monitor
        AutomationTab.WORKFLOWS -> R.string.start_workflow
        AutomationTab.TRIGGERS -> R.string.new_trigger
        else -> return
    }
    var open by androidx.compose.runtime.remember(tab) { mutableStateOf(false) }
    TextButton(
        onClick = { open = true },
        enabled = ui.selectedHostId != null,
    ) {
        Text(stringResource(label))
    }
    if (!open) return
    var input by remember { mutableStateOf("") }
    var argument by remember { mutableStateOf("") }
    val valid = input.isNotBlank() && when (tab) {
        AutomationTab.TRIGGERS -> (argument.toULongOrNull() ?: 0uL) > 0uL
        AutomationTab.WORKFLOWS, AutomationTab.MONITORS -> argument.isNotBlank()
        else -> true
    }
    AlertDialog(
        onDismissRequest = { open = false },
        title = { Text(stringResource(label)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    input,
                    { input = it },
                    modifier = Modifier.fillMaxWidth(),
                    label = {
                        Text(
                            stringResource(
                                if (tab == AutomationTab.MONITORS || tab == AutomationTab.WORKFLOWS) {
                                    R.string.automation_name
                                } else {
                                    R.string.automation_prompt
                                }
                            )
                        )
                    },
                )
                if (tab != AutomationTab.GOALS) {
                    OutlinedTextField(
                        argument,
                        { argument = it },
                        modifier = Modifier.fillMaxWidth(),
                        label = {
                            Text(
                                stringResource(
                                    when (tab) {
                                        AutomationTab.TRIGGERS -> R.string.duration_seconds
                                        AutomationTab.MONITORS -> R.string.log_path
                                        else -> R.string.automation_prompt
                                    }
                                )
                            )
                        },
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = valid,
                onClick = {
                    when (tab) {
                        AutomationTab.GOALS -> store.startGoalRun(input, emptyList(), null, null)
                        AutomationTab.TRIGGERS -> store.createConditionalTimer(argument.toULong(), input)
                        AutomationTab.WORKFLOWS -> store.startSimpleDagWorkflow(input, argument)
                        AutomationTab.MONITORS -> store.createMonitorWatch(
                            input,
                            uniffi.orbit_ffi_uniffi.MonitorKind.LogFileTail(argument, ".*"),
                        )
                        else -> Unit
                    }
                    open = false
                },
            ) {
                Text(stringResource(label))
            }
        },
        dismissButton = {
            TextButton(onClick = { open = false }) {
                Text(stringResource(R.string.nav_cancel))
            }
        },
    )
}
