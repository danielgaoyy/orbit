package dev.orbit.android.ui.stage

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.orbit.android.R
import dev.orbit.android.store.*
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.*

@Composable
internal fun StageLineage(ui: OrbitUiState, store: OrbitAndroidStore) {
    val agent = ui.activeAgent ?: return
    val host = ui.selectedHostId ?: return
    val members = ui.hostDetail?.agents.orEmpty().filter { it.agentId != agent.agentId }
    val children =
        members.filter { it.parentAgentId == agent.agentId && (it.parentHostId ?: host) == host }
    val peers =
        members.filter {
            it.workspaceId == agent.workspaceId &&
                it.parentAgentId == agent.parentAgentId &&
                it.parentHostId == agent.parentHostId &&
                agent.workspaceId.isNotBlank()
        }
    val providerChildren =
        ui.agentStage?.insights?.subagents.orEmpty().filter {
            it.childAgentId == null && it.providerSubagentId != null
        }
    var selectedProviderChild by remember { mutableStateOf<String?>(null) }
    selectedProviderChild?.let { id ->
        var events by remember(id) { mutableStateOf<List<FfiProviderSubagentEvent>>(emptyList()) }
        var error by remember(id) { mutableStateOf<String?>(null) }
        LaunchedEffect(id, ui.hostDetail?.revision, ui.agentStage?.revision) {
            try {
                events = store.providerTimeline(host, agent.agentId, id, events.isEmpty())
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                error = e.localizedMessage
            }
        }
        dev.orbit.android.ui.sheets.ToolSheet(
            R.string.stage_lineage_subagents,
            { selectedProviderChild = null },
        ) {
            error?.let { Text(it) }
            androidx.compose.foundation.lazy.LazyColumn {
                items(events.size) { i ->
                    Text(events[i].summaryText, style = OrbitTheme.typography.body)
                }
            }
        }
    }
    if (
        children.isEmpty() &&
            peers.isEmpty() &&
            providerChildren.isEmpty() &&
            agent.parentAgentId == null
    )
        return
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
        providerChildren.forEach { child ->
            TextButton(onClick = { selectedProviderChild = child.providerSubagentId }) {
                Text(
                    stringResource(R.string.stage_lineage_subagents) +
                        " · " +
                        child.role.ifBlank { child.id }
                )
            }
        }
        agent.parentAgentId?.let { parent ->
            TextButton(onClick = { store.selectHostAndAgent(agent.parentHostId ?: host, parent) }) {
                Text(
                    stringResource(R.string.stage_lineage_parent) +
                        " · " +
                        (members.find { it.agentId == parent }?.title ?: parent)
                )
            }
        }
        listOf(R.string.stage_lineage_subagents to children, R.string.stage_lineage_peers to peers)
            .forEach { (label, rows) ->
                rows.forEach { row ->
                    TextButton(onClick = { store.selectHostAndAgent(host, row.agentId) }) {
                        Text(
                            stringResource(label) + " · " + row.title.ifBlank { row.providerLabel }
                        )
                    }
                }
            }
    }
}

@Composable
internal fun AgentMessageBody(message: FfiTimelineItem.AgentMessage) {
    MarkdownBlocks(blocks = message.blocks, fallbackText = message.text)
    message.deliveryStatus?.let { status ->
        val label =
            when (status) {
                FfiAgentMessageDeliveryStatus.QUEUED -> R.string.stage_delivery_queued
                FfiAgentMessageDeliveryStatus.DISPATCHED -> R.string.stage_delivery_dispatched
                FfiAgentMessageDeliveryStatus.SETTLED -> R.string.stage_delivery_settled
                FfiAgentMessageDeliveryStatus.DROPPED -> R.string.stage_delivery_dropped
            }
        Text(
            stringResource(label),
            Modifier.border(0.5.dp, OrbitTheme.colors.hairline, RoundedCornerShape(6.dp))
                .padding(horizontal = 8.dp, vertical = 2.dp),
            style = OrbitTheme.typography.meta,
            color =
                if (status == FfiAgentMessageDeliveryStatus.DROPPED) OrbitTheme.colors.dangerRemoved
                else OrbitTheme.colors.fgSecondary,
        )
    }
}

@Composable
internal fun PlanReview(plan: PlanApprovalSpec, store: OrbitAndroidStore) =
    key(plan.requestId) {
        var expanded by remember { mutableStateOf(false) }
        var edits by remember { mutableStateOf(plan.planMarkdown) }
        var feedback by remember { mutableStateOf("") }
        var mode by remember { mutableStateOf<SessionMode>(SessionMode.Default) }
        var editing by remember { mutableStateOf(false) }
        Surface(
            onClick = { expanded = true },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
            color = OrbitTheme.colors.elevated,
            shape = RoundedCornerShape(12.dp),
            border = BorderStroke(0.5.dp, OrbitTheme.colors.hairline),
        ) {
            Column(Modifier.heightIn(min = 48.dp).padding(12.dp)) {
                Text(
                    stringResource(R.string.plan_review_title),
                    style = OrbitTheme.typography.subheadMedium,
                )
                Text(
                    edits
                        .lineSequence()
                        .firstOrNull { it.isNotBlank() }
                        .orEmpty()
                        .trimStart('#', ' '),
                    maxLines = 1,
                    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                    style = OrbitTheme.typography.meta,
                    color = OrbitTheme.colors.fgSecondary,
                )
            }
        }
        if (expanded)
            AlertDialog(
                onDismissRequest = { expanded = false },
                title = { Text(stringResource(R.string.plan_review_title)) },
                text = {
                    Column(Modifier.verticalScroll(rememberScrollState())) {
                        TextButton(onClick = { editing = !editing }) {
                            Text(
                                stringResource(
                                    if (editing) R.string.plan_tab_preview
                                    else R.string.plan_tab_edit
                                )
                            )
                        }
                        if (!editing) MarkdownBlocks(blocks = emptyList(), fallbackText = edits)
                        else
                            OutlinedTextField(
                                edits,
                                { edits = it },
                                label = { Text(stringResource(R.string.plan_tab_edit)) },
                            )
                        OutlinedTextField(
                            feedback,
                            { feedback = it },
                            label = { Text(stringResource(R.string.plan_feedback_placeholder)) },
                        )
                        TextButton(
                            onClick = {
                                mode =
                                    if (mode == SessionMode.Default) SessionMode.AcceptEdits
                                    else SessionMode.Default
                            }
                        ) {
                            Text(
                                stringResource(R.string.plan_switch_mode) +
                                    " · " +
                                    stringResource(
                                        if (mode == SessionMode.Default) R.string.mode_default
                                        else R.string.mode_accept_edits
                                    )
                            )
                        }
                    }
                },
                confirmButton = {
                    TextButton(
                        onClick = {
                            store.resolvePlanApproval(
                                plan.requestId,
                                PlanApprovalAction.APPROVE_AND_BUILD,
                                plan.planMarkdown,
                                edits,
                                feedback,
                                mode,
                            )
                            expanded = false
                        }
                    ) {
                        Text(stringResource(R.string.plan_approve_build))
                    }
                },
                dismissButton = {
                    TextButton(
                        onClick = {
                            store.resolvePlanApproval(
                                plan.requestId,
                                PlanApprovalAction.REQUEST_CHANGES,
                                plan.planMarkdown,
                                edits,
                                feedback,
                            )
                            expanded = false
                        }
                    ) {
                        Text(stringResource(R.string.plan_request_changes))
                    }
                },
            )
    }
