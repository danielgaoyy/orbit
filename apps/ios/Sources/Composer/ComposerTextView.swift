import SwiftUI
import UIKit

/// Custom `UITextView` subclass that intercepts Backspace at caret offset 0
/// so an active inline slash directive (e.g. `/plan`) can be removed directly from the keyboard.
final class OrbitComposerUITextView: UITextView {
    var onBackspaceAtStart: (() -> Bool)?
    var onPasteImage: ((UIImage) -> Void)?
    var onLargeTextPaste: ((String) -> Void)?

    override func deleteBackward() {
        if selectedRange.location == 0,
           selectedRange.length == 0,
           markedTextRange == nil,
           onBackspaceAtStart?() == true {
            return
        }
        super.deleteBackward()
    }

    override func paste(_ sender: Any?) {
        let pb = UIPasteboard.general
        if let image = pb.image, pb.string == nil {
            onPasteImage?(image)
            return
        }
        if let pastedString = pb.string, OrbitComposerLogic.isLargeTextPaste(pastedString) {
            onLargeTextPaste?(pastedString)
            return
        }
        super.paste(sender)
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
    var maxLines: Int = 6
    var customMaxHeight: CGFloat? = nil
    let onBackspaceAtStart: () -> Bool
    var onPasteImage: ((UIImage) -> Void)? = nil
    var onLargeTextPaste: ((String) -> Void)? = nil

    static let bodyFont: UIFont = .systemFont(ofSize: 15, weight: .regular)
    static let lineSpacing: CGFloat = 4

    private var resolvedMaxHeight: CGFloat {
        let cap = ceil((Self.bodyFont.lineHeight + Self.lineSpacing) * CGFloat(max(1, maxLines))) + 6
        if let customMaxHeight {
            return min(cap, max(ceil(Self.bodyFont.lineHeight) + 8, customMaxHeight))
        }
        return cap
    }

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
        view.onPasteImage = onPasteImage
        view.onLargeTextPaste = onLargeTextPaste
        context.coordinator.lastAppliedIndent = firstLineHeadIndent
        context.coordinator.lastAppliedColor = textColor
        context.coordinator.lastFocusNonce = focusNonce
        return view
    }

    func updateUIView(_ uiView: OrbitComposerUITextView, context: Context) {
        context.coordinator.parent = self
        uiView.onBackspaceAtStart = onBackspaceAtStart
        uiView.onPasteImage = onPasteImage
        uiView.onLargeTextPaste = onLargeTextPaste
        uiView.tintColor = tintColor

        if focusNonce != context.coordinator.lastFocusNonce {
            context.coordinator.lastFocusNonce = focusNonce
            if uiView.window != nil && !uiView.isFirstResponder {
                DispatchQueue.main.async {
                    if uiView.window != nil && !uiView.isFirstResponder {
                        uiView.becomeFirstResponder()
                    }
                }
            }
        } else if !isFocused && context.coordinator.lastKnownFocused == true && uiView.isFirstResponder && uiView.markedTextRange == nil {
            context.coordinator.lastKnownFocused = false
            DispatchQueue.main.async {
                if !self.isFocused && uiView.isFirstResponder && uiView.markedTextRange == nil {
                    uiView.resignFirstResponder()
                }
            }
        }

        // Never overwrite attributedText while Chinese/Japanese IME composition is active
        guard uiView.markedTextRange == nil else { return }

        let textChanged = uiView.text != text
        let indentChanged = abs(context.coordinator.lastAppliedIndent - firstLineHeadIndent) > 0.25
        let colorChanged = context.coordinator.lastAppliedColor != textColor

        let attrs = Self.makeTypingAttributes(indent: firstLineHeadIndent, textColor: textColor)
        if text.isEmpty && (textChanged || indentChanged || colorChanged) {
            uiView.typingAttributes = attrs
        }

        guard textChanged || indentChanged || colorChanged else { return }

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
        updateScrollEnabledIfNeeded(uiView)
        uiView.invalidateIntrinsicContentSize()
    }

    private func updateScrollEnabledIfNeeded(_ uiView: OrbitComposerUITextView) {
        let width = uiView.bounds.width
        guard width > 0 else { return }
        let maxHeight = resolvedMaxHeight
        let contentHeight = uiView.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude)).height
        let needsScroll = contentHeight > maxHeight + 1
        if uiView.isScrollEnabled != needsScroll {
            uiView.isScrollEnabled = needsScroll
        }
    }

    func sizeThatFits(
        _ proposal: ProposedViewSize,
        uiView: OrbitComposerUITextView,
        context: Context
    ) -> CGSize? {
        let width = proposal.width ?? uiView.bounds.width
        guard width > 0 else { return nil }

        let minHeight = ceil(Self.bodyFont.lineHeight) + 4
        let maxHeight = resolvedMaxHeight
        let fitting: CGSize
        if !uiView.isScrollEnabled {
            fitting = uiView.sizeThatFits(CGSize(width: width, height: .greatestFiniteMagnitude))
        } else {
            let measured = uiView.attributedText?.boundingRect(
                with: CGSize(width: width, height: .greatestFiniteMagnitude),
                options: [.usesLineFragmentOrigin, .usesFontLeading],
                context: nil
            ).size ?? CGSize(width: width, height: minHeight)
            fitting = CGSize(width: width, height: measured.height + 4)
        }
        let clamped = min(max(fitting.height, minHeight), maxHeight)
        return CGSize(width: width, height: clamped)
    }

    private static func stringOffset(fromUtf16Location utf16Loc: Int, in string: String) -> Int {
        let clamped = min(max(0, utf16Loc), string.utf16.count)
        var idx = string.utf16.index(string.utf16.startIndex, offsetBy: clamped)
        while idx > string.utf16.startIndex && idx.samePosition(in: string) == nil {
            idx = string.utf16.index(before: idx)
        }
        if let valid = idx.samePosition(in: string) {
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
        var lastKnownFocused: Bool = false
        var isUpdatingProgrammatically: Bool = false

        init(_ parent: ComposerTextView) {
            self.parent = parent
        }

        func textViewDidBeginEditing(_ textView: UITextView) {
            lastKnownFocused = true
            DispatchQueue.main.async {
                if !self.parent.isFocused {
                    self.parent.isFocused = true
                }
            }
        }

        func textViewDidEndEditing(_ textView: UITextView) {
            lastKnownFocused = false
            DispatchQueue.main.async {
                if self.parent.isFocused {
                    self.parent.isFocused = false
                }
            }
        }

        func textViewDidChange(_ textView: UITextView) {
            guard !isUpdatingProgrammatically else { return }
            if textView.markedTextRange != nil {
                return
            }
            let nextText = textView.text ?? ""
            if nextText.isEmpty {
                let attrs = ComposerTextView.makeTypingAttributes(
                    indent: parent.firstLineHeadIndent,
                    textColor: parent.textColor
                )
                textView.typingAttributes = attrs
            }
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
            DispatchQueue.main.async {
                if let tv = textView as? OrbitComposerUITextView {
                    self.parent.updateScrollEnabledIfNeeded(tv)
                }
                textView.invalidateIntrinsicContentSize()
            }
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
