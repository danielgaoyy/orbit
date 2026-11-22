package dev.orbit.android.ui.sheets

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PersistableBundle
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
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.CloudSync
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import kotlinx.coroutines.launch
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import dev.orbit.android.ui.theme.OrbitCopyIcon
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiCloudSyncState
import uniffi.orbit_ffi_uniffi.FfiCloudSyncStatus
import uniffi.orbit_ffi_uniffi.FfiGitHubDeviceFlowStart

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CloudSyncSheet(
    status: FfiCloudSyncStatus?,
    deviceFlow: FfiGitHubDeviceFlowStart?,
    isBusy: Boolean,
    errorMessage: String?,
    onRefreshStatus: () -> Unit,
    onStartDeviceFlow: (clientIdOverride: String?) -> Unit,
    onCancelDeviceFlow: () -> Unit,
    onLoginWithToken: (token: String) -> Unit,
    onGeneratePassphrase: suspend () -> String,
    onUnlockWithPassphrase: (passphrase: String) -> Unit,
    onSyncNow: () -> Unit,
    onSetAutoSync: (Boolean) -> Unit,
    onDisconnect: (clearLocalHosts: Boolean) -> Unit,
    onShowToast: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val groupedBg = if (colors.isDark) colors.canvas else colors.drawer
    val cardShape = RoundedCornerShape(14.dp)

    val syncState = status?.state ?: FfiCloudSyncState.SIGNED_OUT
    val oauthConfigured = status?.githubOauthConfigured == true

    var useTokenAuth by remember { mutableStateOf(false) }
    var clientIdInput by remember { mutableStateOf(status?.githubClientIdOverride.orEmpty()) }
    var tokenInput by remember { mutableStateOf("") }
    var passphraseInput by remember { mutableStateOf("") }
    var confirmPassphraseInput by remember { mutableStateOf("") }
    var generatedRecoveryKey by remember { mutableStateOf<String?>(null) }
    var localValidationError by remember { mutableStateOf<String?>(null) }
    var confirmDisconnect by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        onRefreshStatus()
    }

    fun copyText(label: String, text: String, sensitive: Boolean = false) {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        val clip = ClipData.newPlainText(label, text)
        if (sensitive && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            clip.description.extras = PersistableBundle().apply {
                putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
            }
        }
        cm?.setPrimaryClip(clip)
        onShowToast("$label copied")
    }

    fun openUrl(url: String) {
        val parsed = runCatching { Uri.parse(url.trim()) }.getOrNull() ?: return
        if (!parsed.scheme.equals("https", ignoreCase = true) || parsed.host.isNullOrBlank()) {
            onShowToast("Blocked non-HTTPS link")
            return
        }
        runCatching {
            context.startActivity(Intent(Intent.ACTION_VIEW, parsed))
        }
    }

    if (confirmDisconnect) {
        AlertDialog(
            onDismissRequest = { confirmDisconnect = false },
            containerColor = colors.elevated,
            titleContentColor = colors.fgPrimary,
            textContentColor = colors.fgSecondary,
            title = { Text("Disconnect Cloud Sync?", style = typography.title) },
            text = {
                Text(
                    text = "Your paired hosts stay on this device, and your encrypted Secret Gist remains on GitHub.",
                    style = typography.subhead,
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmDisconnect = false
                        onDisconnect(false)
                    },
                ) {
                    Text("Disconnect", color = colors.dangerRemoved, style = typography.subheadSemiBold)
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmDisconnect = false }) {
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
                .fillMaxHeight(0.84f)
                .imePadding()
                .navigationBarsPadding(),
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg)
                    .padding(bottom = OrbitSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Icon(
                    imageVector = Icons.Outlined.CloudSync,
                    contentDescription = null,
                    tint = colors.fgPrimary,
                    modifier = Modifier.size(20.dp),
                )
                Text(
                    text = "Cloud Sync",
                    style = typography.title,
                    color = colors.fgPrimary,
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

            Column(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(OrbitSpacing.lg),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.lg),
            ) {
                // Status summary header
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(
                        text = "GitHub Secret Gist · E2EE",
                        style = typography.metaMedium,
                        color = colors.fgMuted,
                    )
                    val (badgeText, badgeColor) = when (syncState) {
                        FfiCloudSyncState.READY -> "Synced" to colors.directAdded
                        FfiCloudSyncState.SYNCING -> "Syncing…" to colors.warning
                        FfiCloudSyncState.LOCKED -> "Locked" to colors.warning
                        FfiCloudSyncState.ERROR -> "Error" to colors.dangerRemoved
                        FfiCloudSyncState.SIGNED_OUT -> "Signed Out" to colors.fgMuted
                    }
                    Text(
                        text = badgeText,
                        style = typography.metaSemiBold,
                        color = badgeColor,
                    )
                }

                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(cardShape)
                        .background(colors.elevated, cardShape)
                        .border(OrbitSpacing.hairline, colors.hairline, cardShape)
                        .padding(OrbitSpacing.md),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                ) {
                    when (syncState) {
                        FfiCloudSyncState.SIGNED_OUT -> {
                            if (deviceFlow != null) {
                                Text(
                                    text = "Enter this code on GitHub",
                                    style = typography.subheadSemiBold,
                                    color = colors.fgPrimary,
                                )
                                Row(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .clip(RoundedCornerShape(10.dp))
                                        .background(colors.subtleFill)
                                        .padding(OrbitSpacing.md),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                ) {
                                    Text(
                                        text = deviceFlow.userCode,
                                        style = typography.title,
                                        color = colors.fgPrimary,
                                    )
                                    Row(
                                        modifier = Modifier
                                            .clip(RoundedCornerShape(8.dp))
                                            .clickable { copyText("Device code", deviceFlow.userCode) }
                                            .padding(horizontal = 8.dp, vertical = 4.dp),
                                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                                        verticalAlignment = Alignment.CenterVertically,
                                    ) {
                                        Icon(
                                            imageVector = OrbitCopyIcon,
                                            contentDescription = "Copy code",
                                            tint = colors.primary,
                                            modifier = Modifier.size(14.dp),
                                        )
                                        Text("Copy", style = typography.metaSemiBold, color = colors.primary)
                                    }
                                }
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                                ) {
                                    PrimaryCloudButton(
                                        label = "Copy & Open GitHub",
                                        busy = false,
                                        onClick = {
                                            copyText("Device code", deviceFlow.userCode)
                                            openUrl(deviceFlow.verificationUri)
                                        },
                                        modifier = Modifier.weight(1f),
                                    )
                                    Box(
                                        modifier = Modifier
                                            .heightIn(min = 42.dp)
                                            .clip(RoundedCornerShape(10.dp))
                                            .background(colors.subtleFill)
                                            .clickable { onCancelDeviceFlow() }
                                            .padding(horizontal = OrbitSpacing.md),
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        Text("Cancel", style = typography.subheadMedium, color = colors.fgSecondary)
                                    }
                                }
                            } else {
                                Text(
                                    text = "Sync paired hosts and projects across devices using your own encrypted GitHub Secret Gist (gist scope only, zero Orbit servers).",
                                    style = typography.subhead,
                                    color = colors.fgSecondary,
                                )

                                if (!useTokenAuth) {
                                    if (!oauthConfigured) {
                                        CloudTextField(
                                            value = clientIdInput,
                                            onValueChange = { clientIdInput = it },
                                            placeholder = "GitHub OAuth Client ID (Ov23li…)",
                                            secret = false,
                                        )
                                    }
                                    PrimaryCloudButton(
                                        label = "Sign in with GitHub",
                                        busy = isBusy,
                                        onClick = {
                                            localValidationError = null
                                            onStartDeviceFlow(clientIdInput.trim().ifEmpty { null })
                                        },
                                    )
                                } else {
                                    CloudTextField(
                                        value = tokenInput,
                                        onValueChange = { tokenInput = it },
                                        placeholder = "GitHub Personal Access Token (gist scope)",
                                        secret = true,
                                    )
                                    Row(
                                        modifier = Modifier.fillMaxWidth(),
                                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                                        verticalAlignment = Alignment.CenterVertically,
                                    ) {
                                        PrimaryCloudButton(
                                            label = if (isBusy) "Verifying…" else "Continue with Token",
                                            busy = isBusy,
                                            enabled = tokenInput.isNotBlank() && !isBusy,
                                            onClick = {
                                                localValidationError = null
                                                val pat = tokenInput.trim()
                                                tokenInput = ""
                                                onLoginWithToken(pat)
                                            },
                                            modifier = Modifier.weight(1f),
                                        )
                                        Text(
                                            text = "Create PAT",
                                            style = typography.metaSemiBold,
                                            color = colors.primary,
                                            modifier = Modifier
                                                .clip(RoundedCornerShape(8.dp))
                                                .clickable {
                                                    openUrl("https://github.com/settings/tokens/new?scopes=gist&description=Orbit%20Cloud%20Sync")
                                                }
                                                .padding(horizontal = 8.dp, vertical = 6.dp),
                                        )
                                    }
                                }

                                Text(
                                    text = if (useTokenAuth) {
                                        "Use GitHub OAuth Device Login instead"
                                    } else {
                                        "Or sign in with a Personal Access Token (gist)"
                                    },
                                    style = typography.metaMedium,
                                    color = colors.fgMuted,
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(8.dp))
                                        .clickable {
                                            useTokenAuth = !useTokenAuth
                                            localValidationError = null
                                        }
                                        .padding(vertical = 4.dp),
                                )
                            }
                        }

                        FfiCloudSyncState.LOCKED -> {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                    Text(
                                        text = "@${status?.accountLogin ?: "github"}",
                                        style = typography.subheadSemiBold,
                                        color = colors.fgPrimary,
                                    )
                                    Text(
                                        text = if (status?.remoteVaultExists == true) {
                                            "Encrypted vault found — enter Sync Passphrase to unlock."
                                        } else {
                                            "Create a Sync Passphrase to encrypt your vault."
                                        },
                                        style = typography.meta,
                                        color = colors.fgSecondary,
                                    )
                                }
                                Text(
                                    text = "Disconnect",
                                    style = typography.metaSemiBold,
                                    color = colors.dangerRemoved,
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(8.dp))
                                        .clickable { confirmDisconnect = true }
                                        .padding(horizontal = 8.dp, vertical = 4.dp),
                                )
                            }

                            generatedRecoveryKey?.let { key ->
                                Column(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .clip(RoundedCornerShape(10.dp))
                                        .background(colors.subtleFill)
                                        .padding(OrbitSpacing.sm),
                                    verticalArrangement = Arrangement.spacedBy(4.dp),
                                ) {
                                    Row(
                                        modifier = Modifier.fillMaxWidth(),
                                        horizontalArrangement = Arrangement.SpaceBetween,
                                        verticalAlignment = Alignment.CenterVertically,
                                    ) {
                                        Text(
                                            text = "Generated Recovery Key (save safely)",
                                            style = typography.metaSemiBold,
                                            color = colors.directAdded,
                                        )
                                        Text(
                                            text = "Copy",
                                            style = typography.metaSemiBold,
                                            color = colors.primary,
                                            modifier = Modifier.clickable { copyText("Recovery Key", key, sensitive = true) },
                                        )
                                    }
                                    Text(text = key, style = typography.code, color = colors.fgPrimary)
                                }
                            }

                            CloudTextField(
                                value = passphraseInput,
                                onValueChange = { passphraseInput = it },
                                placeholder = if (status?.remoteVaultExists == true) {
                                    "Sync Passphrase or Recovery Key"
                                } else {
                                    "Create Sync Passphrase"
                                },
                                secret = true,
                            )

                            if (status?.remoteVaultExists != true && generatedRecoveryKey == null) {
                                CloudTextField(
                                    value = confirmPassphraseInput,
                                    onValueChange = { confirmPassphraseInput = it },
                                    placeholder = "Confirm Sync Passphrase",
                                    secret = true,
                                )
                            }

                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                PrimaryCloudButton(
                                    label = if (isBusy) {
                                        "Unlocking…"
                                    } else if (status?.remoteVaultExists == true) {
                                        "Unlock & Sync"
                                    } else {
                                        "Encrypt & Sync"
                                    },
                                    busy = isBusy,
                                    enabled = passphraseInput.isNotBlank() && !isBusy,
                                    onClick = {
                                        if (status?.remoteVaultExists != true &&
                                            generatedRecoveryKey == null &&
                                            passphraseInput != confirmPassphraseInput
                                        ) {
                                            localValidationError = "Passphrases do not match."
                                            return@PrimaryCloudButton
                                        }
                                        localValidationError = null
                                        val pass = passphraseInput
                                        passphraseInput = ""
                                        confirmPassphraseInput = ""
                                        generatedRecoveryKey = null
                                        onUnlockWithPassphrase(pass)
                                    },
                                    modifier = Modifier.weight(1f),
                                )

                                if (status?.remoteVaultExists != true) {
                                    Row(
                                        modifier = Modifier
                                            .heightIn(min = 42.dp)
                                            .clip(RoundedCornerShape(10.dp))
                                            .background(colors.subtleFill)
                                            .clickable {
                                                scope.launch {
                                                    val rec = onGeneratePassphrase()
                                                    if (rec.isNotBlank()) {
                                                        generatedRecoveryKey = rec
                                                        passphraseInput = rec
                                                        confirmPassphraseInput = rec
                                                    }
                                                }
                                            }
                                            .padding(horizontal = OrbitSpacing.md),
                                        verticalAlignment = Alignment.CenterVertically,
                                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                                    ) {
                                        Icon(
                                            imageVector = Icons.Outlined.Key,
                                            contentDescription = null,
                                            tint = colors.fgPrimary,
                                            modifier = Modifier.size(14.dp),
                                        )
                                        Text("Generate", style = typography.metaSemiBold, color = colors.fgPrimary)
                                    }
                                }
                            }
                        }

                        FfiCloudSyncState.READY,
                        FfiCloudSyncState.SYNCING,
                        FfiCloudSyncState.ERROR -> {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                    Text(
                                        text = "@${status?.accountLogin ?: "github"}",
                                        style = typography.subheadSemiBold,
                                        color = colors.fgPrimary,
                                    )
                                    Text(
                                        text = "${status?.syncedHostCount ?: 0u} hosts · ${status?.syncedProjectCount ?: 0u} projects · rev #${status?.lastVaultRevision ?: 0uL}",
                                        style = typography.meta,
                                        color = colors.fgMuted,
                                    )
                                }

                                Row(
                                    modifier = Modifier
                                        .height(34.dp)
                                        .clip(RoundedCornerShape(10.dp))
                                        .background(colors.primary)
                                        .clickable(enabled = !isBusy) { onSyncNow() }
                                        .padding(horizontal = 12.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                                ) {
                                    if (isBusy) {
                                        CircularProgressIndicator(
                                            modifier = Modifier.size(13.dp),
                                            strokeWidth = 1.8.dp,
                                            color = colors.onPrimary,
                                        )
                                    } else {
                                        Icon(
                                            imageVector = Icons.Outlined.Sync,
                                            contentDescription = null,
                                            tint = colors.onPrimary,
                                            modifier = Modifier.size(15.dp),
                                        )
                                    }
                                    Text(
                                        text = "Sync Now",
                                        style = typography.metaSemiBold,
                                        color = colors.onPrimary,
                                    )
                                }
                            }

                            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(
                                        text = "Auto-sync changes",
                                        style = typography.subhead,
                                        color = colors.fgPrimary,
                                    )
                                    Text(
                                        text = "Push & merge encrypted vault automatically",
                                        style = typography.meta,
                                        color = colors.fgMuted,
                                    )
                                }
                                Switch(
                                    checked = status?.autoSyncEnabled == true,
                                    onCheckedChange = onSetAutoSync,
                                    colors = SwitchDefaults.colors(checkedTrackColor = colors.directAdded),
                                )
                            }

                            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.SpaceBetween,
                            ) {
                                status?.gistHtmlUrl?.takeIf { it.isNotBlank() }?.let { url ->
                                    Row(
                                        modifier = Modifier
                                            .clip(RoundedCornerShape(8.dp))
                                            .clickable { openUrl(url) }
                                            .padding(vertical = 4.dp),
                                        verticalAlignment = Alignment.CenterVertically,
                                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                                    ) {
                                        Icon(
                                            imageVector = Icons.AutoMirrored.Outlined.OpenInNew,
                                            contentDescription = null,
                                            tint = colors.fgSecondary,
                                            modifier = Modifier.size(14.dp),
                                        )
                                        Text(
                                            text = "View Secret Gist",
                                            style = typography.metaMedium,
                                            color = colors.fgSecondary,
                                        )
                                    }
                                } ?: Spacer(modifier = Modifier.size(1.dp))

                                Text(
                                    text = "Disconnect",
                                    style = typography.metaSemiBold,
                                    color = colors.dangerRemoved,
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(8.dp))
                                        .clickable { confirmDisconnect = true }
                                        .padding(horizontal = 8.dp, vertical = 4.dp),
                                )
                            }
                        }
                    }

                    val displayErr = localValidationError ?: errorMessage ?: status?.lastError
                    if (!displayErr.isNullOrBlank()) {
                        Text(
                            text = displayErr,
                            style = typography.meta,
                            color = colors.dangerRemoved,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun CloudTextField(
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
    secret: Boolean,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .background(colors.subtleFill)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        if (value.isEmpty()) {
            Text(text = placeholder, style = typography.subhead, color = colors.fgMuted)
        }
        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            singleLine = true,
            visualTransformation = if (secret) PasswordVisualTransformation() else VisualTransformation.None,
            textStyle = typography.code.copy(color = colors.fgPrimary),
            cursorBrush = SolidColor(colors.primary),
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun PrimaryCloudButton(
    label: String,
    busy: Boolean,
    enabled: Boolean = !busy,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = 42.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(if (enabled) colors.primary else colors.fgMuted.copy(alpha = 0.25f))
            .clickable(enabled = enabled, onClick = onClick)
            .padding(horizontal = OrbitSpacing.md),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.Center,
    ) {
        if (busy) {
            CircularProgressIndicator(
                modifier = Modifier.size(15.dp),
                strokeWidth = 2.dp,
                color = colors.onPrimary,
            )
            Spacer(modifier = Modifier.size(8.dp))
        }
        Text(
            text = label,
            style = typography.subheadSemiBold,
            color = colors.onPrimary,
        )
    }
}
