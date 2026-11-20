import SwiftUI
import UIKit

/// Custom `UITextView` subclass that intercepts Backspace at caret offset 0
/// so an active inline slash directive (e.g. `/plan`) can be removed directly from the keyboard.
final class OrbitComposerUITextView: UITextView {
    var onBackspaceAtStart: (() -> Bool)?

    override func deleteBackward() {
        if selectedRange.location == 0,
           selectedRange.length == 0,
           markedTextRange == nil,
           onBackspaceAtStart?() == true {
            return
        }
        super.deleteBackward()
    }
}

/// Native multi-line `UITextView` wrapper supporting first-line inline indentation
/// (for active directives like `/plan`), reliable cursor offset tracking for `@` / `/`,
/// IME composition safety, and 1..6 line auto-sizing.
struct ComposerTextView: UIViewRepresentable {
    @Binding var text: String
    @Binding var isFocused: Bool
    @Binding var cursorOffset: Int
    let focusNonce: Int
    let firstLineHeadIndent: CGFloat
    let textColor: UIColor
    let tintColor: UIColor
    let onBackspaceAtStart: () -> Bool

    static let bodyFont: UIFont = .systemFont(ofSize: 15, weight: .regular)
    static let lineSpacing: CGFloat = 4

    static func inlineDirectiveIndent(for directive: OrbitSlashDirective?) -> CGFloat {
        guard let directive else { return 0 }
        let labelWidth = (directive.rawValue as NSString)
            .size(withAttributes: [.font: bodyFont])
            .width
        // 15pt icon + 5.5pt gap + directive text + 8pt breathing gap before text/caret
        return ceil(15 + 5.5 + labelWidth + 8)
    }

