import SwiftUI

private enum AutomationFilterKind: String, CaseIterable, Identifiable {
    case all = "All"
    case schedules = "Schedules"
    case heartbeats = "Heartbeats"

    var id: String { rawValue }
}

/// Desktop `AutomationsView` parity — Manage daemon Cron Schedules & Heartbeats,
/// inspect Run History, trigger Run Now, Pause/Resume, and create new scheduled jobs.
struct AutomationsSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var expandedScheduleIds: Set<String> = []
    @State private var isCreatePresented = false
    @State private var confirmDeleteSchedule: FfiScheduleSpec? = nil
    @State private var filterKind: AutomationFilterKind = .all

    private var schedules: [FfiScheduleSpec] {
        store.currentHostDetail?.schedules ?? []
    }

    private var filteredSchedules: [FfiScheduleSpec] {
        switch filterKind {
        case .all:
            return schedules
        case .schedules:
            return schedules.filter { $0.kind == .schedule }
        case .heartbeats:
            return schedules.filter { $0.kind == .heartbeat }
        }
    }

    var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                    if schedules.isEmpty {
                        emptyStateView
                    } else {
                        Picker("Filter", selection: $filterKind) {
                            ForEach(AutomationFilterKind.allCases) { kind in
                                Text(kind.rawValue).tag(kind)
                            }
                        }
                        .pickerStyle(.segmented)

                        if filteredSchedules.isEmpty {
                            Text("No \(filterKind.rawValue.lowercased()) configured on this host.")
                                .font(OrbitTypography.subhead(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .frame(maxWidth: .infinity, alignment: .center)
                                .padding(.vertical, 32)
                        } else {
                            VStack(spacing: OrbitSpacing.sm) {
                                ForEach(filteredSchedules) { sched in
                                    scheduleCard(sched)
                                }
                            }
                        }
                    }
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.md)
            }
            .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
            .navigationTitle("Automations")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button {
                        store.hapticLight()
                        isCreatePresented = true
                    } label: {
                        Image(systemName: "plus")
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(palette.primary)
                    }
                    .accessibilityLabel("New Automation")
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
            .sheet(isPresented: $isCreatePresented) {
                CreateAutomationSheet(store: store)
                    .environment(\.orbitPalette, palette)
                    .presentationDetents([.large])
                    .presentationDragIndicator(.visible)
            }
            .confirmationDialog(
                "Delete automation?",
                isPresented: Binding(
                    get: { confirmDeleteSchedule != nil },
                    set: { if !$0 { confirmDeleteSchedule = nil } }
                ),
                titleVisibility: .visible
            ) {
                if let sched = confirmDeleteSchedule {
                    Button("Delete \"\(sched.name)\"", role: .destructive) {
                        store.deleteSchedule(sched.scheduleId)
                        confirmDeleteSchedule = nil
                    }
                }
                Button("Cancel", role: .cancel) { confirmDeleteSchedule = nil }
            }
        }
    }

    private var emptyStateView: some View {
        VStack(spacing: OrbitSpacing.md) {
            Image(systemName: "clock.arrow.circlepath")
                .font(.system(size: 28, weight: .medium))
                .foregroundStyle(palette.fgMuted)

            Text("No Automations Yet")
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)

            Text("Run recurring checks, nightly reviews, or scheduled agent runs on your host daemon.")
                .orbitSubheadStyle()
                .foregroundStyle(palette.fgSecondary)
                .multilineTextAlignment(.center)

            Button {
                store.hapticLight()
                isCreatePresented = true
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "plus")
                    Text("Create Automation")
                }
                .font(OrbitTypography.subhead(.semibold))
                .foregroundStyle(palette.onPrimary)
                .padding(.horizontal, 18)
                .frame(height: 40)
                .background(palette.primary, in: Capsule())
            }
            .buttonStyle(.plain)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 48)
    }

    private func scheduleCard(_ sched: FfiScheduleSpec) -> some View {
        let isExpanded = expandedScheduleIds.contains(sched.scheduleId)
        let runs = store.scheduleRuns(for: sched.scheduleId)
        let isActive = sched.status == .active
        let isHeartbeat = sched.kind == .heartbeat

        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(alignment: .top, spacing: OrbitSpacing.sm) {
                Image(systemName: isHeartbeat ? "heart.text.square" : "clock.badge.checkmark")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(isActive ? palette.direct : palette.fgMuted)
                    .padding(.top, 2)

                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        Text(sched.name)
                            .font(OrbitTypography.subhead(.semibold))
                            .foregroundStyle(palette.fgPrimary)
                            .lineLimit(1)

                        Text(statusLabel(sched.status))
                            .font(.system(size: 10.5, weight: .semibold))
                            .foregroundStyle(isActive ? palette.direct : palette.fgMuted)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 1.5)
                            .background((isActive ? palette.direct : palette.fgMuted).opacity(0.14), in: Capsule())
                    }

                    Text("\(sched.cronExpr) · \(sched.runCount) \(sched.runCount == 1 ? "run" : "runs")")
                        .font(OrbitTypography.code(.regular))
                        .foregroundStyle(palette.fgMuted)

                    Text(sched.prompt)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgSecondary)
                        .lineLimit(2)
                }

                Spacer(minLength: 4)
            }

            HStack(spacing: OrbitSpacing.sm) {
                if sched.status == .active || sched.status == .paused {
                    Button {
                        if isActive {
                            store.pauseSchedule(sched.scheduleId)
                        } else {
                            store.resumeSchedule(sched.scheduleId)
                        }
                    } label: {
                        Label(isActive ? "Pause" : "Resume", systemImage: isActive ? "pause.fill" : "play.fill")
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.fgPrimary)
                            .padding(.horizontal, 10)
                            .frame(height: 28)
                            .background(palette.subtleFill, in: Capsule())
                    }
                    .buttonStyle(.plain)
                }

                Button {
                    store.runScheduleOnce(sched.scheduleId)
                } label: {
                    Label("Run Now", systemImage: "bolt.fill")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.accentSky)
                        .padding(.horizontal, 10)
                        .frame(height: 28)
                        .background(palette.accentSky.opacity(0.14), in: Capsule())
                }
                .buttonStyle(.plain)

                Spacer()

                Button {
                    store.hapticLight()
                    withAnimation(.easeInOut(duration: 0.18)) {
                        if isExpanded {
                            expandedScheduleIds.remove(sched.scheduleId)
                        } else {
                            expandedScheduleIds.insert(sched.scheduleId)
                            store.fetchScheduleRuns(sched.scheduleId)
                        }
                    }
                } label: {
                    Text(isExpanded ? "Hide Runs" : "History")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgSecondary)
                }
                .buttonStyle(.plain)

                Button {
                    confirmDeleteSchedule = sched
                } label: {
                    Image(systemName: "trash")
                        .font(.system(size: 12))
                        .foregroundStyle(palette.danger)
                        .frame(width: 28, height: 28)
                }
                .buttonStyle(.plain)
            }

            if isExpanded {
                OrbitHairline()
                if runs.isEmpty {
                    Text("No execution history yet.")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                } else {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(Array(runs.prefix(8))) { run in
                            HStack(alignment: .top, spacing: 8) {
                                Circle()
                                    .fill(runStatusColor(run.status))
                                    .frame(width: 6, height: 6)
                                    .padding(.top, 5)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(run.summary ?? run.error ?? "\(run.status)")
                                        .font(OrbitTypography.meta(.regular))
                                        .foregroundStyle(palette.fgPrimary)
                                        .lineLimit(2)
                                    Text(OrbitFormatters.relativeTimeFromRfc3339(run.startedAtRfc3339))
                                        .font(.system(size: 11))
                                        .foregroundStyle(palette.fgMuted)
                                }
                                Spacer(minLength: 0)
                            }
                        }
                    }
                }
            }
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
    }

    private func statusLabel(_ status: FfiScheduleStatus) -> String {
        switch status {
        case .active: return "Active"
        case .paused: return "Paused"
        case .completed: return "Completed"
        }
    }

    private func runStatusColor(_ status: FfiScheduleRunStatus) -> Color {
        switch status {
        case .running: return palette.accentSky
        case .succeeded: return palette.direct
        case .failed: return palette.danger
        case .skipped: return palette.fgMuted
        }
    }
}

