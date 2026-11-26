import SwiftUI

struct GitDiffSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    var body: some View {
        GitDiffWorkspacePanel(
            store: store,
            isModalSheet: true,
            onClose: { dismiss() }
        )
        .background(palette.canvas.ignoresSafeArea())
    }
}

struct GitDiffInspectorView: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.orbitPalette) private var palette

    var body: some View {
        GitDiffWorkspacePanel(
            store: store,
            isModalSheet: false,
            onClose: {
                withAnimation(.spring(response: 0.26, dampingFraction: 0.88)) {
                    store.isPadDiffInspectorVisible = false
                }
            }
        )
        .background(palette.drawer)
    }
}

// MARK: - Auto-Collapse Heuristics for Lockfiles & Generated Bindings

private enum GitDiffHeuristics {
    static let lockfileNames: Set<String> = [
        "cargo.lock",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "podfile.lock",
        "bun.lock",
        "bun.lockb",
        "gemfile.lock",
        "composer.lock"
    ]

    static func shouldAutoCollapse(path: String) -> Bool {
        let normalized = path.replacingOccurrences(of: "\\", with: "/")
        let lowerPath = normalized.lowercased()
        let baseName = OrbitFormatters.shortPath(normalized).lowercased()

        if lockfileNames.contains(baseName) {
            return true
        }
        if lowerPath.contains("uniffi") {
            return true
        }
        if lowerPath.hasPrefix("generated/") || lowerPath.contains("/generated/") {
            return true
        }
        if lowerPath.hasSuffix(".pbxproj") {
            return true
        }
        return false
    }
}

// MARK: - Shared Git Diff Workspace Panel (Modal Sheet & iPad 3rd-Column Inspector)

private struct GitDiffWorkspacePanel: View {
    @ObservedObject var store: OrbitIosStore
    @ObservedObject private var stageState: OrbitStageStreamState
    let isModalSheet: Bool
    let onClose: () -> Void

    @Environment(\.orbitPalette) private var palette
    @State private var expandedOverrides: [String: Bool] = [:]
    @State private var isCommitSheetPresented: Bool = false
    @State private var isPrSheetPresented: Bool = false
    @State private var isStashSheetPresented: Bool = false
    @State private var isBranchAlertPresented: Bool = false
    @State private var newBranchName: String = ""
    @State private var discardConfirmFilePath: String? = nil

    init(store: OrbitIosStore, isModalSheet: Bool, onClose: @escaping () -> Void) {
        self.store = store
        self._stageState = ObservedObject(wrappedValue: store.stageState)
        self.isModalSheet = isModalSheet
        self.onClose = onClose
    }

    private var review: FfiGitReview? {
        store.gitReview
    }

    private var effectiveDiffFiles: [FfiDiffFile] {
        store.effectiveGitDiffFiles(for: store.gitReviewScope)
    }

