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
    let isModalSheet: Bool
    let onClose: () -> Void

    @Environment(\.orbitPalette) private var palette
    @State private var expandedOverrides: [String: Bool] = [:]

    private var review: FfiGitReview? {
        store.gitReview
    }

    private var effectiveDiffFiles: [FfiDiffFile] {
        if let files = review?.diffFiles, !files.isEmpty {
            return files
        }
        if !store.gitStagedMode, let sessionDiffs = store.agentStage?.sessionTurnDiffs, !sessionDiffs.isEmpty {
            return sessionDiffs
        }
        return []
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
            OrbitHairline()
            diffScrollBody
        }
        .onAppear {
            store.refreshGitReview()
        }
    }

    // MARK: - Header

    private var headerBar: some View {
        HStack(spacing: OrbitSpacing.sm) {
            Text("Git Diff")
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

            Button(action: onClose) {
                if isModalSheet {
                    Text("Done")
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                        .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                } else {
                    Image(systemName: "xmark")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
            }
            .buttonStyle(.plain)
            .accessibilityLabel(isModalSheet ? "Close Git Diff" : "Hide Git Diff Inspector")
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

            // Segmented Unstaged / Staged Toggle
            HStack(spacing: OrbitSpacing.xs) {
                stagedSegmentButton(title: "Unstaged", isSelected: !store.gitStagedMode) {
                    store.setGitStagedFilter(false)
                }
                stagedSegmentButton(title: "Staged", isSelected: store.gitStagedMode) {
                    store.setGitStagedFilter(true)
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
                                    }
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
                        if let jump = store.jumpToDiffFilePath {
                            expandedOverrides[jump] = true
                            DispatchQueue.main.asyncAfter(deadline: .now() + 0.12) {
                                withAnimation(.easeInOut(duration: 0.2)) {
                                    proxy.scrollTo("diff-file-\(jump)", anchor: .top)
                                }
                                store.jumpToDiffFilePath = nil
                            }
                        }
                    }
                }
            }
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
                    Text(store.gitStagedMode
                         ? "No staged changes in \(OrbitFormatters.compactPath(store.activeCwd))."
                         : "No unstaged modifications in \(OrbitFormatters.compactPath(store.activeCwd)).")
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
}

// MARK: - Virtualized & Collapsible File Diff Card (120-Line Cap)

private struct DiffFileSectionCard: View {
    @ObservedObject var store: OrbitIosStore
    let file: FfiDiffFile
    let isExpanded: Bool
    let isAutoCollapsedKind: Bool
    let onToggleExpand: () -> Void
    let onJumpToHunk: (Int) -> Void

    @Environment(\.orbitPalette) private var palette
    @State private var showAllLines = false
    @State private var currentHunkCursor = 0

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
                    .padding(.trailing, hunkOffsets.count > 1 ? OrbitSpacing.xs : OrbitSpacing.md)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)

                if hunkOffsets.count > 1 {
                    Button {
                        let nextCursor = (currentHunkCursor + 1) % hunkOffsets.count
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
                    .font(OrbitTypography.code(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .frame(width: 28, alignment: .trailing)

                Text(line.newLine.map(String.init) ?? "")
                    .font(OrbitTypography.code(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .frame(width: 28, alignment: .trailing)

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
