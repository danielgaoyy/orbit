import SwiftUI

struct StageTurnView: View {
    @ObservedObject var store: OrbitIosStore
    let turn: FfiStageTurn

    @Environment(\.orbitPalette) private var palette

    private var isFirstStepBearingTurn: Bool {
        if let firstWithSteps = store.agentStage?.turns.first(where: { !$0.foldedSteps.isEmpty }) {
            return turn.key == firstWithSteps.key
        }
        return turn.turnIndex == 0 || turn.key == store.agentStage?.turns.first?.key
    }
    @State private var isFoldedStepsExpanded = false
    @State private var copiedReply = false

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

    var body: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.md) {
            // 1. User Prompt Bubble (right-aligned elevated card)
            if let promptItem = turn.userPrompt,
               case let .userPrompt(_, _, text, badges, _, _) = promptItem {
                UserPromptBubbleView(text: text, badges: badges)
            }

            // 2. Compaction Command Indicator
            if turn.isCompactionCommand {
                compactionDivider(label: "Context compacted")
            }

            // 3. Settled Turn vs Live Streaming Turn
            if let replyItem = turn.terminalReply {
                if !turn.foldedSteps.isEmpty {
                    settledStepsSection
                }

                if case let .assistantMessage(_, _, text, _, blocks, _) = replyItem {
                    let resolvedReply = resolvedAssistantText(text: text, blocks: blocks)
                    VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
                        RichMarkdownView(blocks: blocks, fallbackText: text)
                            .contextMenu {
                                if !resolvedReply.isEmpty {
                                    Button {
                                        copyReply(resolvedReply)
                                    } label: {
                                        Label("Copy", systemImage: "doc.on.doc")
                                    }

                                    Button {
                                        store.quoteAssistantReply(turnKey: turn.key, text: resolvedReply)
                                    } label: {
                                        Label("Quote", systemImage: "text.quote")
                                    }
                                }
                            }

                        if !resolvedReply.isEmpty {
                            settledReplyMicroActions(resolvedReply: resolvedReply)
                        }
                    }
                }

                ForEach(Array(turn.notices.enumerated()), id: \.offset) { _, noticeItem in
                    if case let .notice(_, level, message, _) = noticeItem {
                        TurnNoticeRow(level: level, message: message)
                    }
                }

                if !turn.turnDiffs.isEmpty {
                    turnDiffPillsStrip
                }
            } else {
                liveStreamingSection
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
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

    // MARK: - Settled Folded Steps Header + Vertical Hairline Rail

    private var settledStepsSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            Button {
                store.hapticLight()
                withAnimation(.spring(response: 0.26, dampingFraction: 0.86)) {
                    isFoldedStepsExpanded.toggle()
                }
            } label: {
                HStack(spacing: OrbitSpacing.sm) {
                    Text(settledSummaryLabel)
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(turn.workSummaryIncludesFailure ? palette.warning : palette.fgSecondary)
                        .lineLimit(1)

                    if let dur = OrbitFormatters.formatDurationMs(turn.turnDurationMs ?? turn.thoughtDurationMs) {
                        Text(dur)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                    }

                    if turn.workCounts.additions > 0 || turn.workCounts.deletions > 0 {
                        HStack(spacing: OrbitSpacing.xs) {
                            if turn.workCounts.additions > 0 {
                                Text("+\(turn.workCounts.additions)")
                                    .font(OrbitTypography.meta(.medium))
                                    .foregroundStyle(palette.added)
                            }
                            if turn.workCounts.deletions > 0 {
                                Text("-\(turn.workCounts.deletions)")
                                    .font(OrbitTypography.meta(.medium))
                                    .foregroundStyle(palette.removed)
                            }
                        }
                    }

                    Image(systemName: "chevron.right")
                        .font(OrbitTypography.iconMicro)
                        .foregroundStyle(palette.fgMuted)
                        .rotationEffect(.degrees(isFoldedStepsExpanded ? 90 : 0))

                    Spacer(minLength: 0)
                }
                .frame(minHeight: OrbitSpacing.hitTarget)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if isFoldedStepsExpanded {
                StepRailListView(
                    store: store,
                    items: turn.foldedSteps,
                    expandFirstToolStep: store.expandFirstStepByDefault && isFirstStepBearingTurn
                )
                .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
    }

    // MARK: - Live Streaming Segments

    @ViewBuilder
    private var liveStreamingSection: some View {
        if turn.liveSegments.isEmpty {
            if turn.hasActiveItem {
                HStack(spacing: OrbitSpacing.sm) {
                    PulsingDotView(color: palette.primary, size: 6)
                    Text(turn.workSummaryLiveLabel.isEmpty ? "Thinking…" : turn.workSummaryLiveLabel)
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgSecondary)
                }
                .padding(.vertical, OrbitSpacing.xs)
            }
        } else {
            VStack(alignment: .leading, spacing: OrbitSpacing.md) {
                ForEach(turn.liveSegments, id: \.segmentKey) { segment in
                    switch segment {
                    case let .stepGroup(_, _, items, summary, hasFollowingContent, hasRunningStep):
                        LiveStepGroupView(
                            store: store,
                            items: items,
                            summary: summary,
                            fallbackLiveLabel: turn.workSummaryLiveLabel,
                            hasFollowingContent: hasFollowingContent,
                            hasRunningStep: hasRunningStep
                        )

                    case let .assistantMessage(_, item):
                        if case let .assistantMessage(_, _, text, _, blocks, _) = item {
                            RichMarkdownView(blocks: blocks, fallbackText: text)
                        }

                    case let .notice(_, item):
                        if case let .notice(_, level, message, _) = item {
                            TurnNoticeRow(level: level, message: message)
                        }

                    case let .compaction(_, label):
                        compactionDivider(label: label.isEmpty ? "Context compacted" : label)
                    }
                }
            }
        }
    }

    // MARK: - Turn Diff Summary Pills

    private var turnDiffPillsStrip: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: OrbitSpacing.sm) {
                ForEach(turn.turnDiffs, id: \.path) { diff in
                    Button {
                        store.hapticLight()
                        store.openGitDiff(focusFilePath: diff.path)
                    } label: {
                        HStack(spacing: OrbitSpacing.xs) {
                            Image(systemName: "doc.text")
                                .font(OrbitTypography.iconMicro)
                                .foregroundStyle(palette.fgMuted)

                            Text(OrbitFormatters.shortPath(diff.path))
                                .font(OrbitTypography.code(.medium))
                                .foregroundStyle(palette.fgSecondary)

                            if diff.additions > 0 {
                                Text("+\(diff.additions)")
                                    .font(OrbitTypography.meta(.medium))
                                    .foregroundStyle(palette.added)
                            }
                            if diff.deletions > 0 {
                                Text("-\(diff.deletions)")
                                    .font(OrbitTypography.meta(.medium))
                                    .foregroundStyle(palette.removed)
                            }
                        }
                        .padding(.horizontal, OrbitSpacing.sm)
                        .frame(minHeight: 36)
                        .background(palette.subtleFill)
                        .clipShape(Capsule())
                        .overlay(Capsule().strokeBorder(palette.hairline, lineWidth: 0.5))
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private func compactionDivider(label: String) -> some View {
        HStack(spacing: OrbitSpacing.sm) {
            OrbitHairline()
            Text(label)
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgMuted)
            OrbitHairline()
        }
        .padding(.vertical, OrbitSpacing.xs)
    }

    // MARK: - Settled Reply Micro-Actions (Copy / Quote)

    private func settledReplyMicroActions(resolvedReply: String) -> some View {
        HStack(spacing: OrbitSpacing.md) {
            Button {
                copyReply(resolvedReply)
            } label: {
                HStack(spacing: OrbitSpacing.xs) {
                    Image(systemName: copiedReply ? "checkmark" : "doc.on.doc")
                        .font(OrbitTypography.iconMicro)
                    Text(copiedReply ? "Copied" : "Copy")
                        .font(OrbitTypography.meta(.medium))
                }
                .foregroundStyle(copiedReply ? palette.direct : palette.fgMuted)
                .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Copy Assistant Reply")

            Button {
                store.quoteAssistantReply(turnKey: turn.key, text: resolvedReply)
            } label: {
                HStack(spacing: OrbitSpacing.xs) {
                    Image(systemName: "text.quote")
                        .font(OrbitTypography.iconMicro)
                    Text("Quote")
                        .font(OrbitTypography.meta(.medium))
                }
                .foregroundStyle(palette.fgMuted)
                .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Quote Assistant Reply")

            Spacer(minLength: 0)
        }
    }

    private func copyReply(_ text: String) {
        store.copyAssistantReply(text)
        withAnimation(.easeOut(duration: 0.15)) {
            copiedReply = true
        }
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 1_500_000_000)
            withAnimation(.easeIn(duration: 0.2)) {
                copiedReply = false
            }
        }
    }

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
        case let .stepGroup(key, _, _, _, _, _): return key
        case let .assistantMessage(key, _): return key
        case let .notice(key, _): return key
        case let .compaction(key, _): return key
        }
    }
}

