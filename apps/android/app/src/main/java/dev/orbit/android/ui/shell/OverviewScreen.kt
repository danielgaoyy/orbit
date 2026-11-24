package dev.orbit.android.ui.shell

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Menu
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.R
import dev.orbit.android.store.*
import dev.orbit.android.ui.composer.ComposerAction
import dev.orbit.android.ui.theme.OrbitTheme
import kotlinx.coroutines.delay

@Composable
internal fun OverviewScreen(
    ui: OrbitUiState,
    store: OrbitAndroidStore,
    onSidebar: () -> Unit,
    onSearch: () -> Unit,
    onSettings: () -> Unit,
    onPair: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val type = OrbitTheme.typography
    var draft by rememberSaveable { mutableStateOf("") }
    var listening by remember { mutableStateOf(false) }
    val sessions = ui.fleetSessions
    val pending = ui.overviewPendingDecisions
    val summary =
        OverviewSafetyReducer.summarizeFleet(
            ui.hosts,
            sessions.count { it.second.isForegroundRunning() },
            pending.size,
        )
    LaunchedEffect(ui.hosts, ui.sidebarView, ui.totalPendingApprovals) {
        delay(150)
        store.refreshOverviewPendingDecisions()
    }
    LaunchedEffect(ui.hosts.map { it.hostId }) { store.refreshRouterAndMesh() }
    Column(
        Modifier.fillMaxSize()
            .statusBarsPadding()
            .navigationBarsPadding()
            .imePadding()
            .padding(horizontal = 16.dp)
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onSidebar) {
                Icon(Icons.Outlined.Menu, stringResource(R.string.nav_open_drawer))
            }
            Text(stringResource(R.string.overview_title), Modifier.weight(1f), style = type.title)
            IconButton(onClick = onSearch) {
                Icon(Icons.Outlined.Search, stringResource(R.string.nav_global_search))
            }
            IconButton(onClick = onSettings) {
                Icon(Icons.Outlined.Settings, stringResource(R.string.nav_settings))
            }
        }
        ui.lastError?.let { Text(it, color = colors.dangerRemoved, style = type.meta) }
        LazyColumn(
            Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            contentPadding = PaddingValues(vertical = 12.dp),
        ) {
            item {
                Text(
                    when {
                        ui.hosts.isEmpty() -> stringResource(R.string.overview_status_empty)
                        summary.tone == OverviewHealthTone.ATTENTION ->
                            stringResource(
                                R.string.overview_status_attention,
                                pending.size,
                                stringResource(
                                    if (pending.size == 1) R.string.overview_decision_singular
                                    else R.string.overview_decision_plural
                                ),
                            )
                        summary.tone == OverviewHealthTone.OFFLINE ->
                            stringResource(R.string.overview_status_offline)
                        summary.tone == OverviewHealthTone.ACTIVE ->
                            stringResource(
                                R.string.overview_status_working,
                                summary.runningAgentCount,
                                stringResource(
                                    if (summary.runningAgentCount == 1)
                                        R.string.overview_agent_singular
                                    else R.string.overview_agent_plural
                                ),
                                summary.onlineHostCount,
                            )
                        else ->
                            stringResource(
                                R.string.overview_status_calm,
                                summary.onlineHostCount,
                                stringResource(
                                    if (summary.onlineHostCount == 1)
                                        R.string.overview_host_singular
                                    else R.string.overview_host_plural
                                ),
                            )
                    },
                    style = type.body,
                    color = colors.fgSecondary,
                )
                if (ui.hosts.isEmpty())
                    TextButton(onClick = onPair) { Text(stringResource(R.string.nav_pair_host)) }
                if (summary.tone == OverviewHealthTone.OFFLINE)
                    TextButton(onClick = { ui.hosts.forEach { store.reconnectHost(it.hostId) } }) {
                        Text(stringResource(R.string.overview_reconnect))
                    }
            }
            if (pending.isNotEmpty()) {
                item { Text(stringResource(R.string.overview_needs_decision), style = type.title) }
                items(
                    items = pending,
                    key = { "${it.hostId}::${it.agentId}::${it.permission.requestId}" },
                ) { row ->
                    val badgeText = when {
                        row.isPlanApproval -> stringResource(R.string.plan_review_title)
                        row.isSafeLowRisk -> stringResource(R.string.overview_risk_safe)
                        row.riskLevel != null -> stringResource(
                            R.string.overview_risk_level,
                            row.riskLevel.name.lowercase().replaceFirstChar { it.uppercase() },
                        )
                        else -> null
                    }
                    val badgeColor = when {
                        row.isPlanApproval -> colors.primary
                        row.isSafeLowRisk -> colors.directAdded
                        row.riskLevel == uniffi.orbit_ffi_uniffi.GuardianRiskLevel.HIGH ||
                            row.riskLevel == uniffi.orbit_ffi_uniffi.GuardianRiskLevel.CRITICAL -> colors.dangerRemoved
                        row.riskLevel == uniffi.orbit_ffi_uniffi.GuardianRiskLevel.MEDIUM -> colors.warning
                        else -> colors.fgSecondary
                    }
                    val commandPreview = row.permission.commandOrPath
                        .trim()
                        .takeIf { it.isNotEmpty() && it != row.permission.title.trim() }
                    OverviewRow(
                        title = row.permission.title,
                        detail = row.hostName + " · " + row.agentTitle,
                        badge = badgeText,
                        badgeColor = badgeColor,
                        codePreview = commandPreview,
                    ) {
                        store.selectHostAndAgent(row.hostId, row.agentId)
                    }
                }
                val safe = pending.filter { it.isSafeLowRisk }
                if (safe.isNotEmpty())
                    item {
                        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                            TextButton(
                                onClick = {
                                    safe
                                        .groupBy { it.hostId }
                                        .forEach { (host, rows) ->
                                            store.resolvePermissionsBatchSafe(
                                                host,
                                                rows.map { it.agentId to it.permission },
                                            )
                                        }
                                }
                            ) {
                                Text(stringResource(R.string.overview_batch_approve_safe, safe.size))
                            }
                            Text(
                                text = stringResource(R.string.overview_batch_approve_hint),
                                style = type.meta,
                                color = colors.fgMuted,
                                modifier = Modifier.padding(horizontal = 4.dp),
                            )
                        }
                    }
            }
            val (active, recent) = sessions.partition { it.second.isForegroundRunning() }
            listOf(R.string.overview_active_sessions to active, R.string.recent_sessions to recent)
                .forEach { (label, rows) ->
                    if (rows.isNotEmpty())
                        item { Text(stringResource(label), style = type.subheadMedium) }
                    items(rows) { (host, agent) ->
                        OverviewRow(
                            agent.title.ifBlank { agent.providerLabel },
                            listOfNotNull(
                                    ui.hosts.find { it.hostId == host }?.displayName,
                                    agent.statusLabel,
                                )
                                .joinToString(" · "),
                        ) {
                            store.selectHostAndAgent(host, agent.agentId)
                        }
                    }
                }
        }
        if (ui.hosts.isNotEmpty()) {
            if (listening)
                Text(stringResource(R.string.composer_voice_listening), style = type.meta)
            Row(
                Modifier.fillMaxWidth().padding(vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedTextField(
                    value = draft,
                    onValueChange = { draft = it },
                    modifier = Modifier.weight(1f),
                    maxLines = 4,
                    placeholder = {
                        Text(
                            stringResource(R.string.overview_composer_placeholder),
                            style = type.body,
                        )
                    },
                )
                ComposerAction(
                    draft.isNotBlank(),
                    false,
                    true,
                    { draft = listOf(draft, it).filter(String::isNotBlank).joinToString(" ") },
                    { listening = it },
                    {
                        val sent = draft
                        store.sendPersonalAgentPrompt(sent) { if (draft == sent) draft = "" }
                    },
                    {},
                    { if (it == dev.orbit.android.ui.composer.VoiceNavigation.SEARCH) onSearch() },
                )
            }
        }
    }
}

