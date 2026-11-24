package dev.orbit.android.ui.sheets

import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.CloudSync
import androidx.compose.material.icons.outlined.Contrast
import androidx.compose.material.icons.outlined.DarkMode
import androidx.compose.material.icons.outlined.LightMode
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material.icons.outlined.QrCodeScanner
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.BuildConfig
import dev.orbit.android.notifications.OrbitNotificationHelper
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitAppearanceMode
import dev.orbit.android.ui.theme.OrbitCopyIcon
import dev.orbit.android.ui.theme.OrbitProviderIcon
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import java.time.Duration
import java.time.Instant
import java.time.OffsetDateTime
import uniffi.orbit_ffi_uniffi.FfiCloudSyncState
import uniffi.orbit_ffi_uniffi.FfiCloudSyncStatus
import uniffi.orbit_ffi_uniffi.FfiConnectionState
import uniffi.orbit_ffi_uniffi.FfiHostCard
import uniffi.orbit_ffi_uniffi.FfiHostRouteMode
import uniffi.orbit_ffi_uniffi.FfiProviderQuota
import uniffi.orbit_ffi_uniffi.FfiPushRegistrationStatus
import kotlin.math.roundToInt

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsSheet(
    onRouter: () -> Unit,
    appearanceMode: OrbitAppearanceMode,
    hosts: List<FfiHostCard>,
    selectedHostId: String?,
    quotas: List<FfiProviderQuota>,
    cloudSyncStatus: FfiCloudSyncStatus? = null,
    pushRegistrationStatus: FfiPushRegistrationStatus? = null,
    onSelectAppearance: (OrbitAppearanceMode) -> Unit,
    onSelectHost: (String) -> Unit,
    onOpenPairHost: () -> Unit,
    onOpenCloudSync: () -> Unit = {},
    onReconnectHost: (String) -> Unit,
    onSaveHostConfig: (
        hostId: String,
        customName: String?,
        routeMode: FfiHostRouteMode,
        customDirectEndpoint: String?,
        relayUrl: String?,
    ) -> Unit,
    onRemoveHost: (String) -> Unit,
    onRefreshQuotas: () -> Unit,
    onShowToast: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    val groupedBg = if (colors.isDark) colors.canvas else colors.drawer
    var inspectedHostId by remember { mutableStateOf<String?>(null) }
    val inspectedHost = remember(hosts, inspectedHostId) {
        inspectedHostId?.let { id -> hosts.firstOrNull { it.hostId == id } }
    }

    LaunchedEffect(selectedHostId) {
        if (selectedHostId != null) {
            onRefreshQuotas()
        }
    }

    BackHandler(enabled = inspectedHost != null) {
        inspectedHostId = null
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
                .fillMaxHeight(0.86f)
                .imePadding()
                .navigationBarsPadding(),
        ) {
            // Sheet Top Navigation Bar
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg)
                    .padding(bottom = OrbitSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                if (inspectedHost != null) {
                    Box(
                        modifier = Modifier
                            .size(36.dp)
                            .clip(CircleShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                inspectedHostId = null
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Outlined.ArrowBack,
                            contentDescription = "Back to Settings",
                            tint = colors.fgPrimary,
                            modifier = Modifier.size(20.dp),
                        )
                    }
                }

                Text(
                    text = inspectedHost?.displayName ?: "Settings",
                    style = typography.title,
                    color = colors.fgPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )

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

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            if (inspectedHost != null) {
                HostDetailSettingsPane(
                    host = inspectedHost,
                    isActiveHost = inspectedHost.hostId == selectedHostId,
                    onReconnect = { onReconnectHost(inspectedHost.hostId) },
                    onSwitchHost = {
                        onSelectHost(inspectedHost.hostId)
                        inspectedHostId = null
                    },
                    onSave = { customName, routeMode, endpoint, relay ->
                        onSaveHostConfig(inspectedHost.hostId, customName, routeMode, endpoint, relay)
                    },
                    onRemove = {
                        val removingLast = hosts.size <= 1
                        onRemoveHost(inspectedHost.hostId)
                        inspectedHostId = null
                        if (removingLast) {
                            onDismiss()
                        }
                    },
                    onShowToast = onShowToast,
                    modifier = Modifier.weight(1f),
                )
            } else {
                SettingsRootPane(
                    appearanceMode = appearanceMode,
                    hosts = hosts,
                    selectedHostId = selectedHostId,
                    quotas = quotas,
                    cloudSyncStatus = cloudSyncStatus,
                    pushRegistrationStatus = pushRegistrationStatus,
                    onRouter = onRouter,
                    onSelectAppearance = onSelectAppearance,
                    onInspectHost = { inspectedHostId = it },
                    onOpenPairHost = {
                        onDismiss()
                        onOpenPairHost()
                    },
                    onOpenCloudSync = {
                        onDismiss()
                        onOpenCloudSync()
                    },
                    onRefreshQuotas = onRefreshQuotas,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SettingsRootPane(
    appearanceMode: OrbitAppearanceMode,
    hosts: List<FfiHostCard>,
    selectedHostId: String?,
    quotas: List<FfiProviderQuota>,
    cloudSyncStatus: FfiCloudSyncStatus?,
    pushRegistrationStatus: FfiPushRegistrationStatus?,
    onRouter: () -> Unit,
    onSelectAppearance: (OrbitAppearanceMode) -> Unit,
    onInspectHost: (String) -> Unit,
    onOpenPairHost: () -> Unit,
    onOpenCloudSync: () -> Unit,
    onRefreshQuotas: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val context = LocalContext.current
    val cardShape = RoundedCornerShape(14.dp)

    Column(
        modifier = modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.lg),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xl),
    ) {
        // 1. Appearance (Strictly System / Light / Dark — M3 SingleChoiceSegmentedButtonRow)
        TextButton(onClick = onRouter) { Text(androidx.compose.ui.res.stringResource(dev.orbit.android.R.string.nav_personal_router)) }
        Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs + 2.dp)) {
            Text(
                text = "Appearance",
                style = typography.metaMedium,
                color = colors.fgMuted,
                modifier = Modifier.padding(horizontal = OrbitSpacing.xs),
            )

            SingleChoiceSegmentedButtonRow(
                modifier = Modifier.fillMaxWidth(),
            ) {
                OrbitAppearanceMode.entries.forEachIndexed { index, mode ->
                    val selected = appearanceMode == mode
                    val modeIcon = when (mode) {
                        OrbitAppearanceMode.SYSTEM -> Icons.Outlined.Contrast
                        OrbitAppearanceMode.LIGHT -> Icons.Outlined.LightMode
                        OrbitAppearanceMode.DARK -> Icons.Outlined.DarkMode
                    }
                    SegmentedButton(
                        selected = selected,
                        onClick = {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onSelectAppearance(mode)
                        },
                        shape = SegmentedButtonDefaults.itemShape(
                            index = index,
                            count = OrbitAppearanceMode.entries.size,
                        ),
                        colors = SegmentedButtonDefaults.colors(
                            activeContainerColor = colors.elevated,
                            activeContentColor = colors.fgPrimary,
                            inactiveContainerColor = if (colors.isDark) colors.drawer else colors.subtleFill,
                            inactiveContentColor = colors.fgSecondary,
                            activeBorderColor = colors.hairline,
                            inactiveBorderColor = colors.hairline,
                        ),
                        icon = {},
                    ) {
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(5.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(
                                imageVector = modeIcon,
                                contentDescription = null,
                                modifier = Modifier.size(15.dp),
                            )
                            Text(
                                text = mode.label,
                                style = if (selected) typography.subheadSemiBold else typography.subheadMedium,
                                maxLines = 1,
                                softWrap = false,
                            )
                        }
                    }
                }
            }
        }

        // 1b. Cloud Sync & Notifications
        Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs + 2.dp)) {
            Text(
                text = "Sync & Notifications",
                style = typography.metaMedium,
                color = colors.fgMuted,
                modifier = Modifier.padding(horizontal = OrbitSpacing.xs),
            )

            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(cardShape)
                    .background(colors.elevated, cardShape)
                    .border(OrbitSpacing.hairline, colors.hairline, cardShape),
            ) {
                val syncSubtitle = when (cloudSyncStatus?.state) {
                    FfiCloudSyncState.READY ->
                        "Synced · @${cloudSyncStatus.accountLogin ?: "github"} · rev #${cloudSyncStatus.lastVaultRevision}"
                    FfiCloudSyncState.LOCKED ->
                        "Locked · @${cloudSyncStatus.accountLogin ?: "github"} — tap to unlock"
                    FfiCloudSyncState.SYNCING -> "Syncing encrypted vault…"
                    FfiCloudSyncState.ERROR -> cloudSyncStatus.lastError ?: "Sync error"
                    else -> "GitHub Secret Gist · E2EE (Signed out)"
                }
                val syncDotColor = when (cloudSyncStatus?.state) {
                    FfiCloudSyncState.READY -> colors.directAdded
                    FfiCloudSyncState.LOCKED, FfiCloudSyncState.SYNCING -> colors.warning
                    FfiCloudSyncState.ERROR -> colors.dangerRemoved
                    else -> colors.fgMuted
                }

                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 54.dp)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onOpenCloudSync()
                        }
                        .padding(horizontal = OrbitSpacing.md, vertical = 9.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Box(modifier = Modifier.size(20.dp), contentAlignment = Alignment.Center) {
                        Icon(
                            imageVector = Icons.Outlined.CloudSync,
                            contentDescription = null,
                            tint = syncDotColor,
                            modifier = Modifier.size(17.dp),
                        )
                    }
                    Column(
                        modifier = Modifier.weight(1f),
                        verticalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        Text(
                            text = "Cloud Sync (GitHub E2EE)",
                            style = typography.subheadMedium,
                            color = colors.fgPrimary,
                        )
                        Text(
                            text = syncSubtitle,
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

                HorizontalDivider(
                    thickness = OrbitSpacing.hairline,
                    color = colors.hairline,
                    modifier = Modifier.padding(start = 42.dp),
                )

                val lifecycleOwner = LocalLifecycleOwner.current
                var notifEnabled by remember(context) {
                    mutableStateOf(OrbitNotificationHelper.hasNotificationPermission(context))
                }
                var requestedNotifOnce by remember { mutableStateOf(false) }

                DisposableEffect(lifecycleOwner, context) {
                    val observer = LifecycleEventObserver { _, event ->
                        if (event == Lifecycle.Event.ON_RESUME) {
                            notifEnabled = OrbitNotificationHelper.hasNotificationPermission(context)
                        }
                    }
                    lifecycleOwner.lifecycle.addObserver(observer)
                    onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
                }

                fun openAppNotificationSettings() {
                    runCatching {
                        val intent = Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).apply {
                            putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
                        }
                        context.startActivity(intent)
                    }
                }

                val notifPermissionLauncher = rememberLauncherForActivityResult(
                    contract = ActivityResultContracts.RequestPermission(),
                ) { granted ->
                    notifEnabled = granted || OrbitNotificationHelper.hasNotificationPermission(context)
                    if (!notifEnabled) {
                        openAppNotificationSettings()
                    }
                }

                val pushLabel = when {
                    pushRegistrationStatus?.tokenRegistered == true ->
                        "FCM registered (${pushRegistrationStatus.tokenPreview.orEmpty()})"
                    notifEnabled -> "Approvals · Completed · Errors"
                    else -> "Notifications disabled — tap to enable"
                }
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 54.dp)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            if (!notifEnabled &&
                                !requestedNotifOnce &&
                                Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
                            ) {
                                requestedNotifOnce = true
                                notifPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                            } else {
                                openAppNotificationSettings()
                            }
                        }
                        .padding(horizontal = OrbitSpacing.md, vertical = 9.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Box(modifier = Modifier.size(20.dp), contentAlignment = Alignment.Center) {
                        Icon(
                            imageVector = Icons.Outlined.Notifications,
                            contentDescription = null,
                            tint = if (notifEnabled) colors.fgSecondary else colors.warning,
                            modifier = Modifier.size(17.dp),
                        )
                    }
                    Column(
                        modifier = Modifier.weight(1f),
                        verticalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        Text(
                            text = "Notifications & Push",
                            style = typography.subheadMedium,
                            color = colors.fgPrimary,
                        )
                        Text(
                            text = pushLabel,
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

        // 2. Hosts Fleet (Grouped List with Drill-Down)
        Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs + 2.dp)) {
            Text(
                text = "Hosts",
                style = typography.metaMedium,
                color = colors.fgMuted,
                modifier = Modifier.padding(horizontal = OrbitSpacing.xs),
            )

            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(cardShape)
                    .background(colors.elevated, cardShape)
                    .border(OrbitSpacing.hairline, colors.hairline, cardShape),
            ) {
                if (hosts.isEmpty()) {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(min = 48.dp)
                            .padding(horizontal = OrbitSpacing.md),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        Text(
                            text = "No hosts paired yet",
                            style = typography.subhead,
                            color = colors.fgMuted,
                        )
                    }
                    HorizontalDivider(
                        thickness = OrbitSpacing.hairline,
                        color = colors.hairline,
                        modifier = Modifier.padding(start = OrbitSpacing.md),
                    )
                } else {
                    hosts.forEach { host ->
                        val isActive = host.hostId == selectedHostId
                        val statusColor = hostConnectionDotColor(host.connectionState, colors)
                        val detailLine = buildString {
                            append(OrbitFormatters.topBarConnectionLabel(host.connectionState, host.viaSshTunnel))
                            host.lastPingMs?.let { append(" · ${it}ms") }
                            host.daemonVersion?.takeIf { it.isNotBlank() }?.let { raw ->
                                append(" · ")
                                append(if (raw.startsWith("v")) raw else "v$raw")
                            }
                        }

                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 54.dp)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onInspectHost(host.hostId)
                                }
                                .padding(horizontal = OrbitSpacing.md, vertical = 9.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                        ) {
                            Box(
                                modifier = Modifier.size(20.dp),
                                contentAlignment = Alignment.Center,
                            ) {
                                Box(
                                    modifier = Modifier
                                        .size(7.dp)
                                        .clip(CircleShape)
                                        .background(statusColor),
                                )
                            }

                            Column(
                                modifier = Modifier.weight(1f),
                                verticalArrangement = Arrangement.spacedBy(2.dp),
                            ) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                                ) {
                                    Text(
                                        text = host.displayName,
                                        style = typography.subheadMedium,
                                        color = colors.fgPrimary,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                        modifier = Modifier.weight(1f, fill = false),
                                    )
                                    if (isActive && hosts.size > 1) {
                                        Box(
                                            modifier = Modifier
                                                .background(colors.subtleFill, CircleShape)
                                                .padding(horizontal = 6.dp, vertical = 1.dp),
                                        ) {
                                            Text(
                                                text = "Active",
                                                style = typography.meta,
                                                color = colors.fgSecondary,
                                            )
                                        }
                                    }
                                }
                                Text(
                                    text = detailLine,
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

                        HorizontalDivider(
                            thickness = OrbitSpacing.hairline,
                            color = colors.hairline,
                            modifier = Modifier.padding(start = 42.dp),
                        )
                    }
                }

                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = OrbitSpacing.touchTarget)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onOpenPairHost()
                        }
                        .padding(horizontal = OrbitSpacing.md, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Box(
                        modifier = Modifier.size(20.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.QrCodeScanner,
                            contentDescription = null,
                            tint = colors.primary,
                            modifier = Modifier.size(16.dp),
                        )
                    }
                    Text(
                        text = "Pair New Host",
                        style = typography.subheadMedium,
                        color = colors.primary,
                    )
                }
            }
        }

        // 3. Usage & Quotas (shown when connected to a host)
        if (selectedHostId != null) {
            Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs + 2.dp)) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = OrbitSpacing.xs),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = "Usage & Quotas",
                        style = typography.metaMedium,
                        color = colors.fgMuted,
                    )
                    Row(
                        modifier = Modifier
                            .clip(RoundedCornerShape(6.dp))
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onRefreshQuotas()
                            }
                            .padding(horizontal = 6.dp, vertical = 3.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Refresh,
                            contentDescription = "Refresh quotas",
                            tint = colors.fgSecondary,
                            modifier = Modifier.size(13.dp),
                        )
                        Text(
                            text = "Refresh",
                            style = typography.metaMedium,
                            color = colors.fgSecondary,
                        )
                    }
                }

                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(cardShape)
                        .background(colors.elevated, cardShape)
                        .border(OrbitSpacing.hairline, colors.hairline, cardShape),
                ) {
                    if (quotas.isEmpty()) {
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 48.dp)
                                .padding(horizontal = OrbitSpacing.md),
                            contentAlignment = Alignment.CenterStart,
                        ) {
                            Text(
                                text = "No active rate-limit windows reported yet.",
                                style = typography.subhead,
                                color = colors.fgMuted,
                            )
                        }
                    } else {
                        quotas.forEachIndexed { idx, quota ->
                            ProviderQuotaBlock(quota = quota)
                            if (idx < quotas.lastIndex) {
                                HorizontalDivider(
                                    thickness = OrbitSpacing.hairline,
                                    color = colors.hairline,
                                    modifier = Modifier.padding(start = OrbitSpacing.md),
                                )
                            }
                        }
                    }
                }
            }
        }

        // 4. About
        Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs + 2.dp)) {
            Text(
                text = "About",
                style = typography.metaMedium,
                color = colors.fgMuted,
                modifier = Modifier.padding(horizontal = OrbitSpacing.xs),
            )
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(cardShape)
                    .background(colors.elevated, cardShape)
                    .border(OrbitSpacing.hairline, colors.hairline, cardShape),
            ) {
                SettingsKeyValueRow(
                    label = "Version",
                    value = "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})",
                )
                HorizontalDivider(
                    thickness = OrbitSpacing.hairline,
                    color = colors.hairline,
                    modifier = Modifier.padding(start = OrbitSpacing.md),
                )
                SettingsKeyValueRow(
                    label = "Encryption",
                    value = "Noise NK · E2EE",
                )
            }
        }

        Spacer(modifier = Modifier.height(OrbitSpacing.md))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HostDetailSettingsPane(
    host: FfiHostCard,
    isActiveHost: Boolean,
    onReconnect: () -> Unit,
    onSwitchHost: () -> Unit,
    onSave: (
        customName: String?,
        routeMode: FfiHostRouteMode,
        customDirectEndpoint: String?,
        relayUrl: String?,
    ) -> Unit,
    onRemove: () -> Unit,
    onShowToast: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val context = LocalContext.current
    val cardShape = RoundedCornerShape(14.dp)

    val selectableRouteModes = remember {
        listOf(FfiHostRouteMode.AUTO, FfiHostRouteMode.DIRECT_ONLY, FfiHostRouteMode.RELAY_ONLY)
    }

    var customName by remember(host.hostId) { mutableStateOf(host.customName.orEmpty()) }
    var routeMode by remember(host.hostId) {
        mutableStateOf(if (host.routeMode == FfiHostRouteMode.SSH_TUNNEL) FfiHostRouteMode.AUTO else host.routeMode)
    }
    var directEndpoint by remember(host.hostId) { mutableStateOf(host.customDirectEndpoint.orEmpty()) }
    var relayUrl by remember(host.hostId) { mutableStateOf(host.relayUrl.orEmpty()) }
    var showRemoveConfirm by remember { mutableStateOf(false) }

    val statusColor = hostConnectionDotColor(host.connectionState, colors)
    val statusText = OrbitFormatters.connectionLabel(host.connectionState, host.viaSshTunnel)
    val pingText = host.lastPingMs?.let { "$it ms" } ?: "—"
    val daemonSummary = remember(host.daemonVersion, host.osArch) {
        listOfNotNull(
            host.daemonVersion?.takeIf { it.isNotBlank() }?.let { if (it.startsWith("v")) it else "v$it" },
            host.osArch?.takeIf { it.isNotBlank() },
        ).joinToString(" · ").ifEmpty { "—" }
    }

    if (showRemoveConfirm) {
        AlertDialog(
            onDismissRequest = { showRemoveConfirm = false },
            containerColor = colors.elevated,
            titleContentColor = colors.fgPrimary,
            textContentColor = colors.fgSecondary,
            title = { Text("Remove ${host.displayName}?", style = typography.title) },
            text = {
                Text(
                    text = "Running remote agent sessions on this host will not be terminated.",
                    style = typography.subhead,
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        showRemoveConfirm = false
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        onRemove()
                    },
                ) {
                    Text("Remove", color = colors.dangerRemoved, style = typography.subheadSemiBold)
                }
            },
            dismissButton = {
                TextButton(onClick = { showRemoveConfirm = false }) {
                    Text("Cancel", color = colors.fgSecondary, style = typography.subheadMedium)
                }
            },
        )
    }

    Column(
        modifier = modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.lg),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xl),
    ) {
        // 1. Diagnostics Card
        Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs + 2.dp)) {
            Text(
                text = "Diagnostics",
                style = typography.metaMedium,
                color = colors.fgMuted,
                modifier = Modifier.padding(horizontal = OrbitSpacing.xs),
            )
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(cardShape)
                    .background(colors.elevated, cardShape)
                    .border(OrbitSpacing.hairline, colors.hairline, cardShape),
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = OrbitSpacing.touchTarget)
                        .padding(horizontal = OrbitSpacing.md),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    Box(
                        modifier = Modifier
                            .size(7.dp)
                            .clip(CircleShape)
                            .background(statusColor),
                    )
                    Text(
                        text = statusText,
                        style = typography.subheadMedium,
                        color = colors.fgPrimary,
                        modifier = Modifier.weight(1f),
                    )
                    Row(
                        modifier = Modifier
                            .height(30.dp)
                            .clip(CircleShape)
                            .background(colors.subtleFill, CircleShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onReconnect()
                            }
                            .padding(horizontal = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Refresh,
                            contentDescription = "Reconnect",
                            tint = colors.primary,
                            modifier = Modifier.size(13.dp),
                        )
                        Text(
                            text = "Reconnect",
                            style = typography.metaSemiBold,
                            color = colors.primary,
                        )
                    }
                }

                HorizontalDivider(
                    thickness = OrbitSpacing.hairline,
                    color = colors.hairline,
                    modifier = Modifier.padding(start = OrbitSpacing.md),
                )
                SettingsKeyValueRow(label = "Latency", value = pingText)

                HorizontalDivider(
                    thickness = OrbitSpacing.hairline,
                    color = colors.hairline,
                    modifier = Modifier.padding(start = OrbitSpacing.md),
                )
                SettingsKeyValueRow(label = "Daemon", value = daemonSummary)

                HorizontalDivider(
                    thickness = OrbitSpacing.hairline,
                    color = colors.hairline,
                    modifier = Modifier.padding(start = OrbitSpacing.md),
                )
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = OrbitSpacing.touchTarget)
                        .clickable {
                            val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                            cm?.setPrimaryClip(ClipData.newPlainText("daemon_pubkey", host.daemonPubkeyB64))
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onShowToast("Copied public key")
                        }
                        .padding(horizontal = OrbitSpacing.md),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    Text(
                        text = "Public Key",
                        style = typography.subhead,
                        color = colors.fgPrimary,
                    )
                    Spacer(modifier = Modifier.weight(1f))
                    Text(
                        text = "${host.daemonPubkeyB64.take(16)}…",
                        style = typography.code,
                        color = colors.fgMuted,
                        maxLines = 1,
                    )
                    Icon(
                        imageVector = OrbitCopyIcon,
                        contentDescription = "Copy public key",
                        tint = colors.fgMuted,
                        modifier = Modifier.size(14.dp),
                    )
                }

                host.lastError?.takeIf { it.isNotBlank() }?.let { err ->
                    HorizontalDivider(
                        thickness = OrbitSpacing.hairline,
                        color = colors.hairline,
                        modifier = Modifier.padding(start = OrbitSpacing.md),
                    )
                    Text(
                        text = err,
                        style = typography.code,
                        color = colors.dangerRemoved,
                        modifier = Modifier.padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                    )
                }

                if (!isActiveHost) {
                    HorizontalDivider(
                        thickness = OrbitSpacing.hairline,
                        color = colors.hairline,
                        modifier = Modifier.padding(start = OrbitSpacing.md),
                    )
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(min = OrbitSpacing.touchTarget)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onSwitchHost()
                            }
                            .padding(horizontal = OrbitSpacing.md),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        Text(
                            text = "Switch to This Host",
                            style = typography.subheadMedium,
                            color = colors.primary,
                        )
                    }
                }
            }
        }

        // 2. Connection Card
        Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm)) {
            Text(
                text = "Connection",
                style = typography.metaMedium,
                color = colors.fgMuted,
                modifier = Modifier.padding(horizontal = OrbitSpacing.xs),
            )

            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(cardShape)
                    .background(colors.elevated, cardShape)
                    .border(OrbitSpacing.hairline, colors.hairline, cardShape),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.md),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs + 2.dp),
                ) {
                    Text(
                        text = "Route Mode",
                        style = typography.metaMedium,
                        color = colors.fgSecondary,
                    )
                    SingleChoiceSegmentedButtonRow(
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        selectableRouteModes.forEachIndexed { idx, mode ->
                            val selected = routeMode == mode
                            SegmentedButton(
                                selected = selected,
                                onClick = {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    routeMode = mode
                                },
                                shape = SegmentedButtonDefaults.itemShape(
                                    index = idx,
                                    count = selectableRouteModes.size,
                                ),
                                colors = SegmentedButtonDefaults.colors(
                                    activeContainerColor = colors.subtleFill,
                                    activeContentColor = colors.fgPrimary,
                                    inactiveContainerColor = Color.Transparent,
                                    inactiveContentColor = colors.fgSecondary,
                                    activeBorderColor = colors.hairline,
                                    inactiveBorderColor = colors.hairline,
                                ),
                                icon = {},
                            ) {
                                Text(
                                    text = OrbitFormatters.routeModeLabel(mode),
                                    style = if (selected) typography.metaSemiBold else typography.metaMedium,
                                    maxLines = 1,
                                    softWrap = false,
                                )
                            }
                        }
                    }
                }

                HorizontalDivider(
                    thickness = OrbitSpacing.hairline,
                    color = colors.hairline,
                    modifier = Modifier.padding(start = OrbitSpacing.md),
                )
                GroupedTextFieldRow(
                    label = "Display Name",
                    placeholder = host.hostname,
                    value = customName,
                    isMono = false,
                    onValueChange = { customName = it },
                )

                HorizontalDivider(
                    thickness = OrbitSpacing.hairline,
                    color = colors.hairline,
                    modifier = Modifier.padding(start = OrbitSpacing.md),
                )
                GroupedTextFieldRow(
                    label = "Direct Endpoint",
                    placeholder = host.directEndpoints.firstOrNull() ?: "ws://192.168.1.x:6868",
                    value = directEndpoint,
                    isMono = true,
                    onValueChange = { directEndpoint = it },
                )

                HorizontalDivider(
                    thickness = OrbitSpacing.hairline,
                    color = colors.hairline,
                    modifier = Modifier.padding(start = OrbitSpacing.md),
                )
                GroupedTextFieldRow(
                    label = "Relay URL",
                    placeholder = "wss://relay.orbit.dev",
                    value = relayUrl,
                    isMono = true,
                    onValueChange = { relayUrl = it },
                )
            }

            val saveShape = RoundedCornerShape(12.dp)
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(OrbitSpacing.touchTarget)
                    .clip(saveShape)
                    .background(colors.primary, saveShape)
                    .clickable {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onSave(
                            customName.trim().ifBlank { null },
                            routeMode,
                            directEndpoint.trim().ifBlank { null },
                            relayUrl.trim().ifBlank { null },
                        )
                    },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = "Save & Reconnect",
                    style = typography.subheadSemiBold,
                    color = colors.onPrimary,
                )
            }
        }

        // 3. Danger Zone
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(OrbitSpacing.touchTarget)
                .clip(cardShape)
                .background(colors.elevated, cardShape)
                .border(OrbitSpacing.hairline, colors.hairline, cardShape)
                .clickable {
                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    showRemoveConfirm = true
                },
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = "Remove Host",
                style = typography.subheadMedium,
                color = colors.dangerRemoved,
            )
        }

        Spacer(modifier = Modifier.height(OrbitSpacing.md))
    }
}

