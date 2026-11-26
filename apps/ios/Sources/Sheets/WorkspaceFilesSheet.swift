import CoreText
import SwiftUI
import UIKit
import WebKit

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
    @State private var showSvgPreview = true
    @State private var wrapCodeLines = false
    @State private var activeMatchCursor = 0
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
        let isImage = file?.contentKind == "image" || OrbitPathLogic.isPreviewableImagePath(path)
        let isSvg = path.lowercased().hasSuffix(".svg") && (file?.content.contains("<svg") ?? false)
        let isBinary = file?.contentKind == "binary"
        let isMarkdown = !isImage && !isSvg && !isBinary && (
            file?.language.lowercased() == "markdown"
            || OrbitPathLogic.isMarkdownPath(path)
        )
        let isDirty = dirtyPaths.contains(path)
        let matchingLineNumbers = computeMatchingLineNumbers(file: file)

        VStack(spacing: 0) {
            if onOpenDiff == nil {
                HStack(spacing: OrbitSpacing.xs) {
                    Button {
                        store.hapticLight()
                        store.activeWorkspaceFilePath = nil
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "chevron.left")
                                .font(.system(size: 12, weight: .semibold))
                            Text(OrbitL10n.tr("Back"))
                                .font(OrbitTypography.subhead(.medium))
                        }
                        .foregroundStyle(palette.primary)
                        .padding(.horizontal, OrbitSpacing.sm)
                        .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Back to Workspace Files")

                    Text(OrbitFormatters.shortPath(path))
                        .font(OrbitTypography.code(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)
                        .truncationMode(.middle)

                    Spacer(minLength: 4)
                }
                .padding(.horizontal, OrbitSpacing.sm)
                .background(palette.elevated)
                OrbitHairline()
            }

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

                    if isSvg {
                        pillToggle(
                            label: showSvgPreview ? "Source" : "Preview",
                            icon: showSvgPreview ? "chevron.left.forwardslash.chevron.right" : "photo",
                            active: showSvgPreview
                        ) {
                            showSvgPreview.toggle()
                        }
                    }

                    if (!isMarkdown || !showMarkdownPreview) && (!isSvg || !showSvgPreview) && !isImage && !isBinary {
                        pillToggle(
                            label: wrapCodeLines ? "Wrapped" : "Wrap",
                            icon: "arrow.turn.down.left",
                            active: wrapCodeLines
                        ) {
                            wrapCodeLines.toggle()
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
            if (!isMarkdown || !showMarkdownPreview) && (!isSvg || !showSvgPreview) {
                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass")
                        .font(.system(size: 12))
                        .foregroundStyle(palette.fgMuted)
                    TextField("Search in file…", text: $inFileSearch)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgPrimary)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .onChange(of: inFileSearch) { _ in
                            activeMatchCursor = 0
                            if let firstMatch = computeMatchingLineNumbers(file: file).first {
                                store.activeWorkspaceFileLine = Int(firstMatch)
                            }
                        }
                    if !inFileSearch.isEmpty {
                        if !matchingLineNumbers.isEmpty {
                            let safeIdx = min(activeMatchCursor, matchingLineNumbers.count - 1)
                            Text("\(safeIdx + 1)/\(matchingLineNumbers.count)")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                                .monospacedDigit()

                            Button {
                                stepSearchMatch(matches: matchingLineNumbers, delta: -1)
                            } label: {
                                Image(systemName: "chevron.up")
                                    .font(.system(size: 11, weight: .semibold))
                                    .foregroundStyle(palette.fgSecondary)
                                    .frame(width: 28, height: 28)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("Previous Match")

                            Button {
                                stepSearchMatch(matches: matchingLineNumbers, delta: 1)
                            } label: {
                                Image(systemName: "chevron.down")
                                    .font(.system(size: 11, weight: .semibold))
                                    .foregroundStyle(palette.fgSecondary)
                                    .frame(width: 28, height: 28)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("Next Match")
                        } else {
                            Text("0")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgMuted)
                        }

                        Button {
                            inFileSearch = ""
                            activeMatchCursor = 0
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .font(.system(size: 12))
                                .foregroundStyle(palette.fgMuted)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 10)
                .frame(height: 32)
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
                if file.contentKind == "image",
                   let decodedData = Data(base64Encoded: file.content, options: .ignoreUnknownCharacters),
                   let uiImage = UIImage(data: decodedData) {
                    ScrollView([.vertical, .horizontal], showsIndicators: true) {
                        VStack(spacing: OrbitSpacing.md) {
                            Image(uiImage: uiImage)
                                .resizable()
                                .scaledToFit()
                                .frame(maxWidth: .infinity)
                                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            Text("\(OrbitFormatters.shortPath(file.path)) · \(ByteCountFormatter.string(fromByteCount: Int64(file.sizeBytes), countStyle: .file))")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }
                        .padding(OrbitSpacing.lg)
                    }
                } else if isSvg && showSvgPreview {
                    OrbitFileSvgPreviewCanvas(svg: file.content, isDark: palette.isDark)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if file.contentKind == "binary" {
                    VStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: "doc.zipper")
                            .font(.system(size: 28, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                        Text("Binary file preview unavailable")
                            .font(OrbitTypography.subhead(.semibold))
                            .foregroundStyle(palette.fgPrimary)
                        Text("\(OrbitFormatters.shortPath(file.path)) · \(ByteCountFormatter.string(fromByteCount: Int64(file.sizeBytes), countStyle: .file))")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .padding(OrbitSpacing.xl)
                } else if isMarkdown && showMarkdownPreview {
                    ScrollView(.vertical, showsIndicators: true) {
                        let blocks = file.markdownBlocks.isEmpty
                            ? parseMarkdownPreview(markdown: file.content, streaming: false)
                            : file.markdownBlocks
                        RichMarkdownView(blocks: blocks, fallbackText: file.content, workspaceCwd: store.activeCwd)
                            .padding(.horizontal, OrbitSpacing.lg)
                            .padding(.vertical, OrbitSpacing.md)
                    }
                } else {
                    highlightedCodeScrollView(file: file, path: path, matchingLines: Set(matchingLineNumbers))
                }
            }
        }
    }

    private func computeMatchingLineNumbers(file: FfiWorkspaceFile?) -> [UInt32] {
        let query = inFileSearch.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !query.isEmpty, let file else { return [] }
        if !file.highlightedLines.isEmpty {
            return file.highlightedLines.compactMap { line in
                let text = line.tokens.map(\.text).joined()
                return text.lowercased().contains(query) ? line.lineNumber : nil
            }
        }
        return file.content.components(separatedBy: .newlines).enumerated().compactMap { idx, text in
            text.lowercased().contains(query) ? UInt32(idx + 1) : nil
        }
    }

    private func stepSearchMatch(matches: [UInt32], delta: Int) {
        guard !matches.isEmpty else { return }
        let nextIdx = (activeMatchCursor + delta + matches.count) % matches.count
        activeMatchCursor = nextIdx
        store.activeWorkspaceFileLine = Int(matches[nextIdx])
        store.hapticLight()
    }

    private func highlightedCodeScrollView(file: FfiWorkspaceFile, path: String, matchingLines: Set<UInt32>) -> some View {
        OrbitCodeTableView(
            file: file,
            path: path,
            wrapLines: wrapCodeLines,
            matchingLines: matchingLines,
            targetLine: store.activeWorkspaceFileLine,
            palette: palette,
            onQuoteLine: { lineNo, content in
                store.quoteFileLine(path: path, lineNumber: lineNo, content: content)
            }
        )
        .background(palette.code)
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

private struct OrbitCodeTableView: UIViewRepresentable {
    let file: FfiWorkspaceFile
    let path: String
    let wrapLines: Bool
    let matchingLines: Set<UInt32>
    let targetLine: Int?
    let palette: OrbitPalette
    let onQuoteLine: (UInt32, String) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    func makeUIView(context: Context) -> UITableView {
        let table = UITableView(frame: .zero, style: .plain)
        table.separatorStyle = .none
        table.backgroundColor = .clear
        table.showsVerticalScrollIndicator = true
        table.showsHorizontalScrollIndicator = false
        table.keyboardDismissMode = .interactive
        table.contentInset = UIEdgeInsets(top: 6, left: 0, bottom: 20, right: 0)
        table.estimatedRowHeight = CGFloat(Coordinator.chunkSize) * Coordinator.fixedLineHeight
        table.rowHeight = UITableView.automaticDimension
        table.register(CodeChunkCell.self, forCellReuseIdentifier: CodeChunkCell.reuseId)
        table.dataSource = context.coordinator
        table.delegate = context.coordinator

        let tap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.handleTap(_:)))
        tap.cancelsTouchesInView = false
        table.addGestureRecognizer(tap)
        context.coordinator.attachedTableView = table

        context.coordinator.reloadDataIfNeeded(tableView: table, parent: self)
        return table
    }

    func updateUIView(_ tableView: UITableView, context: Context) {
        context.coordinator.parent = self
        context.coordinator.reloadDataIfNeeded(tableView: tableView, parent: self)
    }

    final class Coordinator: NSObject, UITableViewDataSource, UITableViewDelegate {
        static let chunkSize = 50
        static let fixedLineHeight: CGFloat = 18.0

        var parent: OrbitCodeTableView
        weak var attachedTableView: UITableView?
        private var lastPath: String = ""
        private var lastContentHash: Int = 0
        private var lastLineCount: Int = 0
        private var lastIsDark: Bool = false
        private var lastWrap: Bool = false
        private var lastTargetLine: Int? = nil
        private var lastMatchingLines: Set<UInt32> = []
        private var rawLines: [String] = []
        private let chunkAttrCache = NSCache<NSNumber, NSAttributedString>()
        private let unwrappedChunkCache = NSCache<NSNumber, UnwrappedChunkData>()
        private var hasTruncatedFooter = false

        init(parent: OrbitCodeTableView) {
            self.parent = parent
            chunkAttrCache.countLimit = 64
            unwrappedChunkCache.countLimit = 64
        }

        private var totalCodeRows: Int {
            parent.file.highlightedLines.isEmpty ? rawLines.count : parent.file.highlightedLines.count
        }

        private var totalChunks: Int {
            let count = totalCodeRows
            return count == 0 ? 0 : (count + Self.chunkSize - 1) / Self.chunkSize
        }

        func reloadDataIfNeeded(tableView: UITableView, parent: OrbitCodeTableView) {
            let contentHash = parent.file.content.hashValue
            let lineCount = !parent.file.highlightedLines.isEmpty
                ? parent.file.highlightedLines.count
                : (rawLines.isEmpty || lastPath != parent.path || lastContentHash != contentHash
                    ? parent.file.content.components(separatedBy: .newlines).count
                    : rawLines.count)

            let fileChanged = lastPath != parent.path
                || lastContentHash != contentHash
                || lastIsDark != parent.palette.isDark
                || lastWrap != parent.wrapLines
                || lastLineCount != lineCount

            let highlightChanged = lastMatchingLines != parent.matchingLines || lastTargetLine != parent.targetLine

            if fileChanged {
                if parent.file.highlightedLines.isEmpty {
                    rawLines = parent.file.content.components(separatedBy: .newlines)
                } else {
                    rawLines = []
                }
                chunkAttrCache.removeAllObjects()
                unwrappedChunkCache.removeAllObjects()
                lastPath = parent.path
                lastContentHash = contentHash
                lastIsDark = parent.palette.isDark
                lastWrap = parent.wrapLines
                lastLineCount = !parent.file.highlightedLines.isEmpty ? parent.file.highlightedLines.count : rawLines.count
                hasTruncatedFooter = parent.file.truncated
                lastMatchingLines = parent.matchingLines
                tableView.reloadData()
            } else if highlightChanged {
                chunkAttrCache.removeAllObjects()
                unwrappedChunkCache.removeAllObjects()
                lastMatchingLines = parent.matchingLines
                for indexPath in tableView.indexPathsForVisibleRows ?? [] {
                    if let cell = tableView.cellForRow(at: indexPath) as? CodeChunkCell {
                        configureCell(cell, chunkIndex: indexPath.row)
                    }
                }
            }

            if let target = parent.targetLine, target != lastTargetLine, target >= 1 {
                lastTargetLine = target
                let lineIndex = min(max(0, target - 1), max(0, totalCodeRows - 1))
                let chunkIdx = lineIndex / Self.chunkSize
                if totalChunks > 0 && chunkIdx < totalChunks {
                    DispatchQueue.main.async {
                        tableView.scrollToRow(at: IndexPath(row: chunkIdx, section: 0), at: .middle, animated: true)
                    }
                }
            } else if parent.targetLine == nil {
                lastTargetLine = nil
            }
        }

        func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
            totalChunks + (hasTruncatedFooter ? 1 : 0)
        }

        func tableView(_ tableView: UITableView, heightForRowAt indexPath: IndexPath) -> CGFloat {
            if indexPath.row >= totalChunks {
                return 40
            }
            if !parent.wrapLines {
                let startIdx = indexPath.row * Self.chunkSize
                let endIdx = min(totalCodeRows, startIdx + Self.chunkSize)
                let linesInChunk = max(1, endIdx - startIdx)
                return CGFloat(linesInChunk) * Self.fixedLineHeight
            }
            return UITableView.automaticDimension
        }

        func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
            guard let cell = tableView.dequeueReusableCell(withIdentifier: CodeChunkCell.reuseId, for: indexPath) as? CodeChunkCell else {
                return UITableViewCell()
            }
            configureCell(cell, chunkIndex: indexPath.row)
            return cell
        }

        @objc func handleTap(_ recognizer: UITapGestureRecognizer) {
            guard recognizer.state == .ended, let tableView = attachedTableView else { return }
            let point = recognizer.location(in: tableView)
            guard let indexPath = tableView.indexPathForRow(at: point),
                  indexPath.row < totalChunks else {
                return
            }
            let startIdx = indexPath.row * Self.chunkSize
            let endIdx = min(totalCodeRows, startIdx + Self.chunkSize)
            let linesInChunk = max(1, endIdx - startIdx)
            let cellRect = tableView.rectForRow(at: indexPath)
            let localY = max(0, point.y - cellRect.minY)
            let lineOffset: Int
            if !parent.wrapLines {
                lineOffset = min(linesInChunk - 1, max(0, Int(localY / Self.fixedLineHeight)))
            } else {
                let fraction = localY / max(1, cellRect.height)
                lineOffset = min(linesInChunk - 1, max(0, Int(fraction * CGFloat(linesInChunk))))
            }
            let actualRow = startIdx + lineOffset
            guard actualRow < totalCodeRows else { return }
            if !parent.file.highlightedLines.isEmpty {
                let line = parent.file.highlightedLines[actualRow]
                parent.onQuoteLine(line.lineNumber, line.tokens.map(\.text).joined())
            } else {
                parent.onQuoteLine(UInt32(actualRow + 1), rawLines[actualRow])
            }
        }

        private func configureCell(_ cell: CodeChunkCell, chunkIndex: Int) {
            if chunkIndex >= totalChunks {
                cell.configureFooter(
                    text: "File truncated for preview (\(parent.file.sizeBytes) bytes total)",
                    mutedColor: UIColor(parent.palette.fgMuted)
                )
                return
            }
            if parent.wrapLines {
                let attributed = attributedChunk(chunkIndex: chunkIndex)
                cell.configureWrapped(attributedChunk: attributed)
            } else {
                let data = unwrappedChunk(chunkIndex: chunkIndex)
                cell.configureUnwrapped(data: data, mutedColor: UIColor(parent.palette.fgMuted))
            }
        }

        private func unwrappedChunk(chunkIndex: Int) -> UnwrappedChunkData {
            let key = NSNumber(value: chunkIndex)
            if let hit = unwrappedChunkCache.object(forKey: key) {
                return hit
            }

            let startIdx = chunkIndex * Self.chunkSize
            let endIdx = min(totalCodeRows, startIdx + Self.chunkSize)
            let codeRegularFont = UIFont.monospacedSystemFont(ofSize: 12.5, weight: .regular)
            let codeBoldFont = UIFont.monospacedSystemFont(ofSize: 12.5, weight: .semibold)
            let gutterFont = UIFont.monospacedDigitSystemFont(ofSize: 11.5, weight: .regular)
            let primaryInk = UIColor(parent.palette.fgPrimary)
            let mutedInk = UIColor(parent.palette.fgMuted)
            let primaryAccentInk = UIColor(parent.palette.primary)
            let isDark = parent.palette.isDark

            var lines: [UnwrappedCodeLine] = []
            lines.reserveCapacity(max(0, endIdx - startIdx))
            var firstLineNo: UInt32 = 0
            var lastLineNo: UInt32 = 0

            for row in startIdx..<endIdx {
                let lineNo: UInt32 = !parent.file.highlightedLines.isEmpty
                    ? parent.file.highlightedLines[row].lineNumber
                    : UInt32(row + 1)
                if row == startIdx { firstLineNo = lineNo }
                lastLineNo = lineNo

                let isMatch = parent.matchingLines.contains(lineNo)
                let isTarget = parent.targetLine.map { UInt32($0) == lineNo } ?? false
                let rowBgColor: UIColor? = isTarget
                    ? UIColor(parent.palette.primary.opacity(0.16))
                    : (isMatch ? UIColor(parent.palette.warning.opacity(0.22)) : nil)

                let out = NSMutableAttributedString()
                let gutterText = String(format: "%5d  ", Int(lineNo))
                out.append(NSAttributedString(string: gutterText, attributes: [
                    .font: gutterFont,
                    .foregroundColor: isTarget ? primaryAccentInk : mutedInk
                ]))

                if !parent.file.highlightedLines.isEmpty {
                    let tokens = parent.file.highlightedLines[row].tokens
                    if tokens.isEmpty {
                        out.append(NSAttributedString(string: " ", attributes: [
                            .font: codeRegularFont,
                            .foregroundColor: primaryInk
                        ]))
                    } else {
                        for tok in tokens {
                            let color = Self.readableInkColor(hex: tok.fgHex, isDark: isDark, fallback: primaryInk)
                            var font = tok.bold ? codeBoldFont : codeRegularFont
                            if tok.italic, let desc = font.fontDescriptor.withSymbolicTraits(
                                tok.bold ? [.traitItalic, .traitBold] : [.traitItalic]
                            ) {
                                font = UIFont(descriptor: desc, size: 12.5)
                            }
                            out.append(NSAttributedString(string: tok.text, attributes: [
                                .font: font,
                                .foregroundColor: color
                            ]))
                        }
                    }
                } else {
                    let text = rawLines[row]
                    out.append(NSAttributedString(string: text.isEmpty ? " " : text, attributes: [
                        .font: codeRegularFont,
                        .foregroundColor: primaryInk
                    ]))
                }

                lines.append(UnwrappedCodeLine(attributedText: out, backgroundColor: rowBgColor))
            }

            let data = UnwrappedChunkData(
                lines: lines,
                lineHeight: Self.fixedLineHeight,
                accessibilityLabel: "Lines \(firstLineNo) to \(lastLineNo)"
            )
            unwrappedChunkCache.setObject(data, forKey: key)
            return data
        }

        private func attributedChunk(chunkIndex: Int) -> NSAttributedString {
            let key = NSNumber(value: chunkIndex)
            if let hit = chunkAttrCache.object(forKey: key) {
                return hit
            }

            let startIdx = chunkIndex * Self.chunkSize
            let endIdx = min(totalCodeRows, startIdx + Self.chunkSize)

            let out = NSMutableAttributedString()
            let codeRegularFont = UIFont.monospacedSystemFont(ofSize: 12.5, weight: .regular)
            let codeBoldFont = UIFont.monospacedSystemFont(ofSize: 12.5, weight: .semibold)
            let gutterFont = UIFont.monospacedDigitSystemFont(ofSize: 11.5, weight: .regular)
            let primaryInk = UIColor(parent.palette.fgPrimary)
            let mutedInk = UIColor(parent.palette.fgMuted)
            let primaryAccentInk = UIColor(parent.palette.primary)
            let isDark = parent.palette.isDark

            let paraStyle = NSMutableParagraphStyle()
            paraStyle.minimumLineHeight = Self.fixedLineHeight
            paraStyle.maximumLineHeight = Self.fixedLineHeight
            paraStyle.lineBreakMode = parent.wrapLines ? .byWordWrapping : .byTruncatingTail

            for row in startIdx..<endIdx {
                if row > startIdx {
                    out.append(NSAttributedString(string: "\n", attributes: [
                        .font: codeRegularFont,
                        .paragraphStyle: paraStyle
                    ]))
                }

                let lineNo: UInt32 = !parent.file.highlightedLines.isEmpty
                    ? parent.file.highlightedLines[row].lineNumber
                    : UInt32(row + 1)

                let isMatch = parent.matchingLines.contains(lineNo)
                let isTarget = parent.targetLine.map { UInt32($0) == lineNo } ?? false
                let rowBgColor: UIColor? = isTarget
                    ? UIColor(parent.palette.primary.opacity(0.16))
                    : (isMatch ? UIColor(parent.palette.warning.opacity(0.22)) : nil)

                let gutterText = String(format: "%5d  ", Int(lineNo))
                var gutterAttrs: [NSAttributedString.Key: Any] = [
                    .font: gutterFont,
                    .foregroundColor: isTarget ? primaryAccentInk : mutedInk,
                    .paragraphStyle: paraStyle
                ]
                if let rowBgColor {
                    gutterAttrs[.backgroundColor] = rowBgColor
                }
                out.append(NSAttributedString(string: gutterText, attributes: gutterAttrs))

                if !parent.file.highlightedLines.isEmpty {
                    let tokens = parent.file.highlightedLines[row].tokens
                    if tokens.isEmpty {
                        var attrs: [NSAttributedString.Key: Any] = [
                            .font: codeRegularFont,
                            .foregroundColor: primaryInk,
                            .paragraphStyle: paraStyle
                        ]
                        if let rowBgColor { attrs[.backgroundColor] = rowBgColor }
                        out.append(NSAttributedString(string: " ", attributes: attrs))
                    } else {
                        for tok in tokens {
                            let color = Self.readableInkColor(hex: tok.fgHex, isDark: isDark, fallback: primaryInk)
                            var font = tok.bold ? codeBoldFont : codeRegularFont
                            if tok.italic, let desc = font.fontDescriptor.withSymbolicTraits(
                                tok.bold ? [.traitItalic, .traitBold] : [.traitItalic]
                            ) {
                                font = UIFont(descriptor: desc, size: 12.5)
                            }
                            var attrs: [NSAttributedString.Key: Any] = [
                                .font: font,
                                .foregroundColor: color,
                                .paragraphStyle: paraStyle
                            ]
                            if let rowBgColor { attrs[.backgroundColor] = rowBgColor }
                            out.append(NSAttributedString(string: tok.text, attributes: attrs))
                        }
                    }
                } else {
                    let text = rawLines[row]
                    var attrs: [NSAttributedString.Key: Any] = [
                        .font: codeRegularFont,
                        .foregroundColor: primaryInk,
                        .paragraphStyle: paraStyle
                    ]
                    if let rowBgColor { attrs[.backgroundColor] = rowBgColor }
                    out.append(NSAttributedString(string: text.isEmpty ? " " : text, attributes: attrs))
                }
            }

            chunkAttrCache.setObject(out, forKey: key)
            return out
        }

        private static func readableInkColor(hex: String, isDark: Bool, fallback: UIColor) -> UIColor {
            let clean = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
            guard clean.count == 6, let rgb = UInt64(clean, radix: 16) else {
                return fallback
            }
            let r = CGFloat((rgb >> 16) & 0xFF) / 255.0
            let g = CGFloat((rgb >> 8) & 0xFF) / 255.0
            let b = CGFloat(rgb & 0xFF) / 255.0
            if !isDark {
                let luminance = 0.299 * r + 0.587 * g + 0.114 * b
                if luminance > 0.70 {
                    return fallback
                }
                if luminance > 0.52 {
                    return UIColor(red: r * 0.62, green: g * 0.62, blue: b * 0.62, alpha: 1.0)
                }
            }
            return UIColor(red: r, green: g, blue: b, alpha: 1.0)
        }
    }

    private final class UnwrappedCodeLine {
        let attributedText: NSAttributedString
        let backgroundColor: UIColor?

        init(attributedText: NSAttributedString, backgroundColor: UIColor?) {
            self.attributedText = attributedText
            self.backgroundColor = backgroundColor
        }
    }

    private final class UnwrappedChunkData {
        let lines: [UnwrappedCodeLine]
        let lineHeight: CGFloat
        let accessibilityLabel: String

        init(lines: [UnwrappedCodeLine], lineHeight: CGFloat, accessibilityLabel: String) {
            self.lines = lines
            self.lineHeight = lineHeight
            self.accessibilityLabel = accessibilityLabel
        }
    }

    private final class UnwrappedChunkView: UIView {
        private var lines: [UnwrappedCodeLine] = []
        private var lineHeight: CGFloat = 18.0
        private var mutedColor: UIColor = .secondaryLabel

        override init(frame: CGRect) {
            super.init(frame: frame)
            isOpaque = false
            backgroundColor = .clear
            contentMode = .redraw
            isAccessibilityElement = true
        }

        required init?(coder: NSCoder) {
            fatalError("init(coder:) has not been implemented")
        }

        func configure(data: UnwrappedChunkData, mutedColor: UIColor) {
            self.lines = data.lines
            self.lineHeight = data.lineHeight
            self.mutedColor = mutedColor
            self.accessibilityLabel = data.accessibilityLabel
            setNeedsDisplay()
        }

        override func draw(_ rect: CGRect) {
            guard !lines.isEmpty, let ctx = UIGraphicsGetCurrentContext() else { return }
            let minRow = max(0, Int(floor(rect.minY / lineHeight)))
            let maxRow = min(lines.count - 1, Int(ceil(rect.maxY / lineHeight)))
            guard minRow <= maxRow else { return }

            let horizontalPad: CGFloat = 10
            let maxWidth = max(40, bounds.width - horizontalPad * 2)
            let ellipsis = NSAttributedString(
                string: "…",
                attributes: [
                    .font: UIFont.monospacedSystemFont(ofSize: 12.5, weight: .regular),
                    .foregroundColor: mutedColor
                ]
            )
            let truncToken = CTLineCreateWithAttributedString(ellipsis)

            for idx in minRow...maxRow {
                let item = lines[idx]
                let rowY = CGFloat(idx) * lineHeight
                if let bg = item.backgroundColor {
                    bg.setFill()
                    UIRectFill(CGRect(x: 0, y: rowY, width: bounds.width, height: lineHeight))
                }
                let fullLine = CTLineCreateWithAttributedString(item.attributedText)
                let width = CTLineGetTypographicBounds(fullLine, nil, nil, nil)
                let drawnLine: CTLine
                if width > Double(maxWidth),
                   let truncated = CTLineCreateTruncatedLine(fullLine, Double(maxWidth), .end, truncToken) {
                    drawnLine = truncated
                } else {
                    drawnLine = fullLine
                }

                ctx.saveGState()
                ctx.translateBy(x: horizontalPad, y: rowY + 13.5)
                ctx.scaleBy(x: 1, y: -1)
                ctx.textPosition = .zero
                CTLineDraw(drawnLine, ctx)
                ctx.restoreGState()
            }
        }
    }

    private final class CodeChunkCell: UITableViewCell {
        static let reuseId = "OrbitCodeChunkCell"
        private let contentLabel = UILabel()
        private let unwrappedView = UnwrappedChunkView()

        override init(style: UITableViewCell.CellStyle, reuseIdentifier: String?) {
            super.init(style: style, reuseIdentifier: reuseIdentifier)
            selectionStyle = .none
            backgroundColor = .clear
            contentView.backgroundColor = .clear

            contentLabel.translatesAutoresizingMaskIntoConstraints = false
            contentLabel.numberOfLines = 0
            contentLabel.lineBreakMode = .byWordWrapping
            contentLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

            unwrappedView.translatesAutoresizingMaskIntoConstraints = false

            contentView.addSubview(contentLabel)
            contentView.addSubview(unwrappedView)

            NSLayoutConstraint.activate([
                contentLabel.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 10),
                contentLabel.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -10),
                contentLabel.topAnchor.constraint(equalTo: contentView.topAnchor),
                contentLabel.bottomAnchor.constraint(equalTo: contentView.bottomAnchor),

                unwrappedView.leadingAnchor.constraint(equalTo: contentView.leadingAnchor),
                unwrappedView.trailingAnchor.constraint(equalTo: contentView.trailingAnchor),
                unwrappedView.topAnchor.constraint(equalTo: contentView.topAnchor),
                unwrappedView.bottomAnchor.constraint(equalTo: contentView.bottomAnchor)
            ])
        }

        required init?(coder: NSCoder) {
            fatalError("init(coder:) has not been implemented")
        }

        func configureWrapped(attributedChunk: NSAttributedString) {
            unwrappedView.isHidden = true
            contentLabel.isHidden = false
            contentLabel.numberOfLines = 0
            contentLabel.lineBreakMode = .byWordWrapping
            contentLabel.attributedText = attributedChunk
            contentView.backgroundColor = .clear
        }

        func configureUnwrapped(data: UnwrappedChunkData, mutedColor: UIColor) {
            contentLabel.isHidden = true
            contentLabel.attributedText = nil
            unwrappedView.isHidden = false
            unwrappedView.configure(data: data, mutedColor: mutedColor)
            contentView.backgroundColor = .clear
        }

        func configureFooter(text: String, mutedColor: UIColor) {
            unwrappedView.isHidden = true
            contentLabel.isHidden = false
            contentLabel.numberOfLines = 0
            contentLabel.lineBreakMode = .byWordWrapping
            contentLabel.attributedText = NSAttributedString(
                string: text,
                attributes: [
                    .font: UIFont.systemFont(ofSize: 11.5, weight: .regular),
                    .foregroundColor: mutedColor
                ]
            )
            contentView.backgroundColor = .clear
        }
    }
}

private struct OrbitFileSvgPreviewCanvas: UIViewRepresentable {
    let svg: String
    let isDark: Bool

    func makeUIView(context: Context) -> WKWebView {
        let cfg = WKWebViewConfiguration()
        cfg.defaultWebpagePreferences.allowsContentJavaScript = false
        let web = WKWebView(frame: .zero, configuration: cfg)
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.backgroundColor = .clear
        web.scrollView.bouncesZoom = true
        web.scrollView.minimumZoomScale = 0.5
        web.scrollView.maximumZoomScale = 5.0
        load(web)
        return web
    }

    func updateUIView(_ uiView: WKWebView, context: Context) {
        load(uiView)
    }

    private func load(_ web: WKWebView) {
        let fgHex = isDark ? "#F4F4F5" : "#18181B"
        let html = """
        <!doctype html>
        <html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=5.0, user-scalable=yes">
        <style>html,body{margin:0;padding:12px;background:transparent;color:\(fgHex);display:flex;align-items:center;justify-content:center;}svg{max-width:100%;height:auto;}</style>
        </head><body>\(svg)</body></html>
        """
        web.loadHTMLString(html, baseURL: nil)
    }
}
