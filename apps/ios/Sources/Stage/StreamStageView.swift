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

    private let bottomAnchorId = "bottom_anchor"

    private var turns: [FfiStageTurn] {
        store.agentStage?.turns ?? []
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

    var body: some View {
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
        .background(palette.canvas.ignoresSafeArea())
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if !store.hosts.isEmpty || store.hasDemoStageOverlay {
                VStack(spacing: 0) {
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
                    LazyVStack(alignment: .leading, spacing: OrbitSpacing.xl) {
                        ForEach(turns, id: \.key) { turn in
                            StageTurnView(store: store, turn: turn)
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
                    .padding(.top, OrbitSpacing.lg)
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
                    withAnimation(.easeOut(duration: 0.2)) {
                        proxy.scrollTo(bottomAnchorId, anchor: .bottom)
                    }
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
                            HStack(spacing: OrbitSpacing.xs + 2) {
                                if store.isActiveAgentRunning {
                                    PulsingDotView(color: palette.primary, size: 6)
                                }
                                Text(store.isActiveAgentRunning ? "↓ Streaming" : "↓ Latest")
                                    .font(OrbitTypography.meta(.semibold))
                                    .foregroundStyle(palette.fgPrimary)
                            }
                            .padding(.horizontal, OrbitSpacing.md)
                            .frame(height: 34)
                            .orbitGlassSurface(
                                in: Capsule(),
                                interactive: true,
                                fallbackFill: palette.elevated,
                                fallbackBorder: palette.hairline,
                                fallbackShadowOpacity: palette.isDark ? 0.24 : 0.10
                            )
                            .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .padding(.bottom, 2)
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

    // MARK: - New Session Editorial Zero-State

    private var newSessionZeroState: some View {
        GeometryReader { geo in
            ScrollView(.vertical, showsIndicators: false) {
                VStack(spacing: OrbitSpacing.lg) {
                    Spacer(minLength: OrbitSpacing.xl)

                    VStack(spacing: OrbitSpacing.sm) {
                        Text("What would you like to build?")
                            .font(OrbitTypography.title)
                            .foregroundStyle(palette.fgPrimary)
                            .multilineTextAlignment(.center)

                        // Active Workspace Directory Pill
                        Button {
                            store.hapticLight()
                            store.openDirectoryBrowser()
                        } label: {
                            HStack(spacing: OrbitSpacing.xs) {
                                Image(systemName: "folder")
                                    .font(OrbitTypography.iconSmall)
                                    .foregroundStyle(palette.fgSecondary)

                                Text(OrbitFormatters.compactPath(store.activeCwd))
                                    .font(OrbitTypography.code(.medium))
                                    .foregroundStyle(palette.fgPrimary)
                                    .lineLimit(1)

                                if let branch = store.activeGitBranch, !branch.isEmpty {
                                    Text("·")
                                        .font(OrbitTypography.meta(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                    Text(branch)
                                        .font(OrbitTypography.code(.regular))
                                        .foregroundStyle(palette.fgSecondary)
                                        .lineLimit(1)
                                }

                                Image(systemName: "chevron.down")
                                    .font(OrbitTypography.iconMicro)
                                    .foregroundStyle(palette.fgMuted)
                            }
                            .padding(.horizontal, OrbitSpacing.md)
                            .frame(minHeight: OrbitSpacing.hitTarget)
                            .orbitGlassSurface(
                                in: Capsule(),
                                interactive: true,
                                fallbackFill: palette.elevated,
                                fallbackBorder: palette.hairline
                            )
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }

                    // 3 Restrained Quick-Start Workflow Chips
                    VStack(spacing: OrbitSpacing.sm) {
                        quickStartChip(
                            icon: "list.bullet.clipboard",
                            label: "/plan",
                            subtitle: "Outline an implementation plan first"
                        ) {
                            store.applyQuickWorkflow(.plan)
                        }

                        quickStartChip(
                            icon: "doc.text.magnifyingglass",
                            label: "Review uncommitted changes",
                            subtitle: "Inspect working tree diffs for issues"
                        ) {
                            store.applyQuickWorkflow(.review, promptText: "Review uncommitted changes")
                        }

                        quickStartChip(
                            icon: "square.stack.3d.up",
                            label: "Explain architecture",
                            subtitle: "Summarize core crates and data flow"
                        ) {
                            store.applyQuickWorkflow(nil, promptText: "Explain the architecture and key modules of this repository.")
                        }
                    }
                    .frame(maxWidth: 380)

                    Spacer(minLength: OrbitSpacing.xl)
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .frame(minWidth: geo.size.width, minHeight: geo.size.height)
            }
            .orbitSoftScrollEdgeEffect()
            .scrollDismissesKeyboard(.interactively)
            .refreshable {
                await store.pullToRefreshStage()
            }
        }
    }

    private func quickStartChip(
        icon: String,
        label: String,
        subtitle: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            HStack(spacing: OrbitSpacing.md) {
                Image(systemName: icon)
                    .font(OrbitTypography.icon)
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: 20)

                VStack(alignment: .leading, spacing: 2) {
                    Text(label)
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgPrimary)
                    Text(subtitle)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                }

                Spacer()

                Image(systemName: "arrow.up.left")
                    .font(OrbitTypography.iconMicro)
                    .foregroundStyle(palette.fgMuted)
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.vertical, OrbitSpacing.sm)
            .frame(minHeight: OrbitSpacing.hitTarget)
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
