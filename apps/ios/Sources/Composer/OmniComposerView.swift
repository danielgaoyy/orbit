import SwiftUI
import UIKit

private struct InlineMentionCandidate: Identifiable, Equatable {
    let id: String
    let title: String
    let subtitle: String
    let icon: String
    let score: Int
    let item: ComposerContextItem
    let onAttachSideEffect: (@MainActor () -> Void)?

    static func == (lhs: InlineMentionCandidate, rhs: InlineMentionCandidate) -> Bool {
        lhs.id == rhs.id && lhs.title == rhs.title && lhs.subtitle == rhs.subtitle && lhs.item == rhs.item
    }
}

struct OmniComposerView: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.orbitPalette) private var palette
    @FocusState private var isComposerFocused: Bool
    @State private var isSoftwareKeyboardVisible: Bool = false

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
        return hasHomeIndicatorInset ? -14 : 10
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
        store.slashDirective != nil
            || !store.attachedContexts.isEmpty
            || !store.quotedDiffSnippets.isEmpty
    }

    private var isRunningWithDraft: Bool {
        store.isActiveAgentRunning && store.canSubmitComposer && store.selectedAgentId != nil
    }

    private var showPrimaryStopButton: Bool {
        store.isActiveAgentRunning && !store.canSubmitComposer
    }

    private var placeholderText: String {
        if store.hosts.isEmpty && !store.hasDemoStageOverlay {
            return "Pair a host to start…"
        }
        if store.isDraftNewSession {
            return "Ask anything, @file, or /plan…"
        }
        if store.isActiveAgentRunning {
            return "Queue next prompt, @file, or /review…"
        }
        return "Send follow-up or steer session…"
    }

    private var normalizedPermissionMode: FfiSessionMode {
        OrbitFormatters.normalizePermissionMode(store.selectedMode)
    }

    private var permissionModeSelectionBinding: Binding<FfiSessionMode> {
        Binding(
            get: { normalizedPermissionMode },
            set: { newMode in
                store.applySessionMode(newMode)
            }
        )
    }

    // MARK: - Inline `/` Slash & `@` Mention Detection

    private var activeSlashQuery: String? {
        let trimmedLeading = store.composerText.drop(while: { $0.isWhitespace })
        guard trimmedLeading.hasPrefix("/"),
              !trimmedLeading.contains(where: { $0.isWhitespace }) else {
            return nil
        }
        return String(trimmedLeading.dropFirst()).lowercased()
    }

    private var matchingSlashDirectives: [OrbitSlashDirective] {
        guard let q = activeSlashQuery else { return [] }
        if q.isEmpty {
            return OrbitSlashDirective.allCases
        }
        return OrbitSlashDirective.allCases.filter { directive in
            directive.rawValue.hasPrefix(q)
                || directive.rawValue.contains(q)
                || directive.title.lowercased().contains(q)
        }
    }

    private var activeAtTokenRange: Range<String.Index>? {
        let text = store.composerText
        guard !text.isEmpty else { return nil }
        let tokenStart: String.Index
        if let lastWs = text.lastIndex(where: { $0.isWhitespace }) {
            tokenStart = text.index(after: lastWs)
        } else {
            tokenStart = text.startIndex
        }
        guard tokenStart < text.endIndex else { return nil }
        let token = text[tokenStart..<text.endIndex]
        guard token.hasPrefix("@") else { return nil }
        return tokenStart..<text.endIndex
    }

    private var activeAtQuery: String? {
        guard activeSlashQuery == nil, let range = activeAtTokenRange else { return nil }
        return String(store.composerText[range].dropFirst()).lowercased()
    }

    private var matchingMentionCandidates: [InlineMentionCandidate] {
        guard let q = activeAtQuery else { return [] }
        var candidates: [InlineMentionCandidate] = []

        // 1. Built-in `@diff`, `@staged`, `@terminal`
        let builtIns: [(id: String, title: String, subtitle: String, icon: String, keywords: [String], item: ComposerContextItem, sideEffect: (@MainActor () -> Void)?)] = [
            (
                id: "builtin:diff",
                title: "@diff",
                subtitle: "Unstaged working tree diff",
                icon: "arrow.triangle.branch",
                keywords: ["diff", "git", "unstaged", "git-diff", "changes"],
                item: .gitDiff(staged: false, label: "Unstaged Diff"),
                sideEffect: { store.refreshGitReview() }
            ),
            (
                id: "builtin:staged",
                title: "@staged",
                subtitle: "Staged git index changes",
                icon: "arrow.triangle.branch",
                keywords: ["staged", "staged-diff", "index"],
                item: .gitDiff(staged: true, label: "Staged Diff"),
                sideEffect: { store.setGitStagedFilter(true) }
            ),
            (
                id: "builtin:terminal",
                title: "@terminal",
                subtitle: "Visible PTY terminal output",
                icon: "terminal",
                keywords: ["terminal", "pty", "shell", "screen"],
                item: .terminalOutput(
                    label: "Terminal Screen",
                    content: store.captureTerminalSnapshotText() ?? "Inspect active terminal output."
                ),
                sideEffect: nil
            )
        ]

        for entry in builtIns {
            if q.isEmpty {
                candidates.append(
                    InlineMentionCandidate(
                        id: entry.id,
                        title: entry.title,
                        subtitle: entry.subtitle,
                        icon: entry.icon,
                        score: 95,
                        item: entry.item,
                        onAttachSideEffect: entry.sideEffect
                    )
                )
            } else if entry.keywords.contains(where: { $0.hasPrefix(q) }) {
                candidates.append(
                    InlineMentionCandidate(
                        id: entry.id,
                        title: entry.title,
                        subtitle: entry.subtitle,
                        icon: entry.icon,
                        score: 110,
                        item: entry.item,
                        onAttachSideEffect: entry.sideEffect
                    )
                )
            } else if entry.keywords.contains(where: { $0.contains(q) }) {
                candidates.append(
                    InlineMentionCandidate(
                        id: entry.id,
                        title: entry.title,
                        subtitle: entry.subtitle,
                        icon: entry.icon,
                        score: 75,
                        item: entry.item,
                        onAttachSideEffect: entry.sideEffect
                    )
                )
            }
        }

        // 2. Workspace files
        if q.isEmpty {
            for path in store.workspaceFilesList.prefix(6) {
                let short = OrbitFormatters.shortPath(path)
                candidates.append(
                    InlineMentionCandidate(
                        id: "file:\(path)",
                        title: "@\(short)",
                        subtitle: path,
                        icon: "doc.text",
                        score: 60,
                        item: .file(path: path),
                        onAttachSideEffect: nil
                    )
                )
            }
        } else {
            for path in store.workspaceFilesList {
                let lower = path.lowercased()
                let short = OrbitFormatters.shortPath(path)
                let lowerShort = short.lowercased()
                let fileScore: Int?
                if lowerShort == q {
                    fileScore = 105
                } else if lowerShort.hasPrefix(q) {
                    fileScore = 90
                } else if lowerShort.contains(q) {
                    fileScore = 70
                } else if lower.contains(q) {
                    fileScore = 50
                } else if isFuzzySubsequence(needle: q, haystack: lower) {
                    fileScore = 30
                } else {
                    fileScore = nil
                }

                if let score = fileScore {
                    candidates.append(
                        InlineMentionCandidate(
                            id: "file:\(path)",
                            title: "@\(short)",
                            subtitle: path,
                            icon: "doc.text",
                            score: score,
                            item: .file(path: path),
                            onAttachSideEffect: nil
                        )
                    )
                }
            }
        }

        return Array(
            candidates
                .sorted { lhs, rhs in
                    if lhs.score != rhs.score { return lhs.score > rhs.score }
                    return lhs.subtitle.count < rhs.subtitle.count
                }
                .prefix(6)
        )
    }

    var body: some View {
        OrbitGlassContainer(spacing: OrbitSpacing.sm) {
            VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                if !matchingSlashDirectives.isEmpty {
                    slashAutocompletePopover
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                } else if !matchingMentionCandidates.isEmpty {
                    mentionAutocompletePopover
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }

                if !store.activeQueuedPrompts.isEmpty {
                    queuedPromptsTray
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }

                composerCard
            }
        }
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.top, OrbitSpacing.sm)
        .padding(.bottom, composerBottomPadding)
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
            if activeAtQuery != nil && store.workspaceFilesList.isEmpty {
                store.refreshWorkspaceFiles()
            }
        }
        .onChange(of: store.composerText) { _ in
            if activeAtQuery != nil && store.workspaceFilesList.isEmpty {
                store.refreshWorkspaceFiles()
            }
        }
    }

    // MARK: - Floating `/` Slash Autocomplete Popover

    private var slashAutocompletePopover: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(matchingSlashDirectives.enumerated()), id: \.element.id) { idx, directive in
                if idx > 0 {
                    OrbitHairline()
                }
                Button {
                    selectInlineSlashDirective(directive)
                } label: {
                    HStack(spacing: OrbitSpacing.md) {
                        Image(systemName: directive.icon)
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(palette.primary)
                            .frame(width: 20)

                        Text(directive.badgeLabel)
                            .font(OrbitTypography.code(.semibold))
                            .foregroundStyle(palette.fgPrimary)

                        Text(directive.subtitle)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgSecondary)
                            .lineLimit(1)

                        Spacer(minLength: 0)

                        Image(systemName: "arrow.turn.down.left")
                            .font(OrbitTypography.iconMicro)
                            .foregroundStyle(palette.fgMuted)
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Slash Workflow \(directive.badgeLabel)")
            }
        }
        .padding(.vertical, OrbitSpacing.xs)
        .orbitGlassSurface(
            in: RoundedRectangle(cornerRadius: 18, style: .continuous),
            fallbackFill: palette.elevated,
            fallbackBorder: palette.hairline,
            fallbackShadowOpacity: palette.isDark ? 0.28 : 0.08
        )
    }

    // MARK: - Floating `@` Mention Autocomplete Popover

    private var mentionAutocompletePopover: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(matchingMentionCandidates.enumerated()), id: \.element.id) { idx, candidate in
                if idx > 0 {
                    OrbitHairline()
                }
                let isAttached = store.attachedContexts.contains(candidate.item)
                Button {
                    selectInlineMentionCandidate(candidate)
                } label: {
                    HStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: candidate.icon)
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(isAttached ? palette.primary : palette.fgSecondary)
                            .frame(width: 20)

                        VStack(alignment: .leading, spacing: 1) {
                            Text(candidate.title)
                                .font(OrbitTypography.code(.semibold))
                                .foregroundStyle(palette.fgPrimary)
                                .lineLimit(1)

                            Text(candidate.subtitle)
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }

                        Spacer(minLength: 0)

                        Image(systemName: isAttached ? "checkmark.circle.fill" : "plus.circle")
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(isAttached ? palette.primary : palette.fgMuted)
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .padding(.vertical, OrbitSpacing.xs)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Attach \(candidate.title)")
            }
        }
        .padding(.vertical, OrbitSpacing.xs)
        .orbitGlassSurface(
            in: RoundedRectangle(cornerRadius: 18, style: .continuous),
            fallbackFill: palette.elevated,
            fallbackBorder: palette.hairline,
            fallbackShadowOpacity: palette.isDark ? 0.28 : 0.08
        )
    }

    private func selectInlineSlashDirective(_ directive: OrbitSlashDirective) {
        store.applyQuickWorkflow(directive)
        store.composerText = ""
    }

    private func selectInlineMentionCandidate(_ candidate: InlineMentionCandidate) {
        candidate.onAttachSideEffect?()
        if !store.attachedContexts.contains(candidate.item) {
            store.attachedContexts.append(candidate.item)
        }
        if let range = activeAtTokenRange {
            var updated = store.composerText
            updated.removeSubrange(range)
            if updated.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                updated = ""
            }
            store.composerText = updated
        }
        store.hapticLight()
    }

    // MARK: - Queued Prompts Tray

    private var queuedPromptsTray: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(store.activeQueuedPrompts.enumerated()), id: \.element.id) { idx, item in
                if idx > 0 {
                    OrbitHairline()
                }
                HStack(spacing: OrbitSpacing.sm) {
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

                    Button {
                        store.editQueuedPrompt(item)
                        isComposerFocused = true
                    } label: {
                        Text("Edit")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgSecondary)
                            .padding(.horizontal, OrbitSpacing.sm)
                            .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
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

            // Auto-expanding 1..6 line TextField
            TextField(placeholderText, text: $store.composerText, axis: .vertical)
                .font(OrbitTypography.body(.regular))
                .lineSpacing(OrbitTypography.bodyLineSpacing)
                .foregroundStyle(palette.fgPrimary)
                .lineLimit(1...6)
                .focused($isComposerFocused)
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.top, hasTrayItems ? OrbitSpacing.xs : OrbitSpacing.md + 2)
                .padding(.bottom, 2)

            // Bottom Control Bar — balanced optical spacing (~12pt paired gaps, concentric 10pt Send inset)
            HStack(spacing: 0) {
                // [+] Context & Slash Picker Trigger (borderless, left edge aligned at 16pt)
                Button {
                    store.hapticLight()
                    if store.selectedHostId == nil && !store.hasDemoStageOverlay {
                        store.activeSheet = .qrPair
                    } else {
                        store.refreshWorkspaceFiles()
                        store.activeSheet = .contextPicker
                    }
                } label: {
                    Image(systemName: "plus")
                        .font(OrbitTypography.icon)
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 34, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Add Context or Slash Workflow")

                // [Agent Icon + Model + Effort ⌄] (aligned 1:1 with Desktop ComposerModelPicker)
                Button {
                    store.hapticLight()
                    store.activeSheet = .modelPicker
                } label: {
                    HStack(spacing: 6) {
                        OrbitProviderIcon(provider: store.selectedProvider, size: 16)

                        Text(store.composerModelDisplayName)
                            .font(.system(size: 13.5, weight: .regular))
                            .foregroundStyle(palette.fgPrimary.opacity(0.88))
                            .lineLimit(1)

                        if let effortLabel = store.composerEffortDisplayLabel {
                            Text(effortLabel)
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
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .layoutPriority(1)
                .accessibilityLabel("Select Provider, Model, and Reasoning Effort")

                Spacer(minLength: OrbitSpacing.xs)

                // [Access Permissions] Menu (aligned with Desktop: Always Ask / Accept Edits / Full Access)
                Menu {
                    Section("Access Permissions") {
                        Picker("Access Permissions", selection: permissionModeSelectionBinding) {
                            Label {
                                Text("Always Ask")
                                Text("Require approval before file edits and commands")
                            } icon: {
                                Image(systemName: "shield")
                            }
                            .tag(FfiSessionMode.default)

                            Label {
                                Text("Accept Edits")
                                Text("Auto-approve file edits, ask before commands")
                            } icon: {
                                Image(systemName: "checkmark.shield")
                            }
                            .tag(FfiSessionMode.acceptEdits)

                            Label {
                                Text("Full Access")
                                Text("Edit files and run commands without prompts")
                            } icon: {
                                Image(systemName: "shield.slash")
                            }
                            .tag(FfiSessionMode.fullAccess)
                        }
                    }
                } label: {
                    Image(systemName: OrbitFormatters.sessionModeIcon(normalizedPermissionMode))
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 36, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .menuOrder(.fixed)
                .accessibilityLabel("Access Permissions: \(OrbitFormatters.sessionModeLabel(normalizedPermissionMode))")

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
                        .frame(minHeight: OrbitSpacing.hitTarget)
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
                        .frame(width: 36, height: OrbitSpacing.hitTarget)
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
                        .frame(width: 36, height: OrbitSpacing.hitTarget)
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
                        .frame(width: 36, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .disabled(!store.canSubmitComposer && store.selectedHostId != nil)
                    .accessibilityLabel("Send Prompt")
                }
            }
            .padding(.leading, 6)
            .padding(.trailing, 8)
            .padding(.bottom, 4)
        }
        .orbitGlassSurface(
            in: RoundedRectangle(cornerRadius: 26, style: .continuous),
            fallbackFill: palette.elevated,
            fallbackBorder: palette.hairline,
            fallbackShadowOpacity: palette.isDark ? 0.22 : 0.06
        )
    }

    // MARK: - Removable Context Chips Tray

    private var attachmentsTray: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: OrbitSpacing.xs) {
                if let directive = store.slashDirective {
                    removableChip(
                        icon: directive.icon,
                        label: directive.badgeLabel,
                        isAccent: true
                    ) {
                        store.slashDirective = nil
                        store.hapticLight()
                    }
                }

                ForEach(store.attachedContexts) { ctx in
                    removableChip(
                        icon: ctx.icon,
                        label: ctx.chipLabel,
                        isAccent: false
                    ) {
                        store.removeContextItem(ctx)
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

    private func removableChip(
        icon: String,
        label: String,
        isAccent: Bool,
        onRemove: @escaping () -> Void
    ) -> some View {
        Button(action: onRemove) {
            HStack(spacing: OrbitSpacing.xs) {
                Image(systemName: icon)
                    .font(OrbitTypography.iconMicro)
                Text(label)
                    .font(OrbitTypography.code(.medium))
                    .lineLimit(1)
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

    private func isFuzzySubsequence(needle: String, haystack: String) -> Bool {
        var hIdx = haystack.startIndex
        for ch in needle {
            guard let found = haystack[hIdx...].firstIndex(of: ch) else {
                return false
            }
            hIdx = haystack.index(after: found)
        }
        return true
    }
}
