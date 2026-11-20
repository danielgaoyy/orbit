import SwiftUI
import UIKit

private struct ProjectConversationItem: Identifiable {
    let agent: FfiAgentCard
    let isWorktree: Bool

    var id: String { agent.agentId }
}

private struct UnifiedMobileProject: Identifiable {
    let id: String
    let name: String
    let primaryCwd: String
    let extraDirs: [String]
    let directories: [String]
    let gitBranch: String?
    let pinnedProject: FfiClientProject?
    var conversations: [ProjectConversationItem]
    var sortWeight: TimeInterval
}

struct FleetDrawerView: View {
    private static let maxVisibleConversationsPerProject = 5

    @ObservedObject var store: OrbitIosStore
    let isCompactOverlay: Bool

    @Environment(\.orbitPalette) private var palette
    @State private var expandedMoreProjectIds: Set<String> = []
    @State private var isSortFilterPopoverPresented: Bool = false
    @State private var isHostPickerPresented: Bool = false

    private var allAgents: [FfiAgentCard] {
        store.currentHostDetail?.agents ?? []
    }

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

    // MARK: - Unified Projects Builder (1:1 Desktop `buildUnifiedProjects`)

    private var unifiedProjects: [UnifiedMobileProject] {
        guard let hostId = store.selectedHostId else { return [] }
        let detail = store.currentHostDetail
        var entries: [UnifiedMobileProject] = []
        var indexById: [String: Int] = [:]

        // 1. User-pinned projects first
        for proj in store.hostPinnedProjects {
            let primary = normalizeProjectPath(proj.directories.first ?? "~")
            let normalizedDirs = proj.directories.isEmpty
                ? [primary]
                : proj.directories.map(normalizeProjectPath)
            let extra = Array(proj.directories.dropFirst())
            let entry = UnifiedMobileProject(
                id: proj.projectId,
                name: proj.name.isEmpty ? OrbitFormatters.shortPath(primary) : proj.name,
                primaryCwd: primary,
                extraDirs: extra,
                directories: normalizedDirs,
                gitBranch: branchForDirectory(primary),
                pinnedProject: proj,
                conversations: [],
                sortWeight: TimeInterval(proj.updatedAtMs) / 1000.0
            )
            indexById[entry.id] = entries.count
            entries.append(entry)
        }

        // 2. Group active/recent agents under their parent Project (nesting worktrees back into base repo)
        let wsWorktreeByCwd: [String: Bool] = {
            var map: [String: Bool] = [:]
            for ws in detail?.workspaces ?? [] {
                map[normalizeProjectPath(ws.cwd)] = ws.isWorktree
            }
            return map
        }()

        for ag in allAgents {
            let cleanCwd = normalizeProjectPath(ag.cwd)
            let isWt = wsWorktreeByCwd[cleanCwd] ?? cleanCwd.contains("/worktrees/")
            var matchedIndex = entries.firstIndex {
                matchesProjectDirectory($0.directories, cwd: cleanCwd)
            }

            if matchedIndex == nil && isWt {
                let candidateRepo = detail?.workspaces.first {
                    !$0.isWorktree && matchesProjectDirectory([$0.cwd], cwd: cleanCwd)
                }?.cwd
                    ?? detail?.discoveredProjects.first {
                        matchesProjectDirectory([$0.path], cwd: cleanCwd)
                    }?.path

                if let rawRepo = candidateRepo {
                    let cleanRepo = normalizeProjectPath(rawRepo)
                    let projId = "\(hostId)::\(cleanRepo)"
                    if let existingIdx = indexById[projId] {
                        matchedIndex = existingIdx
                    } else {
                        let newEntry = UnifiedMobileProject(
                            id: projId,
                            name: OrbitFormatters.shortPath(cleanRepo),
                            primaryCwd: cleanRepo,
                            extraDirs: [],
                            directories: [cleanRepo],
                            gitBranch: branchForDirectory(cleanRepo),
                            pinnedProject: nil,
                            conversations: [],
                            sortWeight: 0
                        )
                        indexById[projId] = entries.count
                        entries.append(newEntry)
                        matchedIndex = entries.count - 1
                    }
                }
            }

            if matchedIndex == nil {
                let projId = "\(hostId)::\(cleanCwd)"
                if let existingIdx = indexById[projId] {
                    matchedIndex = existingIdx
                } else {
                    let newEntry = UnifiedMobileProject(
                        id: projId,
                        name: OrbitFormatters.shortPath(cleanCwd),
                        primaryCwd: cleanCwd,
                        extraDirs: [],
                        directories: [cleanCwd],
                        gitBranch: branchForDirectory(cleanCwd),
                        pinnedProject: nil,
                        conversations: [],
                        sortWeight: 0
                    )
                    indexById[projId] = entries.count
                    entries.append(newEntry)
                    matchedIndex = entries.count - 1
                }
            }

            if let idx = matchedIndex {
                entries[idx].conversations.append(ProjectConversationItem(agent: ag, isWorktree: isWt))
                let ts = parseUpdatedTimestamp(ag.updatedAtRfc3339)
                if ts > entries[idx].sortWeight {
                    entries[idx].sortWeight = ts
                }
            }
        }

        // 3. If host has neither pinned projects nor conversations yet, surface top 4 discovered repos
        if entries.isEmpty, let detail = detail {
            var count = 0
            for dp in detail.discoveredProjects where count < 4 {
                let cleanPath = normalizeProjectPath(dp.path)
                if isBareHomeDir(cleanPath) && detail.discoveredProjects.count > 1 {
                    continue
                }
                let projId = "\(hostId)::\(cleanPath)"
                guard indexById[projId] == nil else { continue }
                let entry = UnifiedMobileProject(
                    id: projId,
                    name: dp.name.isEmpty ? OrbitFormatters.shortPath(cleanPath) : dp.name,
                    primaryCwd: cleanPath,
                    extraDirs: [],
                    directories: [cleanPath],
                    gitBranch: dp.gitBranch,
                    pinnedProject: nil,
                    conversations: [],
                    sortWeight: TimeInterval(dp.lastModifiedUnix)
                )
                indexById[projId] = entries.count
                entries.append(entry)
                count += 1
            }
        }

        // Keep conversations within each project newest-first
        for i in entries.indices {
            entries[i].conversations.sort { lhs, rhs in
                let lTs = parseUpdatedTimestamp(lhs.agent.updatedAtRfc3339)
                let rTs = parseUpdatedTimestamp(rhs.agent.updatedAtRfc3339)
                if lTs != rTs { return lTs > rTs }
                return lhs.agent.latestSeq > rhs.agent.latestSeq
            }
        }

        switch store.sidebarProjectSort {
        case .alphabetical:
            entries.sort { lhs, rhs in
                lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
            }
        case .recent:
            entries.sort { lhs, rhs in
                let lHome = isBareHomeDir(lhs.primaryCwd) ? 1 : 0
                let rHome = isBareHomeDir(rhs.primaryCwd) ? 1 : 0
                if lHome != rHome { return lHome < rHome }
                let lHas = lhs.conversations.isEmpty ? 0 : 1
                let rHas = rhs.conversations.isEmpty ? 0 : 1
                if lHas != rHas { return lHas > rHas }
                if lhs.sortWeight != rhs.sortWeight { return lhs.sortWeight > rhs.sortWeight }
                return lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
            }
        }

        return entries
    }