// MARK: - User Prompt Bubble (Right-Aligned, 18pt Radius, >8 Line Collapse)

struct UserPromptBubbleView: View {
    let text: String
    let badges: [FfiPromptBadge]

    @Environment(\.orbitPalette) private var palette
    @State private var isExpanded = false

    private var cleanText: String {
        text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var shouldCollapse: Bool {
        let lineCount = cleanText.components(separatedBy: .newlines).count
        return lineCount > 8 || cleanText.count > 420
    }

    var body: some View {
        HStack {
            Spacer(minLength: OrbitSpacing.xxl)

            VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                if !badges.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: OrbitSpacing.xs) {
                            ForEach(Array(badges.enumerated()), id: \.offset) { _, badge in
                                badgeChip(badge)
                            }
                        }
                    }
                }

                if !cleanText.isEmpty {
                    Text(cleanText)
                        .orbitBodyStyle()
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(shouldCollapse && !isExpanded ? 8 : nil)
                        .textSelection(.enabled)
                }

                if shouldCollapse {
                    Button {
                        withAnimation(.easeInOut(duration: 0.2)) {
                            isExpanded.toggle()
                        }
                    } label: {
                        Text(isExpanded ? "Show less" : "Show more")
                            .font(OrbitTypography.subhead(.medium))
                            .foregroundStyle(palette.primary)
                            .frame(minHeight: 32, alignment: .leading)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.vertical, OrbitSpacing.sm + 2)
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    private func badgeChip(_ badge: FfiPromptBadge) -> some View {
        let isSlash = badge.kind == "slash"
        return HStack(spacing: OrbitSpacing.xs) {
            Image(systemName: badgeIcon(for: badge.kind))
                .font(OrbitTypography.iconMicro)
            Text(badge.label)
                .font(OrbitTypography.code(.medium))
                .lineLimit(1)
        }
        .foregroundStyle(isSlash ? palette.primary : palette.fgSecondary)
        .padding(.horizontal, OrbitSpacing.sm)
        .padding(.vertical, 3)
        .background(isSlash ? palette.primary.opacity(0.12) : palette.subtleFill)
        .clipShape(Capsule())
    }

    private func badgeIcon(for kind: String) -> String {
        switch kind {
        case "slash": return "sparkles"
        case "file": return "doc.text"
        case "git_diff": return "arrow.triangle.branch"
        case "terminal": return "terminal"
        case "quote": return "text.quote"
        case "image": return "photo"
        default: return "paperclip"
        }
    }
}

// MARK: - Live Step Group

private struct LiveStepGroupView: View {
    @ObservedObject var store: OrbitIosStore
    let items: [FfiTimelineItem]
    let summary: FfiWorkSummaryResult
    let fallbackLiveLabel: String
    let hasFollowingContent: Bool
    let hasRunningStep: Bool

