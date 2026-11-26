import AVFoundation
import SwiftUI
import UIKit

struct QrPairSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var manualUri = ""
    @State private var detectedClipboardPairUri: String?
    @State private var inlineErrorMessage: String?
    @State private var cameraState: QrCameraState = .checking
    @State private var scanGeneration: Int = 0
    @FocusState private var isUriFieldFocused: Bool

    enum QrCameraState: Equatable {
        case checking
        case ready
        case unavailable(reason: String)
    }

    private var trimmedManualUri: String {
        manualUri.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        VStack(spacing: 0) {
            sheetHeader
            OrbitHairline()

            if let bannerError = inlineErrorMessage ?? store.lastError, !bannerError.isEmpty {
                HStack(spacing: OrbitSpacing.sm) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.danger)
                    Text(bannerError)
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(3)
                    Spacer()
                    Button {
                        inlineErrorMessage = nil
                        store.clearError()
                    } label: {
                        Image(systemName: "xmark")
                            .font(OrbitTypography.iconMicro)
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Dismiss Error")
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.xs)
                .background(palette.danger.opacity(0.12))
            }

            ScrollViewReader { proxy in
                ScrollView(.vertical, showsIndicators: true) {
                    VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                        if store.cloudSyncStatus?.state != .ready {
                            cloudSyncQuickCard
                        }

                        if let clipUri = detectedClipboardPairUri {
                            clipboardPairBannerCard(clipUri)
                        }

                        if store.localDaemonUri != nil {
                            localSimulatorDiscoveryCard
                        }

                        cameraScannerSection
                        manualUriSection
                            .id("manual-uri-section")

                        pairHostSubmitButton
                            .id("pair-submit-cta")
                    }
                    .padding(OrbitSpacing.lg)
                }
                .scrollDismissesKeyboard(.interactively)
                .onChange(of: isUriFieldFocused) { focused in
                    if focused {
                        DispatchQueue.main.asyncAfter(deadline: .now() + 0.12) {
                            withAnimation(.easeOut(duration: 0.2)) {
                                proxy.scrollTo("pair-submit-cta", anchor: .bottom)
                            }
                        }
                    }
                }
            }
        }
        .background(palette.canvas.ignoresSafeArea())
        .onAppear {
            inlineErrorMessage = nil
            evaluateCameraAvailability()
        }
    }

    private var pairHostSubmitButton: some View {
        Button {
            submitManualUri()
        } label: {
            Text(OrbitL10n.tr("Pair Host"))
                .font(OrbitTypography.subhead(.semibold))
                .foregroundStyle(trimmedManualUri.isEmpty ? palette.fgSecondary : palette.onPrimary)
                .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                .background(
                    trimmedManualUri.isEmpty
                        ? palette.elevated
                        : palette.primary
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .strokeBorder(trimmedManualUri.isEmpty ? palette.hairline : Color.clear, lineWidth: 0.8)
                )
                .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(trimmedManualUri.isEmpty)
        .padding(.top, OrbitSpacing.xs)
    }

    private func handlePastedUriString(_ raw: String) {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            inlineErrorMessage = "No orbit://pair or orbit://sync-login URI found in clipboard."
            store.showToast("No orbit://pair or orbit://sync-login URI found in clipboard", isWarning: true)
            return
        }
        if let range = trimmed.range(of: "orbit://(pair|sync-login)[^\\s\"']+", options: [.regularExpression, .caseInsensitive]) {
            let extracted = String(trimmed[range])
            manualUri = extracted
            detectedClipboardPairUri = extracted
            inlineErrorMessage = nil
            store.hapticLight()
            return
        }
        manualUri = trimmed
        inlineErrorMessage = "Clipboard text does not start with orbit://pair or orbit://sync-login."
    }

    private func readValidOrRawClipboardUri() -> String? {
        if let verified = store.readPairingUriFromClipboard() {
            return verified
        }
        if let raw = UIPasteboard.general.string?.trimmingCharacters(in: .whitespacesAndNewlines),
           raw.lowercased().hasPrefix("orbit://") {
            return raw
        }
        return nil
    }

    private func submitManualUri() {
        let uri = trimmedManualUri
        guard !uri.isEmpty else { return }
        guard uri.lowercased().hasPrefix("orbit://pair") || uri.lowercased().hasPrefix("orbit://sync-login") else {
            inlineErrorMessage = "Enter a valid orbit://pair or orbit://sync-login URI."
            return
        }
        inlineErrorMessage = nil
        if store.handleScannedOrPastedUri(uri) {
            manualUri = ""
            dismiss()
        } else {
            inlineErrorMessage = store.lastError ?? "Could not pair with this URI. Check host reachability and secret."
        }
    }

    // MARK: - Header

    private var sheetHeader: some View {
        HStack {
            Text("Pair Host")
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)

            Spacer()

            Button {
                dismiss()
            } label: {
                Text("Cancel")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgSecondary)
                    .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .frame(height: 52)
    }

    // MARK: - 1-Tap GitHub E2EE Cloud Sync Banner

    private var cloudSyncQuickCard: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                Image(systemName: "icloud.and.arrow.down")
                    .font(OrbitTypography.icon)
                    .foregroundStyle(palette.accentSky)

                VStack(alignment: .leading, spacing: 2) {
                    Text("One-Scan Device Link / GitHub Sync")
                        .font(OrbitTypography.body(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                    Text("Scan a Device Link QR from Orbit Desktop — logs in, unlocks, and imports all machines + relays in 1 step.")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgSecondary)
                }
            }

            Button {
                dismiss()
                store.presentSheetSafely(.settings)
            } label: {
                Text("Open Cloud Sync in Settings")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                    .background(palette.subtleFill)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
    }

    // MARK: - 1-Tap Clipboard Pairing Banner

    private func clipboardPairBannerCard(_ uri: String) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                Image(systemName: "doc.on.clipboard.fill")
                    .font(OrbitTypography.icon)
                    .foregroundStyle(palette.primary)

                VStack(alignment: .leading, spacing: 2) {
                    Text("Pairing URI on Clipboard")
                        .font(OrbitTypography.body(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                    Text(uri)
                        .font(OrbitTypography.code(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }

            Button {
                if store.handleScannedOrPastedUri(uri) {
                    detectedClipboardPairUri = nil
                    dismiss()
                }
            } label: {
                HStack(spacing: OrbitSpacing.xs) {
                    Image(systemName: "link.badge.plus")
                        .font(OrbitTypography.iconSmall)
                    Text("Pair / Link from Clipboard")
                        .font(OrbitTypography.subhead(.semibold))
                }
                .foregroundStyle(palette.onPrimary)
                .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                .background(palette.primary)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Pair or Link from Clipboard")
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(palette.primary.opacity(0.4), lineWidth: 0.5)
        )
    }

    // MARK: - 1-Tap Local Simulator Host Discovery

    private var localSimulatorDiscoveryCard: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                Image(systemName: "bolt.horizontal.circle.fill")
                    .font(OrbitTypography.icon)
                    .foregroundStyle(palette.direct)

                VStack(alignment: .leading, spacing: 2) {
                    Text("Local Daemon Detected")
                        .font(OrbitTypography.body(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                    Text("An Orbit daemon is running on this Mac (127.0.0.1:6868).")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgSecondary)
                }
            }

            Button {
                if store.connectLocalSimulatorHost() {
                    dismiss()
                }
            } label: {
                Text("Connect Local Simulator Host")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.onPrimary)
                    .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                    .background(palette.primary)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(palette.primary.opacity(0.35), lineWidth: 0.5)
        )
    }

    // MARK: - Camera QR Scanner

    private var cameraScannerSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            Text("SCAN QR CODE")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)

            ZStack {
                switch cameraState {
                case .checking:
                    VStack(spacing: OrbitSpacing.sm) {
                        ProgressView()
                        Text("Preparing camera…")
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgSecondary)
                    }
                    .frame(maxWidth: .infinity)
                    .frame(height: 220)
                    .background(palette.code)

                case .ready:
                    ZStack {
                        QrScannerRepresentable(scanGeneration: scanGeneration) { scannedCode in
                            if store.handleScannedOrPastedUri(scannedCode) {
                                dismiss()
                            } else {
                                scanGeneration &+= 1
                            }
                        } onFailure: { reason in
                            cameraState = .unavailable(reason: reason)
                        }

                        // Subtle viewfinder reticle
                        RoundedRectangle(cornerRadius: 16, style: .continuous)
                            .strokeBorder(Color.white.opacity(0.65), lineWidth: 1.5)
                            .frame(width: 156, height: 156)
                    }
                    .frame(maxWidth: .infinity)
                    .frame(height: 240)

                case let .unavailable(reason):
                    VStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: "qrcode.viewfinder")
                            .font(OrbitTypography.title)
                            .foregroundStyle(palette.fgMuted)

                        Text(reason)
                            .orbitSubheadStyle()
                            .foregroundStyle(palette.fgSecondary)
                            .multilineTextAlignment(.center)
                            .padding(.horizontal, OrbitSpacing.lg)

                        Text("Run `orbit pair` or `orbit sync --export-device-link` on your machine and paste the URI below.")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .multilineTextAlignment(.center)
                            .padding(.horizontal, OrbitSpacing.lg)
                    }
                    .frame(maxWidth: .infinity)
                    .frame(height: 184)
                    .background(palette.code)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    // MARK: - Manual Pairing URI Input

    private var manualUriSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack {
                Text("PAIRING OR DEVICE LINK URI")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)

                Spacer()

                PasteButton(payloadType: String.self) { strings in
                    let raw = strings.first?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                    Task { @MainActor in
                        handlePastedUriString(raw)
                    }
                }
                .labelStyle(.titleAndIcon)
                .buttonBorderShape(.capsule)
                .tint(palette.primary)
                .accessibilityLabel("Paste Pairing or Link URI from Clipboard")
            }

            TextField("orbit://pair/… or orbit://sync-login?…", text: $manualUri)
                .font(OrbitTypography.code(.regular))
                .foregroundStyle(palette.fgPrimary)
                .focused($isUriFieldFocused)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .submitLabel(.go)
                .onSubmit {
                    submitManualUri()
                }
                .padding(.horizontal, OrbitSpacing.md)
                .frame(minHeight: OrbitSpacing.hitTarget)
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )
        }
    }

    // MARK: - Camera Permission & Simulator Detection

    private func evaluateCameraAvailability() {
        #if targetEnvironment(simulator)
        cameraState = .unavailable(reason: "Camera unavailable in iOS Simulator")
        #else
        guard AVCaptureDevice.default(for: .video) != nil else {
            cameraState = .unavailable(reason: "No camera hardware available")
            return
        }
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            cameraState = .ready
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .video) { granted in
                Task { @MainActor in
                    cameraState = granted
                        ? .ready
                        : .unavailable(reason: "Camera access was denied in Settings")
                }
            }
        case .denied, .restricted:
            cameraState = .unavailable(reason: "Camera access is disabled in iOS Settings")
        @unknown default:
            cameraState = .unavailable(reason: "Camera unavailable")
        }
        #endif
    }
}

