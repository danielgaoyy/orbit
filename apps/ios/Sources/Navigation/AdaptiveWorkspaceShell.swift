import SwiftUI

struct AdaptiveWorkspaceShell: View {
    @ObservedObject var store: OrbitIosStore
    @Binding var appearanceRaw: String

    @Environment(\.orbitPalette) private var palette

    @State private var activeSheetDetent: PresentationDetent = .large
    @State private var renamePrimaryField: String = ""
    @State private var renameSecondaryField: String = ""
    @GestureState private var drawerDragTranslation: CGFloat = 0

    private var appearanceMode: OrbitAppearanceMode {
        OrbitAppearanceMode.fromStorage(appearanceRaw)
    }

    var body: some View {
        GeometryReader { geo in
            let width = geo.size.width
            let isSplitLayout = UIDevice.current.userInterfaceIdiom == .pad && width >= 700
            let sidebarOccupied: CGFloat = store.isPadSidebarCollapsed ? 0 : OrbitSpacing.sidebarWidth
            let canShowInlineInspector = isSplitLayout && ((width - sidebarOccupied) >= 780 || width >= 1024)

            ZStack(alignment: .top) {
                if isSplitLayout {
                    regularSplitShell(canShowInlineInspector: canShowInlineInspector)
                } else {
                    compactDrawerShell(availableWidth: width)
                }

                toastAndErrorOverlay
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .onAppear {
                store.updateContainerWidth(width)
            }
            .onChange(of: geo.size.width) { newWidth in
                store.updateContainerWidth(newWidth)
            }
        }
        .background(palette.canvas.ignoresSafeArea())
        .onChange(of: store.activeSheet) { newDestination in
            if let dest = newDestination {
                activeSheetDetent = dest.defaultDetent
            }
        }
        .sheet(item: $store.activeSheet, onDismiss: {
            if let next = store.pendingSheetDestination {
                store.pendingSheetDestination = nil
                DispatchQueue.main.async {
                    store.activeSheet = next
                }
            }
        }) { destination in
            sheetContent(for: destination)
                .environment(\.orbitPalette, palette)
                .preferredColorScheme(appearanceMode.preferredColorScheme)
                .presentationDetents([.medium, .large], selection: $activeSheetDetent)
                .presentationDragIndicator(.visible)
                .onAppear {
                    activeSheetDetent = destination.defaultDetent
                }
        }
        .sheet(item: $store.subagentInspectorTarget) { target in
            SubagentInspectorSheet(store: store, target: target)
                .environment(\.orbitPalette, palette)
                .preferredColorScheme(appearanceMode.preferredColorScheme)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        }
        .onChange(of: store.renameTarget) { target in
            if target != nil && store.activeSheet != nil {
                store.activeSheet = nil
            }
            switch target {
            case let .agent(_, _, currentTitle, currentHandle):
                renamePrimaryField = currentTitle
                renameSecondaryField = currentHandle ?? ""
            case let .workspace(_, _, currentTitle):
                renamePrimaryField = currentTitle
                renameSecondaryField = ""
            case let .project(_, currentName):
                renamePrimaryField = currentName
                renameSecondaryField = ""
            case nil:
                break
            }
        }
        .modifier(OrbitRenameAlertModifier(
            store: store,
            title: renameAlertTitle,
            primaryField: $renamePrimaryField,
            secondaryField: $renameSecondaryField,
            onCommit: { commitRename() }
        ))
    }

    private var renameAlertTitle: String {
        switch store.renameTarget {
        case .agent: return "Rename Conversation"
        case .workspace: return "Rename Workspace"
        case .project: return "Rename Project"
        case nil: return "Rename"
        }
    }

    private func commitRename() {
        guard let target = store.renameTarget else { return }
        let title = renamePrimaryField.trimmingCharacters(in: .whitespacesAndNewlines)
        switch target {
        case let .agent(hid, aid, _, _):
            let handle = renameSecondaryField
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .trimmingCharacters(in: CharacterSet(charactersIn: "@"))
            store.updateAgentIdentity(
                hostId: hid,
                agentId: aid,
                handle: handle.isEmpty ? nil : handle,
                title: title.isEmpty ? nil : title,
                lockHandle: !handle.isEmpty,
                lockTitle: !title.isEmpty
            )
        case let .workspace(hid, wid, _):
            if !title.isEmpty {
                store.renameWorkspace(hostId: hid, workspaceId: wid, title: title)
            }
        case let .project(pid, _):
            if !title.isEmpty {
                store.renameProject(projectId: pid, newName: title)
            }
        }
        store.renameTarget = nil
    }

    // MARK: - iPad Responsive Split Workspace (>= 700pt)

    private func regularSplitShell(canShowInlineInspector: Bool) -> some View {
        HStack(spacing: 0) {
            if !store.isPadSidebarCollapsed {
                FleetDrawerView(store: store, isCompactOverlay: false)
                    .frame(width: OrbitSpacing.sidebarWidth)
                    .transition(.move(edge: .leading).combined(with: .opacity))
                OrbitHairline(axis: .vertical)
            }

            WorkspaceStagePane(
                store: store,
                showSidebarButton: true,
                canShowInlineInspector: canShowInlineInspector,
                onToggleSidebar: {
                    store.hapticLight()
                    withAnimation(.easeOut(duration: 0.2)) {
                        store.togglePadSidebarCollapsed()
                    }
                }
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.canvas)

            if canShowInlineInspector && store.isPadDiffInspectorVisible {
                OrbitHairline(axis: .vertical)
                PadMultiTabInspectorView(store: store)
                    .frame(width: OrbitSpacing.inspectorWidth)
                    .transition(.move(edge: .trailing).combined(with: .opacity))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - iPhone Full-Screen Stage + Interactive Left-Edge Drawer

    private func compactDrawerWidth(for availableWidth: CGFloat) -> CGFloat {
        min(336, max(280, floor(availableWidth * 0.82)))
    }

    private func compactDrawerProgress(drawerWidth: CGFloat) -> CGFloat {
        let base: CGFloat = store.isDrawerOpen ? drawerWidth : 0
        let current = min(drawerWidth, max(0, base + drawerDragTranslation))
        return current / drawerWidth
    }

    private func compactDrawerOffset(drawerWidth: CGFloat) -> CGFloat {
        -drawerWidth * (1.0 - compactDrawerProgress(drawerWidth: drawerWidth))
    }

    private func compactDrawerShell(availableWidth: CGFloat) -> some View {
        let drawerWidth = compactDrawerWidth(for: availableWidth)
        let progress = compactDrawerProgress(drawerWidth: drawerWidth)
        let drawerOffset = compactDrawerOffset(drawerWidth: drawerWidth)
        let stageOffset = drawerWidth * progress

        return ZStack(alignment: .leading) {
            WorkspaceStagePane(
                store: store,
                showSidebarButton: true,
                canShowInlineInspector: false,
                onToggleSidebar: {
                    store.hapticLight()
                    withAnimation(.spring(response: 0.28, dampingFraction: 0.90)) {
                        store.isDrawerOpen.toggle()
                    }
                }
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.canvas.ignoresSafeArea())
            .simultaneousGesture(edgeOpenGesture, including: store.isDrawerOpen ? .none : .all)
            .overlay {
                if progress > 0.001 {
                    Color.black
                        .opacity((palette.isDark ? 0.44 : 0.22) * Double(progress))
                        .ignoresSafeArea()
                        .onTapGesture {
                            withAnimation(.spring(response: 0.28, dampingFraction: 0.90)) {
                                store.isDrawerOpen = false
                            }
                        }
                        .gesture(drawerCloseGesture)
                }
            }
            .offset(x: stageOffset)

            // Full-Height Edge-to-Edge Push-Over Drawer (lazily mounted only when open or being dragged)
            if store.isDrawerOpen || drawerDragTranslation > 0 {
                compactDrawerPanel(drawerWidth: drawerWidth)
                    .offset(x: drawerOffset)
                    .opacity(progress > 0.001 ? 1 : 0)
                    .allowsHitTesting(store.isDrawerOpen || progress > 0.05)
            }
        }
    }

    private func compactDrawerPanel(drawerWidth: CGFloat) -> some View {
        HStack(spacing: 0) {
            FleetDrawerView(store: store, isCompactOverlay: true)
                .frame(width: drawerWidth)
                .frame(maxHeight: .infinity)
                .background(palette.drawer.ignoresSafeArea())

            Rectangle()
                .fill(palette.hairline)
                .frame(width: 0.5)
                .ignoresSafeArea()
        }
    }

    private var edgeOpenGesture: some Gesture {
        DragGesture(minimumDistance: 20, coordinateSpace: .global)
            .updating($drawerDragTranslation) { value, state, _ in
                let isLeftEdge = value.startLocation.x < 24
                let isHorizontal = abs(value.translation.width) > abs(value.translation.height) * 1.6
                if isLeftEdge && isHorizontal && value.translation.width > 0 {
                    state = value.translation.width
                }
            }
            .onEnded { value in
                let isLeftEdge = value.startLocation.x < 24
                let isHorizontal = abs(value.translation.width) > abs(value.translation.height) * 1.6
                guard isLeftEdge && isHorizontal else { return }
                let shouldOpen = value.translation.width > 72 || value.predictedEndTranslation.width > 140
                withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                    store.isDrawerOpen = shouldOpen
                }
                if shouldOpen {
                    store.hapticLight()
                }
            }
    }

    private var drawerCloseGesture: some Gesture {
        DragGesture(minimumDistance: 16, coordinateSpace: .global)
            .updating($drawerDragTranslation) { value, state, _ in
                let isHorizontal = abs(value.translation.width) > abs(value.translation.height) * 1.6
                if isHorizontal && value.translation.width < 0 {
                    state = value.translation.width
                }
            }
            .onEnded { value in
                let isHorizontal = abs(value.translation.width) > abs(value.translation.height) * 1.6
                guard isHorizontal else { return }
                let shouldClose = value.translation.width < -64 || value.predictedEndTranslation.width < -130
                withAnimation(.spring(response: 0.26, dampingFraction: 0.88)) {
                    if shouldClose {
                        store.isDrawerOpen = false
                    }
                }
            }
    }

    // MARK: - Sheet Routing

    @ViewBuilder
    private func sheetContent(for destination: OrbitSheetDestination) -> some View {
        switch destination {
        case .gitDiff:
            GitDiffSheet(store: store)
        case .pty:
            NativePtySheet(store: store)
        case .qrPair:
            QrPairSheet(store: store)
        case .directoryBrowser:
            DirectoryBrowserSheet(store: store)
        case .settings:
            SettingsAndQuotasSheet(store: store, appearanceRaw: $appearanceRaw)
        case let .workspaceBoard(workspaceId):
            WorkspaceBoardSheet(store: store, workspaceId: workspaceId)
        case .sessionOverview:
            SessionOverviewSheet(store: store)
        case .sideQuestion:
            SideQuestionSheet(store: store)
        case let .workspaceFiles(initialPath):
            WorkspaceFilesSheet(store: store, initialPath: initialPath)
        case .automations:
            AutomationsSheet(store: store)
        case .archivedThreads:
            ArchivedThreadsSheet(store: store)
        case let .spawnPeer(workspaceId):
            SpawnPeerSheet(store: store, workspaceId: workspaceId)
        case let .createWorkspace(projectRoot, projectName, isGitRepo, moveAgentId):
            CreateWorkspaceSheet(
                store: store,
                projectRoot: projectRoot,
                projectName: projectName,
                isGitRepo: isGitRepo,
                moveAgentId: moveAgentId
            )
        }
    }

    // MARK: - Subtle Toast & Error Banner

    @ViewBuilder
    private var toastAndErrorOverlay: some View {
        VStack(spacing: OrbitSpacing.sm) {
            if let undo = store.pendingArchiveUndo {
                HStack(spacing: OrbitSpacing.sm) {
                    Image(systemName: "archivebox")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.fgSecondary)
                    Text("Archived \"\(undo.title)\"")
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)
                    Button("Undo") {
                        store.undoPendingArchive()
                    }
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.primary)
                    Button {
                        store.dismissArchiveUndo()
                    } label: {
                        Image(systemName: "xmark")
                            .font(OrbitTypography.iconMicro)
                            .foregroundStyle(palette.fgMuted)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, OrbitSpacing.md)
                .padding(.vertical, OrbitSpacing.sm)
                .orbitGlassSurface(
                    in: Capsule(),
                    fallbackFill: palette.elevated,
                    fallbackBorder: palette.hairline,
                    fallbackShadowOpacity: 0.14
                )
                .transition(.move(edge: .top).combined(with: .opacity))
            } else if let message = store.toastMessage {
                HStack(spacing: OrbitSpacing.sm) {
                    Image(systemName: "checkmark.circle.fill")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.direct)
                    Text(message)
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgPrimary)
                }
                .padding(.horizontal, OrbitSpacing.md)
                .padding(.vertical, OrbitSpacing.sm)
                .orbitGlassSurface(
                    in: Capsule(),
                    fallbackFill: palette.elevated,
                    fallbackBorder: palette.hairline,
                    fallbackShadowOpacity: 0.12
                )
                .transition(.move(edge: .top).combined(with: .opacity))
            }

            if let err = store.lastError {
                HStack(spacing: OrbitSpacing.sm) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.danger)
                    Text(err)
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(2)
                    Button {
                        store.clearError()
                    } label: {
                        Image(systemName: "xmark")
                            .font(OrbitTypography.iconMicro)
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Dismiss Error")
                }
                .padding(.horizontal, OrbitSpacing.md)
                .padding(.vertical, OrbitSpacing.sm)
                .orbitGlassSurface(
                    in: RoundedRectangle(cornerRadius: 16, style: .continuous),
                    tint: palette.danger.opacity(0.12),
                    fallbackFill: palette.elevated,
                    fallbackBorder: palette.danger.opacity(0.35),
                    fallbackShadowOpacity: 0.10
                )
                .padding(.horizontal, OrbitSpacing.lg)
                .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .padding(.top, 64)
    }
}