    @Environment(\.orbitPalette) private var palette
    @State private var manualExpanded: Bool?

    private var isExpanded: Bool {
        if let manual = manualExpanded {
            return manual
        }
        if store.expandAllWorkGroupsByDefault || store.expandFirstStepByDefault {
            return true
        }
        return hasRunningStep && !hasFollowingContent
    }

    private var headerText: String {
        if hasRunningStep {
            if !summary.liveLabel.isEmpty { return summary.liveLabel }
            if !fallbackLiveLabel.isEmpty { return fallbackLiveLabel }
            return "Working…"
        }
        return summary.label.isEmpty ? "Completed \(items.count) steps" : summary.label
    }

    var body: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            Button {
                store.hapticLight()
                withAnimation(.spring(response: 0.25, dampingFraction: 0.86)) {
                    manualExpanded = !isExpanded
                }
            } label: {
                HStack(spacing: OrbitSpacing.sm) {
                    if hasRunningStep {
                        PulsingDotView(color: palette.primary, size: 6)
                    }

                    Text(headerText)
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(summary.labelIncludesFailure ? palette.warning : palette.fgSecondary)
                        .lineLimit(1)

                    Image(systemName: "chevron.right")
                        .font(OrbitTypography.iconMicro)
                        .foregroundStyle(palette.fgMuted)
                        .rotationEffect(.degrees(isExpanded ? 90 : 0))

                    Spacer(minLength: 0)
                }
                .frame(minHeight: OrbitSpacing.hitTarget)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if isExpanded {
                StepRailListView(
                    store: store,
                    items: items,
                    expandFirstToolStep: store.expandFirstStepByDefault
                )
            }
        }
    }
}