    private var trimmedFilterQuery: String {
        store.diffFileFilterQuery.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var filteredDiffFiles: [FfiDiffFile] {
        let q = trimmedFilterQuery.lowercased()
        guard !q.isEmpty else { return effectiveDiffFiles }
        return effectiveDiffFiles.filter { file in
            file.path.lowercased().contains(q)
                || OrbitFormatters.shortPath(file.path).lowercased().contains(q)
        }
    }

    private var areAnyFilteredFilesExpanded: Bool {
        filteredDiffFiles.contains(where: { isFileExpanded($0.path) })
    }

    private func isFileExpanded(_ path: String) -> Bool {
        if let explicit = expandedOverrides[path] {
            return explicit
        }
        return !GitDiffHeuristics.shouldAutoCollapse(path: path)
    }

    private func toggleAllFilteredFiles() {
        store.hapticLight()
        withAnimation(.easeInOut(duration: 0.18)) {
            if areAnyFilteredFilesExpanded {
                for file in filteredDiffFiles {
                    expandedOverrides[file.path] = false
                }
            } else {
                let hasNonGenerated = filteredDiffFiles.contains {
                    !GitDiffHeuristics.shouldAutoCollapse(path: $0.path)
                }
                for file in filteredDiffFiles {
                    expandedOverrides[file.path] = hasNonGenerated
                        ? !GitDiffHeuristics.shouldAutoCollapse(path: file.path)
                        : true
                }
            }
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            headerBar
            branchAndStagedControls
            gitWriteActionsBar
            if let rebase = store.gitRebaseStatus, rebase.inProgress {
                rebaseProgressBanner(rebase)
            }
            if !store.gitConflicts.isEmpty {
                conflictsBanner
            }
            if let pr = store.gitPrStatus, pr.availability == .available, let num = pr.number {
                openPrStatusBanner(pr, number: num)
            }
            if let err = store.gitWriteError, !err.isEmpty {
                HStack(spacing: 6) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.system(size: 11.5))
                        .foregroundStyle(palette.danger)
                    Text(err)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.danger)
                        .lineLimit(2)
                    Spacer()
                    Button { store.gitWriteError = nil } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(palette.fgMuted)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, 6)
                .background(palette.danger.opacity(0.08))
            }
            OrbitHairline()
            diffScrollBody
        }
        .onAppear {
            store.refreshGitReview()
            store.refreshGitWriteMetadata()
        }
        .sheet(isPresented: $isCommitSheetPresented) {
            GitCommitModalSheet(store: store)
                .environment(\.orbitPalette, palette)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $isPrSheetPresented) {
            GitCreatePrModalSheet(store: store)
                .environment(\.orbitPalette, palette)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $isStashSheetPresented) {
            GitStashManagerSheet(store: store)
                .environment(\.orbitPalette, palette)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        }
        .alert("Create & Checkout Branch", isPresented: $isBranchAlertPresented) {
            TextField("feat/my-branch", text: $newBranchName)
            Button("Create") {
                let name = newBranchName.trimmingCharacters(in: .whitespacesAndNewlines)
                if !name.isEmpty {
                    let safeName = name.replacingOccurrences(of: " ", with: "-")
                    onClose()
                    store.runGitCommandInTerminal("git checkout -b '\(safeName)'")
                }
                newBranchName = ""
            }
            Button(OrbitL10n.tr("Cancel"), role: .cancel) { newBranchName = "" }
        }
        .confirmationDialog(
            OrbitL10n.tr("Discard changes to this file?"),
            isPresented: Binding(
                get: { discardConfirmFilePath != nil },
                set: { if !$0 { discardConfirmFilePath = nil } }
            ),
            titleVisibility: .visible
        ) {
            if let path = discardConfirmFilePath {
                Button(OrbitL10n.tr("Discard Changes"), role: .destructive) {
                    discardConfirmFilePath = nil
                    let escaped = path.replacingOccurrences(of: "'", with: "'\\''")
                    store.runGitCommandInTerminal("git checkout -- '\(escaped)' || rm -f '\(escaped)'")
                }
            }
            Button(OrbitL10n.tr("Cancel"), role: .cancel) {
                discardConfirmFilePath = nil
            }
        } message: {
            if let path = discardConfirmFilePath {
                Text(OrbitFormatters.shortPath(path))
            }
        }
    }

    // MARK: - Header

    private var headerBar: some View {
        HStack(spacing: OrbitSpacing.sm) {
            Text(OrbitL10n.tr("Git Diff"))
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)

            Spacer()

            Button {
                store.hapticLight()
                store.refreshGitReview()
            } label: {
                Image(systemName: "arrow.clockwise")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Refresh Git Diff")

            if isModalSheet {
                Button(action: onClose) {
                    Text(OrbitL10n.tr("Done"))
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                        .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close Git Diff")
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .frame(height: 52)
    }

    // MARK: - Branch + Ahead/Behind + Unstaged/Staged Segmented Toggle

    private var branchAndStagedControls: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                HStack(spacing: OrbitSpacing.xs) {
                    Image(systemName: "arrow.triangle.branch")
                        .font(OrbitTypography.iconMicro)
                        .foregroundStyle(palette.primary)

                    Text(review?.branch ?? store.activeGitBranch ?? "HEAD")
                        .font(OrbitTypography.code(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)
                }
                .padding(.horizontal, OrbitSpacing.sm + 2)
                .padding(.vertical, 4)
                .background(palette.subtleFill)
                .clipShape(Capsule())

                if let ahead = review?.ahead, let behind = review?.behind, (ahead > 0 || behind > 0) {
                    HStack(spacing: OrbitSpacing.xs) {
                        if ahead > 0 {
                            Text("↑\(ahead)")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.added)
                        }
                        if behind > 0 {
                            Text("↓\(behind)")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.warning)
                        }
                    }
                }

                Spacer()

                Text("Tap line to quote")
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .lineLimit(1)
            }

            // Segmented Unstaged / Staged / All Changes Toggle
            HStack(spacing: OrbitSpacing.xs) {
                ForEach(OrbitGitReviewScope.allCases) { scope in
                    stagedSegmentButton(title: scope.label, isSelected: store.gitReviewScope == scope) {
                        store.setGitReviewScope(scope)
                    }
                }
            }
            .padding(3)
            .background(palette.subtleFill)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )

            // Instant File Filter + Collapse All / Expand Code Toggle
            if !effectiveDiffFiles.isEmpty {
                HStack(spacing: OrbitSpacing.sm) {
                    HStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: "magnifyingglass")
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(palette.fgMuted)

                        TextField("Filter modified files…", text: $store.diffFileFilterQuery)
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgPrimary)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled(true)

                        if !store.diffFileFilterQuery.isEmpty {
                            Button {
                                store.hapticLight()
                                store.diffFileFilterQuery = ""
                            } label: {
                                Image(systemName: "xmark.circle.fill")
                                    .font(OrbitTypography.iconSmall)
                                    .foregroundStyle(palette.fgMuted)
                                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("Clear Diff File Filter")
                        }
                    }
                    .padding(.leading, OrbitSpacing.md)
                    .frame(height: 36)
                    .background(palette.subtleFill)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .strokeBorder(palette.hairline, lineWidth: 0.5)
                    )

                    Button {
                        toggleAllFilteredFiles()
                    } label: {
                        HStack(spacing: OrbitSpacing.xs) {
                            Image(
                                systemName: areAnyFilteredFilesExpanded
                                    ? "rectangle.compress.vertical"
                                    : "rectangle.expand.vertical"
                            )
                            .font(OrbitTypography.iconMicro)
                            Text(areAnyFilteredFilesExpanded ? "Collapse All" : "Expand Code")
                                .font(OrbitTypography.meta(.semibold))
                                .lineLimit(1)
                        }
                        .foregroundStyle(palette.fgSecondary)
                        .padding(.horizontal, OrbitSpacing.sm + 2)
                        .frame(height: 36)
                        .background(palette.subtleFill)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 10, style: .continuous)
                                .strokeBorder(palette.hairline, lineWidth: 0.5)
                        )
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .fixedSize(horizontal: true, vertical: false)
                    .accessibilityLabel(areAnyFilteredFilesExpanded ? "Collapse All Diff Files" : "Expand Modified Code Files")
                }
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.bottom, OrbitSpacing.sm)
    }

    private func stagedSegmentButton(
        title: String,
        isSelected: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Text(title)
                .font(OrbitTypography.subhead(isSelected ? .semibold : .medium))
                .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgSecondary)
                .frame(maxWidth: .infinity, minHeight: 38)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(isSelected ? palette.elevated : Color.clear)
                )
                .frame(minHeight: OrbitSpacing.hitTarget)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    // MARK: - Git Write Actions Bar (Stage, Commit, Push, Branch, Stash, Create PR)

    private var gitWriteActionsBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                if store.gitReviewScope == .staged {
                    gitActionChip(OrbitL10n.tr("Unstage All"), icon: "minus.circle") {
                        store.gitUnstage(paths: [], all: true)
                    }
                } else {
                    gitActionChip(OrbitL10n.tr("Stage All"), icon: "plus.circle") {
                        store.gitStage(paths: [], all: true)
                    }
                }

                gitActionChip(OrbitL10n.tr("Commit"), icon: "checkmark.circle", prominent: true) {
                    isCommitSheetPresented = true
                }

                    gitActionChip(OrbitL10n.tr("Push"), icon: "arrow.up.circle") {
                        onClose()
                        store.runGitCommandInTerminal("git push")
                    }

                    Menu {
                        Button(OrbitL10n.tr("New Branch…")) {
                            isBranchAlertPresented = true
                        }
                        Button(OrbitL10n.tr("Checkout main")) {
                            store.runGitCommandInTerminal("git checkout main")
                        }
                        Button(OrbitL10n.tr("Diff vs main")) {
                            onClose()
                            store.runGitCommandInTerminal("git --no-pager diff main...HEAD")
                        }
                    } label: {
                        HStack(spacing: 5) {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.system(size: 11, weight: .semibold))
                            Text(OrbitL10n.tr("Branch"))
                                .font(OrbitTypography.meta(.semibold))
                        }
                        .foregroundStyle(palette.fgPrimary)
                        .padding(.horizontal, 11)
                        .frame(minHeight: 36)
                        .background(palette.subtleFill, in: Capsule())
                        .overlay(Capsule().strokeBorder(palette.hairline, lineWidth: 0.5))
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }

                    gitActionChip(
                        store.gitStashes.isEmpty ? OrbitL10n.tr("Stash") : "\(OrbitL10n.tr("Stash")) (\(store.gitStashes.count))",
                        icon: "tray.and.arrow.down"
                    ) {
                        isStashSheetPresented = true
                    }

                    gitActionChip(OrbitL10n.tr("Create PR"), icon: "arrow.triangle.pull") {
                        isPrSheetPresented = true
                    }
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.bottom, OrbitSpacing.sm)
            }
            .disabled(store.gitWriteBusy)
            .opacity(store.gitWriteBusy ? 0.65 : 1.0)
        }

    private func gitActionChip(_ title: String, icon: String, prominent: Bool = false, action: @escaping () -> Void) -> some View {
        Button {
            store.hapticLight()
            action()
        } label: {
            HStack(spacing: 5) {
                Image(systemName: icon)
                    .font(.system(size: 11, weight: .semibold))
                Text(title)
                    .font(OrbitTypography.meta(.semibold))
            }
            .foregroundStyle(prominent ? palette.onPrimary : palette.fgPrimary)
            .padding(.horizontal, 11)
            .frame(minHeight: 36)
            .background(prominent ? palette.primary : palette.subtleFill, in: Capsule())
            .overlay(
                Capsule().strokeBorder(prominent ? Color.clear : palette.hairline, lineWidth: 0.5)
            )
            .frame(minHeight: OrbitSpacing.hitTarget)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func openPrStatusBanner(_ pr: GitPrStatus, number: UInt64) -> some View {
        HStack(spacing: OrbitSpacing.sm) {
            Image(systemName: "arrow.triangle.pull")
                .font(.system(size: 12.5, weight: .semibold))
                .foregroundStyle(palette.direct)

            VStack(alignment: .leading, spacing: 1) {
                Text("PR #\(number) · \(pr.title ?? "")")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)
                HStack(spacing: 6) {
                    if pr.passingChecks > 0 {
                        Text("✓\(pr.passingChecks)")
                            .foregroundStyle(palette.direct)
                    }
                    if pr.failingChecks > 0 {
                        Text("✕\(pr.failingChecks)")
                            .foregroundStyle(palette.danger)
                    }
                    if pr.pendingChecks > 0 {
                        Text("•\(pr.pendingChecks)")
                            .foregroundStyle(palette.warning)
                    }
                    if let decision = pr.reviewDecision, !decision.isEmpty {
                        Text(decision)
                            .foregroundStyle(palette.fgMuted)
                    }
                }
                .font(.system(size: 11, weight: .medium))
            }

            Spacer(minLength: 4)

            if let urlStr = pr.url, let url = URL(string: urlStr) {
                Button("Open") {
                    UIApplication.shared.open(url)
                }
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.accentSky)
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.vertical, 8)
        .background(palette.recessed)
    }

    private func rebaseProgressBanner(_ rebase: GitRebaseStatus) -> some View {
        HStack(spacing: OrbitSpacing.sm) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 12))
                .foregroundStyle(palette.warning)
            Text(OrbitL10n.tr("Rebase in Progress"))
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgPrimary)
            Spacer()
            Button(OrbitL10n.tr("Continue")) { store.performGitRebaseAction("continue") }
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.direct)
            Button(OrbitL10n.tr("Skip")) { store.performGitRebaseAction("skip") }
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgSecondary)
            Button(OrbitL10n.tr("Abort")) { store.performGitRebaseAction("abort") }
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.danger)
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.vertical, 8)
        .background(palette.warning.opacity(0.10))
    }

    private var conflictsBanner: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("\(OrbitL10n.tr("Merge Conflicts")) (\(store.gitConflicts.count))")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.danger)
            ForEach(store.gitConflicts.prefix(4), id: \.path) { file in
                HStack(spacing: 6) {
                    Text(OrbitFormatters.shortPath(file.path))
                        .font(OrbitTypography.code(.medium))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)
                    Spacer()
                    Button(OrbitL10n.tr("Keep Ours")) { store.resolveGitConflict(file: file, resolution: .ours) }
                        .font(.system(size: 11, weight: .semibold)).foregroundStyle(palette.accentSky)
                    Button(OrbitL10n.tr("Keep Theirs")) { store.resolveGitConflict(file: file, resolution: .theirs) }
                        .font(.system(size: 11, weight: .semibold)).foregroundStyle(palette.warning)
                    Button(OrbitL10n.tr("Keep Both")) { store.resolveGitConflict(file: file, resolution: .both) }
                        .font(.system(size: 11, weight: .medium)).foregroundStyle(palette.fgSecondary)
                }
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.vertical, 8)
        .background(palette.danger.opacity(0.08))
    }

    // MARK: - Horizontal File Jump Pills & Virtualized Diff Stream

    private var diffScrollBody: some View {
        ScrollViewReader { proxy in
            VStack(spacing: 0) {
                if !filteredDiffFiles.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: OrbitSpacing.xs) {
                            ForEach(filteredDiffFiles, id: \.path) { file in
                                Button {
                                    store.hapticLight()
                                    expandedOverrides[file.path] = true
                                    withAnimation(.easeInOut(duration: 0.22)) {
                                        proxy.scrollTo("diff-file-\(file.path)", anchor: .top)
                                    }
                                } label: {
                                    HStack(spacing: OrbitSpacing.xs) {
                                        Text(OrbitFormatters.shortPath(file.path))
                                            .font(OrbitTypography.code(.medium))
                                            .foregroundStyle(palette.fgPrimary)
                                            .lineLimit(1)

                                        if file.additions > 0 {
                                            Text("+\(file.additions)")
                                                .font(OrbitTypography.meta(.medium))
                                                .foregroundStyle(palette.added)
                                        }
                                        if file.deletions > 0 {
                                            Text("-\(file.deletions)")
                                                .font(OrbitTypography.meta(.medium))
                                                .foregroundStyle(palette.removed)
                                        }
                                    }
                                    .padding(.horizontal, OrbitSpacing.sm + 2)
                                    .padding(.vertical, 6)
                                    .background(palette.elevated)
                                    .clipShape(Capsule())
                                    .overlay(Capsule().strokeBorder(palette.hairline, lineWidth: 0.5))
                                    .frame(minHeight: OrbitSpacing.hitTarget)
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .padding(.horizontal, OrbitSpacing.lg)
                        .padding(.vertical, OrbitSpacing.xs)
                    }
                    OrbitHairline()
                }

                if effectiveDiffFiles.isEmpty {
                    cleanTreeZeroState
                } else if filteredDiffFiles.isEmpty {
                    filteredZeroState
                } else {
                    ScrollView(.vertical, showsIndicators: true) {
                        LazyVStack(alignment: .leading, spacing: OrbitSpacing.md) {
                            ForEach(filteredDiffFiles, id: \.path) { file in
                                DiffFileSectionCard(
                                    store: store,
                                    file: file,
                                    isExpanded: isFileExpanded(file.path),
                                    isAutoCollapsedKind: GitDiffHeuristics.shouldAutoCollapse(path: file.path),
                                    onToggleExpand: {
                                        store.hapticLight()
                                        withAnimation(.easeInOut(duration: 0.18)) {
                                            expandedOverrides[file.path] = !isFileExpanded(file.path)
                                        }
                                    },
                                    onJumpToHunk: { hunkOffset in
                                        store.hapticLight()
                                        expandedOverrides[file.path] = true
                                        withAnimation(.easeInOut(duration: 0.2)) {
                                            proxy.scrollTo("diff-hunk-\(file.path)-\(hunkOffset)", anchor: .top)
                                        }
                                    },
                                    onRequestDiscard: store.gitReviewScope == .workingTree ? {
                                        discardConfirmFilePath = file.path
                                    } : nil
                                )
                                .id("diff-file-\(file.path)")
                            }
                        }
                        .padding(OrbitSpacing.lg)
                    }
                    .refreshable {
                        await store.pullToRefreshGitDiff()
                    }
                    .onAppear {
                        jumpToRequestedDiffFileIfNeeded(proxy: proxy)
                    }
                    .onChange(of: store.jumpToDiffFilePath) { _ in
                        jumpToRequestedDiffFileIfNeeded(proxy: proxy)
                    }
                }
            }
        }
    }

    private func jumpToRequestedDiffFileIfNeeded(proxy: ScrollViewProxy) {
        guard let jump = store.jumpToDiffFilePath else { return }
        expandedOverrides[jump] = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.12) {
            withAnimation(.easeInOut(duration: 0.2)) {
                proxy.scrollTo("diff-file-\(jump)", anchor: .top)
            }
            store.jumpToDiffFilePath = nil
        }
    }

    private var filteredZeroState: some View {
        VStack(spacing: OrbitSpacing.sm) {
            Spacer()
            Image(systemName: "magnifyingglass")
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgMuted)
            Text("No matching files")
                .font(OrbitTypography.body(.medium))
                .foregroundStyle(palette.fgPrimary)
            Text("No modified files match \"\(trimmedFilterQuery)\".")
                .orbitSubheadStyle()
                .foregroundStyle(palette.fgMuted)
                .multilineTextAlignment(.center)
                .padding(.horizontal, OrbitSpacing.xl)

            Button {
                store.hapticLight()
                store.diffFileFilterQuery = ""
            } label: {
                Text("Clear Filter")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.primary)
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var cleanTreeZeroState: some View {
        GeometryReader { geo in
            ScrollView(.vertical, showsIndicators: false) {
                VStack(spacing: OrbitSpacing.sm) {
                    Spacer(minLength: OrbitSpacing.xl)
                    Image(systemName: "checkmark.circle")
                        .font(OrbitTypography.title)
                        .foregroundStyle(palette.direct)
                    Text("Working tree clean")
                        .font(OrbitTypography.body(.medium))
                        .foregroundStyle(palette.fgPrimary)
                    Text(cleanTreeSubtitle)
                        .orbitSubheadStyle()
                        .foregroundStyle(palette.fgMuted)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, OrbitSpacing.xl)
                    Spacer(minLength: OrbitSpacing.xl)
                }
                .frame(minWidth: geo.size.width, minHeight: geo.size.height)
            }
            .refreshable {
                await store.pullToRefreshGitDiff()
            }
        }
    }

    private var cleanTreeSubtitle: String {
        let dir = OrbitFormatters.compactPath(store.activeCwd)
        switch store.gitReviewScope {
        case .staged: return "No staged changes in \(dir)."
        case .workingTree: return "No unstaged modifications in \(dir)."
        case .all: return "No pending changes in \(dir)."
        }
    }
}

