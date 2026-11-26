import SwiftUI

private enum AutomationTab: String, CaseIterable, Identifiable {
    case goals
    case schedules
    case monitors
    case triggers
    case workflows
    case hooks

    var id: String { rawValue }

    var label: String {
        switch self {
        case .goals: return OrbitL10n.tr("Goals")
        case .schedules: return OrbitL10n.tr("Schedules")
        case .monitors: return OrbitL10n.tr("Monitors")
        case .triggers: return OrbitL10n.tr("Triggers")
        case .workflows: return OrbitL10n.tr("Workflows")
        case .hooks: return OrbitL10n.tr("Hooks")
        }
    }

    var systemImage: String {
        switch self {
        case .goals: return "target"
        case .schedules: return "clock.arrow.circlepath"
        case .monitors: return "waveform.path.ecg"
        case .triggers: return "timer"
        case .workflows: return "square.3.layers.3d"
        case .hooks: return "shield.lefthalf.filled"
        }
    }
}

private enum ScheduleFilterKind: String, CaseIterable, Identifiable {
    case all = "All"
    case schedules = "Schedules"
    case heartbeats = "Heartbeats"
    var id: String { rawValue }
}

/// Complete 6-Tab Automations Sheet (Goals, Schedules, Monitors, Triggers, Workflows, Hooks)
/// wired directly to `OrbitMobileCore` M2-Core UniFFI RPCs.
struct AutomationsSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var activeTab: AutomationTab = .goals
    @State private var expandedScheduleIds: Set<String> = []
    @State private var confirmDeleteSchedule: FfiScheduleSpec? = nil
    @State private var scheduleFilter: ScheduleFilterKind = .all

    // Creation modal toggles
    @State private var isCreateGoalPresented = false
    @State private var isCreateSchedulePresented = false
    @State private var isCreateMonitorPresented = false
    @State private var isCreateTriggerPresented = false
    @State private var isCreateWorkflowPresented = false
    @State private var isCreateRulePresented = false

    private var schedules: [FfiScheduleSpec] {
        store.currentHostDetail?.schedules ?? []
    }

    private var filteredSchedules: [FfiScheduleSpec] {
        switch scheduleFilter {
        case .all: return schedules
        case .schedules: return schedules.filter { $0.kind == .schedule }
        case .heartbeats: return schedules.filter { $0.kind == .heartbeat }
        }
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                tabStrip
                OrbitHairline()

                ScrollView(.vertical, showsIndicators: true) {
                    VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                        switch activeTab {
                        case .goals:
                            goalsTabContent
                        case .schedules:
                            schedulesTabContent
                        case .monitors:
                            monitorsTabContent
                        case .triggers:
                            triggersTabContent
                        case .workflows:
                            workflowsTabContent
                        case .hooks:
                            hooksTabContent
                        }
                    }
                    .padding(.horizontal, OrbitSpacing.lg)
                    .padding(.vertical, OrbitSpacing.md)
                }
                .refreshable {
                    store.refreshAllAutomations()
                }
            }
            .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
            .navigationTitle(OrbitL10n.tr("Automations"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button {
                        store.hapticLight()
                        presentCreatorForCurrentTab()
                    } label: {
                        Image(systemName: "plus")
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(palette.primary)
                    }
                    .accessibilityLabel("Create \(activeTab.label)")
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Done")) { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
            .onAppear {
                store.refreshAllAutomations()
            }
            .sheet(isPresented: $isCreateGoalPresented) {
                CreateGoalSheet(store: store)
                    .environment(\.orbitPalette, palette)
                    .presentationDetents([.large])
                    .presentationDragIndicator(.visible)
            }
            .sheet(isPresented: $isCreateSchedulePresented) {
                CreateAutomationSheet(store: store)
                    .environment(\.orbitPalette, palette)
                    .presentationDetents([.large])
                    .presentationDragIndicator(.visible)
            }
            .sheet(isPresented: $isCreateMonitorPresented) {
                CreateMonitorSheet(store: store)
                    .environment(\.orbitPalette, palette)
                    .presentationDetents([.medium, .large])
                    .presentationDragIndicator(.visible)
            }
            .sheet(isPresented: $isCreateTriggerPresented) {
                CreateTriggerSheet(store: store)
                    .environment(\.orbitPalette, palette)
                    .presentationDetents([.medium, .large])
                    .presentationDragIndicator(.visible)
            }
            .sheet(isPresented: $isCreateWorkflowPresented) {
                CreateWorkflowSheet(store: store)
                    .environment(\.orbitPalette, palette)
                    .presentationDetents([.large])
                    .presentationDragIndicator(.visible)
            }
            .sheet(isPresented: $isCreateRulePresented) {
                CreatePermissionRuleSheet(store: store)
                    .environment(\.orbitPalette, palette)
                    .presentationDetents([.medium, .large])
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
                Button(OrbitL10n.tr("Cancel"), role: .cancel) { confirmDeleteSchedule = nil }
            }
        }
    }

    private func presentCreatorForCurrentTab() {
        switch activeTab {
        case .goals: isCreateGoalPresented = true
        case .schedules: isCreateSchedulePresented = true
        case .monitors: isCreateMonitorPresented = true
        case .triggers: isCreateTriggerPresented = true
        case .workflows: isCreateWorkflowPresented = true
        case .hooks: isCreateRulePresented = true
        }
    }

    // MARK: - 6-Tab Scrollable Pill Bar

    private var tabStrip: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(AutomationTab.allCases) { tab in
                    let selected = activeTab == tab
                    let count = badgeCount(for: tab)
                    Button {
                        store.hapticLight()
                        withAnimation(OrbitMotion.ui) {
                            activeTab = tab
                        }
                    } label: {
                        HStack(spacing: 5) {
                            Image(systemName: tab.systemImage)
                                .font(.system(size: 11.5, weight: .medium))
                            Text(tab.label)
                                .font(OrbitTypography.meta(selected ? .semibold : .medium))
                            if count > 0 {
                                Text("\(count)")
                                    .font(.system(size: 10.5, weight: .semibold))
                                    .monospacedDigit()
                                    .padding(.horizontal, 5)
                                    .padding(.vertical, 1)
                                    .background(
                                        (selected ? palette.onPrimary : palette.fgPrimary).opacity(0.14),
                                        in: Capsule()
                                    )
                            }
                        }
                        .foregroundStyle(selected ? palette.onPrimary : palette.fgSecondary)
                        .padding(.horizontal, 12)
                        .frame(height: 32)
                        .background(selected ? palette.primary : palette.subtleFill, in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.vertical, OrbitSpacing.sm)
        }
        .background(palette.canvas)
    }

    private func badgeCount(for tab: AutomationTab) -> Int {
        switch tab {
        case .goals: return store.goalRuns.filter { $0.status == .running || $0.status == .verifying }.count
        case .schedules: return schedules.filter { $0.status == .active }.count
        case .monitors: return store.monitorWatches.filter { $0.status == .active }.count
        case .triggers: return store.conditionalTimers.filter { $0.state == .armed }.count
        case .workflows: return store.workflowRuns.filter { $0.status == .running || $0.status == .pending }.count
        case .hooks: return store.permissionRules.count
        }
    }

    // MARK: - Tab 1: Goals

    @ViewBuilder
    private var goalsTabContent: some View {
        if store.goalRuns.isEmpty {
            emptyTabView(
                icon: "target",
                title: OrbitL10n.tr("No Goals Yet"),
                subtitle: "Run autonomous iterate-and-verify goals with explicit exit criteria.",
                ctaTitle: OrbitL10n.tr("Start Goal")
            ) {
                isCreateGoalPresented = true
            }
        } else {
            VStack(spacing: OrbitSpacing.sm) {
                ForEach(store.goalRuns, id: \.goalId) { goal in
                    goalCard(goal)
                }
            }
        }
    }

    private func goalCard(_ goal: FfiGoalRunRecord) -> some View {
        let color = goalStatusColor(goal.status)
        let isActive = goal.status == .running || goal.status == .verifying
        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(alignment: .top, spacing: OrbitSpacing.sm) {
                if isActive {
                    OrbitSpinnerRingView(color: color, size: 13)
                        .padding(.top, 3)
                } else {
                    Circle().fill(color).frame(width: 8, height: 8).padding(.top, 5)
                }

                VStack(alignment: .leading, spacing: 3) {
                    Text(goal.objective)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(2)

                    HStack(spacing: 6) {
                        statusChip("\(goal.status)".capitalized, color: color)
                        Text("Iter \(goal.currentIteration)/\(goal.maxIterations)")
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(palette.fgMuted)
                        if let code = goal.lastVerifyExitCode {
                            Text("exit \(code)")
                                .font(OrbitTypography.code(.medium))
                                .foregroundStyle(code == 0 ? palette.direct : palette.danger)
                        }
                    }
                    if goal.status == .paused, let reason = goal.pauseReason, !reason.isEmpty {
                        Text(reason)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.warning)
                            .lineLimit(2)
                    }
                }
                Spacer(minLength: 4)
            }

            if !goal.criteria.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(goal.criteria, id: \.id) { c in
                        HStack(spacing: 6) {
                            Image(systemName: c.passed ? "checkmark.circle.fill" : "circle")
                                .font(.system(size: 11.5))
                                .foregroundStyle(c.passed ? palette.direct : palette.fgFaint)
                            Text(c.label)
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(c.passed ? palette.fgMuted : palette.fgSecondary)
                                .lineLimit(1)
                        }
                    }
                }
                .padding(.vertical, 2)
            }

            if let verify = goal.verifyCommand, !verify.isEmpty {
                Text("$ \(verify)")
                    .font(OrbitTypography.code(.regular))
                    .foregroundStyle(palette.fgSecondary)
                    .lineLimit(1)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(palette.recessed, in: RoundedRectangle(cornerRadius: 7, style: .continuous))
            }

            if goal.status == .running || goal.status == .paused || goal.status == .verifying {
                HStack(spacing: OrbitSpacing.sm) {
                    if goal.status == .running {
                        compactActionPill(OrbitL10n.tr("Pause"), icon: "pause.fill") {
                            store.controlGoal(goalId: goal.goalId, action: .pause)
                        }
                    } else if goal.status == .paused {
                        compactActionPill(OrbitL10n.tr("Resume"), icon: "play.fill") {
                            store.controlGoal(goalId: goal.goalId, action: .resume)
                        }
                    }
                    compactActionPill(OrbitL10n.tr("Verify Now"), icon: "checkmark.seal", tint: palette.accentSky) {
                        store.controlGoal(goalId: goal.goalId, action: .verifyNow)
                    }
                    Spacer()
                    compactActionPill(OrbitL10n.tr("Cancel"), icon: "xmark", tint: palette.danger) {
                        store.controlGoal(goalId: goal.goalId, action: .cancel)
                    }
                }
            }
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
    }

    private func goalStatusColor(_ status: FfiGoalStatus) -> Color {
        switch status {
        case .running, .verifying: return palette.accentSky
        case .paused: return palette.warning
        case .succeeded: return palette.direct
        case .failed: return palette.danger
        case .canceled: return palette.fgMuted
        }
    }

    // MARK: - Tab 2: Schedules & Heartbeats

    @ViewBuilder
    private var schedulesTabContent: some View {
        if schedules.isEmpty {
            emptyTabView(
                icon: "clock.arrow.circlepath",
                title: OrbitL10n.tr("No Automations Yet"),
                subtitle: "Run recurring checks, nightly reviews, or scheduled agent runs on your host daemon.",
                ctaTitle: "Create Schedule"
            ) {
                isCreateSchedulePresented = true
            }
        } else {
            Picker("Filter", selection: $scheduleFilter) {
                ForEach(ScheduleFilterKind.allCases) { kind in
                    Text(kind.rawValue).tag(kind)
                }
            }
            .pickerStyle(.segmented)

            VStack(spacing: OrbitSpacing.sm) {
                ForEach(filteredSchedules) { sched in
                    scheduleCard(sched)
                }
            }
        }
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
                        statusChip(isActive ? OrbitL10n.tr("Active") : "\(sched.status)".capitalized, color: isActive ? palette.direct : palette.fgMuted)
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
                    compactActionPill(
                        isActive ? OrbitL10n.tr("Pause") : OrbitL10n.tr("Resume"),
                        icon: isActive ? "pause.fill" : "play.fill"
                    ) {
                        if isActive { store.pauseSchedule(sched.scheduleId) }
                        else { store.resumeSchedule(sched.scheduleId) }
                    }
                }
                compactActionPill(OrbitL10n.tr("Run Now"), icon: "bolt.fill", tint: palette.accentSky) {
                    store.runScheduleOnce(sched.scheduleId)
                    Task { @MainActor in
                        try? await Task.sleep(nanoseconds: 500_000_000)
                        store.fetchScheduleRuns(sched.scheduleId)
                        if let hid = store.selectedHostId {
                            store.reloadHostDetail(hid)
                        }
                    }
                }
                Spacer()
                Button {
                    store.hapticLight()
                    withAnimation(OrbitMotion.ui) {
                        if isExpanded {
                            expandedScheduleIds.remove(sched.scheduleId)
                        } else {
                            expandedScheduleIds.insert(sched.scheduleId)
                            store.fetchScheduleRuns(sched.scheduleId)
                        }
                    }
                } label: {
                    Text(isExpanded ? OrbitL10n.tr("Hide Runs") : OrbitL10n.tr("History"))
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
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
    }

    private func runStatusColor(_ status: FfiScheduleRunStatus) -> Color {
        switch status {
        case .running: return palette.accentSky
        case .succeeded: return palette.direct
        case .failed: return palette.danger
        case .skipped: return palette.fgMuted
        }
    }

    // MARK: - Tab 3: Monitors

    @ViewBuilder
    private var monitorsTabContent: some View {
        if store.monitorWatches.isEmpty {
            emptyTabView(
                icon: "waveform.path.ecg",
                title: OrbitL10n.tr("No Monitors Yet"),
                subtitle: "Watch log files, streaming commands, or HTTP health endpoints and wake an agent on match.",
                ctaTitle: OrbitL10n.tr("New Monitor")
            ) {
                isCreateMonitorPresented = true
            }
        } else {
            VStack(spacing: OrbitSpacing.sm) {
                ForEach(store.monitorWatches, id: \.monitorId) { mon in
                    monitorCard(mon)
                }
            }
        }
    }

    private func monitorCard(_ mon: FfiMonitorWatchRecord) -> some View {
        let isActive = mon.status == .active
        let color: Color = isActive ? palette.direct : (mon.status == .error ? palette.danger : palette.fgMuted)
        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                if isActive {
                    PulsingDotView(color: color, size: 7)
                } else {
                    Circle().fill(color).frame(width: 7, height: 7)
                }
                Text(mon.name)
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)
                Spacer()
                statusChip("\(mon.matchCount) matches", color: color)
            }

            Text(monitorKindSummary(mon.kind))
                .font(OrbitTypography.code(.regular))
                .foregroundStyle(palette.fgSecondary)
                .lineLimit(2)

            if let preview = mon.lastEventPreview, !preview.isEmpty {
                Text(preview)
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .lineLimit(2)
            }

            if isActive {
                HStack {
                    Spacer()
                    compactActionPill(OrbitL10n.tr("Stop Monitor"), icon: "stop.fill", tint: palette.danger) {
                        store.stopMonitor(monitorId: mon.monitorId)
                    }
                }
            }
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
    }

    private func monitorKindSummary(_ kind: FfiMonitorKind) -> String {
        switch kind {
        case let .commandStream(command, args, pattern):
            let cmd = ([command] + args).joined(separator: " ")
            return pattern.map { "$ \(cmd) · /\($0)/" } ?? "$ \(cmd)"
        case let .logFileTail(path, pattern):
            return "tail \(OrbitPathLogic.shortPath(path)) · /\(pattern)/"
        case let .httpHealth(url, expectStatus, intervalSecs):
            return "GET \(url) (expect \(expectStatus) · \(intervalSecs)s)"
        }
    }

    // MARK: - Tab 4: Triggers (Conditional Timers)

    @ViewBuilder
    private var triggersTabContent: some View {
        if store.conditionalTimers.isEmpty {
            emptyTabView(
                icon: "timer",
                title: OrbitL10n.tr("No Triggers Yet"),
                subtitle: "Arm wake-up timers that cancel early when a message arrives, peers settle, or a file changes.",
                ctaTitle: OrbitL10n.tr("Arm Trigger")
            ) {
                isCreateTriggerPresented = true
            }
        } else {
            VStack(spacing: OrbitSpacing.sm) {
                ForEach(store.conditionalTimers, id: \.timerId) { timer in
                    triggerCard(timer)
                }
            }
        }
    }

    private func triggerCard(_ timer: FfiConditionalTimerRecord) -> some View {
        let isArmed = timer.state == .armed
        let color: Color = isArmed ? palette.warning : (timer.state == .fired ? palette.direct : palette.fgMuted)
        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                Image(systemName: isArmed ? "alarm" : "checkmark.circle")
                    .font(.system(size: 13.5, weight: .semibold))
                    .foregroundStyle(color)
                Text(timer.conditionLabel.isEmpty ? "Conditional Timer" : timer.conditionLabel)
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)
                Spacer()
                statusChip(isArmed ? "Armed · \(timer.durationSeconds)s" : "\(timer.state)", color: color)
            }

            Text(timer.prompt)
                .font(OrbitTypography.meta(.regular))
                .foregroundStyle(palette.fgSecondary)
                .lineLimit(2)

            if isArmed {
                HStack(spacing: OrbitSpacing.sm) {
                    compactActionPill(OrbitL10n.tr("Fire Now"), icon: "bolt.fill", tint: palette.accentSky) {
                        store.fireConditionalTriggerNow(timerId: timer.timerId)
                    }
                    Spacer()
                    compactActionPill(OrbitL10n.tr("Cancel"), icon: "xmark", tint: palette.danger) {
                        store.cancelConditionalTrigger(timerId: timer.timerId)
                    }
                }
            }
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
    }

    // MARK: - Tab 5: Workflows (DAG / Batch / Best-of-N)

    @ViewBuilder
    private var workflowsTabContent: some View {
        if store.workflowRuns.isEmpty {
            emptyTabView(
                icon: "square.3.layers.3d",
                title: OrbitL10n.tr("No Workflows Yet"),
                subtitle: "Orchestrate multi-step DAG pipelines across isolated worktrees.",
                ctaTitle: OrbitL10n.tr("New Workflow")
            ) {
                isCreateWorkflowPresented = true
            }
        } else {
            VStack(spacing: OrbitSpacing.sm) {
                ForEach(store.workflowRuns, id: \.runId) { run in
                    workflowCard(run)
                }
            }
        }
    }

    private func workflowCard(_ run: FfiWorkflowRunRecord) -> some View {
        let isRunning = run.status == .running || run.status == .pending
        let color: Color = {
            switch run.status {
            case .running, .pending: return palette.accentSky
            case .succeeded: return palette.direct
            case .partialFailure: return palette.warning
            case .failed: return palette.danger
            case .canceled: return palette.fgMuted
            }
        }()

        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                if isRunning {
                    OrbitSpinnerRingView(color: color, size: 13)
                } else {
                    Circle().fill(color).frame(width: 8, height: 8)
                }
                Text(run.name)
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)
                Spacer()
                statusChip("\(run.status)".capitalized, color: color)
            }

            VStack(alignment: .leading, spacing: 6) {
                ForEach(run.steps, id: \.stepId) { step in
                    HStack(spacing: 8) {
                        stepDot(step.status)
                        Text(step.name)
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgPrimary)
                            .lineLimit(1)
                        if step.isWinner {
                            statusChip("Winner", color: palette.direct)
                        }
                        Spacer(minLength: 4)
                        if step.diffAdditions > 0 || step.diffDeletions > 0 {
                            Text("+\(step.diffAdditions) -\(step.diffDeletions)")
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }
                    }
                }
            }

            if isRunning {
                HStack {
                    Spacer()
                    compactActionPill(OrbitL10n.tr("Cancel"), icon: "xmark", tint: palette.danger) {
                        store.cancelWorkflow(runId: run.runId)
                    }
                }
            }
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
    }

    @ViewBuilder
    private func stepDot(_ status: FfiWorkflowStepStatus) -> some View {
        switch status {
        case .running, .verifying:
            OrbitSpinnerRingView(color: palette.accentSky, size: 10)
        case .succeeded:
            Image(systemName: "checkmark.circle.fill").font(.system(size: 11)).foregroundStyle(palette.direct)
        case .failed:
            Image(systemName: "xmark.circle.fill").font(.system(size: 11)).foregroundStyle(palette.danger)
        case .pending, .canceled, .skipped:
            Circle().fill(palette.fgFaint).frame(width: 6, height: 6)
        }
    }

    // MARK: - Tab 6: Hooks (Permission Rules & Guardian Policy)

    @ViewBuilder
    private var hooksTabContent: some View {
        if let guardianSnap = store.guardianConfigSnapshot {
            guardianPolicyCard(guardianSnap)
        }

        VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            HStack {
                Text(OrbitL10n.tr("Permission Rules"))
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)
                Spacer()
                Button {
                    store.hapticLight()
                    isCreateRulePresented = true
                } label: {
                    Label(OrbitL10n.tr("Add Rule"), systemImage: "plus")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.primary)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 4)

            if store.permissionRules.isEmpty {
                Text("No persistent host/project permission rules yet.")
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .padding(OrbitSpacing.md)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(store.permissionRules.enumerated()), id: \.element.id) { idx, rule in
                        if idx > 0 { OrbitHairline().padding(.leading, OrbitSpacing.md) }
                        HStack(spacing: OrbitSpacing.sm) {
                            statusChip(
                                rule.decision == .allow ? "Allow" : "Deny",
                                color: rule.decision == .allow ? palette.direct : palette.danger
                            )
                            VStack(alignment: .leading, spacing: 2) {
                                Text(rule.pattern)
                                    .font(OrbitTypography.code(.medium))
                                    .foregroundStyle(palette.fgPrimary)
                                    .lineLimit(1)
                                Text(verbatim: "\(String(describing: rule.toolKind)) · \(String(describing: rule.scopeKind)) · \(rule.matchCount) hits")
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.fgMuted)
                            }
                            Spacer(minLength: 4)
                            Button {
                                store.deleteHookPermissionRule(ruleId: rule.id)
                            } label: {
                                Image(systemName: "trash")
                                    .font(.system(size: 12))
                                    .foregroundStyle(palette.danger)
                                    .frame(width: 32, height: 32)
                            }
                            .buttonStyle(.plain)
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .padding(.vertical, 10)
                    }
                }
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
            }
        }
    }

    private func guardianPolicyCard(_ snap: GuardianConfigSnapshotView) -> some View {
        let cfg = snap.config
        return VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text(OrbitL10n.tr("Guardian Policy"))
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, 4)

            VStack(spacing: 0) {
                Toggle(OrbitL10n.tr("Enable Guardian"), isOn: Binding(
                    get: { cfg.enabled },
                    set: { val in
                        var next = cfg
                        next.enabled = val
                        store.updateGuardianPolicy(next)
                    }
                ))
                .font(OrbitTypography.subhead(.medium))
                .tint(palette.direct)
                .padding(OrbitSpacing.md)

                OrbitHairline().padding(.leading, OrbitSpacing.md)

                Toggle(OrbitL10n.tr("Auto-Approve Read-Only Tools"), isOn: Binding(
                    get: { cfg.allowReadOnlyTools },
                    set: { val in
                        var next = cfg
                        next.allowReadOnlyTools = val
                        store.updateGuardianPolicy(next)
                    }
                ))
                .font(OrbitTypography.subhead(.regular))
                .tint(palette.direct)
                .padding(OrbitSpacing.md)

                OrbitHairline().padding(.leading, OrbitSpacing.md)

                Toggle(OrbitL10n.tr("Auto-Approve Workspace Tests"), isOn: Binding(
                    get: { cfg.allowWorkspaceTests },
                    set: { val in
                        var next = cfg
                        next.allowWorkspaceTests = val
                        store.updateGuardianPolicy(next)
                    }
                ))
                .font(OrbitTypography.subhead(.regular))
                .tint(palette.direct)
                .padding(OrbitSpacing.md)
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))

            if !snap.recentAudits.isEmpty {
                let recentAudits = Array(snap.recentAudits.prefix(6))
                Text(OrbitL10n.tr("Recent Guardian Audits"))
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)
                    .padding(.top, OrbitSpacing.sm)
                    .padding(.horizontal, 4)

                VStack(spacing: 0) {
                    ForEach(recentAudits.indices, id: \.self) { idx in
                        let audit = recentAudits[idx]
                        let decision = audit.assessment.decision
                        if idx > 0 { OrbitHairline().padding(.leading, OrbitSpacing.md) }
                        HStack(spacing: OrbitSpacing.sm) {
                            statusChip(
                                String(describing: decision),
                                color: decision == .autoAllow ? palette.direct : (decision == .autoDeny ? palette.danger : palette.warning)
                            )
                            VStack(alignment: .leading, spacing: 2) {
                                Text(audit.targetSummary.isEmpty ? audit.toolName : audit.targetSummary)
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.fgPrimary)
                                    .lineLimit(1)
                                Text(audit.assessment.reason)
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                    .lineLimit(1)
                            }
                            Spacer(minLength: 0)
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .padding(.vertical, 9)
                    }
                }
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
            }
        }
    }

    // MARK: - Shared Helpers

    private func emptyTabView(
        icon: String,
        title: String,
        subtitle: String,
        ctaTitle: String,
        action: @escaping () -> Void
    ) -> some View {
        VStack(spacing: OrbitSpacing.md) {
            Image(systemName: icon)
                .font(.system(size: 26, weight: .medium))
                .foregroundStyle(palette.fgMuted)
            Text(title)
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)
            Text(subtitle)
                .orbitSubheadStyle()
                .foregroundStyle(palette.fgSecondary)
                .multilineTextAlignment(.center)
            Button {
                store.hapticLight()
                action()
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "plus")
                    Text(ctaTitle)
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
        .padding(.vertical, 44)
    }

    private func statusChip(_ label: String, color: Color) -> some View {
        Text(label)
            .font(.system(size: 10.5, weight: .semibold))
            .foregroundStyle(color)
            .padding(.horizontal, 7)
            .padding(.vertical, 2)
            .background(color.opacity(0.14), in: Capsule())
    }

    private func compactActionPill(_ label: String, icon: String, tint: Color? = nil, action: @escaping () -> Void) -> some View {
        let fg = tint ?? palette.fgPrimary
        let bg = tint?.opacity(0.14) ?? palette.subtleFill
        return Button(action: action) {
            Label(label, systemImage: icon)
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(fg)
                .padding(.horizontal, 10)
                .frame(height: 28)
                .background(bg, in: Capsule())
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Creation Sheets for Goals, Monitors, Triggers, Workflows, Hooks & Schedules

private struct CreateGoalSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var objective = ""
    @State private var criteriaText = ""
    @State private var verifyCommand = ""
    @State private var maxIterations = 8

    var body: some View {
        NavigationStack {
            Form {
                Section(OrbitL10n.tr("Objective")) {
                    TextField("e.g. Fix all failing unit tests and ensure clean clippy", text: $objective, axis: .vertical)
                        .lineLimit(3...6)
                }
                Section("Acceptance Criteria (one per line)") {
                    TextEditor(text: $criteriaText)
                        .frame(minHeight: 72)
                }
                Section(OrbitL10n.tr("Verification Command")) {
                    TextField("cargo test --workspace", text: $verifyCommand)
                        .font(OrbitTypography.code(.regular))
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    Stepper("\(OrbitL10n.tr("Max Iterations")): \(maxIterations)", value: $maxIterations, in: 1...50)
                }
            }
            .navigationTitle(OrbitL10n.tr("New Goal"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button(OrbitL10n.tr("Cancel")) { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Start Goal")) {
                        let criteria = criteriaText.split(separator: "\n").map(String.init)
                        store.startGoalRun(
                            objective: objective,
                            criteria: criteria,
                            verifyCommand: verifyCommand.isEmpty ? nil : verifyCommand,
                            maxIterations: UInt32(maxIterations),
                            maxDurationSecs: 3600
                        )
                        dismiss()
                    }
                    .disabled(objective.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
    }
}

private struct CreateMonitorSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var mode = 0 // 0: HTTP Health, 1: Log Tail, 2: Command
    @State private var target = ""
    @State private var pattern = "ERROR|FATAL"
    @State private var expectStatus = 200
    @State private var intervalSecs = 30

    var body: some View {
        NavigationStack {
            Form {
                Section("Monitor") {
                    TextField("Monitor name", text: $name)
                    Picker("Kind", selection: $mode) {
                        Text("HTTP Health").tag(0)
                        Text("Log Tail").tag(1)
                        Text("Command").tag(2)
                    }
                    .pickerStyle(.segmented)
                }
                Section("Target") {
                    if mode == 0 {
                        TextField("http://127.0.0.1:8080/health", text: $target)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                        Stepper("Expect HTTP \(expectStatus)", value: $expectStatus, in: 100...599)
                        Stepper("Interval: \(intervalSecs)s", value: $intervalSecs, in: 5...300, step: 5)
                    } else if mode == 1 {
                        TextField("/var/log/app.log", text: $target)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                        TextField("Regex pattern", text: $pattern)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                    } else {
                        TextField("kubectl logs -f deploy/api", text: $target)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                        TextField("Optional pattern", text: $pattern)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                    }
                }
            }
            .navigationTitle(OrbitL10n.tr("New Monitor"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button(OrbitL10n.tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Save")) {
                        let kind: MonitorKind
                        if mode == 0 {
                            kind = .httpHealth(url: target, expectStatus: UInt16(expectStatus), intervalSecs: UInt64(intervalSecs))
                        } else if mode == 1 {
                            kind = .logFileTail(path: target, pattern: pattern)
                        } else {
                            let parts = target.split(separator: " ").map(String.init)
                            kind = .commandStream(
                                command: parts.first ?? target,
                                args: Array(parts.dropFirst()),
                                pattern: pattern.isEmpty ? nil : pattern
                            )
                        }
                        store.createMonitorWatch(name: name, kind: kind, expiresInSecs: 3600)
                        dismiss()
                    }
                    .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || target.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
    }
}

private struct CreateTriggerSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss

    @State private var durationSeconds = 300
    @State private var conditionIndex = 0
    @State private var conditionArg = ""
    @State private var prompt = ""

    var body: some View {
        NavigationStack {
            Form {
                Section("Delay & Cancel Condition") {
                    Stepper("Wait \(durationSeconds)s", value: $durationSeconds, in: 15...86400, step: 30)
                    Picker("Cancel Early On", selection: $conditionIndex) {
                        Text("Any Message").tag(0)
                        Text("All Subagents Settled").tag(1)
                        Text("All Peers Settled").tag(2)
                        Text("File Changed").tag(3)
                        Text("Never (Fixed Timer)").tag(4)
                    }
                    if conditionIndex == 3 {
                        TextField("Relative file path", text: $conditionArg)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                    }
                }
                Section("Wake-Up Prompt") {
                    TextField("Check build status and summarize results", text: $prompt, axis: .vertical)
                        .lineLimit(2...5)
                }
            }
            .navigationTitle(OrbitL10n.tr("Arm Trigger"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button(OrbitL10n.tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Save")) {
                        let cond: TimerCondition
                        switch conditionIndex {
                        case 1: cond = .allSubagentsSettled
                        case 2: cond = .allPeersSettled
                        case 3: cond = .fileChanged(path: conditionArg)
                        case 4: cond = .never
                        default: cond = .anyMessage
                        }
                        store.createConditionalTrigger(
                            durationSeconds: UInt64(durationSeconds),
                            prompt: prompt,
                            condition: cond
                        )
                        dismiss()
                    }
                    .disabled(prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
    }
}

private struct CreateWorkflowSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var step1Name = "Implement"
    @State private var step1Prompt = ""
    @State private var step2Name = "Verify & Review"
    @State private var step2Prompt = "Run tests and review changes from step 1."
    @State private var isolatedWorktree = true

    var body: some View {
        NavigationStack {
            Form {
                Section("Workflow") {
                    TextField("Pipeline name", text: $name)
                    Toggle("Isolated Git Worktrees", isOn: $isolatedWorktree)
                }
                Section("Step 1") {
                    TextField("Step name", text: $step1Name)
                    TextField("Prompt template", text: $step1Prompt, axis: .vertical).lineLimit(2...5)
                }
                Section("Step 2 (depends on Step 1)") {
                    TextField("Step name", text: $step2Name)
                    TextField("Prompt template", text: $step2Prompt, axis: .vertical).lineLimit(2...5)
                }
            }
            .navigationTitle(OrbitL10n.tr("New Workflow"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button(OrbitL10n.tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Start") {
                        var steps = [
                            DagStepSpec(
                                stepId: "step-1",
                                name: step1Name.isEmpty ? "Step 1" : step1Name,
                                dependsOn: [],
                                promptTemplate: step1Prompt,
                                provider: nil,
                                model: nil,
                                isolatedWorktree: isolatedWorktree,
                                retryCount: 1,
                                timeoutSecs: 900
                            )
                        ]
                        if !step2Prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            steps.append(DagStepSpec(
                                stepId: "step-2",
                                name: step2Name.isEmpty ? "Step 2" : step2Name,
                                dependsOn: ["step-1"],
                                promptTemplate: step2Prompt,
                                provider: nil,
                                model: nil,
                                isolatedWorktree: isolatedWorktree,
                                retryCount: 1,
                                timeoutSecs: 900
                            ))
                        }
                        store.startDagWorkflow(
                            name: name,
                            cwd: store.activeCwd,
                            steps: steps,
                            defaultProvider: store.selectedProvider,
                            defaultMode: store.selectedMode
                        )
                        dismiss()
                    }
                    .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || step1Prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
    }
}

private struct CreatePermissionRuleSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss

    @State private var pattern = ""
    @State private var decision: PermissionRuleDecision = .allow
    @State private var toolKind: PermissionRuleToolKind = .shell
    @State private var scopeKind: PermissionRuleScopeKind = .project
    @State private var note = ""

    var body: some View {
        NavigationStack {
            Form {
                Section("Rule") {
                    Picker("Decision", selection: $decision) {
                        Text("Allow").tag(PermissionRuleDecision.allow)
                        Text("Deny").tag(PermissionRuleDecision.deny)
                    }
                    .pickerStyle(.segmented)

                    Picker("Tool", selection: $toolKind) {
                        Text("Shell").tag(PermissionRuleToolKind.shell)
                        Text("File Write").tag(PermissionRuleToolKind.fileWrite)
                        Text("File Read").tag(PermissionRuleToolKind.fileRead)
                        Text("MCP Tool").tag(PermissionRuleToolKind.mcpTool)
                        Text("Any").tag(PermissionRuleToolKind.any)
                    }

                    Picker("Scope", selection: $scopeKind) {
                        Text("Project").tag(PermissionRuleScopeKind.project)
                        Text("Host").tag(PermissionRuleScopeKind.host)
                    }

                    TextField("Command prefix or glob (e.g. cargo test)", text: $pattern)
                        .font(OrbitTypography.code(.regular))
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()

                    TextField("Optional note", text: $note)
                }
            }
            .navigationTitle(OrbitL10n.tr("Add Rule"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button(OrbitL10n.tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Save")) {
                        let input = UpsertPermissionRuleInput(
                            id: nil,
                            scopeKind: scopeKind,
                            projectCwd: scopeKind == .project ? store.activeCwd : nil,
                            provider: nil,
                            toolKind: toolKind,
                            patternKind: toolKind == .shell ? .commandPrefix : .pathGlob,
                            pattern: pattern.trimmingCharacters(in: .whitespacesAndNewlines),
                            decision: decision,
                            source: .manual,
                            note: note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : note
                        )
                        store.upsertHookPermissionRule(input)
                        dismiss()
                    }
                    .disabled(pattern.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
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

    private var resolvedCwd: String {
        let trimmed = cwd.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? store.activeCwd : trimmed
    }

    private var isResolvedCwdGitRepo: Bool {
        store.isActiveFolderGitRepo(hostId: store.selectedHostId, cwd: resolvedCwd)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Schedule") {
                    TextField("e.g. Morning Repo Digest", text: $name)
                    TextField("daily 09:00 or */30 * * * *", text: $cronExpr)
                        .font(OrbitTypography.code(.regular))
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    if let cronErr = OrbitCronLogic.validate(cronExpr) {
                        Text(cronErr)
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.danger)
                    }
                    ScrollView(.horizontal, showsIndicators: false) {
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
                    TextField("~/project/path", text: $cwd)
                        .font(OrbitTypography.code(.regular))
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                }

                Section("Execution") {
                    Picker("Provider", selection: $provider) {
                        ForEach(availableProviders, id: \.self) { prov in
                            Text(OrbitFormatters.providerLabel(prov)).tag(prov)
                        }
                    }
                    Toggle("Run in isolated Git Worktree", isOn: $useWorktree)
                        .disabled(!isResolvedCwdGitRepo)
                    Toggle("Archive session on finish", isOn: $archiveOnFinish)
                }

                Section("Prompt") {
                    TextEditor(text: $prompt)
                        .frame(minHeight: 96)
                }
            }
            .navigationTitle("New Schedule")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button(OrbitL10n.tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Save")) {
                        store.createSchedule(
                            name: name,
                            cronExpr: cronExpr,
                            prompt: prompt,
                            cwd: resolvedCwd,
                            provider: provider,
                            mode: mode,
                            worktree: isResolvedCwdGitRepo && useWorktree,
                            archiveOnFinish: archiveOnFinish
                        )
                        dismiss()
                    }
                    .disabled(
                        name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                            || prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                            || OrbitCronLogic.validate(cronExpr) != nil
                    )
                }
            }
            .onAppear {
                cwd = store.activeCwd
                provider = store.selectedProvider
                mode = OrbitFormatters.normalizePermissionMode(store.selectedMode)
                if !isResolvedCwdGitRepo { useWorktree = false }
            }
        }
    }
}
