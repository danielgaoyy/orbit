import SwiftUI
import UIKit

private let liveStepAutoCollapseThreshold = 5
private let liveStepVisibleTailCount = 3
private let orbitDisclosureAnimation: Animation = .timingCurve(0.22, 1.0, 0.36, 1.0, duration: 0.24)

private struct VerticalRevealLayout: Layout {
    var progress: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard let content = subviews.first else { return .zero }
        let clamped = min(1, max(0, progress))
        let natural = content.sizeThatFits(ProposedViewSize(width: proposal.width, height: nil))
        let width = proposal.width ?? natural.width
        return CGSize(width: width, height: natural.height * clamped)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard let content = subviews.first else { return }
        let natural = content.sizeThatFits(ProposedViewSize(width: bounds.width, height: nil))
        content.place(
            at: CGPoint(x: bounds.minX, y: bounds.minY),
            anchor: .topLeading,
            proposal: ProposedViewSize(width: bounds.width, height: natural.height)
        )
    }
}

private struct VerticalRevealModifier: ViewModifier, Animatable {
    var progress: CGFloat

    var animatableData: CGFloat {
        get { progress }
        set { progress = newValue }
    }

    func body(content: Content) -> some View {
        let clamped = min(1, max(0, progress))
        let opacity = Double(min(1, max(0, clamped * 1.25)))
        VerticalRevealLayout(progress: clamped) {
            content
        }
        .clipped()
        .opacity(opacity)
    }
}

private extension AnyTransition {
    static var orbitVerticalDisclosure: AnyTransition {
        .modifier(
            active: VerticalRevealModifier(progress: 0),
            identity: VerticalRevealModifier(progress: 1)
        )
    }
}

struct StageTurnView: View {
    @ObservedObject var store: OrbitIosStore
    let turn: FfiStageTurn
    var isTurnActive: Bool = false
    var agentStatusLabel: String? = nil
    var hasLiveBackgroundTasks: Bool = false

    @Environment(\.orbitPalette) private var palette
    @State private var isFoldedStepsExpanded = false
    @State private var isRewindDialogPresented = false
    @State private var rewindTargetSeq: UInt64? = nil

    private var visibleLiveSegments: [FfiTurnLiveSegment] {
        var result: [FfiTurnLiveSegment] = []
        result.reserveCapacity(turn.liveSegments.count)
        for segment in turn.liveSegments {
            if case let .wakeup(_, label) = segment {
                let trimmed = label.trimmingCharacters(in: .whitespacesAndNewlines)
                if OrbitAgentLogic.isHiddenWakeupLabel(trimmed) {
                    continue
                }
                if case let .wakeup(_, prevLabel)? = result.last,
                   prevLabel.trimmingCharacters(in: .whitespacesAndNewlines).caseInsensitiveCompare(trimmed) == .orderedSame {
                    continue
                }
            }
            result.append(segment)
        }
        return result
    }

    private var isWaitingOnlyOnBackgroundTasks: Bool {
        guard isTurnActive else { return false }
        if hasLiveBackgroundTasks || OrbitAgentLogic.isBackgroundWaitingStatusLabel(agentStatusLabel) {
            return true
        }
        if case let .wakeup(_, label)? = turn.liveSegments.last,
           OrbitAgentLogic.isWaitingWakeupLabel(label) {
            return true
        }
        return false
    }

    private var trailingActivityLabel: String? {
        guard isTurnActive, !isWaitingOnlyOnBackgroundTasks else { return nil }
        let needsTrailing: Bool
        switch turn.liveSegments.last {
        case .none:
            needsTrailing = true
        case let .assistantMessage(_, item):
            if case let .assistantMessage(_, _, _, isMsgStreaming, _, _) = item {
                needsTrailing = !isMsgStreaming
            } else {
                needsTrailing = true
            }
        case .notice, .wakeup, .compaction:
            needsTrailing = true
        case .stepGroup:
            needsTrailing = false
        }
        guard needsTrailing else { return nil }
        let lowerStatus = (agentStatusLabel ?? "").trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if lowerStatus.contains("think") {
            return "Thinking…"
        }
        return "Working…"
    }

    private var isHostOnline: Bool {
        guard let host = store.activeHost else { return true }
        return OrbitFormatters.isHostOnline(host.connectionState)
    }

    private var isFirstStepBearingTurn: Bool {
        if let firstWithSteps = store.agentStage?.turns.first(where: { !$0.foldedSteps.isEmpty }) {
            return turn.key == firstWithSteps.key
        }
        return turn.turnIndex == 0 || turn.key == store.agentStage?.turns.first?.key
    }

    private var settledSummaryLabel: String {
        let label = turn.workSummaryLabel.trimmingCharacters(in: .whitespacesAndNewlines)
        if !label.isEmpty {
            return label
        }
        if let thought = OrbitFormatters.formatDurationMs(turn.thoughtDurationMs) {
            return "Thought for \(thought)"
        }
        return "Completed \(turn.foldedSteps.count) \(turn.foldedSteps.count == 1 ? "step" : "steps")"
    }

    private var canFoldPreReplyWork: Bool {
        turn.liveSegments.isEmpty && store.coalesceExploration && turn.terminalReply != nil && !turn.foldedSteps.isEmpty
    }

    private var terminalReplyText: String? {
        guard let replyItem = turn.terminalReply ?? turn.lastAssistantReply,
              case let .assistantMessage(_, _, text, _, blocks, _) = replyItem else {
            return nil
        }
        let resolved = resolvedAssistantText(text: text, blocks: blocks)
        return resolved.isEmpty ? nil : resolved
    }

    private var hasAssistantBody: Bool {
        !visibleLiveSegments.isEmpty || trailingActivityLabel != nil || turn.terminalReply != nil || !turn.notices.isEmpty
    }

