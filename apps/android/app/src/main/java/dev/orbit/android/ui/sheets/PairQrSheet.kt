package dev.orbit.android.ui.sheets

import android.Manifest
import android.content.ClipboardManager
import android.content.Context
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CameraAlt
import androidx.compose.material.icons.outlined.ContentPaste
import androidx.compose.material.icons.outlined.QrCodeScanner
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
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
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import java.util.EnumMap
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

private val OrbitPairUriPattern = Regex("(orbit://pair[^\\s\"'<>]+)", RegexOption.IGNORE_CASE)

private fun extractOrbitPairUriFromClipboard(context: Context): String? {
    val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return null
    val raw = cm.primaryClip
        ?.takeIf { it.itemCount > 0 }
        ?.getItemAt(0)
        ?.coerceToText(context)
        ?.toString()
        ?.trim()
        .orEmpty()
    if (raw.isEmpty()) return null
    val matched = OrbitPairUriPattern.find(raw)?.value?.trim() ?: return null
    return matched.takeIf { it.length > "orbit://pair/".length }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairQrSheet(
    localDaemonAvailable: Boolean,
    lastError: String?,
    onPairUri: (String) -> Unit,
    onDiscoverLocal: () -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val haptics = LocalHapticFeedback.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    var hasCameraPermission by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        )
    }
    var scannerExpanded by remember { mutableStateOf(hasCameraPermission) }
    var manualUri by remember { mutableStateOf("") }
    var clipboardPairUri by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        clipboardPairUri = extractOrbitPairUriFromClipboard(context)
    }

    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                clipboardPairUri = extractOrbitPairUriFromClipboard(context)
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    val permissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission(),
    ) { granted ->
        hasCameraPermission = granted
        scannerExpanded = granted
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
                .imePadding()
                .navigationBarsPadding()
                .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.sm),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
        ) {
            Text(
                text = "Pair Host",
                style = typography.title,
                color = colors.fgPrimary,
            )
            Text(
                text = "Run `orbit pair` on your workstation or devbox to generate an encrypted Noise NK pairing QR code.",
                style = typography.subhead,
                color = colors.fgSecondary,
            )

            // 1-Tap Clipboard Pairing Banner when clipboard holds a valid `orbit://pair/...` URI
            clipboardPairUri?.let { detectedUri ->
                val bannerShape = RoundedCornerShape(OrbitSpacing.md)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 56.dp)
                        .clip(bannerShape)
                        .background(colors.subtleFill, bannerShape)
                        .border(OrbitSpacing.hairline, colors.primary.copy(alpha = 0.7f), bannerShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onPairUri(detectedUri)
                        }
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                ) {
                    Box(
                        modifier = Modifier
                            .size(36.dp)
                            .background(colors.primary, RoundedCornerShape(OrbitSpacing.sm)),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.ContentPaste,
                            contentDescription = null,
                            tint = colors.onPrimary,
                            modifier = Modifier.size(OrbitSpacing.iconMd),
                        )
                    }
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            text = "Pair from Clipboard",
                            style = typography.bodySemiBold,
                            color = colors.primary,
                            maxLines = 1,
                            softWrap = false,
                        )
                        Text(
                            text = detectedUri,
                            style = typography.code,
                            color = colors.fgSecondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    Box(
                        modifier = Modifier
                            .background(colors.primary, RoundedCornerShape(OrbitSpacing.sm))
                            .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = "Pair",
                            style = typography.subheadSemiBold,
                            color = colors.onPrimary,
                            maxLines = 1,
                            softWrap = false,
                        )
                    }
                }
            }

            // CameraX + ZXing Offline QR Viewfinder
            if (scannerExpanded && hasCameraPermission) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(240.dp)
                        .clip(RoundedCornerShape(OrbitSpacing.lg))
                        .background(Color.Black, RoundedCornerShape(OrbitSpacing.lg))
                        .border(OrbitSpacing.hairline, colors.hairline, RoundedCornerShape(OrbitSpacing.lg)),
                ) {
                    ZxingCameraPreview(
                        onQrDecoded = { decoded ->
                            haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                            onPairUri(decoded)
                        }
                    )
                    // Framing Reticle
                    Box(
                        modifier = Modifier
                            .size(164.dp)
                            .align(Alignment.Center)
                            .border(1.5.dp, Color.White.copy(alpha = 0.72f), RoundedCornerShape(OrbitSpacing.lg))
                    )
                    Text(
                        text = "Scan QR Code",
                        style = typography.metaMedium,
                        color = Color.White,
                        modifier = Modifier
                            .align(Alignment.BottomCenter)
                            .padding(bottom = OrbitSpacing.sm)
                            .background(Color(0x99000000), RoundedCornerShape(OrbitSpacing.sm))
                            .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                    )
                }
            } else {
                val btnShape = RoundedCornerShape(OrbitSpacing.md)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = OrbitSpacing.touchTarget)
                        .clip(btnShape)
                        .background(colors.subtleFill, btnShape)
                        .border(OrbitSpacing.hairline, colors.hairline, btnShape)
                        .clickable {
                            if (hasCameraPermission) {
                                scannerExpanded = true
                            } else {
                                permissionLauncher.launch(Manifest.permission.CAMERA)
                            }
                        }
                        .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.md),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.Center,
                ) {
                    Icon(
                        imageVector = if (hasCameraPermission) Icons.Outlined.QrCodeScanner else Icons.Outlined.CameraAlt,
                        contentDescription = null,
                        tint = colors.primary,
                        modifier = Modifier.size(OrbitSpacing.iconMd),
                    )
                    Spacer(modifier = Modifier.size(OrbitSpacing.sm))
                    Text(
                        text = "Scan QR Code",
                        style = typography.bodySemiBold,
                        color = colors.primary,
                    )
                }
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            // Paste `orbit://pair/...` Fallback
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Pairing URI",
                    style = typography.subheadSemiBold,
                    color = colors.fgSecondary,
                )
                Row(
                    modifier = Modifier
                        .clip(RoundedCornerShape(OrbitSpacing.sm))
                        .clickable {
                            val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                            val clip = cm?.primaryClip
                                ?.takeIf { it.itemCount > 0 }
                                ?.getItemAt(0)
                                ?.coerceToText(context)
                                ?.toString()
                                ?.trim()
                                .orEmpty()
                            if (clip.isNotEmpty()) {
                                manualUri = clip
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            }
                        }
                        .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.ContentPaste,
                        contentDescription = "Paste from clipboard",
                        tint = colors.primary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                    Text(
                        text = "Paste",
                        style = typography.metaMedium,
                        color = colors.primary,
                    )
                }
            }

            val inputShape = RoundedCornerShape(OrbitSpacing.md)
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = OrbitSpacing.touchTarget)
                    .background(colors.code, inputShape)
                    .border(OrbitSpacing.hairline, colors.hairline, inputShape)
                    .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.md),
                contentAlignment = Alignment.CenterStart,
            ) {
                if (manualUri.isEmpty()) {
                    Text(
                        text = "orbit://pair/...",
                        style = typography.code,
                        color = colors.fgMuted,
                    )
                }
                BasicTextField(
                    value = manualUri,
                    onValueChange = { manualUri = it },
                    textStyle = typography.code.copy(color = colors.fgPrimary),
                    cursorBrush = SolidColor(colors.primary),
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            }

            lastError?.let { err ->
                Text(
                    text = err,
                    style = typography.meta,
                    color = colors.dangerRemoved,
                )
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                if (localDaemonAvailable) {
                    val secShape = RoundedCornerShape(OrbitSpacing.md)
                    Box(
                        modifier = Modifier
                            .heightIn(min = OrbitSpacing.touchTarget)
                            .clip(secShape)
                            .background(colors.subtleFill, secShape)
                            .border(OrbitSpacing.hairline, colors.hairline, secShape)
                            .clickable { onDiscoverLocal() }
                            .padding(horizontal = OrbitSpacing.md),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = "Local Host",
                            style = typography.subheadMedium,
                            color = colors.fgPrimary,
                        )
                    }
                }

                val canPair = manualUri.trim().isNotEmpty()
                val priShape = RoundedCornerShape(OrbitSpacing.md)
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = OrbitSpacing.touchTarget)
                        .clip(priShape)
                        .background(if (canPair) colors.primary else colors.subtleFill, priShape)
                        .clickable(enabled = canPair) {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onPairUri(manualUri.trim())
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "Pair Host",
                        style = typography.bodySemiBold,
                        color = if (canPair) colors.onPrimary else colors.fgMuted,
                    )
                }
            }

            Spacer(modifier = Modifier.height(OrbitSpacing.xs))
        }
    }
}

