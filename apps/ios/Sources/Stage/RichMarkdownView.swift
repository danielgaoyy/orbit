import SwiftUI
import UIKit

private struct MarkdownTableModel: Equatable {
    let headers: [[FfiInlineSpan]]
    let rows: [[[FfiInlineSpan]]]

    var columnCount: Int {
        max(headers.count, rows.map(\.count).max() ?? 0)
    }
}

private enum MarkdownRenderSegment {
    case block(FfiRichBlock)
    case table(MarkdownTableModel)
}

struct RichMarkdownView: View {
    let blocks: [FfiRichBlock]
    var fallbackText: String = ""

    @Environment(\.orbitPalette) private var palette

    private var segments: [MarkdownRenderSegment] {
        guard !blocks.isEmpty else {
            return parseFallbackSegments(from: fallbackText)
        }
        var out: [MarkdownRenderSegment] = []
        var idx = 0
        while idx < blocks.count {
            if case let .table(_, headers, rows) = blocks[idx] {
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
                    out.append(.table(model))
                    idx = next
                    continue
                }
            }
            out.append(.block(blocks[idx]))
            idx += 1
        }
        return out
    }

    var body: some View {
        if segments.isEmpty {
            if !fallbackText.isEmpty {
                Text(fallbackText)
                    .orbitBodyStyle()
                    .foregroundStyle(palette.fgPrimary)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        } else {
            VStack(alignment: .leading, spacing: OrbitSpacing.md) {
                ForEach(Array(segments.enumerated()), id: \.offset) { _, segment in
                    switch segment {
                    case let .block(block):
                        blockView(block)
                    case let .table(tableModel):
                        MarkdownTableView(
                            model: tableModel,
                            buildAttributed: { spans, baseColor, isHeader in
                                buildTableCellAttributed(spans, baseColor: baseColor, isHeader: isHeader)
                            }
                        )
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func blockView(_ block: FfiRichBlock) -> some View {
        switch block {
        case let .heading(level, spans):
            Text(buildInlineAttributed(spans, baseColor: palette.fgPrimary, isHeading: true, headingLevel: level))
                .font(level <= 2 ? OrbitTypography.title : OrbitTypography.body(.semibold))
                .lineSpacing(OrbitTypography.subheadLineSpacing)
                .foregroundStyle(palette.fgPrimary)
                .padding(.top, OrbitSpacing.xs)
                .textSelection(.enabled)

        case let .paragraph(spans):
            Text(buildInlineAttributed(spans, baseColor: palette.fgPrimary))
                .orbitBodyStyle()
                .foregroundStyle(palette.fgPrimary)
                .textSelection(.enabled)

        case let .listItem(ordered, index, checked, spans):
            HStack(alignment: .firstTextBaseline, spacing: OrbitSpacing.sm) {
                if let checked = checked {
                    Image(systemName: checked ? "checkmark.square.fill" : "square")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(checked ? palette.primary : palette.fgMuted)
                } else if ordered {
                    Text("\(index).")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgSecondary)
                        .frame(minWidth: 16, alignment: .trailing)
                } else {
                    Text("•")
                        .font(OrbitTypography.body(.semibold))
                        .foregroundStyle(palette.fgSecondary)
                        .frame(minWidth: 12, alignment: .center)
                }

                Text(buildInlineAttributed(spans, baseColor: palette.fgPrimary))
                    .orbitBodyStyle()
                    .foregroundStyle(palette.fgPrimary)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

        case let .blockQuote(spans):
            HStack(alignment: .top, spacing: OrbitSpacing.md) {
                RoundedRectangle(cornerRadius: 1)
                    .fill(palette.primary.opacity(0.5))
                    .frame(width: 2)

                Text(buildInlineAttributed(spans, baseColor: palette.fgSecondary))
                    .orbitBodyStyle()
                    .foregroundStyle(palette.fgSecondary)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.vertical, OrbitSpacing.xs)

        case .horizontalRule:
            OrbitHairline()
                .padding(.vertical, OrbitSpacing.xs)

        case let .codeBlock(language, rawCode, lines):
            CodeBlockView(language: language, rawCode: rawCode, lines: lines)

        case let .table(_, headers, rows):
            MarkdownTableView(
                model: MarkdownTableModel(
                    headers: headers.map(\.spans),
                    rows: rows.map { $0.cells.map(\.spans) }
                ),
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
                                linkUrl: span.linkUrl
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

        // In pulldown-cmark, TableHead does not emit TagEnd::TableRow, so row 0 may hold Header (N) + Row1 (N) = 2N cells
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
                .block(
                    .paragraph(spans: [
                        FfiInlineSpan(
                            text: joined,
                            bold: false,
                            italic: false,
                            strikethrough: false,
                            inlineCode: false,
                            linkUrl: nil
                        )
                    ])
                )
            )
        }

        while i < lines.count {
            if i + 1 < lines.count,
               lines[i].contains("|"),
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
                    linkUrl: nil
                )
            ]
        }
    }

    // MARK: - Inline Attributed String Builders

    private func buildTableCellAttributed(
        _ spans: [FfiInlineSpan],
        baseColor: Color,
        isHeader: Bool
    ) -> AttributedString {
        var result = AttributedString()
        for span in spans {
            var part = AttributedString(span.text)
            if span.inlineCode {
                part.font = OrbitTypography.code(.medium)
                part.foregroundColor = palette.fgPrimary
                part.backgroundColor = palette.subtleFill
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

    private func buildInlineAttributed(
        _ spans: [FfiInlineSpan],
        baseColor: Color,
        isHeading: Bool = false,
        headingLevel: UInt8 = 3
    ) -> AttributedString {
        var result = AttributedString()
        for span in spans {
            var part = AttributedString(span.text)
            if span.inlineCode {
                part.font = OrbitTypography.code(.medium)
                part.foregroundColor = palette.fgPrimary
                part.backgroundColor = palette.subtleFill
            } else if isHeading {
                part.font = headingLevel <= 2 ? OrbitTypography.title : OrbitTypography.body(.semibold)
                part.foregroundColor = baseColor
            } else if span.bold && span.italic {
                part.font = OrbitTypography.bodyItalic(.semibold)
                part.foregroundColor = baseColor
            } else if span.bold {
                part.font = OrbitTypography.body(.semibold)
                part.foregroundColor = baseColor
            } else if span.italic {
                part.font = OrbitTypography.bodyItalic(.regular)
                part.foregroundColor = baseColor
            } else {
                part.font = OrbitTypography.body(.regular)
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
                // Header Row
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

                // Body Rows
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
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
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
            // Header bar with language, line count (>= 8), # line-number toggle, and 44x44pt Copy hit area
            HStack(spacing: OrbitSpacing.xs) {
                HStack(spacing: OrbitSpacing.sm) {
                    Text(displayLanguage)
                        .font(OrbitTypography.code(.medium))
                        .foregroundStyle(palette.fgMuted)

                    if effectiveLineCount >= 8 {
                        Text("\(effectiveLineCount) lines")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
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
                        .font(OrbitTypography.code(.semibold))
                        .foregroundStyle(showLineNumbers ? palette.primary : palette.fgMuted)
                        .padding(.horizontal, OrbitSpacing.sm)
                        .padding(.vertical, 2)
                        .background(
                            Capsule()
                                .fill(showLineNumbers ? palette.primary.opacity(0.12) : Color.clear)
                        )
                        .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(showLineNumbers ? "Hide Line Numbers" : "Show Line Numbers")

                Button {
                    copyCode()
                } label: {
                    HStack(spacing: OrbitSpacing.xs) {
                        Image(systemName: copied ? "checkmark" : "doc.on.doc")
                            .font(OrbitTypography.iconMicro)
                        Text(copied ? "Copied" : "Copy")
                            .font(OrbitTypography.meta(.medium))
                    }
                    .foregroundStyle(copied ? palette.direct : palette.fgSecondary)
                    .padding(.horizontal, OrbitSpacing.sm)
                    .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget, alignment: .trailing)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Copy Code Block")
            }
            .padding(.leading, OrbitSpacing.md)
            .padding(.trailing, OrbitSpacing.xs)
            .frame(height: 36)

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
                        Text(rawCode)
                            .orbitCodeStyle()
                            .foregroundStyle(palette.fgPrimary)
                            .textSelection(.enabled)
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
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(lines, id: \.lineNumber) { line in
                                Text(buildHighlightedLine(line))
                                    .font(OrbitTypography.code(.regular))
                            }
                        }
                        .textSelection(.enabled)
                        .padding(OrbitSpacing.md)
                    }
                }
            }
        }
        .background(palette.code)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
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
        // Syntect token hexes are tuned for dark code surfaces; if in Light theme and token is very light, fall back to readable contrast
        if !palette.isDark {
            let clean = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
            var rgb: UInt64 = 0
            Scanner(string: clean).scanHexInt64(&rgb)
            let r = Double((rgb >> 16) & 0xFF) / 255.0
            let g = Double((rgb >> 8) & 0xFF) / 255.0
            let b = Double(rgb & 0xFF) / 255.0
            let luminance = 0.299 * r + 0.587 * g + 0.114 * b
            if luminance > 0.78 {
                return palette.fgPrimary
            }
            return Color(red: r * 0.72, green: g * 0.72, blue: b * 0.72)
        }
        return Color(hex: hex)
    }
}
