import SwiftUI

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

    private var modeIconColor: Color {
        switch store.selectedMode {
        case .fullAccess:
            return palette.warning
        case .acceptEdits, .plan:
            return palette.primary
        case .default, .custom:
            return palette.fgSecondary
        }
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
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.top, OrbitSpacing.sm)
        .padding(.bottom, OrbitSpacing.sm)
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
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
        .shadow(color: Color.black.opacity(palette.isDark ? 0.28 : 0.08), radius: 12, y: 4)
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
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
        .shadow(color: Color.black.opacity(palette.isDark ? 0.28 : 0.08), radius: 12, y: 4)
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
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
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
                .padding(.horizontal, OrbitSpacing.md + 2)
                .padding(.top, hasTrayItems ? OrbitSpacing.xs : OrbitSpacing.md)
                .padding(.bottom, OrbitSpacing.xs)

            // Bottom Control Bar
            HStack(spacing: OrbitSpacing.xs) {
                // [+] Context & Slash Picker Trigger
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
                        .frame(width: 28, height: 28)
                        .background(palette.subtleFill)
                        .clipShape(Circle())
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Add Context or Slash Workflow")

                // [Provider Dot + Model · Effort] Capsule
                Button {
                    store.hapticLight()
                    store.activeSheet = .modelPicker
                } label: {
                    HStack(spacing: OrbitSpacing.xs + 2) {
                        Circle()
                            .fill(palette.providerAccent(store.selectedProvider))
                            .frame(width: 6, height: 6)

                        Text(store.composerModelSummaryLabel)
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgSecondary)
                            .lineLimit(1)

                        Image(systemName: "chevron.up.chevron.down")
                            .font(OrbitTypography.iconMicro)
                            .foregroundStyle(palette.fgMuted)
                    }
                    .padding(.horizontal, OrbitSpacing.sm + 2)
                    .padding(.vertical, 5)
                    .background(palette.subtleFill)
                    .clipShape(Capsule())
                    .overlay(Capsule().strokeBorder(palette.hairline, lineWidth: 0.5))
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .layoutPriority(1)
                .accessibilityLabel("Select Provider, Model, and Reasoning Effort")

                Spacer(minLength: 0)

                // [Mode Shield] Menu (Always Ask / Accept Edits / Full Access)
                Menu {
                    Button {
                        store.applySessionMode(.default)
                    } label: {
                        Label(
                            "Always Ask",
                            systemImage: store.selectedMode == .default ? "checkmark" : "shield"
                        )
                    }

                    Button {
                        store.applySessionMode(.acceptEdits)
                    } label: {
                        Label(
                            "Accept Edits",
                            systemImage: store.selectedMode == .acceptEdits ? "checkmark" : "checkmark.shield"
                        )
                    }

                    Button {
                        store.applySessionMode(.fullAccess)
                    } label: {
                        Label(
                            "Full Access",
                            systemImage: store.selectedMode == .fullAccess ? "checkmark" : "bolt.shield"
                        )
                    }

                    Divider()

                    Button {
                        store.applySessionMode(.plan)
                    } label: {
                        Label(
                            "Plan Mode",
                            systemImage: store.selectedMode == .plan ? "checkmark" : "list.bullet.clipboard"
                        )
                    }
                } label: {
                    HStack(spacing: OrbitSpacing.xs) {
                        Image(systemName: OrbitFormatters.sessionModeIcon(store.selectedMode))
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(modeIconColor)
                    }
                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .accessibilityLabel("Access Mode: \(OrbitFormatters.sessionModeLabel(store.selectedMode))")

                // Compact Muted Stop Pill when user is typing a queued prompt during active run
                if isRunningWithDraft {
                    Button {
                        store.interruptActiveAgent()
                    } label: {
                        HStack(spacing: OrbitSpacing.xs) {
                            Image(systemName: "stop.fill")
                                .font(OrbitTypography.iconMicro)
                            Text("Stop")
                                .font(OrbitTypography.meta(.semibold))
                        }
                        .foregroundStyle(palette.danger)
                        .padding(.horizontal, OrbitSpacing.sm)
                        .padding(.vertical, OrbitSpacing.xs)
                        .background(palette.danger.opacity(0.12))
                        .clipShape(Capsule())
                        .overlay(
                            Capsule()
                                .strokeBorder(palette.danger.opacity(0.28), lineWidth: 0.5)
                        )
                        .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Stop Generation")
                }

                // Primary Right Circle Action: Stop ■ / Queue ⤒ / Send ↑
                if showPrimaryStopButton {
                    Button {
                        store.interruptActiveAgent()
                    } label: {
                        ZStack {
                            Circle()
                                .fill(palette.danger)
                                .frame(width: 32, height: 32)
                            Image(systemName: "stop.fill")
                                .font(OrbitTypography.iconMicro)
                                .foregroundStyle(Color.white)
                        }
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
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
                                .foregroundStyle(Color.white)
                        }
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
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
                                .font(OrbitTypography.iconSmall)
                                .foregroundStyle(store.canSubmitComposer ? Color.white : palette.fgMuted)
                        }
                        .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .disabled(!store.canSubmitComposer && store.selectedHostId != nil)
                    .accessibilityLabel("Send Prompt")
                }
            }
            .padding(.horizontal, OrbitSpacing.xs)
            .padding(.bottom, 2)
        }
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
        .shadow(color: Color.black.opacity(palette.isDark ? 0.22 : 0.06), radius: 10, y: 2)
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
