package dev.orbit.android.ui.sheets

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DeleteOutline
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitAppearanceMode
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import java.time.Duration
import java.time.Instant
import java.time.OffsetDateTime
import uniffi.orbit_ffi_uniffi.FfiConnectionState
import uniffi.orbit_ffi_uniffi.FfiHostCard
import uniffi.orbit_ffi_uniffi.FfiHostRouteMode
import uniffi.orbit_ffi_uniffi.FfiProviderQuota
import kotlin.math.roundToInt

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun SettingsSheet(
    appearanceMode: OrbitAppearanceMode,
    selectedHost: FfiHostCard?,
    quotas: List<FfiProviderQuota>,
    onSelectAppearance: (OrbitAppearanceMode) -> Unit,
    onReconnectHost: () -> Unit,
    onSaveHostConfig: (
        hostId: String,
        customName: String?,
        routeMode: FfiHostRouteMode,
        customDirectEndpoint: String?,
        relayUrl: String?,
    ) -> Unit,
    onRemoveHost: (String) -> Unit,
    onRefreshQuotas: () -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    LaunchedEffect(selectedHost?.hostId) {
        if (selectedHost != null) {
            onRefreshQuotas()
        }
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = colors.elevated,
        contentColor = colors.fgPrimary,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.86f)
                .imePadding()
                .navigationBarsPadding()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.sm),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.lg),
        ) {
            Text(
                text = "Settings & Quotas",
                style = typography.title,
                color = colors.fgPrimary,
            )

            // 1. Quiet Precision 4-Mode Appearance Selector (2x2 Grid, Zero Truncation)
            Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm)) {
                Text(
                    text = "Appearance",
                    style = typography.subheadSemiBold,
                    color = colors.fgSecondary,
                )
                val rows = remember { OrbitAppearanceMode.entries.chunked(2) }
                rows.forEach { rowModes ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                    ) {
                        rowModes.forEach { mode ->
                            val active = appearanceMode == mode
                            val cardShape = RoundedCornerShape(OrbitSpacing.md)
                            Column(
                                modifier = Modifier
                                    .weight(1f)
                                    .heightIn(min = OrbitSpacing.touchTarget)
                                    .clip(cardShape)
                                    .background(
                                        if (active) colors.subtleFill else colors.code,
                                        cardShape
                                    )
                                    .border(
                                        if (active) 1.dp else OrbitSpacing.hairline,
                                        if (active) colors.primary else colors.hairline,
                                        cardShape
                                    )
                                    .clickable {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        onSelectAppearance(mode)
                                    }
                                    .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs / 2),
                            ) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Text(
                                        text = mode.label,
                                        style = typography.subheadSemiBold,
                                        color = if (active) colors.primary else colors.fgPrimary,
                                        maxLines = 1,
                                        softWrap = false,
                                    )
                                    if (active) {
                                        Box(
                                            modifier = Modifier
                                                .size(6.dp)
                                                .background(colors.primary, CircleShape)
                                        )
                                    }
                                }
                                Text(
                                    text = mode.subtitle,
                                    style = typography.meta,
                                    color = colors.fgSecondary,
                                    maxLines = 1,
                                    softWrap = false,
                                )
                            }
                        }
                    }
                }
            }

            // 2. Active Host Diagnostics & Provider Quotas Section
            if (selectedHost != null) {
                HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

                ActiveHostDiagnosticsCard(
                    host = selectedHost,
                    onReconnect = onReconnectHost,
                )

                HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

                Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm)) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = "Provider Quotas (${selectedHost.displayName})",
                            style = typography.subheadSemiBold,
                            color = colors.fgSecondary,
                        )
                        Box(
                            modifier = Modifier
                                .size(44.dp)
                                .clip(RoundedCornerShape(OrbitSpacing.sm))
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onRefreshQuotas()
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Refresh,
                                contentDescription = "Refresh quotas",
                                tint = colors.fgSecondary,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                        }
                    }

                    if (quotas.isEmpty()) {
                        Text(
                            text = "No active rate-limit windows reported by installed providers.",
                            style = typography.meta,
                            color = colors.fgMuted,
                        )
                    } else {
                        quotas.forEach { q ->
                            ProviderQuotaCard(quota = q)
                        }
                    }
                }

                HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

                // 3. Host Route Mode & E2EE Relay Config
                HostRouteEditorSection(
                    host = selectedHost,
                    onSave = { customName, routeMode, endpoint, relay ->
                        onSaveHostConfig(selectedHost.hostId, customName, routeMode, endpoint, relay)
                    },
                    onRemove = {
                        onRemoveHost(selectedHost.hostId)
                        onDismiss()
                    },
                )
            }

            Spacer(modifier = Modifier.height(OrbitSpacing.md))
        }
    }
}

