import AVFoundation
import Foundation
import Speech
import SwiftUI

/// Native `SFSpeechRecognizer` + `AVAudioEngine` live dictation controller used by the
/// Codex-style single right action button in `OmniComposerView` and `GlobalOverviewView`.
@MainActor
public final class OrbitVoiceDictationController: ObservableObject {
    @Published public private(set) var isRecording: Bool = false
    @Published public private(set) var audioLevel: CGFloat = 0.0
    @Published public private(set) var elapsedSeconds: Int = 0
    @Published public private(set) var partialTranscript: String = ""
    @Published public private(set) var errorMessage: String? = nil

    private var audioEngine: AVAudioEngine?
    private var recognitionRequest: SFSpeechAudioBufferRecognitionRequest?
    private var recognitionTask: SFSpeechRecognitionTask?
    private var elapsedTimer: Timer?
    private var baseDraftBeforeRecording: String = ""
    private var onTranscriptChanged: ((String) -> Void)?

    public init() {}

    deinit {
        audioEngine?.stop()
        audioEngine?.inputNode.removeTap(onBus: 0)
        recognitionRequest?.endAudio()
        recognitionTask?.cancel()
        elapsedTimer?.invalidate()
    }

    public func toggleRecording(
        currentDraft: String,
        onTranscriptChanged: @escaping (String) -> Void,
        onPermissionDenied: @escaping (String) -> Void
    ) {
        if isRecording {
            stopRecording(discard: false)
        } else {
            startRecording(
                currentDraft: currentDraft,
                onTranscriptChanged: onTranscriptChanged,
                onPermissionDenied: onPermissionDenied
            )
        }
    }

    public func startRecording(
        currentDraft: String,
        onTranscriptChanged: @escaping (String) -> Void,
        onPermissionDenied: @escaping (String) -> Void
    ) {
        errorMessage = nil
        baseDraftBeforeRecording = currentDraft
        self.onTranscriptChanged = onTranscriptChanged

        let recognizer = SFSpeechRecognizer(locale: OrbitL10n.effectiveLocale) ?? SFSpeechRecognizer()
        guard let recognizer, recognizer.isAvailable else {
            let msg = OrbitL10n.tr("Speech recognition is unavailable on this device.")
            self.errorMessage = msg
            onPermissionDenied(msg)
            return
        }

        SFSpeechRecognizer.requestAuthorization { [weak self] authStatus in
            Task { @MainActor [weak self] in
                guard let self else { return }
                guard authStatus == .authorized else {
                    let msg = OrbitL10n.tr("Speech recognition permission is required for voice input.")
                    self.errorMessage = msg
                    onPermissionDenied(msg)
                    return
                }

                AVAudioSession.sharedInstance().requestRecordPermission { [weak self] micGranted in
                    Task { @MainActor [weak self] in
                        guard let self else { return }
                        guard micGranted else {
                            let msg = OrbitL10n.tr("Speech recognition permission is required for voice input.")
                            self.errorMessage = msg
                            onPermissionDenied(msg)
                            return
                        }
                        self.beginAudioCapture(onError: onPermissionDenied)
                    }
                }
            }
        }
    }

    public func stopRecording(discard: Bool = false) {
        let originalDraft = baseDraftBeforeRecording
        let callback = onTranscriptChanged

        elapsedTimer?.invalidate()
        elapsedTimer = nil

        audioEngine?.stop()
        audioEngine?.inputNode.removeTap(onBus: 0)
        audioEngine = nil

        recognitionRequest?.endAudio()
        recognitionRequest = nil

        recognitionTask?.cancel()
        recognitionTask = nil

        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)

        isRecording = false
        audioLevel = 0
        elapsedSeconds = 0
        partialTranscript = ""

