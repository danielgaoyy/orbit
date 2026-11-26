import SwiftUI

/// Modal sheets for spawning a Mission Workspace Peer (`+ Peer`) and creating a Curated Mission Workspace.
struct SpawnPeerSheet: View {
    @ObservedObject var store: OrbitIosStore
    let workspaceId: String

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var handle: String = ""
    @State private var provider: FfiProviderKind = .claudeCode
    @State private var mode: FfiSessionMode = .acceptEdits
    @State private var useWorktree: Bool = false
    @State private var prompt: String = ""

    private var availableProviders: [FfiProviderKind] {
        let live = store.activeHost?.availableProviders ?? []
        return live.isEmpty ? [.claudeCode, .codex, .antigravity] : live
    }

    private var workspaceTitle: String {
        store.currentHostDetail?.workspaces.first(where: { $0.workspaceId == workspaceId })?.title
            ?? "Mission Workspace"
    }

    private var workspaceCwd: String? {
        store.currentHostDetail?.workspaces.first(where: { $0.workspaceId == workspaceId })?.cwd
    }

    private var isWorkspaceGitRepo: Bool {
        store.isActiveFolderGitRepo(hostId: store.selectedHostId, cwd: workspaceCwd ?? store.activeCwd)
    }

    var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                    // Quick 1-Tap Presets
                    VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                        Text("Quick Presets")
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.fgMuted)
                            .padding(.horizontal, 4)

                        HStack(spacing: OrbitSpacing.sm) {
                            presetCard(
                                title: "Code Reviewer",
                                subtitle: "Read-only second opinion",
                                icon: "doc.text.magnifyingglass",
                                tint: palette.accentViolet
                            ) {
                                handle = "reviewer"
                                mode = .default
                                useWorktree = false
                                prompt = "Review the recent changes and shared blackboard decisions in this workspace. Identify bugs, regressions, edge cases, and architectural mismatches without editing files."
                            }

                            presetCard(
                                title: "Worktree Worker",
                                subtitle: "Parallel isolated branch",
                                icon: "arrow.triangle.branch",
                                tint: palette.accentSky
                            ) {
                                handle = "worker"
                                mode = .acceptEdits
                                useWorktree = true
                                prompt = "Read BOARD.md and pick up the next independent task on an isolated git worktree."
                            }
                        }
                    }

                    // Configuration Form
                    VStack(spacing: 0) {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Peer Handle (optional)")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                            TextField("e.g. reviewer, backend, tester", text: $handle)
                                .font(OrbitTypography.subhead(.regular))
                                .foregroundStyle(palette.fgPrimary)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                        }
                        .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        VStack(alignment: .leading, spacing: 8) {
                            Text("Provider")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                            HStack(spacing: 6) {
                                ForEach(availableProviders, id: \.self) { prov in
                                    let selected = provider == prov
                                    Button {
                                        store.hapticLight()
                                        provider = prov
                                    } label: {
                                        HStack(spacing: 5) {
                                            OrbitProviderIcon(provider: prov, size: 13)
                                            Text(OrbitFormatters.providerShortLabel(prov))
                                                .font(OrbitTypography.meta(selected ? .semibold : .medium))
                                                .lineLimit(1)
                                        }
                                        .foregroundStyle(selected ? palette.fgPrimary : palette.fgSecondary)
                                        .frame(maxWidth: .infinity, minHeight: 34)
                                        .background(
                                            RoundedRectangle(cornerRadius: 9, style: .continuous)
                                                .fill(selected ? palette.activeRowFill : palette.subtleFill.opacity(0.6))
                                        )
                                        .overlay(
                                            RoundedRectangle(cornerRadius: 9, style: .continuous)
                                                .strokeBorder(selected ? palette.primary.opacity(0.35) : Color.clear, lineWidth: 0.75)
                                        )
                                    }
                                    .buttonStyle(.plain)
                                }
                            }
                        }
                        .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        VStack(alignment: .leading, spacing: 8) {
                            Text("Access Permissions")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                            HStack(spacing: 6) {
                                modeChip(.default, label: "Always Ask")
                                modeChip(.acceptEdits, label: "Accept Edits")
                                modeChip(.fullAccess, label: "Full Access")
                            }
                        }
                        .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        Toggle(isOn: $useWorktree) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text("Isolated Git Worktree")
                                    .font(OrbitTypography.subhead(.medium))
                                    .foregroundStyle(isWorkspaceGitRepo ? palette.fgPrimary : palette.fgMuted)
                                Text(
                                    isWorkspaceGitRepo
                                        ? "Run peer on its own branch to avoid file conflicts"
                                        : "Git repository required for isolated worktrees"
                                )
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                            }
                        }
                        .tint(palette.direct)
                        .disabled(!isWorkspaceGitRepo)
                        .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        VStack(alignment: .leading, spacing: 6) {
                            Text("Initial Task Prompt")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                            TextEditor(text: $prompt)
                                .font(OrbitTypography.body(.regular))
                                .foregroundStyle(palette.fgPrimary)
                                .scrollContentBackground(.hidden)
                                .frame(minHeight: 96)
                                .padding(8)
                                .background(palette.subtleFill)
                                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }
                        .padding(OrbitSpacing.md)
                    }
                    .background(palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 14, style: .continuous)
                            .strokeBorder(palette.hairline, lineWidth: 0.5)
                    )

                    Button {
                        let cleanPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !cleanPrompt.isEmpty else { return }
                        let allowWorktree = OrbitComposerLogic.canCreateWorktreeSession(
                            isWorktreeMode: useWorktree,
                            isGitRepo: isWorkspaceGitRepo
                        ) ? useWorktree : false
                        store.spawnWorkspacePeer(
                            workspaceId: workspaceId,
                            provider: provider,
                            model: nil,
                            effort: nil,
                            mode: mode,
                            handle: handle,
                            initialPrompt: cleanPrompt,
                            useWorktree: allowWorktree
                        )
                        dismiss()
                    } label: {
                        Text("Spawn Peer in \(workspaceTitle)")
                            .font(OrbitTypography.subhead(.semibold))
                            .foregroundStyle(palette.onPrimary)
                            .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                            .background(
                                prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                                    ? palette.fgMuted.opacity(0.35)
                                    : palette.primary,
                                in: RoundedRectangle(cornerRadius: 12, style: .continuous)
                            )
                    }
                    .buttonStyle(.plain)
                    .disabled(prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.md)
            }
            .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
            .navigationTitle("Add Workspace Peer")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                        .foregroundStyle(palette.fgSecondary)
                }
            }
            .onAppear {
                provider = store.selectedProvider
                mode = OrbitFormatters.normalizePermissionMode(store.selectedMode)
                if !isWorkspaceGitRepo {
                    useWorktree = false
                }
            }
        }
    }

    private func presetCard(
        title: String,
        subtitle: String,
        icon: String,
        tint: Color,
        action: @escaping () -> Void
    ) -> some View {
        Button {
            store.hapticLight()
            action()
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                Image(systemName: icon)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(tint)
                Text(title)
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)
                Text(subtitle)
                    .font(.system(size: 11.5))
                    .foregroundStyle(palette.fgMuted)
                    .lineLimit(2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(OrbitSpacing.md)
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
        .buttonStyle(.plain)
    }

    private func modeChip(_ target: FfiSessionMode, label: String) -> some View {
        let selected = OrbitFormatters.normalizePermissionMode(mode) == target
        return Button {
            store.hapticLight()
            mode = target
        } label: {
            Text(label)
                .font(OrbitTypography.meta(selected ? .semibold : .medium))
                .foregroundStyle(selected ? palette.fgPrimary : palette.fgSecondary)
                .frame(maxWidth: .infinity, minHeight: 32)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(selected ? palette.activeRowFill : palette.subtleFill.opacity(0.6))
                )
        }
        .buttonStyle(.plain)
    }
}

