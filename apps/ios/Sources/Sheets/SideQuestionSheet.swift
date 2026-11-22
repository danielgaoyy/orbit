import SwiftUI
import UIKit

/// Desktop `/btw` Side Question experience — non-blocking read-only side Q&A
/// with quick prompts, streaming Markdown rendering, Copy, and "Send to main chat".
struct SideQuestionSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    var body: some View {
        NavigationStack {
            SideQuestionPanel(
                store: store,
                onPromoteToComposer: { entry in
                    dismiss()
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                        store.promoteSideQuestionToComposer(entry)
                    }
                }
            )
            .navigationTitle("Side Question (/btw)")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if !store.activeSideQuestions.isEmpty {
                    ToolbarItem(placement: .topBarLeading) {
                        Button("Clear") {
                            store.clearSideQuestions()
                        }
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgSecondary)
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
        }
    }
}

struct SideQuestionPanel: View {
    @ObservedObject var store: OrbitIosStore
    var onPromoteToComposer: ((SideQuestionEntry) -> Void)? = nil

    @Environment(\.orbitPalette) private var palette
    @State private var draftInput: String = ""

    private let quickPrompts: [String] = [
        "Summarize the current task state and next steps",
        "Explain the latest error or failure concisely",
        "What edge cases or regressions should we watch for?",
        "Summarize the uncommitted git changes"
    ]

    private var entries: [SideQuestionEntry] {
        store.activeSideQuestions
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView(.vertical, showsIndicators: true) {
                    VStack(alignment: .leading, spacing: OrbitSpacing.md) {
                        if entries.isEmpty {
                            emptyStateView
                        } else {
                            ForEach(entries) { entry in
                                sideQuestionCard(entry)
                                    .id(entry.id)
                            }
                        }
                    }
                    .padding(.horizontal, OrbitSpacing.lg)
                    .padding(.vertical, OrbitSpacing.md)
                }
                .scrollDismissesKeyboard(.interactively)
                .onChange(of: entries.count) { _ in
                    if let last = entries.last {
                        withAnimation(.easeOut(duration: 0.2)) {
                            proxy.scrollTo(last.id, anchor: .bottom)
                        }
                    }
                }
            }

            OrbitHairline()
            inputBar
        }
        .background(palette.canvas.ignoresSafeArea())
        .onAppear {
            draftInput = store.sideQuestionDraft(for: store.activeSideQuestionScopeKey)
        }
        .onChange(of: draftInput) { val in
            store.setSideQuestionDraft(val, for: store.activeSideQuestionScopeKey)
        }
    }

    private var emptyStateView: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.md) {
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 8) {
                    Image(systemName: "bubble.left.and.text.bubble.right")
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(palette.accentSky)
                    Text("Ask a Side Question")
                        .font(OrbitTypography.title)
                        .foregroundStyle(palette.fgPrimary)
                }

                Text("Read-only side thread with full context of this session. Does not interrupt or pollute your main agent run.")
                    .orbitSubheadStyle()
                    .foregroundStyle(palette.fgSecondary)
            }
            .padding(.top, OrbitSpacing.md)

            VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                Text("Quick Prompts")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)

                ForEach(quickPrompts, id: \.self) { prompt in
                    Button {
                        store.submitSideQuestion(prompt)
                    } label: {
                        HStack(spacing: 8) {
                            Image(systemName: "sparkles")
                                .font(.system(size: 12, weight: .medium))
                                .foregroundStyle(palette.accentSky)
                            Text(prompt)
                                .font(OrbitTypography.subhead(.regular))
                                .foregroundStyle(palette.fgPrimary)
                                .multilineTextAlignment(.leading)
                            Spacer(minLength: 4)
                            Image(systemName: "arrow.up.right")
                                .font(.system(size: 11, weight: .semibold))
                                .foregroundStyle(palette.fgMuted)
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .padding(.vertical, 10)
                        .background(palette.elevated)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .strokeBorder(palette.hairline, lineWidth: 0.5)
                        )
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private func sideQuestionCard(_ entry: SideQuestionEntry) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(alignment: .top, spacing: 8) {
                Text("Q")
                    .font(OrbitTypography.meta(.bold))
                    .foregroundStyle(palette.accentSky)
                    .frame(width: 20, height: 20)
                    .background(palette.accentSky.opacity(0.14), in: Circle())

                Text(entry.question)
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .textSelection(.enabled)
            }

            OrbitHairline()

            if !entry.answer.isEmpty {
                let blocks = parseMarkdownPreview(markdown: entry.answer, streaming: entry.isStreaming)
                RichMarkdownView(
                    blocks: blocks,
                    fallbackText: entry.answer,
                    isStreaming: entry.isStreaming,
                    workspaceCwd: store.activeCwd
                )

                if entry.isStreaming {
                    HStack(spacing: 6) {
                        PulsingDotView(color: palette.accentSky, size: 5.5)
                        Text("Streaming…")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                    }
                }
            } else if entry.isStreaming && entry.error == nil {
                HStack(spacing: 8) {
                    OrbitSpinnerRingView(color: palette.accentSky, size: 12)
                    Text("Thinking…")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                }
                .padding(.vertical, 4)
            }

            if let err = entry.error, !err.isEmpty {
                HStack(alignment: .top, spacing: 6) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.system(size: 12))
                        .foregroundStyle(palette.danger)
                    Text(err)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.danger)
                }
            }

            if !entry.isStreaming && !entry.answer.isEmpty {
                HStack(spacing: OrbitSpacing.md) {
                    Button {
                        store.copyAssistantReply(entry.answer)
                    } label: {
                        Label("Copy", systemImage: "square.on.square")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgSecondary)
                    }
                    .buttonStyle(.plain)

                    Button {
                        if let onPromoteToComposer {
                            onPromoteToComposer(entry)
                        } else {
                            store.promoteSideQuestionToComposer(entry)
                        }
                    } label: {
                        Label("Send to Main Chat", systemImage: "arrow.uturn.left")
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.primary)
                    }
                    .buttonStyle(.plain)

                    Spacer()
                }
                .padding(.top, 2)
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

    private var inputBar: some View {
        let canSend = !draftInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        return HStack(spacing: OrbitSpacing.sm) {
            TextField("Ask a read-only side question…", text: $draftInput, axis: .vertical)
                .lineLimit(1...4)
                .font(OrbitTypography.body(.regular))
                .foregroundStyle(palette.fgPrimary)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(palette.subtleFill)
                .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))

            Button {
                let q = draftInput.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !q.isEmpty else { return }
                draftInput = ""
                store.submitSideQuestion(q)
            } label: {
                ZStack {
                    Circle()
                        .fill(canSend ? palette.primary : palette.subtleFill)
                        .frame(width: 34, height: 34)
                    Image(systemName: "arrow.up")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(canSend ? palette.onPrimary : palette.fgMuted)
                }
            }
            .buttonStyle(.plain)
            .disabled(!canSend)
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.vertical, OrbitSpacing.sm)
        .background(palette.canvas)
    }
}
