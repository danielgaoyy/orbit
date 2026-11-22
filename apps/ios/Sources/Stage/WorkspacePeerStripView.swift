import SwiftUI

/// Desktop-parity Mission Workspace Peer Strip, Subagent Mode Header, Cluster Suggestion Pill,
/// Conflict Radar, Shared Blackboard trigger, and `+ Peer` spawner.
struct WorkspacePeerStripView: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.orbitPalette) private var palette
    @State private var isWorkspaceSwitcherPresented = false

    private var activeAgent: FfiAgentCard? {
        store.activeAgent
    }

    private var activeWorkspace: FfiWorkspaceSummary? {
        store.activeWorkspace
    }

    private var peers: [FfiAgentCard] {
        store.activeWorkspacePeers
    }

    private var conflictCount: Int {
        store.activeWorkspaceConflictEvents.count
    }

    private var isCuratedWorkspace: Bool {
        activeWorkspace?.isCurated == true
    }

    private var curatedWorkspacesInProject: [FfiWorkspaceSummary] {
        store.activeProjectCuratedWorkspaces
    }

    private var shouldRenderStrip: Bool {
        guard let agent = activeAgent, !store.isDraftNewSession else { return false }
        if agent.parentAgentId != nil || agent.clusterSuggestion != nil {
            return true
        }
        return OrbitSidebarLogic.shouldShowWorkspacePeerStrip(
            isExplicitCluster: isCuratedWorkspace,
            peerCount: peers.count,
            groupTitle: activeWorkspace?.title
        )
    }

    var body: some View {
        if shouldRenderStrip, let agent = activeAgent {
            VStack(spacing: 0) {
                if let parentId = agent.parentAgentId, !parentId.isEmpty {
                    subagentModeHeader(agent: agent, parentId: parentId)
                } else {
                    workspaceStripRow(agent: agent)

                    if let suggestion = agent.clusterSuggestion {
                        clusterSuggestionBar(agent: agent, suggestion: suggestion)
                    }
                }
                OrbitHairline()
            }
            .background(palette.canvas)
        }
    }

    // MARK: - Subagent Mode Banner

    private func subagentModeHeader(agent: FfiAgentCard, parentId: String) -> some View {
        let parentTitle = store.currentHostDetail?.agents.first(where: { $0.agentId == parentId })?.title
            ?? "Parent Session"
        return HStack(spacing: OrbitSpacing.sm) {
            Button {
                store.selectAgent(parentId)
            } label: {
                HStack(spacing: 4) {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 11, weight: .semibold))
                    Text(parentTitle)
                        .font(OrbitTypography.meta(.semibold))
                        .lineLimit(1)
                }
                .foregroundStyle(palette.primary)
                .padding(.horizontal, 10)
                .frame(height: 28)
                .background(palette.subtleFill, in: Capsule())
            }
            .buttonStyle(.plain)

            Text("Subagent")
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(palette.accentViolet)
                .padding(.horizontal, 7)
                .padding(.vertical, 2)
                .background(palette.accentViolet.opacity(0.14), in: Capsule())

            Spacer(minLength: 4)

            Button {
                store.detachAgent(agent.agentId)
            } label: {
                Text("Detach")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgSecondary)
                    .padding(.horizontal, 8)
                    .frame(height: 28)
                    .background(palette.subtleFill, in: Capsule())
            }
            .buttonStyle(.plain)

            Button {
                store.archiveManagedSubagent(agent.agentId)
                store.selectAgent(parentId)
            } label: {
                Image(systemName: "archivebox")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(palette.danger)
                    .frame(width: 28, height: 28)
                    .background(palette.danger.opacity(0.12), in: Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Archive Subagent")
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.vertical, 6)
    }

    // MARK: - Workspace Switcher + Peer Tabs + Board & +Peer

    private func workspaceStripRow(agent: FfiAgentCard) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                // 1. Workspace Switcher Pill (Curated title or `待归类`)
                Button {
                    store.hapticLight()
                    isWorkspaceSwitcherPresented.toggle()
                } label: {
                    HStack(spacing: 5) {
                        Image(systemName: isCuratedWorkspace ? "square.stack.3d.up" : "tray")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(isCuratedWorkspace ? palette.accentViolet : palette.fgMuted)

                        Text(isCuratedWorkspace ? (activeWorkspace?.title ?? "Workspace") : "Unassigned")
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.fgPrimary)
                            .lineLimit(1)

                        Image(systemName: "chevron.down")
                            .font(.system(size: 8.5, weight: .semibold))
                            .foregroundStyle(palette.fgMuted)
                    }
                    .padding(.horizontal, 10)
                    .frame(height: 28)
                    .background(
                        isCuratedWorkspace
                            ? palette.accentViolet.opacity(palette.isDark ? 0.16 : 0.10)
                            : palette.subtleFill,
                        in: Capsule()
                    )
                    .overlay(
                        Capsule().strokeBorder(
                            isCuratedWorkspace ? palette.accentViolet.opacity(0.28) : palette.hairline,
                            lineWidth: 0.5
                        )
                    )
                }
                .buttonStyle(.plain)
                .orbitCompactPopover(
                    isPresented: $isWorkspaceSwitcherPresented,
                    arrowEdge: .top,
                    palette: palette
                ) {
                    workspaceSwitcherPopover(agent: agent)
                }

                // 2. Peer Tabs (when curated workspace or > 1 peer)
                if isCuratedWorkspace || peers.count > 1 {
                    ForEach(peers, id: \.agentId) { peer in
                        peerTabPill(peer: peer, isActive: peer.agentId == agent.agentId)
                    }
                }

                // 3. Conflict Radar Badge
                if conflictCount > 0, let ws = activeWorkspace {
                    Button {
                        store.openWorkspaceBoard(workspaceId: ws.workspaceId)
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .font(.system(size: 10.5, weight: .semibold))
                            Text("\(conflictCount) \(conflictCount == 1 ? "Conflict" : "Conflicts")")
                                .font(OrbitTypography.meta(.semibold))
                        }
                        .foregroundStyle(palette.warning)
                        .padding(.horizontal, 9)
                        .frame(height: 28)
                        .background(palette.warning.opacity(0.14), in: Capsule())
                        .overlay(Capsule().strokeBorder(palette.warning.opacity(0.32), lineWidth: 0.5))
                    }
                    .buttonStyle(.plain)
                }

                // 4. Shared Blackboard Button
                if let ws = activeWorkspace, isCuratedWorkspace || peers.count > 1 || ws.boardRevision > 0 {
                    Button {
                        store.openWorkspaceBoard(workspaceId: ws.workspaceId)
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "doc.richtext")
                                .font(.system(size: 11, weight: .medium))
                            Text(ws.boardRevision > 0 ? "Board v\(ws.boardRevision)" : "Board")
                                .font(OrbitTypography.meta(.medium))
                        }
                        .foregroundStyle(palette.fgSecondary)
                        .padding(.horizontal, 9)
                        .frame(height: 28)
                        .background(palette.subtleFill, in: Capsule())
                    }
                    .buttonStyle(.plain)
                }

                // 5. + Peer Button
                if let ws = activeWorkspace, isCuratedWorkspace || peers.count > 1 {
                    Button {
                        store.hapticLight()
                        store.activeSheet = .spawnPeer(workspaceId: ws.workspaceId)
                    } label: {
                        HStack(spacing: 3) {
                            Image(systemName: "plus")
                                .font(.system(size: 10.5, weight: .bold))
                            Text("Peer")
                                .font(OrbitTypography.meta(.semibold))
                        }
                        .foregroundStyle(palette.primary)
                        .padding(.horizontal, 9)
                        .frame(height: 28)
                        .background(palette.subtleFill, in: Capsule())
                        .overlay(Capsule().strokeBorder(palette.hairline, lineWidth: 0.5))
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.vertical, 6)
        }
    }

    private func peerTabPill(peer: FfiAgentCard, isActive: Bool) -> some View {
        let label = OrbitFormatters.semanticAgentTabLabel(
            handle: peer.handle,
            title: peer.title,
            taskSummary: peer.taskSummary,
            agentId: peer.agentId
        )
        return Button {
            if !isActive {
                store.selectAgent(peer.agentId)
            }
        } label: {
            HStack(spacing: 5) {
                if peer.isAwaitingPermission {
                    Circle()
                        .fill(palette.warning)
                        .frame(width: 6, height: 6)
                } else if peer.isRunning {
                    OrbitSpinnerRingView(color: isActive ? palette.primary : palette.fgSecondary, size: 11)
                } else if peer.isUnread && !isActive {
                    Circle()
                        .fill(palette.accentSky)
                        .frame(width: 5.5, height: 5.5)
                } else {
                    OrbitProviderIcon(provider: peer.provider, size: 12)
                }

                Text(label)
                    .font(OrbitTypography.meta(isActive ? .semibold : .medium))
                    .foregroundStyle(isActive ? palette.fgPrimary : palette.fgSecondary)
                    .lineLimit(1)

                if let br = peer.worktreeBranch, !br.isEmpty {
                    Image(systemName: "arrow.triangle.branch")
                        .font(.system(size: 9.5, weight: .medium))
                        .foregroundStyle(palette.fgMuted)
                }
            }
            .padding(.horizontal, 9)
            .frame(height: 28)
            .background(
                isActive ? palette.activeRowFill : Color.clear,
                in: Capsule()
            )
            .overlay(
                Capsule().strokeBorder(
                    isActive ? palette.hairline : palette.hairline.opacity(0.6),
                    lineWidth: 0.5
                )
            )
        }
        .buttonStyle(.plain)
        .contextMenu {
            if let hid = store.selectedHostId {
                Button {
                    store.renameTarget = .agent(
                        hostId: hid,
                        agentId: peer.agentId,
                        currentTitle: peer.title,
                        currentHandle: peer.handle
                    )
                } label: {
                    Label("Rename Tab / Handle", systemImage: "pencil")
                }

                Button {
                    store.summarizeAgent(peer.agentId)
                } label: {
                    Label("AI Update Label & Summary", systemImage: "sparkles")
                }
            }

            if let ws = activeWorkspace {
                Button {
                    store.openWorkspaceBoard(workspaceId: ws.workspaceId)
                } label: {
                    Label("Open Mission Board", systemImage: "doc.richtext")
                }
            }
        }
    }

    // MARK: - Cluster Suggestion / Auto-Cluster Banner

    private func clusterSuggestionBar(
        agent: FfiAgentCard,
        suggestion: FfiWorkspaceClusterSuggestion
    ) -> some View {
        HStack(spacing: 6) {
            Image(systemName: "sparkles")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(palette.accentViolet)

            if suggestion.autoApplied {
                Text("Auto-grouped into \"\(suggestion.targetWorkspaceTitle)\"")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)

                Spacer(minLength: 4)

                Button {
                    if let prevId = suggestion.previousWorkspaceId, !prevId.isEmpty {
                        store.moveAgentToWorkspace(agentId: agent.agentId, targetWorkspaceId: prevId)
                    } else {
                        store.moveAgentToWorkspace(
                            agentId: agent.agentId,
                            targetWorkspaceId: nil,
                            makeUnclassified: true
                        )
                    }
                    store.dismissClusterSuggestion(agent.agentId)
                } label: {
                    Text("Undo")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.accentViolet)
                        .padding(.horizontal, 8)
                        .frame(height: 24)
                        .background(palette.accentViolet.opacity(0.14), in: Capsule())
                }
                .buttonStyle(.plain)
            } else {
                Text("Suggested workspace: \"\(suggestion.targetWorkspaceTitle)\"")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)

                Spacer(minLength: 4)

                Button {
                    store.moveAgentToWorkspace(
                        agentId: agent.agentId,
                        targetWorkspaceId: suggestion.targetWorkspaceId
                    )
                    store.dismissClusterSuggestion(agent.agentId)
                } label: {
                    Text("Move")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.accentViolet)
                        .padding(.horizontal, 8)
                        .frame(height: 24)
                        .background(palette.accentViolet.opacity(0.14), in: Capsule())
                }
                .buttonStyle(.plain)
            }

            Button {
                store.dismissClusterSuggestion(agent.agentId)
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(palette.fgMuted)
                    .frame(width: 24, height: 24)
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.vertical, 5)
        .background(palette.accentViolet.opacity(palette.isDark ? 0.10 : 0.06))
    }

    // MARK: - Workspace Switcher Popover

    private func workspaceSwitcherPopover(agent: FfiAgentCard) -> some View {
        let projectRoot = store.activeProjectRoot
        let projectName = store.activeProjectGroup?.name ?? OrbitFormatters.shortPath(projectRoot)
        let isGitRepo = store.activeProjectGroup?.isGitRepo ?? (store.activeGitBranch != nil)

        return VStack(alignment: .leading, spacing: 6) {
            OrbitPopoverSectionHeader(
                title: "Mission Workspace",
                trailing: isCuratedWorkspace ? "Active" : "Unassigned"
            )
            .padding(.horizontal, 10)

            VStack(alignment: .leading, spacing: 2) {
                OrbitPopoverActionRow(
                    title: "AI Organize Session",
                    systemImage: "sparkles"
                ) {
                    isWorkspaceSwitcherPresented = false
                    store.organizeProjectWorkspaces(
                        projectRoot: projectRoot,
                        agentIds: [agent.agentId],
                        autoApply: true
                    )
                }

                OrbitPopoverActionRow(
                    title: "AI Summarize Title & Summary",
                    systemImage: "text.badge.star"
                ) {
                    isWorkspaceSwitcherPresented = false
                    store.summarizeAgent(agent.agentId)
                }

                if let ws = activeWorkspace, isCuratedWorkspace {
                    OrbitPopoverActionRow(
                        title: "Open Mission Board",
                        systemImage: "doc.richtext"
                    ) {
                        isWorkspaceSwitcherPresented = false
                        store.openWorkspaceBoard(workspaceId: ws.workspaceId)
                    }

                    if let hid = store.selectedHostId {
                        OrbitPopoverActionRow(
                            title: "Rename Workspace",
                            systemImage: "pencil"
                        ) {
                            isWorkspaceSwitcherPresented = false
                            store.renameTarget = .workspace(
                                hostId: hid,
                                workspaceId: ws.workspaceId,
                                currentTitle: ws.title
                            )
                        }
                    }
                }
            }
            .padding(.horizontal, 10)

            if !curatedWorkspacesInProject.isEmpty {
                OrbitHairline()
                    .padding(.horizontal, 20)
                    .padding(.vertical, 2)

                OrbitPopoverSectionHeader(title: "Switch Workspace")
                    .padding(.horizontal, 10)

                ScrollView(.vertical, showsIndicators: true) {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(curatedWorkspacesInProject, id: \.workspaceId) { ws in
                            let selected = ws.workspaceId == agent.workspaceId
                            OrbitPopoverOptionRow(
                                title: ws.title,
                                subtitle: ws.missionGoal,
                                systemImage: "square.stack.3d.up",
                                isSelected: selected
                            ) {
                                isWorkspaceSwitcherPresented = false
                                if !selected {
                                    store.moveAgentToWorkspace(
                                        agentId: agent.agentId,
                                        targetWorkspaceId: ws.workspaceId
                                    )
                                }
                            }
                        }
                    }
                    .padding(.horizontal, 10)
                }
                .frame(maxHeight: 160)
            }

            OrbitHairline()
                .padding(.horizontal, 20)
                .padding(.vertical, 2)

            VStack(alignment: .leading, spacing: 2) {
                OrbitPopoverActionRow(
                    title: "New Workspace & Move Here",
                    systemImage: "plus.rectangle.on.folder"
                ) {
                    isWorkspaceSwitcherPresented = false
                    store.activeSheet = .createWorkspace(
                        projectRoot: projectRoot,
                        projectName: projectName,
                        isGitRepo: isGitRepo,
                        moveAgentId: agent.agentId
                    )
                }

                if isCuratedWorkspace {
                    OrbitPopoverActionRow(
                        title: "Move to Unassigned",
                        systemImage: "tray"
                    ) {
                        isWorkspaceSwitcherPresented = false
                        store.moveAgentToWorkspace(
                            agentId: agent.agentId,
                            targetWorkspaceId: nil,
                            makeUnclassified: true
                        )
                    }
                }
            }
            .padding(.horizontal, 10)
        }
        .padding(.vertical, 10)
        .frame(width: 280)
    }
}