// MARK: - Shared Workspace Header Context, Title Trigger & Overflow Menu

@MainActor
private struct WorkspaceHeaderContext {
    let store: OrbitIosStore
    let canShowInlineInspector: Bool

    var primaryTitle: String {
        if store.hosts.isEmpty {
            return "Orbit"
        }
        if let agent = store.activeAgent {
            let trimmed = agent.title.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty {
                return trimmed
            }
        }
        return "New Conversation"
    }

    var isDraftTitle: Bool {
        !store.hosts.isEmpty && store.activeAgent == nil
    }

    var isMultiHost: Bool {
        store.hosts.count > 1
    }

    func isConnectionHealthy(for host: FfiHostCard) -> Bool {
        switch host.connectionState {
        case .onlineDirect, .onlineRelay:
            return true
        case .connectingDirect, .connectingRelay, .disconnected:
            return false
        }
    }

    func subtitleText(for host: FfiHostCard) -> String {
        let repo = OrbitFormatters.shortPath(store.activeCwd)
        let rawHost = host.displayName.trimmingCharacters(in: .whitespacesAndNewlines)
        let hostName = rawHost.isEmpty ? host.hostId : rawHost
        let branch = store.activeGitBranch?.trimmingCharacters(in: .whitespacesAndNewlines)

        if !isConnectionHealthy(for: host) {
            let status = OrbitFormatters.compactTopBarConnectionLabel(for: host)
            let target = isMultiHost ? hostName : repo
            return "\(status) · \(target)"
        }
        if isMultiHost {
            return "\(hostName) / \(repo)"
        }
        if let branch = branch, !branch.isEmpty {
            return "\(repo) · \(branch)"
        }
        return repo
    }