    var body: some View {
        Group {
            if turn.isCompactionCommand && turn.liveSegments.isEmpty && turn.terminalReply == nil {
                compactionDivider(label: "Context compacted")
            } else {
                VStack(alignment: .leading, spacing: OrbitSpacing.md) {
                    if let promptItem = turn.userPrompt,
                       case let .userPrompt(promptSeq, _, text, badges, blocks, timestampMs) = promptItem {
                        UserPromptBubbleView(
                            text: text,
                            badges: badges,
                            blocks: blocks,
                            timestampMs: timestampMs,
                            onCopy: {
                                store.copyPromptText(text)
                            },
                            onRewind: !store.isActiveAgentTurnBusy ? {
                                rewindTargetSeq = promptSeq
                                isRewindDialogPresented = true
                            } : nil
                        )
                    }

                    if let agentMsg = turn.agentMessage,
                       case let .agentMessage(_, _, kind, sourceAgentId, sourceHostId, sourceTitle, event, text, permissionContext, blocks, timestampMs) = agentMsg {
                        AgentMessageCardView(
                            store: store,
                            kind: kind,
                            sourceAgentId: sourceAgentId,
                            sourceHostId: sourceHostId,
                            sourceTitle: sourceTitle,
                            event: event,
                            text: text,
                            permissionContext: permissionContext,
                            blocks: blocks,
                            timestampMs: timestampMs
                        )
                    }

                    if hasAssistantBody {
                        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                            if canFoldPreReplyWork {
                                settledStepsSection

                                ForEach(Array(turn.notices.enumerated()), id: \.offset) { _, noticeItem in
                                    if case let .notice(_, level, message, _) = noticeItem {
                                        TurnNoticeRow(level: level, message: message)
                                    }
                                }

                                if let replyItem = turn.terminalReply,
                                   case let .assistantMessage(_, _, text, _, blocks, _) = replyItem {
                                    assistantMarkdownBlock(text: text, blocks: blocks)
                                }
                            } else if !visibleLiveSegments.isEmpty || trailingActivityLabel != nil {
                                if !visibleLiveSegments.isEmpty {
                                    liveSegmentsSection
                                }
                                if let trailingLabel = trailingActivityLabel {
                                    InlineTrailingActivityView(
                                        label: trailingLabel,
                                        startedAtMs: turn.startedAtMs,
                                        isHostOnline: isHostOnline
                                    )
                                }
                            } else {
                                ForEach(Array(turn.notices.enumerated()), id: \.offset) { _, noticeItem in
                                    if case let .notice(_, level, message, _) = noticeItem {
                                        TurnNoticeRow(level: level, message: message)
                                    }
                                }

                                if let replyItem = turn.terminalReply,
                                   case let .assistantMessage(_, _, text, _, blocks, _) = replyItem {
                                    assistantMarkdownBlock(text: text, blocks: blocks)
                                }
                            }

                            if !isTurnActive && !turn.turnDiffs.isEmpty {
                                TurnChangesBarView(
                                    diffs: turn.turnDiffs,
                                    cwd: store.activeCwd,
                                    onOpenDiff: { path in
                                        store.hapticLight()
                                        store.openGitDiff(focusFilePath: path)
                                    }
                                )
                                .padding(.top, 2)
                            }

                            TurnFooterView(
                                isTurnActive: isTurnActive,
                                startedAtMs: turn.startedAtMs,
                                completedAtMs: turn.completedAtMs,
                                turnDurationMs: turn.turnDurationMs,
                                workDurationMs: turn.workDurationMs,
                                thoughtDurationMs: turn.thoughtDurationMs,
                                replyText: !isTurnActive ? terminalReplyText : nil,
                                onCopy: { reply in
                                    store.copyAssistantReply(reply)
                                },
                                onQuote: { reply in
                                    store.quoteAssistantReply(turnKey: turn.key, text: reply)
                                }
                            )
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .confirmationDialog(
                    "Rewind conversation to before this message?",
                    isPresented: $isRewindDialogPresented,
                    titleVisibility: .visible
                ) {
                    if let seq = rewindTargetSeq {
                        Button("Restore Conversation & Files", role: .destructive) {
                            store.rewindAgent(toBeforeSeq: seq, restoreFiles: true)
                            rewindTargetSeq = nil
                        }
                        Button("Rewind Conversation Only") {
                            store.rewindAgent(toBeforeSeq: seq, restoreFiles: false)
                            rewindTargetSeq = nil
                        }
                    }
                    Button("Cancel", role: .cancel) {
                        rewindTargetSeq = nil
                    }
                } message: {
                    Text("Messages after this turn will be removed and this prompt will return to your composer.")
                }
            }
        }
        .onAppear {
            if store.expandAllWorkGroupsByDefault {
                isFoldedStepsExpanded = true
            } else if store.expandFirstStepByDefault && isFirstStepBearingTurn {
                isFoldedStepsExpanded = true
            } else if store.expandFirstTurnByDefault,
                      (turn.turnIndex == 0 || turn.key == store.agentStage?.turns.first?.key || isFirstStepBearingTurn) {
                isFoldedStepsExpanded = true
            }
        }
        .onChange(of: store.expandAllWorkGroupsByDefault) { expandAll in
            if expandAll {
                isFoldedStepsExpanded = true
            }
        }
        .onChange(of: store.expandFirstStepByDefault) { expandFirstStep in
            if expandFirstStep && isFirstStepBearingTurn {
                isFoldedStepsExpanded = true
            }
        }
    }

    // MARK: - Assistant Markdown (Native Range Selection + Quote)

    @State private var expandedSegmentKeys: Set<String> = []

    @ViewBuilder
    private func assistantMarkdownBlock(text: String, blocks: [FfiRichBlock], streaming: Bool = false) -> some View {
        RichMarkdownView(
            blocks: blocks,
            fallbackText: text,
            isStreaming: streaming,
            workspaceCwd: store.activeCwd,
            onQuoteSelection: { selected in
                store.quoteAssistantReply(turnKey: turn.key, text: selected)
            },
            onOpenFileLink: { target in
                store.openWorkspaceFiles(initialPath: target.path, line: target.line)
            }
        )
    }

    // MARK: - Settled Folded Steps Header + Vertical Hairline Rail

    private var settledStepsSection: some View {
        VStack(alignment: .leading, spacing: 0) {
            StepSummaryDisclosureButton(
                label: settledSummaryLabel,
                unrecoveredFailureCount: turn.workCounts.unrecoveredFailureCount,
                labelIncludesFailure: turn.workSummaryIncludesFailure,
                isExpanded: isFoldedStepsExpanded,
                showInlineDiffStat: false,
                additions: turn.workCounts.additions,
                deletions: turn.workCounts.deletions,
                isLiveRunning: false
            ) {
                store.hapticLight()
                withAnimation(orbitDisclosureAnimation) {
                    isFoldedStepsExpanded.toggle()
                }
            }
            .zIndex(1)

            if isFoldedStepsExpanded {
                StepRailListView(
                    store: store,
                    items: turn.foldedSteps,
                    expandFirstToolStep: store.expandFirstStepByDefault && isFirstStepBearingTurn,
                    isLiveActive: false,
                    autoExpandErrors: turn.workCounts.failedCount > 0 || turn.workCounts.deniedCount > 0
                )
                .padding(.top, OrbitSpacing.xs)
                .transition(.orbitVerticalDisclosure)
            }
        }
    }

    // MARK: - Live / Multi-Segment Stream

    private var liveSegmentsSection: some View {
        let segments = visibleLiveSegments
        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            ForEach(Array(segments.enumerated()), id: \.element.segmentKey) { segIdx, segment in
                switch segment {
                case let .stepGroup(segKey, _, items, summary, thoughtDurationMs, workDurationMs, segStartedAtMs, segLatestActivityMs, hasFollowingContent, hasRunningStep):
                    let isLastVisible = segIdx == segments.count - 1
                    let isSegLiveActive = OrbitAgentLogic.isStepGroupLiveActive(
                        isTurnActive: isTurnActive,
                        hasFollowingContent: hasFollowingContent,
                        hasRunningStep: hasRunningStep,
                        isLastVisibleSegment: isLastVisible,
                        unrecoveredFailureCount: summary.counts.unrecoveredFailureCount
                    )
                    let keepOpenForTrailingFailure = !hasFollowingContent && summary.counts.unrecoveredFailureCount > 0
                    let shouldFold = store.coalesceExploration && !isSegLiveActive && !keepOpenForTrailingFailure
                    let isThoughtOnly = summary.counts.actionSteps == 0 && items.allSatisfy {
                        if case .assistantThinking = $0 { return true }
                        return false
                    }
                    let settledGroupLabel: String = {
                        if isThoughtOnly, let thought = OrbitFormatters.formatDurationMs(thoughtDurationMs) {
                            return "Thought for \(thought)"
                        }
                        let base = summary.label.trimmingCharacters(in: .whitespacesAndNewlines)
                        if !base.isEmpty {
                            return base
                        }
                        return "Completed \(items.count) \(items.count == 1 ? "step" : "steps")"
                    }()
                    let groupDurationMs: Int64? = {
                        if isThoughtOnly { return nil }
                        let total = max(0, workDurationMs ?? 0) + max(0, thoughtDurationMs ?? 0)
                        return total > 0 ? total : nil
                    }()

                    LiveStepGroupView(
                        store: store,
                        segmentKey: segKey,
                        items: items,
                        summary: summary,
                        settledLabel: settledGroupLabel,
                        settledDurationMs: groupDurationMs,
                        fallbackLiveLabel: turn.workSummaryLiveLabel,
                        fallbackLiveDetail: turn.workSummaryLiveDetail,
                        startedAtMs: segStartedAtMs ?? turn.startedAtMs,
                        latestActivityMs: segLatestActivityMs,
                        isHostOnline: isHostOnline,
                        shouldFold: shouldFold,
                        isLiveActive: isSegLiveActive,
                        showInlineDiffStat: isTurnActive,
                        isManualExpanded: Binding(
                            get: { expandedSegmentKeys.contains(segKey) },
                            set: { next in
                                if next {
                                    expandedSegmentKeys.insert(segKey)
                                } else {
                                    expandedSegmentKeys.remove(segKey)
                                }
                            }
                        )
                    )
                    .id(segKey)

                case let .assistantMessage(_, item):
                    if case let .assistantMessage(_, _, text, isMsgStreaming, blocks, _) = item {
                        assistantMarkdownBlock(text: text, blocks: blocks, streaming: isMsgStreaming)
                    }

                case let .notice(_, item):
                    if case let .notice(_, level, message, _) = item {
                        TurnNoticeRow(level: level, message: message)
                    }

                case let .compaction(_, label):
                    compactionDivider(label: label.isEmpty ? "Context compacted" : label)

                case let .wakeup(_, label):
                    compactionDivider(label: label.isEmpty ? "Resumed" : label, icon: "arrow.trianglehead.2.clockwise")
                }
            }
        }
    }

    // MARK: - Compaction Divider

    private func compactionDivider(label: String, icon: String = "scissors") -> some View {
        HStack(spacing: 10) {
            OrbitHairline()
            HStack(spacing: 5) {
                Image(systemName: icon)
                    .font(.system(size: 11, weight: .medium))
                    .opacity(0.75)
                Text(label.isEmpty ? "Context compacted" : label)
                    .font(OrbitTypography.meta(.regular))
            }
            .foregroundStyle(palette.fgMuted)
            .fixedSize()
            OrbitHairline()
        }
        .padding(.vertical, OrbitSpacing.sm)
    }

    // MARK: - Utilities

    private func resolvedAssistantText(text: String, blocks: [FfiRichBlock]) -> String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            return trimmed
        }
        var parts: [String] = []
        for block in blocks {
            switch block {
            case let .heading(_, spans),
                 let .paragraph(spans),
                 let .listItem(_, _, _, spans),
                 let .blockQuote(spans):
                let joined = spans.map(\.text).joined().trimmingCharacters(in: .whitespacesAndNewlines)
                if !joined.isEmpty {
                    parts.append(joined)
                }
            case let .codeBlock(_, rawCode, _):
                let cleanCode = rawCode.trimmingCharacters(in: .whitespacesAndNewlines)
                if !cleanCode.isEmpty {
                    parts.append(cleanCode)
                }
            case let .mathBlock(tex, _):
                let cleanTex = tex.trimmingCharacters(in: .whitespacesAndNewlines)
                if !cleanTex.isEmpty {
                    parts.append("$$\n\(cleanTex)\n$$")
                }
            case let .table(_, headers, rows):
                var tableLines: [String] = []
                let headerLine = headers
                    .map { $0.spans.map(\.text).joined().trimmingCharacters(in: .whitespacesAndNewlines) }
                    .joined(separator: " | ")
                if !headerLine.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    tableLines.append(headerLine)
                }
                for row in rows {
                    let rowLine = row.cells
                        .map { $0.spans.map(\.text).joined().trimmingCharacters(in: .whitespacesAndNewlines) }
                        .joined(separator: " | ")
                    if !rowLine.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        tableLines.append(rowLine)
                    }
                }
                if !tableLines.isEmpty {
                    parts.append(tableLines.joined(separator: "\n"))
                }
            case .horizontalRule:
                break
            }
        }
        return parts.joined(separator: "\n\n")
    }
}

private extension FfiTurnLiveSegment {
    var segmentKey: String {
        switch self {
        case let .stepGroup(key, _, _, _, _, _, _, _, _, _): return key
        case let .assistantMessage(key, _): return key
        case let .notice(key, _): return key
        case let .compaction(key, _): return key
        case let .wakeup(key, _): return key
        }
    }
}

// MARK: - Quiet Turn Footer (Settled Icon Actions + Timestamp — Hidden While Active to Match Desktop)

private struct TurnFooterView: View {
    let isTurnActive: Bool
    let startedAtMs: Int64?
    let completedAtMs: Int64?
    var turnDurationMs: Int64? = nil
    var workDurationMs: Int64? = nil
    var thoughtDurationMs: Int64? = nil
    let replyText: String?
    let onCopy: (String) -> Void
    var onQuote: ((String) -> Void)? = nil

    @Environment(\.orbitPalette) private var palette
    @State private var copied = false

    private var hasSettledContent: Bool {
        replyText != nil || completedAtMs != nil
    }

    private var compactDurationBadge: String? {
        if let total = turnDurationMs, total > 0 {
            return OrbitFormatters.formatDurationCompact(UInt64(total))
        }
        let work = max(0, workDurationMs ?? 0)
        let thought = max(0, thoughtDurationMs ?? 0)
        let sum = work + thought
        if sum > 0 {
            return OrbitFormatters.formatDurationCompact(UInt64(sum))
        }
        return nil
    }

