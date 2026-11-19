import SwiftUI

struct AdaptiveWorkspaceShell: View {
    @ObservedObject var store: OrbitIosStore
    @Binding var appearanceRaw: String

    @Environment(\.orbitPalette) private var palette

    @State private var activeSheetDetent: PresentationDetent = .large
    @GestureState private var drawerDragTranslation: CGFloat = 0

    private var appearanceMode: OrbitAppearanceMode {
        OrbitAppearanceMode(rawValue: appearanceRaw) ?? .system
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
                    compactDrawerShell
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
        .sheet(item: $store.activeSheet) { destination in
            sheetContent(for: destination)
                .environment(\.orbitPalette, palette)
                .preferredColorScheme(appearanceMode.preferredColorScheme)
                .presentationDetents([.medium, .large], selection: $activeSheetDetent)
                .presentationDragIndicator(.visible)
                .onAppear {
                    activeSheetDetent = destination.defaultDetent
                }
        }
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
                GitDiffInspectorView(store: store)
                    .frame(width: OrbitSpacing.inspectorWidth)
                    .transition(.move(edge: .trailing).combined(with: .opacity))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - iPhone Full-Screen Stage + Interactive Left-Edge Drawer

    private var compactDrawerProgress: CGFloat {
        let width = OrbitSpacing.drawerWidth
        let base: CGFloat = store.isDrawerOpen ? width : 0
        let current = min(width, max(0, base + drawerDragTranslation))
        return current / width
    }

    private var compactDrawerOffset: CGFloat {
        let width = OrbitSpacing.drawerWidth
        return -width * (1.0 - compactDrawerProgress)
    }

    private var compactDrawerShell: some View {
        ZStack(alignment: .leading) {
            WorkspaceStagePane(
                store: store,
                showSidebarButton: true,
                canShowInlineInspector: false,
                onToggleSidebar: {
                    store.hapticLight()
                    withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                        store.isDrawerOpen.toggle()
                    }
                }
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.canvas)

            // Interactive left-edge swipe strip when drawer is closed
            if !store.isDrawerOpen {
                Color.clear
                    .frame(width: 20)
                    .contentShape(Rectangle())
                    .gesture(edgeOpenGesture)
            }

            // Dimmed Scrim + Fleet Drawer Overlay
            if compactDrawerProgress > 0.001 {
                Color.black
                    .opacity(0.36 * Double(compactDrawerProgress))
                    .ignoresSafeArea()
                    .onTapGesture {
                        withAnimation(.spring(response: 0.26, dampingFraction: 0.88)) {
                            store.isDrawerOpen = false
                        }
                    }
                    .gesture(drawerCloseGesture)

                compactDrawerPanel
                    .offset(x: compactDrawerOffset)
                    .gesture(drawerCloseGesture)
            }
        }
    }