    var terminalCount: Int {
        store.currentHostDetail?.terminals.count ?? 0
    }

    var diffSubtitle: String? {
        let totals = store.diffTotals
        guard totals.fileCount > 0 else {
            return nil
        }
        let unit = totals.fileCount == 1 ? "file" : "files"
        return "\(totals.fileCount) \(unit)"
    }
}

private struct WorkspaceHeaderTitleTrigger: View {
    @ObservedObject var store: OrbitIosStore
    let canShowInlineInspector: Bool

    @Environment(\.orbitPalette) private var palette

    private var ctx: WorkspaceHeaderContext {
        WorkspaceHeaderContext(store: store, canShowInlineInspector: canShowInlineInspector)
    }

    var body: some View {
        if let host = store.activeHost {
            Menu {
                Button {
                    store.hapticLight()
                    store.openDirectoryBrowser()
                } label: {
                    Label("Open Folder", systemImage: "folder")
                }

                Button {
                    store.copyPathToClipboard(store.activeCwd)
                } label: {
                    Label("Copy Path", systemImage: "square.on.square")
                }
            } label: {
                VStack(alignment: .leading, spacing: 1) {
                    Text(ctx.primaryTitle)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(ctx.isDraftTitle ? palette.fgPrimary.opacity(0.76) : palette.fgPrimary)
                        .lineLimit(1)

                    HStack(spacing: OrbitSpacing.xs) {
                        Circle()
                            .fill(palette.connectionColor(host.connectionState))
                            .frame(width: 6, height: 6)

                        Text(ctx.subtitleText(for: host))
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgSecondary)
                            .lineLimit(1)

                        Image(systemName: "chevron.down")
                            .font(.system(size: 8.5, weight: .semibold))
                            .foregroundStyle(palette.fgMuted)
                    }
                }
                .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .tint(palette.fgPrimary)
            .accessibilityLabel("\(ctx.primaryTitle), \(ctx.subtitleText(for: host))")
        } else {
            Button {
                store.activeSheet = .qrPair
            } label: {
                VStack(alignment: .leading, spacing: 1) {
                    Text("Orbit")
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    Text("No Host Paired")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                }
                .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Pair Host")
        }
    }
}

@MainActor
@ViewBuilder
private func workspaceHeaderMoreMenuItems(
    store: OrbitIosStore,
    canShowInlineInspector: Bool,
    ctx: WorkspaceHeaderContext
) -> some View {
    Section {
        if store.activeAgent != nil {
            Button {
                store.openSessionOverview()
            } label: {
                let bgCount = store.activeRunningBackgroundTaskCount
                Text("Session Overview")
                if bgCount > 0 {
                    Text("\(bgCount) running")
                }
                Image(systemName: "sidebar.right")
            }
        }

        if let ws = store.activeWorkspace, ws.isCurated || store.activeWorkspacePeers.count > 1 {
            Button {
                store.openWorkspaceBoard(workspaceId: ws.workspaceId)
            } label: {
                Text("Mission Board")
                if ws.boardRevision > 0 {
                    Text("v\(ws.boardRevision)")
                }
                Image(systemName: "doc.richtext")
            }
        }

        Button {
            store.openWorkspaceFiles()
        } label: {
            Text("Files")
            if !store.workspaceFilesList.isEmpty {
                Text("\(store.workspaceFilesList.count)")
            }
            Image(systemName: "folder")
        }

        Button {
            store.hapticLight()
            if canShowInlineInspector {
                store.refreshGitReview()
                store.padInspectorTab = .diff
                withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                    store.isPadDiffInspectorVisible = true
                }
            } else {
                store.openGitDiff()
            }
        } label: {
            Text("Diff Review")
            if let diffSubtitle = ctx.diffSubtitle {
                Text(diffSubtitle)
            }
            Image(systemName: "arrow.left.arrow.right")
        }

        Button {
            store.hapticLight()
            store.activeSheet = .pty
        } label: {
            Text("Terminal")
            if ctx.terminalCount > 0 {
                Text("\(ctx.terminalCount) active")
            }
            Image(systemName: "terminal")
        }

        Button {
            store.openSideQuestion()
        } label: {
            Text("Side Question (/btw)")
            if !store.activeSideQuestions.isEmpty {
                Text("\(store.activeSideQuestions.count)")
            }
            Image(systemName: "bubble.left.and.text.bubble.right")
        }
    }