    var body: some View {
        // Desktop parity (`TurnFooter` in `stream-stage.tsx`):
        // Never render a bottom `• MM:SS` pending footer while a turn is active;
        // live elapsed duration is shown inline inside the active step disclosure instead.
        if !isTurnActive && hasSettledContent {
            HStack(spacing: 4) {
                if let reply = replyText {
                    Button {
                        onCopy(reply)
                        withAnimation(.easeOut(duration: 0.15)) {
                            copied = true
                        }
                        Task { @MainActor in
                            try? await Task.sleep(nanoseconds: 1_500_000_000)
                            withAnimation(.easeIn(duration: 0.2)) {
                                copied = false
                            }
                        }
                    } label: {
                        Image(systemName: copied ? "checkmark" : "square.on.square")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(copied ? palette.direct : palette.fgSecondary.opacity(0.75))
                            .orbitSymbolReplaceTransition()
                            .frame(width: 28, height: 28)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(copied ? "Copied Reply" : "Copy Assistant Reply")

                    if let onQuote {
                        Button {
                            onQuote(reply)
                        } label: {
                            Image(systemName: "text.quote")
                                .font(.system(size: 12.5, weight: .medium))
                                .foregroundStyle(palette.fgSecondary.opacity(0.75))
                                .frame(width: 28, height: 28)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Quote Reply into Composer")
                    }
                }

                if let completedMs = completedAtMs {
                    RelativeTimestampText(timestampMs: completedMs)
                        .padding(.leading, replyText != nil ? 2 : 0)
                }

                if let dur = compactDurationBadge {
                    Text("· \(dur)")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                }

                Spacer(minLength: 0)
            }
            .padding(.leading, replyText != nil ? -6 : 0)
            .frame(minHeight: 28)
        }
    }
}

private struct InlineLiveElapsedText: View {
    let startedAtMs: Int64?
    var latestActivityMs: Int64? = nil
    var isHostOnline: Bool = true

    @Environment(\.orbitPalette) private var palette
    @State private var fallbackStartDate = Date()

    var body: some View {
        let startDate = startedAtMs.map { Date(timeIntervalSince1970: Double($0) / 1000.0) } ?? fallbackStartDate

        TimelineView(.periodic(from: .now, by: 1)) { context in
            let nowMs = Int64(context.date.timeIntervalSince1970 * 1000)
            let elapsedMs = max(0, Int64(context.date.timeIntervalSince(startDate) * 1000))
            let compact = elapsedMs >= 1000
                ? OrbitFormatters.formatDurationCompact(UInt64(elapsedMs))
                : nil
            let idleMs = latestActivityMs.map { max(0, nowMs - $0) } ?? 0

            HStack(spacing: 5) {
                if let compact {
                    Text("· \(compact)")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .monospacedDigit()
                }
                if !isHostOnline {
                    Text("· offline")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.warning)
                } else if idleMs >= 25_000 {
                    Text("· waiting for response")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted.opacity(0.85))
                }
            }
        }
    }
}

private struct InlineTrailingActivityView: View {
    let label: String
    let startedAtMs: Int64?
    var isHostOnline: Bool = true

    @Environment(\.orbitPalette) private var palette

    var body: some View {
        HStack(spacing: 6) {
            PulsingDotView(color: palette.primary, size: 5.5)
            Text(label)
                .font(OrbitTypography.subhead(.regular))
                .foregroundStyle(palette.fgSecondary)
                .lineLimit(1)
            InlineLiveElapsedText(
                startedAtMs: startedAtMs,
                latestActivityMs: nil,
                isHostOnline: isHostOnline
            )
            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
    }
}

private struct RelativeTimestampText: View {
    let timestampMs: Int64

    @Environment(\.orbitPalette) private var palette

    private var needsLiveRefresh: Bool {
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        return (nowMs - timestampMs) < 3_600_000
    }

    var body: some View {
        if needsLiveRefresh {
            TimelineView(.periodic(from: .now, by: 30)) { context in
                let nowMs = Int64(context.date.timeIntervalSince1970 * 1000)
                Text(OrbitFormatters.relativeTimeLabel(timestampMs: timestampMs, nowMs: nowMs))
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgSecondary.opacity(0.78))
            }
        } else {
            Text(OrbitFormatters.relativeTimeLabel(timestampMs: timestampMs))
                .font(OrbitTypography.meta(.regular))
                .foregroundStyle(palette.fgSecondary.opacity(0.78))
        }
    }
}

// MARK: - Turn-End Aggregated Diff Pill (`Edited foo.rs +12 -4 Review ›` / `N files changed`)

private struct TurnChangesBarView: View {
    let diffs: [FfiTurnFileDiffSummary]
    let cwd: String
    let onOpenDiff: (String?) -> Void

    @Environment(\.orbitPalette) private var palette

    var body: some View {
        let primaryPath = diffs.first?.path
        let totalAdd = diffs.reduce(UInt32(0)) { $0 + $1.additions }
        let totalDel = diffs.reduce(UInt32(0)) { $0 + $1.deletions }

        let iconName: String = {
            if diffs.count == 1, let p = primaryPath {
                return OrbitFormatters.semanticFileIcon(for: OrbitFormatters.shortPath(p))
            }
            return "chevron.left.forwardslash.chevron.right"
        }()

        let titleLabel: String = {
            if diffs.count == 1, let p = primaryPath {
                return "Edited \(OrbitFormatters.stripWorkspacePrefix(p, cwd: cwd))"
            }
            return "\(diffs.count) files changed"
        }()

        Button {
            onOpenDiff(primaryPath)
        } label: {
            HStack(spacing: 8) {
                Image(systemName: iconName)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(palette.fgMuted)

                Text(titleLabel)
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)
                    .truncationMode(.middle)

                if totalAdd > 0 || totalDel > 0 {
                    HStack(spacing: 4) {
                        if totalAdd > 0 {
                            Text("+\(totalAdd)")
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.added)
                        }
                        if totalDel > 0 {
                            Text("-\(totalDel)")
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.removed)
                        }
                    }
                }

                HStack(spacing: 3) {
                    Text("Review")
                        .font(OrbitTypography.meta(.regular))
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9.5, weight: .semibold))
                }
                .foregroundStyle(palette.fgMuted)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(palette.isDark ? palette.elevated : palette.canvas)
            .clipShape(Capsule())
            .overlay(
                Capsule()
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(titleLabel). Review changes")
    }
}

// MARK: - User Prompt Bubble (Soft Surface, Inline Slash Prefix, Wrapping Chips, Fade Collapse)

private struct PromptPresentation {
    let cleanText: String
    let badges: [FfiPromptBadge]
    let sanitizedLegacy: Bool
}

private let legacyPlanPrefix =
    "Analyze the relevant codebase and propose a concise step-by-step plan before making edits:"
private let legacyReviewPrompt =
    "Review the uncommitted working tree changes for bugs, edge cases, and architectural consistency."

private func extractUserPromptPresentation(
    text: String,
    badges: [FfiPromptBadge]
) -> PromptPresentation {
    if !badges.isEmpty {
        return PromptPresentation(
            cleanText: text.trimmingCharacters(in: .whitespacesAndNewlines),
            badges: badges,
            sanitizedLegacy: false
        )
    }

    var working = text
    var inferred: [FfiPromptBadge] = []
    var sanitized = false

    func stripRegex(_ pattern: String, options: NSRegularExpression.Options = [.anchorsMatchLines], handler: ([String]) -> Void) {
        guard let regex = try? NSRegularExpression(pattern: pattern, options: options) else { return }
        let full = NSRange(working.startIndex..<working.endIndex, in: working)
        let matches = regex.matches(in: working, options: [], range: full)
        guard !matches.isEmpty else { return }
        sanitized = true
        for match in matches {
            var groups: [String] = []
            for idx in 1..<match.numberOfRanges {
                if let r = Range(match.range(at: idx), in: working) {
                    groups.append(String(working[r]))
                }
            }
            handler(groups)
        }
        for match in matches.reversed() {
            if let r = Range(match.range, in: working) {
                working.removeSubrange(r)
            }
        }
    }

    stripRegex(#"^Attached image file:\s*`([^`]+)`\s*$"#) { groups in
        if let raw = groups.first {
            let name = raw.split(separator: "/").last.map(String.init) ?? raw
            inferred.append(FfiPromptBadge(kind: "image", label: name, path: raw))
        }
    }

    stripRegex(#"^Referenced workspace files:\s*(.+)$"#) { groups in
        if let listStr = groups.first,
           let itemRegex = try? NSRegularExpression(pattern: #"`@?([^`]+)`"#) {
            let range = NSRange(listStr.startIndex..<listStr.endIndex, in: listStr)
            for match in itemRegex.matches(in: listStr, range: range) {
                if let r = Range(match.range(at: 1), in: listStr) {
                    let raw = String(listStr[r])
                    let name = raw.split(separator: "/").last.map(String.init) ?? raw
                    inferred.append(FfiPromptBadge(kind: "file", label: name, path: raw))
                }
            }
        }
    }

    stripRegex(#"^>\s*(Working Tree Diff|Staged Diff):\r?\n```diff\r?\n[\s\S]*?\r?\n```\s*$"#) { groups in
        if let label = groups.first {
            inferred.append(FfiPromptBadge(kind: "git_diff", label: label, path: nil))
        }
    }

    stripRegex(#"^>\s*Context:\s*Inspect (staged|working tree) git changes\.\s*$"#, options: [.anchorsMatchLines, .caseInsensitive]) { groups in
        let isStaged = groups.first?.lowercased() == "staged"
        inferred.append(FfiPromptBadge(kind: "git_diff", label: isStaged ? "Staged Diff" : "Working Tree Diff", path: nil))
    }

    stripRegex(#"^>\s*([^:\r\n]*lines):\r?\n```text\r?\n[\s\S]*?\r?\n```\s*$"#) { groups in
        if let label = groups.first {
            inferred.append(FfiPromptBadge(kind: "terminal", label: label.trimmingCharacters(in: .whitespaces), path: nil))
        }
    }

    stripRegex(#"^>\s*Reference\s*`([^`]+)`:\r?\n```[\s\S]*?\r?\n```\s*$"#) { groups in
        if let loc = groups.first {
            let shortLoc = loc.split(separator: "/").last.map(String.init) ?? loc
            inferred.append(FfiPromptBadge(kind: "quote", label: shortLoc, path: loc))
        }
    }

    working = working.trimmingCharacters(in: .whitespacesAndNewlines)

    if working.hasPrefix(legacyPlanPrefix) {
        sanitized = true
        inferred.insert(FfiPromptBadge(kind: "slash", label: "/plan", path: nil), at: 0)
        working = String(working.dropFirst(legacyPlanPrefix.count)).trimmingCharacters(in: .whitespacesAndNewlines)
    } else if working == legacyReviewPrompt {
        sanitized = true
        inferred.insert(FfiPromptBadge(kind: "slash", label: "/review", path: nil), at: 0)
        working = ""
    }

    return PromptPresentation(cleanText: working, badges: inferred, sanitizedLegacy: sanitized)
}

