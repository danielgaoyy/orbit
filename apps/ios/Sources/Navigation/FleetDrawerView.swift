import SwiftUI

struct FleetDrawerView: View {
    @ObservedObject var store: OrbitIosStore
    let isCompactOverlay: Bool

    @Environment(\.orbitPalette) private var palette

    private var allAgents: [FfiAgentCard] {
        store.currentHostDetail?.agents ?? []
    }

    private var trimmedSearchQuery: String {
        store.drawerSearchQuery.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var needsApprovalSessions: [FfiAgentCard] {
        guard store.drawerFilter == .all || store.drawerFilter == .approvals else { return [] }
        return allAgents.filter { $0.isAwaitingPermission && matchesSessionSearch($0) }
    }

    private var activeSessions: [FfiAgentCard] {
        guard store.drawerFilter == .all || store.drawerFilter == .active else { return [] }
        return allAgents.filter { $0.isRunning && !$0.isAwaitingPermission && matchesSessionSearch($0) }
    }

    private var recentSessions: [FfiAgentCard] {
        guard store.drawerFilter == .all else { return [] }
        return allAgents.filter { !$0.isRunning && !$0.isAwaitingPermission && matchesSessionSearch($0) }
    }

    private var filteredPinnedProjects: [FfiClientProject] {
        guard store.drawerFilter == .all else { return [] }
        return store.hostPinnedProjects.filter { matchesProjectSearch($0) }
    }

    private var discoveredRepos: [FfiDiscoveredProject] {
        guard store.drawerFilter == .all else { return [] }
        let pinnedDirs = Set(store.hostPinnedProjects.flatMap(\.directories))
        return (store.currentHostDetail?.discoveredProjects ?? [])
            .filter { !pinnedDirs.contains($0.path) && matchesRepoSearch($0) }
    }

    private var hasAnyVisibleDrawerContent: Bool {
        !needsApprovalSessions.isEmpty
            || !activeSessions.isEmpty
            || !recentSessions.isEmpty
            || !filteredPinnedProjects.isEmpty
            || !discoveredRepos.isEmpty
    }

    var body: some View {
        VStack(spacing: 0) {
            drawerHeader
            hostSwitcherStrip
            if !store.hosts.isEmpty || !trimmedSearchQuery.isEmpty || store.drawerFilter != .all {
                drawerSearchAndFilterBar
            }
            OrbitHairline()
            drawerList
            OrbitHairline()
            drawerFooterBar
        }
        .background(palette.drawer.ignoresSafeArea())
    }

    // MARK: - Search & Filter Helpers

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
            OrbitFormatters.arePathsEquivalent($0.cwd, dir)
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

    private func matchesSessionSearch(_ agent: FfiAgentCard) -> Bool {
        let q = trimmedSearchQuery.lowercased()
        guard !q.isEmpty else { return true }
        let displayTitle = agent.title.isEmpty ? "Untitled Session" : agent.title
        if displayTitle.lowercased().contains(q) { return true }
        if agent.cwd.lowercased().contains(q) { return true }
        if OrbitFormatters.shortPath(agent.cwd).lowercased().contains(q) { return true }
        if OrbitFormatters.compactPath(agent.cwd).lowercased().contains(q) { return true }
        if agent.providerLabel.lowercased().contains(q) { return true }
        if OrbitFormatters.providerLabel(agent.provider).lowercased().contains(q) { return true }
        if OrbitFormatters.providerShortLabel(agent.provider).lowercased().contains(q) { return true }
        if let branch = branchForAgent(agent), branch.lowercased().contains(q) { return true }
        return false
    }

    private func matchesProjectSearch(_ project: FfiClientProject) -> Bool {
        let q = trimmedSearchQuery.lowercased()
        guard !q.isEmpty else { return true }
        if project.name.lowercased().contains(q) { return true }
        for dir in project.directories {
            if dir.lowercased().contains(q) { return true }
            if OrbitFormatters.shortPath(dir).lowercased().contains(q) { return true }
            if OrbitFormatters.compactPath(dir).lowercased().contains(q) { return true }
            if let branch = branchForDirectory(dir), branch.lowercased().contains(q) { return true }
        }
        return false
    }

    private func matchesRepoSearch(_ repo: FfiDiscoveredProject) -> Bool {
        let q = trimmedSearchQuery.lowercased()
        guard !q.isEmpty else { return true }
        if repo.name.lowercased().contains(q) { return true }
        if let branch = repo.gitBranch, branch.lowercased().contains(q) { return true }
        if repo.path.lowercased().contains(q) { return true }
        if OrbitFormatters.compactPath(repo.path).lowercased().contains(q) { return true }
        return false
    }

    // MARK: - Header

    private var drawerHeader: some View {
        HStack(spacing: OrbitSpacing.sm) {
            Text("Orbit")
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)

            if store.hosts.count > 0 {
                Text("\(store.hosts.count) \(store.hosts.count == 1 ? "host" : "hosts")")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgMuted)
                    .padding(.horizontal, OrbitSpacing.sm)
                    .padding(.vertical, 2)
                    .background(palette.subtleFill)
                    .clipShape(Capsule())
            }

            Spacer()

            if !store.hosts.isEmpty {
                Button {
                    store.startNewSessionDraft()
                } label: {
                    HStack(spacing: OrbitSpacing.xs) {
                        Image(systemName: "square.and.pencil")
                            .font(OrbitTypography.iconSmall)
                        Text("New Session")
                            .font(OrbitTypography.subhead(.medium))
                    }
                    .foregroundStyle(palette.primary)
                    .padding(.horizontal, OrbitSpacing.sm)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .frame(height: OrbitSpacing.hitTarget)
    }

    // MARK: - Multi-Host Switcher Pills

    @ViewBuilder
    private var hostSwitcherStrip: some View {
        if store.hosts.isEmpty {
            VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                Text(store.localDaemonUri != nil
                     ? "Local Orbit daemon detected on this Mac."
                     : "No hosts connected yet. Use Pair Host below to connect.")
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted)

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
                        .foregroundStyle(Color.white)
                        .padding(.horizontal, OrbitSpacing.md)
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .background(palette.primary)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }
                    .buttonStyle(.plain)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.bottom, OrbitSpacing.md)
        } else {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: OrbitSpacing.sm) {
                    ForEach(store.hosts, id: \.hostId) { host in
                        hostPill(host)
                    }

                    Button {
                        store.hapticLight()
                        store.activeSheet = .qrPair
                    } label: {
                        Image(systemName: "plus")
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(palette.fgSecondary)
                            .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                            .background(palette.subtleFill)
                            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 12, style: .continuous)
                                    .strokeBorder(palette.hairline, lineWidth: 0.5)
                            )
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Pair Another Host")
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.bottom, OrbitSpacing.sm)
            }
        }
    }

    private func hostPill(_ host: FfiHostCard) -> some View {
        let isSelected = host.hostId == store.selectedHostId
        return Button {
            store.hapticLight()
            store.selectHost(host.hostId)
        } label: {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: OrbitSpacing.xs) {
                    Circle()
                        .fill(palette.connectionColor(host.connectionState))
                        .frame(width: 6, height: 6)

                    Text(host.displayName)
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    if host.pendingApprovalCount > 0 {
                        Text("\(host.pendingApprovalCount)")
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.warning)
                            .padding(.horizontal, OrbitSpacing.xs)
                            .background(palette.warning.opacity(0.15))
                            .clipShape(Capsule())
                    }
                }

                HStack(spacing: OrbitSpacing.xs) {
                    Text(OrbitFormatters.connectionLabel(host.connectionState))
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgSecondary)

                    if let rtt = host.lastPingMs {
                        Text("· \(rtt)ms")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                    }
                }
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.vertical, OrbitSpacing.sm)
            .frame(minHeight: OrbitSpacing.hitTarget)
            .background(isSelected ? palette.elevated : palette.subtleFill)
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(
                        isSelected ? palette.primary.opacity(0.55) : palette.hairline,
                        lineWidth: isSelected ? 1.0 : 0.5
                    )
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    // MARK: - Search Field & Filter Chips

    private var drawerSearchAndFilterBar: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                Image(systemName: "magnifyingglass")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(palette.fgMuted)

                TextField("Search sessions or repos…", text: $store.drawerSearchQuery)
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgPrimary)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled(true)

                if !store.drawerSearchQuery.isEmpty {
                    Button {
                        store.hapticLight()
                        store.drawerSearchQuery = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear Search")
                }
            }
            .padding(.leading, OrbitSpacing.md)
            .frame(height: 40)
            .background(palette.subtleFill)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )

            HStack(spacing: OrbitSpacing.xs) {
                ForEach(OrbitDrawerFilter.allCases) { filter in
                    drawerFilterChip(filter)
                }
                Spacer(minLength: 0)
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.bottom, OrbitSpacing.sm)
    }

    private func drawerFilterChip(_ filter: OrbitDrawerFilter) -> some View {
        let isSelected = store.drawerFilter == filter
        let count: Int = {
            switch filter {
            case .all:
                return 0
            case .active:
                return allAgents.filter { $0.isRunning && !$0.isAwaitingPermission }.count
            case .approvals:
                return allAgents.filter { $0.isAwaitingPermission }.count
            }
        }()

        return Button {
            store.hapticLight()
            withAnimation(.easeInOut(duration: 0.16)) {
                store.drawerFilter = filter
            }
        } label: {
            HStack(spacing: OrbitSpacing.xs) {
                Text(filter.label)
                    .font(OrbitTypography.meta(isSelected ? .semibold : .medium))
                    .lineLimit(1)

                if count > 0 {
                    Text("\(count)")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(
                            filter == .approvals
                                ? palette.warning
                                : (isSelected ? palette.primary : palette.fgMuted)
                        )
                }
            }
            .foregroundStyle(isSelected ? palette.primary : palette.fgSecondary)
            .padding(.horizontal, OrbitSpacing.sm + 2)
            .padding(.vertical, OrbitSpacing.xs)
            .background(isSelected ? palette.primary.opacity(0.14) : palette.subtleFill)
            .clipShape(Capsule())
            .overlay(
                Capsule()
                    .strokeBorder(
                        isSelected ? palette.primary.opacity(0.35) : palette.hairline,
                        lineWidth: 0.5
                    )
            )
            .frame(minHeight: 32)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    // MARK: - Main Grouped List (Sessions + Projects + Repos)

    private var drawerList: some View {
        List {
            if !hasAnyVisibleDrawerContent && store.selectedHostId != nil {
                VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
                    Text(emptyFilterMessage)
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgMuted)

                    if !trimmedSearchQuery.isEmpty || store.drawerFilter != .all {
                        Button {
                            store.hapticLight()
                            store.drawerSearchQuery = ""
                            store.drawerFilter = .all
                        } label: {
                            Text("Clear search & filters")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.primary)
                                .frame(minHeight: 32, alignment: .leading)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, OrbitSpacing.md)
                .padding(.vertical, OrbitSpacing.md)
                .listRowInsets(EdgeInsets(top: 0, leading: OrbitSpacing.sm, bottom: 0, trailing: OrbitSpacing.sm))
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
            }

            // 1. Needs Approval
            if !needsApprovalSessions.isEmpty {
                sectionHeader("Needs Approval", badgeCount: needsApprovalSessions.count, accent: palette.warning)
                ForEach(needsApprovalSessions, id: \.agentId) { agent in
                    sessionRow(agent)
                }
            }

            // 2. Active Sessions
            if !activeSessions.isEmpty {
                sectionHeader("Active", badgeCount: activeSessions.count, accent: palette.direct)
                ForEach(activeSessions, id: \.agentId) { agent in
                    sessionRow(agent)
                }
            }

            // 3. Recent Sessions
            if !recentSessions.isEmpty {
                sectionHeader("Recent", badgeCount: nil, accent: nil)
                ForEach(recentSessions, id: \.agentId) { agent in
                    sessionRow(agent)
                }
            }

            // 4. Pinned Projects
            if !filteredPinnedProjects.isEmpty {
                sectionHeader("Pinned Projects", badgeCount: nil, accent: nil)
                ForEach(filteredPinnedProjects, id: \.projectId) { project in
                    pinnedProjectRow(project)
                }
            }

            // 5. Discovered Git Repositories & Directory Browser
            if store.selectedHostId != nil && store.drawerFilter == .all && (!discoveredRepos.isEmpty || trimmedSearchQuery.isEmpty) {
                sectionHeader("Workspaces", badgeCount: nil, accent: nil)

                ForEach(discoveredRepos.prefix(trimmedSearchQuery.isEmpty ? 8 : 24), id: \.path) { repo in
                    discoveredRepoRow(repo)
                }

                if trimmedSearchQuery.isEmpty {
                    Button {
                        store.hapticLight()
                        store.openDirectoryBrowser()
                    } label: {
                        HStack(spacing: OrbitSpacing.sm) {
                            Image(systemName: "folder.badge.gearshape")
                                .font(OrbitTypography.iconSmall)
                                .foregroundStyle(palette.fgSecondary)
                                .frame(width: 20)
                            Text("Browse Remote Directory")
                                .font(OrbitTypography.subhead(.medium))
                                .foregroundStyle(palette.fgSecondary)
                            Spacer()
                            Image(systemName: "chevron.right")
                                .font(OrbitTypography.iconMicro)
                                .foregroundStyle(palette.fgMuted)
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .listRowInsets(EdgeInsets(top: 0, leading: OrbitSpacing.sm, bottom: OrbitSpacing.sm, trailing: OrbitSpacing.sm))
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
                }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(palette.drawer)
    }

    private var emptyFilterMessage: String {
        if !trimmedSearchQuery.isEmpty {
            return "No matches for \"\(trimmedSearchQuery)\""
        }
        switch store.drawerFilter {
        case .approvals:
            return "No pending approvals"
        case .active:
            return "No active sessions"
        case .all:
            return "No sessions or workspaces yet"
        }
    }

    private func sectionHeader(_ title: String, badgeCount: Int?, accent: Color?) -> some View {
        HStack(spacing: OrbitSpacing.xs) {
            Text(title)
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(accent ?? palette.fgMuted)

            if let count = badgeCount, count > 0 {
                Text("\(count)")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(accent ?? palette.fgSecondary)
            }
            Spacer()
        }
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.top, OrbitSpacing.md)
        .padding(.bottom, OrbitSpacing.xs)
        .listRowInsets(EdgeInsets(top: 0, leading: OrbitSpacing.sm, bottom: 0, trailing: OrbitSpacing.sm))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private func sessionRow(_ agent: FfiAgentCard) -> some View {
        let isSelected = store.selectedAgentId == agent.agentId
        return Button {
            store.selectAgent(agent.agentId)
        } label: {
            HStack(alignment: .center, spacing: OrbitSpacing.sm) {
                if agent.isAwaitingPermission {
                    Circle()
                        .fill(palette.warning)
                        .frame(width: 6, height: 6)
                } else if agent.isRunning {
                    PulsingDotView(color: palette.direct, size: 6)
                } else {
                    Circle()
                        .fill(isSelected ? palette.primary : palette.fgMuted.opacity(0.45))
                        .frame(width: 6, height: 6)
                }

                VStack(alignment: .leading, spacing: 2) {
                    Text(agent.title.isEmpty ? "Untitled Session" : agent.title)
                        .font(OrbitTypography.body(isSelected ? .medium : .regular))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    HStack(spacing: OrbitSpacing.xs) {
                        Text(OrbitFormatters.shortPath(agent.cwd))
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(palette.fgSecondary)
                            .lineLimit(1)

                        Text("·")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)

                        Text(agent.providerLabel)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(1)
                    }
                }

                Spacer(minLength: OrbitSpacing.xs)

                Text(OrbitFormatters.relativeUpdatedTime(rfc3339: agent.updatedAtRfc3339))
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted)
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.vertical, OrbitSpacing.sm)
            .frame(minHeight: OrbitSpacing.hitTarget)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(isSelected ? palette.activeRowFill : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button {
                store.startNewSessionDraft(cwd: agent.cwd, provider: agent.provider)
            } label: {
                Label("New Session in Workspace", systemImage: "square.and.pencil")
            }

            Button {
                store.openTerminal(inCwd: agent.cwd)
            } label: {
                Label("Open Terminal Here", systemImage: "terminal")
            }

            Button {
                store.copyPathToClipboard(agent.cwd)
            } label: {
                Label("Copy Workspace Path", systemImage: "doc.on.doc")
            }

            Divider()

            Button(role: .destructive) {
                store.archiveSession(agent.agentId)
            } label: {
                Label("Archive Session", systemImage: "archivebox")
            }
        }
        .swipeActions(edge: .trailing, allowsFullSwipe: true) {
            Button(role: .destructive) {
                store.archiveSession(agent.agentId)
            } label: {
                Label("Archive", systemImage: "archivebox")
            }
        }
        .listRowInsets(EdgeInsets(top: 2, leading: OrbitSpacing.sm, bottom: 2, trailing: OrbitSpacing.sm))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private func pinnedProjectRow(_ project: FfiClientProject) -> some View {
        let primaryDir = project.directories.first ?? "~"
        let extraDirs = Array(project.directories.dropFirst())
        let isActiveDir = store.isDraftNewSession && OrbitFormatters.arePathsEquivalent(store.activeCwd, primaryDir)
        let liveBranch = branchForDirectory(primaryDir)

        return Button {
            store.startNewSessionDraft(cwd: primaryDir, extraDirs: extraDirs)
        } label: {
            HStack(spacing: OrbitSpacing.sm) {
                Image(systemName: "pin.fill")
                    .font(OrbitTypography.iconMicro)
                    .foregroundStyle(palette.primary)
                    .frame(width: 16)

                VStack(alignment: .leading, spacing: 2) {
                    Text(project.name)
                        .font(OrbitTypography.body(.regular))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    if let branch = liveBranch, !branch.isEmpty {
                        Text(branch)
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(1)
                    } else {
                        Text(OrbitFormatters.compactPath(primaryDir))
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(1)
                    }
                }

                Spacer()

                Text("New")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.primary)
                    .padding(.horizontal, OrbitSpacing.sm)
                    .padding(.vertical, 2)
                    .background(palette.primary.opacity(0.1))
                    .clipShape(Capsule())
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.vertical, OrbitSpacing.xs)
            .frame(minHeight: OrbitSpacing.hitTarget)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(isActiveDir ? palette.activeRowFill : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button {
                store.startNewSessionDraft(cwd: primaryDir, extraDirs: extraDirs)
            } label: {
                Label("New Session Here", systemImage: "square.and.pencil")
            }

            Button {
                store.inspectGitDiff(inCwd: primaryDir)
            } label: {
                Label("Inspect Git Diff", systemImage: "arrow.triangle.branch")
            }

            Button {
                store.openTerminal(inCwd: primaryDir)
            } label: {
                Label("Open Terminal Here", systemImage: "terminal")
            }

            Button {
                store.copyPathToClipboard(primaryDir)
            } label: {
                Label("Copy Path", systemImage: "doc.on.doc")
            }

            Divider()

            Button(role: .destructive) {
                store.unpinProject(project.projectId)
            } label: {
                Label("Unpin Workspace", systemImage: "pin.slash")
            }
        }
        .swipeActions(edge: .trailing) {
            Button(role: .destructive) {
                store.unpinProject(project.projectId)
            } label: {
                Label("Unpin", systemImage: "pin.slash")
            }
        }
        .listRowInsets(EdgeInsets(top: 2, leading: OrbitSpacing.sm, bottom: 2, trailing: OrbitSpacing.sm))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    private func discoveredRepoRow(_ repo: FfiDiscoveredProject) -> some View {
        let isCurrent = store.isDraftNewSession && OrbitFormatters.arePathsEquivalent(store.activeCwd, repo.path)
        let liveBranch = branchForDirectory(repo.path) ?? repo.gitBranch
        return HStack(spacing: OrbitSpacing.xs) {
            Button {
                store.startNewSessionDraft(cwd: repo.path)
            } label: {
                HStack(spacing: OrbitSpacing.sm) {
                    Image(systemName: repo.isGitRepo ? "arrow.triangle.branch" : "folder")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 18)

                    VStack(alignment: .leading, spacing: 2) {
                        Text(repo.name)
                            .font(OrbitTypography.body(.regular))
                            .foregroundStyle(palette.fgPrimary)
                            .lineLimit(1)

                        if let branch = liveBranch, !branch.isEmpty {
                            Text(branch)
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .lineLimit(1)
                        } else {
                            Text(OrbitFormatters.compactPath(repo.path))
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .lineLimit(1)
                        }
                    }

                    Spacer(minLength: OrbitSpacing.xs)
                }
                .padding(.leading, OrbitSpacing.md)
                .frame(minHeight: OrbitSpacing.hitTarget)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            Button {
                store.pinProject(name: repo.name, directories: [repo.path])
            } label: {
                Image(systemName: "pin")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(palette.fgMuted)
                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Pin \(repo.name)")
        }
        .background(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .fill(isCurrent ? palette.activeRowFill : Color.clear)
        )
        .contextMenu {
            Button {
                store.startNewSessionDraft(cwd: repo.path)
            } label: {
                Label("New Session Here", systemImage: "square.and.pencil")
            }

            Button {
                store.pinProject(name: repo.name, directories: [repo.path])
            } label: {
                Label("Pin Workspace", systemImage: "pin")
            }

            Button {
                store.inspectGitDiff(inCwd: repo.path)
            } label: {
                Label("Inspect Git Diff", systemImage: "arrow.triangle.branch")
            }

            Button {
                store.openTerminal(inCwd: repo.path)
            } label: {
                Label("Open Terminal Here", systemImage: "terminal")
            }

            Button {
                store.copyPathToClipboard(repo.path)
            } label: {
                Label("Copy Path", systemImage: "doc.on.doc")
            }
        }
        .listRowInsets(EdgeInsets(top: 1, leading: OrbitSpacing.sm, bottom: 1, trailing: OrbitSpacing.sm))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    // MARK: - Bottom Bar (Pair Host & Settings / Quotas)

    private var drawerFooterBar: some View {
        HStack(spacing: OrbitSpacing.sm) {
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
                .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                .background(palette.subtleFill)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            Button {
                store.hapticLight()
                if isCompactOverlay {
                    store.isDrawerOpen = false
                }
                store.refreshQuotas()
                store.activeSheet = .settings
            } label: {
                HStack(spacing: OrbitSpacing.xs) {
                    Image(systemName: "slider.horizontal.3")
                        .font(OrbitTypography.iconSmall)
                    Text("Settings")
                        .font(OrbitTypography.subhead(.medium))
                        .lineLimit(1)
                }
                .foregroundStyle(palette.fgPrimary)
                .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                .background(palette.subtleFill)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.vertical, OrbitSpacing.sm)
    }
}