// MARK: - Native AVCaptureSession QR Scanner Representable

private struct QrScannerRepresentable: UIViewControllerRepresentable {
    let scanGeneration: Int
    let onCodeScanned: (String) -> Void
    let onFailure: (String) -> Void

    func makeUIViewController(context: Context) -> QrScannerViewController {
        let controller = QrScannerViewController()
        controller.lastScanGeneration = scanGeneration
        controller.onCodeScanned = onCodeScanned
        controller.onFailure = onFailure
        return controller
    }

    func updateUIViewController(_ uiViewController: QrScannerViewController, context: Context) {
        uiViewController.onCodeScanned = onCodeScanned
        uiViewController.onFailure = onFailure
        uiViewController.resumeScanningIfNeeded(generation: scanGeneration)
    }
}

private final class QrScannerViewController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
    var onCodeScanned: ((String) -> Void)?
    var onFailure: ((String) -> Void)?
    var lastScanGeneration: Int = 0

    private let captureSession = AVCaptureSession()
    private let sessionQueue = DispatchQueue(label: "dev.orbit.ios.qrscanner", qos: .userInitiated)
    private var previewLayer: AVCaptureVideoPreviewLayer?
    private var hasDeliveredScan = false

    func resumeScanningIfNeeded(generation: Int) {
        guard generation != lastScanGeneration else { return }
        lastScanGeneration = generation
        hasDeliveredScan = false
        sessionQueue.async { [weak self] in
            guard let self else { return }
            if !self.captureSession.isRunning {
                self.captureSession.startRunning()
            }
        }
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black

        let layer = AVCaptureVideoPreviewLayer(session: captureSession)
        layer.videoGravity = .resizeAspectFill
        view.layer.addSublayer(layer)
        self.previewLayer = layer

        sessionQueue.async { [weak self] in
            self?.configureSession()
        }
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        previewLayer?.frame = view.bounds
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        sessionQueue.async { [weak self] in
            if self?.captureSession.isRunning == true {
                self?.captureSession.stopRunning()
            }
        }
    }

    private func configureSession() {
        guard let videoDevice = AVCaptureDevice.default(for: .video),
              let videoInput = try? AVCaptureDeviceInput(device: videoDevice),
              captureSession.canAddInput(videoInput) else {
            DispatchQueue.main.async { [weak self] in
                self?.onFailure?("Unable to initialize camera input")
            }
            return
        }

        captureSession.beginConfiguration()
        captureSession.addInput(videoInput)

        let metadataOutput = AVCaptureMetadataOutput()
        if captureSession.canAddOutput(metadataOutput) {
            captureSession.addOutput(metadataOutput)
            metadataOutput.setMetadataObjectsDelegate(self, queue: DispatchQueue.main)
            if metadataOutput.availableMetadataObjectTypes.contains(.qr) {
                metadataOutput.metadataObjectTypes = [.qr]
            }
        }
        captureSession.commitConfiguration()
        captureSession.startRunning()
    }

    func metadataOutput(
        _ output: AVCaptureMetadataOutput,
        didOutput metadataObjects: [AVMetadataObject],
        from connection: AVCaptureConnection
    ) {
        guard !hasDeliveredScan else { return }
        for obj in metadataObjects {
            if let readable = obj as? AVMetadataMachineReadableCodeObject,
               readable.type == .qr,
               let stringValue = readable.stringValue?.trimmingCharacters(in: .whitespacesAndNewlines),
               stringValue.lowercased().hasPrefix("orbit://") {
                hasDeliveredScan = true
                sessionQueue.async { [weak self] in
                    self?.captureSession.stopRunning()
                }
                onCodeScanned?(stringValue)
                break
            }
        }
    }
}