private struct WrappingHStackLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = proposal.width ?? .infinity
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0
        var usedWidth: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(ProposedViewSize(width: maxWidth, height: nil))
            if x > 0 && x + size.width > maxWidth {
                usedWidth = max(usedWidth, max(0, x - spacing))
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
        usedWidth = max(usedWidth, max(0, x - spacing))
        return CGSize(width: min(usedWidth, maxWidth), height: y + rowHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        var y = bounds.minY
        var rowHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(ProposedViewSize(width: bounds.width, height: nil))
            if x > bounds.minX && x + size.width > bounds.maxX {
                x = bounds.minX
                y += rowHeight + spacing
                rowHeight = 0
            }
            subview.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
    }
}

struct UserPromptBubbleView: View {
    let text: String
    let badges: [FfiPromptBadge]
    var blocks: [FfiRichBlock] = []
    var timestampMs: Int64? = nil
    var onCopy: (() -> Void)? = nil
    var onRewind: (() -> Void)? = nil

    @Environment(\.orbitPalette) private var palette
    @State private var isExpanded = false

    private var presentation: PromptPresentation {
        extractUserPromptPresentation(text: text, badges: badges)
    }

    private var slashBadge: FfiPromptBadge? {
        presentation.badges.first(where: { $0.kind == "slash" })
    }

    private var attachmentBadges: [FfiPromptBadge] {
        presentation.badges.filter { $0.kind != "slash" }
    }

    private var shouldCollapse: Bool {
        let clean = presentation.cleanText
        let lineCount = clean.components(separatedBy: .newlines).count
        return lineCount > 8 || clean.count > 420
    }

    private var bubbleFill: Color {
        palette.elevated
    }

    private var hasRichMarkdownBlocks: Bool {
        guard !presentation.sanitizedLegacy && !blocks.isEmpty else { return false }
        if blocks.count > 1 { return true }
        guard let first = blocks.first else { return false }
        switch first {
        case let .paragraph(spans):
            return spans.contains { $0.bold || $0.italic || $0.strikethrough || $0.inlineCode || $0.linkUrl != nil }
        case .heading, .listItem, .blockQuote, .codeBlock, .table, .horizontalRule, .mathBlock:
            return true
        }
    }

    var body: some View {
        HStack {
            Spacer(minLength: 52)

            VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
                if !attachmentBadges.isEmpty {
                    WrappingHStackLayout(spacing: 6) {
                        ForEach(Array(attachmentBadges.enumerated()), id: \.offset) { _, badge in
                            attachmentChip(badge)
                        }
                    }
                }

                promptBodyView
                    .overlay(alignment: .bottom) {
                        if shouldCollapse && !isExpanded {
                            LinearGradient(
                                colors: [bubbleFill.opacity(0), bubbleFill],
                                startPoint: .top,
                                endPoint: .bottom
                            )
                            .frame(height: 36)
                            .allowsHitTesting(false)
                        }
                    }

                if shouldCollapse {
                    Button {
                        withAnimation(.easeInOut(duration: 0.2)) {
                            isExpanded.toggle()
                        }
                    } label: {
                        Text(isExpanded ? "Show less" : "Show more")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgSecondary)
                            .padding(.top, 2)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8.5)
            .background {
                let shape = RoundedRectangle(cornerRadius: 20, style: .continuous)
                shape
                    .fill(bubbleFill)
                    .overlay {
                        if palette.isDark {
                            shape.strokeBorder(
                                LinearGradient(
                                    colors: [Color.white.opacity(0.055), .clear, .clear],
                                    startPoint: .top,
                                    endPoint: .bottom
                                ),
                                lineWidth: 0.5
                            )
                        }
                    }
                    .overlay(
                        shape.strokeBorder(
                            palette.isDark ? Color.white.opacity(0.09) : Color.black.opacity(0.075),
                            lineWidth: 0.5
                        )
                    )
                    .shadow(
                        color: Color.black.opacity(palette.isDark ? 0.10 : 0.028),
                        radius: 3,
                        x: 0,
                        y: 1
                    )
            }
            .contextMenu {
                if let onCopy {
                    Button {
                        onCopy()
                    } label: {
                        Label("Copy Prompt", systemImage: "square.on.square")
                    }
                }
                if let onRewind {
                    Button {
                        onRewind()
                    } label: {
                        Label("Rewind to Here", systemImage: "arrow.uturn.backward")
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var promptBodyView: some View {
        let clean = presentation.cleanText
        if slashBadge != nil {
            OrbitSelectableTextView(
                attributedText: buildUserPromptAttributedString(),
                stretchesToProposedWidth: false,
                maxLines: shouldCollapse && !isExpanded ? 8 : 0,
                isDarkAppearance: palette.isDark,
                linkColor: UIColor(palette.primary)
            )
        } else if !clean.isEmpty {
            if hasRichMarkdownBlocks && (!shouldCollapse || isExpanded) {
                RichMarkdownView(blocks: blocks, fallbackText: clean)
            } else {
                OrbitSelectableTextView(
                    attributedText: buildUserPromptAttributedString(),
                    stretchesToProposedWidth: false,
                    maxLines: shouldCollapse && !isExpanded ? 8 : 0,
                    isDarkAppearance: palette.isDark,
                    linkColor: UIColor(palette.primary)
                )
            }
        }
    }

    private func buildUserPromptAttributedString() -> NSAttributedString {
        let clean = presentation.cleanText
        let uiPrimary = UIColor(palette.fgPrimary)
        let uiSecondary = UIColor(palette.fgSecondary)
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = OrbitTypography.bodyLineSpacing

        let result = NSMutableAttributedString()
        if let slash = slashBadge {
            let slashIcon = badgeIcon(for: slash)
            let slashLabel = slash.label.hasPrefix("/") ? String(slash.label.dropFirst()) : slash.label
            let config = UIImage.SymbolConfiguration(pointSize: 13.5, weight: .medium)
            if let sym = UIImage(systemName: slashIcon, withConfiguration: config)?
                .withTintColor(uiSecondary, renderingMode: .alwaysOriginal) {
                let attachment = NSTextAttachment()
                attachment.image = sym
                let bodyFont = UIFont.systemFont(ofSize: 15, weight: .regular)
                let yOffset = (bodyFont.capHeight - sym.size.height) / 2.0
                attachment.bounds = CGRect(x: 0, y: yOffset, width: sym.size.width, height: sym.size.height)
                result.append(NSAttributedString(attachment: attachment))
            }
            result.append(NSAttributedString(
                string: " \(slashLabel)",
                attributes: [
                    .font: UIFont.systemFont(ofSize: 15, weight: .medium),
                    .foregroundColor: uiPrimary,
                    .paragraphStyle: paragraph
                ]
            ))
            if !clean.isEmpty {
                result.append(NSAttributedString(
                    string: " \(clean)",
                    attributes: [
                        .font: UIFont.systemFont(ofSize: 15, weight: .regular),
                        .foregroundColor: uiPrimary,
                        .paragraphStyle: paragraph
                    ]
                ))
            }
        } else {
            result.append(NSAttributedString(
                string: clean,
                attributes: [
                    .font: UIFont.systemFont(ofSize: 15, weight: .regular),
                    .foregroundColor: uiPrimary,
                    .paragraphStyle: paragraph
                ]
            ))
        }
        return result
    }

    private func attachmentChip(_ badge: FfiPromptBadge) -> some View {
        let isMono = badge.kind == "file" || badge.kind == "quote"
        return HStack(spacing: 5) {
            Image(systemName: badgeIcon(for: badge))
                .font(.system(size: 11.5, weight: .medium))
                .foregroundStyle(palette.fgMuted)
            Text(badge.label)
                .font(isMono ? OrbitTypography.code(.medium) : OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgSecondary)
                .lineLimit(1)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(palette.isDark ? Color.white.opacity(0.06) : Color.black.opacity(0.04))
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
    }

    private func badgeIcon(for badge: FfiPromptBadge) -> String {
        switch badge.kind {
        case "slash":
            switch badge.label {
            case "/plan": return "checklist"
            case "/review": return "doc.text.magnifyingglass"
            case "/compact": return "rectangle.compress.vertical"
            default: return "sparkles"
            }
        case "file":
            return OrbitFormatters.semanticFileIcon(for: badge.label)
        case "git_diff":
            return badge.label.localizedCaseInsensitiveContains("staged") ? "arrow.left.arrow.right" : "doc.badge.plus"
        case "terminal":
            return "terminal"
        case "quote":
            return "text.quote"
        case "image":
            return "photo"
        case "paste":
            let prefix = badge.label.components(separatedBy: "·").first?.trimmingCharacters(in: .whitespaces) ?? ""
            return prefix.contains(".") ? OrbitFormatters.semanticFileIcon(for: prefix) : "doc.text"
        default:
            return "doc.text"
        }
    }
}

// MARK: - Step Group Disclosure & Live Segments

private struct StepSummaryDisclosureButton: View {
    let label: String
    var durationBadge: String? = nil
    let unrecoveredFailureCount: UInt32
    let labelIncludesFailure: Bool
    let isExpanded: Bool
    let showInlineDiffStat: Bool
    let additions: UInt32
    let deletions: UInt32
    let isLiveRunning: Bool
    let action: () -> Void

    @Environment(\.orbitPalette) private var palette

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if isLiveRunning {
                    PulsingDotView(color: palette.primary, size: 5.5)
                }

                Text(label)
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(labelIncludesFailure ? palette.danger : palette.fgSecondary)
                    .lineLimit(1)

                if unrecoveredFailureCount > 0 && !labelIncludesFailure {
                    Text("· \(unrecoveredFailureCount) failed")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.danger)
                }

                if let dur = durationBadge, !dur.isEmpty {
                    Text("· \(dur)")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                }

                if showInlineDiffStat && (additions > 0 || deletions > 0) {
                    HStack(spacing: 4) {
                        if additions > 0 {
                            Text("+\(additions)")
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.added)
                        }
                        if deletions > 0 {
                            Text("-\(deletions)")
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.removed)
                        }
                    }
                }

                Image(systemName: "chevron.right")
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(palette.fgMuted.opacity(0.75))
                    .rotationEffect(.degrees(isExpanded ? 90 : 0))

                Spacer(minLength: 0)
            }
            .padding(.vertical, 5)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

private struct LiveStepGroupView: View {
    @ObservedObject var store: OrbitIosStore
    let segmentKey: String
    let items: [FfiTimelineItem]
    let summary: FfiWorkSummaryResult
    var settledLabel: String? = nil
    var settledDurationMs: Int64? = nil
    let fallbackLiveLabel: String
    var fallbackLiveDetail: String? = nil
    var startedAtMs: Int64? = nil
    var latestActivityMs: Int64? = nil
    var isHostOnline: Bool = true
    let shouldFold: Bool
    let isLiveActive: Bool
    let showInlineDiffStat: Bool
    @Binding var isManualExpanded: Bool

    @Environment(\.orbitPalette) private var palette
    @State private var isLiveCollapsed = false

    private var liveTitleText: String {
        if !summary.liveLabel.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return summary.liveLabel
        }
        if !fallbackLiveLabel.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return fallbackLiveLabel
        }
        return "Working…"
    }

    private var liveDetailText: String? {
        let raw = (summary.liveDetail ?? fallbackLiveDetail)?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let raw, !raw.isEmpty else { return nil }
        return raw
    }

    private var resolvedSettledLabel: String {
        if let settled = settledLabel?.trimmingCharacters(in: .whitespacesAndNewlines), !settled.isEmpty {
            return settled
        }
        let base = summary.label.trimmingCharacters(in: .whitespacesAndNewlines)
        return base.isEmpty ? "Completed \(items.count) \(items.count == 1 ? "step" : "steps")" : base
    }

    private var settledDurationBadge: String? {
        guard let ms = settledDurationMs, ms >= 1000 else { return nil }
        return OrbitFormatters.formatDurationCompact(UInt64(ms))
    }

    private var forceExpanded: Bool {
        store.expandAllWorkGroupsByDefault || store.expandFirstStepByDefault
    }

    private var isFoldOpen: Bool {
        forceExpanded || isManualExpanded
    }

    var body: some View {
        if isLiveActive {
            // Desktop parity (`TurnActivityDisclosure`): compact inline live status + elapsed duration header
            // directly above the active step rail (replacing the removed bottom `• MM:SS` turn footer).
            VStack(alignment: .leading, spacing: 0) {
                Button {
                    store.hapticLight()
                    withAnimation(orbitDisclosureAnimation) {
                        isLiveCollapsed.toggle()
                    }
                } label: {
                    HStack(spacing: 6) {
                        PulsingDotView(color: palette.primary, size: 5.5)

                        Text(liveTitleText)
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgSecondary)
                            .lineLimit(1)

                        if let detail = liveDetailText {
                            Text(detail)
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }

                        if items.count >= 2 {
                            Text("· \(items.count) steps")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }

                        InlineLiveElapsedText(
                            startedAtMs: startedAtMs,
                            latestActivityMs: latestActivityMs,
                            isHostOnline: isHostOnline
                        )

                        if showInlineDiffStat && (summary.counts.additions > 0 || summary.counts.deletions > 0) {
                            HStack(spacing: 4) {
                                if summary.counts.additions > 0 {
                                    Text("+\(summary.counts.additions)")
                                        .font(OrbitTypography.code(.regular))
                                        .foregroundStyle(palette.added)
                                }
                                if summary.counts.deletions > 0 {
                                    Text("-\(summary.counts.deletions)")
                                        .font(OrbitTypography.code(.regular))
                                        .foregroundStyle(palette.removed)
                                }
                            }
                        }

                        Image(systemName: "chevron.right")
                            .font(.system(size: 10.5, weight: .semibold))
                            .foregroundStyle(palette.fgMuted.opacity(0.75))
                            .rotationEffect(.degrees(isLiveCollapsed ? 0 : 90))

                        Spacer(minLength: 0)
                    }
                    .padding(.vertical, 5)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .zIndex(1)

                if !isLiveCollapsed {
                    StepRailListView(
                        store: store,
                        items: items,
                        expandFirstToolStep: store.expandFirstStepByDefault,
                        isLiveActive: true,
                        autoExpandErrors: false
                    )
                    .padding(.top, OrbitSpacing.xs)
                    .transition(.orbitVerticalDisclosure)
                }
            }
        } else if !shouldFold || forceExpanded {
            StepRailListView(
                store: store,
                items: items,
                expandFirstToolStep: store.expandFirstStepByDefault,
                isLiveActive: false,
                autoExpandErrors: summary.counts.unrecoveredFailureCount > 0
            )
        } else {
            VStack(alignment: .leading, spacing: 0) {
                StepSummaryDisclosureButton(
                    label: resolvedSettledLabel,
                    durationBadge: settledDurationBadge,
                    unrecoveredFailureCount: summary.counts.unrecoveredFailureCount,
                    labelIncludesFailure: summary.labelIncludesFailure,
                    isExpanded: isFoldOpen,
                    showInlineDiffStat: showInlineDiffStat,
                    additions: summary.counts.additions,
                    deletions: summary.counts.deletions,
                    isLiveRunning: false
                ) {
                    store.hapticLight()
                    withAnimation(orbitDisclosureAnimation) {
                        isManualExpanded.toggle()
                    }
                }
                .zIndex(1)

                if isFoldOpen {
                    StepRailListView(
                        store: store,
                        items: items,
                        expandFirstToolStep: store.expandFirstStepByDefault,
                        isLiveActive: false,
                        autoExpandErrors: summary.counts.failedCount > 0 || summary.counts.deniedCount > 0
                    )
                    .padding(.top, OrbitSpacing.xs)
                    .transition(.orbitVerticalDisclosure)
                }
            }
        }
    }
}

// MARK: - Vertical Hairline Rail & Step Rows

private struct IndexedStepRow: Identifiable {
    let id: String
    let item: FfiTimelineItem
}

struct StepRailListView: View {
    @ObservedObject var store: OrbitIosStore
    let items: [FfiTimelineItem]
    var expandFirstToolStep: Bool = false
    var isLiveActive: Bool = false
    var autoExpandErrors: Bool = false