private struct CreateAutomationSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var name = ""
    @State private var cronExpr = "daily 09:00"
    @State private var prompt = ""
    @State private var cwd = ""
    @State private var provider: FfiProviderKind = .claudeCode
    @State private var mode: FfiSessionMode = .acceptEdits
    @State private var useWorktree = true
    @State private var archiveOnFinish = true

    private let cronPresets = ["15m", "1h", "daily 09:00", "weekdays 09:00"]

    private var availableProviders: [FfiProviderKind] {
        let live = store.activeHost?.availableProviders ?? []
        return live.isEmpty ? [.claudeCode, .codex, .antigravity] : live
    }

    private var candidateCwds: [String] {
        var seen: Set<String> = []
        var result: [String] = []
        let projectDirs = store.projects
            .filter { $0.hostId == store.selectedHostId }
            .flatMap(\.directories)
        let discovered = (store.currentHostDetail?.discoveredProjects ?? [])
            .map(\.path)
        for path in [store.activeCwd] + projectDirs + discovered {
            let norm = OrbitPathLogic.normalizePath(path)
            if !norm.isEmpty && norm != "~" && !seen.contains(norm) {
                seen.insert(norm)
                result.append(norm)
            }
        }
        return Array(result.prefix(6))
    }

    private var resolvedCwd: String {
        let trimmed = cwd.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? store.activeCwd : trimmed
    }

    private var isResolvedCwdGitRepo: Bool {
        store.isActiveFolderGitRepo(hostId: store.selectedHostId, cwd: resolvedCwd)
    }

    var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                    // Templates
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            templateChip("Daily Digest", cron: "daily 09:00", prompt: "Review yesterday's commits, open branches, and TODOs. Summarize risks and priorities.")
                            templateChip("Hourly Test Check", cron: "1h", prompt: "Run the test suite and summarize any regressions or flaky tests.")
                            templateChip("Dependency Audit", cron: "weekdays 09:00", prompt: "Check for security advisories and outdated dependencies in this workspace.")
                        }
                    }

                    VStack(spacing: 0) {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Name")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                            TextField("e.g. Morning Repo Digest", text: $name)
                                .font(OrbitTypography.subhead(.regular))
                                .foregroundStyle(palette.fgPrimary)
                        }
                        .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        VStack(alignment: .leading, spacing: 8) {
                            Text("Schedule (Cron or Preset)")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                            TextField("daily 09:00 or */30 * * * *", text: $cronExpr)
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgPrimary)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()

                            HStack(spacing: 6) {
                                ForEach(cronPresets, id: \.self) { preset in
                                    Button(preset) {
                                        cronExpr = preset
                                        store.hapticLight()
                                    }
                                    .font(.system(size: 11, weight: .medium, design: .monospaced))
                                    .foregroundStyle(cronExpr == preset ? palette.onPrimary : palette.fgSecondary)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 4)
                                    .background(cronExpr == preset ? palette.primary : palette.subtleFill, in: Capsule())
                                    .buttonStyle(.plain)
                                }
                            }
                        }
                        .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        VStack(alignment: .leading, spacing: 8) {
                            Text("Working Directory")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgSecondary)
                            TextField("~/project/path", text: $cwd)
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgPrimary)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                            if !candidateCwds.isEmpty {
                                ScrollView(.horizontal, showsIndicators: false) {
                                    HStack(spacing: 6) {
                                        ForEach(candidateCwds, id: \.self) { dir in
                                            let isCurrent = OrbitFormatters.arePathsEquivalent(resolvedCwd, dir)
                                            Button(OrbitFormatters.shortPath(dir)) {
                                                cwd = dir
                                                if !store.isActiveFolderGitRepo(hostId: store.selectedHostId, cwd: dir) {
                                                    useWorktree = false
                                                }
                                                store.hapticLight()
                                            }
                                            .font(OrbitTypography.meta(isCurrent ? .semibold : .medium))
                                            .foregroundStyle(isCurrent ? palette.onPrimary : palette.fgSecondary)
                                            .padding(.horizontal, 8)
                                            .padding(.vertical, 4)
                                            .background(isCurrent ? palette.primary : palette.subtleFill, in: Capsule())
                                            .buttonStyle(.plain)
                                        }
                                    }
                                }
                            }
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
                                Text("Run in isolated Git Worktree")
                                    .font(OrbitTypography.subhead(.regular))
                                    .foregroundStyle(isResolvedCwdGitRepo ? palette.fgPrimary : palette.fgMuted)
                                if !isResolvedCwdGitRepo {
                                    Text("Git repository required for isolated worktrees")
                                        .font(OrbitTypography.meta(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                }
                            }
                        }
                        .tint(palette.direct)
                        .disabled(!isResolvedCwdGitRepo)
                        .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        Toggle("Archive session on finish", isOn: $archiveOnFinish)
                            .font(OrbitTypography.subhead(.regular))
                            .tint(palette.direct)
                            .padding(OrbitSpacing.md)

                        OrbitHairline().padding(.leading, OrbitSpacing.md)

                        VStack(alignment: .leading, spacing: 6) {
                            Text("Prompt")
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

                    let canSave = !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        && !cronExpr.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        && !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty

                    Button {
                        let allowWorktree = isResolvedCwdGitRepo && useWorktree
                        store.createSchedule(
                            name: name,
                            cronExpr: cronExpr,
                            prompt: prompt,
                            cwd: resolvedCwd,
                            provider: provider,
                            mode: mode,
                            worktree: allowWorktree,
                            archiveOnFinish: archiveOnFinish
                        )
                        dismiss()
                    } label: {
                        Text("Create Automation")
                            .font(OrbitTypography.subhead(.semibold))
                            .foregroundStyle(palette.onPrimary)
                            .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                            .background(
                                canSave ? palette.primary : palette.fgMuted.opacity(0.35),
                                in: RoundedRectangle(cornerRadius: 12, style: .continuous)
                            )
                    }
                    .buttonStyle(.plain)
                    .disabled(!canSave)
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.md)
            }
            .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
            .navigationTitle("New Automation")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                        .foregroundStyle(palette.fgSecondary)
                }
            }
            .onAppear {
                cwd = store.activeCwd
                provider = store.selectedProvider
                mode = OrbitFormatters.normalizePermissionMode(store.selectedMode)
                if !isResolvedCwdGitRepo {
                    useWorktree = false
                }
            }
        }
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

    private func templateChip(_ title: String, cron: String, prompt text: String) -> some View {
        Button {
            name = title
            cronExpr = cron
            prompt = text
            store.hapticLight()
        } label: {
            Text(title)
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgPrimary)
                .padding(.horizontal, 12)
                .frame(height: 30)
                .background(palette.elevated, in: Capsule())
                .overlay(Capsule().strokeBorder(palette.hairline, lineWidth: 0.5))
        }
        .buttonStyle(.plain)
    }
}
