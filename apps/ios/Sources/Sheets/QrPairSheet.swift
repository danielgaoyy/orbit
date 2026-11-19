import AVFoundation
import SwiftUI
import UIKit

struct QrPairSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var manualUri = ""
    @State private var detectedClipboardPairUri: String?
    @State private var cameraState: QrCameraState = .checking

    enum QrCameraState: Equatable {
        case checking
        case ready
        case unavailable(reason: String)
    }

    var body: some View {
        VStack(spacing: 0) {
            sheetHeader
            OrbitHairline()

            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                    if let clipUri = detectedClipboardPairUri {
                        clipboardPairBannerCard(clipUri)
                    }

                    if store.localDaemonUri != nil {
                        localSimulatorDiscoveryCard
                    }

                    cameraScannerSection
                    manualUriSection
                }
                .padding(OrbitSpacing.lg)
            }
        }
        .background(palette.canvas.ignoresSafeArea())
        .onAppear {
            store.refreshPasteboardAvailability()
            detectedClipboardPairUri = store.detectValidPairUriInClipboard()
            evaluateCameraAvailability()
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
                if store.pairHost(uri: uri) {
                    detectedClipboardPairUri = nil
                    dismiss()
                }
            } label: {
                HStack(spacing: OrbitSpacing.xs) {
                    Image(systemName: "link.badge.plus")
                        .font(OrbitTypography.iconSmall)
                    Text("Pair from Clipboard")
                        .font(OrbitTypography.subhead(.semibold))
                }
                .foregroundStyle(Color.white)
                .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                .background(palette.primary)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Pair from Clipboard")
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
                    .foregroundStyle(Color.white)
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
                        QrScannerRepresentable { scannedCode in
                            if store.pairHost(uri: scannedCode) {
                                dismiss()
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

                        Text("Run `orbit pair` on your machine and paste the `orbit://pair/…` URI below.")
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
                Text("PAIRING URI")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)

                Spacer()

                Button {
                    if let clip = store.readPairingUriFromClipboard() {
                        manualUri = clip
                        store.hapticLight()
                    }
                } label: {
                    HStack(spacing: OrbitSpacing.xs) {
                        Image(systemName: "doc.on.clipboard")
                            .font(OrbitTypography.iconMicro)
                        Text("Paste")
                            .font(OrbitTypography.meta(.semibold))
                    }
                    .foregroundStyle(palette.primary)
                    .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget, alignment: .trailing)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Paste Pairing URI from Clipboard")
            }

            TextField("orbit://pair/…", text: $manualUri)
                .font(OrbitTypography.code(.regular))
                .foregroundStyle(palette.fgPrimary)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .padding(.horizontal, OrbitSpacing.md)
                .frame(minHeight: OrbitSpacing.hitTarget)
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )

            Button {
                if store.pairHost(uri: manualUri) {
                    manualUri = ""
                    dismiss()
                }
            } label: {
                Text("Pair Host")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(manualUri.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? palette.fgMuted : Color.white)
                    .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                    .background(
                        manualUri.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                            ? palette.subtleFill
                            : palette.primary
                    )
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(manualUri.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
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
    let onCodeScanned: (String) -> Void
    let onFailure: (String) -> Void

    func makeUIViewController(context: Context) -> QrScannerViewController {
        let controller = QrScannerViewController()
        controller.onCodeScanned = onCodeScanned
        controller.onFailure = onFailure
        return controller
    }

    func updateUIViewController(_ uiViewController: QrScannerViewController, context: Context) {}
}

private final class QrScannerViewController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
    var onCodeScanned: ((String) -> Void)?
    var onFailure: ((String) -> Void)?

    private let captureSession = AVCaptureSession()
    private let sessionQueue = DispatchQueue(label: "dev.orbit.ios.qrscanner", qos: .userInitiated)
    private var previewLayer: AVCaptureVideoPreviewLayer?
    private var hasDeliveredScan = false

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