    @Environment(\.orbitPalette) private var palette
    @State private var showAllLiveSteps = false

    private var canRollUpEarlier: Bool {
        isLiveActive && items.count >= liveStepAutoCollapseThreshold
    }

    private var hiddenCount: Int {
        canRollUpEarlier ? max(0, items.count - liveStepVisibleTailCount) : 0
    }

    private var firstToolCallSeq: UInt64? {
        items.first(where: {
            if case .toolCall = $0 { return true }
            return false
        }).map(stepSeq) ?? items.first.map(stepSeq)
    }

    private var lastSeq: UInt64? {
        items.last.map(stepSeq)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            if canRollUpEarlier && hiddenCount > 0 {
                let earlierCount = items.count - liveStepVisibleTailCount
                let earlierRows = indexedRows(for: Array(items.prefix(earlierCount)), indexOffset: 0)
                let tailRows = indexedRows(for: Array(items.suffix(liveStepVisibleTailCount)), indexOffset: earlierCount)

                VStack(alignment: .leading, spacing: 0) {
                    Button {
                        store.hapticLight()
                        withAnimation(orbitDisclosureAnimation) {
                            showAllLiveSteps.toggle()
                        }
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: "chevron.right")
                                .font(.system(size: 10.5, weight: .semibold))
                                .foregroundStyle(palette.fgMuted.opacity(0.8))
                                .rotationEffect(.degrees(showAllLiveSteps ? 90 : 0))
                                .frame(width: 16, alignment: .center)

                            Text(showAllLiveSteps ? "Hide steps" : "\(hiddenCount) \(hiddenCount == 1 ? "step" : "steps")")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)

                            Spacer(minLength: 0)
                        }
                        .padding(.vertical, 5)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .zIndex(1)

                    if showAllLiveSteps {
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(earlierRows) { row in
                                stepRowView(for: row.item)
                            }
                        }
                        .padding(.bottom, 2)
                        .transition(.orbitVerticalDisclosure)
                    }
                }

                ForEach(tailRows) { row in
                    stepRowView(for: row.item)
                }
            } else {
                ForEach(indexedRows(for: items, indexOffset: 0)) { row in
                    stepRowView(for: row.item)
                }
            }
        }
        .padding(.leading, 19)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(alignment: .leading) {
            Rectangle()
                .fill(palette.hairline)
                .frame(width: 1)
                .padding(.leading, 6)
                .padding(.vertical, 4)
                .allowsHitTesting(false)
        }
    }

    private func stepRowView(for item: FfiTimelineItem) -> some View {
        let seq = stepSeq(item)
        let isLastInLiveGroup = isLiveActive && seq == lastSeq
        let isActivelyThinking: Bool = {
            if case .assistantThinking = item { return isLastInLiveGroup }
            return false
        }()

        return StepRowView(
            store: store,
            item: item,
            autoExpandOnAppear: expandFirstToolStep && (seq == firstToolCallSeq),
            autoExpandError: autoExpandErrors,
            isActivelyThinking: isActivelyThinking
        )
    }

    private func indexedRows(for slice: [FfiTimelineItem], indexOffset: Int) -> [IndexedStepRow] {
        slice.enumerated().map { idx, item in
            IndexedStepRow(
                id: stepRowKey(item, fallbackIndex: indexOffset + idx),
                item: item
            )
        }
    }

    private func stepRowKey(_ item: FfiTimelineItem, fallbackIndex: Int) -> String {
        switch item {
        case let .userPrompt(seq, id, _, _, _, _):
            return id.isEmpty ? "prompt-seq-\(seq)" : "prompt-\(id)"
        case let .assistantThinking(seq, id, _, _, _):
            return id.isEmpty ? "think-seq-\(seq)" : "think-\(id)"
        case let .assistantMessage(seq, id, _, _, _, _):
            return id.isEmpty ? "msg-seq-\(seq)" : "msg-\(id)"
        case let .toolCall(seq, callId, _, _, _, _, _, _, _):
            return callId.isEmpty ? "tool-seq-\(seq)" : "tool-\(callId)"
        case let .permissionRequest(seq, id, _, _, _, _, _, _):
            return id.isEmpty ? "perm-seq-\(seq)" : "perm-\(id)"
        case let .questionRequest(seq, id, _, _, _):
            return id.isEmpty ? "q-seq-\(seq)" : "q-\(id)"
        case let .notice(seq, _, _, _):
            return "notice-\(seq)-\(fallbackIndex)"
        case let .agentMessage(seq, id, _, _, _, _, _, _, _, _, _):
            return id.isEmpty ? "agmsg-seq-\(seq)" : "agmsg-\(id)"
        }
    }

    private func stepSeq(_ item: FfiTimelineItem) -> UInt64 {
        switch item {
        case let .userPrompt(seq, _, _, _, _, _): return seq
        case let .assistantThinking(seq, _, _, _, _): return seq
        case let .assistantMessage(seq, _, _, _, _, _): return seq
        case let .toolCall(seq, _, _, _, _, _, _, _, _): return seq
        case let .permissionRequest(seq, _, _, _, _, _, _, _): return seq
        case let .questionRequest(seq, _, _, _, _): return seq
        case let .notice(seq, _, _, _): return seq
        case let .agentMessage(seq, _, _, _, _, _, _, _, _, _, _): return seq
        }
    }
}

