import SwiftUI

struct DirectoryBrowserSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var manualPathInput = ""

    private var listing: FfiDirectoryListing? {
        store.currentHostDetail?.directoryListing
    }

    private var currentPath: String {
        listing?.currentPath ?? store.activeCwd
    }

    var body: some View {
        VStack(spacing: 0) {
            sheetHeader
            currentPathToolbar
            OrbitHairline()

            ScrollView(.vertical, showsIndicators: true) {
                LazyVStack(alignment: .leading, spacing: OrbitSpacing.md) {
                    if let listing = listing {
                        directoryEntriesSection(listing)
                    } else {
                        loadingOrFallbackSection
                    }

                    if let repos = store.currentHostDetail?.discoveredProjects, !repos.isEmpty {
                        discoveredReposSection(repos)
                    }
                }
                .padding(OrbitSpacing.lg)
            }
        }
        .background(palette.canvas.ignoresSafeArea())
        .onAppear {
            manualPathInput = store.activeCwd
            store.listRemoteDirectory(store.activeCwd == "~" ? nil : store.activeCwd)
        }
        .onChange(of: listing?.currentPath) { newPath in
            if let path = newPath, !path.isEmpty {
                manualPathInput = path
            }
        }
    }

    private var sheetHeader: some View {
        HStack {
            Text("Choose Workspace")
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)

            Spacer()

            Button {
                dismiss()
            } label: {
                Text("Close")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgSecondary)
                    .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .frame(height: 52)
    }

    private var currentPathToolbar: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                Image(systemName: "folder")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(palette.fgMuted)

                TextField("/path/to/workspace", text: $manualPathInput)
                    .font(OrbitTypography.code(.regular))
                    .foregroundStyle(palette.fgPrimary)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .onSubmit {
                        let trimmed = manualPathInput.trimmingCharacters(in: .whitespacesAndNewlines)
                        store.listRemoteDirectory(trimmed.isEmpty ? nil : trimmed)
                    }

                Button {
                    let trimmed = manualPathInput.trimmingCharacters(in: .whitespacesAndNewlines)
                    store.hapticLight()
                    store.listRemoteDirectory(trimmed.isEmpty ? nil : trimmed)
                } label: {
                    Text("Go")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.primary)
                        .padding(.horizontal, OrbitSpacing.sm)
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            .padding(.leading, OrbitSpacing.md)
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )

            HStack(spacing: OrbitSpacing.sm) {
                Button {
                    store.startNewSessionDraft(cwd: currentPath)
                    dismiss()
                } label: {
                    HStack(spacing: OrbitSpacing.xs) {
                        Image(systemName: "checkmark.circle.fill")
                            .font(OrbitTypography.iconSmall)
                        Text("Open Session Here")
                            .font(OrbitTypography.subhead(.semibold))
                    }
                    .foregroundStyle(Color.white)
                    .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                    .background(palette.primary)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)

                Button {
                    store.pinProject(
                        name: OrbitFormatters.shortPath(currentPath),
                        directories: [currentPath]
                    )
                } label: {
                    HStack(spacing: OrbitSpacing.xs) {
                        Image(systemName: "pin")
                            .font(OrbitTypography.iconSmall)
                        Text("Pin")
                            .font(OrbitTypography.subhead(.medium))
                    }
                    .foregroundStyle(palette.fgPrimary)
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .background(palette.elevated)
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
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.bottom, OrbitSpacing.md)
    }

    private func directoryEntriesSection(_ listing: FfiDirectoryListing) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            Text("DIRECTORIES")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)

            if let parent = listing.parentPath {
                Button {
                    store.hapticLight()
                    store.listRemoteDirectory(parent)
                } label: {
                    HStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: "arrow.turn.left.up")
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(palette.fgSecondary)
                            .frame(width: 20)
                        Text(".. (Parent Directory)")
                            .font(OrbitTypography.code(.medium))
                            .foregroundStyle(palette.fgSecondary)
                        Spacer()
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .background(palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .strokeBorder(palette.hairline, lineWidth: 0.5)
                    )
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }

            if listing.entries.isEmpty {
                Text("No subdirectories found.")
                    .orbitSubheadStyle()
                    .foregroundStyle(palette.fgMuted)
                    .padding(.vertical, OrbitSpacing.sm)
            } else {
                ForEach(listing.entries, id: \.path) { entry in
                    HStack(spacing: OrbitSpacing.xs) {
                        Button {
                            store.hapticLight()
                            store.listRemoteDirectory(entry.path)
                        } label: {
                            HStack(spacing: OrbitSpacing.sm) {
                                Image(systemName: entry.isGitRepo ? "arrow.triangle.branch" : "folder")
                                    .font(OrbitTypography.iconSmall)
                                    .foregroundStyle(entry.isGitRepo ? palette.primary : palette.fgSecondary)
                                    .frame(width: 20)

                                Text(entry.name)
                                    .font(OrbitTypography.body(.regular))
                                    .foregroundStyle(palette.fgPrimary)
                                    .lineLimit(1)

                                if entry.isGitRepo {
                                    Text("git")
                                        .font(OrbitTypography.meta(.medium))
                                        .foregroundStyle(palette.primary)
                                        .padding(.horizontal, OrbitSpacing.xs + 2)
                                        .padding(.vertical, 1)
                                        .background(palette.primary.opacity(0.12))
                                        .clipShape(Capsule())
                                }

                                Spacer()
                            }
                            .padding(.leading, OrbitSpacing.md)
                            .frame(minHeight: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)

                        Button {
                            store.startNewSessionDraft(cwd: entry.path)
                            dismiss()
                        } label: {
                            Text("Select")
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(palette.primary)
                                .padding(.horizontal, OrbitSpacing.md)
                                .frame(minHeight: OrbitSpacing.hitTarget)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                    .background(palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .strokeBorder(palette.hairline, lineWidth: 0.5)
                    )
                }
            }
        }
    }

    private var loadingOrFallbackSection: some View {
        HStack(spacing: OrbitSpacing.sm) {
            ProgressView()
            Text("Fetching remote directory listing…")
                .orbitSubheadStyle()
                .foregroundStyle(palette.fgSecondary)
        }
        .padding(.vertical, OrbitSpacing.md)
    }

    private func discoveredReposSection(_ repos: [FfiDiscoveredProject]) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            Text("DISCOVERED GIT REPOSITORIES")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)
                .padding(.top, OrbitSpacing.sm)

            ForEach(repos, id: \.path) { repo in
                Button {
                    store.startNewSessionDraft(cwd: repo.path)
                    dismiss()
                } label: {
                    HStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: "arrow.triangle.branch")
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(palette.primary)
                            .frame(width: 20)

                        VStack(alignment: .leading, spacing: 2) {
                            Text(repo.name)
                                .font(OrbitTypography.body(.regular))
                                .foregroundStyle(palette.fgPrimary)
                            Text(OrbitFormatters.compactPath(repo.path))
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .lineLimit(1)
                        }

                        Spacer()

                        if let branch = repo.gitBranch {
                            Text(branch)
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgSecondary)
                        }
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .padding(.vertical, OrbitSpacing.xs)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .background(palette.elevated)
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
