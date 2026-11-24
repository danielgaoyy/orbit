package dev.orbit.android.ui.sheets

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.orbit.android.R
import dev.orbit.android.store.*
import dev.orbit.android.ui.shell.OverviewRow
import dev.orbit.android.ui.theme.OrbitTheme
import kotlinx.coroutines.delay
import uniffi.orbit_ffi_uniffi.*

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ToolSheet(
    title: Int,
    onClose: () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = OrbitTheme.colors.elevated,
    ) {
        Column(
            Modifier.fillMaxWidth()
                .heightIn(max = 760.dp)
                .navigationBarsPadding()
                .imePadding()
                .padding(horizontal = 16.dp)
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    stringResource(title),
                    Modifier.weight(1f),
                    style = OrbitTheme.typography.title,
                )
                TextButton(onClick = onClose) { Text(stringResource(R.string.nav_close)) }
            }
            content()
        }
    }
}

@Composable
internal fun GlobalSearchSheet(ui: OrbitUiState, store: OrbitAndroidStore, onClose: () -> Unit) {
    var query by rememberSaveable { mutableStateOf(ui.globalSearchQuery) }
    var remote by rememberSaveable { mutableStateOf(true) }
    LaunchedEffect(query, remote) {
        delay(220)
        store.runGlobalSearch(query, remote)
    }
    ToolSheet(R.string.search_title, onClose) {
        OutlinedTextField(
            query,
            { query = it },
            Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text(stringResource(R.string.nav_search)) },
        )
        ToggleRow(R.string.search_include_remote, remote) { remote = it }
        if (query.trim().length >= 2 && ui.globalSearchQuery == query) {
            if (ui.isSearchingGlobal) LinearProgressIndicator(Modifier.fillMaxWidth())
            else if (ui.globalSearchHits.isEmpty()) Text(stringResource(R.string.search_no_results))
            if (remote && !ui.isSearchingGlobal) {
                val failedCount = ui.globalSearchRemoteFailed.toInt()
                Text(
                    text = if (failedCount > 0) {
                        stringResource(
                            R.string.search_remote_failed_summary,
                            ui.globalSearchRemoteQueried.toInt(),
                            failedCount,
                        )
                    } else {
                        stringResource(
                            R.string.search_remote_summary,
                            ui.globalSearchRemoteQueried.toInt(),
                        )
                    },
                    style = OrbitTheme.typography.meta,
                    color = if (failedCount > 0) OrbitTheme.colors.warning else OrbitTheme.colors.fgSecondary,
                )
            }
            LazyColumn(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                items(ui.globalSearchHits) { hit ->
                    OverviewRow(hit.hostName + " · " + hit.title, hit.snippet) {
                        store.selectHostAndAgent(hit.hostId, hit.agentId)
                        onClose()
                    }
                }
            }
        }
    }
}

@Composable
internal fun ToggleRow(label: Int, value: Boolean, change: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 48.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(stringResource(label), Modifier.weight(1f), style = OrbitTheme.typography.subhead)
        Switch(checked = value, onCheckedChange = change)
    }
}

@Composable
internal fun RouterSheet(ui: OrbitUiState, store: OrbitAndroidStore, onClose: () -> Unit) {
    LaunchedEffect(ui.selectedHostId) {
        store.refreshRouterAndMesh()
        store.refreshPushDevices()
    }
    ToolSheet(R.string.router_title, onClose) {
        Column(
            Modifier.verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (ui.isLoadingRouterMesh) LinearProgressIndicator(Modifier.fillMaxWidth())
            if (ui.routerHosts.size > 1)
                Row(Modifier.horizontalScroll(rememberScrollState())) {
                    ui.routerHosts.forEach { host ->
                        TextButton(
                            onClick = { store.refreshRouterAndMesh(host.hostId) },
                            enabled = host.online && !ui.isLoadingRouterMesh,
                        ) {
                            Text(host.hostName)
                        }
                    }
                }
            ui.routerStatus?.let { snap ->
                val host = snap.hostCapabilities
                Text(host.hostName, style = OrbitTheme.typography.title)
                ToggleRow(R.string.router_section, snap.config.enabled) {
                    store.updatePersonalRouterConfig(
                        host.hostId,
                        snap.config.copy(enabled = it),
                        it,
                    )
                }
                ToggleRow(
                    R.string.router_scope_global,
                    snap.config.scope == RouterScope.GLOBAL_FLEET,
                ) {
                    store.updatePersonalRouterConfig(
                        host.hostId,
                        snap.config.copy(
                            scope =
                                if (it) RouterScope.GLOBAL_FLEET else RouterScope.LOCAL_HOST_ONLY
                        ),
                        false,
                    )
                }
            }
            Text(
                stringResource(R.string.router_mesh_section),
                style = OrbitTheme.typography.subheadMedium,
            )
            if (ui.fleetMeshSnapshot.values.all { it.isEmpty() })
                Text(stringResource(R.string.router_mesh_empty))
            ui.fleetMeshSnapshot.forEach { (host, peers) ->
                peers.forEach { peer ->
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            (ui.hosts.find { it.hostId == host }?.displayName ?: host) +
                                " · " +
                                peer.label,
                            Modifier.weight(1f),
                        )
                        Switch(
                            peer.enabled,
                            { store.toggleMeshPeer(host, peer.registrationId, peer.label, it) },
                        )
                    }
                }
            }
            ui.pushDevicesSnapshot?.let { snap ->
                Text(stringResource(R.string.push_registered_devices, snap.devices.size))
                ToggleRow(R.string.push_section_title, snap.preferences.enabled) {
                    store.updateHostPushPreferences(
                        OrbitFfiTypeBridge.toPushPreferences(snap.preferences).copy(enabled = it)
                    )
                }
                TextButton(onClick = store::sendTestPushNotification) {
                    Text(stringResource(R.string.push_test_button))
                }
            }
        }
    }
}

@Composable
internal fun ExportSheet(onSelect: (FfiExportFormat?) -> Unit) {
    ToolSheet(R.string.export_format_title, { onSelect(null) }) {
        listOf(
                R.string.export_markdown to FfiExportFormat.MARKDOWN,
                R.string.export_json to FfiExportFormat.JSON,
                R.string.export_html to FfiExportFormat.HTML,
            )
            .forEach { (label, format) ->
                TextButton(onClick = { onSelect(format) }) { Text(stringResource(label)) }
            }
    }
}