        if discard {
            callback?(originalDraft)
        }
    }

    private func beginAudioCapture(onError: @escaping (String) -> Void) {
        let recognizer = SFSpeechRecognizer(locale: Locale.current) ?? SFSpeechRecognizer(locale: Locale(identifier: "en-US"))
        guard let recognizer, recognizer.isAvailable else {
            let msg = OrbitL10n.tr("Voice dictation is unavailable on this device.")
            errorMessage = msg
            onError(msg)
            return
        }

        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.record, mode: .measurement, options: .duckOthers)
            try session.setActive(true, options: .notifyOthersOnDeactivation)

            let request = SFSpeechAudioBufferRecognitionRequest()
            request.shouldReportPartialResults = true
            if #available(iOS 16.0, *) {
                request.addsPunctuation = true
            }

            let engine = AVAudioEngine()
            let inputNode = engine.inputNode
            let recordingFormat = inputNode.outputFormat(forBus: 0)
            guard recordingFormat.sampleRate > 0 && recordingFormat.channelCount > 0 else {
                let msg = OrbitL10n.tr("Voice dictation is unavailable on this device.")
                errorMessage = msg
                onError(msg)
                return
            }

            inputNode.removeTap(onBus: 0)
            inputNode.installTap(onBus: 0, bufferSize: 1024, format: recordingFormat) { [weak self] buffer, _ in
                request.append(buffer)
                let rms = Self.computeNormalizedLevel(buffer: buffer)
                Task { @MainActor [weak self] in
                    guard let self, self.isRecording else { return }
                    self.audioLevel = (self.audioLevel * 0.65) + (rms * 0.35)
                }
            }

            engine.prepare()
            try engine.start()

            self.audioEngine = engine
            self.recognitionRequest = request
            self.isRecording = true
            self.elapsedSeconds = 0
            self.partialTranscript = ""

            self.elapsedTimer?.invalidate()
            self.elapsedTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
                Task { @MainActor [weak self] in
                    guard let self, self.isRecording else { return }
                    self.elapsedSeconds += 1
                }
            }

            self.recognitionTask = recognizer.recognitionTask(with: request) { [weak self] result, error in
                Task { @MainActor [weak self] in
                    guard let self, self.isRecording else { return }
                    if let result {
                        let spoken = result.bestTranscription.formattedString
                        self.partialTranscript = spoken
                        switch OrbitVoiceCommandLogic.classifyTranscript(spoken) {
                        case .clearDraft:
                            self.onTranscriptChanged?("")
                        case let .appendText(cleaned):
                            let merged = OrbitVoiceCommandLogic.mergeDictatedText(
                                existingDraft: self.baseDraftBeforeRecording,
                                spokenSegment: cleaned
                            )
                            self.onTranscriptChanged?(merged)
                        case .focusOverview:
                            break
                        }
                        if result.isFinal {
                            self.stopRecording(discard: false)
                        }
                    } else if let error {
                        let hadTranscript = !self.partialTranscript.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        self.stopRecording(discard: false)
                        if !hadTranscript {
                            let msg = OrbitErrorFormatter.humanize(error)
                            self.errorMessage = msg
                            onError(msg)
                        }
                    }
                }
            }
        } catch {
            stopRecording(discard: false)
            let msg = error.localizedDescription
            errorMessage = msg
            onError(msg)
        }
    }

    private nonisolated static func computeNormalizedLevel(buffer: AVAudioPCMBuffer) -> CGFloat {
        guard let channelData = buffer.floatChannelData?[0] else { return 0 }
        let frameCount = Int(buffer.frameLength)
        guard frameCount > 0 else { return 0 }
        var sumSquares: Float = 0
        for i in 0..<frameCount {
            let sample = channelData[i]
            sumSquares += sample * sample
        }
        let rms = sqrt(sumSquares / Float(frameCount))
        let scaled = min(1.0, max(0.08, rms * 14.0))
        return CGFloat(scaled)
    }

    public var formattedDuration: String {
        String(format: "%d:%02d", elapsedSeconds / 60, elapsedSeconds % 60)
    }
}

/// Minimal inline recording strip displayed inside the composer ONLY while actively dictating.
public struct OrbitVoiceRecordingStrip: View {
    @ObservedObject var controller: OrbitVoiceDictationController
    let onCancel: () -> Void

    @Environment(\.orbitPalette) private var palette
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(controller: OrbitVoiceDictationController, onCancel: @escaping () -> Void) {
        self.controller = controller
        self.onCancel = onCancel
    }

    public var body: some View {
        HStack(spacing: OrbitSpacing.sm) {
            PulsingDotView(color: palette.danger, size: 7)

            Text(OrbitL10n.tr("Listening…"))
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgPrimary)

            // Restrained 5-bar monochrome audio level meter
            HStack(spacing: 2.5) {
                ForEach(0..<5, id: \.self) { idx in
                    let factor: CGFloat = [0.55, 0.85, 1.0, 0.78, 0.50][idx]
                    let level = reduceMotion ? 0.45 : max(0.18, min(1.0, controller.audioLevel * factor))
                    Capsule()
                        .fill(palette.fgSecondary)
                        .frame(width: 2.5, height: 4 + (level * 12))
                        .animation(reduceMotion ? nil : .easeOut(duration: 0.09), value: controller.audioLevel)
                }
            }
            .frame(height: 16)

            Text(controller.formattedDuration)
                .font(OrbitTypography.code(.regular))
                .foregroundStyle(palette.fgMuted)

            Spacer(minLength: 4)

            Button(action: onCancel) {
                Image(systemName: "xmark")
                    .font(OrbitTypography.iconMicro)
                    .foregroundStyle(palette.fgMuted)
                    .frame(width: 28, height: 28)
                    .background(palette.subtleFill, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(OrbitL10n.tr("Cancel"))
        }
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.vertical, 6)
        .background(palette.recessed)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.top, OrbitSpacing.sm)
    }
}
