import SwiftUI
import UIKit

/// Desktop `FilesExplorerTab` + `FileViewerTab` parity — Searchable directory tree,
/// git status badges, quick `@` attach, syntax-highlighted viewer, Markdown preview toggle,
/// tap-line-to-quote, and live Follow mode for background task logs.
struct WorkspaceFilesSheet: View {
    @ObservedObject var store: OrbitIosStore
    let initialPath: String?

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    var body: some View {
        NavigationStack {
            WorkspaceFilesPanel(
                store: store,
                initialPath: initialPath,
                onOpenDiff: { path in
                    dismiss()
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                        store.openGitDiff(focusFilePath: path)
                    }
                }
            )
            .navigationTitle(store.activeWorkspaceFilePath.map { OrbitFormatters.shortPath($0) } ?? "Files")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if store.activeWorkspaceFilePath != nil {
                    ToolbarItem(placement: .topBarLeading) {
                        Button {
                            store.hapticLight()
                            store.activeWorkspaceFilePath = nil
                        } label: {
                            HStack(spacing: 3) {
                                Image(systemName: "chevron.left")
                                    .font(.system(size: 12, weight: .semibold))
                                Text("Tree")
                                    .font(OrbitTypography.subhead(.medium))
                            }
                            .foregroundStyle(palette.primary)
                        }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
        }
    }
}

struct WorkspaceFilesPanel: View {
    @ObservedObject var store: OrbitIosStore
    var initialPath: String? = nil
    var onOpenDiff: ((String) -> Void)? = nil

    @Environment(\.orbitPalette) private var palette
    @State private var searchQuery = ""
    @State private var expandedDirs: Set<String> = []
    @State private var inFileSearch = ""
    @State private var showMarkdownPreview = true
    @State private var followLogMode = false

    private var dirtyPaths: Set<String> {
        Set((store.gitReview?.changedFiles ?? []).map(\.path))
    }

    private struct TreeNode: Identifiable {
        let id: String
        let name: String
        let fullPath: String
        let isDirectory: Bool
        let depth: Int
    }

    private var filteredFiles: [String] {
        let q = searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return store.workspaceFilesList }
        return store.workspaceFilesList.filter { $0.lowercased().contains(q) }
    }

    private var flattenedTreeRows: [TreeNode] {
        let files = filteredFiles
        let q = searchQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        if !q.isEmpty {
            return files.prefix(200).map { path in
                TreeNode(
                    id: "file::\(path)",
                    name: path,
                    fullPath: path,
                    isDirectory: false,
                    depth: 0
                )
            }
        }

        // Build lightweight nested directory map
        var childrenDirs: [String: Set<String>] = [:]
        var childrenFiles: [String: [String]] = [:]

        for raw in files {
            let parts = raw.split(separator: "/").map(String.init)
            guard !parts.isEmpty else { continue }
            var parent = ""
            if parts.count > 1 {
                for idx in 0..<(parts.count - 1) {
                    let dirPath = parent.isEmpty ? parts[idx] : "\(parent)/\(parts[idx])"
                    childrenDirs[parent, default: []].insert(dirPath)
                    parent = dirPath
                }
            }
            childrenFiles[parent, default: []].append(raw)
        }

        var rows: [TreeNode] = []
        func walk(dir: String, depth: Int) {
            let subdirs = (childrenDirs[dir] ?? []).sorted()
            for sub in subdirs {
                let name = OrbitFormatters.shortPath(sub)
                rows.append(TreeNode(id: "dir::\(sub)", name: name, fullPath: sub, isDirectory: true, depth: depth))
                if expandedDirs.contains(sub) {
                    walk(dir: sub, depth: depth + 1)
                }
            }
            let dirFiles = (childrenFiles[dir] ?? []).sorted()
            for file in dirFiles {
                rows.append(TreeNode(
                    id: "file::\(file)",
                    name: OrbitFormatters.shortPath(file),
                    fullPath: file,
                    isDirectory: false,
                    depth: depth
                ))
            }
        }
        walk(dir: "", depth: 0)
        return rows
    }