    @ViewBuilder
    private var compactDrawerPanel: some View {
        if #available(iOS 26.0, *) {
            FleetDrawerView(store: store, isCompactOverlay: true)
                .frame(width: OrbitSpacing.drawerWidth)
                .clipShape(RoundedRectangle(cornerRadius: 28, style: .continuous))
                .glassEffect(.regular, in: RoundedRectangle(cornerRadius: 28, style: .continuous))
                .padding(.leading, OrbitSpacing.sm)
                .padding(.vertical, OrbitSpacing.xs)
        } else {
            HStack(spacing: 0) {
                FleetDrawerView(store: store, isCompactOverlay: true)
                    .frame(width: OrbitSpacing.drawerWidth)
                OrbitHairline(axis: .vertical)
            }
        }
    }

    private var edgeOpenGesture: some Gesture {
        DragGesture(minimumDistance: 12, coordinateSpace: .global)
            .updating($drawerDragTranslation) { value, state, _ in
                if value.translation.width > 0 {
                    state = value.translation.width
                }
            }
            .onEnded { value in
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
        DragGesture(minimumDistance: 10, coordinateSpace: .global)
            .updating($drawerDragTranslation) { value, state, _ in
                if value.translation.width < 0 {
                    state = value.translation.width
                }
            }
            .onEnded { value in
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
        case .modelPicker:
            ModelPickerSheet(store: store)
        case .contextPicker:
            ContextPickerSheet(store: store)
        case .qrPair:
            QrPairSheet(store: store)
        case .directoryBrowser:
            DirectoryBrowserSheet(store: store)
        case .settings:
            SettingsAndQuotasSheet(store: store, appearanceRaw: $appearanceRaw)
        }
    }

    // MARK: - Subtle Toast & Error Banner

    @ViewBuilder
    private var toastAndErrorOverlay: some View {
        VStack(spacing: OrbitSpacing.sm) {
            if let message = store.toastMessage {
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

// MARK: - Workspace Stage Pane (iOS 26+ Native Floating Liquid Glass NavigationStack + Pre-26 Fallback)

private struct WorkspaceStagePane: View {
    @ObservedObject var store: OrbitIosStore
    let showSidebarButton: Bool
    let canShowInlineInspector: Bool
    let onToggleSidebar: () -> Void

    @Environment(\.orbitPalette) private var palette

    private var sessionTitle: String {
        if let agent = store.activeAgent {
            let trimmed = agent.title.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? "New Session" : trimmed
        }
        return "New Session"
    }

    private var connectionState: FfiConnectionState {
        store.activeHost?.connectionState ?? .disconnected
    }

    private var hasTerminals: Bool {
        !(store.currentHostDetail?.terminals.isEmpty ?? true)
    }

    var body: some View {
        if #available(iOS 26.0, *) {
            NavigationStack {
                StreamStageView(store: store)
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

                        ToolbarItem(placement: .title) {
                            Button {
                                if store.activeHost == nil {
                                    store.activeSheet = .qrPair
                                } else {
                                    store.openDirectoryBrowser()
                                }
                            } label: {
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(sessionTitle)
                                        .font(OrbitTypography.subhead(.semibold))
                                        .foregroundStyle(palette.fgPrimary)
                                        .lineLimit(1)

                                    HStack(spacing: OrbitSpacing.xs) {
                                        Circle()
                                            .fill(palette.connectionColor(connectionState))
                                            .frame(width: 6, height: 6)

                                        if let host = store.activeHost {
                                            Text(OrbitFormatters.compactTopBarConnectionLabel(for: host))
                                                .font(OrbitTypography.meta(.regular))
                                                .foregroundStyle(palette.fgSecondary)
                                                .lineLimit(1)
                                                .fixedSize(horizontal: true, vertical: false)

                                            Text("·")
                                                .font(OrbitTypography.meta(.regular))
                                                .foregroundStyle(palette.fgMuted)

                                            Text(OrbitFormatters.shortPath(store.activeCwd))
                                                .font(OrbitTypography.code(.regular))
                                                .foregroundStyle(palette.fgSecondary)
                                                .lineLimit(1)
                                        } else {
                                            Text("No Host Paired")
                                                .font(OrbitTypography.meta(.regular))
                                                .foregroundStyle(palette.fgMuted)
                                        }
                                    }
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("Workspace Directory: \(OrbitFormatters.shortPath(store.activeCwd))")
                        }

                        if store.hosts.isEmpty {
                            ToolbarItem(placement: .topBarTrailing) {
                                Button {
                                    store.hapticLight()
                                    store.activeSheet = .settings
                                } label: {
                                    Image(systemName: "slider.horizontal.3")
                                        .foregroundStyle(palette.fgPrimary)
                                }
                                .accessibilityLabel("Settings")
                            }
                        } else {
                            ToolbarItemGroup(placement: .topBarTrailing) {
                                glassDiffToolbarButton

                                Button {
                                    store.hapticLight()
                                    if store.selectedHostId == nil {
                                        store.activeSheet = .qrPair
                                    } else {
                                        store.activeSheet = .pty
                                    }
                                } label: {
                                    Image(systemName: "terminal")
                                        .foregroundStyle(palette.fgPrimary)
                                        .overlay(alignment: .topTrailing) {
                                            if hasTerminals {
                                                Circle()
                                                    .fill(palette.direct)
                                                    .frame(width: 6, height: 6)
                                                    .offset(x: 3, y: -2)
                                            }
                                        }
                                }
                                .accessibilityLabel("Open Terminal")

                                Button {
                                    store.startNewSessionDraft()
                                } label: {
                                    Image(systemName: "plus")
                                        .foregroundStyle(palette.fgPrimary)
                                }
                                .accessibilityLabel("New Session")
                            }
                        }
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

    @ViewBuilder
    private var glassDiffToolbarButton: some View {
        let totals = store.diffTotals
        Button {
            store.hapticLight()
            guard store.selectedHostId != nil else {
                store.activeSheet = .qrPair
                return
            }
            if canShowInlineInspector {
                store.refreshGitReview()
                withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                    store.isPadDiffInspectorVisible.toggle()
                }
            } else {
                store.openGitDiff()
            }
        } label: {
            if totals.additions > 0 || totals.deletions > 0 {
                HStack(spacing: OrbitSpacing.xs) {
                    Text("+\(totals.additions)")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.added)
                    Text("-\(totals.deletions)")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.removed)
                }
                .fixedSize(horizontal: true, vertical: false)
            } else {
                Image(systemName: "arrow.triangle.branch")
                    .foregroundStyle(
                        canShowInlineInspector && store.isPadDiffInspectorVisible
                            ? palette.primary
                            : palette.fgPrimary
                    )
            }
        }
        .contextMenu {
            Button {
                store.refreshGitReview()
                store.activeSheet = .gitDiff
            } label: {
                Label("Open Full Diff Sheet", systemImage: "arrow.up.left.and.arrow.down.right")
            }
        }
        .accessibilityLabel("Git Diff")
    }
}

// MARK: - Workspace Top Bar (44pt Height, 16pt Horizontal Anchor)

struct WorkspaceTopBar: View {
    @ObservedObject var store: OrbitIosStore
    let showSidebarButton: Bool
    let canShowInlineInspector: Bool
    let onToggleSidebar: () -> Void

    @Environment(\.orbitPalette) private var palette

    private var sessionTitle: String {
        if let agent = store.activeAgent {
            let trimmed = agent.title.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? "New Session" : trimmed
        }
        return "New Session"
    }

    private var connectionState: FfiConnectionState {
        store.activeHost?.connectionState ?? .disconnected
    }

    private var hasTerminals: Bool {
        !(store.currentHostDetail?.terminals.isEmpty ?? true)
    }

    var body: some View {
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

            // Session Title + Connection Dot
            Button {
                if store.activeHost == nil {
                    store.activeSheet = .qrPair
                } else {
                    store.openDirectoryBrowser()
                }
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Text(sessionTitle)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    HStack(spacing: OrbitSpacing.xs) {
                        Circle()
                            .fill(palette.connectionColor(connectionState))
                            .frame(width: 6, height: 6)

                        if let host = store.activeHost {
                            Text(OrbitFormatters.compactTopBarConnectionLabel(for: host))
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgSecondary)
                                .lineLimit(1)
                                .fixedSize(horizontal: true, vertical: false)

                            Text("·")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)

                            Text(OrbitFormatters.shortPath(store.activeCwd))
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgSecondary)
                                .lineLimit(1)
                        } else {
                            Text("No Host Paired")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }
                    }
                }
                .frame(minHeight: OrbitSpacing.hitTarget, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Workspace Directory: \(OrbitFormatters.shortPath(store.activeCwd))")

            Spacer(minLength: OrbitSpacing.xs)

            if store.hosts.isEmpty {
                Button {
                    store.hapticLight()
                    store.activeSheet = .settings
                } label: {
                    Image(systemName: "slider.horizontal.3")
                        .font(OrbitTypography.icon)
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Settings")
                .padding(.trailing, -OrbitSpacing.sm)
            } else {
                // Contextual Right Actions: Diff Pill, Native PTY, New Session
                diffActionButton

                Button {
                    store.hapticLight()
                    if store.selectedHostId == nil {
                        store.activeSheet = .qrPair
                    } else {
                        store.activeSheet = .pty
                    }
                } label: {
                    ZStack(alignment: .topTrailing) {
                        Image(systemName: "terminal")
                            .font(OrbitTypography.icon)
                            .foregroundStyle(palette.fgSecondary)
                            .padding(OrbitSpacing.xs)

                        if hasTerminals {
                            Circle()
                                .fill(palette.direct)
                                .frame(width: 6, height: 6)
                                .offset(x: 1, y: 1)
                        }
                    }
                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Open Terminal")

                Button {
                    store.startNewSessionDraft()
                } label: {
                    Image(systemName: "plus")
                        .font(OrbitTypography.icon)
                        .foregroundStyle(palette.fgPrimary)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("New Session")
                .padding(.trailing, -OrbitSpacing.sm)
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .frame(height: OrbitSpacing.hitTarget)
        .background(palette.canvas)
    }

    @ViewBuilder
    private var diffActionButton: some View {
        let totals = store.diffTotals
        Button {
            store.hapticLight()
            guard store.selectedHostId != nil else {
                store.activeSheet = .qrPair
                return
            }
            if canShowInlineInspector {
                store.refreshGitReview()
                withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                    store.isPadDiffInspectorVisible.toggle()
                }
            } else {
                store.openGitDiff()
            }
        } label: {
            Group {
                if totals.additions > 0 || totals.deletions > 0 {
                    HStack(spacing: OrbitSpacing.xs) {
                        Text("+\(totals.additions)")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.added)
                        Text("-\(totals.deletions)")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.removed)
                    }
                    .padding(.horizontal, OrbitSpacing.sm)
                    .padding(.vertical, OrbitSpacing.xs)
                    .background(palette.subtleFill)
                    .clipShape(Capsule())
                    .overlay(Capsule().strokeBorder(palette.hairline, lineWidth: 0.5))
                } else {
                    Image(systemName: "arrow.triangle.branch")
                        .font(OrbitTypography.icon)
                        .foregroundStyle(canShowInlineInspector && store.isPadDiffInspectorVisible ? palette.primary : palette.fgSecondary)
                }
            }
            .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button {
                store.refreshGitReview()
                store.activeSheet = .gitDiff
            } label: {
                Label("Open Full Diff Sheet", systemImage: "arrow.up.left.and.arrow.down.right")
            }
        }
        .accessibilityLabel("Git Diff")
    }
}