// MARK: - Virtualized & Collapsible File Diff Card (120-Line Cap)

private struct DiffFileSectionCard: View {
    @ObservedObject var store: OrbitIosStore
    let file: FfiDiffFile
    let isExpanded: Bool
    let isAutoCollapsedKind: Bool
    let onToggleExpand: () -> Void
    let onJumpToHunk: (Int) -> Void
    var onRequestDiscard: (() -> Void)? = nil

    @Environment(\.orbitPalette) private var palette
    @State private var showAllLines = false
    @State private var currentHunkCursor = -1

    private let initialLineCap = 120

    private var hunkOffsets: [Int] {
        file.lines.enumerated().compactMap { idx, line in
            line.kind == .hunkHeader ? idx : nil
        }
    }

    private var displayedLines: ArraySlice<FfiDiffLine> {
        if showAllLines || file.lines.count <= initialLineCap {
            return file.lines[...]
        }
        return file.lines.prefix(initialLineCap)
    }

    private var hasCappedRemainingLines: Bool {
        !showAllLines && file.lines.count > initialLineCap
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Collapsible File Header (>= 44pt hit target) + Multi-Hunk Stepper
            HStack(spacing: 0) {
                Button(action: onToggleExpand) {
                    HStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: "chevron.right")
                            .font(OrbitTypography.iconMicro)
                            .foregroundStyle(palette.fgMuted)
                            .rotationEffect(.degrees(isExpanded ? 90 : 0))

                        Text(file.path)
                            .font(OrbitTypography.code(.semibold))
                            .foregroundStyle(palette.fgPrimary)
                            .lineLimit(1)
                            .truncationMode(.middle)

                        Spacer(minLength: OrbitSpacing.xs)

                        if !isExpanded && isAutoCollapsedKind {
                            Text("Generated")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .padding(.horizontal, OrbitSpacing.xs + 2)
                                .padding(.vertical, 1)
                                .background(palette.subtleFill)
                                .clipShape(Capsule())
                        }

                        HStack(spacing: OrbitSpacing.xs) {
                            if file.additions > 0 {
                                Text("+\(file.additions)")
                                    .font(OrbitTypography.meta(.medium))
                                    .foregroundStyle(palette.added)
                            }
                            if file.deletions > 0 {
                                Text("-\(file.deletions)")
                                    .font(OrbitTypography.meta(.medium))
                                    .foregroundStyle(palette.removed)
                            }
                        }
                    }
                    .padding(.leading, OrbitSpacing.md)
                    .padding(.trailing, OrbitSpacing.xs)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)

