import Foundation

// MARK: - 6-Tab Automations Store Extensions (Goals, Schedules, Monitors, Triggers, Workflows, Hooks)

extension OrbitIosStore {
    public func refreshAllAutomations() {
        guard let core, let hid = selectedHostId else { return }
        let cwd = activeCwd
        automationsLoading = true
        Task { @MainActor [weak self] in
            let fetched: (
                [FfiGoalRunRecord],
                [FfiMonitorWatchRecord],
                [FfiConditionalTimerRecord],
                [FfiWorkflowRunRecord],
                [PermissionRuleRecord],
                GuardianConfigSnapshotView?
            ) = await Task.detached(priority: .userInitiated) {
                let goals = (try? core.listGoalRuns(hostId: hid, agentId: nil)) ?? []
                let monitors = (try? core.listMonitorWatches(hostId: hid, agentId: nil)) ?? []
                let timers = (try? core.listConditionalTimers(hostId: hid, agentId: nil, includeResolved: true)) ?? []
                let workflows = (try? core.listWorkflowRuns(hostId: hid, includeCompleted: true)) ?? []
                let rules = (try? core.listPermissionRules(hostId: hid, projectCwd: cwd == "~" ? nil : cwd)) ?? []
                let guardian = try? core.getGuardianConfig(hostId: hid)
                return (goals, monitors, timers, workflows, rules, guardian)
            }.value
            guard let self, self.selectedHostId == hid else { return }
            self.goalRuns = fetched.0
            self.monitorWatches = fetched.1
            self.conditionalTimers = fetched.2
            self.workflowRuns = fetched.3
            self.permissionRules = fetched.4
            self.guardianConfigSnapshot = fetched.5
            self.automationsLoading = false
        }
    }

    // MARK: - 1. Goals

