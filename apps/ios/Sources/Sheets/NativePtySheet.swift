import SwiftUI
import UIKit

struct NativePtySheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var commandBuffer = ""
    @State private var isCtrlArmed = false
    @State private var isAltArmed = false
    @State private var hasInitializedPty = false
    @State private var copiedSnapshot = false
    @FocusState private var isInputFocused: Bool

    private let terminalSurfaceBg = Color(hex: "#0A0B0E")
    private let terminalDefaultFg = Color(hex: "#E6E8EE")

    private var terminals: [FfiTerminalSummary] {
        store.currentHostDetail?.terminals ?? []
    }

    private var activeTerminalId: String? {
        store.currentHostDetail?.activeTerminalId ?? terminals.first?.terminalId
    }

    var body: some View {
        GeometryReader { rootGeo in
            let isCompactHeight = rootGeo.size.height > 0 && rootGeo.size.height < 430

            VStack(spacing: 0) {
                sheetHeader(isCompactHeight: isCompactHeight)
                if !isCompactHeight {
                    terminalTabsBar
                }
                OrbitHairline()
                terminalViewport
                OrbitHairline()
                developerAccessoryKeybar(isCompactHeight: isCompactHeight)
                commandInputBar(isCompactHeight: isCompactHeight)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(palette.canvas.ignoresSafeArea())
    }

    // MARK: - Header

    private func sheetHeader(isCompactHeight: Bool) -> some View {
        HStack(spacing: OrbitSpacing.xs) {
            Text("Terminal")
                .font(isCompactHeight ? OrbitTypography.body(.semibold) : OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)

            if let screen = store.terminalScreen {
                Text("\(screen.cols)×\(screen.rows)")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgMuted)
                    .padding(.horizontal, OrbitSpacing.sm)
                    .padding(.vertical, 2)
                    .background(palette.subtleFill)
                    .clipShape(Capsule())
                    .fixedSize(horizontal: true, vertical: false)
            }

            if isCompactHeight && !terminals.isEmpty {
                inlineCompactTabsStrip
            } else {
                Spacer(minLength: OrbitSpacing.xs)
            }

            HStack(spacing: 0) {
                Button {
                    if store.copyTerminalOutput() {
                        withAnimation(.easeOut(duration: 0.15)) {
                            copiedSnapshot = true
                        }
                        Task { @MainActor in
                            try? await Task.sleep(nanoseconds: 1_500_000_000)
                            withAnimation(.easeIn(duration: 0.2)) {
                                copiedSnapshot = false
                            }
                        }
                    }
                } label: {
                    Image(systemName: copiedSnapshot ? "checkmark" : "doc.on.doc")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(copiedSnapshot ? palette.direct : palette.fgSecondary)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Copy Terminal Output")

                Button {
                    store.quoteTerminalOutputToComposer()
                    dismiss()
                } label: {
                    Image(systemName: "text.quote")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.primary)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Quote Terminal Output into Composer")

                Button {
                    store.startNewTerminal()
                } label: {
                    Image(systemName: "plus")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.fgPrimary)
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("New Terminal")

                if let tid = activeTerminalId {
                    Button {
                        store.killTerminal(tid)
                    } label: {
                        Image(systemName: "trash")
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(palette.danger)
                            .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Kill Active Terminal")
                }

                Button {
                    dismiss()
                } label: {
                    Text("Done")
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                        .padding(.leading, OrbitSpacing.xs)
                        .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget, alignment: .trailing)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close Terminal")
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .frame(height: isCompactHeight ? OrbitSpacing.hitTarget : 52)
    }

    private var inlineCompactTabsStrip: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: OrbitSpacing.xs) {
                ForEach(Array(terminals.enumerated()), id: \.element.terminalId) { idx, term in
                    let isSelected = term.terminalId == activeTerminalId
                    Button {
                        store.attachTerminal(term.terminalId)
                    } label: {
                        HStack(spacing: OrbitSpacing.xs) {
                            Circle()
                                .fill(isSelected ? palette.direct : palette.fgMuted)
                                .frame(width: 6, height: 6)

                            Text(term.title.isEmpty ? "tty-\(idx + 1)" : term.title)
                                .font(OrbitTypography.code(isSelected ? .semibold : .regular))
                                .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgSecondary)
                                .lineLimit(1)
                        }
                        .padding(.horizontal, OrbitSpacing.sm + 2)
                        .padding(.vertical, OrbitSpacing.xs)
                        .background(isSelected ? palette.elevated : palette.subtleFill)
                        .clipShape(Capsule())
                        .overlay(
                            Capsule()
                                .strokeBorder(
                                    isSelected ? palette.primary.opacity(0.55) : palette.hairline,
                                    lineWidth: 0.5
                                )
                        )
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, OrbitSpacing.xs)
        }
    }

    // MARK: - Session Switcher Tabs

    @ViewBuilder
    private var terminalTabsBar: some View {
        if !terminals.isEmpty {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: OrbitSpacing.xs) {
                    ForEach(Array(terminals.enumerated()), id: \.element.terminalId) { idx, term in
                        let isSelected = term.terminalId == activeTerminalId
                        Button {
                            store.attachTerminal(term.terminalId)
                        } label: {
                            HStack(spacing: OrbitSpacing.xs) {
                                Circle()
                                    .fill(isSelected ? palette.direct : palette.fgMuted)
                                    .frame(width: 6, height: 6)

                                Text(term.title.isEmpty ? "tty-\(idx + 1)" : term.title)
                                    .font(OrbitTypography.code(isSelected ? .semibold : .regular))
                                    .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgSecondary)
                                    .lineLimit(1)

                                Text("(\(OrbitFormatters.shortPath(term.cwd)))")
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                    .lineLimit(1)
                            }
                            .padding(.horizontal, OrbitSpacing.md)
                            .padding(.vertical, 6)
                            .background(isSelected ? palette.elevated : palette.subtleFill)
                            .clipShape(Capsule())
                            .overlay(
                                Capsule()
                                    .strokeBorder(
                                        isSelected ? palette.primary.opacity(0.55) : palette.hairline,
                                        lineWidth: 0.5
                                    )
                            )
                            .frame(minHeight: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.bottom, OrbitSpacing.xs)
            }
        }
    }

    // MARK: - GeometryReader Exact Cell-Sized PTY Viewport

    private var terminalViewport: some View {
        GeometryReader { geo in
            let insetPad: CGFloat = OrbitSpacing.sm * 2
            let availW = max(120, geo.size.width - insetPad)
            let availH = max(80, geo.size.height - insetPad)

            ScrollView([.vertical, .horizontal], showsIndicators: false) {
                VStack(alignment: .leading, spacing: OrbitTypography.metaLineSpacing) {
                    if let screen = store.terminalScreen, !screen.rowsData.isEmpty {
                        ForEach(screen.rowsData, id: \.rowIndex) { row in
                            Text(
                                renderTerminalRow(
                                    row: row,
                                    cursorRow: screen.cursorRow,
                                    cursorCol: screen.cursorCol
                                )
                            )
                            .font(OrbitTypography.code(.regular))
                            .fixedSize(horizontal: true, vertical: false)
                        }
                    } else {
                        Text("Connecting to remote PTY in \(OrbitFormatters.compactPath(store.activeCwd))…")
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(terminalDefaultFg.opacity(0.6))
                    }
                }
                .padding(OrbitSpacing.sm)
                .frame(minWidth: geo.size.width, minHeight: geo.size.height, alignment: .topLeading)
            }
            .background(terminalSurfaceBg)
            .contentShape(Rectangle())
            .onTapGesture {
                isInputFocused = true
            }
            .onAppear {
                let grid = computeGridSize(width: availW, height: availH)
                if !hasInitializedPty {
                    hasInitializedPty = true
                    store.ensureTerminalReady(cols: grid.cols, rows: grid.rows)
                } else {
                    store.resizeTerminalIfNeeded(cols: grid.cols, rows: grid.rows)
                }
            }
            .onChange(of: geo.size) { newSize in
                let w = max(120, newSize.width - insetPad)
                let h = max(80, newSize.height - insetPad)
                let grid = computeGridSize(width: w, height: h)
                store.resizeTerminalIfNeeded(cols: grid.cols, rows: grid.rows)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func computeGridSize(width: CGFloat, height: CGFloat) -> (cols: UInt16, rows: UInt16) {
        let cell = OrbitTypography.terminalCellMetrics()
        let cols = UInt16(max(24, min(220, Int(floor(width / cell.width)))))
        let rows = UInt16(max(6, min(100, Int(floor(height / cell.height)))))
        return (cols, rows)
    }

    private func renderTerminalRow(
        row: FfiTerminalRow,
        cursorRow: UInt16,
        cursorCol: UInt16
    ) -> AttributedString {
        var out = AttributedString()
        var currentCol: Int = 0
        let isCursorLine = (row.rowIndex == cursorRow)

        for span in row.spans {
            let spanLen = span.text.count
            let cursorOffsetInSpan: Int? = {
                guard isCursorLine else { return nil }
                let c = Int(cursorCol)
                if c >= currentCol && c < currentCol + spanLen {
                    return c - currentCol
                }
                return nil
            }()

            if let cIdx = cursorOffsetInSpan, !span.inverse {
                let prefixText = String(span.text.prefix(cIdx))
                let restAfterPrefix = span.text.dropFirst(cIdx)
                let cursorChar = String(restAfterPrefix.prefix(1))
                let suffixText = String(restAfterPrefix.dropFirst(1))

                if !prefixText.isEmpty {
                    out.append(styledSpanFragment(text: prefixText, span: span, forceInverse: false))
                }
                if !cursorChar.isEmpty {
                    out.append(styledSpanFragment(text: cursorChar, span: span, forceInverse: true))
                }
                if !suffixText.isEmpty {
                    out.append(styledSpanFragment(text: suffixText, span: span, forceInverse: false))
                }
            } else {
                out.append(styledSpanFragment(text: span.text, span: span, forceInverse: false))
            }

            currentCol += spanLen
        }

        // Append block cursor at end of line if cursorCol is beyond trailing span
        if isCursorLine && Int(cursorCol) >= currentCol {
            let padCount = max(0, Int(cursorCol) - currentCol)
            if padCount > 0 {
                var pad = AttributedString(String(repeating: " ", count: padCount))
                pad.font = OrbitTypography.code(.regular)
                out.append(pad)
            }
            var cursorBlock = AttributedString(" ")
            cursorBlock.font = OrbitTypography.code(.regular)
            cursorBlock.backgroundColor = terminalDefaultFg
            cursorBlock.foregroundColor = terminalSurfaceBg
            out.append(cursorBlock)
        }

        return out
    }

    private func styledSpanFragment(
        text: String,
        span: FfiTerminalSpan,
        forceInverse: Bool
    ) -> AttributedString {
        var part = AttributedString(text)
        part.font = span.bold ? OrbitTypography.code(.semibold) : OrbitTypography.code(.regular)

        let rawFg = span.fgHex.isEmpty ? terminalDefaultFg : Color(hex: span.fgHex)
        let rawBg = span.bgHex.flatMap { $0.isEmpty ? nil : Color(hex: $0) }
        let inverted = span.inverse || forceInverse

        if inverted {
            part.foregroundColor = rawBg ?? terminalSurfaceBg
            part.backgroundColor = rawFg
        } else {
            part.foregroundColor = rawFg
            if let bg = rawBg {
                part.backgroundColor = bg
            }
        }

        if span.underline {
            part.underlineStyle = .single
        }

        return part
    }

    // MARK: - Developer Accessory Keybar

    private func developerAccessoryKeybar(isCompactHeight: Bool) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: OrbitSpacing.xs) {
                accessoryKey("ESC") { sendRawKey("\u{001B}") }
                accessoryKey("TAB") { sendRawKey("\t") }

                stickyModifierKey("CTRL", isArmed: isCtrlArmed) {
                    store.hapticLight()
                    isCtrlArmed.toggle()
                }
                stickyModifierKey("ALT", isArmed: isAltArmed) {
                    store.hapticLight()
                    isAltArmed.toggle()
                }

                accessoryKey("^C") { sendRawKey("\u{0003}") }
                accessoryKey("^D") { sendRawKey("\u{0004}") }
                accessoryKey("^L") { sendRawKey("\u{000C}") }
                accessoryKey("↑") { sendRawKey("\u{001B}[A") }
                accessoryKey("↓") { sendRawKey("\u{001B}[B") }
                accessoryKey("←") { sendRawKey("\u{001B}[D") }
                accessoryKey("→") { sendRawKey("\u{001B}[C") }
                accessoryKey("|") { insertOrSendSymbol("|") }
                accessoryKey("/") { insertOrSendSymbol("/") }
                accessoryKey("-") { insertOrSendSymbol("-") }
                accessoryKey("~") { insertOrSendSymbol("~") }
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.vertical, isCompactHeight ? 0 : OrbitSpacing.xs)
        }
        .background(palette.drawer)
    }

    private func accessoryKey(_ label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label)
                .font(OrbitTypography.code(.semibold))
                .foregroundStyle(palette.fgPrimary)
                .padding(.horizontal, OrbitSpacing.sm + 2)
                .frame(minWidth: 38, minHeight: 34)
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )
                .frame(minHeight: OrbitSpacing.hitTarget)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func stickyModifierKey(_ label: String, isArmed: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label)
                .font(OrbitTypography.code(.semibold))
                .foregroundStyle(isArmed ? Color.white : palette.fgPrimary)
                .padding(.horizontal, OrbitSpacing.sm + 2)
                .frame(minWidth: 44, minHeight: 34)
                .background(isArmed ? palette.primary : palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(isArmed ? palette.primary : palette.hairline, lineWidth: 0.5)
                )
                .frame(minHeight: OrbitSpacing.hitTarget)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    // MARK: - Command & Keystroke Input Bar

    private func commandInputBar(isCompactHeight: Bool) -> some View {
        HStack(spacing: OrbitSpacing.xs) {
            Text("$")
                .font(OrbitTypography.code(.semibold))
                .foregroundStyle(palette.primary)
                .padding(.leading, OrbitSpacing.sm)

            TextField(
                isCtrlArmed ? "Press key for CTRL+…" : (isAltArmed ? "Press key for ALT+…" : "Send command or keys…"),
                text: $commandBuffer
            )
            .font(OrbitTypography.code(.regular))
            .foregroundStyle(palette.fgPrimary)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .focused($isInputFocused)
            .onChange(of: commandBuffer) { newVal in
                handleLiveModifierIfArmed(newVal)
            }
            .onSubmit {
                submitCommandLine()
            }

            // Backspace key for interactive TUI control
            Button {
                store.hapticLight()
                if !commandBuffer.isEmpty {
                    commandBuffer.removeLast()
                } else {
                    store.sendTerminalKeys("\u{007F}")
                }
            } label: {
                Image(systemName: "delete.left")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Backspace")

            // Return / Enter key
            Button {
                submitCommandLine()
            } label: {
                Image(systemName: "return")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(Color.white)
                    .frame(width: 34, height: 34)
                    .background(palette.primary)
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Enter")
        }
        .padding(.horizontal, OrbitSpacing.sm)
        .padding(.vertical, isCompactHeight ? 0 : OrbitSpacing.xs)
        .background(palette.elevated)
    }

    private func handleLiveModifierIfArmed(_ text: String) {
        guard (isCtrlArmed || isAltArmed), let firstChar = text.first else { return }
        if isCtrlArmed {
            if let ascii = firstChar.asciiValue {
                let ctrlByte = ascii & 0x1F
                store.sendTerminalBytes(Data([ctrlByte]))
            } else {
                store.sendTerminalKeys(String(firstChar))
            }
            isCtrlArmed = false
        } else if isAltArmed {
            store.sendTerminalKeys("\u{001B}" + String(firstChar))
            isAltArmed = false
        }
        store.hapticLight()
        commandBuffer = ""
    }

    private func sendRawKey(_ seq: String) {
        store.hapticLight()
        if isAltArmed {
            store.sendTerminalKeys("\u{001B}" + seq)
            isAltArmed = false
        } else {
            store.sendTerminalKeys(seq)
        }
    }

    private func insertOrSendSymbol(_ sym: String) {
        store.hapticLight()
        if isInputFocused {
            commandBuffer.append(sym)
        } else {
            store.sendTerminalKeys(sym)
        }
    }

    private func submitCommandLine() {
        store.hapticLight()
        let line = commandBuffer
        commandBuffer = ""
        store.sendTerminalKeys(line + "\r")
    }
}
