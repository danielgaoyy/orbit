import SwiftUI

/// Desktop v0.7.1 redesigned Mission Overview & Shared Blackboard (`BOARD.md`).
struct WorkspaceBoardSheet: View {
    @ObservedObject var store: OrbitIosStore
    let workspaceId: String

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    var body: some View {
        NavigationStack {
            WorkspaceBoardPanel(
                store: store,
                workspaceId: workspaceId,
                onSelectPeer: { agentId in
                    dismiss()
                    store.selectAgent(agentId)
                },
                onOpenFile: { path in
                    dismiss()
                    store.openWorkspaceFiles(initialPath: path)
                }
            )
            .navigationTitle("Mission Overview")
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

struct WorkspaceBoardPanel: View {
    @ObservedObject var store: OrbitIosStore
    let workspaceId: String
    var onSelectPeer: ((String) -> Void)? = nil
    var onOpenFile: ((String) -> Void)? = nil

    @Environment(\.orbitPalette) private var palette
    @State private var isEditingGoal = false
    @State private var goalDraft = ""
    @State private var isEditingRawBoard = false
    @State private var rawBoardDraft = ""
    @State private var isTimelineExpanded = false

    private var workspace: FfiWorkspaceSummary? {
        store.currentHostDetail?.workspaces.first(where: { $0.workspaceId == workspaceId })
    }

    private var peers: [FfiAgentCard] {
        (store.currentHostDetail?.agents ?? []).filter { ag in
            ag.workspaceId == workspaceId
                && ag.archivedAtRfc3339 == nil
                && !(ag.parentAgentId != nil && ag.parentHostId == nil)
        }
    }

    private var board: FfiWorkspaceBoardSnapshot? {
        store.workspaceBoard(for: workspaceId)
    }

    private var events: [FfiWorkspaceEvent] {
        store.workspaceEvents(for: workspaceId)
    }

    private var conflictEvents: [FfiWorkspaceEvent] {
        events.filter { $0.kind == .conflictWarning }
    }

    private var touchedFiles: [String] {
        var seen = Set<String>()
        var result: [String] = []
        for ev in events.reversed() where ev.kind == .fileEdited {
            if let fp = ev.filePath?.trimmingCharacters(in: .whitespacesAndNewlines),
               !fp.isEmpty,
               seen.insert(fp).inserted {
                result.append(fp)
            }
        }
        return result
    }

    private struct ParsedBoardSection: Identifiable {
        let id: String
        let title: String
        let displayTitle: String
        let body: String
    }

    private var parsedSections: [ParsedBoardSection] {
        guard let raw = board?.content, !raw.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return []
        }
        let lines = raw.components(separatedBy: .newlines)
        var sections: [(heading: String, lines: [String])] = []
        var currentHeading: String? = nil
        var currentLines: [String] = []

        for line in lines {
            if line.hasPrefix("## ") {
                if let h = currentHeading {
                    sections.append((h, currentLines))
                }
                currentHeading = String(line.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                currentLines = []
            } else if currentHeading != nil {
                currentLines.append(line)
            }
        }
        if let h = currentHeading {
            sections.append((h, currentLines))
        }

        return sections.compactMap { sec in
            let cleaned = sec.lines
                .joined(separator: "\n")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if cleaned.isEmpty || isBoilerplateSectionBody(cleaned) {
                return nil
            }
            return ParsedBoardSection(
                id: sec.heading,
                title: sec.heading,
                displayTitle: localizedSectionTitle(sec.heading),
                body: cleaned
            )
        }
    }

    private func isBoilerplateSectionBody(_ text: String) -> Bool {
        let stripped = text
            .replacingOccurrences(of: #"(?s)<!--.*?-->"#, with: "", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if stripped.isEmpty { return true }
        let lower = stripped.lowercased()
        return lower == "_none yet._"
            || lower == "none yet."
            || lower == "- tbd"
            || lower == "tbd"
            || lower == "(none)"
    }

    private func localizedSectionTitle(_ raw: String) -> String {
        OrbitBoardLogic.englishSectionTitle(raw)
    }

    var body: some View {
        ScrollView(.vertical, showsIndicators: true) {
            VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                headerCard
                missionGoalCard

                if !conflictEvents.isEmpty {
                    conflictsCard
                }

                peersRosterCard
                collaborationNotesCard

                if !touchedFiles.isEmpty {
                    touchedFilesCard
                }

                activityTimelineCard
            }
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.vertical, OrbitSpacing.md)
        }
        .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
        .onAppear {
            store.fetchWorkspaceBoardAndEvents(workspaceId)
            goalDraft = workspace?.missionGoal ?? ""
            rawBoardDraft = board?.content ?? ""
        }
        .onChange(of: board?.revision) { _ in
            if !isEditingRawBoard {
                rawBoardDraft = board?.content ?? ""
            }
        }
    }

    // MARK: - 1. Header Stats Card

    private var headerCard: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(alignment: .center, spacing: OrbitSpacing.sm) {
                Image(systemName: "square.stack.3d.up.fill")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(palette.accentViolet)

                Text(workspace?.title ?? "Mission Workspace")
                    .font(OrbitTypography.title)
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(2)

                Spacer(minLength: 4)

                if let ws = workspace, let hid = store.selectedHostId {
                    Button {
                        store.renameTarget = .workspace(
                            hostId: hid,
                            workspaceId: ws.workspaceId,
                            currentTitle: ws.title
                        )
                    } label: {
                        Image(systemName: "pencil")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(palette.fgSecondary)
                            .frame(width: 30, height: 30)
                            .background(palette.subtleFill, in: Circle())
                    }
                    .buttonStyle(.plain)
                }
            }

            HStack(spacing: 8) {
                statChip(icon: "person.2", text: "\(peers.count) \(peers.count == 1 ? "peer" : "peers")")
                statChip(icon: "doc.richtext", text: "v\(board?.revision ?? workspace?.boardRevision ?? 0)")
                if let branch = workspace?.gitBranch, !branch.isEmpty {
                    statChip(icon: "arrow.triangle.branch", text: branch)
                }
                Spacer(minLength: 0)
                Button {
                    store.fetchWorkspaceBoardAndEvents(workspaceId)
                    store.hapticLight()
                } label: {
                    Image(systemName: "arrow.clockwise")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(palette.fgMuted)
                        .frame(width: 28, height: 28)
                }
                .buttonStyle(.plain)
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

    private func statChip(icon: String, text: String) -> some View {
        HStack(spacing: 4) {
            Image(systemName: icon)
                .font(.system(size: 10.5, weight: .medium))
            Text(text)
                .font(OrbitTypography.meta(.medium))
                .lineLimit(1)
        }
        .foregroundStyle(palette.fgSecondary)
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(palette.subtleFill, in: Capsule())
    }

    // MARK: - 2. Editable Mission Goal Card

    private var missionGoalCard: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack {
                Label("Mission Goal", systemImage: "target")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)
                Spacer()
                Button(isEditingGoal ? "Save" : "Edit") {
                    if isEditingGoal {
                        store.updateWorkspaceGoal(workspaceId: workspaceId, goal: goalDraft)
                        isEditingGoal = false
                    } else {
                        goalDraft = workspace?.missionGoal ?? ""
                        isEditingGoal = true
                    }
                }
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.primary)
            }

            if isEditingGoal {
                TextField("Define the shared mission objective…", text: $goalDraft, axis: .vertical)
                    .lineLimit(2...5)
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgPrimary)
                    .padding(10)
                    .background(palette.subtleFill)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            } else {
                let goalText = workspace?.missionGoal?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                Text(goalText.isEmpty ? "No mission goal set yet. Tap Edit to align all workspace peers." : goalText)
                    .orbitSubheadStyle()
                    .foregroundStyle(goalText.isEmpty ? palette.fgMuted : palette.fgPrimary)
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

    // MARK: - 3. Conflict Alerts Card

    private var conflictsCard: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: 6) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .font(.system(size: 13))
                    .foregroundStyle(palette.warning)
                Text("File Conflict Radar (\(conflictEvents.count))")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.warning)
            }

            ForEach(Array(conflictEvents.suffix(5))) { ev in
                HStack(alignment: .top, spacing: 8) {
                    Circle()
                        .fill(palette.warning)
                        .frame(width: 5, height: 5)
                        .padding(.top, 6)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(ev.summary)
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgPrimary)
                        if let fp = ev.filePath, !fp.isEmpty {
                            Text(OrbitFormatters.shortFileLabel(fp, rootCwd: workspace?.cwd ?? store.activeCwd))
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }
                    }
                    Spacer(minLength: 0)
                }
            }
        }
        .padding(OrbitSpacing.md)
        .background(palette.warning.opacity(palette.isDark ? 0.12 : 0.08))
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(palette.warning.opacity(0.35), lineWidth: 0.5)
        )
    }

    // MARK: - 4. Participating Sessions Roster

    private var peersRosterCard: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack {
                Text("Participating Sessions (\(peers.count))")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)
                Spacer()
                Button {
                    store.hapticLight()
                    store.activeSheet = .spawnPeer(workspaceId: workspaceId)
                } label: {
                    HStack(spacing: 3) {
                        Image(systemName: "plus")
                            .font(.system(size: 10.5, weight: .bold))
                        Text("Add Peer")
                            .font(OrbitTypography.meta(.semibold))
                    }
                    .foregroundStyle(palette.primary)
                }
                .buttonStyle(.plain)
            }

            if peers.isEmpty {
                Text("No active sessions in this workspace yet.")
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgMuted)
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(peers.enumerated()), id: \.element.agentId) { idx, peer in
                        if idx > 0 {
                            OrbitHairline().padding(.leading, 36)
                        }
                        let isSelected = store.selectedAgentId == peer.agentId && !store.isDraftNewSession
                        Button {
                            if let onSelectPeer {
                                onSelectPeer(peer.agentId)
                            } else {
                                store.selectAgent(peer.agentId)
                            }
                        } label: {
                            HStack(spacing: 10) {
                                if peer.isAwaitingPermission {
                                    Circle().fill(palette.warning).frame(width: 7, height: 7)
                                } else if peer.isRunning {
                                    OrbitSpinnerRingView(color: palette.primary, size: 12)
                                } else {
                                    OrbitProviderIcon(provider: peer.provider, size: 14)
                                }

                                VStack(alignment: .leading, spacing: 2) {
                                    HStack(spacing: 6) {
                                        Text(OrbitFormatters.semanticAgentTabLabel(
                                            handle: peer.handle,
                                            title: peer.title,
                                            taskSummary: peer.taskSummary,
                                            agentId: peer.agentId
                                        ))
                                        .font(OrbitTypography.subhead(.semibold))
                                        .foregroundStyle(palette.fgPrimary)
                                        .lineLimit(1)

                                        if isSelected {
                                            Text("Current")
                                                .font(.system(size: 10, weight: .semibold))
                                                .foregroundStyle(palette.accentViolet)
                                                .padding(.horizontal, 6)
                                                .padding(.vertical, 1)
                                                .background(palette.accentViolet.opacity(0.14), in: Capsule())
                                        }
                                    }

                                    Text(peer.taskSummary ?? peer.statusLabel)
                                        .font(OrbitTypography.meta(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                        .lineLimit(1)
                                }

                                Spacer(minLength: 4)

                                Image(systemName: "chevron.right")
                                    .font(.system(size: 10.5, weight: .semibold))
                                    .foregroundStyle(palette.fgMuted)
                            }
                            .padding(.vertical, 9)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
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

    // MARK: - 5. Structured Collaboration Notes + Raw Markdown Editor

    private var collaborationNotesCard: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack {
                Label("Collaboration Board (BOARD.md)", systemImage: "doc.richtext")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)

                Spacer()

                if isEditingRawBoard {
                    Button("Cancel") {
                        isEditingRawBoard = false
                        rawBoardDraft = board?.content ?? ""
                    }
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgMuted)
                    .padding(.trailing, 8)

                    Button("Save") {
                        store.updateWorkspaceBoard(workspaceId: workspaceId, content: rawBoardDraft)
                        isEditingRawBoard = false
                    }
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.primary)
                } else {
                    Button("Edit Markdown") {
                        rawBoardDraft = board?.content ?? ""
                        isEditingRawBoard = true
                    }
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.primary)
                }
            }

            if isEditingRawBoard {
                TextEditor(text: $rawBoardDraft)
                    .font(OrbitTypography.code(.regular))
                    .foregroundStyle(palette.fgPrimary)
                    .scrollContentBackground(.hidden)
                    .frame(minHeight: 220)
                    .padding(10)
                    .background(palette.code)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .strokeBorder(palette.hairline, lineWidth: 0.5)
                    )
            } else if parsedSections.isEmpty {
                let raw = board?.content.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                if raw.isEmpty {
                    Text("No shared notes recorded yet. Agents and users can post decisions, contracts, and findings here.")
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgMuted)
                } else {
                    RichMarkdownView(
                        blocks: parseMarkdownPreview(markdown: raw, streaming: false),
                        fallbackText: raw
                    )
                }
            } else {
                VStack(alignment: .leading, spacing: OrbitSpacing.md) {
                    ForEach(parsedSections) { sec in
                        VStack(alignment: .leading, spacing: 6) {
                            Text(sec.displayTitle)
                                .font(OrbitTypography.subhead(.semibold))
                                .foregroundStyle(palette.fgPrimary)

                            RichMarkdownView(
                                blocks: parseMarkdownPreview(markdown: sec.body, streaming: false),
                                fallbackText: sec.body
                            )
                        }
                        .padding(OrbitSpacing.sm + 2)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(palette.subtleFill.opacity(0.6))
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    }
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

    // MARK: - 6. Deduplicated Touched Files

    private var touchedFilesCard: some View {
        let rootCwd = workspace?.cwd ?? store.activeCwd
        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            Text("Touched Files (\(touchedFiles.count))")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)

            VStack(spacing: 0) {
                ForEach(Array(touchedFiles.prefix(15).enumerated()), id: \.element) { idx, path in
                    if idx > 0 {
                        OrbitHairline().padding(.leading, 28)
                    }
                    let rel = OrbitFormatters.shortFileLabel(path, rootCwd: rootCwd)
                    let resolvedPath = OrbitPathLogic.resolveWorkspaceFilePath(path, rootCwd: rootCwd)
                    Button {
                        if let onOpenFile {
                            onOpenFile(resolvedPath)
                        } else {
                            store.openWorkspaceFiles(initialPath: resolvedPath)
                        }
                    } label: {
                        HStack(spacing: 8) {
                            Image(systemName: OrbitFormatters.semanticFileIcon(for: OrbitFormatters.shortPath(path)))
                                .font(.system(size: 12.5, weight: .medium))
                                .foregroundStyle(palette.fgMuted)
                                .frame(width: 18)

                            Text(rel)
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgPrimary)
                                .lineLimit(1)
                                .truncationMode(.middle)

                            Spacer(minLength: 4)

                            Image(systemName: "chevron.right")
                                .font(.system(size: 10, weight: .semibold))
                                .foregroundStyle(palette.fgMuted)
                        }
                        .padding(.vertical, 8)
                        .contentShape(Rectangle())
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
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
    }

    // MARK: - 7. Collapsible Activity Timeline

    private var activityTimelineCard: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            Button {
                store.hapticLight()
                withAnimation(.easeInOut(duration: 0.2)) {
                    isTimelineExpanded.toggle()
                }
            } label: {
                HStack {
                    Text("Activity Timeline (\(events.count))")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.fgMuted)
                    Spacer()
                    Image(systemName: "chevron.right")
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(palette.fgMuted)
                        .rotationEffect(.degrees(isTimelineExpanded ? 90 : 0))
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if isTimelineExpanded {
                if events.isEmpty {
                    Text("No workspace events recorded yet.")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                } else {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(Array(events.reversed().prefix(30))) { ev in
                            HStack(alignment: .top, spacing: 8) {
                                Image(systemName: eventIcon(ev.kind))
                                    .font(.system(size: 11.5, weight: .medium))
                                    .foregroundStyle(ev.kind == .conflictWarning ? palette.warning : palette.fgMuted)
                                    .frame(width: 16)
                                    .padding(.top, 2)

                                VStack(alignment: .leading, spacing: 2) {
                                    Text(ev.summary)
                                        .font(OrbitTypography.meta(.regular))
                                        .foregroundStyle(palette.fgPrimary)
                                    Text(OrbitFormatters.relativeTimeFromRfc3339(ev.createdAtRfc3339))
                                        .font(.system(size: 11))
                                        .foregroundStyle(palette.fgMuted)
                                }
                                Spacer(minLength: 0)
                            }
                        }
                    }
                    .padding(.top, 4)
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

    private func eventIcon(_ kind: FfiWorkspaceEventKind) -> String {
        switch kind {
        case .peerJoined: return "person.badge.plus"
        case .peerFinished: return "checkmark.circle"
        case .boardUpdated: return "doc.richtext"
        case .peerMessage: return "bubble.left.and.bubble.right"
        case .fileEdited: return "pencil"
        case .conflictWarning: return "exclamationmark.triangle.fill"
        case .workspaceMerged: return "arrow.triangle.merge"
        }
    }
}