struct StepRowView: View {
    @ObservedObject var store: OrbitIosStore
    let item: FfiTimelineItem
    var autoExpandOnAppear: Bool = false
    var autoExpandError: Bool = false
    var isActivelyThinking: Bool = false

    @Environment(\.orbitPalette) private var palette
    @State private var isDetailExpanded = false
    @State private var copiedStepDetail = false

    private var shouldAutoExpandFailedTool: Bool {
        guard autoExpandError, case let .toolCall(_, _, status, _, _, _, descriptor, _, _) = item else {
            return false
        }
        let isFailed = descriptor?.isFailed ?? (status == .failed)
        let isDenied = descriptor?.isDenied ?? (status == .denied)
        return isFailed || isDenied
    }

    var body: some View {
        Group {
            switch item {
            case let .assistantThinking(_, _, text, headline, _):
                thinkingStepRow(text: text, headline: headline)

            case let .assistantMessage(_, _, text, _, blocks, _):
                intermediateMessageRow(text: text, blocks: blocks)

            case let .permissionRequest(_, _, title, _, _, _, resolvedDecision, _):
                resolvedPermissionRow(title: title, resolvedDecision: resolvedDecision)

            case let .questionRequest(_, _, questions, _, _):
                resolvedQuestionRow(count: questions.count)

            case let .toolCall(_, _, status, summaryTitle, previewBody, detail, stepDescriptor, diffFiles, _):
                let descriptor = stepDescriptor ?? store.describeTool(item)
                toolStepRow(
                    status: status,
                    summaryTitle: summaryTitle,
                    previewBody: previewBody,
                    detail: detail,
                    descriptor: descriptor,
                    diffFiles: diffFiles
                )

            case let .notice(_, level, message, _):
                TurnNoticeRow(level: level, message: message)

            case let .agentMessage(_, _, kind, sourceAgentId, sourceHostId, sourceTitle, event, text, permissionContext, blocks, timestampMs):
                AgentMessageCardView(
                    store: store,
                    kind: kind,
                    sourceAgentId: sourceAgentId,
                    sourceHostId: sourceHostId,
                    sourceTitle: sourceTitle,
                    event: event,
                    text: text,
                    permissionContext: permissionContext,
                    blocks: blocks,
                    timestampMs: timestampMs
                )

            case .userPrompt:
                EmptyView()
            }
        }
        .onAppear {
            if autoExpandOnAppear || shouldAutoExpandFailedTool {
                isDetailExpanded = true
            }
        }
        .onChange(of: autoExpandOnAppear) { shouldExpand in
            if shouldExpand {
                isDetailExpanded = true
            }
        }
        .onChange(of: autoExpandError) { _ in
            if shouldAutoExpandFailedTool {
                isDetailExpanded = true
            }
        }
    }

    // MARK: - Thinking Step Row

    private func thinkingStepRow(text: String, headline: String) -> some View {
        let cleanHeadline = headline.trimmingCharacters(in: .whitespacesAndNewlines)
        let cleanBody = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let canExpand = !cleanBody.isEmpty
        let hasDistinctHeadline = !cleanHeadline.isEmpty && cleanHeadline != "Reasoning" && cleanHeadline != "Thought"

        return VStack(alignment: .leading, spacing: 0) {
            Button {
                guard canExpand else { return }
                store.hapticLight()
                withAnimation(orbitDisclosureAnimation) {
                    isDetailExpanded.toggle()
                }
            } label: {
                HStack(spacing: 8) {
                    Group {
                        if isActivelyThinking {
                            PulsingDotView(color: palette.primary, size: 5.5)
                        } else {
                            Image(systemName: "sparkles")
                                .font(.system(size: 12.5, weight: .medium))
                                .foregroundStyle(palette.fgMuted)
                        }
                    }
                    .frame(width: 16, alignment: .center)

                    Text(isActivelyThinking ? "Thinking" : "Thought")
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(isActivelyThinking ? palette.fgPrimary : palette.fgPrimary.opacity(0.78))

                    if !isDetailExpanded && hasDistinctHeadline {
                        Text(cleanHeadline)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgSecondary)
                            .lineLimit(1)
                            .transition(.opacity)
                    }

                    if canExpand {
                        Spacer(minLength: OrbitSpacing.xs)
                        Image(systemName: "chevron.right")
                            .font(.system(size: 10.5, weight: .semibold))
                            .foregroundStyle(palette.fgMuted.opacity(0.65))
                            .rotationEffect(.degrees(isDetailExpanded ? 90 : 0))
                    } else {
                        Spacer(minLength: 0)
                    }
                }
                .padding(.vertical, 5)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .zIndex(1)

            if isDetailExpanded && canExpand {
                let paragraph = NSMutableParagraphStyle()
                let _ = (paragraph.lineSpacing = OrbitTypography.metaLineSpacing)
                let bodyAttr = NSAttributedString(
                    string: cleanBody,
                    attributes: [
                        .font: UIFont.monospacedSystemFont(ofSize: 12, weight: .regular),
                        .foregroundColor: UIColor(palette.fgSecondary),
                        .paragraphStyle: paragraph
                    ]
                )
                OrbitSelectableTextView(
                    attributedText: bodyAttr,
                    stretchesToProposedWidth: true,
                    isDarkAppearance: palette.isDark,
                    linkColor: UIColor(palette.primary)
                )
                .padding(.leading, 24)
                .padding(.top, 2)
                .padding(.bottom, OrbitSpacing.xs)
                .transition(.orbitVerticalDisclosure)
            }
        }
    }

    // MARK: - Intermediate Assistant Message Row

