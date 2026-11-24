import SwiftUI

/// Desktop `StageOverviewPanel` parity — Session Summary, Quick Actions, Subagents,
/// Live Background Tasks (with elapsed ticker, log open, stop), and Touched Files.
struct SessionOverviewSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    var body: some View {
        NavigationStack {
            SessionOverviewPanel(
                store: store,
                onNavigateAction: { dismiss() }
            )
            .navigationTitle("Session Overview")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
        }
    }
}

struct SessionOverviewPanel: View {
    @ObservedObject var store: OrbitIosStore
    var onNavigateAction: (() -> Void)? = nil

    @Environment(\.orbitPalette) private var palette
    @State private var expandedTaskIds: Set<String> = []

    private var insights: FfiSessionInsights? {
        store.agentStage?.insights
    }

    private var backgroundTasks: [FfiSessionBackgroundTask] {
        insights?.backgroundTasks ?? []
    }

    private var touchedFiles: [FfiSessionTouchedFile] {
        insights?.touchedFiles ?? []
    }

    var body: some View {
        ScrollView(.vertical, showsIndicators: true) {
            VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                if let agent = store.activeAgent {
                    sessionSummaryCard(agent)
                }

                if let usage = store.activeSessionUsage, usage.turns > 0 {
                    sessionUsageCard(usage)
                }

                quickActionsSection

                if !backgroundTasks.isEmpty {
                    backgroundTasksSection
                }

                if !touchedFiles.isEmpty {
                    touchedFilesSection
                }
            }
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.vertical, OrbitSpacing.md)
        }
        .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
        .onAppear {
            store.refreshActiveSessionUsage()
        }
    }

    private func sessionUsageCard(_ usage: SessionUsage) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text(OrbitL10n.tr("Session Usage"))
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, 4)

            HStack(spacing: OrbitSpacing.md) {
                usageMetric(label: "Turns", value: "\(usage.turns)")
                OrbitHairline(axis: .vertical).frame(height: 26)
                usageMetric(label: "In / Out", value: "\(formatTokenCount(usage.inputTokens)) / \(formatTokenCount(usage.outputTokens))")
                OrbitHairline(axis: .vertical).frame(height: 26)
                usageMetric(label: "Est. Cost", value: String(format: "$%.3f", usage.estimatedCostUsd))
            }
            .padding(OrbitSpacing.md)
            .frame(maxWidth: .infinity)
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
        }
    }

    private func usageMetric(label: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(.system(size: 10.5, weight: .medium))
                .foregroundStyle(palette.fgMuted)
            Text(value)
                .font(OrbitTypography.code(.semibold))
                .foregroundStyle(palette.fgPrimary)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func formatTokenCount(_ count: UInt64) -> String {
        if count >= 1_000_000 {
            return String(format: "%.1fM", Double(count) / 1_000_000.0)
        }
        if count >= 1_000 {
            return String(format: "%.1fk", Double(count) / 1_000.0)
        }
        return "\(count)"
    }

    // MARK: - 1. Session Summary + AI Sparkle

    private func sessionSummaryCard(_ agent: FfiAgentCard) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: 8) {
                OrbitProviderIcon(provider: agent.provider, size: 15)

                Text(agent.title.isEmpty ? "Untitled Session" : agent.title)
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(2)

                Spacer(minLength: 4)

                Button {
                    store.summarizeAgent(agent.agentId)
                } label: {
                    Image(systemName: "sparkles")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(palette.accentViolet)
                        .frame(width: 30, height: 30)
                        .background(palette.accentViolet.opacity(0.12), in: Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("AI Summarize Session")

                if let hid = store.selectedHostId {
                    Button {
                        store.renameTarget = .agent(
                            hostId: hid,
                            agentId: agent.agentId,
                            currentTitle: agent.title,
                            currentHandle: agent.handle
                        )
                    } label: {
                        Image(systemName: "pencil")
                            .font(.system(size: 12.5, weight: .medium))
                            .foregroundStyle(palette.fgSecondary)
                            .frame(width: 30, height: 30)
                            .background(palette.subtleFill, in: Circle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Rename Session")
                }
            }

            if let summary = agent.taskSummary, !summary.isEmpty {
                Text(summary)
                    .orbitMetaStyle()
                    .foregroundStyle(palette.fgSecondary)
            }

            HStack(spacing: 6) {
                if let hid = store.selectedHostId {
                    let tone = store.overviewBadgeTone(forAgent: agent, hostId: hid)
                    if tone != .none {
                        Circle()
                            .fill(overviewToneColor(tone))
                            .frame(width: 7, height: 7)
                    }
                }
                metaPill(agent.statusLabel)
                if let branch = agent.worktreeBranch ?? store.activeGitBranch, !branch.isEmpty {
                    metaPill(branch, icon: "arrow.triangle.branch")
                }
                if let handle = agent.handle, !handle.isEmpty {
                    metaPill("@\(handle)")
                }
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

    private func overviewToneColor(_ tone: OrbitBadgeTone) -> Color {
        switch tone {
        case .critical: return palette.danger
        case .accent: return palette.primary
        case .muted: return palette.warning
        case .none: return .clear
        }
    }

    private func metaPill(_ text: String, icon: String? = nil) -> some View {
        HStack(spacing: 3) {
            if let icon {
                Image(systemName: icon)
                    .font(.system(size: 9.5, weight: .medium))
            }
            Text(text)
                .font(.system(size: 11, weight: .medium))
                .lineLimit(1)
        }
        .foregroundStyle(palette.fgSecondary)
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(palette.subtleFill, in: Capsule())
    }

    // MARK: - 2. Quick Workspace Actions

    private var quickActionsSection: some View {
        let diff = store.diffTotals
        let terminalCount = store.currentHostDetail?.terminals.count ?? 0
        let fileCount = store.workspaceFilesList.count
        let btwCount = store.activeSideQuestions.count

        return VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text("Workspace Tools")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, 4)

            VStack(spacing: 0) {
                actionRow(
                    title: "Diff Review",
                    icon: "arrow.left.arrow.right",
                    badge: diff.fileCount > 0 ? "\(diff.fileCount) files (+\(diff.additions) -\(diff.deletions))" : nil
                ) {
                    onNavigateAction?()
                    store.openGitDiff()
                }

                OrbitHairline().padding(.leading, 44)

                actionRow(
                    title: "Files Explorer",
                    icon: "folder",
                    badge: fileCount > 0 ? "\(fileCount)" : nil
                ) {
                    onNavigateAction?()
                    store.openWorkspaceFiles()
                }

                OrbitHairline().padding(.leading, 44)

                actionRow(
                    title: "Terminal",
                    icon: "terminal",
                    badge: terminalCount > 0 ? "\(terminalCount) active" : nil
                ) {
                    onNavigateAction?()
                    store.openTerminal()
                }

                OrbitHairline().padding(.leading, 44)

                actionRow(
                    title: OrbitL10n.tr("Side Question (/btw)"),
                    icon: "bubble.left.and.text.bubble.right",
                    badge: btwCount > 0 ? "\(btwCount)" : nil
                ) {
                    onNavigateAction?()
                    store.openSideQuestion()
                }

                if store.activeAgent != nil {
                    OrbitHairline().padding(.leading, 44)

                    actionRow(
                        title: OrbitL10n.tr("Export Transcript"),
                        icon: "square.and.arrow.up",
                        badge: nil
                    ) {
                        onNavigateAction?()
                        store.presentSheetSafely(.transcriptExport)
                    }
                }

                if let ws = store.activeWorkspace, ws.isCurated || store.activeWorkspacePeers.count > 1 {
                    OrbitHairline().padding(.leading, 44)

                    actionRow(
                        title: "Mission Board",
                        icon: "doc.richtext",
                        badge: ws.boardRevision > 0 ? "v\(ws.boardRevision)" : nil
                    ) {
                        onNavigateAction?()
                        store.openWorkspaceBoard(workspaceId: ws.workspaceId)
                    }
                }
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    private func actionRow(
        title: String,
        icon: String,
        badge: String?,
        action: @escaping () -> Void
    ) -> some View {
        Button {
            store.hapticLight()
            action()
        } label: {
            HStack(spacing: 12) {
                Image(systemName: icon)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: 20)

                Text(title)
                    .font(OrbitTypography.subhead(.medium))
                    .foregroundStyle(palette.fgPrimary)

                Spacer(minLength: 4)

                if let badge, !badge.isEmpty {
                    Text(badge)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                }

                Image(systemName: "chevron.right")
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(palette.fgMuted)
            }
            .padding(.horizontal, OrbitSpacing.md)
            .frame(minHeight: OrbitSpacing.hitTarget)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    // MARK: - 3. Live Background Tasks

    private var backgroundTasksSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text("Background Tasks (\(backgroundTasks.count))")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, 4)

            VStack(spacing: OrbitSpacing.sm) {
                ForEach(backgroundTasks, id: \.id) { task in
                    backgroundTaskCard(task)
                }
            }
        }
    }

    private func backgroundTaskCard(_ task: FfiSessionBackgroundTask) -> some View {
        let isExpanded = expandedTaskIds.contains(task.id)
        let isRunning = task.status == .running

        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                if isRunning {
                    PulsingDotView(color: palette.direct, size: 7)
                } else {
                    Circle().fill(palette.fgMuted).frame(width: 6, height: 6)
                }

                VStack(alignment: .leading, spacing: 2) {
                    Text(task.title)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    HStack(spacing: 6) {
                        if !task.subtitle.isEmpty {
                            Text(task.subtitle)
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .lineLimit(1)
                        }
                        if isRunning, let startMs = task.startedAtMs {
                            TimelineView(.periodic(from: .now, by: 1)) { ctx in
                                let sec = max(0, Int(ctx.date.timeIntervalSince1970) - Int(startMs / 1000))
                                Text(String(format: "%d:%02d", sec / 60, sec % 60))
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.direct)
                            }
                        }
                    }
                }

                Spacer(minLength: 4)

                Button {
                    withAnimation(.easeInOut(duration: 0.18)) {
                        if isExpanded {
                            expandedTaskIds.remove(task.id)
                        } else {
                            expandedTaskIds.insert(task.id)
                        }
                    }
                } label: {
                    Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(palette.fgMuted)
                        .frame(width: 28, height: 28)
                        .background(palette.subtleFill, in: Circle())
                }
                .buttonStyle(.plain)
            }

            if isExpanded {
                if let cmd = task.command, !cmd.isEmpty {
                    Text("$ \(cmd)")
                        .font(OrbitTypography.code(.regular))
                        .foregroundStyle(palette.fgPrimary)
                        .padding(8)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(palette.code)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
                if let preview = task.outputPreview, !preview.isEmpty {
                    Text(preview)
                        .font(OrbitTypography.code(.regular))
                        .foregroundStyle(palette.fgSecondary)
                        .lineLimit(8)
                }
            }

            HStack(spacing: OrbitSpacing.md) {
                if let logPath = task.logPath, !logPath.isEmpty {
                    Button {
                        onNavigateAction?()
                        store.openWorkspaceFiles(initialPath: logPath)
                    } label: {
                        Label("Open Log", systemImage: "doc.text")
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.primary)
                    }
                    .buttonStyle(.plain)
                }

                Spacer()

                if isRunning {
                    Button {
                        store.stopBackgroundTask(task)
                    } label: {
                        Label("Stop Task", systemImage: "stop.circle")
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.danger)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(isRunning ? palette.direct.opacity(0.32) : palette.hairline, lineWidth: 0.5)
        )
    }

    // MARK: - 4. Touched Files Section

    private var touchedFilesSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text("Touched Files (\(touchedFiles.count))")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, 4)

            VStack(spacing: 0) {
                ForEach(Array(touchedFiles.prefix(20).enumerated()), id: \.element.path) { idx, tf in
                    if idx > 0 { OrbitHairline().padding(.leading, 40) }
                    Button {
                        onNavigateAction?()
                        if tf.action == .edited || tf.action == .created {
                            store.openGitDiff(focusFilePath: tf.path)
                        } else {
                            let target = tf.path.isEmpty ? tf.displayPath : tf.path
                            store.openWorkspaceFiles(initialPath: target)
                        }
                    } label: {
                        HStack(spacing: 10) {
                            Image(systemName: OrbitFormatters.semanticFileIcon(for: OrbitFormatters.shortPath(tf.displayPath)))
                                .font(.system(size: 12.5, weight: .medium))
                                .foregroundStyle(palette.fgMuted)
                                .frame(width: 18)

                            Text(tf.displayPath)
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgPrimary)
                                .lineLimit(1)
                                .truncationMode(.middle)

                            Spacer(minLength: 4)

                            if tf.additions > 0 || tf.deletions > 0 {
                                HStack(spacing: 4) {
                                    if tf.additions > 0 {
                                        Text("+\(tf.additions)")
                                            .font(OrbitTypography.code(.regular))
                                            .foregroundStyle(palette.added)
                                    }
                                    if tf.deletions > 0 {
                                        Text("-\(tf.deletions)")
                                            .font(OrbitTypography.code(.regular))
                                            .foregroundStyle(palette.removed)
                                    }
                                }
                            } else {
                                Text(actionLabel(tf.action))
                                    .font(.system(size: 11))
                                    .foregroundStyle(palette.fgMuted)
                            }
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .frame(minHeight: 40)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    private func actionLabel(_ action: FfiSessionTouchedFileAction) -> String {
        switch action {
        case .created: return "New"
        case .edited: return "Edited"
        case .read: return "Read"
        }
    }
}