    if let agent = store.activeAgent {
        Section {
            if let hid = store.selectedHostId {
                Button {
                    store.renameTarget = .agent(
                        hostId: hid,
                        agentId: agent.agentId,
                        currentTitle: agent.title,
                        currentHandle: agent.handle
                    )
                } label: {
                    Label("Rename Conversation", systemImage: "pencil")
                }
            }

            Button {
                store.summarizeAgent(agent.agentId)
            } label: {
                Label("AI Summarize Session", systemImage: "sparkles")
            }

            Button(role: .destructive) {
                store.hapticLight()
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
    }
}

// MARK: - Workspace Stage Pane (iOS 26+ Native Floating Liquid Glass NavigationStack + Pre-26 Fallback)

private struct WorkspaceStagePane: View {
    @ObservedObject var store: OrbitIosStore
    let showSidebarButton: Bool
    let canShowInlineInspector: Bool
    let onToggleSidebar: () -> Void

    @Environment(\.orbitPalette) private var palette

    var body: some View {
        if #available(iOS 26.0, *) {
            let ctx = WorkspaceHeaderContext(
                store: store,
                canShowInlineInspector: canShowInlineInspector
            )
            NavigationStack {
                applyNavigationHeader(StreamStageView(store: store), ctx: ctx)
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        if showSidebarButton {
                            ToolbarItem(placement: .topBarLeading) {
                                Button(action: onToggleSidebar) {
                                    Image(systemName: "sidebar.left")
                                        .foregroundStyle(palette.fgPrimary)
                                        .overlay(alignment: .topTrailing) {
                                            if store.totalPendingApprovals > 0 {
                                                Circle()
                                                    .fill(palette.warning)
                                                    .frame(width: 7, height: 7)
                                                    .offset(x: 3, y: -2)
                                            }
                                        }
                                }
                                .accessibilityLabel("Toggle Fleet Sidebar")
                            }
                        }

                        trailingToolbarContent(ctx: ctx)
                    }
            }
        } else {
            VStack(spacing: 0) {
                WorkspaceTopBar(
                    store: store,
                    showSidebarButton: showSidebarButton,
                    canShowInlineInspector: canShowInlineInspector,
                    onToggleSidebar: onToggleSidebar
                )
                OrbitHairline()
                StreamStageView(store: store)
            }
        }
    }

