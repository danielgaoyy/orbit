import SwiftUI
import UIKit

/// True hardware & software keyboard raw PTY input bridge (`UIKeyInput` + `pressesBegan`)
/// streaming raw UTF-8 / VT100 escape sequences directly to `sendTerminalBytes(...)`.
public struct RawPtyInputView: UIViewRepresentable {
    @Binding public var isFirstResponder: Bool
    @Binding public var isCtrlArmed: Bool
    @Binding public var isAltArmed: Bool
    public let onSendBytes: (Data) -> Void

    public init(
        isFirstResponder: Binding<Bool>,
        isCtrlArmed: Binding<Bool>,
        isAltArmed: Binding<Bool>,
        onSendBytes: @escaping (Data) -> Void
    ) {
        self._isFirstResponder = isFirstResponder
        self._isCtrlArmed = isCtrlArmed
        self._isAltArmed = isAltArmed
        self.onSendBytes = onSendBytes
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    public func makeUIView(context: Context) -> OrbitRawPtyUIKeyInputView {
        let view = OrbitRawPtyUIKeyInputView()
        view.backgroundColor = .clear
        view.coordinator = context.coordinator
        return view
    }

    public func updateUIView(_ uiView: OrbitRawPtyUIKeyInputView, context: Context) {
        context.coordinator.parent = self
        uiView.coordinator = context.coordinator

        if isFirstResponder && !uiView.isFirstResponder {
            DispatchQueue.main.async {
                _ = uiView.becomeFirstResponder()
            }
        } else if !isFirstResponder && uiView.isFirstResponder {
            DispatchQueue.main.async {
                _ = uiView.resignFirstResponder()
            }
        }
    }

    public final class Coordinator {
        var parent: RawPtyInputView

        init(parent: RawPtyInputView) {
            self.parent = parent
        }

        func handleRawText(_ text: String, hardwareCtrl: Bool = false, hardwareAlt: Bool = false) {
            let useCtrl = hardwareCtrl || parent.isCtrlArmed
            let useAlt = hardwareAlt || parent.isAltArmed
            let bytes = OrbitPtyInputLogic.encodeText(text, ctrl: useCtrl, alt: useAlt)
            if !bytes.isEmpty {
                parent.onSendBytes(bytes)
            }
            if parent.isCtrlArmed {
                DispatchQueue.main.async { self.parent.isCtrlArmed = false }
            }
            if parent.isAltArmed {
                DispatchQueue.main.async { self.parent.isAltArmed = false }
            }
        }

        func handleSpecialKey(_ key: OrbitPtySpecialKey, hardwareAlt: Bool = false) {
            let useAlt = hardwareAlt || parent.isAltArmed
            var bytes = OrbitPtyInputLogic.encodeSpecialKey(key)
            if useAlt && key != .escape {
                var prefixed = Data([0x1B])
                prefixed.append(bytes)
                bytes = prefixed
            }
            if !bytes.isEmpty {
                parent.onSendBytes(bytes)
            }
            if parent.isAltArmed {
                DispatchQueue.main.async { self.parent.isAltArmed = false }
            }
        }

        func updateFocusState(_ focused: Bool) {
            if parent.isFirstResponder != focused {
                DispatchQueue.main.async {
                    self.parent.isFirstResponder = focused
                }
            }
        }
    }
}

public final class OrbitRawPtyUIKeyInputView: UIView, UIKeyInput, UITextInputTraits {
    weak var coordinator: RawPtyInputView.Coordinator?

    public override var canBecomeFirstResponder: Bool { true }

    public var hasText: Bool { true }

    // Disable all iOS text transformations so terminal bytes arrive verbatim.
    public var autocorrectionType: UITextAutocorrectionType = .no
    public var autocapitalizationType: UITextAutocapitalizationType = .none
    public var spellCheckingType: UITextSpellCheckingType = .no
    public var smartQuotesType: UITextSmartQuotesType = .no
    public var smartDashesType: UITextSmartDashesType = .no
    public var smartInsertDeleteType: UITextSmartInsertDeleteType = .no
    public var keyboardType: UIKeyboardType = .asciiCapable
    public var keyboardAppearance: UIKeyboardAppearance = .dark
    public var returnKeyType: UIReturnKeyType = .default

    @discardableResult
    public override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { coordinator?.updateFocusState(true) }
        return ok
    }

    @discardableResult
    public override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { coordinator?.updateFocusState(false) }
        return ok
    }

    public func insertText(_ text: String) {
        coordinator?.handleRawText(text)
    }

    public func deleteBackward() {
        coordinator?.handleSpecialKey(.backspace)
    }

    public override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var handled = false
        for press in presses {
            guard let key = press.key else { continue }
            let mods = key.modifierFlags
            let isCtrl = mods.contains(.control)
            let isAlt = mods.contains(.alternate)
            let isShift = mods.contains(.shift)

            // Let system Command shortcuts pass through untouched.
            if mods.contains(.command) {
                continue
            }

            switch key.keyCode {
            case .keyboardEscape:
                coordinator?.handleSpecialKey(.escape)
                handled = true
            case .keyboardTab:
                coordinator?.handleSpecialKey(isShift ? .backTab : .tab, hardwareAlt: isAlt)
                handled = true
            case .keyboardUpArrow:
                coordinator?.handleSpecialKey(.arrowUp, hardwareAlt: isAlt)
                handled = true
            case .keyboardDownArrow:
                coordinator?.handleSpecialKey(.arrowDown, hardwareAlt: isAlt)
                handled = true
            case .keyboardLeftArrow:
                coordinator?.handleSpecialKey(.arrowLeft, hardwareAlt: isAlt)
                handled = true
            case .keyboardRightArrow:
                coordinator?.handleSpecialKey(.arrowRight, hardwareAlt: isAlt)
                handled = true
            case .keyboardHome:
                coordinator?.handleSpecialKey(.home)
                handled = true
            case .keyboardEnd:
                coordinator?.handleSpecialKey(.end)
                handled = true
            case .keyboardPageUp:
                coordinator?.handleSpecialKey(.pageUp)
                handled = true
            case .keyboardPageDown:
                coordinator?.handleSpecialKey(.pageDown)
                handled = true
            case .keyboardDeleteForward:
                coordinator?.handleSpecialKey(.deleteForward)
                handled = true
            case .keyboardDeleteOrBackspace:
                coordinator?.handleSpecialKey(.backspace, hardwareAlt: isAlt)
                handled = true
            case .keyboardReturnOrEnter, .keyboardReturn:
                coordinator?.handleSpecialKey(.enter, hardwareAlt: isAlt)
                handled = true
            default:
                if isCtrl || isAlt {
                    let chars = key.charactersIgnoringModifiers
                    if !chars.isEmpty {
                        coordinator?.handleRawText(chars, hardwareCtrl: isCtrl, hardwareAlt: isAlt)
                        handled = true
                    }
                }
            }
        }

        if !handled {
            super.pressesBegan(presses, with: event)
        }
    }
}