@Composable
internal fun ZxingCameraPreview(
    accepts: (String) -> Boolean = { raw ->
        val action = dev.orbit.android.notifications.OrbitPushRouter.parseDeepLink(raw)
        action is dev.orbit.android.notifications.OrbitDeepLinkAction.PairHost ||
            action is dev.orbit.android.notifications.OrbitDeepLinkAction.ImportDeviceLink
    },
    onQrDecoded: (String) -> Unit,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val currentAccepts by rememberUpdatedState(accepts)
    val currentOnQrDecoded by rememberUpdatedState(onQrDecoded)
    val decodedOnce = remember { AtomicBoolean(false) }
    val analysisExecutor = remember { Executors.newSingleThreadExecutor() }

    val previewView = remember {
        PreviewView(context).apply {
            implementationMode = PreviewView.ImplementationMode.COMPATIBLE
            scaleType = PreviewView.ScaleType.FILL_CENTER
        }
    }

    DisposableEffect(lifecycleOwner) {
        val cameraProviderFuture = ProcessCameraProvider.getInstance(context)
        val mainExecutor = ContextCompat.getMainExecutor(context)
        val disposed = AtomicBoolean(false)

        val listener = Runnable {
            if (disposed.get()) return@Runnable
            val provider = runCatching { cameraProviderFuture.get() }.getOrNull() ?: return@Runnable
            if (disposed.get()) {
                runCatching { provider.unbindAll() }
                return@Runnable
            }
            val preview = Preview.Builder().build().also {
                it.surfaceProvider = previewView.surfaceProvider
            }

            val reader = QRCodeReader()
            val hints = EnumMap<DecodeHintType, Any>(DecodeHintType::class.java).apply {
                put(DecodeHintType.POSSIBLE_FORMATS, listOf(BarcodeFormat.QR_CODE))
                put(DecodeHintType.TRY_HARDER, true)
            }

            val analysis = ImageAnalysis.Builder()
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .build()
                .also { imageAnalysis ->
                    imageAnalysis.setAnalyzer(analysisExecutor) { imageProxy ->
                        try {
                            if (!disposed.get() && !decodedOnce.get()) {
                                val resultText = decodeOrbitQrFromImageProxy(imageProxy, reader, hints)
                                if (resultText != null && currentAccepts(resultText) && decodedOnce.compareAndSet(false, true)) {
                                    mainExecutor.execute {
                                        if (!disposed.get()) {
                                            currentOnQrDecoded(resultText)
                                        }
                                    }
                                }
                            }
                        } finally {
                            imageProxy.close()
                        }
                    }
                }

            runCatching {
                provider.unbindAll()
                if (!disposed.get()) {
                    provider.bindToLifecycle(
                        lifecycleOwner,
                        CameraSelector.DEFAULT_BACK_CAMERA,
                        preview,
                        analysis,
                    )
                }
            }
        }

        cameraProviderFuture.addListener(listener, mainExecutor)

        onDispose {
            disposed.set(true)
            runCatching {
                if (cameraProviderFuture.isDone) {
                    cameraProviderFuture.get().unbindAll()
                }
            }
            analysisExecutor.shutdown()
        }
    }

    AndroidView(
        factory = { previewView },
        modifier = Modifier.fillMaxSize(),
    )
}