    static func makeTypingAttributes(indent: CGFloat, textColor: UIColor) -> [NSAttributedString.Key: Any] {
        let style = NSMutableParagraphStyle()
        style.lineSpacing = lineSpacing
        style.firstLineHeadIndent = indent
        return [
            .font: bodyFont,
            .foregroundColor: textColor,
            .paragraphStyle: style,
        ]
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeUIView(context: Context) -> OrbitComposerUITextView {
        let view = OrbitComposerUITextView()
        view.delegate = context.coordinator
        view.backgroundColor = .clear
        view.font = Self.bodyFont
        view.textColor = textColor
        view.tintColor = tintColor
        view.textContainerInset = UIEdgeInsets(top: 2, left: 0, bottom: 2, right: 0)
        view.textContainer.lineFragmentPadding = 0
        view.isScrollEnabled = false
        view.autocorrectionType = .default
        view.keyboardDismissMode = .interactive
        view.showsVerticalScrollIndicator = false
        view.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        view.accessibilityLabel = "Prompt input"

        let attrs = Self.makeTypingAttributes(indent: firstLineHeadIndent, textColor: textColor)
        view.typingAttributes = attrs
        if !text.isEmpty {
            view.attributedText = NSAttributedString(string: text, attributes: attrs)
        }
        view.onBackspaceAtStart = onBackspaceAtStart
        context.coordinator.lastAppliedIndent = firstLineHeadIndent
        context.coordinator.lastAppliedColor = textColor
        context.coordinator.lastFocusNonce = focusNonce
        return view
    }

    func updateUIView(_ uiView: OrbitComposerUITextView, context: Context) {
        context.coordinator.parent = self
        uiView.onBackspaceAtStart = onBackspaceAtStart
        uiView.tintColor = tintColor

        if focusNonce != context.coordinator.lastFocusNonce {
            context.coordinator.lastFocusNonce = focusNonce
            DispatchQueue.main.async {
                uiView.becomeFirstResponder()
            }
        } else if !isFocused && uiView.isFirstResponder {
            DispatchQueue.main.async {
                uiView.resignFirstResponder()
            }
        }

        // Never overwrite attributedText while Chinese/Japanese IME composition is active
        guard uiView.markedTextRange == nil else { return }

        let textChanged = uiView.text != text
        let indentChanged = abs(context.coordinator.lastAppliedIndent - firstLineHeadIndent) > 0.25
        let colorChanged = context.coordinator.lastAppliedColor != textColor

        guard textChanged || indentChanged || colorChanged else { return }

        let attrs = Self.makeTypingAttributes(indent: firstLineHeadIndent, textColor: textColor)
        context.coordinator.isUpdatingProgrammatically = true
        defer { context.coordinator.isUpdatingProgrammatically = false }

        let prevRange = uiView.selectedRange
        uiView.typingAttributes = attrs
        uiView.attributedText = NSAttributedString(string: text, attributes: attrs)
        uiView.typingAttributes = attrs

        let utf16Len = (text as NSString).length
        let targetLoc: Int
        if textChanged {
            targetLoc = Self.utf16Location(fromStringOffset: cursorOffset, in: text)
        } else {
            targetLoc = min(prevRange.location, utf16Len)
        }
        let targetLen = textChanged ? 0 : min(prevRange.length, max(0, utf16Len - targetLoc))
        uiView.selectedRange = NSRange(location: targetLoc, length: targetLen)

        context.coordinator.lastAppliedIndent = firstLineHeadIndent
        context.coordinator.lastAppliedColor = textColor
        uiView.invalidateIntrinsicContentSize()
    }

    func sizeThatFits(
        _ proposal: ProposedViewSize,
        uiView: OrbitComposerUITextView,
        context: Context
    ) -> CGSize? {
        let width = proposal.width ?? uiView.bounds.width
        guard width > 0 else { return nil }

        let wasScrollEnabled = uiView.isScrollEnabled
        if wasScrollEnabled {
            uiView.isScrollEnabled = false
        }
        let fitting = uiView.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude))
        let minHeight = ceil(Self.bodyFont.lineHeight) + 4
        let maxHeight = ceil((Self.bodyFont.lineHeight + Self.lineSpacing) * 6) + 6
        let needsScroll = fitting.height > maxHeight + 1
        if uiView.isScrollEnabled != needsScroll {
            uiView.isScrollEnabled = needsScroll
        }
        let clamped = min(max(fitting.height, minHeight), maxHeight)
        return CGSize(width: width, height: clamped)
    }

    private static func stringOffset(fromUtf16Location utf16Loc: Int, in string: String) -> Int {
        let clamped = min(max(0, utf16Loc), string.utf16.count)
        let utf16Idx = string.utf16.index(string.utf16.startIndex, offsetBy: clamped)
        if let valid = utf16Idx.samePosition(in: string) {
            return string.distance(from: string.startIndex, to: valid)
        }
        return string.count
    }

    private static func utf16Location(fromStringOffset offset: Int, in string: String) -> Int {
        let clamped = min(max(0, offset), string.count)
        let idx = string.index(string.startIndex, offsetBy: clamped)
        return string.utf16.distance(from: string.utf16.startIndex, to: idx)
    }

    final class Coordinator: NSObject, UITextViewDelegate {
        var parent: ComposerTextView
        var lastAppliedIndent: CGFloat = 0
        var lastAppliedColor: UIColor = .label
        var lastFocusNonce: Int = 0
        var isUpdatingProgrammatically: Bool = false

        init(_ parent: ComposerTextView) {
            self.parent = parent
        }

        func textViewDidBeginEditing(_ textView: UITextView) {
            DispatchQueue.main.async {
                if !self.parent.isFocused {
                    self.parent.isFocused = true
                }
            }
        }

        func textViewDidEndEditing(_ textView: UITextView) {
            DispatchQueue.main.async {
                if self.parent.isFocused {
                    self.parent.isFocused = false
                }
            }
        }

        func textViewDidChange(_ textView: UITextView) {
            guard !isUpdatingProgrammatically else { return }
            let nextText = textView.text ?? ""
            let nextOffset = ComposerTextView.stringOffset(
                fromUtf16Location: textView.selectedRange.location,
                in: nextText
            )
            if parent.text != nextText {
                parent.text = nextText
            }
            if parent.cursorOffset != nextOffset {
                parent.cursorOffset = nextOffset
            }
            textView.invalidateIntrinsicContentSize()
        }

        func textViewDidChangeSelection(_ textView: UITextView) {
            guard !isUpdatingProgrammatically else { return }
            let currentText = textView.text ?? ""
            let nextOffset = ComposerTextView.stringOffset(
                fromUtf16Location: textView.selectedRange.location,
                in: currentText
            )
            DispatchQueue.main.async {
                if self.parent.cursorOffset != nextOffset {
                    self.parent.cursorOffset = nextOffset
                }
            }
        }
    }
}
