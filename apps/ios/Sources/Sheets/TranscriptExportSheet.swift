import SwiftUI

/// Native Transcript Export sheet powered by `exportAgentTranscript` + native `ShareLink`.
struct TranscriptExportSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var format: FfiExportFormat = .markdown
    @State private var includeThinking: Bool = false
    @State private var includeToolCalls: Bool = true
    @State private var includeTimestamps: Bool = true
    @State private var isExporting: Bool = false
    @State private var exportedURL: URL? = nil
    @State private var exportedInfo: FfiExportedTranscript? = nil
    @State private var exportError: String? = nil
    @State private var showCopiedBanner: Bool = false

    var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                    VStack(spacing: 0) {
                        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                            Text(OrbitL10n.tr("Format"))
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)

                            HStack(spacing: 4) {
                                formatChip(.markdown, label: "Markdown")
                                formatChip(.json, label: "JSON")
                                formatChip(.html, label: "HTML")
                            }
                            .padding(3)
                            .background(palette.recessed, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }
                        .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        Toggle(OrbitL10n.tr("Include Thinking"), isOn: $includeThinking)
                            .font(OrbitTypography.subhead(.regular))
                            .tint(palette.direct)
                            .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        Toggle(OrbitL10n.tr("Include Tool Calls"), isOn: $includeToolCalls)
                            .font(OrbitTypography.subhead(.regular))
                            .tint(palette.direct)
                            .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        Toggle(OrbitL10n.tr("Include Timestamps"), isOn: $includeTimestamps)
                            .font(OrbitTypography.subhead(.regular))
                            .tint(palette.direct)
                            .padding(OrbitSpacing.md)
                    }
                    .background(palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 14, style: .continuous)
                            .strokeBorder(palette.hairline, lineWidth: 0.5)
                    )

                    if let info = exportedInfo, let url = exportedURL {
                        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                            HStack {
                                Image(systemName: "doc.text.fill")
                                    .foregroundStyle(palette.direct)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(info.suggestedFilename)
                                        .font(OrbitTypography.code(.semibold))
                                        .foregroundStyle(palette.fgPrimary)
                                        .lineLimit(1)
                                    Text("\(info.turnCount) \(info.turnCount == 1 ? "turn" : "turns") · \(ByteCountFormatter.string(fromByteCount: Int64(info.byteLen), countStyle: .file))")
                                        .font(OrbitTypography.meta(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                }
                                Spacer()
                            }

                            ShareLink(item: url) {
                                HStack(spacing: 6) {
                                    Image(systemName: "square.and.arrow.up")
                                    Text(OrbitL10n.tr("Share Transcript"))
                                        .font(OrbitTypography.subhead(.semibold))
                                }
                                .foregroundStyle(palette.onPrimary)
                                .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                                .background(palette.primary, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                            }
                            .buttonStyle(.plain)
                            .contentShape(Rectangle())

                            Button {
                                UIPasteboard.general.string = info.content
                                store.hapticSuccess()
                                store.showToast(OrbitL10n.tr("Copied transcript to clipboard"))
                                withAnimation(.easeOut(duration: 0.16)) {
                                    showCopiedBanner = true
                                }
                                Task { @MainActor in
                                    try? await Task.sleep(nanoseconds: 2_000_000_000)
                                    withAnimation(.easeIn(duration: 0.2)) {
                                        showCopiedBanner = false
                                    }
                                }
                            } label: {
                                HStack(spacing: 6) {
                                    Image(systemName: showCopiedBanner ? "checkmark.circle.fill" : "square.on.square")
                                    Text(showCopiedBanner ? OrbitL10n.tr("Copied") : OrbitL10n.tr("Copy"))
                                        .font(OrbitTypography.subhead(.medium))
                                }
                                .foregroundStyle(showCopiedBanner ? palette.direct : palette.fgPrimary)
                                .frame(maxWidth: .infinity, minHeight: 40)
                                .background(
                                    showCopiedBanner ? palette.direct.opacity(0.12) : palette.subtleFill,
                                    in: RoundedRectangle(cornerRadius: 10, style: .continuous)
                                )
                            }
                            .buttonStyle(.plain)

                            if showCopiedBanner {
                                HStack(spacing: 6) {
                                    Image(systemName: "checkmark.circle.fill")
                                        .font(.system(size: 12))
                                        .foregroundStyle(palette.direct)
                                    Text(OrbitL10n.tr("Copied transcript to clipboard"))
                                        .font(OrbitTypography.meta(.medium))
                                        .foregroundStyle(palette.fgPrimary)
                                }
                                .padding(.horizontal, 10)
                                .padding(.vertical, 6)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(palette.direct.opacity(0.10), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                            }
                        }
                        .padding(OrbitSpacing.md)
                        .background(palette.elevated)
                        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 14, style: .continuous)
                                .strokeBorder(palette.hairline, lineWidth: 0.5)
                        )
                    }

                    if let err = exportError {
                        Text(err)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.danger)
                    }

                    Button {
                        runExport()
                    } label: {
                        HStack(spacing: 8) {
                            if isExporting {
                                ProgressView().tint(palette.onPrimary)
                            }
                            Text(exportedInfo == nil ? OrbitL10n.tr("Export Transcript") : OrbitL10n.tr("Refresh"))
                                .font(OrbitTypography.subhead(.semibold))
                        }
                        .foregroundStyle(palette.onPrimary)
                        .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                        .background(palette.primary, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }
                    .buttonStyle(.plain)
                    .disabled(isExporting || (store.selectedAgentId == nil && !store.hasDemoStageOverlay && !store.isDemoChatMode))
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.md)
            }
            .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
            .navigationTitle(OrbitL10n.tr("Export Transcript"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Done")) { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
            .onAppear {
                runExport()
            }
            .onChange(of: format) { _ in runExport() }
            .onChange(of: includeThinking) { _ in runExport() }
            .onChange(of: includeToolCalls) { _ in runExport() }
            .onChange(of: includeTimestamps) { _ in runExport() }
        }
    }

    private func formatChip(_ target: FfiExportFormat, label: String) -> some View {
        let selected = format == target
        return Button {
            store.hapticLight()
            format = target
        } label: {
            Text(label)
                .font(OrbitTypography.meta(selected ? .semibold : .medium))
                .foregroundStyle(selected ? palette.fgPrimary : palette.fgSecondary)
                .frame(maxWidth: .infinity, minHeight: 32)
                .background(selected ? palette.elevated : Color.clear, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        }
        .buttonStyle(.plain)
    }

    private func runExport() {
        if store.selectedAgentId == nil && (store.hasDemoStageOverlay || store.isDemoChatMode) {
            isExporting = true
            exportError = nil
            let demoText = buildDemoTranscriptExport()
            let ext = format == .json ? "json" : (format == .html ? "html" : "md")
            let filename = "orbit-demo-transcript.\(ext)"
            let tempDir = FileManager.default.temporaryDirectory.appendingPathComponent("orbit-exports", isDirectory: true)
            try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
            let url = tempDir.appendingPathComponent(filename)
            try? Data(demoText.utf8).write(to: url, options: [.atomic])
            exportedInfo = FfiExportedTranscript(
                suggestedFilename: filename,
                mimeType: format == .json ? "application/json" : (format == .html ? "text/html" : "text/markdown"),
                content: demoText,
                turnCount: UInt64(max(1, store.stageState.agentStage?.turns.count ?? 1)),
                byteLen: UInt64(demoText.utf8.count)
            )
            exportedURL = url
            isExporting = false
            return
        }
        guard store.selectedAgentId != nil else { return }
        isExporting = true
        exportError = nil
        Task {
            do {
                let result = try await store.exportActiveTranscript(
                    format: format,
                    includeThinking: includeThinking,
                    includeToolCalls: includeToolCalls,
                    includeTimestamps: includeTimestamps
                )
                exportedInfo = result.exported
                exportedURL = result.fileURL
                isExporting = false
            } catch {
                exportError = OrbitErrorFormatter.humanize(error)
                isExporting = false
            }
        }
    }

    private func buildDemoTranscriptExport() -> String {
        let turns = store.stageState.agentStage?.turns ?? []
        if format == .json {
            let items = turns.map { ["turn": $0.key] }
            let data = (try? JSONSerialization.data(withJSONObject: ["demo": true, "turns": items], options: [.prettyPrinted])) ?? Data("{}".utf8)
            return String(decoding: data, as: UTF8.self)
        }
        var lines: [String] = ["# Orbit Demo Transcript", ""]
        for turn in turns {
            if let prompt = turn.userPrompt,
               case let .userPrompt(_, _, text, _, _, _) = prompt {
                lines.append("## User\n\(text)\n")
            }
            if let reply = turn.terminalReply ?? turn.lastAssistantReply,
               case let .assistantMessage(_, _, text, _, _, _) = reply {
                lines.append("## Assistant\n\(text)\n")
            }
        }
        return lines.joined(separator: "\n")
    }
}
