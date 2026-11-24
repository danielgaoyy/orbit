import SwiftUI
import UIKit

// MARK: - Bounded Caches for Attributed Strings, Text Heights & Syntax Colors

private enum MarkdownRenderCache {
    static let attributedStrings: NSCache<NSString, NSAttributedString> = {
        let cache = NSCache<NSString, NSAttributedString>()
        cache.countLimit = 256
        return cache
    }()

    static let textMeasurements: NSCache<NSString, NSValue> = {
        let cache = NSCache<NSString, NSValue>()
        cache.countLimit = 512
        return cache
    }()

    private static let colorLock = NSLock()
    private static var hexColorMap: [String: Color] = [:]

    static func attributedString(forKey key: String, build: () -> NSAttributedString) -> NSAttributedString {
        let nsKey = key as NSString
        if let hit = attributedStrings.object(forKey: nsKey) {
            return hit
        }
        let built = build()
        attributedStrings.setObject(built, forKey: nsKey)
        return built
    }

    static func cachedColor(hex: String, isDark: Bool, fallback: Color) -> Color {
        let key = "\(isDark ? "d" : "l"):\(hex)"
        colorLock.lock()
        if let hit = hexColorMap[key] {
            colorLock.unlock()
            return hit
        }
        colorLock.unlock()

        let resolved: Color
        if !isDark {
            let clean = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
            var rgb: UInt64 = 0
            Scanner(string: clean).scanHexInt64(&rgb)
            let r = Double((rgb >> 16) & 0xFF) / 255.0
            let g = Double((rgb >> 8) & 0xFF) / 255.0
            let b = Double(rgb & 0xFF) / 255.0
            let luminance = 0.299 * r + 0.587 * g + 0.114 * b
            resolved = luminance > 0.78 ? fallback : Color(red: r * 0.72, green: g * 0.72, blue: b * 0.72)
        } else {
            resolved = Color(hex: hex)
        }

        colorLock.lock()
        if hexColorMap.count > 256 {
            hexColorMap.removeAll(keepingCapacity: true)
        }
        hexColorMap[key] = resolved
        colorLock.unlock()
        return resolved
    }
}

// MARK: - Native Auto-Sizing Selectable TextView (Genuine iOS Range Selection)

struct OrbitSelectableTextView: UIViewRepresentable {
    let attributedText: NSAttributedString
    var stretchesToProposedWidth: Bool = true
    var maxLines: Int = 0
    var isDarkAppearance: Bool = false
    var linkColor: UIColor = .systemBlue
    var workspaceCwd: String? = nil
    var onQuoteSelection: ((String) -> Void)? = nil
    var onOpenFileLink: ((OrbitPathLogic.FileTarget) -> Void)? = nil

    func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    func makeUIView(context: Context) -> UITextView {
        let view = UITextView()
        view.isEditable = false
        view.isSelectable = true
        view.isScrollEnabled = false
        view.showsVerticalScrollIndicator = false
        view.showsHorizontalScrollIndicator = false
        view.backgroundColor = .clear
        view.textContainerInset = .zero
        view.textContainer.lineFragmentPadding = 0
        view.dataDetectorTypes = []
        view.tintColor = .systemBlue
        view.adjustsFontForContentSizeCategory = false
        view.delegate = context.coordinator
        view.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        view.setContentHuggingPriority(stretchesToProposedWidth ? .defaultLow : .required, for: .horizontal)
        syncContentIfNeeded(on: view, coordinator: context.coordinator)
        return view
    }

    func updateUIView(_ uiView: UITextView, context: Context) {
        context.coordinator.parent = self
        uiView.setContentHuggingPriority(stretchesToProposedWidth ? .defaultLow : .required, for: .horizontal)
        syncContentIfNeeded(on: uiView, coordinator: context.coordinator)
    }

    @available(iOS 16.0, *)
    func sizeThatFits(_ proposal: ProposedViewSize, uiView: UITextView, context: Context) -> CGSize? {
        context.coordinator.parent = self

        let widthBucket = proposal.width.map { Int(max(1, $0).rounded()) } ?? -1
        let measureKey = "\(attributedText.length):\(attributedText.string.hashValue):\(isDarkAppearance ? 1 : 0):\(maxLines):\(stretchesToProposedWidth ? 1 : 0):\(widthBucket)" as NSString
        if let cachedVal = MarkdownRenderCache.textMeasurements.object(forKey: measureKey) {
            return cachedVal.cgSizeValue
        }

        let result: CGSize
        if let proposedWidth = proposal.width {
            let safeProposed = max(1, proposedWidth)
            if stretchesToProposedWidth {
                let measured = attributedText.boundingRect(
                    with: CGSize(width: safeProposed, height: .greatestFiniteMagnitude),
                    options: [.usesLineFragmentOrigin, .usesFontLeading],
                    context: nil
                )
                result = CGSize(width: safeProposed, height: max(1, ceil(measured.height) + 4))
            } else {
                let measured = attributedText.boundingRect(
                    with: CGSize(width: safeProposed, height: .greatestFiniteMagnitude),
                    options: [.usesLineFragmentOrigin, .usesFontLeading],
                    context: nil
                )
                let tightWidth = min(safeProposed, max(1, ceil(measured.width) + 2))
                let tightMeasured = attributedText.boundingRect(
                    with: CGSize(width: tightWidth, height: .greatestFiniteMagnitude),
                    options: [.usesLineFragmentOrigin, .usesFontLeading],
                    context: nil
                )
                result = CGSize(width: tightWidth, height: max(1, ceil(tightMeasured.height) + 4))
            }
        } else {
            let measured = attributedText.boundingRect(
                with: CGSize(width: CGFloat.greatestFiniteMagnitude, height: .greatestFiniteMagnitude),
                options: [.usesLineFragmentOrigin, .usesFontLeading],
                context: nil
            )
            result = CGSize(width: max(1, ceil(measured.width) + 2), height: max(1, ceil(measured.height) + 4))
        }
        MarkdownRenderCache.textMeasurements.setObject(NSValue(cgSize: result), forKey: measureKey)
        return result
    }