    public func startGoalRun(
        objective: String,
        criteria: [String],
        verifyCommand: String?,
        maxIterations: UInt32?,
        maxDurationSecs: UInt64?
    ) {
        guard let core, let hid = selectedHostId else { return }
        let targetAgentId = selectedAgentId
            ?? currentHostDetail?.agents.first(where: { $0.archivedAtRfc3339 == nil })?.agentId
            ?? ""
        guard !targetAgentId.isEmpty else {
            showToast("Start or select a session first")
            return
        }
        let cleanVerify = verifyCommand?.trimmingCharacters(in: .whitespacesAndNewlines)
        let req = StartGoalRunRequest(
            hostId: hid,
            agentId: targetAgentId,
            objective: objective.trimmingCharacters(in: .whitespacesAndNewlines),
            criteria: criteria.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty },
            verifyCommand: (cleanVerify?.isEmpty == false) ? cleanVerify : nil,
            maxIterations: maxIterations,
            maxDurationSecs: maxDurationSecs,
            replaceExisting: false,
            dispatchInitialPrompt: nil
        )
        Task { @MainActor [weak self] in
            do {
                let record: FfiGoalRunRecord = try await Task.detached(priority: .userInitiated) {
                    try core.startGoal(req: req)
                }.value
                guard let self else { return }
                self.upsertGoalRecord(record)
                self.hapticSuccess()
                self.showToast("Goal started")
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func controlGoal(goalId: String, action: FfiGoalControlAction) {
        guard let core, let hid = selectedHostId else { return }
        Task { @MainActor [weak self] in
            do {
                let updated: FfiGoalRunRecord = try await Task.detached(priority: .userInitiated) {
                    try core.controlGoalRun(hostId: hid, goalId: goalId, action: action)
                }.value
                guard let self else { return }
                self.upsertGoalRecord(updated)
                self.hapticLight()
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    private func upsertGoalRecord(_ record: FfiGoalRunRecord) {
        if let idx = goalRuns.firstIndex(where: { $0.goalId == record.goalId }) {
            goalRuns[idx] = record
        } else {
            goalRuns.insert(record, at: 0)
        }
    }

    // MARK: - 3. Monitors

    public func createMonitorWatch(
        name: String,
        kind: MonitorKind,
        expiresInSecs: UInt64?
    ) {
        guard let core, let hid = selectedHostId else { return }
        let targetAgentId = selectedAgentId
            ?? currentHostDetail?.agents.first(where: { $0.archivedAtRfc3339 == nil })?.agentId
            ?? ""
        guard !targetAgentId.isEmpty else {
            showToast("Start or select a session first")
            return
        }
        let req = StartMonitorWatchRequest(
            hostId: hid,
            agentId: targetAgentId,
            name: name.trimmingCharacters(in: .whitespacesAndNewlines),
            kind: kind,
            expiresInSecs: expiresInSecs
        )
        Task { @MainActor [weak self] in
            do {
                let record: FfiMonitorWatchRecord = try await Task.detached(priority: .userInitiated) {
                    try core.createMonitor(req: req)
                }.value
                guard let self else { return }
                self.upsertMonitorRecord(record)
                self.hapticSuccess()
                self.showToast("Monitor active")
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func stopMonitor(monitorId: String) {
        guard let core, let hid = selectedHostId else { return }
        Task { @MainActor [weak self] in
            do {
                let updated: FfiMonitorWatchRecord = try await Task.detached(priority: .userInitiated) {
                    try core.stopMonitorWatch(hostId: hid, monitorId: monitorId)
                }.value
                guard let self else { return }
                self.upsertMonitorRecord(updated)
                self.hapticLight()
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    private func upsertMonitorRecord(_ record: FfiMonitorWatchRecord) {
        if let idx = monitorWatches.firstIndex(where: { $0.monitorId == record.monitorId }) {
            monitorWatches[idx] = record
        } else {
            monitorWatches.insert(record, at: 0)
        }
    }

    // MARK: - 4. Triggers (Conditional Timers)

    public func createConditionalTrigger(
        durationSeconds: UInt64,
        prompt: String,
        condition: TimerCondition
    ) {
        guard let core, let hid = selectedHostId else { return }
        let targetAgentId = selectedAgentId
            ?? currentHostDetail?.agents.first(where: { $0.archivedAtRfc3339 == nil })?.agentId
            ?? ""
        guard !targetAgentId.isEmpty else {
            showToast("Start or select a session first")
            return
        }
        let cleanPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        Task { @MainActor [weak self] in
            do {
                let record: FfiConditionalTimerRecord = try await Task.detached(priority: .userInitiated) {
                    try core.createTimer(
                        hostId: hid,
                        agentId: targetAgentId,
                        durationSeconds: durationSeconds,
                        prompt: cleanPrompt,
                        condition: condition
                    )
                }.value
                guard let self else { return }
                self.upsertTimerRecord(record)
                self.hapticSuccess()
                self.showToast("Trigger armed")
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func fireConditionalTriggerNow(timerId: String) {
        guard let core, let hid = selectedHostId else { return }
        Task { @MainActor [weak self] in
            do {
                let updated: FfiConditionalTimerRecord = try await Task.detached(priority: .userInitiated) {
                    try core.fireConditionalTimerNow(hostId: hid, timerId: timerId)
                }.value
                guard let self else { return }
                self.upsertTimerRecord(updated)
                self.hapticMedium()
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func cancelConditionalTrigger(timerId: String) {
        guard let core, let hid = selectedHostId else { return }
        Task { @MainActor [weak self] in
            do {
                let updated: FfiConditionalTimerRecord = try await Task.detached(priority: .userInitiated) {
                    try core.cancelConditionalTimer(hostId: hid, timerId: timerId)
                }.value
                guard let self else { return }
                self.upsertTimerRecord(updated)
                self.hapticLight()
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    private func upsertTimerRecord(_ record: FfiConditionalTimerRecord) {
        if let idx = conditionalTimers.firstIndex(where: { $0.timerId == record.timerId }) {
            conditionalTimers[idx] = record
        } else {
            conditionalTimers.insert(record, at: 0)
        }
    }

    // MARK: - 5. Workflows (DAG / Multi-Step)

    public func startDagWorkflow(
        name: String,
        cwd: String,
        steps: [DagStepSpec],
        defaultProvider: FfiProviderKind,
        defaultMode: FfiSessionMode,
        maxConcurrency: UInt32? = 2
    ) {
        guard let core, let hid = selectedHostId else { return }
        let req = StartDagWorkflowRequest(
            hostId: hid,
            name: name.trimmingCharacters(in: .whitespacesAndNewlines),
            cwd: cwd,
            steps: steps,
            defaultProvider: defaultProvider.coreProviderKind,
            defaultMode: defaultMode.coreSessionMode,
            maxConcurrency: maxConcurrency
        )
        Task { @MainActor [weak self] in
            do {
                let record: FfiWorkflowRunRecord = try await Task.detached(priority: .userInitiated) {
                    try core.startWorkflow(req: req)
                }.value
                guard let self else { return }
                self.upsertWorkflowRecord(record)
                self.hapticSuccess()
                self.showToast("Workflow started")
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func cancelWorkflow(runId: String) {
        guard let core, let hid = selectedHostId else { return }
        Task { @MainActor [weak self] in
            do {
                let updated: FfiWorkflowRunRecord = try await Task.detached(priority: .userInitiated) {
                    try core.cancelWorkflowRun(hostId: hid, runId: runId)
                }.value
                guard let self else { return }
                self.upsertWorkflowRecord(updated)
                self.hapticLight()
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    private func upsertWorkflowRecord(_ record: FfiWorkflowRunRecord) {
        if let idx = workflowRuns.firstIndex(where: { $0.runId == record.runId }) {
            workflowRuns[idx] = record
        } else {
            workflowRuns.insert(record, at: 0)
        }
    }

    // MARK: - 6. Hooks (Permission Rules & Guardian Policy)

    public func upsertHookPermissionRule(_ input: UpsertPermissionRuleInput) {
        guard let core, let hid = selectedHostId else { return }
        Task { @MainActor [weak self] in
            do {
                let saved: PermissionRuleRecord = try await Task.detached(priority: .userInitiated) {
                    try core.upsertPermissionRule(hostId: hid, rule: input)
                }.value
                guard let self else { return }
                if let idx = self.permissionRules.firstIndex(where: { $0.id == saved.id }) {
                    self.permissionRules[idx] = saved
                } else {
                    self.permissionRules.insert(saved, at: 0)
                }
                self.hapticSuccess()
                self.showToast("Rule saved")
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func deleteHookPermissionRule(ruleId: String) {
        guard let core, let hid = selectedHostId else { return }
        Task { @MainActor [weak self] in
            do {
                _ = try await Task.detached(priority: .userInitiated) {
                    try core.deletePermissionRule(hostId: hid, ruleId: ruleId)
                }.value
                guard let self else { return }
                self.permissionRules.removeAll(where: { $0.id == ruleId })
                self.hapticMedium()
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func updateGuardianPolicy(_ config: GuardianPolicyConfig) {
        guard let core, let hid = selectedHostId else { return }
        Task { @MainActor [weak self] in
            do {
                let snap: GuardianConfigSnapshotView = try await Task.detached(priority: .userInitiated) {
                    try core.updateGuardianConfig(hostId: hid, config: config)
                }.value
                guard let self else { return }
                self.guardianConfigSnapshot = snap
                self.hapticLight()
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }
}