    @ViewBuilder
    private func intermediateMessageRow(text: String, blocks: [FfiRichBlock]) -> some View {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            let singleLinePreview = trimmed.replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
            let isCompactSingleLine = !trimmed.contains("\n") && singleLinePreview.count <= 120

            if isCompactSingleLine {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "text.bubble")
                        .font(.system(size: 12.5, weight: .medium))
                        .foregroundStyle(palette.fgMuted)
                        .frame(width: 16, alignment: .center)
                        .padding(.top, 2)

                    Text(trimmed)
                        .orbitSubheadStyle()
                        .foregroundStyle(palette.fgSecondary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(.vertical, 5)
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    Button {
                        store.hapticLight()
                        withAnimation(orbitDisclosureAnimation) {
                            isDetailExpanded.toggle()
                        }
                    } label: {
                        HStack(spacing: 8) {
                            Image(systemName: "text.bubble")
                                .font(.system(size: 12.5, weight: .medium))
                                .foregroundStyle(palette.fgMuted)
                                .frame(width: 16, alignment: .center)

                            Text(singleLinePreview)
                                .font(OrbitTypography.subhead(.regular))
                                .foregroundStyle(palette.fgSecondary)
                                .lineLimit(1)

                            Spacer(minLength: OrbitSpacing.xs)

                            Image(systemName: "chevron.right")
                                .font(.system(size: 10.5, weight: .semibold))
                                .foregroundStyle(palette.fgMuted.opacity(0.65))
                                .rotationEffect(.degrees(isDetailExpanded ? 90 : 0))
                        }
                        .padding(.vertical, 5)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .zIndex(1)

                    if isDetailExpanded {
                        RichMarkdownView(blocks: blocks, fallbackText: trimmed)
                            .padding(.leading, 24)
                            .padding(.top, 2)
                            .padding(.bottom, OrbitSpacing.xs)
                            .transition(.orbitVerticalDisclosure)
                    }
                }
            }
        }
    }

    // MARK: - Resolved Permission & Question Rows

    private func resolvedPermissionRow(title: String, resolvedDecision: String?) -> some View {
        let rawDecision = resolvedDecision ?? "resolved"
        let denied = rawDecision.range(of: "deny|reject|decline", options: [.regularExpression, .caseInsensitive]) != nil
        let formattedDecision = OrbitFormatters.decisionLabel(rawDecision)

        return HStack(spacing: 8) {
            Image(systemName: denied ? "exclamationmark.triangle" : "checkmark")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(denied ? palette.danger : palette.direct)
                .frame(width: 16, alignment: .center)

            Text("\(title) · \(formattedDecision)")
                .font(OrbitTypography.meta(.regular))
                .foregroundStyle(palette.fgSecondary)
                .lineLimit(1)

            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
    }

    private func resolvedQuestionRow(count: Int) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "checkmark")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(palette.primary)
                .frame(width: 16, alignment: .center)

            Text("Answered \(count) clarifying \(count == 1 ? "question" : "questions")")
                .font(OrbitTypography.meta(.regular))
                .foregroundStyle(palette.fgSecondary)
                .lineLimit(1)

            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
    }

    // MARK: - Tool Call Step Row

    private func toolStepRow(
        status: FfiToolStatus,
        summaryTitle: String,
        previewBody: String?,
        detail: FfiToolCallDetail,
        descriptor: FfiToolStepDescriptor?,
        diffFiles: [FfiDiffFile]
    ) -> some View {
        let verb = descriptor?.verb ?? fallbackVerb(for: detail)
        let target = descriptor?.target ?? summaryTitle
        let isFailed = descriptor?.isFailed ?? (status == .failed)
        let isDenied = descriptor?.isDenied ?? (status == .denied)
        let isRunning = descriptor?.isRunning ?? (status == .running)
        let isError = isFailed || isDenied
        let additions = descriptor?.additions ?? 0
        let deletions = descriptor?.deletions ?? 0
        let openPath = descriptor?.openPath
        let canOpenFileInDiff = openPath != nil && (descriptor?.kind == .fileEdit || descriptor?.kind == .fileCreate)

        let hasInlineDiff: Bool = {
            if let first = diffFiles.first, !first.lines.isEmpty { return true }
            if case let .fileEdit(_, unified) = detail,
               let raw = unified?.trimmingCharacters(in: .whitespacesAndNewlines),
               !raw.isEmpty {
                return true
            }
            return false
        }()
        let descriptorExpandable = descriptor?.hasExpandableDetail ?? hasRichToolDetail(detail)
        let canExpand = descriptorExpandable && {
            if case .fileEdit = detail {
                return hasInlineDiff
            }
            return true
        }()
        let isInteractive = canExpand || canOpenFileInDiff
        let iconName = toolStepIcon(descriptor: descriptor, detail: detail, summaryTitle: summaryTitle)

        return VStack(alignment: .leading, spacing: 0) {
            Button {
                if canExpand {
                    store.hapticLight()
                    withAnimation(orbitDisclosureAnimation) {
                        isDetailExpanded.toggle()
                    }
                } else if canOpenFileInDiff, let path = openPath {
                    store.hapticLight()
                    store.openGitDiff(focusFilePath: path)
                }
            } label: {
                HStack(spacing: 8) {
                    Group {
                        if isRunning {
                            PulsingDotView(color: palette.primary, size: 5.5)
                        } else if isError {
                            Image(systemName: "exclamationmark.triangle")
                                .font(.system(size: 12.5, weight: .medium))
                                .foregroundStyle(palette.danger)
                        } else {
                            Image(systemName: iconName)
                                .font(.system(size: 12.5, weight: .medium))
                                .foregroundStyle(palette.fgMuted)
                        }
                    }
                    .frame(width: 16, alignment: .center)

                    Text(verb)
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(
                            isFailed ? palette.danger :
                            isDenied ? palette.warning :
                            palette.fgPrimary.opacity(0.78)
                        )

                    if !target.isEmpty {
                        Text(target)
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(isRunning ? palette.fgPrimary : palette.fgSecondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }

                    if let secondaryMeta = descriptor?.secondaryMeta, !secondaryMeta.isEmpty {
                        Text(secondaryMeta)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgSecondary.opacity(0.78))
                            .lineLimit(1)
                    }

                    Spacer(minLength: OrbitSpacing.xs)

                    if additions > 0 || deletions > 0 {
                        HStack(spacing: 4) {
                            if additions > 0 {
                                Text("+\(additions)")
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.added)
                            }
                            if deletions > 0 {
                                Text("-\(deletions)")
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.removed)
                            }
                        }
                    }

                    if descriptor?.kind == .shell, !isRunning, let code = descriptor?.exitCode, code != 0 {
                        Text("exit \(code)")
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(palette.danger)
                    }

                    if canExpand {
                        Image(systemName: "chevron.right")
                            .font(.system(size: 10.5, weight: .semibold))
                            .foregroundStyle(palette.fgMuted.opacity(0.65))
                            .rotationEffect(.degrees(isDetailExpanded ? 90 : 0))
                    }
                }
                .padding(.vertical, 5)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(!isInteractive)
            .accessibilityLabel("\(verb) \(target)")
            .zIndex(1)

            if isDetailExpanded && canExpand {
                expandedToolDetailView(
                    summaryTitle: summaryTitle,
                    detail: detail,
                    previewBody: previewBody,
                    diffFiles: diffFiles,
                    isError: isError
                )
                .padding(.leading, 24)
                .padding(.top, OrbitSpacing.xs)
                .padding(.bottom, OrbitSpacing.xs)
                .transition(.orbitVerticalDisclosure)
            }
        }
    }

    private func toolStepIcon(
        descriptor: FfiToolStepDescriptor?,
        detail: FfiToolCallDetail,
        summaryTitle: String
    ) -> String {
        let kind: FfiToolStepKind = descriptor?.kind ?? {
            switch detail {
            case .fileRead: return .fileRead
            case .fileEdit: return .fileEdit
            case .search: return .search
            case .shell: return .shell
            case .subagent: return .subagent
            case .generic: return .generic
            }
        }()
        switch kind {
        case .fileRead:
            return "doc.text"
        case .fileCreate:
            return "doc.badge.plus"
        case .fileEdit:
            return "chevron.left.forwardslash.chevron.right"
        case .search:
            if case let .search(query, _) = detail {
                let hay = "\(query) \(summaryTitle)".lowercased()
                if hay.contains("web") || hay.contains("http://") || hay.contains("https://") || hay.contains("fetch") {
                    return "globe"
                }
                if hay.contains("glob") || hay.contains("find") || hay.contains("files") || hay.contains("*.") {
                    return "doc.text.magnifyingglass"
                }
            }
            return "magnifyingglass"
        case .shell:
            return "terminal"
        case .subagent:
            return "square.stack.3d.up"
        case .generic:
            return "wrench"
        }
    }

    @ViewBuilder
    private func expandedToolDetailView(
        summaryTitle: String,
        detail: FfiToolCallDetail,
        previewBody: String?,
        diffFiles: [FfiDiffFile],
        isError: Bool
    ) -> some View {
        switch detail {
        case let .fileEdit(path, unifiedDiff):
            VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
                if !diffFiles.isEmpty {
                    ForEach(diffFiles, id: \.path) { df in
                        inlineDiffCard(df)
                    }
                } else if let raw = unifiedDiff, !raw.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    let rawLines = raw.components(separatedBy: .newlines)
                    let shown = Array(rawLines.prefix(16))
                    ScrollView(.horizontal, showsIndicators: false) {
                        VStack(alignment: .leading, spacing: 1) {
                            ForEach(Array(shown.enumerated()), id: \.offset) { _, line in
                                Text(line)
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(rawDiffLineColor(line))
                            }
                            if rawLines.count > 16 {
                                Text("… +\(rawLines.count - 16) more lines")
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                    .padding(.top, OrbitSpacing.xs)
                            }
                        }
                        .padding(OrbitSpacing.sm)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(palette.subtleFill)
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .strokeBorder(palette.hairline, lineWidth: 0.5)
                    )
                }

                Button {
                    store.hapticLight()
                    store.openGitDiff(focusFilePath: path)
                } label: {
                    HStack(spacing: 5) {
                        Image(systemName: "arrow.left.arrow.right")
                            .font(.system(size: 11, weight: .medium))
                        Text("Open in Diff Review")
                            .font(OrbitTypography.meta(.medium))
                    }
                    .foregroundStyle(palette.fgSecondary)
                    .padding(.vertical, 4)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Inspect Full Diff for \(OrbitFormatters.shortPath(path))")
            }

        case let .shell(command, cwd, exitCode, outputPreview):
            let out = (outputPreview ?? previewBody)?.trimmingCharacters(in: .whitespacesAndNewlines)
            let meta: String? = {
                var parts: [String] = []
                if let cwd = cwd, !cwd.isEmpty {
                    parts.append("cwd: \(OrbitFormatters.compactPath(cwd))")
                }
                if let code = exitCode {
                    parts.append("exit \(code)")
                }
                return parts.isEmpty ? nil : parts.joined(separator: " · ")
            }()
            let copyText = [
                "$ \(command)",
                out
            ].compactMap { $0?.isEmpty == false ? $0 : nil }.joined(separator: "\n")

            monospacedStepSnippetCard(
                headerLine: "$ \(command)",
                metaLine: meta,
                bodyText: out,
                copyPayload: copyText,
                isError: isError
            )

        case let .fileRead(path, startLine, endLine):
            let rangeText: String = {
                if let s = startLine, let e = endLine { return "L\(s)–L\(e)" }
                if let s = startLine { return "from L\(s)" }
                return "full file"
            }()
            let cleanPreview = previewBody?.trimmingCharacters(in: .whitespacesAndNewlines)
            let copyText = cleanPreview?.isEmpty == false ? "\(path) (\(rangeText))\n\(cleanPreview!)" : path

            monospacedStepSnippetCard(
                headerLine: path,
                metaLine: rangeText,
                bodyText: cleanPreview,
                copyPayload: copyText,
                isError: isError
            )

        case let .search(query, scope):
            let header = scope.map { "\(query) in \($0)" } ?? query
            let cleanPreview = previewBody?.trimmingCharacters(in: .whitespacesAndNewlines)
            let copyText = cleanPreview?.isEmpty == false ? "\(header)\n\(cleanPreview!)" : header

            monospacedStepSnippetCard(
                headerLine: header,
                metaLine: "Search query",
                bodyText: cleanPreview,
                copyPayload: copyText,
                isError: isError
            )

        case let .subagent(_, role, promptPreview):
            let cleanPrompt = promptPreview.trimmingCharacters(in: .whitespacesAndNewlines)
            monospacedStepSnippetCard(
                headerLine: role.isEmpty ? "Subagent" : role,
                metaLine: nil,
                bodyText: cleanPrompt.isEmpty ? previewBody : cleanPrompt,
                copyPayload: cleanPrompt.isEmpty ? role : "\(role)\n\(cleanPrompt)",
                isError: isError
            )

        case let .generic(toolName, argumentsJson):
            let bodyText = (previewBody?.isEmpty == false ? previewBody : argumentsJson)?
                .trimmingCharacters(in: .whitespacesAndNewlines)
            let header = toolName.isEmpty ? summaryTitle : toolName
            monospacedStepSnippetCard(
                headerLine: header,
                metaLine: nil,
                bodyText: bodyText,
                copyPayload: bodyText?.isEmpty == false ? "\(header)\n\(bodyText!)" : header,
                isError: isError
            )
        }
    }

    private func monospacedStepSnippetCard(
        headerLine: String,
        metaLine: String?,
        bodyText: String?,
        copyPayload: String,
        isError: Bool = false
    ) -> some View {
        let bodyLines: [String] = {
            guard let raw = bodyText?.trimmingCharacters(in: .whitespacesAndNewlines), !raw.isEmpty else {
                return []
            }
            return raw.components(separatedBy: .newlines)
        }()
        let cappedLines = Array(bodyLines.prefix(14))

        return VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            HStack(alignment: .top, spacing: OrbitSpacing.sm) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(headerLine)
                        .font(OrbitTypography.code(.medium))
                        .foregroundStyle(palette.fgPrimary)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)

                    if let meta = metaLine, !meta.isEmpty {
                        Text(meta)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                    }
                }
                .padding(.top, 8)

                Spacer(minLength: OrbitSpacing.xs)

                Button {
                    copyStepSnippet(copyPayload)
                } label: {
                    Image(systemName: copiedStepDetail ? "checkmark" : "square.on.square")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(copiedStepDetail ? palette.direct : palette.fgMuted)
                        .orbitSymbolReplaceTransition()
                        .frame(width: 32, height: 32)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Copy Step Output")
            }
            .padding(.leading, OrbitSpacing.md)
            .padding(.trailing, 4)

            if !cappedLines.isEmpty {
                OrbitHairline()

                ScrollView(.horizontal, showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(Array(cappedLines.enumerated()), id: \.offset) { _, line in
                            Text(line.isEmpty ? " " : line)
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgSecondary)
                        }
                        if bodyLines.count > 14 {
                            Text("… +\(bodyLines.count - 14) more lines")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .padding(.top, 2)
                        }
                    }
                    .textSelection(.enabled)
                    .padding(.horizontal, OrbitSpacing.md)
                    .padding(.vertical, OrbitSpacing.sm)
                }
            } else {
                Spacer()
                    .frame(height: OrbitSpacing.xs)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isError ? palette.danger.opacity(0.05) : palette.subtleFill)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .strokeBorder(isError ? palette.danger.opacity(0.30) : palette.hairline, lineWidth: 0.5)
        )
    }

    private func copyStepSnippet(_ payload: String) {
        let trimmed = payload.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        UIPasteboard.general.string = trimmed
        store.refreshPasteboardAvailability()
        store.hapticLight()
        store.showToast("Copied step details")
        withAnimation(.easeOut(duration: 0.15)) {
            copiedStepDetail = true
        }
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 1_500_000_000)
            withAnimation(.easeIn(duration: 0.2)) {
                copiedStepDetail = false
            }
        }
    }

    private func inlineDiffCard(_ diffFile: FfiDiffFile) -> some View {
        let capped = Array(diffFile.lines.prefix(16))
        return ScrollView(.horizontal, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 1) {
                ForEach(Array(capped.enumerated()), id: \.offset) { _, line in
                    HStack(spacing: OrbitSpacing.xs + 2) {
                        Text(line.newLine.map(String.init) ?? line.oldLine.map(String.init) ?? "")
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: 28, alignment: .trailing)

                        Text(inlineDiffPrefix(line.kind))
                            .font(OrbitTypography.code(.semibold))
                            .foregroundStyle(diffLineTextColor(line.kind))
                            .frame(width: 10, alignment: .center)

                        Text(line.content)
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(diffLineTextColor(line.kind))
                    }
                    .padding(.horizontal, OrbitSpacing.sm)
                    .padding(.vertical, 1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(diffLineBgColor(line.kind))
                }

                if diffFile.lines.count > 16 {
                    Text("… +\(diffFile.lines.count - 16) more lines")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .padding(.horizontal, OrbitSpacing.sm)
                        .padding(.top, OrbitSpacing.xs)
                }
            }
            .padding(.vertical, OrbitSpacing.xs)
        }
        .background(palette.subtleFill)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
    }

    private func inlineDiffPrefix(_ kind: FfiDiffLineKind) -> String {
        switch kind {
        case .added: return "+"
        case .removed: return "-"
        case .hunkHeader, .meta: return ""
        case .context: return " "
        }
    }

    private func diffLineTextColor(_ kind: FfiDiffLineKind) -> Color {
        switch kind {
        case .added: return palette.added
        case .removed: return palette.removed
        case .hunkHeader: return palette.primary
        case .meta: return palette.fgMuted
        case .context: return palette.fgPrimary
        }
    }

    private func diffLineBgColor(_ kind: FfiDiffLineKind) -> Color {
        switch kind {
        case .added: return palette.added.opacity(0.11)
        case .removed: return palette.removed.opacity(0.11)
        case .hunkHeader: return palette.primary.opacity(0.08)
        case .meta, .context: return Color.clear
        }
    }

    private func rawDiffLineColor(_ line: String) -> Color {
        if line.hasPrefix("+") && !line.hasPrefix("+++") {
            return palette.added
        } else if line.hasPrefix("-") && !line.hasPrefix("---") {
            return palette.removed
        } else if line.hasPrefix("@@") {
            return palette.primary
        }
        return palette.fgSecondary
    }

    private func fallbackVerb(for detail: FfiToolCallDetail) -> String {
        switch detail {
        case .shell: return "Ran"
        case .fileRead: return "Read"
        case .fileEdit: return "Edited"
        case .search: return "Searched"
        case .subagent: return "Delegated"
        case let .generic(toolName, _): return toolName
        }
    }

    private func hasRichToolDetail(_ detail: FfiToolCallDetail) -> Bool {
        switch detail {
        case .shell, .fileEdit, .subagent:
            return true
        case .fileRead, .search:
            return true
        case let .generic(_, args):
            return !args.isEmpty
        }
    }
}