struct CreateWorkspaceSheet: View {
    @ObservedObject var store: OrbitIosStore
    let projectRoot: String
    let projectName: String
    let isGitRepo: Bool
    let moveAgentId: String?

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var title: String = ""
    @State private var missionGoal: String = ""
    @State private var useWorktree: Bool = false

    var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                    VStack(spacing: 0) {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Workspace Title")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                            TextField("e.g. iOS UI Parity & v0.7.3 Release", text: $title)
                                .font(OrbitTypography.subhead(.regular))
                                .foregroundStyle(palette.fgPrimary)
                        }
                        .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        VStack(alignment: .leading, spacing: 6) {
                            Text("Mission Goal (optional)")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                            TextField("Shared objective visible on BOARD.md", text: $missionGoal, axis: .vertical)
                                .lineLimit(2...4)
                                .font(OrbitTypography.subhead(.regular))
                                .foregroundStyle(palette.fgPrimary)
                        }
                        .padding(OrbitSpacing.md)

                        if moveAgentId == nil {
                            OrbitHairline().padding(.leading, OrbitSpacing.md)
                            Toggle(isOn: $useWorktree) {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text("Create Git Worktree")
                                        .font(OrbitTypography.subhead(.medium))
                                        .foregroundStyle(isGitRepo ? palette.fgPrimary : palette.fgMuted)
                                    Text(
                                        isGitRepo
                                            ? "Isolate workspace files on a dedicated branch"
                                            : "Git repository required to create a worktree"
                                    )
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                }
                            }
                            .tint(palette.direct)
                            .disabled(!isGitRepo)
                            .padding(OrbitSpacing.md)
                        }
                    }
                    .background(palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 14, style: .continuous)
                            .strokeBorder(palette.hairline, lineWidth: 0.5)
                    )

                    Text("Project: \(projectName) (\(OrbitFormatters.compactPath(projectRoot)))")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .padding(.horizontal, 4)

                    Button {
                        let clean = title.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !clean.isEmpty else { return }
                        let allowWorktree = moveAgentId == nil
                            && OrbitComposerLogic.canCreateWorktreeSession(
                                isWorktreeMode: useWorktree,
                                isGitRepo: isGitRepo
                            )
                            && useWorktree
                        store.createMissionWorkspace(
                            sourceCwd: projectRoot,
                            title: clean,
                            missionGoal: missionGoal,
                            useWorktree: allowWorktree,
                            moveAgentId: moveAgentId
                        )
                        dismiss()
                    } label: {
                        Text(moveAgentId != nil ? "Create & Move Session" : "Create Mission Workspace")
                            .font(OrbitTypography.subhead(.semibold))
                            .foregroundStyle(palette.onPrimary)
                            .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                            .background(
                                title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                                    ? palette.fgMuted.opacity(0.35)
                                    : palette.primary,
                                in: RoundedRectangle(cornerRadius: 12, style: .continuous)
                            )
                    }
                    .buttonStyle(.plain)
                    .disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.md)
            }
        .scrollDismissesKeyboard(.interactively)
        .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
        .navigationTitle("New Workspace")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button("Cancel") { dismiss() }
                    .foregroundStyle(palette.fgSecondary)
            }
        }
        }
    }
}