                // Jump to File Preview
                Button {
                    store.hapticLight()
                    store.openWorkspaceFiles(initialPath: file.path)
                } label: {
                    Image(systemName: "doc.text")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Open \(OrbitFormatters.shortPath(file.path)) in Files")

                // Per-file Stage / Unstage button
                Button {
                    if store.gitReviewScope == .staged {
                        store.gitUnstage(paths: [file.path], all: false)
                    } else {
                        store.gitStage(paths: [file.path], all: false)
                    }
                } label: {
                    Image(systemName: store.gitReviewScope == .staged ? "minus.circle" : "plus.circle")
                        .font(.system(size: 13.5, weight: .medium))
                        .foregroundStyle(store.gitReviewScope == .staged ? palette.warning : palette.direct)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(store.gitReviewScope == .staged ? OrbitL10n.tr("Unstage") : OrbitL10n.tr("Stage"))

                if let onRequestDiscard, store.gitReviewScope == .workingTree {
                    Button(action: onRequestDiscard) {
                        Image(systemName: "arrow.uturn.backward")
                            .font(.system(size: 12.5, weight: .medium))
                            .foregroundStyle(palette.danger)
                            .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Discard changes to \(OrbitFormatters.shortPath(file.path))")
                }

                if hunkOffsets.count > 1 {
                    Button {
                        let nextCursor = (currentHunkCursor < 0 || currentHunkCursor + 1 >= hunkOffsets.count)
                            ? 0
                            : (currentHunkCursor + 1)
                        currentHunkCursor = nextCursor
                        let targetOffset = hunkOffsets[nextCursor]
                        if targetOffset >= initialLineCap {
                            showAllLines = true
                        }
                        onJumpToHunk(targetOffset)
                    } label: {
                        HStack(spacing: 2) {
                            Image(systemName: "arrow.down")
                                .font(OrbitTypography.iconMicro)
                            Text("\(hunkOffsets.count)")
                                .font(OrbitTypography.meta(.medium))
                        }
                        .foregroundStyle(palette.fgSecondary)
                        .padding(.horizontal, OrbitSpacing.sm)
                        .padding(.vertical, 3)
                        .background(palette.subtleFill)
                        .clipShape(Capsule())
                        .padding(.trailing, OrbitSpacing.sm)
                        .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Jump Next Hunk (\(hunkOffsets.count) hunks)")
                }
            }
            .background(palette.elevated)

            if isExpanded {
                OrbitHairline()

                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(displayedLines.enumerated()), id: \.offset) { offset, line in
                        diffLineRow(line)
                            .id("diff-hunk-\(file.path)-\(offset)")
                    }

                    if hasCappedRemainingLines {
                        OrbitHairline()
                        Button {
                            store.hapticLight()
                            showAllLines = true
                        } label: {
                            HStack(spacing: OrbitSpacing.xs) {
                                Image(systemName: "arrow.down.to.line")
                                    .font(OrbitTypography.iconMicro)
                                Text("Show all \(file.lines.count) lines")
                                    .font(OrbitTypography.subhead(.medium))
                            }
                            .foregroundStyle(palette.primary)
                            .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                            .background(palette.elevated)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .background(palette.code)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
    }

    @ViewBuilder
    private func diffLineRow(_ line: FfiDiffLine) -> some View {
        let canQuote = (line.kind == .added || line.kind == .removed || line.kind == .context)

        Button {
            guard canQuote else { return }
            store.quoteDiffLine(path: file.path, line: line)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: OrbitSpacing.xs + 2) {
                Text(line.oldLine.map(String.init) ?? "")
                    .font(OrbitTypography.code(.regular).monospacedDigit())
                    .foregroundStyle(palette.fgMuted)
                    .lineLimit(1)
                    .frame(minWidth: 38, alignment: .trailing)
                    .fixedSize(horizontal: true, vertical: false)

                Text(line.newLine.map(String.init) ?? "")
                    .font(OrbitTypography.code(.regular).monospacedDigit())
                    .foregroundStyle(palette.fgMuted)
                    .lineLimit(1)
                    .frame(minWidth: 38, alignment: .trailing)
                    .fixedSize(horizontal: true, vertical: false)

                Text(linePrefix(line.kind))
                    .font(OrbitTypography.code(.semibold))
                    .foregroundStyle(lineColor(line.kind))
                    .frame(width: 10, alignment: .center)

                Text(line.content)
                    .font(OrbitTypography.code(.regular))
                    .foregroundStyle(lineColor(line.kind))
                    .multilineTextAlignment(.leading)
                    .frame(maxWidth: .infinity, alignment: .leading)

                if canQuote {
                    Image(systemName: "text.quote")
                        .font(OrbitTypography.iconMicro)
                        .foregroundStyle(palette.fgMuted.opacity(0.55))
                }
            }
            .padding(.horizontal, OrbitSpacing.sm)
            .padding(.vertical, 3)
            .background(lineBackground(line.kind))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!canQuote)
    }

    private func linePrefix(_ kind: FfiDiffLineKind) -> String {
        switch kind {
        case .added: return "+"
        case .removed: return "-"
        case .hunkHeader, .meta: return ""
        case .context: return " "
        }
    }

    private func lineColor(_ kind: FfiDiffLineKind) -> Color {
        switch kind {
        case .added: return palette.added
        case .removed: return palette.removed
        case .hunkHeader: return palette.primary
        case .meta: return palette.fgMuted
        case .context: return palette.fgPrimary
        }
    }

    private func lineBackground(_ kind: FfiDiffLineKind) -> Color {
        switch kind {
        case .added: return palette.added.opacity(0.12)
        case .removed: return palette.removed.opacity(0.12)
        case .hunkHeader: return palette.primary.opacity(0.08)
        case .meta, .context: return Color.clear
        }
    }
}

// MARK: - Git Commit, Stash & Create PR Modals

private struct GitCommitModalSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var message: String = ""
    @State private var amend: Bool = false
    @State private var isGenerating: Bool = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Commit Message") {
                    TextEditor(text: $message)
                        .font(OrbitTypography.body(.regular))
                        .frame(minHeight: 110)

                    Button {
                        isGenerating = true
                        Task {
                            if let generated = await store.generateGitCommitMessage() {
                                message = generated
                            }
                            isGenerating = false
                        }
                    } label: {
                        HStack(spacing: 6) {
                            if isGenerating {
                                ProgressView().controlSize(.small)
                            } else {
                                Image(systemName: "sparkles")
                            }
                            Text(OrbitL10n.tr("Generate Message"))
                        }
                    }
                    .disabled(isGenerating || store.gitWriteBusy)

                    Toggle(OrbitL10n.tr("Amend previous commit"), isOn: $amend)
                }
            }
            .navigationTitle(OrbitL10n.tr("Commit Changes"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button(OrbitL10n.tr("Cancel")) { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Commit")) {
                        store.commitGitChanges(message: message, amend: amend) {
                            dismiss()
                        }
                    }
                    .disabled(message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || store.gitWriteBusy)
                }
            }
        }
    }
}