private fun hostConnectionDotColor(
    state: FfiConnectionState,
    colors: dev.orbit.android.ui.theme.OrbitColors,
): Color = when (state) {
    FfiConnectionState.ONLINE_DIRECT -> colors.directAdded
    FfiConnectionState.ONLINE_RELAY -> colors.relay
    FfiConnectionState.CONNECTING_DIRECT, FfiConnectionState.CONNECTING_RELAY -> colors.warning
    FfiConnectionState.DISCONNECTED -> colors.dangerRemoved
}

@Composable
private fun SettingsKeyValueRow(
    label: String,
    value: String,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = OrbitSpacing.touchTarget)
            .padding(horizontal = OrbitSpacing.md),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            text = label,
            style = typography.subhead,
            color = colors.fgPrimary,
        )
        Text(
            text = value,
            style = typography.code,
            color = colors.fgMuted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun GroupedTextFieldRow(
    label: String,
    placeholder: String,
    value: String,
    isMono: Boolean,
    onValueChange: (String) -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val textStyle = if (isMono) typography.code else typography.subhead

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = OrbitSpacing.md, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(
            text = label,
            style = typography.metaMedium,
            color = colors.fgSecondary,
        )
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 26.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            if (value.isEmpty()) {
                Text(
                    text = placeholder,
                    style = textStyle,
                    color = colors.fgMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            BasicTextField(
                value = value,
                onValueChange = onValueChange,
                singleLine = true,
                textStyle = textStyle.copy(color = colors.fgPrimary),
                cursorBrush = SolidColor(colors.primary),
                modifier = Modifier.fillMaxWidth(),
            )
        }
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
private fun ProviderQuotaBlock(quota: FfiProviderQuota) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(OrbitSpacing.md),
        verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm + 2.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        ) {
            OrbitProviderIcon(provider = quota.provider, size = 15.dp)

            Text(
                text = quota.providerLabel,
                style = typography.subheadSemiBold,
                color = colors.fgPrimary,
            )

            quota.accountLabel?.takeIf { it.isNotBlank() }?.let { acct ->
                Text(
                    text = acct,
                    style = typography.meta,
                    color = colors.fgMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
            }

            Spacer(modifier = Modifier.weight(1f))

            quota.creditsRemaining?.let { credits ->
                Text(
                    text = "${"$%.2f".format(credits)}",
                    style = typography.codeMedium,
                    color = colors.directAdded,
                )
            }
        }

        quota.error?.takeIf { it.isNotBlank() }?.let { err ->
            Text(
                text = err,
                style = typography.meta,
                color = colors.warning,
            )
        }

        if (quota.windows.isEmpty() && quota.error.isNullOrBlank()) {
            Text(
                text = "No active rate-limit windows",
                style = typography.meta,
                color = colors.fgMuted,
            )
        } else {
            quota.windows.forEach { win ->
                val pct = win.usedPercent.coerceIn(0f, 100f)
                val barColor = when {
                    pct > 90f -> colors.dangerRemoved
                    pct >= 70f -> colors.warning
                    else -> colors.directAdded
                }
                val barShape = CircleShape

                Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    ) {
                        Text(
                            text = win.label,
                            style = typography.metaMedium,
                            color = colors.fgSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        win.resetsAtRfc3339?.takeIf { it.isNotBlank() }?.let { reset ->
                            Text(
                                text = formatQuotaResetCountdown(reset),
                                style = typography.meta,
                                color = colors.fgMuted,
                                maxLines = 1,
                            )
                        }
                        Text(
                            text = "${pct.roundToInt()}%",
                            style = typography.codeMedium,
                            color = barColor,
                        )
                    }
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(4.dp)
                            .clip(barShape)
                            .background(colors.subtleFill, barShape),
                    ) {
                        Box(
                            modifier = Modifier
                                .fillMaxWidth(fraction = (pct / 100f).coerceIn(0.02f, 1f))
                                .height(4.dp)
                                .background(barColor, barShape),
                        )
                    }
                }
            }
        }
    }
}
