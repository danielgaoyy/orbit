import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers

struct OmniComposerView: View {
    private static let controlBarHeight: CGFloat = 36

    @ObservedObject var store: OrbitIosStore

    @Environment(\.orbitPalette) private var palette
    @State private var isComposerFocused: Bool = false
    @State private var cursorOffset: Int = 0
    @State private var isSoftwareKeyboardVisible: Bool = false
    @State private var isPhotosPickerPresented: Bool = false
    @State private var selectedPhotoItems: [PhotosPickerItem] = []
    @State private var isCameraPickerPresented: Bool = false
    @State private var isFileImporterPresented: Bool = false
    @State private var isPermissionPickerPresented: Bool = false
    @State private var cachedSlashCommands: [FfiSlashCommand] = []
    @State private var cachedMentionCandidates: [FfiMentionCandidate] = []

    private var hasHomeIndicatorInset: Bool {
        guard let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene,
              let window = scene.windows.first(where: \.isKeyWindow) ?? scene.windows.first else {
            return false
        }
        return window.safeAreaInsets.bottom >= 20
    }

    private var composerBottomPadding: CGFloat {
        if isSoftwareKeyboardVisible {
            return OrbitSpacing.sm
        }
        return hasHomeIndicatorInset ? 2 : 10
    }

    private func updateSoftwareKeyboardVisibility(from notification: Notification) {
        guard let endFrame = notification.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect else {
            return
        }
        let windowHeight: CGFloat = {
            if let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene,
               let window = scene.windows.first(where: \.isKeyWindow) ?? scene.windows.first {
                return window.bounds.height
            }
            return UIScreen.main.bounds.height
        }()
        let visibleOverlap = max(0, windowHeight - endFrame.minY)
        let nextVisible = visibleOverlap > 100
        if nextVisible != isSoftwareKeyboardVisible {
            withAnimation(.easeOut(duration: 0.22)) {
                isSoftwareKeyboardVisible = nextVisible
            }
        }
    }

    private var hasTrayItems: Bool {
        !store.attachedContexts.isEmpty || !store.quotedDiffSnippets.isEmpty
    }

    private var isRunningWithDraft: Bool {
        store.isActiveAgentTurnBusy && store.canSubmitComposer && store.selectedAgentId != nil
    }

    private var showPrimaryStopButton: Bool {
        store.isActiveAgentTurnBusy && !store.canSubmitComposer
    }

    private var placeholderText: String {
        "Ask anything, @ to mention, / for actions"
    }

    private var normalizedPermissionMode: FfiSessionMode {
        OrbitFormatters.normalizePermissionMode(store.selectedMode)
    }

    // MARK: - Inline `/` Slash & `@` Mention Detection

    private struct SlashTriggerMatch {
        let tokenRange: Range<String.Index>
        let query: String
        let commands: [FfiSlashCommand]
    }

    private struct MentionTriggerMatch {
        let tokenRange: Range<String.Index>
        let query: String
        let candidates: [FfiMentionCandidate]
    }

    private var clampedCursorUtf16: Int {
        let text = store.composerText
        let safeOffset = min(max(0, cursorOffset), text.count)
        let idx = text.index(text.startIndex, offsetBy: safeOffset)
        return text.utf16.distance(from: text.utf16.startIndex, to: idx)
    }

    private var rawActiveTrigger: OrbitComposerTrigger? {
        OrbitComposerLogic.findActiveTrigger(in: store.composerText, cursorUtf16Offset: clampedCursorUtf16)
    }

    private var activeSlashTrigger: SlashTriggerMatch? {
        guard let trig = rawActiveTrigger, trig.kind == .slash,
              !cachedSlashCommands.isEmpty,
              let range = Range(NSRange(location: trig.rangeLocationUtf16, length: trig.rangeLengthUtf16), in: store.composerText) else {
            return nil
        }
        return SlashTriggerMatch(tokenRange: range, query: trig.query, commands: cachedSlashCommands)
    }

    private var activeMentionTrigger: MentionTriggerMatch? {
        guard let trig = rawActiveTrigger, trig.kind == .mention,
              !cachedMentionCandidates.isEmpty,
              let range = Range(NSRange(location: trig.rangeLocationUtf16, length: trig.rangeLengthUtf16), in: store.composerText) else {
            return nil
        }
        return MentionTriggerMatch(tokenRange: range, query: trig.query, candidates: cachedMentionCandidates)
    }