struct WorktreeHandoffSheet: View {
    @ObservedObject var store: OrbitIosStore
    let workspaceId: String

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var direction: FfiWorktreeHandoffDirection = .worktreeToLocal
    @State private var mode: FfiWorktreeHandoffMode = .apply
    @State private var includeCommitted: Bool = true
    @State private var selectedSourceAgentId: String? = nil
    @State private var preview: FfiWorktreeHandoffPreview? = nil
    @State private var isLoading: Bool = false
    @State private var isExecuting: Bool = false
    @State private var errorMessage: String? = nil

    private var workspace: FfiWorkspaceSummary? {
        store.currentHostDetail?.workspaces.first(where: { $0.workspaceId == workspaceId })
    }

    private var worktreePeers: [FfiAgentCard] {
        (store.currentHostDetail?.agents ?? []).filter {
            $0.workspaceId == workspaceId
                && !($0.worktreeBranch?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ?? true)
        }
    }

    private var effectiveSourceAgentId: String? {
        guard direction == .worktreeToLocal else { return nil }
        if let selected = selectedSourceAgentId,
           worktreePeers.contains(where: { $0.agentId == selected }) {
            return selected
        }
        if workspace?.isWorktree != true {
            return worktreePeers.first?.agentId
        }
        return nil
    }

