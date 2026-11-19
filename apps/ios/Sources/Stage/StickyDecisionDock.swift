import SwiftUI

struct StickyDecisionDock: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.orbitPalette) private var palette
    @State private var focusedIndex: Int = 0

    private var pendingPermissions: [FfiTimelineItem] {
        let live = store.agentStage?.pendingPermissions ?? []
        return live.isEmpty ? store.demoPendingPermissions : live
    }

    private var pendingQuestions: [FfiTimelineItem] {
        let live = store.agentStage?.pendingQuestions ?? []
        return live.isEmpty ? store.demoPendingQuestions : live
    }

    private var combinedQueue: [FfiTimelineItem] {
        (pendingPermissions + pendingQuestions).sorted { lhs, rhs in
            itemSeq(lhs) < itemSeq(rhs)
        }
    }

    private func itemSeq(_ item: FfiTimelineItem) -> UInt64 {
        switch item {
        case let .permissionRequest(seq, _, _, _, _, _, _, _):
            return seq
        case let .questionRequest(seq, _, _, _, _):
            return seq
        default:
            return 0
        }
    }

    private var activeIndex: Int {
        guard !combinedQueue.isEmpty else { return 0 }
        return min(max(0, focusedIndex), combinedQueue.count - 1)
    }

    var body: some View {
        if !combinedQueue.isEmpty {
            let currentIdx = activeIndex
            let currentItem = combinedQueue[currentIdx]
            let totalCount = combinedQueue.count

            VStack(spacing: 0) {
                switch currentItem {
                case let .permissionRequest(_, requestId, title, commandOrPath, detail, allowedDecisions, _, _):
                    PermissionApprovalCard(
                        store: store,
                        requestId: requestId,
                        title: title,
                        commandOrPath: commandOrPath,
                        detail: detail,
                        allowedDecisions: allowedDecisions,
                        queuePosition: currentIdx + 1,
                        totalQueued: totalCount,
                        onStepQueue: { delta in
                            stepQueue(by: delta, total: totalCount)
                        }
                    )

                case let .questionRequest(_, requestId, questions, _, _):
                    QuestionDecisionCard(
                        store: store,
                        requestId: requestId,
                        questions: questions,
                        queuePosition: currentIdx + 1,
                        totalQueued: totalCount,
                        onStepQueue: { delta in
                            stepQueue(by: delta, total: totalCount)
                        }
                    )

                default:
                    EmptyView()
                }
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.top, OrbitSpacing.sm)
            .transition(.move(edge: .bottom).combined(with: .opacity))
            .onChange(of: combinedQueue.count) { newCount in
                if focusedIndex >= newCount {
                    focusedIndex = max(0, newCount - 1)
                }
            }
        }
    }

    private func stepQueue(by delta: Int, total: Int) {
        guard total > 1 else { return }
        store.hapticLight()
        withAnimation(.easeInOut(duration: 0.18)) {
            focusedIndex = (activeIndex + delta + total) % total
        }
    }
}

// MARK: - Queue Stepper Pill (`< 1 of N >`)

private struct DecisionQueueStepperPill: View {
    let position: Int
    let total: Int
    let accent: Color
    let onStep: (Int) -> Void

    @Environment(\.orbitPalette) private var palette

    var body: some View {
        HStack(spacing: 0) {
            Button {
                onStep(-1)
            } label: {
                Image(systemName: "chevron.left")
                    .font(OrbitTypography.iconMicro)
                    .foregroundStyle(accent)
                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Previous Pending Decision")

            Text("\(position) of \(total)")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(accent)

            Button {
                onStep(1)
            } label: {
                Image(systemName: "chevron.right")
                    .font(OrbitTypography.iconMicro)
                    .foregroundStyle(accent)
                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Next Pending Decision")
        }
        .frame(height: 28)
        .background(accent.opacity(0.12))
        .clipShape(Capsule())
        .overlay(
            Capsule()
                .strokeBorder(accent.opacity(0.28), lineWidth: 0.5)
        )
    }
}

// MARK: - 1-Tap Permission Approval Card

private struct PermissionApprovalCard: View {
    @ObservedObject var store: OrbitIosStore
    let requestId: String
    let title: String
    let commandOrPath: String
    let detail: FfiToolCallDetail
    let allowedDecisions: [String]
    let queuePosition: Int
    let totalQueued: Int
    let onStepQueue: (Int) -> Void

    @Environment(\.orbitPalette) private var palette

    private var decisions: [String] {
        allowedDecisions.isEmpty ? ["deny", "allow_always", "allow"] : allowedDecisions
    }

    private var targetPreview: String {
        switch detail {
        case let .shell(command, _, _, _):
            return "$ \(command)"
        case let .fileEdit(path, _), let .fileRead(path, _, _):
            return path
        default:
            return commandOrPath
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.xs) {
                Image(systemName: "exclamationmark.shield.fill")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(palette.warning)

                Text(title.isEmpty ? "Approval Required" : title)
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)

                Spacer(minLength: OrbitSpacing.xs)

                if totalQueued > 1 {
                    DecisionQueueStepperPill(
                        position: queuePosition,
                        total: totalQueued,
                        accent: palette.warning,
                        onStep: onStepQueue
                    )
                }
            }

            if !targetPreview.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    Text(targetPreview)
                        .orbitCodeStyle()
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(3)
                        .padding(.horizontal, OrbitSpacing.sm + 2)
                        .padding(.vertical, OrbitSpacing.sm)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(palette.code)
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )
            }