    private func refreshAutocompleteCache() {
        guard let trig = rawActiveTrigger else {
            if !cachedSlashCommands.isEmpty { cachedSlashCommands = [] }
            if !cachedMentionCandidates.isEmpty { cachedMentionCandidates = [] }
            return
        }
        switch trig.kind {
        case .slash:
            cachedMentionCandidates = []
            guard trig.query.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "_" || $0 == "-") }) else {
                cachedSlashCommands = []
                return
            }
            cachedSlashCommands = store.filterSlashCommands(query: trig.query)
        case .mention:
            cachedSlashCommands = []
            if store.workspaceFilesList.isEmpty {
                store.refreshWorkspaceFiles()
            }
            if store.gitReview == nil {
                store.refreshGitReview()
            }
            cachedMentionCandidates = store.rankMentionCandidates(query: trig.query, limit: 11)
        }
    }

    var body: some View {
        OrbitGlassContainer(spacing: OrbitSpacing.sm) {
            VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                if let slashMatch = activeSlashTrigger {
                    ComposerSlashCommandPanel(commands: slashMatch.commands) { cmd in
                        executeSlashCommand(cmd, trigger: slashMatch)
                    }
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                } else if let mentionMatch = activeMentionTrigger {
                    ComposerMentionPanel(candidates: mentionMatch.candidates) { candidate in
                        executeMentionSelect(candidate, trigger: mentionMatch)
                    }
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                }

                if !store.activeQueuedPrompts.isEmpty {
                    queuedPromptsTray
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }

                composerCard
                    .photosPicker(
                        isPresented: $isPhotosPickerPresented,
                        selection: $selectedPhotoItems,
                        maxSelectionCount: 8,
                        matching: .images
                    )
                    .onChange(of: selectedPhotoItems) { items in
                        handlePickedPhotos(items)
                    }
            }
        }
        .fileImporter(
            isPresented: $isFileImporterPresented,
            allowedContentTypes: [.item],
            allowsMultipleSelection: true
        ) { result in
            handleImportedFiles(result)
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.top, OrbitSpacing.sm)
        .padding(.bottom, composerBottomPadding)
        .fullScreenCover(isPresented: $isCameraPickerPresented) {
            CameraPickerView { image in
                handleCapturedPhoto(image)
            }
            .ignoresSafeArea()
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillChangeFrameNotification)) { note in
            updateSoftwareKeyboardVisibility(from: note)
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in
            if isSoftwareKeyboardVisible {
                withAnimation(.easeOut(duration: 0.22)) {
                    isSoftwareKeyboardVisible = false
                }
            }
        }
        .onAppear {
            cursorOffset = store.composerText.count
            refreshAutocompleteCache()
        }
        .onChange(of: store.composerText) { newText in
            if !isComposerFocused || cursorOffset > newText.count {
                cursorOffset = newText.count
            }
            promoteTrailingSpaceSlashDirectiveIfNeeded(in: newText)
            refreshAutocompleteCache()
        }
        .onChange(of: cursorOffset) { _ in
            refreshAutocompleteCache()
        }
    }

    private func collapseStrippedToken(before: String, after: String) -> (text: String, cursor: Int) {
        let cleanBefore = before.replacingOccurrences(of: "[ \\t]+$", with: "", options: .regularExpression)
        let cleanAfter = after.replacingOccurrences(of: "^[ \\t]+", with: "", options: .regularExpression)
        if cleanBefore.isEmpty {
            return (cleanAfter, 0)
        }
        if cleanAfter.isEmpty {
            return (cleanBefore, cleanBefore.count)
        }
        return ("\(cleanBefore) \(cleanAfter)", cleanBefore.count + 1)
    }

    private func promoteTrailingSpaceSlashDirectiveIfNeeded(in text: String) {
        guard store.slashDirective == nil, !text.isEmpty else { return }
        let safeOffset = min(max(0, cursorOffset), text.count)
        guard safeOffset > 0 else { return }
        let endIdx = text.index(text.startIndex, offsetBy: safeOffset)
        let before = String(text[..<endIdx])
        guard before.hasSuffix(" ") else { return }

        let withoutTrailingSpace = String(before.dropLast())
        let tokenStart = withoutTrailingSpace.lastIndex(where: { $0.isWhitespace })
            .map { withoutTrailingSpace.index(after: $0) }
            ?? withoutTrailingSpace.startIndex
        let token = String(withoutTrailingSpace[tokenStart...]).lowercased()
        guard token.hasPrefix("/"),
              let directive = OrbitSlashDirective(rawValue: String(token.dropFirst())) else {
            return
        }
        if directive == .compact && (store.isDraftNewSession || store.selectedAgentId == nil) {
            return
        }
        let collapsed = collapseStrippedToken(
            before: String(withoutTrailingSpace[..<tokenStart]),
            after: String(text[endIdx...])
        )
        store.composerText = collapsed.text
        cursorOffset = collapsed.cursor
        store.applyQuickWorkflow(directive)
    }

    private func executeSlashCommand(_ cmd: FfiSlashCommand, trigger: SlashTriggerMatch) {
        let text = store.composerText
        let collapsed = collapseStrippedToken(
            before: String(text[..<trigger.tokenRange.lowerBound]),
            after: String(text[trigger.tokenRange.upperBound...])
        )
        store.composerText = collapsed.text
        cursorOffset = collapsed.cursor

        switch cmd.id {
        case "plan":
            store.applyQuickWorkflow(.plan)
        case "review":
            store.applyQuickWorkflow(.review)
        case "compact":
            if !store.isDraftNewSession && store.selectedAgentId != nil {
                store.applyQuickWorkflow(.compact)
            } else {
                store.hapticWarning()
                store.showToast("Start an active session first to compact context")
            }
        case "orbit", "orbit-handoff", "orbit-committee", "orbit-advisor":
            if let dir = OrbitSlashDirective(rawValue: cmd.id) {
                store.applyQuickWorkflow(dir)
            }
        case "ask", "supervised":
            store.applySessionMode(.default)
        case "auto":
            store.applySessionMode(.auto)
        case "edits":
            store.applySessionMode(.acceptEdits)
        case "full":
            store.applySessionMode(.fullAccess)
        case "diff":
            store.openGitDiff()
        case "files":
            store.openWorkspaceFiles()
        case "btw":
            let remaining = collapsed.text.trimmingCharacters(in: .whitespacesAndNewlines)
            if !remaining.isEmpty {
                store.composerText = ""
                cursorOffset = 0
                store.openSideQuestion(prefillAndSubmit: remaining)
            } else {
                store.openSideQuestion()
            }
            return
        case "terminal":
            store.openTerminal()
        case "new":
            store.startNewSessionDraft()
        case "settings":
            store.activeSheet = .settings
        default:
            break
        }
        store.requestComposerFocus()
        store.hapticLight()
    }

    private func executeMentionSelect(_ candidate: FfiMentionCandidate, trigger: MentionTriggerMatch) {
        let text = store.composerText
        let beforeAt = String(text[..<trigger.tokenRange.lowerBound])
        let afterToken = String(text[trigger.tokenRange.upperBound...])

        func insertInlineToken(_ token: String) {
            let needsLeadingSpace = !beforeAt.isEmpty && !(beforeAt.last?.isWhitespace ?? true)
            let inserted = "\(needsLeadingSpace ? " " : "")\(token) "
            let trimmedAfter = String(afterToken.drop(while: { $0 == " " || $0 == "\t" }))
            store.composerText = "\(beforeAt)\(inserted)\(trimmedAfter)"
            cursorOffset = beforeAt.count + inserted.count
            store.requestComposerFocus()
        }

        switch candidate.kind {
        case "file":
            let path = candidate.filePath.isEmpty ? candidate.key : candidate.filePath
            let token = candidate.tokenLabel.isEmpty ? "@\(candidate.title)" : candidate.tokenLabel
            store.registerInlineMention(token: token, item: .file(path: path))
            insertInlineToken(token)
        case "git_diff":
            store.refreshGitReview()
            let token = candidate.tokenLabel.isEmpty ? "@working-diff" : candidate.tokenLabel
            store.registerInlineMention(token: token, item: .gitDiff(staged: false, label: candidate.title))
            insertInlineToken(token)
        case "staged":
            store.setGitStagedFilter(true)
            let token = candidate.tokenLabel.isEmpty ? "@staged-diff" : candidate.tokenLabel
            store.registerInlineMention(token: token, item: .gitDiff(staged: true, label: candidate.title))
            insertInlineToken(token)
        case "peer", "all", "workspace_peer":
            let token = candidate.tokenLabel.isEmpty ? "@\(candidate.title)" : candidate.tokenLabel
            insertInlineToken(token)
        case "terminal":
            if let snap = store.captureTerminalSnapshotText() {
                let token = candidate.tokenLabel.isEmpty ? "@terminal" : candidate.tokenLabel
                store.registerInlineMention(token: token, item: .terminalOutput(label: candidate.title, content: snap))
                insertInlineToken(token)
            } else {
                let collapsed = collapseStrippedToken(before: beforeAt, after: afterToken)
                store.composerText = collapsed.text
                cursorOffset = collapsed.cursor
                store.requestComposerFocus()
                store.showToast("No active terminal output yet")
            }
        default:
            break
        }
    }

    // MARK: - Queued Prompts Tray

    private var queuedPromptsTray: some View {
        let queuedList = store.activeQueuedPrompts
        return VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(queuedList.enumerated()), id: \.element.id) { idx, item in
                if idx > 0 {
                    OrbitHairline()
                }
                HStack(spacing: OrbitSpacing.xs) {
                    Image(systemName: "text.append")
                        .font(OrbitTypography.iconMicro)
                        .foregroundStyle(palette.primary)

                    Text(item.summaryPreview)
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    if item.attachmentCount > 0 && !item.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        Text("+\(item.attachmentCount)")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgMuted)
                            .padding(.horizontal, OrbitSpacing.xs + 2)
                            .padding(.vertical, 1)
                            .background(palette.subtleFill)
                            .clipShape(Capsule())
                    }

                    Spacer(minLength: OrbitSpacing.xs)

                    if queuedList.count > 1 {
                        Menu {
                            if idx > 0 {
                                Button {
                                    store.reorderQueuedPrompts(fromIndex: idx, toOffset: idx - 1)
                                } label: {
                                    Label("Move Up", systemImage: "arrow.up")
                                }
                            }
                            if idx + 1 < queuedList.count {
                                Button {
                                    store.reorderQueuedPrompts(fromIndex: idx, toOffset: idx + 2)
                                } label: {
                                    Label("Move Down", systemImage: "arrow.down")
                                }
                                Button {
                                    store.mergeQueuedPromptWithNext(item)
                                } label: {
                                    Label("Merge with Next", systemImage: "arrow.triangle.merge")
                                }
                            }
                        } label: {
                            Image(systemName: "ellipsis.circle")
                                .font(.system(size: 13.5, weight: .medium))
                                .foregroundStyle(palette.fgMuted)
                                .frame(width: 28, height: OrbitSpacing.hitTarget)
                                .contentShape(Rectangle())
                        }
                        .accessibilityLabel("Queue Options")
                    }

                    Button {
                        store.editQueuedPrompt(item)
                        isComposerFocused = true
                    } label: {
                        Text("Edit")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgSecondary)
                            .padding(.horizontal, OrbitSpacing.sm)
                            .frame(minWidth: 40, minHeight: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Edit Queued Prompt")

                    Button {
                        store.sendQueuedPromptNow(item)
                    } label: {
                        Text("Send Now")
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.primary)
                            .padding(.horizontal, OrbitSpacing.xs + 2)
                            .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Send Queued Prompt Now")

                    Button {
                        store.removeQueuedPrompt(item)
                    } label: {
                        Image(systemName: "xmark")
                            .font(OrbitTypography.iconMicro)
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Remove Queued Prompt")
                }
                .padding(.leading, OrbitSpacing.md)
                .frame(minHeight: OrbitSpacing.hitTarget)
            }
        }
        .orbitGlassSurface(
            in: RoundedRectangle(cornerRadius: 16, style: .continuous),
            fallbackFill: palette.elevated,
            fallbackBorder: palette.hairline
        )
    }

    // MARK: - Main Composer Pill

    private var composerCard: some View {
        VStack(alignment: .leading, spacing: 0) {
            if hasTrayItems {
                attachmentsTray
            }

            // Auto-expanding 1..6 line native TextView with inline directive prefix (1:1 Desktop)
            ZStack(alignment: .topLeading) {
                if store.slashDirective == nil && store.composerText.isEmpty {
                    Text(placeholderText)
                        .font(OrbitTypography.body(.regular))
                        .foregroundStyle(palette.fgMuted.opacity(0.58))
                        .lineLimit(1)
                        .padding(.top, 2)
                        .allowsHitTesting(false)
                }

                ComposerTextView(
                    text: $store.composerText,
                    isFocused: $isComposerFocused,
                    cursorOffset: $cursorOffset,
                    focusNonce: store.composerFocusNonce,
                    firstLineHeadIndent: ComposerTextView.inlineDirectiveIndent(for: store.slashDirective),
                    textColor: UIColor(palette.fgPrimary),
                    tintColor: UIColor(palette.primary),
                    onBackspaceAtStart: {
                        guard store.slashDirective != nil else { return false }
                        store.slashDirective = nil
                        store.hapticLight()
                        return true
                    },
                    onPasteImage: { image in
                        handleCapturedPhoto(image)
                    },
                    onLargeTextPaste: { pastedText in
                        let data = Data(pastedText.utf8)
                        store.attachFile(
                            filename: "pasted-snippet.txt",
                            mimeType: "text/plain",
                            data: data,
                            isImageHint: false
                        )
                        store.showToast("Attached large paste as snippet")
                    }
                )

                if let directive = store.slashDirective {
                    Button {
                        store.slashDirective = nil
                        store.hapticLight()
                    } label: {
                        HStack(spacing: 5.5) {
                            Image(systemName: directive.icon)
                                .font(.system(size: 15, weight: .medium))
                                .foregroundStyle(palette.fgMuted)
                            Text(directive.rawValue)
                                .font(.system(size: 15, weight: .regular))
                                .foregroundStyle(palette.fgPrimary)
                        }
                        .frame(height: ceil(ComposerTextView.bodyFont.lineHeight))
                        .padding(.top, 2)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear /\(directive.rawValue) directive")
                }
            }
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.top, hasTrayItems ? OrbitSpacing.xs : OrbitSpacing.md)
            .padding(.bottom, 2)

            // Bottom Control Bar — balanced optical spacing (~12pt paired gaps, concentric 10pt Send inset)
            HStack(spacing: 0) {
                // [+] Native Attachment Menu (Photos / Camera / Files)
                if store.selectedHostId == nil && !store.hasDemoStageOverlay {
                    Button {
                        store.hapticLight()
                        store.activeSheet = .qrPair
                    } label: {
                        plusButtonLabel
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Pair a Host to Attach Photos or Files")
                } else {
                    Menu {
                        Button {
                            store.hapticLight()
                            isPhotosPickerPresented = true
                        } label: {
                            Label("Photos", systemImage: "photo.on.rectangle")
                        }

                        Button {
                            store.hapticLight()
                            if UIImagePickerController.isSourceTypeAvailable(.camera) {
                                isCameraPickerPresented = true
                            } else {
                                store.showToast("Camera is unavailable on this device")
                            }
                        } label: {
                            Label("Camera", systemImage: "camera")
                        }

                        Button {
                            store.hapticLight()
                            isFileImporterPresented = true
                        } label: {
                            Label("Files", systemImage: "folder")
                        }
                    } label: {
                        plusButtonLabel
                    }
                    .menuOrder(.fixed)
                    .accessibilityLabel("Attach Photos, Camera, or Files")
                }

                // [Agent Icon + Model + Effort ⌄] anchored native Liquid Glass Popover (aligned 1:1 with Desktop)
                ComposerModelTriggerButton(
                    provider: store.selectedProvider,
                    modelDisplayName: store.composerModelDisplayName,
                    effortDisplayLabel: store.composerEffortDisplayLabel,
                    isPresented: $store.isModelPickerPresented,
                    onTap: {
                        isComposerFocused = false
                        store.hapticLight()
                        store.isModelPickerPresented.toggle()
                    },
                    popoverContent: {
                        ModelPickerPopoverView(store: store)
                            .environment(\.orbitPalette, palette)
                    }
                )

                Spacer(minLength: OrbitSpacing.xs)

                // [Access Permissions] Compact Popover (aligned 1:1 with ModelPickerPopoverView)
                Button {
                    isComposerFocused = false
                    store.hapticLight()
                    isPermissionPickerPresented.toggle()
                } label: {
                    Image(systemName: OrbitFormatters.sessionModeIcon(normalizedPermissionMode))
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 36, height: Self.controlBarHeight)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Access Permissions: \(OrbitFormatters.sessionModeLabel(normalizedPermissionMode))")
                .orbitCompactPopover(
                    isPresented: $isPermissionPickerPresented,
                    arrowEdge: .bottom,
                    palette: palette
                ) {
                    VStack(alignment: .leading, spacing: 6) {
                        OrbitPopoverSectionHeader(title: "Access Permissions")

                        VStack(alignment: .leading, spacing: 2) {
                            OrbitPopoverOptionRow(
                                title: "Always Ask",
                                subtitle: "Require approval before file edits and commands",
                                systemImage: "shield",
                                isSelected: normalizedPermissionMode == .default
                            ) {
                                store.applySessionMode(.default)
                                isPermissionPickerPresented = false
                            }

                            OrbitPopoverOptionRow(
                                title: "Accept Edits",
                                subtitle: "Auto-approve file edits, ask before commands",
                                systemImage: "checkmark.shield",
                                isSelected: normalizedPermissionMode == .acceptEdits
                            ) {
                                store.applySessionMode(.acceptEdits)
                                isPermissionPickerPresented = false
                            }

                            OrbitPopoverOptionRow(
                                title: "Full Access",
                                subtitle: "Edit files and run commands without prompts",
                                systemImage: "shield.slash",
                                isSelected: normalizedPermissionMode == .fullAccess
                            ) {
                                store.applySessionMode(.fullAccess)
                                isPermissionPickerPresented = false
                            }
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 10)
                    .frame(width: 296)
                }

                // Borderless Stop action when user is typing a queued prompt during active run
                if isRunningWithDraft {
                    Button {
                        store.interruptActiveAgent()
                    } label: {
                        HStack(spacing: OrbitSpacing.xs) {
                            Image(systemName: "stop.circle.fill")
                                .font(OrbitTypography.iconSmall)
                            Text("Stop")
                                .font(OrbitTypography.meta(.semibold))
                        }
                        .foregroundStyle(palette.danger)
                        .padding(.horizontal, OrbitSpacing.xs + 2)
                        .frame(minHeight: Self.controlBarHeight)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Stop Generation")
                }

                // Primary Right Circle Action: Stop ■ / Queue ⤒ / Send ↑ (only control with background)
                if showPrimaryStopButton {
                    Button {
                        store.interruptActiveAgent()
                    } label: {
                        ZStack {
                            Circle()
                                .fill(palette.subtleFill)
                                .frame(width: 32, height: 32)
                            RoundedRectangle(cornerRadius: 2.5, style: .continuous)
                                .fill(Color(hex: "#EF4444"))
                                .frame(width: 11, height: 11)
                        }
                        .frame(width: 36, height: Self.controlBarHeight)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Stop Generation")
                } else if isRunningWithDraft {
                    Button {
                        store.enqueueComposerPrompt()
                    } label: {
                        ZStack {
                            Circle()
                                .fill(palette.primary)
                                .frame(width: 32, height: 32)
                            Image(systemName: "arrow.up.to.line")
                                .font(OrbitTypography.iconSmall)
                                .foregroundStyle(palette.onPrimary)
                        }
                        .frame(width: 36, height: Self.controlBarHeight)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Queue Next Prompt")
                } else {
                    Button {
                        if store.canSubmitComposer {
                            store.submitComposer()
                        } else if store.selectedHostId == nil {
                            store.activeSheet = .qrPair
                        }
                    } label: {
                        ZStack {
                            Circle()
                                .fill(store.canSubmitComposer ? palette.primary : palette.subtleFill)
                                .frame(width: 32, height: 32)
                            Image(systemName: "arrow.up")
                                .font(.system(size: 14, weight: .semibold))
                                .foregroundStyle(store.canSubmitComposer ? palette.onPrimary : palette.fgMuted)
                        }
                        .frame(width: 36, height: Self.controlBarHeight)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .disabled(!store.canSubmitComposer && store.selectedHostId != nil)
                    .accessibilityLabel("Send Prompt")
                }
            }
            .padding(.leading, 6)
            .padding(.trailing, 6)
            .padding(.bottom, 6)
        }
        .orbitGlassSurface(
            in: RoundedRectangle(cornerRadius: 24, style: .continuous),
            fallbackFill: palette.elevated,
            fallbackBorder: palette.hairline,
            fallbackShadowOpacity: palette.isDark ? 0.22 : 0.06
        )
    }

    private var plusButtonLabel: some View {
        Image(systemName: "plus")
            .font(OrbitTypography.icon)
            .foregroundStyle(palette.fgSecondary)
            .frame(width: 34, height: Self.controlBarHeight)
            .contentShape(Rectangle())
    }

    // MARK: - Attachment Pickers Processing

    private func handlePickedPhotos(_ items: [PhotosPickerItem]) {
        guard !items.isEmpty else { return }
        let batch = items
        selectedPhotoItems = []

        Task {
            for (idx, item) in batch.enumerated() {
                guard let rawData = try? await item.loadTransferable(type: Data.self),
                      !rawData.isEmpty else {
                    continue
                }
                let ts = Int(Date().timeIntervalSince1970)
                let filename = batch.count > 1 ? "photo-\(ts)-\(idx + 1).jpg" : "photo-\(ts).jpg"
                let normalized = await Task.detached(priority: .userInitiated) {
                    OrbitAttachmentNormalizer.normalizeImageData(rawData)
                }.value
                guard let normalized else {
                    await MainActor.run {
                        store.showToast("Unsupported image format")
                    }
                    continue
                }
                await MainActor.run {
                    store.attachImage(
                        filename: filename,
                        uploadData: normalized.uploadData,
                        previewData: normalized.previewData
                    )
                }
            }
        }
    }

    private func handleCapturedPhoto(_ image: UIImage) {
        let filename = "camera-\(Int(Date().timeIntervalSince1970)).jpg"
        Task {
            let normalized = await Task.detached(priority: .userInitiated) {
                OrbitAttachmentNormalizer.normalizeImage(image)
            }.value
            await MainActor.run {
                guard let normalized else {
                    store.showToast("Failed to process photo")
                    return
                }
                store.attachImage(
                    filename: filename,
                    uploadData: normalized.uploadData,
                    previewData: normalized.previewData
                )
            }
        }
    }

    private func handleImportedFiles(_ result: Result<[URL], Error>) {
        switch result {
        case let .failure(error):
            store.showToast("File import failed: \(error.localizedDescription)")
        case let .success(urls):
            for url in urls {
                let scoped = url.startAccessingSecurityScopedResource()
                defer {
                    if scoped {
                        url.stopAccessingSecurityScopedResource()
                    }
                }
                do {
                    let data = try Data(contentsOf: url)
                    let filename = url.lastPathComponent.isEmpty ? "file.bin" : url.lastPathComponent
                    let utType = UTType(filenameExtension: url.pathExtension)
                    let mimeType = utType?.preferredMIMEType
                    let isImageHint = utType?.conforms(to: .image) ?? false
                    store.attachFile(
                        filename: filename,
                        mimeType: mimeType,
                        data: data,
                        isImageHint: isImageHint
                    )
                } catch {
                    store.showToast("Could not read \(url.lastPathComponent): \(error.localizedDescription)")
                }
            }
        }
    }

    // MARK: - Removable Context Chips Tray

    private var attachmentsTray: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: OrbitSpacing.xs) {
                ForEach(store.attachedContexts) { ctx in
                    let uploadState = store.composerUploadStates[ctx.id]
                    let failedMsg: String? = {
                        if case let .failed(msg)? = uploadState { return msg }
                        return nil
                    }()
                    switch ctx {
                    case let .stagedImage(_, filename, _, previewData, _, isUploading):
                        imageThumbnailChip(
                            filename: filename,
                            previewData: previewData,
                            isUploading: isUploading,
                            failedError: failedMsg,
                            onRetry: {
                                store.retryAttachmentUpload(contextId: ctx.id)
                            },
                            onRemove: {
                                store.removeContextItem(ctx)
                            }
                        )
                    case let .stagedFile(_, filename, _, sizeBytes, isUploading):
                        removableChip(
                            icon: failedMsg != nil ? "exclamationmark.triangle" : ctx.icon,
                            label: filename,
                            detail: failedMsg != nil ? "Retry" : OrbitAttachmentNormalizer.formatByteCount(sizeBytes),
                            isAccent: failedMsg != nil,
                            isLoading: isUploading
                        ) {
                            if failedMsg != nil {
                                store.retryAttachmentUpload(contextId: ctx.id)
                            } else {
                                store.removeContextItem(ctx)
                            }
                        }
                        .contextMenu {
                            if failedMsg != nil {
                                Button("Retry Upload") {
                                    store.retryAttachmentUpload(contextId: ctx.id)
                                }
                            }
                            Button("Remove", role: .destructive) {
                                store.removeContextItem(ctx)
                            }
                        }
                    case let .inlineFile(_, filename, _, lineCount, _):
                        removableChip(
                            icon: ctx.icon,
                            label: filename,
                            detail: "\(lineCount)L",
                            isAccent: false
                        ) {
                            store.removeContextItem(ctx)
                        }
                    default:
                        removableChip(
                            icon: ctx.icon,
                            label: ctx.chipLabel,
                            isAccent: false
                        ) {
                            store.removeContextItem(ctx)
                        }
                    }
                }

                ForEach(store.quotedDiffSnippets) { snippet in
                    removableChip(
                        icon: "text.quote",
                        label: snippet.chipLabel,
                        isAccent: false
                    ) {
                        store.removeQuotedSnippet(snippet)
                    }
                }
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.top, OrbitSpacing.sm + 2)
            .padding(.bottom, 2)
        }
    }

    private func imageThumbnailChip(
        filename: String,
        previewData: Data,
        isUploading: Bool,
        failedError: String? = nil,
        onRetry: (() -> Void)? = nil,
        onRemove: @escaping () -> Void
    ) -> some View {
        ZStack(alignment: .topTrailing) {
            Group {
                if let uiImage = UIImage(data: previewData) {
                    Image(uiImage: uiImage)
                        .resizable()
                        .scaledToFill()
                } else {
                    Image(systemName: "photo")
                        .font(OrbitTypography.icon)
                        .foregroundStyle(palette.fgMuted)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(palette.subtleFill)
                }
            }
            .frame(width: 56, height: 56)
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay {
                if isUploading {
                    ZStack {
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .fill(Color.black.opacity(0.36))
                        ProgressView()
                            .tint(.white)
                            .controlSize(.small)
                    }
                } else if failedError != nil {
                    Button {
                        onRetry?()
                    } label: {
                        ZStack {
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .fill(palette.danger.opacity(0.55))
                            Image(systemName: "arrow.clockwise")
                                .font(.system(size: 16, weight: .bold))
                                .foregroundStyle(.white)
                        }
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Retry uploading \(filename)")
                }
            }
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )

            Button(action: onRemove) {
                Image(systemName: "xmark.circle.fill")
                    .font(.system(size: 16, weight: .semibold))
                    .symbolRenderingMode(.palette)
                    .foregroundStyle(.white, Color.black.opacity(0.68))
                    .padding(4)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Remove \(filename)")
        }
    }

    private func removableChip(
        icon: String,
        label: String,
        detail: String? = nil,
        isAccent: Bool,
        isLoading: Bool = false,
        onRemove: @escaping () -> Void
    ) -> some View {
        Button(action: onRemove) {
            HStack(spacing: OrbitSpacing.xs) {
                if isLoading {
                    ProgressView()
                        .controlSize(.mini)
                } else {
                    Image(systemName: icon)
                        .font(OrbitTypography.iconMicro)
                }
                Text(label)
                    .font(OrbitTypography.code(.medium))
                    .lineLimit(1)
                    .truncationMode(.middle)
                if let detail {
                    Text(detail)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                }
                Image(systemName: "xmark")
                    .font(OrbitTypography.iconMicro)
                    .opacity(0.75)
            }
            .foregroundStyle(isAccent ? palette.primary : palette.fgSecondary)
            .padding(.horizontal, OrbitSpacing.sm + 2)
            .padding(.vertical, 5)
            .background(isAccent ? palette.primary.opacity(0.12) : palette.subtleFill)
            .clipShape(Capsule())
            .overlay(
                Capsule()
                    .strokeBorder(isAccent ? palette.primary.opacity(0.3) : palette.hairline, lineWidth: 0.5)
            )
            .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Remove \(label)")
    }
}

private struct ComposerModelTriggerLabel: View, Equatable {
    let provider: FfiProviderKind
    let modelDisplayName: String
    let effortDisplayLabel: String?
    let palette: OrbitPalette

    var body: some View {
        HStack(spacing: 6) {
            OrbitProviderIcon(provider: provider, size: 16)

            Text(modelDisplayName)
                .font(.system(size: 13.5, weight: .regular))
                .foregroundStyle(palette.fgPrimary.opacity(0.88))
                .lineLimit(1)

            if let effortDisplayLabel {
                Text(effortDisplayLabel)
                    .font(.system(size: 13.5, weight: .regular))
                    .foregroundStyle(palette.fgPrimary.opacity(0.48))
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
            }

            Image(systemName: "chevron.down")
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(palette.fgPrimary.opacity(0.45))
        }
        .padding(.leading, 2)
        .padding(.trailing, OrbitSpacing.sm)
        .frame(minHeight: 36)
        .contentShape(Rectangle())
    }
}

private struct ComposerModelTriggerButton<PopoverContent: View>: View {
    let provider: FfiProviderKind
    let modelDisplayName: String
    let effortDisplayLabel: String?
    @Binding var isPresented: Bool
    let onTap: () -> Void
    @ViewBuilder let popoverContent: () -> PopoverContent

    @Environment(\.orbitPalette) private var palette

    var body: some View {
        Button(action: onTap) {
            ComposerModelTriggerLabel(
                provider: provider,
                modelDisplayName: modelDisplayName,
                effortDisplayLabel: effortDisplayLabel,
                palette: palette
            )
            .equatable()
        }
        .buttonStyle(.plain)
        .layoutPriority(1)
        .accessibilityLabel("Select Provider, Model, and Reasoning Effort")
        .popover(
            isPresented: $isPresented,
            attachmentAnchor: .rect(.bounds),
            arrowEdge: .bottom
        ) {
            popoverContent()
                .presentationCompactAdaptation(.popover)
        }
    }
}