    private func syncContentIfNeeded(on uiView: UITextView, coordinator: Coordinator) {
        if coordinator.lastIsDark != isDarkAppearance {
            uiView.overrideUserInterfaceStyle = isDarkAppearance ? .dark : .light
            coordinator.lastIsDark = isDarkAppearance
        }
        if coordinator.lastMaxLines != maxLines {
            uiView.textContainer.maximumNumberOfLines = maxLines
            uiView.textContainer.lineBreakMode = maxLines > 0 ? .byTruncatingTail : .byWordWrapping
            coordinator.lastMaxLines = maxLines
        }
        if coordinator.lastLinkColor != linkColor {
            uiView.linkTextAttributes = [
                .foregroundColor: linkColor,
                .underlineStyle: NSUnderlineStyle.single.rawValue
            ]
            coordinator.lastLinkColor = linkColor
        }
        let unchanged = coordinator.lastAttributedText.map { prev in
            prev === attributedText || prev.isEqual(to: attributedText)
        } ?? false
        if !unchanged {
            uiView.attributedText = attributedText
            coordinator.lastAttributedText = attributedText
            uiView.invalidateIntrinsicContentSize()
        }
    }

    final class Coordinator: NSObject, UITextViewDelegate {
        var parent: OrbitSelectableTextView
        var lastAttributedText: NSAttributedString?
        var lastMaxLines: Int?
        var lastIsDark: Bool?
        var lastLinkColor: UIColor?

        init(parent: OrbitSelectableTextView) {
            self.parent = parent
        }

        func textView(
            _ textView: UITextView,
            editMenuForTextIn range: NSRange,
            suggestedActions: [UIMenuElement]
        ) -> UIMenu? {
            guard let onQuote = parent.onQuoteSelection,
                  range.length > 0,
                  let text = textView.attributedText?.string,
                  let swiftRange = Range(range, in: text) else {
                return nil
            }
            let selected = String(text[swiftRange]).trimmingCharacters(in: .whitespacesAndNewlines)
            guard !selected.isEmpty else { return nil }

            let quoteAction = UIAction(
                title: "Quote",
                image: UIImage(systemName: "text.quote")
            ) { _ in
                onQuote(selected)
                textView.selectedTextRange = nil
            }
            var actions = suggestedActions
            actions.insert(quoteAction, at: min(1, actions.count))
            return UIMenu(children: actions)
        }

        func textView(
            _ textView: UITextView,
            shouldInteractWith URL: URL,
            in characterRange: NSRange,
            interaction: UITextItemInteraction
        ) -> Bool {
            switch OrbitMarkdownLinkLogic.parseLink(URL.absoluteString, workspaceCwd: parent.workspaceCwd) {
            case let .filePath(target):
                parent.onOpenFileLink?(target)
                return false
            case let .externalWeb(externalUrl):
                UIApplication.shared.open(externalUrl)
                return false
            case let .mailto(mailtoUrl):
                UIApplication.shared.open(mailtoUrl)
                return false
            case .anchor, .unknown:
                return true
            }
        }
    }
}

// MARK: - Markdown Models & RichMarkdownView

private struct MarkdownTableModel: Equatable {
    let headers: [[FfiInlineSpan]]
    let rows: [[[FfiInlineSpan]]]

    var columnCount: Int {
        max(headers.count, rows.map(\.count).max() ?? 0)
    }
}

private enum MarkdownRenderSegment {
    case prose([FfiRichBlock])
    case checkListItem(checked: Bool, depth: UInt8, spans: [FfiInlineSpan])
    case blockQuote([FfiInlineSpan])
    case horizontalRule
    case codeBlock(language: String, rawCode: String, lines: [FfiHighlightedLine])
    case table(MarkdownTableModel)
}

struct RichMarkdownView: View {
    let blocks: [FfiRichBlock]
    var fallbackText: String = ""
    var isStreaming: Bool = false
    var workspaceCwd: String? = nil
    var onQuoteSelection: ((String) -> Void)? = nil
    var onOpenFileLink: ((OrbitPathLogic.FileTarget) -> Void)? = nil