            HStack(spacing: OrbitSpacing.sm) {
                ForEach(decisions, id: \.self) { dec in
                    decisionButton(dec)
                }
            }
        }
        .padding(OrbitSpacing.md)
        .orbitGlassSurface(
            in: RoundedRectangle(cornerRadius: 18, style: .continuous),
            tint: palette.warning.opacity(0.08),
            fallbackFill: palette.elevated,
            fallbackBorder: palette.warning.opacity(0.4),
            fallbackShadowOpacity: 0.08
        )
    }

    private func decisionButton(_ decision: String) -> some View {
        let lower = decision.lowercased()
        let isDeny = lower.contains("deny") || lower.contains("reject")
        let isAlways = lower.contains("always") || lower.contains("session")
        let isPrimaryAllow = !isDeny && !isAlways

        return Button {
            store.resolvePermission(requestId: requestId, decision: decision)
        } label: {
            Text(OrbitFormatters.decisionLabel(decision))
                .font(OrbitTypography.subhead(.semibold))
                .foregroundStyle(
                    isDeny ? palette.danger :
                    isPrimaryAllow ? palette.onPrimary :
                    palette.fgPrimary
                )
                .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                .background(
                    isDeny ? palette.danger.opacity(0.12) :
                    isPrimaryAllow ? palette.primary :
                    palette.subtleFill
                )
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .strokeBorder(
                            isDeny ? palette.danger.opacity(0.3) :
                            isPrimaryAllow ? Color.clear :
                            palette.hairline,
                            lineWidth: 0.5
                        )
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Single / Multi-Select & Custom Answer Question Card (Unclipped Natural Height)

private struct QuestionDecisionCard: View {
    @ObservedObject var store: OrbitIosStore
    let requestId: String
    let questions: [FfiQuestionItem]
    let queuePosition: Int
    let totalQueued: Int
    let onStepQueue: (Int) -> Void

    @Environment(\.orbitPalette) private var palette
    @State private var selectedByQuestion: [String: Set<String>] = [:]
    @State private var customInputByQuestion: [String: String] = [:]

    private var totalOptionCount: Int {
        questions.reduce(0) { $0 + $1.options.count }
    }

    private var requiresScrolling: Bool {
        questions.count > 1 || totalOptionCount > 6
    }

    private var canSubmit: Bool {
        guard !questions.isEmpty else { return false }
        return questions.allSatisfy { q in
            let selected = selectedByQuestion[q.id] ?? []
            let custom = (customInputByQuestion[q.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            return !selected.isEmpty || !custom.isEmpty
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.md) {
            HStack(spacing: OrbitSpacing.xs) {
                Image(systemName: "questionmark.bubble.fill")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(palette.primary)

                Text("Input Requested")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)

                Spacer(minLength: OrbitSpacing.xs)

                if totalQueued > 1 {
                    DecisionQueueStepperPill(
                        position: queuePosition,
                        total: totalQueued,
                        accent: palette.primary,
                        onStep: onStepQueue
                    )
                }
            }

            if requiresScrolling {
                ScrollView(.vertical, showsIndicators: true) {
                    questionsListContent
                }
                .frame(maxHeight: 320)
            } else {
                questionsListContent
                    .fixedSize(horizontal: false, vertical: true)
            }

            Button {
                submitAnswers()
            } label: {
                Text("Submit Answer")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(canSubmit ? palette.onPrimary : palette.fgMuted)
                    .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                    .background(canSubmit ? palette.primary : palette.subtleFill)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(!canSubmit)
        }
        .padding(OrbitSpacing.md)
        .orbitGlassSurface(
            in: RoundedRectangle(cornerRadius: 18, style: .continuous),
            tint: palette.primary.opacity(0.06),
            fallbackFill: palette.elevated,
            fallbackBorder: palette.primary.opacity(0.35),
            fallbackShadowOpacity: 0.08
        )
        .onChange(of: requestId) { _ in
            selectedByQuestion = [:]
            customInputByQuestion = [:]
        }
    }

    private var questionsListContent: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.md) {
            ForEach(questions, id: \.id) { q in
                questionBlock(q)
            }
        }
    }

    private func questionBlock(_ q: FfiQuestionItem) -> some View {
        let selected = selectedByQuestion[q.id] ?? []

        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            if let header = q.header, !header.isEmpty {
                Text(header.uppercased())
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)
            }

            Text(q.question)
                .orbitBodyStyle(.medium)
                .foregroundStyle(palette.fgPrimary)
                .fixedSize(horizontal: false, vertical: true)

            if q.isMultiSelect {
                Text("Select all that apply")
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted)
            }

            if !q.options.isEmpty {
                VStack(spacing: OrbitSpacing.xs) {
                    ForEach(q.options, id: \.self) { option in
                        let isChosen = selected.contains(option)
                        Button {
                            toggleOption(question: q, option: option)
                        } label: {
                            HStack(spacing: OrbitSpacing.sm) {
                                Image(
                                    systemName: q.isMultiSelect
                                        ? (isChosen ? "checkmark.square.fill" : "square")
                                        : (isChosen ? "largecircle.fill.circle" : "circle")
                                )
                                .font(OrbitTypography.iconSmall)
                                .foregroundStyle(isChosen ? palette.primary : palette.fgMuted)

                                Text(option)
                                    .font(OrbitTypography.subhead(isChosen ? .medium : .regular))
                                    .foregroundStyle(palette.fgPrimary)
                                    .multilineTextAlignment(.leading)
                                    .fixedSize(horizontal: false, vertical: true)

                                Spacer(minLength: 0)
                            }
                            .padding(.horizontal, OrbitSpacing.md)
                            .padding(.vertical, OrbitSpacing.sm)
                            .frame(minHeight: OrbitSpacing.hitTarget)
                            .background(isChosen ? palette.activeRowFill : palette.subtleFill)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .strokeBorder(isChosen ? palette.primary.opacity(0.5) : palette.hairline, lineWidth: 0.5)
                            )
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            }

            HStack(spacing: OrbitSpacing.sm) {
                Image(systemName: "pencil")
                    .font(OrbitTypography.iconMicro)
                    .foregroundStyle(palette.fgMuted)

                TextField(
                    "Custom answer…",
                    text: Binding(
                        get: { customInputByQuestion[q.id] ?? "" },
                        set: { val in
                            customInputByQuestion[q.id] = val
                            if !q.isMultiSelect && !val.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                                selectedByQuestion[q.id] = []
                            }
                        }
                    )
                )
                .font(OrbitTypography.subhead(.regular))
                .foregroundStyle(palette.fgPrimary)
            }
            .padding(.horizontal, OrbitSpacing.md)
            .frame(minHeight: OrbitSpacing.hitTarget)
            .background(palette.code)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    private func toggleOption(question: FfiQuestionItem, option: String) {
        store.hapticLight()
        var current = selectedByQuestion[question.id] ?? []
        if question.isMultiSelect {
            if current.contains(option) {
                current.remove(option)
            } else {
                current.insert(option)
            }
        } else {
            if current.contains(option) {
                current.removeAll()
            } else {
                current = [option]
                customInputByQuestion[question.id] = ""
            }
        }
        selectedByQuestion[question.id] = current
    }

    private func submitAnswers() {
        let payload: [[String]] = questions.map { q in
            var list = Array(selectedByQuestion[q.id] ?? [])
            let custom = (customInputByQuestion[q.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            if !custom.isEmpty {
                list.append(custom)
            }
            return list
        }
        store.answerQuestion(requestId: requestId, answers: payload)
    }
}