    @available(iOS 26.0, *)
    @ToolbarContentBuilder
    private func trailingToolbarContent(ctx: WorkspaceHeaderContext) -> some ToolbarContent {
        #if compiler(>=6.4)
        if #available(iOS 27.0, *) {
            if store.hosts.isEmpty {
                ToolbarItem(placement: .topBarPinnedTrailing) {
                    Button {
                        store.hapticLight()
                        store.activeSheet = .settings
                    } label: {
                        Label("Settings", systemImage: "gearshape")
                    }
                }
            } else {
                ToolbarOverflowMenu {
                    workspaceHeaderMoreMenuItems(
                        store: store,
                        canShowInlineInspector: canShowInlineInspector,
                        ctx: ctx
                    )
                }

                if store.activeAgent != nil {
                    ToolbarItem(placement: .topBarPinnedTrailing) {
                        Button {
                            store.hapticLight()
                            store.startNewSessionDraft()
                        } label: {
                            Label("New Conversation", systemImage: "square.and.pencil")
                        }
                    }
                }
            }
        } else {
            trailingToolbarContentIOS26(ctx: ctx)
        }
        #else
        trailingToolbarContentIOS26(ctx: ctx)
        #endif
    }

    @available(iOS 26.0, *)
    @ToolbarContentBuilder
    private func trailingToolbarContentIOS26(ctx: WorkspaceHeaderContext) -> some ToolbarContent {
        if store.hosts.isEmpty {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    store.hapticLight()
                    store.activeSheet = .settings
                } label: {
                    Label("Settings", systemImage: "gearshape")
                }
            }
        } else if let agent = store.activeAgent {
            let overviewColor: Color? = {
                guard let hid = store.selectedHostId else { return nil }
                switch store.overviewBadgeTone(forAgent: agent, hostId: hid) {
                case .critical: return palette.warning
                case .accent: return palette.direct
                case .muted: return palette.fgMuted
                case .none: return nil
                }
            }()
            ToolbarItemGroup(placement: .topBarTrailing) {
                if store.activeWorkspace == nil && store.activeProjectCuratedWorkspaces.isEmpty == false {
                    StandaloneGroupButton(store: store)
                }

                Button {
                    store.openSessionOverview()
                } label: {
                    Image(systemName: "sidebar.right")
                        .overlay(alignment: .topTrailing) {
                            if let dotColor = overviewColor {
                                Circle()
                                    .fill(dotColor)
                                    .frame(width: 6.5, height: 6.5)
                                    .offset(x: 2, y: -2)
                            }
                        }
                }
                .accessibilityLabel("Session Overview")

                Menu {
                    workspaceHeaderMoreMenuItems(
                        store: store,
                        canShowInlineInspector: canShowInlineInspector,
                        ctx: ctx
                    )
                } label: {
                    Label("More Actions", systemImage: "ellipsis")
                }

                Button {
                    store.hapticLight()
                    store.startNewSessionDraft()
                } label: {
                    Label("New Conversation", systemImage: "square.and.pencil")
                }
            }
        } else {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    workspaceHeaderMoreMenuItems(
                        store: store,
                        canShowInlineInspector: canShowInlineInspector,
                        ctx: ctx
                    )
                } label: {
                    Label("More Actions", systemImage: "ellipsis")
                }
            }
        }
    }

    @available(iOS 26.0, *)
    @ViewBuilder
    private func applyNavigationHeader<Content: View>(
        _ content: Content,
        ctx: WorkspaceHeaderContext
    ) -> some View {
        if ctx.isDraftTitle {
            content
                .navigationTitle("")
                .navigationSubtitle(Text(""))
        } else {
            content
                .navigationTitle(ctx.primaryTitle)
                .navigationSubtitle(nativeSubtitleText(ctx: ctx))
                .toolbarTitleMenu {
                    if store.activeHost != nil {
                        Button {
                            store.hapticLight()
                            store.openDirectoryBrowser()
                        } label: {
                            Label("Open Folder", systemImage: "folder")
                        }

                        Button {
                            store.copyPathToClipboard(store.activeCwd)
                        } label: {
                            Label("Copy Path", systemImage: "square.on.square")
                        }
                    } else {
                        Button {
                            store.activeSheet = .qrPair
                        } label: {
                            Label("Pair Host", systemImage: "qrcode.viewfinder")
                        }
                    }
                }
        }
    }

    @available(iOS 26.0, *)
    private func nativeSubtitleText(ctx: WorkspaceHeaderContext) -> Text {
        if let host = store.activeHost {
            return Text(ctx.subtitleText(for: host))
        }
        return Text("No Host Paired")
    }
}