private struct GitCreatePrModalSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss

    @State private var title: String = ""
    @State private var bodyText: String = ""
    @State private var baseBranch: String = "main"
    @State private var isDraft: Bool = false

    var body: some View {
        NavigationStack {
            Form {
                Section(OrbitL10n.tr("Pull Request")) {
                    TextField("PR title", text: $title)
                    TextField("Base branch (e.g. main)", text: $baseBranch)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    Toggle(OrbitL10n.tr("Draft PR"), isOn: $isDraft)
                }
                Section("Description") {
                    TextEditor(text: $bodyText)
                        .frame(minHeight: 120)
                }
            }
            .navigationTitle(OrbitL10n.tr("Create PR"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button(OrbitL10n.tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Create PR")) {
                        store.createGitPullRequest(
                            title: title,
                            body: bodyText,
                            base: baseBranch.isEmpty ? nil : baseBranch,
                            draft: isDraft
                        ) { _ in
                            dismiss()
                        }
                    }
                    .disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || store.gitWriteBusy)
                }
            }
        }
    }
}

private struct GitStashManagerSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var stashMessage: String = ""
    @State private var includeUntracked: Bool = true
    @State private var dropConfirmEntry: GitStashEntry? = nil

    var body: some View {
        NavigationStack {
            List {
                if let err = store.gitWriteError, !err.isEmpty {
                    Section {
                        HStack(spacing: 8) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .foregroundStyle(palette.danger)
                            Text(err)
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.danger)
                            Spacer()
                            Button { store.gitWriteError = nil } label: {
                                Image(systemName: "xmark")
                                    .foregroundStyle(palette.fgMuted)
                                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }

                Section(OrbitL10n.tr("Save Stash")) {
                    TextField("Optional stash message", text: $stashMessage)
                    Toggle("Include untracked files", isOn: $includeUntracked)
                    Button(OrbitL10n.tr("Save Stash")) {
                        store.saveGitStash(message: stashMessage, includeUntracked: includeUntracked)
                        stashMessage = ""
                    }
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .disabled(store.gitWriteBusy)
                }

                Section("Saved Stashes (\(store.gitStashes.count))") {
                    if store.gitStashes.isEmpty {
                        Text("No stashes in this repository.")
                            .foregroundStyle(palette.fgMuted)
                    } else {
                        ForEach(store.gitStashes, id: \.stashRef) { entry in
                            VStack(alignment: .leading, spacing: 6) {
                                Text(entry.message.isEmpty ? entry.stashRef : entry.message)
                                    .font(OrbitTypography.subhead(.medium))
                                Text("\(entry.stashRef) · \(entry.branch ?? "")")
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                HStack(spacing: 10) {
                                    Button(OrbitL10n.tr("Pop")) { store.applyGitStash(entry, pop: true) }
                                        .font(OrbitTypography.meta(.semibold))
                                        .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                                        .contentShape(Rectangle())
                                    Button(OrbitL10n.tr("Apply")) { store.applyGitStash(entry, pop: false) }
                                        .font(OrbitTypography.meta(.medium))
                                        .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                                        .contentShape(Rectangle())
                                    Spacer()
                                    Button(OrbitL10n.tr("Drop"), role: .destructive) { dropConfirmEntry = entry }
                                        .font(OrbitTypography.meta(.medium))
                                        .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                                        .contentShape(Rectangle())
                                }
                                .buttonStyle(.borderless)
                            }
                            .padding(.vertical, 4)
                        }
                    }
                }
            }
            .navigationTitle(OrbitL10n.tr("Stash"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { Button(OrbitL10n.tr("Done")) { dismiss() } }
            }
            .onAppear {
                store.refreshGitWriteMetadata()
            }
            .confirmationDialog(
                OrbitL10n.tr("Drop this stash permanently?"),
                isPresented: Binding(
                    get: { dropConfirmEntry != nil },
                    set: { if !$0 { dropConfirmEntry = nil } }
                ),
                titleVisibility: .visible
            ) {
                if let entry = dropConfirmEntry {
                    Button(OrbitL10n.tr("Drop Stash"), role: .destructive) {
                        dropConfirmEntry = nil
                        store.dropGitStash(entry)
                    }
                }
                Button(OrbitL10n.tr("Cancel"), role: .cancel) {
                    dropConfirmEntry = nil
                }
            } message: {
                if let entry = dropConfirmEntry {
                    Text(entry.message.isEmpty ? entry.stashRef : "\(entry.stashRef): \(entry.message)")
                }
            }
        }
    }
}