    @Environment(\.orbitPalette) private var palette

    private var segments: [MarkdownRenderSegment] {
        guard !blocks.isEmpty else {
            return parseFallbackSegments(from: fallbackText)
        }
        var out: [MarkdownRenderSegment] = []
        var proseBuffer: [FfiRichBlock] = []

        func flushProseBuffer() {
            guard !proseBuffer.isEmpty else { return }
            out.append(.prose(proseBuffer))
            proseBuffer.removeAll()
        }

        var idx = 0
        while idx < blocks.count {
            if case let .table(_, headers, rows) = blocks[idx] {
                flushProseBuffer()
                let model = MarkdownTableModel(
                    headers: headers.map(\.spans),
                    rows: rows.map { $0.cells.map(\.spans) }
                )
                out.append(.table(model))
                idx += 1
                continue
            }
            if let tableRowCells = extractDelimitedTableCells(from: blocks[idx]) {
                var rawRows: [[[FfiInlineSpan]]] = [tableRowCells]
                var next = idx + 1
                while next < blocks.count, let moreCells = extractDelimitedTableCells(from: blocks[next]) {
                    rawRows.append(moreCells)
                    next += 1
                }
                if let model = normalizeTableModel(from: rawRows) {
                    flushProseBuffer()
                    out.append(.table(model))
                    idx = next
                    continue
                }
            }

            let block = blocks[idx]
            switch block {
            case .heading:
                flushProseBuffer()
                proseBuffer.append(block)
                flushProseBuffer()
            case .paragraph:
                proseBuffer.append(block)
                if proseBuffer.count >= 3 {
                    flushProseBuffer()
                }
            case let .listItem(_, _, checked, depth, spans):
                if let checked = checked {
                    flushProseBuffer()
                    out.append(.checkListItem(checked: checked, depth: depth, spans: spans))
                } else {
                    proseBuffer.append(block)
                    if proseBuffer.count >= 4 {
                        flushProseBuffer()
                    }
                }
            case let .blockQuote(spans):
                flushProseBuffer()
                out.append(.blockQuote(spans))
            case .horizontalRule:
                flushProseBuffer()
                out.append(.horizontalRule)
            case let .codeBlock(language, rawCode, lines):
                flushProseBuffer()
                out.append(.codeBlock(language: language, rawCode: rawCode, lines: lines))
            case let .mathBlock(tex, _):
                flushProseBuffer()
                out.append(.codeBlock(language: "math", rawCode: tex, lines: []))
            case .table:
                break
            }
            idx += 1
        }
        flushProseBuffer()
        return out
    }

