package dev.orbit.android.ui.sheets

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.Bitmap
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.SecureFlagPolicy
import androidx.core.content.ContextCompat
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import dev.orbit.android.R
import dev.orbit.android.notifications.OrbitDeepLinkAction
import dev.orbit.android.notifications.OrbitPushRouter
import dev.orbit.android.store.*

@Composable
internal fun DeviceLinkControls(ui: OrbitUiState, store: OrbitAndroidStore, onScan: () -> Unit) {
    val status = ui.cloudSyncStatus
    var show by remember { mutableStateOf(false) }
    TextButton(onClick = onScan) { Text(stringResource(R.string.sync_scan_device_link)) }
    if (status?.accountLogin == null) return
    ToggleRow(R.string.sync_instant_unlock, status.instantUnlockEnabled) {
        store.setCloudManualPassphraseMode(!it)
    }
    TextButton(enabled = !ui.cloudSyncBusy, onClick = { show = true }) {
        Text(stringResource(R.string.sync_show_qr))
    }
    if (show) {
        val close = { show = false }
        val owner = androidx.lifecycle.compose.LocalLifecycleOwner.current
        DisposableEffect(owner) {
            val observer =
                androidx.lifecycle.LifecycleEventObserver { _, event ->
                    if (event == androidx.lifecycle.Lifecycle.Event.ON_STOP) close()
                }
            owner.lifecycle.addObserver(observer)
            onDispose { owner.lifecycle.removeObserver(observer) }
        }
        var image by remember { mutableStateOf<androidx.compose.ui.graphics.ImageBitmap?>(null) }
        var failed by remember { mutableStateOf(false) }
        LaunchedEffect(Unit) {
            try {
                val uri = store.exportCloudSyncDeviceLinkUri()
                image =
                    kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Default) {
                        val matrix = QRCodeWriter().encode(uri, BarcodeFormat.QR_CODE, 512, 512)
                        val pixels =
                            IntArray(matrix.width * matrix.height) { i ->
                                if (matrix[i % matrix.width, i / matrix.width])
                                    android.graphics.Color.BLACK
                                else android.graphics.Color.WHITE
                            }
                        Bitmap.createBitmap(
                                pixels,
                                matrix.width,
                                matrix.height,
                                Bitmap.Config.ARGB_8888,
                            )
                            .asImageBitmap()
                    }
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (_: Exception) {
                failed = true
            }
        }
        AlertDialog(
            onDismissRequest = close,
            properties = DialogProperties(securePolicy = SecureFlagPolicy.SecureOn),
            title = { Text(stringResource(R.string.sync_device_link_title)) },
            text = {
                val bitmap = image
                if (bitmap != null)
                    Image(
                        bitmap,
                        stringResource(R.string.sync_device_link_title),
                        Modifier.fillMaxWidth().aspectRatio(1f),
                    )
                else if (failed) Text(stringResource(R.string.sync_qr_failed))
                else LinearProgressIndicator(Modifier.fillMaxWidth())
            },
            confirmButton = {
                TextButton(onClick = close) { Text(stringResource(R.string.nav_close)) }
            },
        )
    }
}

@Composable
internal fun DeviceLinkScanner(onLink: (String) -> Unit, onClose: () -> Unit) {
    val context = LocalContext.current
    var allowed by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        )
    }
    val permission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
            allowed = it
        }
    ToolSheet(R.string.sync_scan_device_link, onClose) {
        if (allowed)
            Box(Modifier.fillMaxWidth().height(260.dp)) {
                ZxingCameraPreview(
                    accepts = {
                        OrbitPushRouter.parseDeepLink(it) is OrbitDeepLinkAction.ImportDeviceLink
                    },
                    onQrDecoded = onLink,
                )
            }
        else
            TextButton(onClick = { permission.launch(Manifest.permission.CAMERA) }) {
                Text(stringResource(R.string.composer_camera))
            }
    }
}