@Composable
internal fun OverviewRow(
    title: String,
    detail: String,
    badge: String? = null,
    badgeColor: androidx.compose.ui.graphics.Color = OrbitTheme.colors.fgSecondary,
    codePreview: String? = null,
    onClick: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val shape = RoundedCornerShape(12.dp)
    Column(
        Modifier.fillMaxWidth()
            .background(colors.elevated, shape)
            .border(0.5.dp, colors.hairline, shape)
            .clickable(onClick = onClick)
            .heightIn(min = 48.dp)
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                title,
                style = OrbitTheme.typography.subheadMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (!badge.isNullOrBlank()) {
                Text(
                    text = badge,
                    style = OrbitTheme.typography.metaSemiBold,
                    color = badgeColor,
                    maxLines = 1,
                    modifier = Modifier
                        .background(badgeColor.copy(alpha = 0.14f), RoundedCornerShape(6.dp))
                        .padding(horizontal = 6.dp, vertical = 2.dp),
                )
            }
        }
        Text(
            detail,
            style = OrbitTheme.typography.meta,
            color = colors.fgSecondary,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        if (!codePreview.isNullOrBlank()) {
            Text(
                text = codePreview,
                style = OrbitTheme.typography.code,
                color = colors.fgMuted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
internal fun HostConnectionBanner(ui: OrbitUiState, store: OrbitAndroidStore) {
    val host = ui.selectedHost ?: return
    val state = host.connectionState
    if (
        state == uniffi.orbit_ffi_uniffi.FfiConnectionState.ONLINE_DIRECT ||
            state == uniffi.orbit_ffi_uniffi.FfiConnectionState.ONLINE_RELAY
    )
        return
    val offline = state == uniffi.orbit_ffi_uniffi.FfiConnectionState.DISCONNECTED
    Row(
        Modifier.fillMaxWidth()
            .background(OrbitTheme.colors.surfaceRecessed)
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            stringResource(
                if (offline) R.string.stage_offline_banner else R.string.stage_connecting_banner,
                host.displayName,
            ),
            Modifier.weight(1f).padding(vertical = 8.dp),
            style = OrbitTheme.typography.meta,
        )
        if (offline)
            TextButton(onClick = { store.reconnectHost(host.hostId) }) {
                Text(stringResource(R.string.overview_reconnect))
            }
    }
}