private fun decodeOrbitQrFromImageProxy(
    imageProxy: ImageProxy,
    reader: QRCodeReader,
    hints: Map<DecodeHintType, Any>,
): String? {
    val plane = imageProxy.planes.firstOrNull() ?: return null
    val width = imageProxy.width
    val height = imageProxy.height
    if (width <= 0 || height <= 0) return null

    val buffer = plane.buffer
    buffer.rewind()
    val rowStride = plane.rowStride
    val yBytes = ByteArray(width * height)

    if (rowStride == width) {
        val count = minOf(buffer.remaining(), yBytes.size)
        buffer.get(yBytes, 0, count)
    } else {
        for (row in 0 until height) {
            val pos = row * rowStride
            if (pos + width <= buffer.limit()) {
                buffer.position(pos)
                buffer.get(yBytes, row * width, width)
            }
        }
    }

    val source = PlanarYUVLuminanceSource(
        yBytes,
        width,
        height,
        0,
        0,
        width,
        height,
        false,
    )

    // Try standard black-on-white QR first, then inverted white-on-black (dark terminal `orbit pair` output)
    val standardText = runCatching {
        reader.decode(BinaryBitmap(HybridBinarizer(source)), hints).text
    }.getOrNull().also { reader.reset() }

    if (standardText != null && standardText.startsWith("orbit://", ignoreCase = true)) {
        return standardText
    }

    val invertedText = runCatching {
        reader.decode(BinaryBitmap(HybridBinarizer(source.invert())), hints).text
    }.getOrNull().also { reader.reset() }

    return invertedText?.takeIf { it.startsWith("orbit://", ignoreCase = true) } ?: standardText
}