// MARK: - Inline Notice Row

struct TurnNoticeRow: View {
    let level: String
    let message: String

    @Environment(\.orbitPalette) private var palette

    private var accentColor: Color {
        let lower = level.lowercased()
        if lower.contains("err") || lower.contains("fail") {
            return palette.danger
        } else if lower.contains("warn") {
            return palette.warning
        }
        return palette.fgSecondary
    }

    private var noticeAttributedText: NSAttributedString {
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = OrbitTypography.subheadLineSpacing
        return NSAttributedString(
            string: message,
            attributes: [
                .font: UIFont.systemFont(ofSize: 13, weight: .regular),
                .foregroundColor: UIColor(accentColor),
                .paragraphStyle: paragraph
            ]
        )
    }

    var body: some View {
        HStack(alignment: .top, spacing: OrbitSpacing.sm) {
            Image(systemName: level.lowercased().contains("err") ? "exclamationmark.circle" : "exclamationmark.triangle")
                .font(OrbitTypography.iconSmall)
                .foregroundStyle(accentColor)
                .padding(.top, 2)

            OrbitSelectableTextView(
                attributedText: noticeAttributedText,
                stretchesToProposedWidth: true,
                isDarkAppearance: palette.isDark,
                linkColor: UIColor(palette.primary)
            )
        }
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.vertical, OrbitSpacing.sm)
        .background(accentColor.opacity(0.06))
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(accentColor.opacity(0.22), lineWidth: 0.5)
        )
    }
}

// MARK: - Inter-Agent Peer Message / Finish Notification / Heartbeat / Child Permission Card

struct AgentMessageCardView: View {
    @ObservedObject var store: OrbitIosStore
    let kind: FfiAgentMessageKind
    let sourceAgentId: String?
    let sourceHostId: String?
    let sourceTitle: String?
    let event: String?
    let text: String
    let permissionContext: FfiPermissionCheckpointRef?
    let blocks: [FfiRichBlock]
    let timestampMs: Int64?

    @Environment(\.orbitPalette) private var palette
    @State private var isHeartbeatExpanded = false
    @State private var resolvedChildDecision: String? = nil

    private var headerTitle: String {
        let sender = sourceTitle?.trimmingCharacters(in: .whitespacesAndNewlines)
        switch kind {
        case .heartbeat:
            return (sender?.isEmpty == false) ? "Heartbeat · \(sender!)" : "Heartbeat Wake-up"
        case .finishNotification:
            return (sender?.isEmpty == false) ? "\(sender!) finished" : "Child agent finished"
        case .directMessage:
            if permissionContext != nil {
                return (sender?.isEmpty == false) ? "Approval Needed · \(sender!)" : "Child Approval Needed"
            }
            return (sender?.isEmpty == false) ? "Message from \(sender!)" : "Peer Message"
        }
    }

    private var accentColor: Color {
        if permissionContext != nil { return palette.warning }
        switch kind {
        case .heartbeat: return palette.fgMuted
        case .finishNotification: return palette.direct
        case .directMessage: return palette.accentViolet
        }
    }

    private var iconName: String {
        if permissionContext != nil { return "exclamationmark.shield.fill" }
        switch kind {
        case .heartbeat: return "heart.text.square"
        case .finishNotification: return "checkmark.circle.fill"
        case .directMessage: return "bubble.left.and.bubble.right.fill"
        }
    }

    private var canJumpToSource: Bool {
        guard let aid = sourceAgentId, !aid.isEmpty else { return false }
        if let shid = sourceHostId, !shid.isEmpty, shid != store.selectedHostId {
            return false
        }
        return true
    }

    var body: some View {
        if kind == .heartbeat && permissionContext == nil {
            VStack(alignment: .leading, spacing: 6) {
                Button {
                    store.hapticLight()
                    withAnimation(.easeInOut(duration: 0.18)) {
                        isHeartbeatExpanded.toggle()
                    }
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: iconName)
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                        Text(headerTitle)
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgSecondary)
                        Spacer()
                        Image(systemName: "chevron.right")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(palette.fgMuted)
                            .rotationEffect(.degrees(isHeartbeatExpanded ? 90 : 0))
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)

                if isHeartbeatExpanded && !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    RichMarkdownView(
                        blocks: blocks.isEmpty ? parseMarkdownPreview(markdown: text, streaming: false) : blocks,
                        fallbackText: text
                    )
                }
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.vertical, 8)
            .background(palette.subtleFill.opacity(0.65))
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        } else {
            VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                HStack(spacing: 8) {
                    Image(systemName: iconName)
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(accentColor)

                    Text(headerTitle)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    Spacer(minLength: 4)

                    if canJumpToSource, let aid = sourceAgentId {
                        Button {
                            store.selectAgent(aid)
                        } label: {
                            Text("Open")
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(accentColor)
                                .padding(.horizontal, 9)
                                .frame(height: 24)
                                .background(accentColor.opacity(0.14), in: Capsule())
                        }
                        .buttonStyle(.plain)
                    }
                }

                if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    RichMarkdownView(
                        blocks: blocks.isEmpty ? parseMarkdownPreview(markdown: text, streaming: false) : blocks,
                        fallbackText: text
                    )
                }

                if let perm = permissionContext {
                    OrbitHairline()
                    if let decided = resolvedChildDecision {
                        HStack(spacing: 6) {
                            Image(systemName: "checkmark.circle.fill")
                                .font(.system(size: 12))
                                .foregroundStyle(palette.direct)
                            Text("Responded: \(OrbitFormatters.decisionLabel(decided))")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                        }
                    } else {
                        let decisions = perm.allowedDecisions.isEmpty
                            ? ["deny", "allow_always", "allow"]
                            : perm.allowedDecisions
                        HStack(spacing: OrbitSpacing.sm) {
                            ForEach(decisions, id: \.self) { dec in
                                let lower = dec.lowercased()
                                let isDeny = lower.contains("deny") || lower.contains("reject")
                                let isPrimary = !isDeny && !lower.contains("always")
                                Button {
                                    resolvedChildDecision = dec
                                    store.resolveChildPermission(
                                        childHostId: perm.childHostId,
                                        childAgentId: perm.childAgentId,
                                        requestId: perm.permissionRequestId,
                                        decision: dec
                                    )
                                } label: {
                                    Text(OrbitFormatters.decisionLabel(dec))
                                        .font(OrbitTypography.meta(.semibold))
                                        .foregroundStyle(
                                            isDeny ? palette.danger :
                                            isPrimary ? palette.onPrimary :
                                            palette.fgPrimary
                                        )
                                        .frame(maxWidth: .infinity, minHeight: 34)
                                        .background(
                                            isDeny ? palette.danger.opacity(0.12) :
                                            isPrimary ? palette.primary :
                                            palette.subtleFill,
                                            in: RoundedRectangle(cornerRadius: 8, style: .continuous)
                                        )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }
            }
            .padding(OrbitSpacing.md)
            .background(accentColor.opacity(palette.isDark ? 0.10 : 0.06))
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(accentColor.opacity(0.28), lineWidth: 0.5)
            )
        }
    }
}