// MARK: - Vertical Hairline Rail & Step Rows

struct StepRailListView: View {
    @ObservedObject var store: OrbitIosStore
    let items: [FfiTimelineItem]
    var expandFirstToolStep: Bool = false

    @Environment(\.orbitPalette) private var palette

    private var firstToolCallIndex: Int? {
        items.firstIndex { item in
            if case .toolCall = item { return true }
            return false
        } ?? (items.isEmpty ? nil : 0)
    }

    var body: some View {
        HStack(alignment: .top, spacing: OrbitSpacing.md) {
            Rectangle()
                .fill(palette.hairline)
                .frame(width: 1)
                .padding(.leading, OrbitSpacing.xs)
                .padding(.vertical, OrbitSpacing.xs)

            VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
                ForEach(Array(items.enumerated()), id: \.offset) { idx, item in
                    StepRowView(
                        store: store,
                        item: item,
                        autoExpandOnAppear: expandFirstToolStep && (idx == firstToolCallIndex)
                    )
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct StepRowView: View {
    @ObservedObject var store: OrbitIosStore
    let item: FfiTimelineItem
    var autoExpandOnAppear: Bool = false

    @Environment(\.orbitPalette) private var palette
    @State private var isDetailExpanded = false
    @State private var copiedStepDetail = false

    var body: some View {
        Group {
            switch item {
            case let .assistantThinking(_, _, text, headline, _):
                thinkingStepRow(text: text, headline: headline)

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

            default:
                EmptyView()
            }
        }
        .onAppear {
            if autoExpandOnAppear {
                isDetailExpanded = true
            }
        }
        .onChange(of: autoExpandOnAppear) { shouldExpand in
            if shouldExpand {
                isDetailExpanded = true
            }
        }
    }

    private func thinkingStepRow(text: String, headline: String) -> some View {
        let cleanHeadline = headline.trimmingCharacters(in: .whitespacesAndNewlines)
        let title = cleanHeadline.isEmpty ? "Reasoning" : cleanHeadline
        let cleanBody = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let canExpand = !cleanBody.isEmpty

        return VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            Button {
                guard canExpand else { return }
                withAnimation(.easeInOut(duration: 0.18)) {
                    isDetailExpanded.toggle()
                }
            } label: {
                HStack(spacing: OrbitSpacing.xs) {
                    Text("Thought")
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgMuted)

                    if title != "Reasoning" && title != "Thought" {
                        Text(title)
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgSecondary)
                            .lineLimit(1)
                    }

                    if canExpand {
                        Image(systemName: "chevron.right")
                            .font(OrbitTypography.iconMicro)
                            .foregroundStyle(palette.fgMuted)
                            .rotationEffect(.degrees(isDetailExpanded ? 90 : 0))
                    }

                    Spacer(minLength: 0)
                }
                .frame(minHeight: OrbitSpacing.hitTarget)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if isDetailExpanded && canExpand {
                Text(cleanBody)
                    .orbitSubheadStyle()
                    .foregroundStyle(palette.fgSecondary)
                    .padding(.bottom, OrbitSpacing.xs)
                    .textSelection(.enabled)
            }
        }
    }

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
        let additions = descriptor?.additions ?? 0
        let deletions = descriptor?.deletions ?? 0

        return VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            Button {
                store.hapticLight()
                withAnimation(.easeInOut(duration: 0.2)) {
                    isDetailExpanded.toggle()
                }
            } label: {
                HStack(spacing: OrbitSpacing.sm) {
                    if isRunning {
                        PulsingDotView(color: palette.primary, size: 5)
                    }

                    Text(verb)
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(
                            isFailed ? palette.danger :
                            isDenied ? palette.warning :
                            palette.fgSecondary
                        )

                    if !target.isEmpty {
                        Text(target)
                            .font(OrbitTypography.code(.medium))
                            .foregroundStyle(palette.fgPrimary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }

                    if additions > 0 || deletions > 0 {
                        HStack(spacing: OrbitSpacing.xs) {
                            if additions > 0 {
                                Text("+\(additions)")
                                    .font(OrbitTypography.meta(.medium))
                                    .foregroundStyle(palette.added)
                            }
                            if deletions > 0 {
                                Text("-\(deletions)")
                                    .font(OrbitTypography.meta(.medium))
                                    .foregroundStyle(palette.removed)
                            }
                        }
                    }

                    if let code = descriptor?.exitCode, code != 0 {
                        Text("exit \(code)")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.danger)
                    }

                    Image(systemName: "chevron.right")
                        .font(OrbitTypography.iconMicro)
                        .foregroundStyle(palette.fgMuted)
                        .rotationEffect(.degrees(isDetailExpanded ? 90 : 0))

                    Spacer(minLength: 0)
                }
                .frame(minHeight: OrbitSpacing.hitTarget)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(verb) \(target)")

            if isDetailExpanded {
                expandedToolDetailView(
                    summaryTitle: summaryTitle,
                    detail: detail,
                    previewBody: previewBody,
                    diffFiles: diffFiles
                )
                .padding(.bottom, OrbitSpacing.xs)
                .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
    }

    @ViewBuilder
    private func expandedToolDetailView(
        summaryTitle: String,
        detail: FfiToolCallDetail,
        previewBody: String?,
        diffFiles: [FfiDiffFile]
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
                } else {
                    monospacedStepSnippetCard(
                        headerLine: path,
                        metaLine: nil,
                        bodyText: previewBody,
                        copyPayload: path
                    )
                }

                Button {
                    store.openGitDiff(focusFilePath: path)
                } label: {
                    HStack(spacing: OrbitSpacing.xs) {
                        Image(systemName: "arrow.triangle.branch")
                            .font(OrbitTypography.iconMicro)
                        Text("Inspect Full Diff")
                            .font(OrbitTypography.meta(.semibold))
                    }
                    .foregroundStyle(palette.primary)
                    .frame(minHeight: OrbitSpacing.hitTarget)
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
                copyPayload: copyText
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
                copyPayload: copyText
            )

        case let .search(query, scope):
            let header = scope.map { "\(query) in \($0)" } ?? query
            let cleanPreview = previewBody?.trimmingCharacters(in: .whitespacesAndNewlines)
            let copyText = cleanPreview?.isEmpty == false ? "\(header)\n\(cleanPreview!)" : header

            monospacedStepSnippetCard(
                headerLine: header,
                metaLine: "Search query",
                bodyText: cleanPreview,
                copyPayload: copyText
            )

        case let .subagent(_, role, promptPreview):
            let cleanPrompt = promptPreview.trimmingCharacters(in: .whitespacesAndNewlines)
            monospacedStepSnippetCard(
                headerLine: role.isEmpty ? "Subagent" : role,
                metaLine: nil,
                bodyText: cleanPrompt.isEmpty ? previewBody : cleanPrompt,
                copyPayload: cleanPrompt.isEmpty ? role : "\(role)\n\(cleanPrompt)"
            )

        case let .generic(toolName, argumentsJson):
            let bodyText = (previewBody?.isEmpty == false ? previewBody : argumentsJson)?
                .trimmingCharacters(in: .whitespacesAndNewlines)
            let header = toolName.isEmpty ? summaryTitle : toolName
            monospacedStepSnippetCard(
                headerLine: header,
                metaLine: nil,
                bodyText: bodyText,
                copyPayload: bodyText?.isEmpty == false ? "\(header)\n\(bodyText!)" : header
            )
        }
    }

    private func monospacedStepSnippetCard(
        headerLine: String,
        metaLine: String?,
        bodyText: String?,
        copyPayload: String
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
                .padding(.top, OrbitSpacing.sm)

                Spacer(minLength: OrbitSpacing.xs)

                Button {
                    copyStepSnippet(copyPayload)
                } label: {
                    Image(systemName: copiedStepDetail ? "checkmark" : "doc.on.doc")
                        .font(OrbitTypography.iconMicro)
                        .foregroundStyle(copiedStepDetail ? palette.direct : palette.fgMuted)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Copy Step Output")
            }
            .padding(.leading, OrbitSpacing.md)

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
        .background(palette.subtleFill)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
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

    var body: some View {
        HStack(alignment: .top, spacing: OrbitSpacing.sm) {
            Image(systemName: level.lowercased().contains("err") ? "exclamationmark.circle" : "info.circle")
                .font(OrbitTypography.iconSmall)
                .foregroundStyle(accentColor)
                .padding(.top, 2)

            Text(message)
                .orbitSubheadStyle()
                .foregroundStyle(accentColor)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.vertical, OrbitSpacing.sm)
        .background(accentColor.opacity(0.08))
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(accentColor.opacity(0.25), lineWidth: 0.5)
        )
    }
}