    private var needsAttentionItems: [(agent: FfiAgentCard, projectName: String)] {
        allAgents
            .filter(\.isAwaitingPermission)
            .map { ag in
                (agent: ag, projectName: projectDisplayName(forCwd: ag.cwd))
            }
    }

    private var pinnedItems: [(key: String, agent: FfiAgentCard, isWorktree: Bool)] {
        guard let hostId = store.selectedHostId, !store.pinnedThreadKeys.isEmpty else { return [] }
        var lookup: [String: ProjectConversationItem] = [:]
        for proj in unifiedProjects {
            for conv in proj.conversations {
                let k = store.threadKey(hostId: hostId, agentId: conv.agent.agentId)
                lookup[k] = conv
            }
        }
        return store.pinnedThreadKeys.compactMap { key in
            guard let item = lookup[key] else { return nil }
            if isFilterActive {
                let unread = store.isThreadUnread(hostId: hostId, agent: item.agent)
                guard item.agent.isRunning || item.agent.isAwaitingPermission || unread else {
                    return nil
                }
            }
            return (key: key, agent: item.agent, isWorktree: item.isWorktree)
        }
    }

    private var displayProjects: [(project: UnifiedMobileProject, conversations: [ProjectConversationItem])] {
        guard let hostId = store.selectedHostId else { return [] }
        return unifiedProjects.compactMap { proj in
            let visible = proj.conversations.filter { conv in
                if store.isThreadPinned(hostId: hostId, agentId: conv.agent.agentId) {
                    return false
                }
                if isFilterActive {
                    let unread = store.isThreadUnread(hostId: hostId, agent: conv.agent)
                    return conv.agent.isRunning || conv.agent.isAwaitingPermission || unread
                }
                return true
            }

            let isActiveTarget = matchesProjectDirectory(proj.directories, cwd: store.activeCwd)
            if isFilterActive && visible.isEmpty && proj.pinnedProject == nil && !isActiveTarget {
                return nil
            }
            return (project: proj, conversations: visible)
        }
    }