@Composable
private fun ActiveHostDiagnosticsCard(
    host: FfiHostCard,
    onReconnect: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val cardShape = RoundedCornerShape(OrbitSpacing.md)

    val statusColor = when (host.connectionState) {
        FfiConnectionState.ONLINE_DIRECT -> colors.directAdded
        FfiConnectionState.ONLINE_RELAY -> colors.relay
        FfiConnectionState.CONNECTING_DIRECT, FfiConnectionState.CONNECTING_RELAY -> colors.warning
        FfiConnectionState.DISCONNECTED -> colors.dangerRemoved
    }
    val statusText = OrbitFormatters.connectionLabel(host.connectionState, host.viaSshTunnel)
    val pingText = host.lastPingMs?.let { "${it} ms" } ?: "—"
    val versionText = host.daemonVersion?.takeIf { it.isNotBlank() }?.let { raw ->
        if (raw.startsWith("v")) raw else "v$raw"
    } ?: "—"
    val archText = host.osArch?.takeIf { it.isNotBlank() } ?: "remote"

    Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm)) {
        Text(
            text = "Host Diagnostics",
            style = typography.subheadSemiBold,
            color = colors.fgSecondary,
        )

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .background(colors.code, cardShape)
                .border(OrbitSpacing.hairline, colors.hairline, cardShape)
                .padding(OrbitSpacing.md),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(
                    modifier = Modifier.weight(1f),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    Box(
                        modifier = Modifier
                            .size(6.dp)
                            .background(statusColor, CircleShape)
                    )
                    Text(
                        text = host.displayName,
                        style = typography.bodySemiBold,
                        color = colors.fgPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }

                val btnShape = RoundedCornerShape(OrbitSpacing.sm)
                Row(
                    modifier = Modifier
                        .heightIn(min = 36.dp)
                        .clip(btnShape)
                        .background(colors.subtleFill, btnShape)
                        .border(OrbitSpacing.hairline, colors.primary.copy(alpha = 0.6f), btnShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onReconnect()
                        }
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Refresh,
                        contentDescription = "Reconnect host",
                        tint = colors.primary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                    Text(
                        text = "Reconnect",
                        style = typography.subheadSemiBold,
                        color = colors.primary,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
            ) {
                DiagnosticMetricColumn(
                    label = "State",
                    value = statusText,
                    valueColor = statusColor,
                    modifier = Modifier.weight(1f),
                )
                DiagnosticMetricColumn(
                    label = "Latency",
                    value = pingText,
                    valueColor = colors.fgPrimary,
                    modifier = Modifier.weight(1f),
                )
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
            ) {
                DiagnosticMetricColumn(
                    label = "Daemon",
                    value = versionText,
                    valueColor = colors.fgPrimary,
                    modifier = Modifier.weight(1f),
                )
                DiagnosticMetricColumn(
                    label = "Platform",
                    value = archText,
                    valueColor = colors.fgPrimary,
                    modifier = Modifier.weight(1f),
                )
            }

            host.lastError?.takeIf { it.isNotBlank() }?.let { err ->
                val errShape = RoundedCornerShape(OrbitSpacing.sm)
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(colors.diffRemovedBg, errShape)
                        .border(OrbitSpacing.hairline, colors.dangerRemoved.copy(alpha = 0.45f), errShape)
                        .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                ) {
                    Text(
                        text = err,
                        style = typography.code,
                        color = colors.dangerRemoved,
                    )
                }
            }
        }
    }
}