    var body: some View {
        Group {
            if let path = store.activeWorkspaceFilePath, !path.isEmpty {
                fileViewerContent(path: path)
            } else {
                fileExplorerContent
            }
        }
        .background(palette.canvas.ignoresSafeArea())
        .onAppear {
            if store.workspaceFilesList.isEmpty {
                store.refreshWorkspaceFiles()
            }
            if let initPath = initialPath, !initPath.isEmpty {
                store.activeWorkspaceFilePath = initPath
                store.loadWorkspaceFile(path: initPath)
            }
        }
        .task(id: followLogMode ? (store.activeWorkspaceFilePath ?? "") : "") {
            guard followLogMode, let path = store.activeWorkspaceFilePath, !path.isEmpty else { return }
            while !Task.isCancelled && followLogMode {
                try? await Task.sleep(nanoseconds: 2_000_000_000)
                guard !Task.isCancelled, followLogMode else { break }
                store.loadWorkspaceFile(path: path, forceRefresh: true)
            }
        }
    }

    // MARK: - File Explorer Tree

    private var fileExplorerContent: some View {
        VStack(spacing: 0) {
            HStack(spacing: OrbitSpacing.sm) {
                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass")
                        .font(.system(size: 13))
                        .foregroundStyle(palette.fgMuted)
                    TextField("Filter files…", text: $searchQuery)
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgPrimary)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    if !searchQuery.isEmpty {
                        Button {
                            searchQuery = ""
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .font(.system(size: 13))
                                .foregroundStyle(palette.fgMuted)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 10)
                .frame(height: 36)
                .background(palette.subtleFill, in: RoundedRectangle(cornerRadius: 10, style: .continuous))

                Button {
                    store.refreshWorkspaceFiles()
                    store.hapticLight()
                } label: {
                    Image(systemName: "arrow.clockwise")
                        .font(.system(size: 13.5, weight: .medium))
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 36, height: 36)
                        .background(palette.subtleFill, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.vertical, OrbitSpacing.sm)

            OrbitHairline()

            if flattenedTreeRows.isEmpty {
                VStack(spacing: OrbitSpacing.sm) {
                    Image(systemName: "folder")
                        .font(.system(size: 24))
                        .foregroundStyle(palette.fgMuted)
                    Text(store.workspaceFilesList.isEmpty ? "Loading workspace files…" : "No matching files")
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgMuted)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView(.vertical, showsIndicators: true) {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(flattenedTreeRows) { node in
                            treeRow(node)
                        }
                    }
                    .padding(.vertical, OrbitSpacing.xs)
                }
                .scrollDismissesKeyboard(.interactively)
            }
        }
    }