    var body: some View {
        if segments.isEmpty {
            if !fallbackText.isEmpty {
                OrbitSelectableTextView(
                    attributedText: buildPlainNSAttributed(fallbackText, color: UIColor(palette.fgPrimary)),
                    stretchesToProposedWidth: true,
                    isDarkAppearance: palette.isDark,
                    linkColor: UIColor(palette.primary),
                    onQuoteSelection: onQuoteSelection
                )
            }
        } else {
            VStack(alignment: .leading, spacing: 10) {
                ForEach(Array(segments.enumerated()), id: \.offset) { _, segment in
                    segmentView(segment)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func segmentView(_ segment: MarkdownRenderSegment) -> some View {
        switch segment {
        case let .prose(proseBlocks):
            OrbitSelectableTextView(
                attributedText: buildMergedProseNSAttributed(proseBlocks),
                stretchesToProposedWidth: true,
                isDarkAppearance: palette.isDark,
                linkColor: UIColor(palette.primary),
                workspaceCwd: workspaceCwd,
                onQuoteSelection: onQuoteSelection,
                onOpenFileLink: onOpenFileLink
            )

        case let .checkListItem(checked, depth, spans):
            HStack(alignment: .firstTextBaseline, spacing: OrbitSpacing.sm) {
                Image(systemName: checked ? "checkmark.square.fill" : "square")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(checked ? palette.primary : palette.fgMuted)

                OrbitSelectableTextView(
                    attributedText: buildInlineNSAttributed(
                        spans,
                        baseColor: UIColor(palette.fgPrimary),
                        paragraphStyle: bodyParagraphStyle(spacingAfter: 0)
                    ),
                    stretchesToProposedWidth: true,
                    isDarkAppearance: palette.isDark,
                    linkColor: UIColor(palette.primary),
                    workspaceCwd: workspaceCwd,
                    onQuoteSelection: onQuoteSelection,
                    onOpenFileLink: onOpenFileLink
                )
            }
            .padding(.leading, CGFloat(min(depth, 6)) * 16)

        case let .blockQuote(spans):
            HStack(alignment: .top, spacing: OrbitSpacing.md) {
                RoundedRectangle(cornerRadius: 1.5, style: .continuous)
                    .fill(palette.fgMuted.opacity(0.35))
                    .frame(width: 2.5)

                OrbitSelectableTextView(
                    attributedText: buildInlineNSAttributed(
                        spans,
                        baseColor: UIColor(palette.fgSecondary),
                        paragraphStyle: bodyParagraphStyle(spacingAfter: 0)
                    ),
                    stretchesToProposedWidth: true,
                    isDarkAppearance: palette.isDark,
                    linkColor: UIColor(palette.primary),
                    workspaceCwd: workspaceCwd,
                    onQuoteSelection: onQuoteSelection,
                    onOpenFileLink: onOpenFileLink
                )
            }
            .padding(.vertical, 2)

        case .horizontalRule:
            OrbitHairline()
                .padding(.vertical, OrbitSpacing.xs)

        case let .codeBlock(language, rawCode, lines):
            if let diagramKind = detectDiagramKind(language: language, source: rawCode) {
                DiagramBlockView(
                    kind: diagramKind,
                    language: language,
                    rawCode: rawCode,
                    lines: lines,
                    isStreaming: isStreaming
                )
            } else {
                CodeBlockView(language: language, rawCode: rawCode, lines: lines)
            }

        case let .table(tableModel):
            MarkdownTableView(
                model: tableModel,
                buildAttributed: { spans, baseColor, isHeader in
                    buildTableCellAttributed(spans, baseColor: baseColor, isHeader: isHeader)
                }
            )
        }
    }

    // MARK: - Table Cell Extraction & Normalization

    private func extractDelimitedTableCells(from block: FfiRichBlock) -> [[FfiInlineSpan]]? {
        guard case let .paragraph(spans) = block else { return nil }
        let fullText = spans.map(\.text).joined()
        let lines = fullText.components(separatedBy: .newlines)
        if OrbitMarkdownLinkLogic.isASCIIBoxTableFalsePositive(lines) {
            return nil
        }
        guard fullText.contains("│") else { return nil }

        var cells: [[FfiInlineSpan]] = []
        var currentCell: [FfiInlineSpan] = []

        for span in spans {
            if span.text.contains("│") {
                let pieces = span.text.components(separatedBy: "│")
                for (pieceIdx, rawPiece) in pieces.enumerated() {
                    let trimmedPiece = pieceIdx == 0
                        ? rawPiece.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression)
                        : (pieceIdx == pieces.count - 1
                            ? rawPiece.replacingOccurrences(of: "^\\s+", with: "", options: .regularExpression)
                            : rawPiece.trimmingCharacters(in: .whitespaces))
                    if !trimmedPiece.isEmpty {
                        currentCell.append(
                            FfiInlineSpan(
                                text: trimmedPiece,
                                bold: span.bold,
                                italic: span.italic,
                                strikethrough: span.strikethrough,
                                inlineCode: span.inlineCode,
                                linkUrl: span.linkUrl,
                                mathMl: span.mathMl
                            )
                        )
                    }
                    if pieceIdx < pieces.count - 1 {
                        cells.append(currentCell)
                        currentCell = []
                    }
                }
            } else {
                currentCell.append(span)
            }
        }

        let trailingText = currentCell.map(\.text).joined().trimmingCharacters(in: .whitespacesAndNewlines)
        if !trailingText.isEmpty {
            cells.append(currentCell)
        }

        return cells.count >= 2 ? cells : nil
    }

    private func normalizeTableModel(from rawRows: [[[FfiInlineSpan]]]) -> MarkdownTableModel? {
        guard let first = rawRows.first, !first.isEmpty else { return nil }

        if rawRows.count >= 2 {
            let secondCount = rawRows[1].count
            if secondCount >= 2 && first.count == secondCount * 2 {
                let headers = Array(first.prefix(secondCount))
                let firstDataRow = Array(first.dropFirst(secondCount))
                let body = [firstDataRow] + Array(rawRows.dropFirst())
                return MarkdownTableModel(headers: headers, rows: body)
            }
            return MarkdownTableModel(headers: first, rows: Array(rawRows.dropFirst()))
        } else if first.count >= 4 && first.count % 2 == 0 {
            let colCount = first.count / 2
            let headers = Array(first.prefix(colCount))
            let singleRow = Array(first.dropFirst(colCount))
            return MarkdownTableModel(headers: headers, rows: [singleRow])
        }
        return nil
    }

    private func parseFallbackSegments(from raw: String) -> [MarkdownRenderSegment] {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return [] }
        let lines = raw.components(separatedBy: .newlines)
        var segments: [MarkdownRenderSegment] = []
        var textBuffer: [String] = []
        var i = 0

        func flushTextBuffer() {
            let joined = textBuffer.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
            textBuffer.removeAll()
            guard !joined.isEmpty else { return }
            segments.append(
                .prose([
                    .paragraph(spans: [
                        FfiInlineSpan(
                            text: joined,
                            bold: false,
                            italic: false,
                            strikethrough: false,
                            inlineCode: false,
                            linkUrl: nil,
                            mathMl: nil
                        )
                    ])
                ])
            )
        }

        while i < lines.count {
            if i + 1 < lines.count,
               lines[i].contains("|"),
               !OrbitMarkdownLinkLogic.isASCIIBoxTableFalsePositive([lines[i], lines[i + 1]]),
               isPipeSeparatorRow(lines[i + 1]) {
                flushTextBuffer()
                let headers = splitPipeRow(lines[i])
                i += 2
                var rows: [[[FfiInlineSpan]]] = []
                while i < lines.count && lines[i].contains("|") {
                    let rowCells = splitPipeRow(lines[i])
                    if !rowCells.isEmpty {
                        rows.append(rowCells)
                    }
                    i += 1
                }
                if !headers.isEmpty {
                    segments.append(.table(MarkdownTableModel(headers: headers, rows: rows)))
                    continue
                }
            }
            textBuffer.append(lines[i])
            i += 1
        }
        flushTextBuffer()
        return segments
    }

    private func isPipeSeparatorRow(_ line: String) -> Bool {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard trimmed.contains("|") && trimmed.contains("-") else { return false }
        let allowed = CharacterSet(charactersIn: "|-: ")
        return trimmed.unicodeScalars.allSatisfy { allowed.contains($0) }
    }

    private func splitPipeRow(_ line: String) -> [[FfiInlineSpan]] {
        var clean = line.trimmingCharacters(in: .whitespaces)
        if clean.hasPrefix("|") { clean.removeFirst() }
        if clean.hasSuffix("|") { clean.removeLast() }
        return clean.components(separatedBy: "|").map { cell in
            [
                FfiInlineSpan(
                    text: cell.trimmingCharacters(in: .whitespaces),
                    bold: false,
                    italic: false,
                    strikethrough: false,
                    inlineCode: false,
                    linkUrl: nil,
                    mathMl: nil
                )
            ]
        }
    }

    // MARK: - NSAttributedString Builders for Rich Prose Range Selection

    private var inlineCodeFill: Color {
        palette.isDark ? Color.white.opacity(0.08) : Color.black.opacity(0.048)
    }

    private func uiFont(size: CGFloat, weight: UIFont.Weight, italic: Bool = false, monospaced: Bool = false) -> UIFont {
        let base = monospaced
            ? UIFont.monospacedSystemFont(ofSize: size, weight: weight)
            : UIFont.systemFont(ofSize: size, weight: weight)
        guard italic else { return base }
        if let desc = base.fontDescriptor.withSymbolicTraits(base.fontDescriptor.symbolicTraits.union(.traitItalic)) {
            return UIFont(descriptor: desc, size: size)
        }
        return base
    }

    private func bodyParagraphStyle(spacingAfter: CGFloat) -> NSParagraphStyle {
        let p = NSMutableParagraphStyle()
        p.lineSpacing = OrbitTypography.bodyLineSpacing
        p.paragraphSpacing = spacingAfter
        return p
    }

    private func buildPlainNSAttributed(_ text: String, color: UIColor) -> NSAttributedString {
        let key = "plain:\(palette.isDark ? 1 : 0):\(text.hashValue):\(text.utf8.count)"
        return MarkdownRenderCache.attributedString(forKey: key) {
            NSAttributedString(
                string: text,
                attributes: [
                    .font: uiFont(size: 15, weight: .regular),
                    .foregroundColor: color,
                    .paragraphStyle: bodyParagraphStyle(spacingAfter: 0)
                ]
            )
        }
    }

    private func buildMergedProseNSAttributed(_ proseBlocks: [FfiRichBlock]) -> NSAttributedString {
        if isStreaming {
            return buildMergedProseNSAttributedUncached(proseBlocks)
        }
        var hasher = Hasher()
        hasher.combine(palette.isDark)
        hasher.combine(proseBlocks)
        let key = "prose:\(proseBlocks.count):\(hasher.finalize())"
        return MarkdownRenderCache.attributedString(forKey: key) {
            buildMergedProseNSAttributedUncached(proseBlocks)
        }
    }

    private func buildMergedProseNSAttributedUncached(_ proseBlocks: [FfiRichBlock]) -> NSAttributedString {
        let out = NSMutableAttributedString()
        let uiPrimary = UIColor(palette.fgPrimary)
        let uiSecondary = UIColor(palette.fgSecondary)

        for (idx, block) in proseBlocks.enumerated() {
            let isFirst = idx == 0
            let isLast = idx == proseBlocks.count - 1
            if !isFirst {
                out.append(NSAttributedString(string: "\n"))
            }

            switch block {
            case let .heading(level, spans):
                let p = NSMutableParagraphStyle()
                p.lineSpacing = OrbitTypography.subheadLineSpacing
                p.paragraphSpacingBefore = isFirst ? 0 : (level <= 2 ? 12 : 8)
                p.paragraphSpacing = isLast ? 0 : 6
                let headingSize: CGFloat = level == 1 ? 18 : (level == 2 ? 16 : 15)
                out.append(buildInlineNSAttributed(
                    spans,
                    baseColor: uiPrimary,
                    baseSize: headingSize,
                    isHeading: true,
                    paragraphStyle: p
                ))

            case let .paragraph(spans):
                let p = bodyParagraphStyle(spacingAfter: isLast ? 0 : 10)
                out.append(buildInlineNSAttributed(
                    spans,
                    baseColor: uiPrimary,
                    baseSize: 15,
                    isHeading: false,
                    paragraphStyle: p
                ))

            case let .listItem(ordered, index, _, depth, spans):
                let nestedIndent = CGFloat(min(depth, 6)) * 16
                let p = NSMutableParagraphStyle()
                p.firstLineHeadIndent = 4 + nestedIndent
                p.headIndent = 20 + nestedIndent
                p.tabStops = [NSTextTab(textAlignment: .left, location: 20 + nestedIndent)]
                p.lineSpacing = OrbitTypography.bodyLineSpacing
                p.paragraphSpacing = isLast ? 0 : 6

                let bulletGlyph: String = switch depth % 3 {
                case 1: "◦"
                case 2: "▪"
                default: "•"
                }
                let marker = ordered ? "\(index).\t" : "\(bulletGlyph)\t"
                let markerFont = ordered
                    ? uiFont(size: 13, weight: .medium)
                    : uiFont(size: 15, weight: .semibold)
                out.append(NSAttributedString(
                    string: marker,
                    attributes: [
                        .font: markerFont,
                        .foregroundColor: uiSecondary,
                        .paragraphStyle: p
                    ]
                ))
                out.append(buildInlineNSAttributed(
                    spans,
                    baseColor: uiPrimary,
                    baseSize: 15,
                    isHeading: false,
                    paragraphStyle: p
                ))

            default:
                break
            }
        }
        return out
    }

    private func buildInlineNSAttributed(
        _ spans: [FfiInlineSpan],
        baseColor: UIColor,
        baseSize: CGFloat = 15,
        isHeading: Bool = false,
        paragraphStyle: NSParagraphStyle
    ) -> NSAttributedString {
        let out = NSMutableAttributedString()
        let uiPrimary = UIColor(palette.fgPrimary)
        let uiCodeFill = UIColor(inlineCodeFill)
        let uiLink = UIColor(palette.primary)

        for span in spans {
            var attrs: [NSAttributedString.Key: Any] = [
                .paragraphStyle: paragraphStyle
            ]
            if span.inlineCode {
                attrs[.font] = uiFont(size: 13, weight: .regular, monospaced: true)
                attrs[.foregroundColor] = uiPrimary
                attrs[.backgroundColor] = uiCodeFill
            } else if isHeading {
                attrs[.font] = uiFont(size: baseSize, weight: .semibold, italic: span.italic)
                attrs[.foregroundColor] = baseColor
            } else {
                attrs[.font] = uiFont(
                    size: baseSize,
                    weight: span.bold ? .semibold : .regular,
                    italic: span.italic
                )
                attrs[.foregroundColor] = baseColor
            }

            if span.strikethrough {
                attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
            }
            if let rawUrl = span.linkUrl, let url = URL(string: rawUrl) {
                attrs[.link] = url
                attrs[.foregroundColor] = uiLink
                attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue
            }
            out.append(NSAttributedString(string: span.text, attributes: attrs))
        }
        return out
    }

    // MARK: - SwiftUI AttributedString Builder for Tables

    private func buildTableCellAttributed(
        _ spans: [FfiInlineSpan],
        baseColor: Color,
        isHeader: Bool
    ) -> AttributedString {
        var result = AttributedString()
        for span in spans {
            var part = AttributedString(span.text)
            if span.inlineCode {
                part.font = .system(size: 13, weight: .regular, design: .monospaced)
                part.foregroundColor = palette.fgPrimary
                part.backgroundColor = inlineCodeFill
            } else if isHeader {
                part.font = OrbitTypography.subhead(.semibold)
                part.foregroundColor = baseColor
            } else if span.bold && span.italic {
                part.font = OrbitTypography.subheadItalic(.semibold)
                part.foregroundColor = baseColor
            } else if span.bold {
                part.font = OrbitTypography.subhead(.semibold)
                part.foregroundColor = baseColor
            } else if span.italic {
                part.font = OrbitTypography.subheadItalic(.regular)
                part.foregroundColor = baseColor
            } else {
                part.font = OrbitTypography.subhead(.regular)
                part.foregroundColor = baseColor
            }

            if span.strikethrough {
                part.strikethroughStyle = .single
            }
            if let rawUrl = span.linkUrl, let url = URL(string: rawUrl) {
                part.link = url
                part.foregroundColor = palette.primary
                part.underlineStyle = .single
            }
            result.append(part)
        }
        return result
    }
}

// MARK: - Horizontal-Scrolling GFM Markdown Table View

private struct MarkdownTableView: View {
    let model: MarkdownTableModel
    let buildAttributed: ([FfiInlineSpan], Color, Bool) -> AttributedString

    @Environment(\.orbitPalette) private var palette

    private func cellSpans(in row: [[FfiInlineSpan]], at col: Int) -> [FfiInlineSpan] {
        guard col < row.count else { return [] }
        return row[col]
    }

    var body: some View {
        let colCount = max(1, model.columnCount)

        ScrollView(.horizontal, showsIndicators: true) {
            Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    ForEach(0..<colCount, id: \.self) { col in
                        HStack(spacing: 0) {
                            if col > 0 {
                                OrbitHairline(axis: .vertical)
                            }
                            Text(buildAttributed(cellSpans(in: model.headers, at: col), palette.fgPrimary, true))
                                .font(OrbitTypography.subhead(.semibold))
                                .foregroundStyle(palette.fgPrimary)
                                .padding(.horizontal, OrbitSpacing.md)
                                .padding(.vertical, OrbitSpacing.sm)
                                .fixedSize(horizontal: true, vertical: false)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
                .background(palette.subtleFill)

                ForEach(Array(model.rows.enumerated()), id: \.offset) { rowIdx, row in
                    OrbitHairline()
                        .gridCellUnsizedAxes(.horizontal)

                    GridRow {
                        ForEach(0..<colCount, id: \.self) { col in
                            HStack(spacing: 0) {
                                if col > 0 {
                                    OrbitHairline(axis: .vertical)
                                }
                                Text(buildAttributed(cellSpans(in: row, at: col), palette.fgPrimary, false))
                                    .font(OrbitTypography.subhead(.regular))
                                    .foregroundStyle(palette.fgPrimary)
                                    .padding(.horizontal, OrbitSpacing.md)
                                    .padding(.vertical, OrbitSpacing.sm)
                                    .fixedSize(horizontal: true, vertical: false)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                    }
                    .background(rowIdx % 2 == 1 ? palette.subtleFill.opacity(0.45) : Color.clear)
                }
            }
            .textSelection(.enabled)
        }
        .background(palette.code)
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
    }
}

// MARK: - Syntax-Highlighted Code Block with 1-Tap Copy, Line Count & Line Numbers Toggle

struct CodeBlockView: View {
    let language: String
    let rawCode: String
    let lines: [FfiHighlightedLine]

    @Environment(\.orbitPalette) private var palette
    @State private var copied = false
    @State private var showLineNumbers = false

    private var displayLanguage: String {
        let trimmed = language.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "code" : trimmed.lowercased()
    }

    private var fallbackLines: [String] {
        var split = rawCode.components(separatedBy: "\n")
        if split.count > 1 && split.last == "" {
            split.removeLast()
        }
        return split
    }

    private var effectiveLineCount: Int {
        lines.isEmpty ? fallbackLines.count : lines.count
    }

    private var lineNumberGutterWidth: CGFloat {
        effectiveLineCount >= 100 ? 28 : 20
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: OrbitSpacing.xs) {
                HStack(spacing: 6) {
                    Text(displayLanguage)
                        .font(OrbitTypography.code(.medium))
                        .foregroundStyle(palette.fgMuted)

                    if effectiveLineCount >= 8 {
                        Text("· \(effectiveLineCount) lines")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted.opacity(0.85))
                    }
                }

                Spacer()

                Button {
                    UIImpactFeedbackGenerator(style: .light).impactOccurred()
                    withAnimation(.easeOut(duration: 0.15)) {
                        showLineNumbers.toggle()
                    }
                } label: {
                    Text("#")
                        .font(OrbitTypography.code(.medium))
                        .foregroundStyle(showLineNumbers ? palette.primary : palette.fgMuted)
                        .frame(width: 24, height: 24)
                        .background(
                            Capsule()
                                .fill(showLineNumbers ? palette.primary.opacity(0.10) : Color.clear)
                        )
                        .frame(width: 32, height: 32)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(showLineNumbers ? "Hide Line Numbers" : "Show Line Numbers")

                Button {
                    copyCode()
                } label: {
                    Image(systemName: copied ? "checkmark" : "square.on.square")
                        .font(.system(size: 12.5, weight: .medium))
                        .foregroundStyle(copied ? palette.direct : palette.fgMuted)
                        .orbitSymbolReplaceTransition()
                        .frame(width: 34, height: 34)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(copied ? "Copied Code Block" : "Copy Code Block")
            }
            .padding(.leading, OrbitSpacing.md)
            .padding(.trailing, OrbitSpacing.xs)
            .frame(height: 34)
            .background(palette.isDark ? Color.white.opacity(0.025) : Color.black.opacity(0.022))

            OrbitHairline()

            ScrollView(.horizontal, showsIndicators: true) {
                if lines.isEmpty {
                    if showLineNumbers {
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(Array(fallbackLines.enumerated()), id: \.offset) { idx, rawLine in
                                HStack(alignment: .firstTextBaseline, spacing: OrbitSpacing.md) {
                                    Text("\(idx + 1)")
                                        .font(OrbitTypography.code(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                        .frame(minWidth: lineNumberGutterWidth, alignment: .trailing)

                                    Text(rawLine.isEmpty ? " " : rawLine)
                                        .font(OrbitTypography.code(.regular))
                                        .foregroundStyle(palette.fgPrimary)
                                        .textSelection(.enabled)
                                }
                            }
                        }
                        .padding(OrbitSpacing.md)
                    } else {
                        OrbitSelectableTextView(
                            attributedText: buildFallbackCodeNSAttributed(),
                            stretchesToProposedWidth: false,
                            isDarkAppearance: palette.isDark,
                            linkColor: UIColor(palette.primary)
                        )
                        .padding(OrbitSpacing.md)
                    }
                } else {
                    if showLineNumbers {
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(Array(lines.enumerated()), id: \.offset) { idx, line in
                                HStack(alignment: .firstTextBaseline, spacing: OrbitSpacing.md) {
                                    Text("\(idx + 1)")
                                        .font(OrbitTypography.code(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                        .frame(minWidth: lineNumberGutterWidth, alignment: .trailing)

                                    Text(buildHighlightedLine(line))
                                        .font(OrbitTypography.code(.regular))
                                        .textSelection(.enabled)
                                }
                            }
                        }
                        .padding(OrbitSpacing.md)
                    } else {
                        OrbitSelectableTextView(
                            attributedText: buildHighlightedCodeNSAttributed(),
                            stretchesToProposedWidth: false,
                            isDarkAppearance: palette.isDark,
                            linkColor: UIColor(palette.primary)
                        )
                        .padding(OrbitSpacing.md)
                    }
                }
            }
        }
        .background(palette.code)
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
    }

    private func copyCode() {
        UIPasteboard.general.string = rawCode
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        withAnimation(.easeOut(duration: 0.15)) {
            copied = true
        }
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 1_500_000_000)
            withAnimation(.easeIn(duration: 0.2)) {
                copied = false
            }
        }
    }

    private func codeUIFont(bold: Bool, italic: Bool) -> UIFont {
        let base = UIFont.monospacedSystemFont(ofSize: 12, weight: bold ? .semibold : .regular)
        guard italic,
              let desc = base.fontDescriptor.withSymbolicTraits(base.fontDescriptor.symbolicTraits.union(.traitItalic)) else {
            return base
        }
        return UIFont(descriptor: desc, size: 12)
    }

    private func codeParagraphStyle() -> NSParagraphStyle {
        let p = NSMutableParagraphStyle()
        p.lineSpacing = OrbitTypography.metaLineSpacing
        return p
    }

    private func buildFallbackCodeNSAttributed() -> NSAttributedString {
        let key = "codefb:\(palette.isDark ? 1 : 0):\(rawCode.hashValue):\(rawCode.utf8.count)"
        return MarkdownRenderCache.attributedString(forKey: key) {
            NSAttributedString(
                string: rawCode.hasSuffix("\n") ? String(rawCode.dropLast()) : rawCode,
                attributes: [
                    .font: codeUIFont(bold: false, italic: false),
                    .foregroundColor: UIColor(palette.fgPrimary),
                    .paragraphStyle: codeParagraphStyle()
                ]
            )
        }
    }

    private func buildHighlightedCodeNSAttributed() -> NSAttributedString {
        var hasher = Hasher()
        hasher.combine(palette.isDark)
        hasher.combine(lines)
        let key = "codehl:\(lines.count):\(hasher.finalize())"
        return MarkdownRenderCache.attributedString(forKey: key) {
            let out = NSMutableAttributedString()
            let p = codeParagraphStyle()
            let uiPrimary = UIColor(palette.fgPrimary)

            for (idx, line) in lines.enumerated() {
                if idx > 0 {
                    out.append(NSAttributedString(string: "\n", attributes: [.font: codeUIFont(bold: false, italic: false), .paragraphStyle: p]))
                }
                for token in line.tokens {
                    let fg = token.fgHex.isEmpty ? uiPrimary : UIColor(adjustedTokenColor(hex: token.fgHex))
                    out.append(NSAttributedString(
                        string: token.text,
                        attributes: [
                            .font: codeUIFont(bold: token.bold, italic: token.italic),
                            .foregroundColor: fg,
                            .paragraphStyle: p
                        ]
                    ))
                }
            }
            return out
        }
    }

    private func buildHighlightedLine(_ line: FfiHighlightedLine) -> AttributedString {
        var out = AttributedString()
        for token in line.tokens {
            var part = AttributedString(token.text)
            if token.bold && token.italic {
                part.font = OrbitTypography.codeItalic(.semibold)
            } else if token.bold {
                part.font = OrbitTypography.code(.semibold)
            } else if token.italic {
                part.font = OrbitTypography.codeItalic(.regular)
            } else {
                part.font = OrbitTypography.code(.regular)
            }
            if !token.fgHex.isEmpty {
                part.foregroundColor = adjustedTokenColor(hex: token.fgHex)
            } else {
                part.foregroundColor = palette.fgPrimary
            }
            out.append(part)
        }
        return out
    }

    private func adjustedTokenColor(hex: String) -> Color {
        MarkdownRenderCache.cachedColor(hex: hex, isDark: palette.isDark, fallback: palette.fgPrimary)
    }
}