@Composable
private fun DiagnosticMetricColumn(
    label: String,
    value: String,
    valueColor: Color,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography

    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs / 2),
    ) {
        Text(
            text = label,
            style = typography.meta,
            color = colors.fgMuted,
            maxLines = 1,
        )
        Text(
            text = value,
            style = typography.codeMedium,
            color = valueColor,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

private fun formatQuotaResetCountdown(rawRfc3339: String): String {
    val trimmed = rawRfc3339.trim()
    val targetInstant = runCatching { Instant.parse(trimmed) }
        .recoverCatching { OffsetDateTime.parse(trimmed).toInstant() }
        .getOrNull() ?: return "Resets $trimmed"

    val remainingMinutes = Duration.between(Instant.now(), targetInstant).toMinutes()
    if (remainingMinutes <= 0L) return "Resets momentarily"

    val days = remainingMinutes / (24L * 60L)
    val hours = (remainingMinutes % (24L * 60L)) / 60L
    val mins = remainingMinutes % 60L
    return when {
        days > 0L && hours > 0L -> "Resets in ${days}d ${hours}h"
        days > 0L -> "Resets in ${days}d"
        hours > 0L && mins > 0L -> "Resets in ${hours}h ${mins}m"
        hours > 0L -> "Resets in ${hours}h"
        else -> "Resets in ${mins}m"
    }
}

@Composable
private fun ProviderQuotaCard(quota: FfiProviderQuota) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val cardShape = RoundedCornerShape(OrbitSpacing.md)

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(colors.code, cardShape)
            .border(OrbitSpacing.hairline, colors.hairline, cardShape)
            .padding(OrbitSpacing.md),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = quota.providerLabel,
                style = typography.bodySemiBold,
                color = colors.fgPrimary,
            )
            quota.accountLabel?.takeIf { it.isNotBlank() }?.let { acct ->
                Text(
                    text = acct,
                    style = typography.meta,
                    color = colors.fgSecondary,
                )
            }
        }

        quota.creditsRemaining?.let { credits ->
            Text(
                text = "Credits remaining: ${"%.2f".format(credits)}",
                style = typography.metaMedium,
                color = colors.directAdded,
            )
        }

        quota.windows.forEach { win ->
            val pct = win.usedPercent.coerceIn(0f, 100f)
            val barColor = when {
                pct > 90f -> colors.dangerRemoved
                pct >= 70f -> colors.warning
                else -> colors.directAdded
            }
            val barShape = RoundedCornerShape(OrbitSpacing.xs / 2)

            Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = win.label,
                        style = typography.metaMedium,
                        color = colors.fgSecondary,
                    )
                    Text(
                        text = "${pct.roundToInt()}% used",
                        style = typography.metaMedium,
                        color = barColor,
                    )
                }
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(OrbitSpacing.xs)
                        .clip(barShape)
                        .background(colors.subtleFill, barShape),
                ) {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth(fraction = (pct / 100f).coerceIn(0.02f, 1f))
                            .height(OrbitSpacing.xs)
                            .background(barColor, barShape)
                    )
                }
                win.resetsAtRfc3339?.takeIf { it.isNotBlank() }?.let { reset ->
                    Text(
                        text = formatQuotaResetCountdown(reset),
                        style = typography.meta,
                        color = colors.fgMuted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }

        quota.error?.takeIf { it.isNotBlank() }?.let { err ->
            Text(
                text = err,
                style = typography.meta,
                color = colors.warning,
            )
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun HostRouteEditorSection(
    host: FfiHostCard,
    onSave: (
        customName: String?,
        routeMode: FfiHostRouteMode,
        customDirectEndpoint: String?,
        relayUrl: String?,
    ) -> Unit,
    onRemove: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current

    var customName by remember(host.hostId) { mutableStateOf(host.customName.orEmpty()) }
    var routeMode by remember(host.hostId) { mutableStateOf(host.routeMode) }
    var directEndpoint by remember(host.hostId) { mutableStateOf(host.customDirectEndpoint.orEmpty()) }
    var relayUrl by remember(host.hostId) { mutableStateOf(host.relayUrl.orEmpty()) }

    Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md)) {
        Text(
            text = "Host Connection & Routing",
            style = typography.subheadSemiBold,
            color = colors.fgSecondary,
        )

        Text(
            text = "${host.hostname} · ${host.osArch ?: "remote"} · Pubkey ${host.daemonPubkeyB64.take(12)}…",
            style = typography.code,
            color = colors.fgMuted,
        )

        // Route Mode Chips (quiet selection border; Save & Reconnect is sole primary filled CTA)
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
        ) {
            FfiHostRouteMode.entries.forEach { mode ->
                val selected = routeMode == mode
                val chipShape = RoundedCornerShape(OrbitSpacing.md)
                Box(
                    modifier = Modifier
                        .heightIn(min = 40.dp)
                        .clip(chipShape)
                        .background(if (selected) colors.subtleFill else colors.code, chipShape)
                        .border(
                            if (selected) 1.dp else OrbitSpacing.hairline,
                            if (selected) colors.primary else colors.hairline,
                            chipShape
                        )
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            routeMode = mode
                        }
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = OrbitFormatters.routeModeLabel(mode),
                        style = typography.subheadMedium,
                        color = if (selected) colors.primary else colors.fgPrimary,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
            }
        }

        LabeledConfigInput(
            label = "Display Alias",
            placeholder = host.hostname,
            value = customName,
            onValueChange = { customName = it },
        )

        LabeledConfigInput(
            label = "Direct Endpoint Override",
            placeholder = host.directEndpoints.firstOrNull() ?: "ws://192.168.1.10:9741",
            value = directEndpoint,
            onValueChange = { directEndpoint = it },
        )

        LabeledConfigInput(
            label = "Zero-Knowledge Relay URL",
            placeholder = "wss://relay.example.com",
            value = relayUrl,
            onValueChange = { relayUrl = it },
        )

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            val dangerShape = RoundedCornerShape(OrbitSpacing.md)
            Row(
                modifier = Modifier
                    .heightIn(min = OrbitSpacing.touchTarget)
                    .clip(dangerShape)
                    .background(colors.subtleFill, dangerShape)
                    .border(OrbitSpacing.hairline, colors.hairline, dangerShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        onRemove()
                    }
                    .padding(horizontal = OrbitSpacing.md),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                Icon(
                    imageVector = Icons.Outlined.DeleteOutline,
                    contentDescription = "Remove host",
                    tint = colors.dangerRemoved,
                    modifier = Modifier.size(OrbitSpacing.iconSm),
                )
                Text(
                    text = "Unpair",
                    style = typography.subheadSemiBold,
                    color = colors.dangerRemoved,
                )
            }

            val saveShape = RoundedCornerShape(OrbitSpacing.md)
            Box(
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = OrbitSpacing.touchTarget)
                    .clip(saveShape)
                    .background(colors.primary, saveShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onSave(customName, routeMode, directEndpoint, relayUrl)
                    },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = "Save & Reconnect",
                    style = typography.bodySemiBold,
                    color = Color.White,
                )
            }
        }
    }
}

@Composable
private fun LabeledConfigInput(
    label: String,
    placeholder: String,
    value: String,
    onValueChange: (String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val shape = RoundedCornerShape(OrbitSpacing.md)

    Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs)) {
        Text(
            text = label,
            style = typography.metaMedium,
            color = colors.fgSecondary,
        )
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 44.dp)
                .background(colors.code, shape)
                .border(OrbitSpacing.hairline, colors.hairline, shape)
                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
            contentAlignment = Alignment.CenterStart,
        ) {
            if (value.isEmpty()) {
                Text(
                    text = placeholder,
                    style = typography.code,
                    color = colors.fgMuted,
                )
            }
            BasicTextField(
                value = value,
                onValueChange = onValueChange,
                textStyle = typography.code.copy(color = colors.fgPrimary),
                cursorBrush = SolidColor(colors.primary),
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