    // MARK: - Search Mode Matches

    private var searchMatchedConversations: [(agent: FfiAgentCard, projectName: String, isWorktree: Bool)] {
        guard !trimmedSearchQuery.isEmpty else { return [] }
        let q = trimmedSearchQuery.lowercased()
        let wsMap = Dictionary(
            (store.currentHostDetail?.workspaces ?? []).map { (normalizeProjectPath($0.cwd), $0.isWorktree) },
            uniquingKeysWith: { first, _ in first }
        )
        return allAgents.compactMap { ag in
            let projName = projectDisplayName(forCwd: ag.cwd)
            let displayTitle = ag.title.isEmpty ? "New Conversation" : ag.title
            let matches = displayTitle.localizedCaseInsensitiveContains(q)
                || projName.localizedCaseInsensitiveContains(q)
                || OrbitFormatters.shortPath(ag.cwd).localizedCaseInsensitiveContains(q)
                || (branchForAgent(ag)?.localizedCaseInsensitiveContains(q) ?? false)
            guard matches else { return nil }
            let clean = normalizeProjectPath(ag.cwd)
            let isWt = wsMap[clean] ?? clean.contains("/worktrees/")
            return (agent: ag, projectName: projName, isWorktree: isWt)
        }
    }

    private var searchMatchedProjects: [(id: String, name: String, cwd: String, branch: String?, isPinned: Bool)] {
        guard !trimmedSearchQuery.isEmpty else { return [] }
        let q = trimmedSearchQuery.lowercased()
        var results: [(id: String, name: String, cwd: String, branch: String?, isPinned: Bool)] = []
        var seenCwds: Set<String> = []

        for proj in unifiedProjects {
            let branch = proj.gitBranch
            let matched = proj.name.localizedCaseInsensitiveContains(q)
                || proj.primaryCwd.localizedCaseInsensitiveContains(q)
                || (branch?.localizedCaseInsensitiveContains(q) ?? false)
            if matched {
                seenCwds.insert(proj.primaryCwd)
                results.append((
                    id: proj.id,
                    name: proj.name,
                    cwd: proj.primaryCwd,
                    branch: branch,
                    isPinned: proj.pinnedProject != nil
                ))
            }
        }

        for dp in store.currentHostDetail?.discoveredProjects ?? [] {
            let clean = normalizeProjectPath(dp.path)
            guard !seenCwds.contains(clean) else { continue }
            let matched = dp.name.localizedCaseInsensitiveContains(q)
                || clean.localizedCaseInsensitiveContains(q)
                || (dp.gitBranch?.localizedCaseInsensitiveContains(q) ?? false)
            if matched {
                seenCwds.insert(clean)
                results.append((
                    id: "repo::\(clean)",
                    name: dp.name.isEmpty ? OrbitFormatters.shortPath(clean) : dp.name,
                    cwd: clean,
                    branch: dp.gitBranch,
                    isPinned: false
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
                ForEach(searchMatchedConversations, id: \.agent.agentId) { entry in
                    sessionRow(
                        agent: entry.agent,
                        isWorktree: entry.isWorktree,
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
        // 1. Conditional Needs Attention (0pt when empty)
        if !needsAttentionItems.isEmpty {
            sectionHeaderRow(
                "Needs Attention",
                count: needsAttentionItems.count,
                accentColor: palette.warning,
                topPadding: 4
            )
            ForEach(needsAttentionItems, id: \.agent.agentId) { item in
                attentionRow(agent: item.agent, projectName: item.projectName)
            }
        }

        // 2. Conditional Pinned Conversations (0pt when empty)
        if !pinnedItems.isEmpty {
            sectionHeaderRow(
                "Pinned",
                topPadding: needsAttentionItems.isEmpty ? 4 : 14
            )
            ForEach(pinnedItems, id: \.key) { item in
                sessionRow(agent: item.agent, isWorktree: item.isWorktree)
            }
        }

        // 3. Projects Tree Header + Groups
        projectsSectionHeaderRow(
            topPadding: (needsAttentionItems.isEmpty && pinnedItems.isEmpty) ? 4 : 14
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
            ForEach(displayProjects, id: \.project.id) { entry in
                let project = entry.project
                let conversations = entry.conversations
                let isCollapsed = store.collapsedProjectIds.contains(project.id)
                let hostId = store.selectedHostId ?? ""
                let selectedIdx = conversations.firstIndex {
                    !store.isDraftNewSession && store.selectedAgentId == $0.agent.agentId
                } ?? -1
                let isExpandedAll = expandedMoreProjectIds.contains(project.id)
                    || selectedIdx >= Self.maxVisibleConversationsPerProject
                let hiddenCount = max(0, conversations.count - Self.maxVisibleConversationsPerProject)
                let sliced = isExpandedAll
                    ? conversations
                    : Array(conversations.prefix(Self.maxVisibleConversationsPerProject))

                projectHeaderRow(
                    project: project,
                    conversations: conversations,
                    isCollapsed: isCollapsed,
                    hostId: hostId
                )

                if !isCollapsed && !sliced.isEmpty {
                    ForEach(sliced, id: \.agent.agentId) { conv in
                        sessionRow(agent: conv.agent, isWorktree: conv.isWorktree)
                    }

                    if hiddenCount > 0 {
                        showMoreLessRow(projectId: project.id, hiddenCount: hiddenCount, isExpandedAll: isExpandedAll)
                    }
                }
            }
        }
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
        project: UnifiedMobileProject,
        conversations: [ProjectConversationItem],
        isCollapsed: Bool,
        hostId: String
    ) -> some View {
        let hasCollapsedApproval = isCollapsed && conversations.contains { $0.agent.isAwaitingPermission }
        let hasCollapsedRunning = isCollapsed && !hasCollapsedApproval && conversations.contains { $0.agent.isRunning }
        let hasCollapsedUnread = isCollapsed
            && !hasCollapsedApproval
            && !hasCollapsedRunning
            && conversations.contains { store.isThreadUnread(hostId: hostId, agent: $0.agent) }
        let isActiveDraft = store.isDraftNewSession && matchesProjectDirectory(project.directories, cwd: store.activeCwd)

        return HStack(spacing: 4) {
            Button {
                withAnimation(.easeInOut(duration: 0.16)) {
                    store.toggleProjectCollapsed(project.id)
                }
            } label: {
                HStack(spacing: 10) {
                    Image(systemName: isCollapsed ? "folder" : "folder")
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 16, height: 16)

                    Text(project.name)
                        .font(OrbitTypography.body(.medium))
                        .foregroundStyle(palette.fgPrimary.opacity(0.92))
                        .lineLimit(1)

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
                    .fill(Color(hex: "#0EA5E9"))
                    .frame(width: 6, height: 6)
                    .padding(.trailing, 2)
            }

            Button {
                store.startNewSessionDraft(cwd: project.primaryCwd, extraDirs: project.extraDirs)
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
                store.startNewSessionDraft(cwd: project.primaryCwd, extraDirs: project.extraDirs)
            } label: {
                Label("New Conversation", systemImage: "square.and.pencil")
            }

            if let pinned = project.pinnedProject {
                Button {
                    store.unpinProject(pinned.projectId)
                } label: {
                    Label("Unpin Project", systemImage: "pin.slash")
                }
            } else {
                Button {
                    store.pinProject(name: project.name, directories: [project.primaryCwd])
                } label: {
                    Label("Pin Project", systemImage: "pin")
                }
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

    private func attentionRow(agent: FfiAgentCard, projectName: String) -> some View {
        let isSelected = !store.isDraftNewSession && store.selectedAgentId == agent.agentId
        return Button {
            store.selectAgent(agent.agentId)
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
        agent: FfiAgentCard,
        isWorktree: Bool,
        trailingTag: String? = nil
    ) -> some View {
        let hostId = store.selectedHostId ?? ""
        let isSelected = !store.isDraftNewSession && store.selectedAgentId == agent.agentId
        let isPinned = store.isThreadPinned(hostId: hostId, agentId: agent.agentId)
        let isUnread = store.isThreadUnread(hostId: hostId, agent: agent)
        let titleWeight: Font.Weight = (isSelected || isUnread) ? .medium : .regular
        let titleColor = (isSelected || isUnread) ? palette.fgPrimary : mutedRowForeground
        let rightLabel = trailingTag
            ?? (agent.isRunning ? "now" : OrbitFormatters.relativeUpdatedTime(rfc3339: agent.updatedAtRfc3339))

        return Button {
            store.selectAgent(agent.agentId)
        } label: {
            HStack(spacing: 10) {
                sessionStatusGutter(agent: agent, isUnread: isUnread, isWorktree: isWorktree)
                    .frame(width: 16, height: 16)

                Text(agent.title.isEmpty ? "New Conversation" : agent.title)
                    .font(OrbitTypography.body(titleWeight))
                    .foregroundStyle(titleColor)
                    .lineLimit(1)

                Spacer(minLength: OrbitSpacing.xs)

                if !rightLabel.isEmpty {
                    Text(rightLabel)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted.opacity(0.75))
                        .lineLimit(1)
                }
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
        .contextMenu {
            if !hostId.isEmpty {
                Button {
                    store.togglePinThread(hostId: hostId, agentId: agent.agentId)
                } label: {
                    Label(isPinned ? "Unpin" : "Pin", systemImage: isPinned ? "pin.slash" : "pin")
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
                store.copyPathToClipboard(agent.cwd)
            } label: {
                Label("Copy Path", systemImage: "square.on.square")
            }

            Divider()

            Button(role: .destructive) {
                store.archiveSession(agent.agentId)
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
        }
        .swipeActions(edge: .trailing, allowsFullSwipe: true) {
            Button(role: .destructive) {
                store.archiveSession(agent.agentId)
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
        } else if agent.isRunning {
            OrbitSpinnerRingView(color: palette.fgSecondary, size: 12.5)
        } else if isUnread {
            Circle()
                .fill(Color(hex: "#0EA5E9"))
                .frame(width: 6, height: 6)
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
                            DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) {
                                store.activeSheet = .qrPair
                            }
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

            Button {
                store.hapticLight()
                if isCompactOverlay {
                    store.isDrawerOpen = false
                }
                store.refreshQuotas()
                store.activeSheet = .settings
            } label: {
                Image(systemName: "gearshape")
                    .font(.system(size: 15.5, weight: .medium))
                    .foregroundStyle(palette.fgPrimary)
                    .frame(width: 38, height: 38)
                    .background(softSurfaceFill, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Settings")
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.top, 6)
        .padding(.bottom, OrbitSpacing.sm)
    }

    private func hostCompactDetailLabel(for host: FfiHostCard) -> String {
        let route = OrbitFormatters.compactTopBarConnectionLabel(for: host)
        if let ping = host.lastPingMs {
            return "\(route) · \(ping)ms"
        }
        return route
    }

    // MARK: - Path, Branch & Timestamp Helpers

    private func normalizeProjectPath(_ raw: String) -> String {
        var trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty || trimmed == "~" { return "~" }
        while trimmed.count > 1 && trimmed.hasSuffix("/") {
            trimmed.removeLast()
        }
        return trimmed
    }

    private func isBareHomeDir(_ path: String) -> Bool {
        let clean = normalizeProjectPath(path)
        if clean == "~" || clean == "/" { return true }
        let parts = clean.split(separator: "/")
        return parts.count == 2 && (parts[0] == "Users" || parts[0] == "home")
    }

    private func matchesProjectDirectory(_ directories: [String], cwd: String) -> Bool {
        let cleanCwd = normalizeProjectPath(cwd)
        for dir in directories {
            if OrbitFormatters.arePathsEquivalent(dir, cleanCwd) {
                return true
            }
            let repoBase = OrbitFormatters.shortPath(dir)
            if !repoBase.isEmpty && repoBase != "~" && repoBase != "/"
                && cleanCwd.contains("/worktrees/\(repoBase)-") {
                return true
            }
        }
        return false
    }

    private func branchForDirectory(_ dir: String) -> String? {
        if OrbitFormatters.arePathsEquivalent(dir, store.activeCwd),
           let reviewBranch = store.gitReview?.branch,
           !reviewBranch.isEmpty {
            return reviewBranch
        }
        if let repoBranch = store.currentHostDetail?.discoveredProjects.first(where: {
            OrbitFormatters.arePathsEquivalent($0.path, dir)
        })?.gitBranch, !repoBranch.isEmpty {
            return repoBranch
        }
        if let wsBranch = store.currentHostDetail?.workspaces.first(where: {
            !$0.isWorktree && OrbitFormatters.arePathsEquivalent($0.cwd, dir)
        })?.gitBranch, !wsBranch.isEmpty {
            return wsBranch
        }
        return nil
    }

    private func branchForAgent(_ agent: FfiAgentCard) -> String? {
        if let dirBranch = branchForDirectory(agent.cwd) {
            return dirBranch
        }
        if let wsBranch = store.currentHostDetail?.workspaces.first(where: {
            $0.workspaceId == agent.workspaceId
        })?.gitBranch, !wsBranch.isEmpty {
            return wsBranch
        }
        return nil
    }

    private func projectDisplayName(forCwd cwd: String) -> String {
        let clean = normalizeProjectPath(cwd)
        if let matched = unifiedProjects.first(where: {
            matchesProjectDirectory($0.directories, cwd: clean)
        }) {
            return matched.name
        }
        return OrbitFormatters.shortPath(clean)
    }

    private func parseUpdatedTimestamp(_ rfc3339: String) -> TimeInterval {
        guard !rfc3339.isEmpty else { return 0 }
        let f1 = ISO8601DateFormatter()
        f1.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let d = f1.date(from: rfc3339) {
            return d.timeIntervalSince1970
        }
        let f2 = ISO8601DateFormatter()
        if let d = f2.date(from: rfc3339) {
            return d.timeIntervalSince1970
        }
        return 0
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
        VStack(alignment: .leading, spacing: 6) {
            OrbitPopoverSectionHeader(title: "Hosts")
                .padding(.horizontal, 10)

            ViewThatFits(in: .vertical) {
                hostRowsStack
                ScrollView(.vertical, showsIndicators: true) {
                    hostRowsStack
                }
                .frame(height: 220)
            }
            .frame(maxHeight: 220)

            if showPairingActions {
                OrbitHairline()
                    .padding(.horizontal, 20)
                    .padding(.vertical, 2)

                VStack(alignment: .leading, spacing: 2) {
                    OrbitPopoverActionRow(
                        title: "Pair New Host",
                        systemImage: "qrcode.viewfinder"
                    ) {
                        onPairNewHost?()
                    }

                    if store.localDaemonUri != nil {
                        OrbitPopoverActionRow(
                            title: "Connect Local Host",
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
        .frame(width: 264)
    }

    private var hostRowsStack: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(store.hosts, id: \.hostId) { h in
                let route = OrbitFormatters.compactTopBarConnectionLabel(for: h)
                let detail = h.lastPingMs.map { "\(route) · \($0)ms" } ?? route
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
