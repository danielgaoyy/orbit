import SwiftUI

struct ContextPickerSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var query = ""

    private var filteredFiles: [String] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let all = store.workspaceFilesList
        guard !trimmed.isEmpty else {
            return Array(all.prefix(60))
        }
        let scored: [(path: String, score: Int)] = all.compactMap { path in
            let lower = path.lowercased()
            let base = OrbitFormatters.shortPath(path).lowercased()
            if base == trimmed {
                return (path, 100)
            } else if base.hasPrefix(trimmed) {
                return (path, 80)
            } else if base.contains(trimmed) {
                return (path, 60)
            } else if lower.contains(trimmed) {
                return (path, 40)
            } else if isFuzzySubsequence(needle: trimmed, haystack: lower) {
                return (path, 20)
            }
            return nil
        }
        return scored
            .sorted { lhs, rhs in
                if lhs.score != rhs.score { return lhs.score > rhs.score }
                return lhs.path.count < rhs.path.count
            }
            .prefix(60)
            .map(\.path)
    }

    var body: some View {
        VStack(spacing: 0) {
            sheetHeader
            searchBar
            OrbitHairline()

            ScrollView(.vertical, showsIndicators: true) {
                LazyVStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                    if query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        slashWorkflowsSection
                        gitAndTerminalContextSection
                    }
                    workspaceFilesSection
                }
                .padding(OrbitSpacing.lg)
            }
        }
        .background(palette.canvas.ignoresSafeArea())
        .onAppear {
            store.refreshWorkspaceFiles()
        }
    }

    private var sheetHeader: some View {
        HStack {
            Text("Context & Workflows")
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)

            Spacer()

            Button {
                dismiss()
            } label: {
                Text("Done")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.primary)
                    .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .frame(height: 52)
    }

    private var searchBar: some View {
        HStack(spacing: OrbitSpacing.sm) {
            Image(systemName: "magnifyingglass")
                .font(OrbitTypography.iconSmall)
                .foregroundStyle(palette.fgMuted)

            TextField("Fuzzy search @files in \(OrbitFormatters.shortPath(store.activeCwd))…", text: $query)
                .font(OrbitTypography.body(.regular))
                .foregroundStyle(palette.fgPrimary)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()

            if !query.isEmpty {
                Button {
                    query = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.fgMuted)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.leading, OrbitSpacing.md)
        .frame(height: OrbitSpacing.hitTarget)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.bottom, OrbitSpacing.md)
    }

    private var slashWorkflowsSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            Text("SLASH WORKFLOWS")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)

            VStack(spacing: OrbitSpacing.xs) {
                ForEach(OrbitSlashDirective.allCases) { directive in
                    let isSelected = store.slashDirective == directive
                    Button {
                        if isSelected {
                            store.slashDirective = nil
                        } else {
                            store.applyQuickWorkflow(directive)
                        }
                        dismiss()
                    } label: {
                        HStack(spacing: OrbitSpacing.md) {
                            Image(systemName: directive.icon)
                                .font(OrbitTypography.icon)
                                .foregroundStyle(palette.primary)
                                .frame(width: 20)

                            VStack(alignment: .leading, spacing: 2) {
                                Text(directive.badgeLabel)
                                    .font(OrbitTypography.code(.semibold))
                                    .foregroundStyle(palette.fgPrimary)
                                Text(directive.subtitle)
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.fgSecondary)
                                    .lineLimit(1)
                            }

                            Spacer()

                            if isSelected {
                                Image(systemName: "checkmark")
                                    .font(OrbitTypography.iconSmall)
                                    .foregroundStyle(palette.primary)
                            }
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .padding(.vertical, OrbitSpacing.sm)
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .background(isSelected ? palette.activeRowFill : palette.elevated)
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
        }
    }

    private var gitAndTerminalContextSection: some View {
        let unstagedItem = ComposerContextItem.gitDiff(staged: false, label: "Unstaged Diff")
        let stagedItem = ComposerContextItem.gitDiff(staged: true, label: "Staged Diff")

        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            Text("GIT & TERMINAL CONTEXT")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)

            VStack(spacing: OrbitSpacing.xs) {
                contextToggleRow(
                    item: unstagedItem,
                    title: "@git-diff (Unstaged)",
                    subtitle: "Attach working tree modifications"
                ) {
                    store.refreshGitReview()
                    store.toggleContextItem(unstagedItem)
                }

                contextToggleRow(
                    item: stagedItem,
                    title: "@staged-diff",
                    subtitle: "Attach staged git index changes"
                ) {
                    store.setGitStagedFilter(true)
                    store.toggleContextItem(stagedItem)
                }

                if let termSnapshot = store.captureTerminalSnapshotText() {
                    let termItem = ComposerContextItem.terminalOutput(label: "Terminal Screen", content: termSnapshot)
                    contextToggleRow(
                        item: termItem,
                        title: "@terminal",
                        subtitle: "Attach visible PTY viewport output"
                    ) {
                        store.toggleContextItem(termItem)
                    }
                }
            }
        }
    }

    private func contextToggleRow(
        item: ComposerContextItem,
        title: String,
        subtitle: String,
        action: @escaping () -> Void
    ) -> some View {
        let isAttached = store.attachedContexts.contains(item)
        return Button(action: action) {
            HStack(spacing: OrbitSpacing.md) {
                Image(systemName: item.icon)
                    .font(OrbitTypography.icon)
                    .foregroundStyle(isAttached ? palette.primary : palette.fgSecondary)
                    .frame(width: 20)

                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(OrbitTypography.code(.medium))
                        .foregroundStyle(palette.fgPrimary)
                    Text(subtitle)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgSecondary)
                }

                Spacer()

                Image(systemName: isAttached ? "checkmark.circle.fill" : "plus.circle")
                    .font(OrbitTypography.icon)
                    .foregroundStyle(isAttached ? palette.primary : palette.fgMuted)
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.vertical, OrbitSpacing.sm)
            .frame(minHeight: OrbitSpacing.hitTarget)
            .background(isAttached ? palette.activeRowFill : palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private var workspaceFilesSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack {
                Text("WORKSPACE FILES (@FILE)")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)

                Spacer()

                Button {
                    store.hapticLight()
                    store.refreshWorkspaceFiles()
                } label: {
                    Text("Refresh")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.primary)
                }
                .buttonStyle(.plain)
            }

            if filteredFiles.isEmpty {
                Text(store.workspaceFilesList.isEmpty
                     ? "No indexed files yet in \(OrbitFormatters.compactPath(store.activeCwd))."
                     : "No files match \"\(query)\".")
                    .orbitSubheadStyle()
                    .foregroundStyle(palette.fgMuted)
                    .padding(.vertical, OrbitSpacing.md)
            } else {
                VStack(spacing: OrbitSpacing.xs) {
                    ForEach(filteredFiles, id: \.self) { filePath in
                        let item = ComposerContextItem.file(path: filePath)
                        let isAttached = store.attachedContexts.contains(item)
                        Button {
                            store.toggleContextItem(item)
                        } label: {
                            HStack(spacing: OrbitSpacing.sm) {
                                Image(systemName: "doc.text")
                                    .font(OrbitTypography.iconSmall)
                                    .foregroundStyle(isAttached ? palette.primary : palette.fgMuted)
                                    .frame(width: 18)

                                VStack(alignment: .leading, spacing: 2) {
                                    Text(OrbitFormatters.shortPath(filePath))
                                        .font(OrbitTypography.code(.medium))
                                        .foregroundStyle(palette.fgPrimary)
                                        .lineLimit(1)

                                    Text(filePath)
                                        .font(OrbitTypography.code(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                }

                                Spacer()

                                Image(systemName: isAttached ? "checkmark.circle.fill" : "plus")
                                    .font(OrbitTypography.iconSmall)
                                    .foregroundStyle(isAttached ? palette.primary : palette.fgMuted)
                            }
                            .padding(.horizontal, OrbitSpacing.md)
                            .padding(.vertical, OrbitSpacing.xs)
                            .frame(minHeight: OrbitSpacing.hitTarget)
                            .background(isAttached ? palette.activeRowFill : palette.elevated)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .strokeBorder(palette.hairline, lineWidth: 0.5)
                            )
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    private func isFuzzySubsequence(needle: String, haystack: String) -> Bool {
        var hIdx = haystack.startIndex
        for ch in needle {
            guard let found = haystack[hIdx...].firstIndex(of: ch) else {
                return false
            }
            hIdx = haystack.index(after: found)
        }
        return true
    }
}