// MARK: - Workspace Top Bar (44pt Height, 16pt Horizontal Anchor)

struct WorkspaceTopBar: View {
    @ObservedObject var store: OrbitIosStore
    let showSidebarButton: Bool
    let canShowInlineInspector: Bool
    let onToggleSidebar: () -> Void

    @Environment(\.orbitPalette) private var palette

    var body: some View {
        let ctx = WorkspaceHeaderContext(
            store: store,
            canShowInlineInspector: canShowInlineInspector
        )
        HStack(spacing: OrbitSpacing.xs) {
            if showSidebarButton {
                Button(action: onToggleSidebar) {
                    ZStack(alignment: .topTrailing) {
                        Image(systemName: "sidebar.left")
                            .font(OrbitTypography.icon)
                            .foregroundStyle(palette.fgPrimary)
                            .padding(OrbitSpacing.xs)

                        if store.totalPendingApprovals > 0 {
                            Circle()
                                .fill(palette.warning)
                                .frame(width: 6, height: 6)
                                .offset(x: 1, y: 1)
                        }
                    }
                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Toggle Fleet Sidebar")
                // Shift -10pt so visual icon aligns cleanly with the 16pt left margin
                .padding(.leading, -OrbitSpacing.sm)
            }

            if !ctx.isDraftTitle {
                WorkspaceHeaderTitleTrigger(
                    store: store,
                    canShowInlineInspector: canShowInlineInspector
                )
                .frame(minHeight: OrbitSpacing.hitTarget, alignment: .leading)
            }

            Spacer(minLength: OrbitSpacing.xs)

            if store.hosts.isEmpty {
                Button {
                    store.hapticLight()
                    store.activeSheet = .settings
                } label: {
                    Image(systemName: "gearshape")
                        .font(OrbitTypography.icon)
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Settings")
                .padding(.trailing, -OrbitSpacing.sm)
            } else {
                if let agent = store.activeAgent {
                    let overviewColor: Color? = {
                        guard let hid = store.selectedHostId else { return nil }
                        switch store.overviewBadgeTone(forAgent: agent, hostId: hid) {
                        case .critical: return palette.warning
                        case .accent: return palette.direct
                        case .muted: return palette.fgMuted
                        case .none: return nil
                        }
                    }()
                    if store.activeWorkspace == nil && !store.activeProjectCuratedWorkspaces.isEmpty {
                        StandaloneGroupButton(store: store)
                    }
                    Button {
                        store.openSessionOverview()
                    } label: {
                        ZStack(alignment: .topTrailing) {
                            Image(systemName: "sidebar.right")
                                .font(OrbitTypography.icon)
                                .foregroundStyle(palette.fgPrimary)
                                .padding(OrbitSpacing.xs)
                            if let dotColor = overviewColor {
                                Circle()
                                    .fill(dotColor)
                                    .frame(width: 6, height: 6)
                                    .offset(x: 1, y: 1)
                            }
                        }
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Session Overview")
                }

                Menu {
                    workspaceHeaderMoreMenuItems(
                        store: store,
                        canShowInlineInspector: canShowInlineInspector,
                        ctx: ctx
                    )
                } label: {
                    Image(systemName: "ellipsis")
                        .font(OrbitTypography.icon)
                        .foregroundStyle(palette.fgPrimary)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .tint(palette.fgPrimary)
                .accessibilityLabel("More Actions")
                .padding(.trailing, store.activeAgent == nil ? -OrbitSpacing.sm : 0)

                if store.activeAgent != nil {
                    Button {
                        store.hapticLight()
                        store.startNewSessionDraft()
                    } label: {
                        Image(systemName: "square.and.pencil")
                            .font(OrbitTypography.icon)
                            .foregroundStyle(palette.fgPrimary)
                            .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("New Conversation")
                    .padding(.trailing, -OrbitSpacing.sm)
                }
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .frame(height: OrbitSpacing.hitTarget)
        .background(palette.canvas)
    }
}

// MARK: - iPad 3rd-Column Multi-Tab Inspector (Overview / Diff / Board / Files / Side Q&A)

private struct PadMultiTabInspectorView: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.orbitPalette) private var palette

    private var availableTabs: [OrbitPadInspectorTab] {
        var tabs: [OrbitPadInspectorTab] = [.overview, .diff]
        if let ws = store.activeWorkspace, ws.isCurated || store.activeWorkspacePeers.count > 1 {
            tabs.append(.board)
        }
        tabs.append(.files)
        tabs.append(.sideQuestion)
        return tabs
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 4) {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 4) {
                        ForEach(availableTabs) { tab in
                            let selected = store.padInspectorTab == tab
                            Button {
                                store.hapticLight()
                                store.padInspectorTab = tab
                                if tab == .diff {
                                    store.refreshGitReview()
                                } else if tab == .files && store.workspaceFilesList.isEmpty {
                                    store.refreshWorkspaceFiles()
                                } else if tab == .board, let wsId = store.activeWorkspace?.workspaceId {
                                    store.fetchWorkspaceBoardAndEvents(wsId)
                                }
                            } label: {
                                HStack(spacing: 4) {
                                    Image(systemName: tab.systemImage)
                                        .font(.system(size: 11, weight: .medium))
                                    Text(tab.label)
                                        .font(OrbitTypography.meta(selected ? .semibold : .medium))
                                }
                                .foregroundStyle(selected ? palette.fgPrimary : palette.fgMuted)
                                .padding(.horizontal, 9)
                                .frame(height: 28)
                                .background(selected ? palette.activeRowFill : Color.clear, in: Capsule())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }

                Button {
                    store.hapticLight()
                    withAnimation(.spring(response: 0.26, dampingFraction: 0.88)) {
                        store.isPadDiffInspectorVisible = false
                    }
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 11.5, weight: .semibold))
                        .foregroundStyle(palette.fgMuted)
                        .frame(width: 28, height: 28)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close Inspector")
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(palette.drawer)

            OrbitHairline()

            switch store.padInspectorTab {
            case .overview:
                SessionOverviewPanel(store: store)
            case .diff:
                GitDiffInspectorView(store: store)
            case .board:
                if let wsId = store.activeWorkspace?.workspaceId {
                    WorkspaceBoardPanel(store: store, workspaceId: wsId)
                } else {
                    SessionOverviewPanel(store: store)
                }
            case .files:
                WorkspaceFilesPanel(store: store, initialPath: store.activeWorkspaceFilePath)
            case .sideQuestion:
                SideQuestionPanel(store: store)
            }
        }
        .background(palette.drawer)
        .onChange(of: availableTabs.count) { _ in
            if !availableTabs.contains(store.padInspectorTab) {
                store.padInspectorTab = .overview
            }
        }
    }
}

private struct StandaloneGroupButton: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.orbitPalette) private var palette

    var body: some View {
        Menu {
            ForEach(store.activeProjectCuratedWorkspaces, id: \.workspaceId) { ws in
                Button {
                    if let aid = store.selectedAgentId {
                        store.moveAgentToWorkspace(agentId: aid, targetWorkspaceId: ws.workspaceId)
                    }
                } label: {
                    Label(ws.title, systemImage: "square.stack.3d.up")
                }
            }
            if let aid = store.selectedAgentId {
                Divider()
                Button {
                    let proj = store.activeProjectGroup
                    store.presentSheetSafely(.createWorkspace(
                        projectRoot: store.activeProjectRoot,
                        projectName: proj?.name ?? OrbitFormatters.shortPath(store.activeProjectRoot),
                        isGitRepo: proj?.isGitRepo ?? store.isActiveCwdGitRepo,
                        moveAgentId: aid
                    ))
                } label: {
                    Label("New Mission Workspace…", systemImage: "plus.rectangle.on.folder")
                }
            }
        } label: {
            Image(systemName: "tray.and.arrow.up")
                .font(OrbitTypography.icon)
                .foregroundStyle(palette.fgSecondary)
        }
        .accessibilityLabel("Group Standalone Session")
    }
}

private struct OrbitRenameAlertModifier: ViewModifier {
    @ObservedObject var store: OrbitIosStore
    let title: String
    @Binding var primaryField: String
    @Binding var secondaryField: String
    let onCommit: () -> Void

    func body(content: Content) -> some View {
        content.alert(
            title,
            isPresented: Binding(
                get: { store.renameTarget != nil },
                set: { if !$0 { store.renameTarget = nil } }
            )
        ) {
            TextField("Title", text: $primaryField)
            if case .agent = store.renameTarget {
                TextField("Peer @handle (optional)", text: $secondaryField)
            }
            Button("Save", action: onCommit)
            Button("Cancel", role: .cancel) {
                store.renameTarget = nil
            }
        }
    }
}