    var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                    VStack(spacing: 0) {
                        Picker("Direction", selection: $direction) {
                            Text("Worktree → Local").tag(FfiWorktreeHandoffDirection.worktreeToLocal)
                            Text("Local → Worktree").tag(FfiWorktreeHandoffDirection.localToWorktree)
                        }
                        .pickerStyle(.segmented)
                        .padding(OrbitSpacing.md)

                        if direction == .worktreeToLocal,
                           !worktreePeers.isEmpty,
                           (workspace?.isWorktree != true || worktreePeers.count > 1) {
                            sourceWorktreePickerRow
                        }

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        Picker("Mode", selection: $mode) {
                            Text("Apply Copy").tag(FfiWorktreeHandoffMode.apply)
                            Text("Move & Clean").tag(FfiWorktreeHandoffMode.move)
                        }
                        .pickerStyle(.segmented)
                        .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        Toggle("Include Committed Branch Changes", isOn: $includeCommitted)
                            .font(OrbitTypography.subhead(.regular))
                            .tint(palette.direct)
                            .padding(OrbitSpacing.md)
                    }
                    .background(palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))

                    if let err = errorMessage {
                        Text(err)
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.danger)
                            .padding(OrbitSpacing.md)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(palette.danger.opacity(0.10), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                    }

                    if isLoading {
                        HStack(spacing: OrbitSpacing.sm) {
                            OrbitSpinnerRingView(color: palette.primary, size: 15)
                            Text("Analyzing git worktree changes…")
                                .font(OrbitTypography.subhead(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, OrbitSpacing.lg)
                    } else if let preview {
                        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                            HStack {
                                let count = preview.files.count
                                Text("\(count) \(count == 1 ? "file" : "files") · +\(preview.totalInsertions) -\(preview.totalDeletions)")
                                    .font(OrbitTypography.subhead(.semibold))
                                    .foregroundStyle(palette.fgPrimary)
                                Spacer()
                                if let srcBr = preview.sourceBranch, let dstBr = preview.targetBranch {
                                    Text("\(srcBr) → \(dstBr)")
                                        .font(OrbitTypography.code(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                }
                            }

                            if let warn = preview.warning, !warn.isEmpty {
                                Text(warn)
                                    .font(OrbitTypography.meta(.medium))
                                    .foregroundStyle(palette.warning)
                            }

                            if !preview.conflicts.isEmpty {
                                Text("Potential conflicts: \(preview.conflicts.joined(separator: ", "))")
                                    .font(OrbitTypography.meta(.medium))
                                    .foregroundStyle(palette.danger)
                            }

                            ForEach(preview.files.prefix(30), id: \.path) { file in
                                HStack {
                                    Text(file.path)
                                        .font(OrbitTypography.code(.regular))
                                        .foregroundStyle(palette.fgPrimary)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                    Spacer()
                                    Text("+\(file.insertions) -\(file.deletions)")
                                        .font(OrbitTypography.meta(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                }
                                .padding(.vertical, 2)
                            }
                        }
                        .padding(OrbitSpacing.md)
                        .background(palette.elevated)
                        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
                    }

                    Button {
                        runExecute()
                    } label: {
                        HStack {
                            if isExecuting { ProgressView().tint(palette.onPrimary) }
                            Text(mode == .move ? "Move Changes & Handoff" : "Apply Changes")
                                .font(OrbitTypography.subhead(.semibold))
                        }
                        .foregroundStyle(palette.onPrimary)
                        .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                        .background(
                            (isExecuting || isLoading || (preview?.files.isEmpty ?? true))
                                ? palette.fgMuted.opacity(0.4)
                                : palette.primary,
                            in: RoundedRectangle(cornerRadius: 12, style: .continuous)
                        )
                    }
                    .buttonStyle(.plain)
                    .disabled(isExecuting || isLoading || (preview?.files.isEmpty ?? true))
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.md)
            }
            .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
            .navigationTitle("Worktree Handoff")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button(OrbitL10n.tr("Cancel")) { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        loadPreview()
                    } label: {
                        Image(systemName: "arrow.clockwise")
                    }
                }
            }
            .onAppear {
                if selectedSourceAgentId == nil {
                    if let active = store.activeAgent,
                       active.workspaceId == workspaceId,
                       worktreePeers.contains(where: { $0.agentId == active.agentId }) {
                        selectedSourceAgentId = active.agentId
                    } else if workspace?.isWorktree != true {
                        selectedSourceAgentId = worktreePeers.first?.agentId
                    }
                }
                loadPreview()
            }
            .onChange(of: direction) { _ in loadPreview() }
            .onChange(of: includeCommitted) { _ in loadPreview() }
            .onChange(of: selectedSourceAgentId) { _ in loadPreview() }
        }
    }

    @ViewBuilder
    private var sourceWorktreePickerRow: some View {
        let activePeer = worktreePeers.first(where: { $0.agentId == effectiveSourceAgentId }) ?? worktreePeers.first
        OrbitHairline().padding(.leading, OrbitSpacing.md)

        HStack(spacing: OrbitSpacing.md) {
            Text("Source Worktree")
                .font(OrbitTypography.subhead(.regular))
                .foregroundStyle(palette.fgPrimary)
                .fixedSize(horizontal: true, vertical: false)
            Spacer(minLength: OrbitSpacing.sm)
            Menu {
                Picker("Source Worktree", selection: sourceAgentBinding) {
                    ForEach(worktreePeers, id: \.agentId) { peer in
                        Text(peerSourceLabel(peer)).tag(peer.agentId)
                    }
                }
            } label: {
                HStack(spacing: 4) {
                    Text(activePeer.map(peerSourceLabel) ?? "Select worktree")
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.primary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Image(systemName: "chevron.up.chevron.down")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(palette.primary)
                }
                .frame(maxWidth: .infinity, alignment: .trailing)
                .contentShape(Rectangle())
            }
        }
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.vertical, 10)
    }

    private var sourceAgentBinding: Binding<String> {
        Binding<String>(
            get: { effectiveSourceAgentId ?? "" },
            set: { selectedSourceAgentId = $0.isEmpty ? nil : $0 }
        )
    }

    private func peerSourceLabel(_ peer: FfiAgentCard) -> String {
        let handle = peer.handle?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let cleanHandle = handle.trimmingCharacters(in: CharacterSet(charactersIn: "@"))
        let titleFallback = peer.title.trimmingCharacters(in: .whitespacesAndNewlines)
        let prefix = cleanHandle.isEmpty ? (titleFallback.isEmpty ? "Peer" : titleFallback) : "@\(cleanHandle)"
        let branch = peer.worktreeBranch ?? "worktree"
        return "\(prefix) · \(branch)"
    }

    private func loadPreview() {
        isLoading = true
        errorMessage = nil
        let agentId = effectiveSourceAgentId
        Task {
            do {
                let res = try await store.previewWorktreeHandoff(
                    workspaceId: workspaceId,
                    sourceAgentId: agentId,
                    direction: direction,
                    includeCommitted: includeCommitted
                )
                preview = res
                isLoading = false
            } catch {
                errorMessage = OrbitErrorFormatter.humanize(error)
                isLoading = false
            }
        }
    }

    private func runExecute() {
        isExecuting = true
        errorMessage = nil
        let agentId = effectiveSourceAgentId
        Task {
            do {
                _ = try await store.executeWorktreeHandoff(
                    workspaceId: workspaceId,
                    sourceAgentId: agentId,
                    direction: direction,
                    mode: mode,
                    includeCommitted: includeCommitted
                )
                isExecuting = false
                dismiss()
            } catch {
                errorMessage = OrbitErrorFormatter.humanize(error)
                isExecuting = false
            }
        }
    }
}
