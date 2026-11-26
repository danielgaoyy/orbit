import SwiftUI

struct DirectoryBrowserSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var manualPathInput = ""
    @State private var isEditingPath = false
    @State private var filterQuery = ""
    @State private var didRequestInitialListing = false
    @State private var pendingRequestedPath: String? = nil
    @State private var manualPathError: String? = nil
    @FocusState private var isPathFieldFocused: Bool

    private var groupedBackground: Color {
        palette.isDark ? palette.canvas : palette.drawer
    }

    private var listing: FfiDirectoryListing? {
        store.currentHostDetail?.directoryListing
    }

    private var currentPath: String {
        let raw = listing?.currentPath ?? store.activeCwd
        return raw.isEmpty ? "~" : raw
    }

    private var currentFolderName: String {
        OrbitFormatters.shortPath(currentPath)
    }

    private var normalizedFilter: String {
        filterQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }

    private var filteredEntries: [FfiDirectoryEntry] {
        let entries = listing?.entries ?? []
        guard !normalizedFilter.isEmpty else { return entries }
        return entries.filter { $0.name.lowercased().contains(normalizedFilter) }
    }

    private var filteredRepos: [FfiDiscoveredProject] {
        let repos = (store.currentHostDetail?.discoveredProjects ?? []).filter {
            !$0.path.contains("/worktrees/")
        }
        guard !normalizedFilter.isEmpty else { return repos }
        return repos.filter {
            $0.name.lowercased().contains(normalizedFilter)
                || $0.path.lowercased().contains(normalizedFilter)
                || ($0.gitBranch?.lowercased().contains(normalizedFilter) ?? false)
        }
    }

    private var pinnedProjectForCurrentPath: FfiClientProject? {
        guard let hostId = store.selectedHostId else { return nil }
        return store.projects.first { proj in
            proj.hostId == hostId
                && proj.directories.contains { OrbitFormatters.arePathsEquivalent($0, currentPath) }
        }
    }

    private struct BreadcrumbSegment: Identifiable, Equatable {
        let id: String
        let label: String
        let targetPath: String
    }

    private var breadcrumbSegments: [BreadcrumbSegment] {
        let trimmed = currentPath.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty || trimmed == "~" {
            return [BreadcrumbSegment(id: "~", label: "~", targetPath: "~")]
        }
        if trimmed == "/" {
            return [BreadcrumbSegment(id: "/", label: "/", targetPath: "/")]
        }

        let isAbsolute = trimmed.hasPrefix("/")
        let parts = trimmed.split(separator: "/").map(String.init)
        var segments: [BreadcrumbSegment] = []

        if isAbsolute {
            // Collapse /Users/<name> or /home/<name> into clean ~ root when present
            if parts.count >= 2 && (parts[0] == "Users" || parts[0] == "home") {
                let homePath = "/\(parts[0])/\(parts[1])"
                segments.append(BreadcrumbSegment(id: homePath, label: "~", targetPath: homePath))
                var running = homePath
                for part in parts.dropFirst(2) {
                    running += "/\(part)"
                    segments.append(BreadcrumbSegment(id: running, label: part, targetPath: running))
                }
            } else {
                segments.append(BreadcrumbSegment(id: "/", label: "/", targetPath: "/"))
                var running = ""
                for part in parts {
                    running += "/\(part)"
                    segments.append(BreadcrumbSegment(id: running, label: part, targetPath: running))
                }
            }
        } else {
            var running = ""
            for (idx, part) in parts.enumerated() {
                running = idx == 0 ? part : "\(running)/\(part)"
                segments.append(BreadcrumbSegment(id: running, label: part, targetPath: running))
            }
        }
        return segments
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                topNavigationControls
                OrbitHairline()

                let displayedError = manualPathError ?? store.lastError
                if let err = displayedError, !err.isEmpty {
                    HStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .font(.system(size: 13))
                            .foregroundStyle(palette.danger)
                        Text(err)
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgPrimary)
                            .lineLimit(2)
                        Spacer()
                        Button {
                            manualPathError = nil
                            store.clearError()
                            let trimmed = manualPathInput.trimmingCharacters(in: .whitespacesAndNewlines)
                            let retryTarget: String? = (!trimmed.isEmpty && trimmed != "~")
                                ? trimmed
                                : (currentPath == "~" ? nil : currentPath)
                            pendingRequestedPath = retryTarget
                            store.listRemoteDirectory(retryTarget)
                        } label: {
                            Text(OrbitL10n.tr("Retry"))
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(palette.primary)
                        }
                        .buttonStyle(.plain)
                    }
                    .padding(.horizontal, OrbitSpacing.lg)
                    .padding(.vertical, OrbitSpacing.xs + 2)
                    .background(palette.danger.opacity(0.10))
                    OrbitHairline()
                }

                ScrollView(.vertical, showsIndicators: true) {
                    LazyVStack(alignment: .leading, spacing: OrbitSpacing.xl) {
                        foldersSection

                        if !filteredRepos.isEmpty {
                            repositoriesSection
                        }
                    }
                    .padding(.horizontal, OrbitSpacing.lg)
                    .padding(.vertical, OrbitSpacing.lg)
                }
                .scrollDismissesKeyboard(.interactively)
            }
            .background(groupedBackground.ignoresSafeArea())
            .safeAreaInset(edge: .bottom, spacing: 0) {
                bottomActionDock
            }
            .navigationTitle("Choose Workspace")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Close") {
                        dismiss()
                    }
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgSecondary)
                }
            }
        }
        .onAppear {
            manualPathInput = store.activeCwd
            if !didRequestInitialListing {
                didRequestInitialListing = true
                if listing == nil {
                    store.listRemoteDirectory(store.activeCwd == "~" ? nil : store.activeCwd)
                }
            }
        }
        .onChange(of: listing?.currentPath) { newPath in
            if let path = newPath, !path.isEmpty {
                if let requested = pendingRequestedPath {
                    pendingRequestedPath = nil
                    let normalizedReq = requested.hasSuffix("/") && requested.count > 1 ? String(requested.dropLast()) : requested
                    let normalizedResp = path.hasSuffix("/") && path.count > 1 ? String(path.dropLast()) : path
                    if normalizedReq != "~" && normalizedResp != normalizedReq {
                        manualPathError = OrbitL10n.fmt("Directory not found: %@", requested)
                        manualPathInput = requested
                        isEditingPath = true
                        return
                    }
                }
                manualPathError = nil
                manualPathInput = path
            }
        }
    }

    // MARK: - Top Breadcrumb / Manual Path + Filter Controls

    private var topNavigationControls: some View {
        VStack(spacing: OrbitSpacing.sm) {
            if isEditingPath {
                HStack(spacing: OrbitSpacing.sm) {
                    Image(systemName: "folder")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(palette.fgMuted)

                    TextField("/path/to/workspace", text: $manualPathInput)
                        .font(OrbitTypography.code(.regular))
                        .foregroundStyle(palette.fgPrimary)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .submitLabel(.go)
                        .focused($isPathFieldFocused)
                        .onSubmit {
                            submitManualPath()
                        }

                    Button("Go") {
                        submitManualPath()
                    }
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.primary)
                    .buttonStyle(.plain)

                    Button {
                        isEditingPath = false
                        manualPathInput = currentPath
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 15))
                            .foregroundStyle(palette.fgMuted)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Cancel path editing")
                }
                .padding(.horizontal, OrbitSpacing.md)
                .frame(height: 38)
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )
            } else {
                HStack(spacing: OrbitSpacing.xs) {
                    if let parent = listing?.parentPath {
                        Button {
                            store.hapticLight()
                            filterQuery = ""
                            store.listRemoteDirectory(parent)
                        } label: {
                            Image(systemName: "chevron.left")
                                .font(.system(size: 12, weight: .semibold))
                                .foregroundStyle(palette.fgSecondary)
                                .frame(width: 30, height: 30)
                                .background(palette.elevated)
                                .clipShape(Circle())
                                .overlay(
                                    Circle()
                                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                                )
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Parent directory")
                    }

                    ScrollViewReader { proxy in
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 4) {
                                ForEach(Array(breadcrumbSegments.enumerated()), id: \.element.id) { idx, seg in
                                    let isLast = idx == breadcrumbSegments.count - 1
                                    if idx > 0 {
                                        Image(systemName: "chevron.right")
                                            .font(.system(size: 9, weight: .semibold))
                                            .foregroundStyle(palette.fgMuted)
                                    }
                                    Button {
                                        guard !isLast else { return }
                                        store.hapticLight()
                                        filterQuery = ""
                                        store.listRemoteDirectory(seg.targetPath)
                                    } label: {
                                        Text(seg.label)
                                            .font(OrbitTypography.meta(isLast ? .semibold : .medium))
                                            .foregroundStyle(isLast ? palette.fgPrimary : palette.fgSecondary)
                                            .padding(.horizontal, 8)
                                            .padding(.vertical, 5)
                                            .background(isLast ? palette.elevated : Color.clear)
                                            .clipShape(Capsule())
                                            .overlay(
                                                Capsule()
                                                    .strokeBorder(isLast ? palette.hairline : Color.clear, lineWidth: 0.5)
                                            )
                                    }
                                    .buttonStyle(.plain)
                                    .id(seg.id)
                                }
                            }
                            .padding(.vertical, 2)
                        }
                        .onAppear {
                            if let lastId = breadcrumbSegments.last?.id {
                                proxy.scrollTo(lastId, anchor: .trailing)
                            }
                        }
                        .onChange(of: currentPath) { _ in
                            if let lastId = breadcrumbSegments.last?.id {
                                withAnimation(.easeOut(duration: 0.16)) {
                                    proxy.scrollTo(lastId, anchor: .trailing)
                                }
                            }
                        }
                    }

                    Button {
                        store.hapticLight()
                        manualPathInput = currentPath
                        isEditingPath = true
                        DispatchQueue.main.asyncAfter(deadline: .now() + 0.06) {
                            isPathFieldFocused = true
                        }
                    } label: {
                        Image(systemName: "pencil")
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(palette.fgSecondary)
                            .frame(width: 30, height: 30)
                            .background(palette.elevated)
                            .clipShape(Circle())
                            .overlay(
                                Circle()
                                    .strokeBorder(palette.hairline, lineWidth: 0.5)
                            )
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Enter path manually")
                }
            }

            // Quiet single-line filter input for both folders and repositories
            HStack(spacing: OrbitSpacing.xs + 2) {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(palette.fgMuted)

                TextField("Filter folders or repositories…", text: $filterQuery)
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgPrimary)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()

                if !filterQuery.isEmpty {
                    Button {
                        filterQuery = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 14))
                            .foregroundStyle(palette.fgMuted)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear filter")
                }
            }
            .padding(.horizontal, OrbitSpacing.md)
            .frame(height: 36)
            .background(palette.isDark ? palette.elevated : palette.subtleFill)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.top, OrbitSpacing.sm)
        .padding(.bottom, OrbitSpacing.md)
        .background(groupedBackground)
    }

    private func submitManualPath() {
        let trimmed = manualPathInput.trimmingCharacters(in: .whitespacesAndNewlines)
        store.hapticLight()
        filterQuery = ""
        manualPathError = nil
        pendingRequestedPath = (trimmed.isEmpty || trimmed == "~") ? nil : trimmed
        store.listRemoteDirectory(trimmed.isEmpty ? nil : trimmed)
        isPathFieldFocused = false
        if pendingRequestedPath == nil {
            isEditingPath = false
        }
    }

    // MARK: - Grouped Folders Section

    private var foldersSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            HStack {
                Text("Folders")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgMuted)
                Spacer()
                if let host = store.activeHost, store.hosts.count > 1 {
                    Text(host.displayName)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                }
            }
            .padding(.horizontal, OrbitSpacing.xs)

            VStack(spacing: 0) {
                if listing == nil {
                    HStack(spacing: OrbitSpacing.sm) {
                        ProgressView()
                            .controlSize(.small)
                        Text("Loading directory…")
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgSecondary)
                        Spacer()
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(minHeight: 48)
                } else if filteredEntries.isEmpty {
                    HStack {
                        Text(normalizedFilter.isEmpty ? "No subfolders in this directory" : "No matching folders")
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgMuted)
                        Spacer()
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(minHeight: 48)
                } else {
                    ForEach(Array(filteredEntries.enumerated()), id: \.element.path) { idx, entry in
                        Button {
                            store.hapticLight()
                            filterQuery = ""
                            store.listRemoteDirectory(entry.path)
                        } label: {
                            HStack(spacing: OrbitSpacing.sm + 2) {
                                Image(systemName: entry.isGitRepo ? "arrow.triangle.branch" : "folder")
                                    .font(.system(size: 14, weight: .medium))
                                    .foregroundStyle(entry.isGitRepo ? palette.primary : palette.fgSecondary)
                                    .frame(width: 20, alignment: .center)

                                Text(entry.name)
                                    .font(OrbitTypography.subhead(.regular))
                                    .foregroundStyle(palette.fgPrimary)
                                    .lineLimit(1)
                                    .truncationMode(.middle)

                                if entry.isGitRepo {
                                    Text("git")
                                        .font(.system(size: 10.5, weight: .medium))
                                        .foregroundStyle(palette.fgSecondary)
                                        .padding(.horizontal, 6)
                                        .padding(.vertical, 1.5)
                                        .background(palette.subtleFill, in: Capsule())
                                }

                                Spacer(minLength: OrbitSpacing.xs)

                                Image(systemName: "chevron.right")
                                    .font(.system(size: 11, weight: .semibold))
                                    .foregroundStyle(palette.fgMuted)
                            }
                            .padding(.horizontal, OrbitSpacing.md)
                            .frame(minHeight: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)

                        if idx < filteredEntries.count - 1 {
                            OrbitHairline()
                                .padding(.leading, 44)
                        }
                    }
                }
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    // MARK: - Grouped Discovered Repositories Section (1-Tap Direct Open)

    private var repositoriesSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text("Repositories")
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, OrbitSpacing.xs)

            VStack(spacing: 0) {
                ForEach(Array(filteredRepos.enumerated()), id: \.element.path) { idx, repo in
                    let isCurrent = OrbitFormatters.arePathsEquivalent(repo.path, currentPath)
                    Button {
                        store.hapticLight()
                        store.startNewSessionDraft(cwd: repo.path)
                        dismiss()
                    } label: {
                        HStack(spacing: OrbitSpacing.sm + 2) {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.system(size: 14, weight: .medium))
                                .foregroundStyle(palette.primary)
                                .frame(width: 20, alignment: .center)

                            VStack(alignment: .leading, spacing: 2) {
                                Text(repo.name)
                                    .font(OrbitTypography.subhead(isCurrent ? .semibold : .regular))
                                    .foregroundStyle(palette.fgPrimary)
                                    .lineLimit(1)

                                Text(OrbitFormatters.compactPath(repo.path))
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                            }

                            Spacer(minLength: OrbitSpacing.sm)

                            if let branch = repo.gitBranch, !branch.isEmpty {
                                Text(branch)
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.fgSecondary)
                                    .lineLimit(1)
                                    .frame(maxWidth: 110, alignment: .trailing)
                            }
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .padding(.vertical, 9)
                        .frame(minHeight: 52)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)

                    if idx < filteredRepos.count - 1 {
                        OrbitHairline()
                            .padding(.leading, 44)
                    }
                }
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    // MARK: - Sticky Bottom Thumb-Zone Action Dock

    private var bottomActionDock: some View {
        let pinnedProj = pinnedProjectForCurrentPath
        let isPinned = pinnedProj != nil

        return VStack(spacing: OrbitSpacing.sm) {
            OrbitHairline()

            VStack(spacing: OrbitSpacing.sm) {
                HStack(spacing: OrbitSpacing.xs) {
                    Image(systemName: "folder")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(palette.fgMuted)
                    Text(OrbitFormatters.compactPath(currentPath))
                        .font(OrbitTypography.code(.regular))
                        .foregroundStyle(palette.fgSecondary)
                        .lineLimit(1)
                        .truncationMode(.head)
                    Spacer(minLength: 0)
                }

                HStack(spacing: OrbitSpacing.sm) {
                    Button {
                        store.hapticLight()
                        if let existing = pinnedProj {
                            store.unpinProject(existing.projectId)
                        } else {
                            store.pinProject(
                                name: currentFolderName,
                                directories: [currentPath]
                            )
                        }
                    } label: {
                        HStack(spacing: 5) {
                            Image(systemName: isPinned ? "pin.fill" : "pin")
                                .font(.system(size: 13, weight: .medium))
                            Text(isPinned ? "Pinned" : "Pin")
                                .font(OrbitTypography.subhead(.medium))
                        }
                        .foregroundStyle(isPinned ? palette.primary : palette.fgPrimary)
                        .padding(.horizontal, OrbitSpacing.md + 2)
                        .frame(height: OrbitSpacing.hitTarget)
                        .background(palette.elevated)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .strokeBorder(palette.hairline, lineWidth: 0.5)
                        )
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)

                    Button {
                        store.hapticLight()
                        if pinnedProjectForCurrentPath == nil && currentPath != "~" {
                            store.addProjectDirectory(name: currentFolderName, path: currentPath)
                        }
                        store.startNewSessionDraft(cwd: currentPath)
                        dismiss()
                    } label: {
                        Text("Open \(currentFolderName)")
                            .font(OrbitTypography.subhead(.semibold))
                            .foregroundStyle(palette.onPrimary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .padding(.horizontal, OrbitSpacing.md)
                            .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                            .background(palette.primary)
                            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.bottom, OrbitSpacing.sm)
        }
        .background(groupedBackground)
    }
}