    private func treeRow(_ node: TreeNode) -> some View {
        let isExpanded = expandedDirs.contains(node.fullPath)
        let isDirty = !node.isDirectory && dirtyPaths.contains(node.fullPath)

        return HStack(spacing: 8) {
            if node.depth > 0 {
                Spacer()
                    .frame(width: CGFloat(node.depth) * 14)
            }

            Button {
                store.hapticLight()
                if node.isDirectory {
                    withAnimation(.easeInOut(duration: 0.16)) {
                        if isExpanded {
                            expandedDirs.remove(node.fullPath)
                        } else {
                            expandedDirs.insert(node.fullPath)
                        }
                    }
                } else {
                    followLogMode = false
                    inFileSearch = ""
                    store.loadWorkspaceFile(path: node.fullPath)
                }
            } label: {
                HStack(spacing: 8) {
                    if node.isDirectory {
                        Image(systemName: "chevron.right")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(palette.fgMuted)
                            .rotationEffect(.degrees(isExpanded ? 90 : 0))
                            .frame(width: 12)
                        Image(systemName: isExpanded ? "folder.fill" : "folder")
                            .font(.system(size: 13.5, weight: .medium))
                            .foregroundStyle(palette.fgSecondary)
                    } else {
                        Image(systemName: OrbitFormatters.semanticFileIcon(for: OrbitFormatters.shortPath(node.fullPath)))
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: 18)
                    }

                    Text(node.name)
                        .font(node.isDirectory ? OrbitTypography.subhead(.medium) : OrbitTypography.code(.regular))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)
                        .truncationMode(.middle)

                    if isDirty {
                        Circle()
                            .fill(palette.warning)
                            .frame(width: 6, height: 6)
                    }

                    Spacer(minLength: 4)
                }
                .frame(minHeight: 38)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if !node.isDirectory {
                Button {
                    store.attachWorkspaceFileToComposer(node.fullPath)
                } label: {
                    Text("@")
                        .font(OrbitTypography.code(.semibold))
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 28, height: 28)
                        .background(palette.subtleFill, in: Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Attach \(node.name) to composer")
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
    }

    // MARK: - Syntax-Highlighted File Viewer

    @ViewBuilder
    private func fileViewerContent(path: String) -> some View {
        let file = store.cachedWorkspaceFile(path: path)
        let isMarkdown = (file?.contentKind == "markdown")
            || path.lowercased().hasSuffix(".md")
            || path.lowercased().hasSuffix(".mdx")
        let isDirty = dirtyPaths.contains(path)

        VStack(spacing: 0) {
            // Viewer Action Toolbar
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    if isMarkdown {
                        pillToggle(
                            label: showMarkdownPreview ? "Source" : "Preview",
                            icon: showMarkdownPreview ? "chevron.left.forwardslash.chevron.right" : "doc.richtext",
                            active: showMarkdownPreview
                        ) {
                            showMarkdownPreview.toggle()
                        }
                    }

                    pillToggle(
                        label: followLogMode ? "Following" : "Follow",
                        icon: followLogMode ? "dot.radiowaves.left.and.right" : "arrow.clockwise",
                        active: followLogMode
                    ) {
                        followLogMode.toggle()
                        if followLogMode {
                            store.loadWorkspaceFile(path: path, forceRefresh: true)
                        }
                    }

                    pillToggle(label: "Attach @", icon: "paperclip", active: false) {
                        store.attachWorkspaceFileToComposer(path)
                    }

                    if let content = file?.content, !content.isEmpty {
                        pillToggle(label: "Copy", icon: "square.on.square", active: false) {
                            UIPasteboard.general.string = content
                            store.hapticLight()
                            store.showToast("Copied file")
                        }
                    }

                    if isDirty {
                        pillToggle(label: "Diff", icon: "arrow.left.arrow.right", active: false) {
                            if let onOpenDiff {
                                onOpenDiff(path)
                            } else {
                                store.openGitDiff(focusFilePath: path)
                            }
                        }
                    }
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, 6)
            }

            // In-File Search Bar (when in source view)
            if !isMarkdown || !showMarkdownPreview {
                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass")
                        .font(.system(size: 12))
                        .foregroundStyle(palette.fgMuted)
                    TextField("Search in file…", text: $inFileSearch)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgPrimary)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    if !inFileSearch.isEmpty {
                        Button {
                            inFileSearch = ""
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .font(.system(size: 12))
                                .foregroundStyle(palette.fgMuted)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 10)
                .frame(height: 30)
                .background(palette.subtleFill, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.bottom, 6)
            }

            OrbitHairline()

            if store.workspaceFileLoadingPath == path && file == nil {
                VStack(spacing: OrbitSpacing.sm) {
                    OrbitSpinnerRingView(color: palette.primary, size: 18)
                    Text("Loading \(OrbitFormatters.shortPath(path))…")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let err = store.workspaceFileError, file == nil {
                VStack(spacing: OrbitSpacing.sm) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.system(size: 22))
                        .foregroundStyle(palette.danger)
                    Text(err)
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgSecondary)
                        .multilineTextAlignment(.center)
                    Button("Retry") {
                        store.loadWorkspaceFile(path: path, forceRefresh: true)
                    }
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.primary)
                }
                .padding(OrbitSpacing.xl)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let file {
                if isMarkdown && showMarkdownPreview {
                    ScrollView(.vertical, showsIndicators: true) {
                        let blocks = file.markdownBlocks.isEmpty
                            ? parseMarkdownPreview(markdown: file.content, streaming: false)
                            : file.markdownBlocks
                        RichMarkdownView(blocks: blocks, fallbackText: file.content)
                            .padding(.horizontal, OrbitSpacing.lg)
                            .padding(.vertical, OrbitSpacing.md)
                    }
                } else {
                    highlightedCodeScrollView(file: file, path: path)
                }
            }
        }
    }

    private func highlightedCodeScrollView(file: FfiWorkspaceFile, path: String) -> some View {
        let query = inFileSearch.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let lines = file.highlightedLines

        return ScrollView([.vertical, .horizontal], showsIndicators: true) {
            LazyVStack(alignment: .leading, spacing: 1) {
                if !lines.isEmpty {
                    ForEach(lines, id: \.lineNumber) { line in
                        let plainText = line.tokens.map(\.text).joined()
                        let isMatch = !query.isEmpty && plainText.lowercased().contains(query)

                        Button {
                            store.quoteFileLine(path: path, lineNumber: line.lineNumber, content: plainText)
                        } label: {
                            HStack(alignment: .top, spacing: 10) {
                                Text("\(line.lineNumber)")
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                    .frame(width: 38, alignment: .trailing)

                                renderedHighlightedTokens(line.tokens)
                                    .fixedSize(horizontal: true, vertical: false)
                            }
                            .padding(.horizontal, OrbitSpacing.md)
                            .padding(.vertical, 1.5)
                            .background(isMatch ? palette.warning.opacity(0.22) : Color.clear)
                        }
                        .buttonStyle(.plain)
                    }
                } else {
                    let rawLines = file.content.components(separatedBy: .newlines)
                    ForEach(Array(rawLines.enumerated()), id: \.offset) { idx, text in
                        let lineNo = UInt32(idx + 1)
                        let isMatch = !query.isEmpty && text.lowercased().contains(query)
                        Button {
                            store.quoteFileLine(path: path, lineNumber: lineNo, content: text)
                        } label: {
                            HStack(alignment: .top, spacing: 10) {
                                Text("\(lineNo)")
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                    .frame(width: 38, alignment: .trailing)
                                Text(text.isEmpty ? " " : text)
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.fgPrimary)
                            }
                            .padding(.horizontal, OrbitSpacing.md)
                            .padding(.vertical, 1.5)
                            .background(isMatch ? palette.warning.opacity(0.22) : Color.clear)
                        }
                        .buttonStyle(.plain)
                    }
                }

                if file.truncated {
                    Text("File truncated for preview (\(file.sizeBytes) bytes total)")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .padding(OrbitSpacing.md)
                }
            }
            .padding(.vertical, OrbitSpacing.sm)
        }
        .background(palette.code)
    }

    private func renderedHighlightedTokens(_ tokens: [FfiHighlightedToken]) -> Text {
        tokens.reduce(Text("")) { acc, tok in
            var piece = Text(tok.text)
                .font(OrbitTypography.code(tok.bold ? .semibold : .regular))
                .foregroundColor(tok.fgHex.isEmpty ? palette.fgPrimary : Color(hex: tok.fgHex))
            if tok.italic {
                piece = piece.italic()
            }
            return acc + piece
        }
    }

    private func pillToggle(
        label: String,
        icon: String,
        active: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button {
            store.hapticLight()
            action()
        } label: {
            HStack(spacing: 4) {
                Image(systemName: icon)
                    .font(.system(size: 11, weight: .medium))
                Text(label)
                    .font(OrbitTypography.meta(.medium))
            }
            .foregroundStyle(active ? palette.accentSky : palette.fgSecondary)
            .padding(.horizontal, 10)
            .frame(height: 28)
            .background(active ? palette.accentSky.opacity(0.14) : palette.subtleFill, in: Capsule())
        }
        .buttonStyle(.plain)
    }
}
