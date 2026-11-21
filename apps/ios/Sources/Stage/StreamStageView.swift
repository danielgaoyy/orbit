import SwiftUI

private struct StageBottomAnchorKey: PreferenceKey {
    static var defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

struct StreamStageView: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.orbitPalette) private var palette
    @State private var isBottomAnchorMounted: Bool = true
    @State private var isScrolledUpByGeometry: Bool = false
    @State private var wasPinnedToBottom: Bool = true
    @State private var isReanchoringForViewportShrink: Bool = false
    @State private var stageViewportHeight: CGFloat = 0
    @State private var isProjectPickerPresented: Bool = false
    @State private var isWorkspacePickerPresented: Bool = false
    @State private var isHostPickerPresented: Bool = false
    @State private var isEnvironmentPickerPresented: Bool = false
    @State private var projectSearchQuery: String = ""

    private let bottomAnchorId = "bottom_anchor"

    private var turns: [FfiStageTurn] {
        store.agentStage?.turns ?? []
    }

    private var isLatestTurnActive: Bool {
        guard let last = turns.last else { return false }
        return store.isActiveAgentRunning || last.hasActiveItem
    }

    private var pendingDecisionCount: Int {
        (store.agentStage?.pendingPermissions.count ?? 0)
            + (store.agentStage?.pendingQuestions.count ?? 0)
            + store.demoPendingPermissions.count
            + store.demoPendingQuestions.count
    }

    private var showScrollToBottomPill: Bool {
        !turns.isEmpty && !isReanchoringForViewportShrink && (!isBottomAnchorMounted || isScrolledUpByGeometry)
    }

    private func dismissKeyboard() {
        UIApplication.shared.sendAction(
            #selector(UIResponder.resignFirstResponder),
            to: nil,
            from: nil,
            for: nil
        )
    }

    var body: some View {
        VStack(spacing: 0) {
            if !store.hosts.isEmpty {
                ConnectionStatusBannerView(store: store)
                WorkspacePeerStripView(store: store)
            }

            Group {
                if store.hosts.isEmpty {
                    unpairedZeroState
                } else if turns.isEmpty {
                    newSessionZeroState
                } else {
                    turnStreamScrollView
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .contentShape(Rectangle())
        .simultaneousGesture(
            TapGesture().onEnded {
                dismissKeyboard()
            }
        )
        .background(palette.canvas.ignoresSafeArea())
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if !store.hosts.isEmpty || store.hasDemoStageOverlay {
                VStack(spacing: 0) {
                    SubagentsTrackPill(store: store)
                    StickyDecisionDock(store: store)
                    OmniComposerView(store: store)
                }
            }
        }
    }

    // MARK: - Turn Stream ScrollView

    private var turnStreamScrollView: some View {
        GeometryReader { outerGeo in
            ScrollViewReader { proxy in
                ScrollView(.vertical, showsIndicators: true) {
                    LazyVStack(alignment: .leading, spacing: 22) {
                        ForEach(Array(turns.enumerated()), id: \.element.key) { idx, turn in
                            let isTurnActive = (idx == turns.count - 1) && isLatestTurnActive
                            StageTurnView(store: store, turn: turn, isTurnActive: isTurnActive)
                        }

                        Color.clear
                            .frame(height: 1)
                            .id(bottomAnchorId)
                            .onAppear {
                                isBottomAnchorMounted = true
                            }
                            .onDisappear {
                                isBottomAnchorMounted = false
                            }
                            .background(
                                GeometryReader { anchorGeo in
                                    Color.clear.preference(
                                        key: StageBottomAnchorKey.self,
                                        value: anchorGeo.frame(in: .named("stageScrollSpace")).maxY
                                    )
                                }
                            )
                    }
                    .padding(.horizontal, OrbitSpacing.lg)
                    .padding(.top, OrbitSpacing.md)
                    .padding(.bottom, OrbitSpacing.lg)
                }
                .coordinateSpace(name: "stageScrollSpace")
                .orbitSoftScrollEdgeEffect()
                .scrollDismissesKeyboard(.interactively)
                .refreshable {
                    await store.pullToRefreshStage()
                }
                .onAppear {
                    stageViewportHeight = outerGeo.size.height
                    if store.keepStageScrolledToTop {
                        wasPinnedToBottom = false
                        isBottomAnchorMounted = false
                        if let firstKey = turns.first?.key {
                            proxy.scrollTo(firstKey, anchor: .top)
                        }
                    } else {
                        wasPinnedToBottom = true
                        isBottomAnchorMounted = true
                        isScrolledUpByGeometry = false
                        proxy.scrollTo(bottomAnchorId, anchor: .bottom)
                        DispatchQueue.main.async {
                            guard !store.keepStageScrolledToTop else { return }
                            proxy.scrollTo(bottomAnchorId, anchor: .bottom)
                        }
                    }
                }
                .onChange(of: outerGeo.size.height) { newHeight in
                    let previousHeight = stageViewportHeight
                    stageViewportHeight = newHeight
                    guard !store.keepStageScrolledToTop, !turns.isEmpty else { return }
                    let shrank = previousHeight > 0 && newHeight < previousHeight - 2
                    if previousHeight == 0 || (shrank && (wasPinnedToBottom || isBottomAnchorMounted)) {
                        reanchorToBottomAfterLayout(proxy: proxy)
                    }
                }
                .onChange(of: pendingDecisionCount) { newCount in
                    guard newCount > 0, !store.keepStageScrolledToTop, !turns.isEmpty else { return }
                    if wasPinnedToBottom || isBottomAnchorMounted {
                        reanchorToBottomAfterLayout(proxy: proxy)
                    }
                }
                .onPreferenceChange(StageBottomAnchorKey.self) { anchorMaxY in
                    guard stageViewportHeight > 0, isBottomAnchorMounted, !isReanchoringForViewportShrink else {
                        return
                    }
                    let delta = anchorMaxY - stageViewportHeight
                    let scrolledAway = delta > 72 && !turns.isEmpty
                    wasPinnedToBottom = !scrolledAway
                    if scrolledAway != isScrolledUpByGeometry {
                        withAnimation(.easeInOut(duration: 0.18)) {
                            isScrolledUpByGeometry = scrolledAway
                        }
                    }
                }
                .onChange(of: store.selectedAgentId) { _ in
                    if store.keepStageScrolledToTop {
                        wasPinnedToBottom = false
                        isBottomAnchorMounted = false
                        DispatchQueue.main.async {
                            if let firstKey = turns.first?.key {
                                proxy.scrollTo(firstKey, anchor: .top)
                            }
                        }
                    } else {
                        wasPinnedToBottom = true
                        isScrolledUpByGeometry = false
                        isBottomAnchorMounted = true
                        DispatchQueue.main.async {
                            proxy.scrollTo(bottomAnchorId, anchor: .bottom)
                        }
                    }
                }
                .onChange(of: store.agentStage?.latestSeq) { _ in
                    guard !showScrollToBottomPill && !store.keepStageScrolledToTop else { return }
                    proxy.scrollTo(bottomAnchorId, anchor: .bottom)
                }
                .onChange(of: turns.count) { _ in
                    guard !store.keepStageScrolledToTop else { return }
                    withAnimation(.easeOut(duration: 0.22)) {
                        proxy.scrollTo(bottomAnchorId, anchor: .bottom)
                    }
                }
                .overlay(alignment: .bottom) {
                    if showScrollToBottomPill {
                        Button {
                            store.hapticLight()
                            store.keepStageScrolledToTop = false
                            wasPinnedToBottom = true
                            withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                                isScrolledUpByGeometry = false
                                isBottomAnchorMounted = true
                                proxy.scrollTo(bottomAnchorId, anchor: .bottom)
                            }
                        } label: {
                            Group {
                                if store.isActiveAgentRunning {
                                    HStack(spacing: 6) {
                                        PulsingDotView(color: palette.primary, size: 6)
                                        Image(systemName: "arrow.down")
                                            .font(.system(size: 13, weight: .semibold))
                                            .foregroundStyle(palette.fgPrimary)
                                    }
                                    .padding(.horizontal, 12)
                                    .frame(height: 36)
                                    .orbitGlassSurface(
                                        in: Capsule(),
                                        interactive: true,
                                        fallbackFill: palette.elevated,
                                        fallbackBorder: palette.hairline,
                                        fallbackShadowOpacity: palette.isDark ? 0.24 : 0.10
                                    )
                                } else {
                                    Image(systemName: "arrow.down")
                                        .font(.system(size: 14, weight: .semibold))
                                        .foregroundStyle(palette.fgPrimary)
                                        .frame(width: 36, height: 36)
                                        .orbitGlassSurface(
                                            in: Circle(),
                                            interactive: true,
                                            fallbackFill: palette.elevated,
                                            fallbackBorder: palette.hairline,
                                            fallbackShadowOpacity: palette.isDark ? 0.24 : 0.10
                                        )
                                }
                            }
                            .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .padding(.bottom, 10)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                        .accessibilityLabel(store.isActiveAgentRunning ? "Scroll to Streaming Output" : "Scroll to Latest Turn")
                    }
                }
            }
        }
    }

    private func reanchorToBottomAfterLayout(proxy: ScrollViewProxy) {
        isReanchoringForViewportShrink = true
        isScrolledUpByGeometry = false
        wasPinnedToBottom = true
        withAnimation(.easeOut(duration: 0.18)) {
            proxy.scrollTo(bottomAnchorId, anchor: .bottom)
        }
        DispatchQueue.main.async {
            guard !store.keepStageScrolledToTop else {
                isReanchoringForViewportShrink = false
                return
            }
            proxy.scrollTo(bottomAnchorId, anchor: .bottom)
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) {
                if !store.keepStageScrolledToTop {
                    withAnimation(.easeOut(duration: 0.14)) {
                        proxy.scrollTo(bottomAnchorId, anchor: .bottom)
                    }
                }
                isReanchoringForViewportShrink = false
            }
        }
    }

    // MARK: - Unpaired Onboarding Zero-State

    private var unpairedZeroState: some View {
        VStack(spacing: OrbitSpacing.lg) {
            Spacer()

            VStack(spacing: OrbitSpacing.sm) {
                Image(systemName: "network")
                    .font(OrbitTypography.title)
                    .foregroundStyle(palette.primary)
                    .frame(width: 48, height: 48)
                    .background(palette.primary.opacity(0.1))
                    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))

                Text("Pair Your First Host")
                    .font(OrbitTypography.title)
                    .foregroundStyle(palette.fgPrimary)

                Text("Run `orbit pair` on your Mac or Linux workstation to stream agents over LAN or E2EE Relay.")
                    .orbitSubheadStyle()
                    .foregroundStyle(palette.fgSecondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, OrbitSpacing.xl)
            }

            VStack(spacing: OrbitSpacing.sm) {
                if store.localDaemonUri != nil {
                    Button {
                        _ = store.connectLocalSimulatorHost()
                    } label: {
                        HStack(spacing: OrbitSpacing.sm) {
                            Image(systemName: "bolt.horizontal.circle.fill")
                                .font(OrbitTypography.icon)
                            Text("Connect Local Simulator Host")
                                .font(OrbitTypography.subhead(.semibold))
                        }
                        .foregroundStyle(palette.onPrimary)
                        .padding(.horizontal, OrbitSpacing.lg)
                        .frame(minWidth: 240, minHeight: OrbitSpacing.hitTarget)
                        .background(palette.primary)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }
                    .buttonStyle(.plain)
                }

                Button {
                    store.hapticLight()
                    store.activeSheet = .qrPair
                } label: {
                    HStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: "qrcode.viewfinder")
                            .font(OrbitTypography.icon)
                        Text("Scan QR Code")
                            .font(OrbitTypography.subhead(.semibold))
                    }
                    .foregroundStyle(store.localDaemonUri == nil ? palette.onPrimary : palette.fgPrimary)
                    .padding(.horizontal, OrbitSpacing.lg)
                    .frame(minWidth: 240, minHeight: OrbitSpacing.hitTarget)
                    .background(store.localDaemonUri == nil ? palette.primary : palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .strokeBorder(store.localDaemonUri == nil ? Color.clear : palette.hairline, lineWidth: 0.5)
                    )
                }
                .buttonStyle(.plain)

                if store.pasteboardHasStrings {
                    Button {
                        if let uri = store.readPairingUriFromClipboard() {
                            _ = store.pairHost(uri: uri)
                        } else {
                            store.activeSheet = .qrPair
                        }
                    } label: {
                        HStack(spacing: OrbitSpacing.xs) {
                            Image(systemName: "doc.on.clipboard")
                                .font(OrbitTypography.iconSmall)
                            Text("Paste Pairing URI")
                                .font(OrbitTypography.subhead(.medium))
                        }
                        .foregroundStyle(palette.fgSecondary)
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }

            Spacer()
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - New Session Minimalist Zen Canvas (Plan B)

    private var activeProjectLabel: String {
        if let matched = store.launchpadProjects.first(where: {
            OrbitFormatters.arePathsEquivalent($0.path, store.activeCwd)
        }) {
            return matched.name
        }
        return OrbitFormatters.shortPath(store.activeCwd)
    }

    private func isHostOnline(_ host: FfiHostCard) -> Bool {
        switch host.connectionState {
        case .onlineDirect, .onlineRelay:
            return true
        case .connectingDirect, .connectingRelay, .disconnected:
            return false
        }
    }

    private var activeLaunchpadProject: FfiLaunchpadProject? {
        store.launchpadProjects.first(where: {
            OrbitFormatters.arePathsEquivalent($0.path, store.activeCwd)
        })
    }

    private var activeGitInspection: FfiGitReposInspection? {
        store.gitReposInspection(for: store.activeCwd)
    }

    private var isActiveFolderGitRepo: Bool {
        if let insp = activeGitInspection { return insp.isGitRepo }
        if let proj = activeLaunchpadProject { return proj.isGitRepo }
        return store.activeGitBranch != nil
    }

    private var activeNestedGitRepos: [FfiNestedGitRepo] {
        if let insp = activeGitInspection, !insp.nestedGitRepos.isEmpty {
            return insp.nestedGitRepos
        }
        return activeLaunchpadProject?.nestedGitRepos ?? []
    }

    private var activeWorktreeLabel: String {
        guard store.draftUseWorktree else { return "Local" }
        if let nestedPath = store.draftWorktreeRepoPath, !nestedPath.isEmpty {
            return "Worktree · \(OrbitFormatters.shortPath(nestedPath))"
        }
        return "Worktree"
    }

    private var draftSelectedWorkspaceTitle: String {
        if let wsId = store.draftWorkspaceId,
           let ws = store.activeProjectCuratedWorkspaces.first(where: { $0.workspaceId == wsId }) {
            return ws.title
        }
        return "待归类"
    }

    private var newSessionZeroState: some View {
        GeometryReader { geo in
            ScrollView(.vertical, showsIndicators: false) {
                VStack(spacing: 0) {
                    Spacer(minLength: OrbitSpacing.xl)

                    Text("What would you like to build?")
                        .font(.system(size: 23, weight: .medium, design: .default))
                        .tracking(-0.35)
                        .foregroundStyle(palette.fgPrimary)
                        .multilineTextAlignment(.center)
                        .lineLimit(1)
                        .minimumScaleFactor(0.84)

                    // Compact Contextual Readiness Pills: [Project ▾] [Workspace ▾] [Host ▾] [Local / Worktree ▾]
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: OrbitSpacing.sm) {
                            Button {
                                store.hapticLight()
                                projectSearchQuery = ""
                                isProjectPickerPresented.toggle()
                            } label: {
                                launchpadPillLabel {
                                    Image(systemName: "folder")
                                        .font(.system(size: 12, weight: .medium))
                                        .foregroundStyle(palette.fgSecondary)
                                    Text(activeProjectLabel)
                                        .font(OrbitTypography.subhead(.medium))
                                        .foregroundStyle(palette.fgPrimary)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                    Image(systemName: "chevron.down")
                                        .font(.system(size: 8.5, weight: .semibold))
                                        .foregroundStyle(palette.fgMuted)
                                }
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("Project: \(activeProjectLabel)")
                            .orbitCompactPopover(
                                isPresented: $isProjectPickerPresented,
                                arrowEdge: .top,
                                palette: palette
                            ) {
                                projectPickerPopover
                            }

                            if store.isDraftNewSession && !store.activeProjectCuratedWorkspaces.isEmpty {
                                Button {
                                    store.hapticLight()
                                    isWorkspacePickerPresented.toggle()
                                } label: {
                                    launchpadPillLabel {
                                        Image(systemName: store.draftWorkspaceId != nil ? "square.stack.3d.up" : "tray")
                                            .font(.system(size: 11.5, weight: .medium))
                                            .foregroundStyle(store.draftWorkspaceId != nil ? palette.accentViolet : palette.fgSecondary)
                                        Text(draftSelectedWorkspaceTitle)
                                            .font(OrbitTypography.subhead(.medium))
                                            .foregroundStyle(palette.fgPrimary)
                                            .lineLimit(1)
                                        Image(systemName: "chevron.down")
                                            .font(.system(size: 8.5, weight: .semibold))
                                            .foregroundStyle(palette.fgMuted)
                                    }
                                }
                                .buttonStyle(.plain)
                                .accessibilityLabel("Workspace: \(draftSelectedWorkspaceTitle)")
                                .orbitCompactPopover(
                                    isPresented: $isWorkspacePickerPresented,
                                    arrowEdge: .top,
                                    palette: palette
                                ) {
                                    draftWorkspacePickerPopover
                                }
                            }

                            if store.hosts.count > 1, let activeHost = store.activeHost {
                                Button {
                                    store.hapticLight()
                                    isHostPickerPresented.toggle()
                                } label: {
                                    launchpadPillLabel {
                                        Circle()
                                            .fill(palette.connectionColor(activeHost.connectionState))
                                            .frame(width: 6, height: 6)
                                        Text(activeHost.displayName)
                                            .font(OrbitTypography.subhead(.medium))
                                            .foregroundStyle(palette.fgPrimary)
                                            .lineLimit(1)
                                        Image(systemName: "chevron.down")
                                            .font(.system(size: 8.5, weight: .semibold))
                                            .foregroundStyle(palette.fgMuted)
                                    }
                                }
                                .buttonStyle(.plain)
                                .accessibilityLabel("Host: \(activeHost.displayName)")
                                .orbitCompactPopover(
                                    isPresented: $isHostPickerPresented,
                                    arrowEdge: .top,
                                    palette: palette
                                ) {
                                    OrbitHostPickerPopover(
                                        store: store,
                                        showPairingActions: false,
                                        onSelectHost: { hostId in
                                            store.hapticLight()
                                            store.selectHost(hostId)
                                            isHostPickerPresented = false
                                        }
                                    )
                                }
                            }

                            if store.isDraftNewSession {
                                Button {
                                    store.hapticLight()
                                    store.requestGitReposInspection(for: store.activeCwd)
                                    isEnvironmentPickerPresented.toggle()
                                } label: {
                                    launchpadPillLabel {
                                        Image(systemName: store.draftUseWorktree ? "arrow.triangle.branch" : "laptopcomputer")
                                            .font(.system(size: 12, weight: .medium))
                                            .foregroundStyle(palette.fgSecondary)
                                        Text(activeWorktreeLabel)
                                            .font(OrbitTypography.subhead(.medium))
                                            .foregroundStyle(palette.fgPrimary)
                                            .lineLimit(1)
                                        Image(systemName: "chevron.down")
                                            .font(.system(size: 8.5, weight: .semibold))
                                            .foregroundStyle(palette.fgMuted)
                                    }
                                }
                                .buttonStyle(.plain)
                                .accessibilityLabel("Execution environment: \(activeWorktreeLabel)")
                                .orbitCompactPopover(
                                    isPresented: $isEnvironmentPickerPresented,
                                    arrowEdge: .top,
                                    palette: palette
                                ) {
                                    environmentPickerPopover
                                }
                            }
                        }
                        .padding(.horizontal, 4)
                    }
                    .padding(.top, 14)

                    // Quiet single-line Git branch or host connection caption
                    if let host = store.activeHost, !isHostOnline(host) {
                        HStack(spacing: 5) {
                            Circle()
                                .fill(palette.connectionColor(host.connectionState))
                                .frame(width: 6, height: 6)
                            Text("\(OrbitFormatters.compactTopBarConnectionLabel(for: host)) · \(host.displayName)")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .lineLimit(1)
                        }
                        .padding(.top, 10)
                    } else if let branch = store.activeGitBranch, !branch.isEmpty {
                        HStack(spacing: 4) {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.system(size: 10.5, weight: .medium))
                                .foregroundStyle(palette.fgMuted)
                            Text(store.draftUseWorktree ? "Worktree from \(branch)" : branch)
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                        .padding(.top, 10)
                    } else if !isActiveFolderGitRepo {
                        Text(activeNestedGitRepos.isEmpty ? "Local folder (not a Git repo)" : "Multi-repo folder · \(activeNestedGitRepos.count) nested Git repos")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .padding(.top, 10)
                    }

                    Spacer(minLength: OrbitSpacing.xl)
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.bottom, 24)
                .frame(minWidth: geo.size.width, minHeight: geo.size.height)
            }
            .orbitSoftScrollEdgeEffect()
            .scrollDismissesKeyboard(.interactively)
            .refreshable {
                await store.pullToRefreshStage()
            }
        }
    }

    private var filteredLaunchpadProjects: [FfiLaunchpadProject] {
        let q = projectSearchQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return store.launchpadProjects }
        return store.launchpadProjects.filter { proj in
            proj.name.lowercased().contains(q)
                || proj.path.lowercased().contains(q)
                || (proj.branch?.lowercased().contains(q) ?? false)
        }
    }

    private var projectPickerPopover: some View {
        VStack(alignment: .leading, spacing: 6) {
            OrbitPopoverSectionHeader(title: "Projects")
                .padding(.horizontal, 10)

            HStack(spacing: 6) {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: 12))
                    .foregroundStyle(palette.fgMuted)
                TextField("Filter projects…", text: $projectSearchQuery)
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgPrimary)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }
            .padding(.horizontal, 8)
            .frame(height: 30)
            .background(palette.subtleFill, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .padding(.horizontal, 10)

            ViewThatFits(in: .vertical) {
                projectRowsStack
                ScrollView(.vertical, showsIndicators: true) {
                    projectRowsStack
                }
                .frame(height: 220)
            }
            .frame(maxHeight: 220)

            OrbitHairline()
                .padding(.horizontal, 20)
                .padding(.vertical, 2)

            OrbitPopoverActionRow(
                title: "Add new project…",
                systemImage: "folder.badge.plus"
            ) {
                isProjectPickerPresented = false
                store.openDirectoryBrowser()
            }
            .padding(.horizontal, 10)
        }
        .padding(.vertical, 10)
        .frame(width: 268)
    }

    private var projectRowsStack: some View {
        VStack(alignment: .leading, spacing: 2) {
            if filteredLaunchpadProjects.isEmpty {
                Text(projectSearchQuery.isEmpty ? "No projects" : "No matching projects")
                    .font(.system(size: 12.5, weight: .regular))
                    .foregroundStyle(palette.fgMuted)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 12)
            } else {
                ForEach(filteredLaunchpadProjects) { proj in
                    let isSelected = OrbitFormatters.arePathsEquivalent(proj.path, store.activeCwd)
                    OrbitPopoverOptionRow(
                        title: proj.name,
                        subtitle: proj.branch,
                        systemImage: "folder",
                        isSelected: isSelected
                    ) {
                        store.draftWorkspaceId = nil
                        store.selectWorkspaceDirectory(proj.path)
                        isProjectPickerPresented = false
                    }
                }
            }
        }
        .padding(.horizontal, 10)
    }

    private var draftWorkspacePickerPopover: some View {
        VStack(alignment: .leading, spacing: 6) {
            OrbitPopoverSectionHeader(title: "Target Workspace")
                .padding(.horizontal, 10)

            VStack(alignment: .leading, spacing: 2) {
                OrbitPopoverOptionRow(
                    title: "待归类",
                    subtitle: "Start standalone in project inbox",
                    systemImage: "tray",
                    isSelected: store.draftWorkspaceId == nil
                ) {
                    store.draftWorkspaceId = nil
                    isWorkspacePickerPresented = false
                }

                ForEach(store.activeProjectCuratedWorkspaces, id: \.workspaceId) { ws in
                    OrbitPopoverOptionRow(
                        title: ws.title,
                        subtitle: ws.missionGoal,
                        systemImage: "square.stack.3d.up",
                        isSelected: store.draftWorkspaceId == ws.workspaceId
                    ) {
                        store.draftWorkspaceId = ws.workspaceId
                        isWorkspacePickerPresented = false
                    }
                }
            }
            .padding(.horizontal, 10)
        }
        .padding(.vertical, 10)
        .frame(width: 268)
    }

    private var environmentPickerPopover: some View {
        let canWorktreeRoot = isActiveFolderGitRepo
        let nestedRepos = activeNestedGitRepos

        return VStack(alignment: .leading, spacing: 6) {
            OrbitPopoverSectionHeader(
                title: "Execution",
                trailing: store.activeGitBranch
            )

            VStack(alignment: .leading, spacing: 2) {
                OrbitPopoverOptionRow(
                    title: "Local",
                    subtitle: "Work directly in project folder",
                    systemImage: "laptopcomputer",
                    isSelected: !store.draftUseWorktree
                ) {
                    store.draftWorktreeRepoPath = nil
                    store.setDraftUseWorktree(false)
                    isEnvironmentPickerPresented = false
                }

                if canWorktreeRoot {
                    OrbitPopoverOptionRow(
                        title: "Worktree",
                        subtitle: "Isolated Git worktree for parallel tasks",
                        systemImage: "arrow.triangle.branch",
                        isSelected: store.draftUseWorktree && store.draftWorktreeRepoPath == nil
                    ) {
                        store.draftWorktreeRepoPath = nil
                        store.setDraftUseWorktree(true)
                        isEnvironmentPickerPresented = false
                    }
                } else if !nestedRepos.isEmpty {
                    OrbitHairline()
                        .padding(.horizontal, 10)
                        .padding(.vertical, 2)

                    OrbitPopoverSectionHeader(title: "Worktree in Nested Repo")

                    ForEach(nestedRepos) { repo in
                        let isSelected = store.draftUseWorktree
                            && OrbitFormatters.arePathsEquivalent(store.draftWorktreeRepoPath ?? "", repo.path)
                        OrbitPopoverOptionRow(
                            title: repo.name,
                            subtitle: repo.gitBranch ?? repo.relativePath,
                            systemImage: "arrow.triangle.branch",
                            isSelected: isSelected
                        ) {
                            store.draftWorktreeRepoPath = repo.path
                            store.setDraftUseWorktree(true)
                            isEnvironmentPickerPresented = false
                        }
                    }
                } else {
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: "arrow.triangle.branch")
                            .font(.system(size: 13.5, weight: .medium))
                            .foregroundStyle(palette.fgMuted.opacity(0.6))
                            .frame(width: 16)
                            .padding(.top, 2)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Worktree unavailable")
                                .font(.system(size: 13, weight: .medium))
                                .foregroundStyle(palette.fgMuted)
                            Text("Selected folder is not a Git repository")
                                .font(.system(size: 11.5))
                                .foregroundStyle(palette.fgMuted.opacity(0.8))
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                }
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 10)
        .frame(width: 292)
    }

    private func launchpadPillLabel<Content: View>(
        @ViewBuilder content: () -> Content
    ) -> some View {
        HStack(spacing: 5) {
            content()
        }
        .padding(.horizontal, 11)
        .frame(height: 32)
        .background(palette.subtleFill)
        .clipShape(Capsule())
        .contentShape(Capsule())
    }
}
