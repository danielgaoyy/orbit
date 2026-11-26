import SwiftUI
import UIKit

struct FleetDrawerView: View {
    private static let maxVisibleConversationsPerProject = 5

    @ObservedObject var store: OrbitIosStore
    let isCompactOverlay: Bool

    @Environment(\.orbitPalette) private var palette
    @State private var expandedMoreProjectIds: Set<String> = []
    @State private var isSortFilterPopoverPresented: Bool = false
    @State private var isHostPickerPresented: Bool = false
    @State private var confirmArchiveWorkspace: (workspace: FfiSidebarWorkspaceGroup, keepUnclassified: Bool)? = nil

    private var trimmedSearchQuery: String {
        store.drawerSearchQuery.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var softSurfaceFill: Color {
        palette.isDark ? Color.white.opacity(0.08) : Color.black.opacity(0.055)
    }

    private var selectedRowFill: Color {
        palette.isDark ? Color.white.opacity(0.095) : Color.black.opacity(0.06)
    }

    private var mutedRowForeground: Color {
        palette.fgPrimary.opacity(0.80)
    }

    private var isFilterActive: Bool {
        store.sidebarThreadFilter == .activeUnread
    }

    // MARK: - Rust-Authoritative Sidebar Projection

    private struct ScopedConvRow: Identifiable {
        let id: String
        let conv: FfiSidebarConversation
        let projectName: String?
    }

    private func hostDisplayName(for hostId: String) -> String? {
        store.hosts.first(where: { $0.hostId == hostId })?.displayName
    }

    private var allOrderedProjects: [FfiSidebarProjectGroup] {
        let all = store.sidebarView?.projects ?? []
        guard let activeHost = store.selectedHostId else { return all }
        return all.sorted { lhs, rhs in
            if (lhs.hostId == activeHost) != (rhs.hostId == activeHost) {
                return lhs.hostId == activeHost
            }
            return false
        }
    }

    private var hostProjects: [FfiSidebarProjectGroup] {
        guard let hostId = store.selectedHostId else { return [] }
        return (store.sidebarView?.projects ?? []).filter { $0.hostId == hostId }
    }

    private var needsAttentionItems: [ScopedConvRow] {
        let activeHost = store.selectedHostId
        let list = (store.sidebarView?.needsAttention ?? [])
            .filter { !isHiddenSameHostSubagent($0.agent) || $0.agent.isAwaitingPermission }
            .sorted { lhs, rhs in
                if (lhs.hostId == activeHost) != (rhs.hostId == activeHost) {
                    return lhs.hostId == activeHost
                }
                return false
            }
        return list.map { conv in
            ScopedConvRow(
                id: OrbitSidebarLogic.scopedRowId(section: "attention", hostId: conv.hostId, agentId: conv.agent.agentId),
                conv: conv,
                projectName: projectDisplayName(forCwd: conv.agent.cwd)
            )
        }
    }

    private var pinnedItems: [ScopedConvRow] {
        let activeHost = store.selectedHostId
        let list = (store.sidebarView?.pinnedConversations ?? [])
            .filter { !isHiddenSameHostSubagent($0.agent) }
            .sorted { lhs, rhs in
                if (lhs.hostId == activeHost) != (rhs.hostId == activeHost) {
                    return lhs.hostId == activeHost
                }
                return false
            }
        return list.map { conv in
            ScopedConvRow(
                id: OrbitSidebarLogic.scopedRowId(section: "pinned", hostId: conv.hostId, agentId: conv.agent.agentId),
                conv: conv,
                projectName: projectDisplayName(forCwd: conv.agent.cwd)
            )
        }
    }

    private func isHiddenSameHostSubagent(_ agent: FfiAgentCard) -> Bool {
        agent.parentAgentId != nil && agent.parentHostId == nil
    }

    private var activeScheduleCount: Int {
        (store.currentHostDetail?.schedules ?? []).filter { $0.status == .active }.count
    }

    private var archivedCount: Int {
        store.allArchivedAgentsAcrossHosts.count
    }

    private var displayProjects: [FfiSidebarProjectGroup] {
        allOrderedProjects
    }

    // MARK: - Search Mode Matches

    private var searchMatchedConversations: [ScopedConvRow] {
        guard !trimmedSearchQuery.isEmpty else { return [] }
        var seen = Set<String>()
        var results: [ScopedConvRow] = []

        func matches(_ conv: FfiSidebarConversation, projectName: String?, groupTitle: String?) -> Bool {
            OrbitSidebarLogic.matchesSearch(
                query: trimmedSearchQuery,
                fields: [
                    conv.agent.title,
                    conv.agent.taskSummary,
                    conv.agent.cwd,
                    conv.agent.handle,
                    projectName,
                    groupTitle,
                    hostDisplayName(for: conv.hostId)
                ]
            )
        }

        for row in pinnedItems {
            let key = "\(row.conv.hostId)::\(row.conv.agent.agentId)"
            if matches(row.conv, projectName: row.projectName, groupTitle: nil), seen.insert(key).inserted {
                results.append(
                    ScopedConvRow(
                        id: OrbitSidebarLogic.scopedRowId(section: "search", hostId: row.conv.hostId, agentId: row.conv.agent.agentId),
                        conv: row.conv,
                        projectName: row.projectName
                    )
                )
            }
        }
        for grp in allOrderedProjects {
            for ws in grp.workspaces {
                for conv in ws.conversations where !isHiddenSameHostSubagent(conv.agent) {
                    let key = "\(conv.hostId)::\(conv.agent.agentId)"
                    if matches(conv, projectName: grp.name, groupTitle: ws.title), seen.insert(key).inserted {
                        results.append(
                            ScopedConvRow(
                                id: OrbitSidebarLogic.scopedRowId(section: "search", hostId: conv.hostId, agentId: conv.agent.agentId),
                                conv: conv,
                                projectName: grp.name
                            )
                        )
                    }
                }
            }
            for conv in grp.conversations + grp.unclassifiedConversations where !isHiddenSameHostSubagent(conv.agent) {
                let key = "\(conv.hostId)::\(conv.agent.agentId)"
                if matches(conv, projectName: grp.name, groupTitle: nil), seen.insert(key).inserted {
                    results.append(
                        ScopedConvRow(
                            id: OrbitSidebarLogic.scopedRowId(section: "search", hostId: conv.hostId, agentId: conv.agent.agentId),
                            conv: conv,
                            projectName: grp.name
                        )
                    )
                }
            }
        }
        return results
    }

    private var searchMatchedProjects: [(id: String, name: String, cwd: String, branch: String?)] {
        guard !trimmedSearchQuery.isEmpty else { return [] }
        let q = trimmedSearchQuery.lowercased()
        var results: [(id: String, name: String, cwd: String, branch: String?)] = []
        var seenCwds: Set<String> = []

        for proj in allOrderedProjects {
            let matched = proj.name.localizedCaseInsensitiveContains(q)
                || proj.primaryCwd.localizedCaseInsensitiveContains(q)
                || (proj.gitBranch?.localizedCaseInsensitiveContains(q) ?? false)
            if matched {
                seenCwds.insert(proj.primaryCwd)
                results.append((
                    id: proj.id,
                    name: proj.name,
                    cwd: proj.primaryCwd,
                    branch: proj.gitBranch
                ))
            }
        }

        for dp in store.currentHostDetail?.discoveredProjects ?? [] {
            let clean = dp.path.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !clean.isEmpty, !seenCwds.contains(clean) else { continue }
            let matched = dp.name.localizedCaseInsensitiveContains(q)
                || clean.localizedCaseInsensitiveContains(q)
                || (dp.gitBranch?.localizedCaseInsensitiveContains(q) ?? false)
            if matched {
                seenCwds.insert(clean)
                results.append((
                    id: "repo::\(clean)",
                    name: dp.name.isEmpty ? OrbitFormatters.shortPath(clean) : dp.name,
                    cwd: clean,
                    branch: dp.gitBranch
                ))
            }
        }
        return results
    }

    // MARK: - Root Body

    var body: some View {
        VStack(spacing: 0) {
            if !store.hosts.isEmpty {
                drawerTopBar
            } else {
                unpairedTopHeader
            }

            drawerContentList

            drawerFooterBar
        }
        .background(drawerSurfaceBackground)
    }

    private var drawerSurfaceBackground: some View {
        palette.drawer.ignoresSafeArea()
    }

    // MARK: - Top Bar: Single Row `[ 🔍 Search ]  [ ✎ ]`

    private var drawerTopBar: some View {
        HStack(spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(palette.fgMuted)

                TextField(
                    "",
                    text: $store.drawerSearchQuery,
                    prompt: Text("Search").foregroundColor(palette.fgMuted.opacity(0.65))
                )
                .font(OrbitTypography.body(.regular))
                .foregroundStyle(palette.fgPrimary)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled(true)
                .submitLabel(.search)

                if !store.drawerSearchQuery.isEmpty {
                    Button {
                        store.hapticLight()
                        store.drawerSearchQuery = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 14, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: 28, height: 28)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear Search")
                }
            }
            .padding(.leading, OrbitSpacing.md)
            .padding(.trailing, store.drawerSearchQuery.isEmpty ? OrbitSpacing.md : 4)
            .frame(height: 38)
            .background(softSurfaceFill, in: Capsule())

            Button {
                store.hapticLight()
                store.startNewSessionDraft()
            } label: {
                Image(systemName: "square.and.pencil")
                    .font(.system(size: 15.5, weight: .medium))
                    .foregroundStyle(palette.fgPrimary)
                    .frame(width: 38, height: 38)
                    .background(softSurfaceFill, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("New Conversation")
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.top, OrbitSpacing.sm)
        .padding(.bottom, OrbitSpacing.sm)
    }

    private var unpairedTopHeader: some View {
        HStack {
            Text("Orbit")
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)
            Spacer()
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.top, OrbitSpacing.md)
        .padding(.bottom, OrbitSpacing.xs)
    }

    // MARK: - Main Scrollable List (Zero Hairlines, Transparent Backing)

    private var drawerContentList: some View {
        List {
            if store.hosts.isEmpty {
                unpairedDrawerContent
            } else if !trimmedSearchQuery.isEmpty {
                searchResultsContent
            } else {
                missionControlContent
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(Color.clear)
        .orbitSoftScrollEdgeEffect()
        .scrollDismissesKeyboard(.interactively)
        .environment(\.defaultMinListRowHeight, 22)
    }

    @ViewBuilder
    private var unpairedDrawerContent: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.md) {
            Text(
                store.localDaemonUri != nil
                    ? "Local Orbit daemon detected on this Mac."
                    : "Pair your workstation or Linux devbox to inspect projects and live conversations."
            )
            .font(OrbitTypography.subhead(.regular))
            .foregroundStyle(palette.fgSecondary)

            if store.localDaemonUri != nil {
                Button {
                    _ = store.connectLocalSimulatorHost()
                } label: {
                    HStack(spacing: OrbitSpacing.xs) {
                        Image(systemName: "bolt.horizontal.circle.fill")
                            .font(OrbitTypography.iconSmall)
                        Text("Connect Local Host")
                            .font(OrbitTypography.subhead(.medium))
                    }
                    .foregroundStyle(palette.onPrimary)
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(height: 40)
                    .background(palette.primary, in: Capsule())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, OrbitSpacing.lg)
        .listRowInsets(EdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    @ViewBuilder
    private var searchResultsContent: some View {
        Button {
            store.hapticLight()
            let q = trimmedSearchQuery
            if isCompactOverlay {
                store.isDrawerOpen = false
            }
            store.globalSearchQuery = q
            store.performGlobalSearch(query: q)
            store.activeSheet = .globalSearch
        } label: {
            HStack(spacing: 10) {
                Image(systemName: "text.magnifyingglass")
                    .font(.system(size: 13.5, weight: .semibold))
                    .foregroundStyle(palette.primary)
                    .frame(width: 16, height: 16)

                Text(OrbitL10n.tr("Search across all hosts"))
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)

                Spacer(minLength: 4)

                Image(systemName: "chevron.right")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(palette.fgMuted)
            }
            .padding(.horizontal, 10)
            .frame(minHeight: 38)
            .background(palette.elevated, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .listRowInsets(EdgeInsets(top: 4, leading: 8, bottom: 6, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)

        if searchMatchedConversations.isEmpty && searchMatchedProjects.isEmpty {
            VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
                Text("No matches for \"\(trimmedSearchQuery)\"")
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgMuted)

                Button {
                    store.hapticLight()
                    store.drawerSearchQuery = ""
                } label: {
                    Text("Clear search")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.primary)
                        .frame(minHeight: 32, alignment: .leading)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, OrbitSpacing.md)
            .listRowInsets(EdgeInsets(top: 2, leading: 8, bottom: 2, trailing: 8))
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
        } else {
            if !searchMatchedConversations.isEmpty {
                sectionHeaderRow("Conversations")
                ForEach(searchMatchedConversations) { entry in
                    sessionRow(
                        conv: entry.conv,
                        trailingTag: entry.projectName
                    )
                }
            }

            if !searchMatchedProjects.isEmpty {
                sectionHeaderRow("Projects", topPadding: searchMatchedConversations.isEmpty ? 6 : 14)
                ForEach(searchMatchedProjects, id: \.id) { item in
                    Button {
                        store.startNewSessionDraft(cwd: item.cwd)
                    } label: {
                        HStack(spacing: 10) {
                            Image(systemName: "folder")
                                .font(.system(size: 14, weight: .medium))
                                .foregroundStyle(palette.fgSecondary)
                                .frame(width: 16, height: 16)

                            Text(item.name)
                                .font(OrbitTypography.body(.medium))
                                .foregroundStyle(palette.fgPrimary.opacity(0.90))
                                .lineLimit(1)

                            Spacer(minLength: 4)

                            if let br = item.branch, !br.isEmpty {
                                Text(br)
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                    .lineLimit(1)
                            }
                        }
                        .padding(.horizontal, 8)
                        .frame(minHeight: 40)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .listRowInsets(EdgeInsets(top: 1, leading: 8, bottom: 1, trailing: 8))
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
                }
            }
        }
    }

    @ViewBuilder
    private var missionControlContent: some View {
        // 0a. Personal Agent Global Overview Entry
        homeOverviewNavRow

        // 0b. Quick Automations Entry
        automationsNavRow

        // 1. Conditional Needs Attention (0pt when empty)
        if !needsAttentionItems.isEmpty {
            sectionHeaderRow(
                "Needs Attention",
                count: needsAttentionItems.count,
                accentColor: palette.warning,
                topPadding: 6
            )
            ForEach(needsAttentionItems) { item in
                attentionRow(conv: item.conv, projectName: item.projectName ?? "Session")
            }
        }

        // 2. Conditional Pinned Conversations (0pt when empty)
        if !pinnedItems.isEmpty {
            sectionHeaderRow(
                "Pinned",
                topPadding: needsAttentionItems.isEmpty ? 6 : 14
            )
            ForEach(pinnedItems) { row in
                sessionRow(conv: row.conv, project: projectForCwd(row.conv.agent.cwd))
            }
        }

        // 3. Projects Tree Header + Groups
        projectsSectionHeaderRow(
            topPadding: (needsAttentionItems.isEmpty && pinnedItems.isEmpty) ? 6 : 14
        )

        if displayProjects.isEmpty {
            VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
                Text(isFilterActive ? "No active or unread conversations" : "No projects yet")
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgMuted)

                if isFilterActive {
                    Button {
                        store.hapticLight()
                        store.sidebarThreadFilter = .all
                    } label: {
                        Text("Show all conversations")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.primary)
                            .frame(minHeight: 32, alignment: .leading)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, OrbitSpacing.sm)
            .listRowInsets(EdgeInsets(top: 2, leading: 8, bottom: 2, trailing: 8))
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
        } else {
            ForEach(Array(displayProjects.enumerated()), id: \.element.id) { projIdx, project in
                let isCollapsed = store.collapsedProjectIds.contains(project.id)
                let hostId = project.hostId
                let curatedWorkspaces = project.workspaces.filter { $0.isCurated || $0.isMultiAgent }
                let visibleUnclassified = (curatedWorkspaces.isEmpty ? project.conversations : project.unclassifiedConversations)
                    .filter { !isHiddenSameHostSubagent($0.agent) }
                let orgStatus = store.organizeStatus(forProjectRoot: project.primaryCwd)

                projectHeaderRow(
                    project: project,
                    projectIndex: projIdx,
                    totalProjects: displayProjects.count,
                    isCollapsed: isCollapsed,
                    hostId: hostId
                )

                if !isCollapsed {
                    if let status = orgStatus, status.phase == .running {
                        organizingProgressRow(status: status)
                    }

                    // Curated Mission Workspace Groups
                    ForEach(curatedWorkspaces) { ws in
                        let wsCollapsed = store.collapsedWorkspaceIds.contains(ws.workspaceId)
                        let wsConvs = ws.conversations.filter { !isHiddenSameHostSubagent($0.agent) }
                        workspaceHeaderRow(
                            workspace: ws,
                            project: project,
                            siblingWorkspaces: curatedWorkspaces,
                            isCollapsed: wsCollapsed
                        )
                        if !wsCollapsed {
                            if wsConvs.isEmpty {
                                emptyWorkspacePlaceholderRow(workspace: ws, project: project)
                            } else {
                                let scopedWsRows = wsConvs.map {
                                    ScopedConvRow(
                                        id: OrbitSidebarLogic.scopedRowId(
                                            section: "ws",
                                            hostId: $0.hostId,
                                            agentId: $0.agent.agentId,
                                            containerId: ws.workspaceId
                                        ),
                                        conv: $0,
                                        projectName: nil
                                    )
                                }
                                ForEach(scopedWsRows) { row in
                                    sessionRow(conv: row.conv, project: project, isInsideWorkspace: true)
                                }
                            }
                        }
                    }

                    // Unclassified (`Unassigned`) Inbox or Flat Conversations
                    if OrbitSidebarLogic.shouldShowLegacyInbox(itemCount: visibleUnclassified.count) {
                        if !curatedWorkspaces.isEmpty {
                            let inboxCollapsed = store.isInboxCollapsed(for: project)
                            unclassifiedInboxHeaderRow(
                                project: project,
                                count: visibleUnclassified.count,
                                isCollapsed: inboxCollapsed,
                                unclassifiedIds: visibleUnclassified.map(\.agent.agentId)
                            )
                            if !inboxCollapsed {
                                unclassifiedConversationSlice(
                                    project: project,
                                    conversations: visibleUnclassified,
                                    indented: true
                                )
                            }
                        } else {
                            unclassifiedConversationSlice(
                                project: project,
                                conversations: visibleUnclassified,
                                indented: false
                            )
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func unclassifiedConversationSlice(
        project: FfiSidebarProjectGroup,
        conversations: [FfiSidebarConversation],
        indented: Bool
    ) -> some View {
        let selectedIdx = conversations.firstIndex {
            !store.isDraftNewSession && store.selectedAgentId == $0.agent.agentId
        } ?? -1
        let isExpandedAll = expandedMoreProjectIds.contains(project.id)
            || selectedIdx >= Self.maxVisibleConversationsPerProject
        let hiddenCount = max(0, conversations.count - Self.maxVisibleConversationsPerProject)
        let sliced = isExpandedAll
            ? conversations
            : Array(conversations.prefix(Self.maxVisibleConversationsPerProject))
        let scopedSliced = sliced.map {
            ScopedConvRow(
                id: OrbitSidebarLogic.scopedRowId(
                    section: "project",
                    hostId: $0.hostId,
                    agentId: $0.agent.agentId,
                    containerId: project.id
                ),
                conv: $0,
                projectName: nil
            )
        }

        ForEach(scopedSliced) { row in
            sessionRow(conv: row.conv, project: project, isInsideWorkspace: indented)
        }

        if hiddenCount > 0 {
            showMoreLessRow(projectId: project.id, hiddenCount: hiddenCount, isExpandedAll: isExpandedAll)
        }
    }

    private var homeOverviewNavRow: some View {
        let isSelected = store.isShowingGlobalOverview
        let pendingCount = Int(store.totalPendingApprovals)
        return Button {
            store.openGlobalOverview()
        } label: {
            HStack(spacing: 10) {
                Image(systemName: "square.grid.2x2")
                    .font(.system(size: 13.5, weight: .medium))
                    .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgSecondary)
                    .frame(width: 16, height: 16)

                Text(OrbitL10n.tr("Overview"))
                    .font(OrbitTypography.subhead(isSelected ? .semibold : .medium))
                    .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgPrimary.opacity(0.9))

                Spacer(minLength: 4)

                if pendingCount > 0 {
                    Text("\(pendingCount)")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.warning)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(palette.warning.opacity(0.15), in: Capsule())
                }
            }
            .padding(.horizontal, 8)
            .frame(minHeight: 34)
            .background(
                RoundedRectangle(cornerRadius: 9, style: .continuous)
                    .fill(isSelected ? selectedRowFill : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .listRowInsets(EdgeInsets(top: 1, leading: 8, bottom: 1, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private var automationsNavRow: some View {
        Button {
            store.hapticLight()
            if isCompactOverlay {
                store.isDrawerOpen = false
            }
            store.activeSheet = .automations
        } label: {
            HStack(spacing: 10) {
                Image(systemName: "clock.arrow.circlepath")
                    .font(.system(size: 13.5, weight: .medium))
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: 16, height: 16)

                Text("Automations")
                    .font(OrbitTypography.subhead(.medium))
                    .foregroundStyle(palette.fgPrimary.opacity(0.9))

                Spacer(minLength: 4)

                if activeScheduleCount > 0 {
                    Text("\(activeScheduleCount)")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.direct)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(palette.direct.opacity(0.14), in: Capsule())
                }
            }
            .padding(.horizontal, 8)
            .frame(minHeight: 34)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .listRowInsets(EdgeInsets(top: 1, leading: 8, bottom: 2, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private func organizingProgressRow(status: FfiWorkspaceOrganizeStatus) -> some View {
        HStack(spacing: 8) {
            OrbitSpinnerRingView(color: palette.accentViolet, size: 11.5)
            Text(status.message.isEmpty ? "AI organizing workspaces…" : status.message)
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.accentViolet)
                .lineLimit(1)
            Spacer(minLength: 0)
        }
        .padding(.leading, 22)
        .padding(.trailing, 8)
        .padding(.vertical, 5)
        .background(palette.accentViolet.opacity(0.08), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        .listRowInsets(EdgeInsets(top: 1, leading: 10, bottom: 2, trailing: 10))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private func workspaceHeaderRow(
        workspace: FfiSidebarWorkspaceGroup,
        project: FfiSidebarProjectGroup,
        siblingWorkspaces: [FfiSidebarWorkspaceGroup],
        isCollapsed: Bool
    ) -> some View {
        let extraDirs = Array(project.directories.dropFirst())
        let otherWorkspaces = siblingWorkspaces.filter { $0.workspaceId != workspace.workspaceId }

        return HStack(spacing: 4) {
            Button {
                withAnimation(.easeInOut(duration: 0.16)) {
                    store.toggleWorkspaceCollapsed(workspace.workspaceId)
                }
            } label: {
                HStack(spacing: 7) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(palette.fgMuted)
                        .rotationEffect(.degrees(isCollapsed ? 0 : 90))
                        .frame(width: 10)

                    Image(systemName: "square.stack.3d.up")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(palette.accentViolet)

                    Text(workspace.title)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary.opacity(0.92))
                        .lineLimit(1)

                    if workspace.peerCount > 0 {
                        Text("\(workspace.peerCount)")
                            .font(.system(size: 10.5, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                    }

                    Spacer(minLength: 2)
                }
                .padding(.leading, 14)
                .padding(.vertical, 5)
                .frame(minHeight: 34)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if workspace.hasConflict {
                Circle().fill(palette.warning).frame(width: 6, height: 6)
            } else if workspace.runningCount > 0 {
                OrbitSpinnerRingView(color: palette.fgSecondary, size: 11)
            } else if workspace.unreadCount > 0 {
                Circle().fill(palette.accentSky).frame(width: 5.5, height: 5.5)
            }

            Button {
                if isCompactOverlay { store.isDrawerOpen = false }
                store.openWorkspaceBoard(workspaceId: workspace.workspaceId)
            } label: {
                Image(systemName: "doc.richtext")
                    .font(.system(size: 11.5, weight: .medium))
                    .foregroundStyle(palette.fgMuted)
                    .frame(width: 26, height: 30)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Open Board for \(workspace.title)")

            Button {
                store.startNewSessionDraft(
                    cwd: workspace.cwd,
                    extraDirs: extraDirs,
                    workspaceId: workspace.workspaceId
                )
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(palette.fgMuted)
                    .frame(width: 26, height: 30)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("New Session in \(workspace.title)")
        }
        .background(
            RoundedRectangle(cornerRadius: 9, style: .continuous)
                .fill(palette.accentViolet.opacity(palette.isDark ? 0.08 : 0.05))
        )
        .contextMenu {
            Button {
                store.startNewSessionDraft(
                    cwd: workspace.cwd,
                    extraDirs: extraDirs,
                    workspaceId: workspace.workspaceId
                )
            } label: {
                Label("New Conversation in Workspace", systemImage: "square.and.pencil")
            }

            Button {
                if isCompactOverlay { store.isDrawerOpen = false }
                store.openWorkspaceBoard(workspaceId: workspace.workspaceId)
            } label: {
                Label("Open Mission Board", systemImage: "doc.richtext")
            }

            Button {
                if isCompactOverlay { store.isDrawerOpen = false }
                store.activeSheet = .spawnPeer(workspaceId: workspace.workspaceId)
            } label: {
                Label("Add Workspace Peer", systemImage: "person.badge.plus")
            }

            if !project.hostId.isEmpty {
                Button {
                    store.renameTarget = .workspace(
                        hostId: project.hostId,
                        workspaceId: workspace.workspaceId,
                        currentTitle: workspace.title
                    )
                } label: {
                    Label("Rename Workspace", systemImage: "pencil")
                }
            }

            if !otherWorkspaces.isEmpty {
                Menu {
                    ForEach(otherWorkspaces) { target in
                        Button(target.title) {
                            store.mergeWorkspaces(
                                sourceWorkspaceId: workspace.workspaceId,
                                targetWorkspaceId: target.workspaceId
                            )
                        }
                    }
                } label: {
                    Label("Merge Into…", systemImage: "arrow.triangle.merge")
                }
            }

            Divider()

            Button {
                confirmArchiveWorkspace = (workspace: workspace, keepUnclassified: true)
            } label: {
                Label("Dissolve (Keep Sessions)", systemImage: "tray")
            }

            Button(role: .destructive) {
                confirmArchiveWorkspace = (workspace: workspace, keepUnclassified: false)
            } label: {
                Label("Archive Workspace & Sessions", systemImage: "archivebox")
            }
        }
        .confirmationDialog(
            confirmArchiveWorkspace?.keepUnclassified == true
                ? "Dissolve \"\(confirmArchiveWorkspace?.workspace.title ?? "")\"?"
                : "Archive \"\(confirmArchiveWorkspace?.workspace.title ?? "")\"?",
            isPresented: Binding(
                get: { confirmArchiveWorkspace != nil },
                set: { if !$0 { confirmArchiveWorkspace = nil } }
            ),
            titleVisibility: .visible
        ) {
            if let req = confirmArchiveWorkspace {
                Button(req.keepUnclassified ? "Dissolve Workspace" : "Archive Workspace & Sessions", role: .destructive) {
                    confirmArchiveWorkspace = nil
                    store.archiveWorkspace(req.workspace.workspaceId, keepAgentsUnclassified: req.keepUnclassified)
                }
            }
            Button(OrbitL10n.tr("Cancel"), role: .cancel) {
                confirmArchiveWorkspace = nil
            }
        } message: {
            Text(
                confirmArchiveWorkspace?.keepUnclassified == true
                    ? "Conversations will be kept in Unassigned."
                    : "All conversations in this workspace will be archived."
            )
        }
        .listRowInsets(EdgeInsets(top: 2, leading: 8, bottom: 1, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private func emptyWorkspacePlaceholderRow(
        workspace: FfiSidebarWorkspaceGroup,
        project: FfiSidebarProjectGroup
    ) -> some View {
        Button {
            store.startNewSessionDraft(
                cwd: workspace.cwd,
                extraDirs: Array(project.directories.dropFirst()),
                workspaceId: workspace.workspaceId
            )
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "plus.circle")
                    .font(.system(size: 11.5))
                Text("Start first conversation")
                    .font(OrbitTypography.meta(.regular))
                Spacer()
            }
            .foregroundStyle(palette.fgMuted)
            .padding(.leading, 32)
            .frame(minHeight: 30)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .listRowInsets(EdgeInsets(top: 0, leading: 8, bottom: 2, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private func unclassifiedInboxHeaderRow(
        project: FfiSidebarProjectGroup,
        count: Int,
        isCollapsed: Bool,
        unclassifiedIds: [String]
    ) -> some View {
        HStack(spacing: 4) {
            Button {
                withAnimation(.easeInOut(duration: 0.16)) {
                    store.toggleInboxCollapsed(projectId: project.id, currentlyCollapsed: isCollapsed)
                }
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(palette.fgMuted)
                        .rotationEffect(.degrees(isCollapsed ? 0 : 90))
                        .frame(width: 10)

                    Image(systemName: "tray")
                        .font(.system(size: 11.5, weight: .medium))
                        .foregroundStyle(palette.fgMuted)

                    Text("Unassigned (\(count))")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgMuted)

                    Spacer(minLength: 2)
                }
                .padding(.leading, 14)
                .frame(minHeight: 30)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            Button {
                store.organizeProjectWorkspaces(
                    projectRoot: project.primaryCwd,
                    agentIds: unclassifiedIds,
                    autoApply: true
                )
            } label: {
                HStack(spacing: 3) {
                    Image(systemName: "sparkles")
                        .font(.system(size: 10.5, weight: .semibold))
                    Text("AI Organize")
                        .font(.system(size: 11, weight: .medium))
                }
                .foregroundStyle(palette.accentViolet)
                .padding(.horizontal, 7)
                .frame(height: 24)
                .background(palette.accentViolet.opacity(0.12), in: Capsule())
            }
            .buttonStyle(.plain)
        }
        .listRowInsets(EdgeInsets(top: 2, leading: 8, bottom: 1, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    // MARK: - Section & Project Headers

    private func sectionHeaderRow(
        _ title: String,
        count: Int? = nil,
        accentColor: Color? = nil,
        topPadding: CGFloat = 6
    ) -> some View {
        HStack(spacing: OrbitSpacing.xs) {
            Text(title)
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(accentColor ?? palette.fgMuted)

            if let count = count, count > 0 {
                Text("\(count)")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle((accentColor ?? palette.fgMuted).opacity(0.85))
            }

            Spacer()
        }
        .padding(.horizontal, 8)
        .padding(.top, topPadding)
        .padding(.bottom, 2)
        .listRowInsets(EdgeInsets(top: 0, leading: 8, bottom: 0, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private func projectsSectionHeaderRow(topPadding: CGFloat) -> some View {
        HStack(spacing: 4) {
            Text("Projects")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)

            Spacer()

            Button {
                store.hapticLight()
                isSortFilterPopoverPresented.toggle()
            } label: {
                ZStack(alignment: .topTrailing) {
                    Image(systemName: "line.3.horizontal.decrease")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(isFilterActive ? palette.fgPrimary : palette.fgMuted)
                        .frame(width: 28, height: 28)
                        .contentShape(Rectangle())

                    if isFilterActive {
                        Circle()
                            .fill(Color(hex: "#0EA5E9"))
                            .frame(width: 4.5, height: 4.5)
                            .offset(x: -5, y: 5)
                    }
                }
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Sort and Filter Projects")
            .orbitCompactPopover(
                isPresented: $isSortFilterPopoverPresented,
                arrowEdge: .top,
                palette: palette
            ) {
                VStack(alignment: .leading, spacing: 6) {
                    OrbitPopoverSectionHeader(title: "Sort Projects")

                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(OrbitSidebarProjectSort.allCases) { option in
                            OrbitPopoverOptionRow(
                                title: option.label,
                                isSelected: store.sidebarProjectSort == option
                            ) {
                                store.hapticLight()
                                store.sidebarProjectSort = option
                                isSortFilterPopoverPresented = false
                            }
                        }
                    }

                    OrbitHairline()
                        .padding(.horizontal, 10)
                        .padding(.vertical, 2)

                    OrbitPopoverSectionHeader(title: "Show Conversations")

                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(OrbitSidebarThreadFilter.allCases) { option in
                            OrbitPopoverOptionRow(
                                title: option.label,
                                isSelected: store.sidebarThreadFilter == option
                            ) {
                                store.hapticLight()
                                store.sidebarThreadFilter = option
                                isSortFilterPopoverPresented = false
                            }
                        }
                    }
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 10)
                .frame(width: 216)
            }

            Button {
                store.hapticLight()
                if isCompactOverlay {
                    store.isDrawerOpen = false
                }
                store.openDirectoryBrowser()
            } label: {
                Image(systemName: "folder.badge.plus")
                    .font(.system(size: 13.5, weight: .medium))
                    .foregroundStyle(palette.fgMuted)
                    .frame(width: 28, height: 28)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Open Folder")
        }
        .padding(.leading, 8)
        .padding(.trailing, 2)
        .padding(.top, topPadding)
        .padding(.bottom, 2)
        .listRowInsets(EdgeInsets(top: 0, leading: 8, bottom: 0, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private func projectHeaderRow(
        project: FfiSidebarProjectGroup,
        projectIndex: Int,
        totalProjects: Int,
        isCollapsed: Bool,
        hostId: String
    ) -> some View {
        let conversations = project.conversations
        let extraDirs = Array(project.directories.dropFirst())
        let hasCollapsedApproval = isCollapsed && conversations.contains { $0.agent.isAwaitingPermission }
        let hasCollapsedRunning = isCollapsed && !hasCollapsedApproval && conversations.contains { $0.agent.isForegroundRunning }
        let hasCollapsedUnread = isCollapsed
            && !hasCollapsedApproval
            && !hasCollapsedRunning
            && conversations.contains { $0.isUnread }
        let isActiveDraft = store.isDraftNewSession && project.directories.contains {
            OrbitFormatters.arePathsEquivalent($0, store.activeCwd)
        }

        return HStack(spacing: 4) {
            Button {
                withAnimation(.easeInOut(duration: 0.16)) {
                    store.toggleProjectCollapsed(project.id)
                }
            } label: {
                HStack(spacing: 7) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9.5, weight: .semibold))
                        .foregroundStyle(palette.fgMuted)
                        .rotationEffect(.degrees(isCollapsed ? 0 : 90))
                        .frame(width: 10)

                    Image(systemName: isCollapsed ? "folder" : "folder.fill")
                        .font(.system(size: 13.5, weight: .medium))
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 16, height: 16)

                    Text(project.name)
                        .font(OrbitTypography.body(.medium))
                        .foregroundStyle(palette.fgPrimary.opacity(0.92))
                        .lineLimit(1)

                    if store.hosts.count > 1,
                       let hostLabel = store.hosts.first(where: { $0.hostId == hostId })?.displayName,
                       !hostLabel.isEmpty {
                        Text(hostLabel)
                            .font(.system(size: 10, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 1.5)
                            .background(palette.subtleFill, in: Capsule())
                            .lineLimit(1)
                    }

                    Spacer(minLength: 4)
                }
                .padding(.leading, 8)
                .padding(.vertical, 6)
                .frame(minHeight: 38)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if hasCollapsedApproval {
                Circle()
                    .fill(palette.warning)
                    .frame(width: 6, height: 6)
                    .padding(.trailing, 2)
            } else if hasCollapsedRunning {
                OrbitSpinnerRingView(color: palette.fgSecondary, size: 12)
                    .padding(.trailing, 2)
            } else if hasCollapsedUnread {
                Circle()
                    .fill(palette.accentSky)
                    .frame(width: 6, height: 6)
                    .padding(.trailing, 2)
            }

            Button {
                store.startNewSessionDraft(cwd: project.primaryCwd, extraDirs: extraDirs)
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 13.5, weight: .medium))
                    .foregroundStyle(palette.fgMuted)
                    .frame(width: 32, height: 34)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("New Conversation in \(project.name)")
        }
        .padding(.trailing, 0)
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(isActiveDraft ? selectedRowFill.opacity(0.55) : Color.clear)
        )
        .contextMenu {
            Button {
                store.startNewSessionDraft(cwd: project.primaryCwd, extraDirs: extraDirs)
            } label: {
                Label("New Conversation", systemImage: "square.and.pencil")
            }

            Button {
                if isCompactOverlay { store.isDrawerOpen = false }
                store.activeSheet = .createWorkspace(
                    projectRoot: project.primaryCwd,
                    projectName: project.name,
                    isGitRepo: project.isGitRepo,
                    moveAgentId: nil
                )
            } label: {
                Label("New Mission Workspace", systemImage: "square.stack.3d.up")
            }

            Button {
                store.organizeProjectWorkspaces(
                    projectRoot: project.primaryCwd,
                    agentIds: project.unclassifiedConversations.map(\.agent.agentId),
                    autoApply: true
                )
            } label: {
                Label("AI Organize Workspaces", systemImage: "sparkles")
            }

            if projectIndex > 0 {
                Button {
                    store.moveProject(sortOrderKey: project.sortOrderKey, offset: -1, within: displayProjects)
                } label: {
                    Label("Move Project Up", systemImage: "arrow.up")
                }
            }

            if projectIndex < totalProjects - 1 {
                Button {
                    store.moveProject(sortOrderKey: project.sortOrderKey, offset: 1, within: displayProjects)
                } label: {
                    Label("Move Project Down", systemImage: "arrow.down")
                }
            }

            if let pinnedId = project.pinnedProjectId {
                Button {
                    store.renameTarget = .project(projectId: pinnedId, currentName: project.name)
                } label: {
                    Label("Rename Project", systemImage: "pencil")
                }

                Button {
                    store.unpinProject(pinnedId)
                } label: {
                    Label("Unpin Project", systemImage: "pin.slash")
                }

                Button(role: .destructive) {
                    store.deleteProject(pinnedId)
                } label: {
                    Label("Remove Project", systemImage: "trash")
                }
            } else {
                Button {
                    store.pinProject(name: project.name, directories: [project.primaryCwd])
                } label: {
                    Label("Pin Project", systemImage: "pin")
                }
            }

            Button {
                if isCompactOverlay { store.isDrawerOpen = false }
                store.selectWorkspaceDirectory(project.primaryCwd)
                store.openWorkspaceFiles()
            } label: {
                Label("Files Explorer", systemImage: "folder")
            }

            Button {
                store.inspectGitDiff(inCwd: project.primaryCwd)
            } label: {
                Label("Diff Review", systemImage: "arrow.left.arrow.right")
            }

            Button {
                store.openTerminal(inCwd: project.primaryCwd)
            } label: {
                Label("Terminal", systemImage: "terminal")
            }

            Button {
                store.copyPathToClipboard(project.primaryCwd)
            } label: {
                Label("Copy Path", systemImage: "square.on.square")
            }
        }
        .listRowInsets(EdgeInsets(top: 1, leading: 8, bottom: 1, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    // MARK: - Single-Line Rows (Attention / Conversation / Show More)

    private func attentionRow(conv: FfiSidebarConversation, projectName: String) -> some View {
        let agent = conv.agent
        let isSelected = !store.isDraftNewSession && store.selectedHostId == conv.hostId && store.selectedAgentId == agent.agentId
        return Button {
            store.selectAgent(hostId: conv.hostId, agentId: agent.agentId)
        } label: {
            HStack(spacing: 10) {
                Circle()
                    .fill(palette.warning)
                    .frame(width: 6.5, height: 6.5)
                    .frame(width: 16, height: 16)

                Text(agent.title.isEmpty ? "New Conversation" : agent.title)
                    .font(OrbitTypography.body(.medium))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)

                Spacer(minLength: OrbitSpacing.xs)

                if store.hosts.count > 1, let hostName = hostDisplayName(for: conv.hostId), conv.hostId != store.selectedHostId {
                    Text(hostName)
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(palette.fgMuted)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(softSurfaceFill, in: Capsule())
                        .lineLimit(1)
                }

                Text(projectName)
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted.opacity(0.75))
                    .lineLimit(1)
                    .frame(maxWidth: 84, alignment: .trailing)
            }
            .padding(.horizontal, 8)
            .frame(minHeight: 40)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(isSelected ? selectedRowFill : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .listRowInsets(EdgeInsets(top: 1, leading: 8, bottom: 1, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private func sessionRow(
        conv: FfiSidebarConversation,
        project: FfiSidebarProjectGroup? = nil,
        isInsideWorkspace: Bool = false,
        trailingTag: String? = nil
    ) -> some View {
        let hostId = conv.hostId
        let agent = conv.agent
        let isWorktree = conv.isWorktree
        let isSelected = !store.isDraftNewSession && store.selectedHostId == hostId && store.selectedAgentId == agent.agentId
        let isPinned = conv.isPinned || store.isThreadPinned(hostId: hostId, agentId: agent.agentId)
        let isUnread = conv.isUnread && !isSelected && !agent.isForegroundRunning
        let titleWeight: Font.Weight = (isSelected || isUnread) ? .medium : .regular
        let titleColor = (isSelected || isUnread) ? palette.fgPrimary : mutedRowForeground
        let rightLabel = trailingTag
            ?? (agent.isForegroundRunning ? "now" : OrbitFormatters.relativeUpdatedTime(rfc3339: agent.updatedAtRfc3339))
        let semanticHandle: String? = {
            guard isInsideWorkspace,
                  let raw = agent.handle?.trimmingCharacters(in: .whitespacesAndNewlines),
                  !raw.isEmpty,
                  !OrbitFormatters.isGenericHarnessHandle(raw) else {
                return nil
            }
            return raw.hasPrefix("@") ? raw : "@\(raw)"
        }()
        let curatedWorkspaces = (project?.workspaces ?? []).filter { $0.isCurated || $0.isMultiAgent }
        let effectiveProjectRoot = project?.primaryCwd ?? agent.cwd

        return Button {
            store.selectAgent(hostId: hostId, agentId: agent.agentId)
        } label: {
            HStack(spacing: 10) {
                sessionStatusGutter(agent: agent, isUnread: isUnread, isWorktree: isWorktree)
                    .frame(width: 16, height: 16)

                Text(agent.title.isEmpty ? "New Conversation" : agent.title)
                    .font(OrbitTypography.body(titleWeight))
                    .foregroundStyle(titleColor)
                    .lineLimit(1)

                if let handleTag = semanticHandle {
                    Text(handleTag)
                        .font(.system(size: 10.5, weight: .medium))
                        .foregroundStyle(palette.accentViolet)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(palette.accentViolet.opacity(0.12), in: Capsule())
                        .lineLimit(1)
                }

                if store.hosts.count > 1, let hostName = hostDisplayName(for: hostId), hostId != store.selectedHostId {
                    Text(hostName)
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(palette.fgMuted)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(softSurfaceFill, in: Capsule())
                        .lineLimit(1)
                }

                Spacer(minLength: OrbitSpacing.xs)

                if !rightLabel.isEmpty {
                    Text(rightLabel)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted.opacity(0.75))
                        .lineLimit(1)
                }
            }
            .padding(.leading, isInsideWorkspace ? 20 : 8)
            .padding(.trailing, 8)
            .frame(minHeight: 40)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(isSelected ? selectedRowFill : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .contextMenu {
            if !hostId.isEmpty {
                Button {
                    store.renameTarget = .agent(
                        hostId: hostId,
                        agentId: agent.agentId,
                        currentTitle: agent.title,
                        currentHandle: agent.handle
                    )
                } label: {
                    Label("Rename", systemImage: "pencil")
                }

                Button {
                    store.organizeProjectWorkspaces(
                        projectRoot: effectiveProjectRoot,
                        agentIds: [agent.agentId],
                        autoApply: true
                    )
                } label: {
                    Label("AI Classify Session", systemImage: "sparkles")
                }

                Button {
                    store.summarizeAgent(agent.agentId)
                } label: {
                    Label("AI Update Summary", systemImage: "text.badge.star")
                }

                Menu {
                    ForEach(curatedWorkspaces) { ws in
                        Button {
                            store.moveAgentToWorkspace(
                                agentId: agent.agentId,
                                targetWorkspaceId: ws.workspaceId
                            )
                        } label: {
                            Label(
                                ws.title,
                                systemImage: ws.workspaceId == agent.workspaceId ? "checkmark" : "square.stack.3d.up"
                            )
                        }
                    }
                    Button {
                        if isCompactOverlay { store.isDrawerOpen = false }
                        store.activeSheet = .createWorkspace(
                            projectRoot: effectiveProjectRoot,
                            projectName: project?.name ?? OrbitFormatters.shortPath(effectiveProjectRoot),
                            isGitRepo: project?.isGitRepo ?? false,
                            moveAgentId: agent.agentId
                        )
                    } label: {
                        Label("New Workspace…", systemImage: "plus.rectangle.on.folder")
                    }
                    Button {
                        store.moveAgentToWorkspace(
                            agentId: agent.agentId,
                            targetWorkspaceId: nil,
                            makeUnclassified: true
                        )
                    } label: {
                        Label("Move to Unassigned", systemImage: "tray")
                    }
                } label: {
                    Label("Move to Workspace", systemImage: "folder.badge.gearshape")
                }

                Divider()

                Button {
                    store.togglePinThread(hostId: hostId, agentId: agent.agentId)
                } label: {
                    Label(isPinned ? "Unpin" : "Pin", systemImage: isPinned ? "pin.slash" : "pin")
                }

                if isUnread {
                    Button {
                        store.markAgentRead(hostId: hostId, agentId: agent.agentId)
                    } label: {
                        Label("Mark Read", systemImage: "envelope.open")
                    }
                } else if !isSelected {
                    Button {
                        store.markAgentUnread(hostId: hostId, agentId: agent.agentId)
                    } label: {
                        Label("Mark Unread", systemImage: "envelope.badge")
                    }
                }
            }

            Button {
                store.startNewSessionDraft(cwd: agent.cwd, provider: agent.provider)
            } label: {
                Label("New Conversation Here", systemImage: "square.and.pencil")
            }

            Button {
                store.openTerminal(inCwd: agent.cwd)
            } label: {
                Label("Terminal", systemImage: "terminal")
            }

            Button {
                store.copyThreadTranscript(hostId: hostId, agentId: agent.agentId)
            } label: {
                Label("Copy Transcript", systemImage: "doc.on.doc")
            }

            Button {
                store.copyPathToClipboard(agent.cwd)
            } label: {
                Label("Copy Path", systemImage: "square.on.square")
            }

            Button {
                UIPasteboard.general.string = agent.agentId
                store.hapticLight()
                store.showToast("Copied thread ID")
            } label: {
                Label("Copy Thread ID", systemImage: "number")
            }

            if !agent.workspaceId.isEmpty || agent.parentAgentId != nil {
                Button {
                    store.detachAgent(agent.agentId, hostId: hostId)
                } label: {
                    Label("Detach Session", systemImage: "arrow.up.right.square")
                }
            }

            Divider()

            Button(role: .destructive) {
                store.archiveSession(agent.agentId, hostId: hostId)
            } label: {
                Label {
                    Text("Archive")
                } icon: {
                    Image(systemName: "archivebox")
                        .symbolRenderingMode(.palette)
                        .foregroundStyle(Color(uiColor: .systemRed), Color(uiColor: .systemRed))
                }
            }
            .tint(nil)

            Button(role: .destructive) {
                store.deleteAgentPermanently(agent.agentId, hostId: hostId)
            } label: {
                Label("Delete Permanently", systemImage: "trash")
            }
        }
        .swipeActions(edge: .leading, allowsFullSwipe: false) {
            Button {
                store.togglePinThread(hostId: hostId, agentId: agent.agentId)
            } label: {
                Label(isPinned ? "Unpin" : "Pin", systemImage: isPinned ? "pin.slash" : "pin")
            }
            .tint(palette.primary)

            if isUnread {
                Button {
                    store.markAgentRead(hostId: hostId, agentId: agent.agentId)
                } label: {
                    Label("Mark Read", systemImage: "envelope.open")
                }
                .tint(palette.accentSky)
            } else if !isSelected {
                Button {
                    store.markAgentUnread(hostId: hostId, agentId: agent.agentId)
                } label: {
                    Label("Mark Unread", systemImage: "envelope.badge")
                }
                .tint(palette.accentSky)
            }
        }
        .swipeActions(edge: .trailing, allowsFullSwipe: false) {
            Button(role: .destructive) {
                store.archiveSession(agent.agentId, hostId: hostId)
            } label: {
                Label("Archive", systemImage: "archivebox")
            }
        }
        .listRowInsets(EdgeInsets(top: 1, leading: 8, bottom: 1, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    @ViewBuilder
    private func sessionStatusGutter(agent: FfiAgentCard, isUnread: Bool, isWorktree: Bool) -> some View {
        if agent.isAwaitingPermission {
            Circle()
                .fill(palette.warning)
                .frame(width: 6.5, height: 6.5)
        } else if agent.isForegroundRunning {
            OrbitSpinnerRingView(color: palette.fgSecondary, size: 12.5)
        } else if isUnread {
            Circle()
                .fill(Color(hex: "#0EA5E9"))
                .frame(width: 6, height: 6)
        } else if agent.isBackgroundWaiting {
            Image(systemName: "timer")
                .font(.system(size: 11.5, weight: .medium))
                .foregroundStyle(palette.fgMuted.opacity(0.8))
        } else if isWorktree {
            Image(systemName: "arrow.triangle.branch")
                .font(.system(size: 11.5, weight: .medium))
                .foregroundStyle(palette.fgMuted.opacity(0.65))
        } else {
            Color.clear
        }
    }

    private func showMoreLessRow(projectId: String, hiddenCount: Int, isExpandedAll: Bool) -> some View {
        Button {
            store.hapticLight()
            withAnimation(.easeInOut(duration: 0.16)) {
                if isExpandedAll {
                    expandedMoreProjectIds.remove(projectId)
                } else {
                    expandedMoreProjectIds.insert(projectId)
                }
            }
        } label: {
            HStack(spacing: 10) {
                Color.clear
                    .frame(width: 16, height: 16)

                Text(isExpandedAll ? "Show less" : "Show \(hiddenCount) more…")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgMuted.opacity(0.80))

                Spacer()
            }
            .padding(.horizontal, 8)
            .frame(minHeight: 32)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .listRowInsets(EdgeInsets(top: 1, leading: 8, bottom: 2, trailing: 8))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    // MARK: - Bottom Bar: Thumb-Zone Host Switcher + Settings Circle

    private var drawerFooterBar: some View {
        HStack(spacing: OrbitSpacing.sm) {
            if let host = store.activeHost {
                Button {
                    store.hapticLight()
                    isHostPickerPresented.toggle()
                } label: {
                    HStack(spacing: OrbitSpacing.xs + 2) {
                        Circle()
                            .fill(palette.connectionColor(host.connectionState))
                            .frame(width: 7, height: 7)

                        Text(host.displayName)
                            .font(OrbitTypography.subhead(.medium))
                            .foregroundStyle(palette.fgPrimary)
                            .lineLimit(1)

                        Text(hostCompactDetailLabel(for: host))
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(1)

                        Spacer(minLength: 4)

                        if store.otherHostsPendingApprovals > 0 {
                            Text("\(store.otherHostsPendingApprovals)")
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(palette.warning)
                                .padding(.horizontal, 6)
                                .padding(.vertical, 1)
                                .background(palette.warning.opacity(0.16), in: Capsule())
                        }

                        Image(systemName: "chevron.up.chevron.down")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(palette.fgMuted)
                    }
                    .padding(.horizontal, 12)
                    .frame(maxWidth: .infinity, minHeight: 38, maxHeight: 38)
                    .background(softSurfaceFill, in: Capsule())
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Switch Host: \(host.displayName)")
                .orbitCompactPopover(
                    isPresented: $isHostPickerPresented,
                    arrowEdge: .bottom,
                    palette: palette
                ) {
                    OrbitHostPickerPopover(
                        store: store,
                        showPairingActions: true,
                        onSelectHost: { hostId in
                            store.hapticLight()
                            store.selectHost(hostId)
                            isHostPickerPresented = false
                        },
                        onPairNewHost: {
                            store.hapticLight()
                            isHostPickerPresented = false
                            if isCompactOverlay {
                                store.isDrawerOpen = false
                            }
                            store.presentSheetSafely(.qrPair)
                        },
                        onConnectLocalHost: {
                            isHostPickerPresented = false
                            _ = store.connectLocalSimulatorHost()
                        }
                    )
                }
            } else {
                Button {
                    store.hapticLight()
                    if isCompactOverlay {
                        store.isDrawerOpen = false
                    }
                    store.activeSheet = .qrPair
                } label: {
                    HStack(spacing: OrbitSpacing.xs) {
                        Image(systemName: "qrcode.viewfinder")
                            .font(OrbitTypography.iconSmall)
                        Text("Pair Host")
                            .font(OrbitTypography.subhead(.medium))
                    }
                    .foregroundStyle(palette.fgPrimary)
                    .frame(maxWidth: .infinity, minHeight: 38, maxHeight: 38)
                    .background(softSurfaceFill, in: Capsule())
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
            }

            if archivedCount > 0 {
                Button {
                    store.hapticLight()
                    if isCompactOverlay {
                        store.isDrawerOpen = false
                    }
                    store.activeSheet = .archivedThreads
                } label: {
                    ZStack(alignment: .topTrailing) {
                        Image(systemName: "archivebox")
                            .font(.system(size: 14.5, weight: .medium))
                            .foregroundStyle(palette.fgSecondary)
                            .frame(width: 38, height: 38)
                            .background(softSurfaceFill, in: Circle())
                            .contentShape(Circle())

                        Text("\(min(archivedCount, 99))")
                            .font(.system(size: 9.5, weight: .bold))
                            .foregroundStyle(palette.onPrimary)
                            .padding(.horizontal, 4)
                            .frame(minWidth: 16, minHeight: 16)
                            .background(palette.fgMuted, in: Capsule())
                            .offset(x: 3, y: -2)
                    }
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Archived Conversations (\(archivedCount))")
            }

            Button {
                store.hapticLight()
                if isCompactOverlay {
                    store.isDrawerOpen = false
                }
                store.refreshQuotas()
                store.activeSheet = .settings
            } label: {
                ZStack(alignment: .topTrailing) {
                    Image(systemName: "gearshape")
                        .font(.system(size: 15.5, weight: .medium))
                        .foregroundStyle(palette.fgPrimary)
                        .frame(width: 38, height: 38)
                        .background(softSurfaceFill, in: Circle())
                        .contentShape(Circle())

                    if !store.outdatedOnlineHosts.isEmpty {
                        Circle()
                            .fill(palette.accentSky)
                            .frame(width: 7.5, height: 7.5)
                            .offset(x: -2, y: 2)
                    }
                }
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Settings")
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.top, 6)
        .padding(.bottom, OrbitSpacing.sm + 4)
    }

    private func hostCompactDetailLabel(for host: FfiHostCard) -> String {
        let route = OrbitFormatters.compactTopBarConnectionLabel(for: host)
        if let ping = host.lastPingMs {
            return "\(route) · \(ping)ms"
        }
        return route
    }

    // MARK: - Sidebar Helpers

    private func projectForCwd(_ cwd: String) -> FfiSidebarProjectGroup? {
        hostProjects.first(where: { grp in
            grp.directories.contains { OrbitFormatters.arePathsEquivalent($0, cwd) }
                || OrbitFormatters.arePathsEquivalent(grp.primaryCwd, cwd)
        })
    }

    private func projectDisplayName(forCwd cwd: String) -> String {
        if let matched = projectForCwd(cwd) {
            return matched.name
        }
        return OrbitFormatters.shortPath(cwd)
    }
}

struct OrbitHostPickerPopover: View {
    @ObservedObject var store: OrbitIosStore
    let showPairingActions: Bool
    let onSelectHost: (String) -> Void
    var onPairNewHost: (() -> Void)? = nil
    var onConnectLocalHost: (() -> Void)? = nil

    @Environment(\.orbitPalette) private var palette

    var body: some View {
        let hostCount = max(1, store.hosts.count)
        let listHeight = min(CGFloat(hostCount) * 48 + 8, 220)
        return VStack(alignment: .leading, spacing: 6) {
            OrbitPopoverSectionHeader(title: OrbitL10n.tr("Hosts"))
                .padding(.horizontal, 10)

            ScrollView(.vertical, showsIndicators: true) {
                hostRowsStack
            }
            .frame(height: listHeight)

            if showPairingActions {
                OrbitHairline()
                    .padding(.horizontal, 20)
                    .padding(.vertical, 2)

                VStack(alignment: .leading, spacing: 2) {
                    OrbitPopoverActionRow(
                        title: OrbitL10n.tr("Pair New Host"),
                        systemImage: "qrcode.viewfinder"
                    ) {
                        onPairNewHost?()
                    }

                    if store.localDaemonUri != nil {
                        OrbitPopoverActionRow(
                            title: OrbitL10n.tr("Connect Local Host"),
                            systemImage: "bolt.horizontal.circle"
                        ) {
                            onConnectLocalHost?()
                        }
                    }
                }
                .padding(.horizontal, 10)
            }
        }
        .padding(.vertical, 10)
        .frame(width: 268)
        .fixedSize(horizontal: false, vertical: true)
    }

    private var duplicateDisplayNames: Set<String> {
        var counts: [String: Int] = [:]
        for h in store.hosts { counts[h.displayName, default: 0] += 1 }
        return Set(counts.filter { $0.value > 1 }.keys)
    }

    private var hostRowsStack: some View {
        let dupNames = duplicateDisplayNames
        return VStack(alignment: .leading, spacing: 2) {
            ForEach(store.hosts, id: \.hostId) { h in
                let route = OrbitFormatters.compactTopBarConnectionLabel(for: h)
                let baseDetail = h.lastPingMs.map { "\(route) · \($0)ms" } ?? route
                let disambig = dupNames.contains(h.displayName)
                    ? OrbitEndpointLogic.disambiguationSuffix(
                        hostId: h.hostId,
                        remotePort: h.remoteDaemonPort,
                        directEndpoints: h.directEndpoints
                    )
                    : ""
                let detail = disambig.isEmpty ? baseDetail : "\(baseDetail) · \(disambig)"
                let isSelected = h.hostId == store.selectedHostId

                OrbitPopoverOptionRow(
                    title: h.displayName,
                    subtitle: detail,
                    isSelected: isSelected,
                    action: {
                        onSelectHost(h.hostId)
                    },
                    leading: {
                        Circle()
                            .fill(palette.connectionColor(h.connectionState))
                            .frame(width: 7, height: 7)
                            .frame(width: 16, alignment: .center)
                    },
                    trailingBadge: {
                        if h.pendingApprovalCount > 0 {
                            Text("\(h.pendingApprovalCount)")
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(palette.warning)
                                .padding(.horizontal, 6)
                                .padding(.vertical, 1)
                                .background(palette.warning.opacity(0.16), in: Capsule())
                        }
                    }
                )
            }
        }
        .padding(.horizontal, 10)
    }
}
