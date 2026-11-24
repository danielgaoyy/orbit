import Foundation
import SwiftUI
import UIKit
import UserNotifications

// MARK: - Core Enum Conversions (Ffi* <-> M2-Core Protocol Enums)

extension FfiProviderKind {
    public var coreProviderKind: ProviderKind {
        switch self {
        case .antigravity: return .antigravity
        case .codex: return .codex
        case .claudeCode: return .claudeCode
        case .openCode: return .openCode
        case .geminiCli: return .geminiCli
        case .cursorAgent: return .cursorAgent
        case .copilot: return .copilot
        case .goose: return .goose
        case .pi: return .pi
        case .kimi: return .kimi
        case .cline: return .cline
        case .mistralVibe: return .mistralVibe
        case .augment: return .augment
        case .acp: return .acp
        }
    }
}

extension ProviderKind {
    public var ffiProviderKind: FfiProviderKind {
        switch self {
        case .antigravity: return .antigravity
        case .codex: return .codex
        case .claudeCode: return .claudeCode
        case .openCode: return .openCode
        case .geminiCli: return .geminiCli
        case .cursorAgent: return .cursorAgent
        case .copilot: return .copilot
        case .goose: return .goose
        case .pi: return .pi
        case .kimi: return .kimi
        case .cline: return .cline
        case .mistralVibe: return .mistralVibe
        case .augment: return .augment
        case .acp: return .acp
        }
    }
}

extension FfiSessionMode {
    public var coreSessionMode: SessionMode {
        switch self {
        case .default: return .default
        case .auto: return .auto
        case .plan: return .plan
        case .acceptEdits: return .acceptEdits
        case .fullAccess: return .fullAccess
        case let .custom(name): return .custom(name)
        }
    }
}

extension FfiReasoningEffort {
    public var coreReasoningEffort: ReasoningEffort {
        switch self {
        case .off: return .off
        case .low: return .low
        case .medium: return .medium
        case .high: return .high
        case .xhigh: return .xhigh
        case .max: return .max
        case .ultra: return .ultra
        }
    }
}

extension FfiPushPreferences {
    public var asPushPreferences: PushPreferences {
        PushPreferences(
            enabled: enabled,
            notifyPermissionRequired: notifyPermissionRequired,
            notifyTurnCompleted: notifyTurnCompleted,
            notifyAgentError: notifyAgentError,
            notifyGoalFinished: notifyGoalFinished,
            notifyTimerFired: notifyTimerFired,
            includeMessagePreview: includeMessagePreview,
            quietHoursEnabled: quietHoursEnabled,
            quietStartHour: quietStartHour,
            quietEndHour: quietEndHour,
            pushGatewayUrl: pushGatewayUrl,
            hasGatewayAuthToken: hasGatewayAuthToken
        )
    }
}

// MARK: - Global Overview Pending Decision Item

public struct OrbitGlobalPendingDecision: Identifiable, Equatable {
    public let id: String
    public let hostId: String
    public let hostName: String
    public let agentId: String
    public let agentTitle: String
    public let requestId: String
    public let title: String
    public let commandOrPath: String
    public let toolName: String?
    public let argumentsJson: String?
    public let allowedDecisions: [String]
    public let isPlanApproval: Bool
    public let isQuestion: Bool
    public let riskLevel: OrbitPermissionRiskLevel

    public var isSafeForBatchAllow: Bool {
        !isQuestion && !isPlanApproval && (riskLevel == .low || riskLevel == .medium)
            && OrbitBatchApprovalLogic.permitsSingleAllowDecision(allowedDecisions)
    }
}

// MARK: - 5.0 Parity Store Methods (Push, Live Activity, Plan/Batch Approvals, Export, Search, Router & Mesh)

extension OrbitIosStore {

    // MARK: - Navigation & Global Overview

    public func openGlobalOverview() {
        isShowingGlobalOverview = true
        isDrawerOpen = false
        refreshRouterAndMesh()
        hapticLight()
    }

    public func navigateToSessionFromDeepLink(hostId: String, agentId: String?) {
        isShowingGlobalOverview = false
        isDrawerOpen = false
        if let agentId, !agentId.isEmpty {
            selectAgent(hostId: hostId, agentId: agentId)
        } else {
            selectHost(hostId)
        }
    }

    /// Aggregates pending decisions from active stage + all loaded hosts for the Global Overview triage queue.
    public var globalPendingDecisions: [OrbitGlobalPendingDecision] {
        var out: [OrbitGlobalPendingDecision] = []
        var seenKeys: Set<String> = []

        func appendFromStage(hostId: String, agentId: String, agentTitle: String, stage: FfiAgentStage) {
            let hostName = hosts.first(where: { $0.hostId == hostId })?.displayName ?? hostId
            for item in stage.pendingPermissions {
                guard case let .permissionRequest(_, rid, title, cmdOrPath, detail, allowed, _, _) = item else { continue }
                let key = "\(hostId)::\(agentId)::\(rid)"
                guard seenKeys.insert(key).inserted else { continue }
                let (toolName, argsJson): (String?, String?) = {
                    if case let .generic(tn, args) = detail { return (tn, args) }
                    return (nil, nil)
                }()
                let isPlan = OrbitPlanApprovalLogic.isPlanApprovalPermission(
                    title: title,
                    commandOrPath: cmdOrPath,
                    toolName: toolName,
                    argumentsJson: argsJson
                )
                let risk = OrbitBatchApprovalLogic.assessRisk(title: title, commandOrPath: cmdOrPath, toolName: toolName)
                out.append(OrbitGlobalPendingDecision(
                    id: key,
                    hostId: hostId,
                    hostName: hostName,
                    agentId: agentId,
                    agentTitle: agentTitle,
                    requestId: rid,
                    title: title,
                    commandOrPath: cmdOrPath,
                    toolName: toolName,
                    argumentsJson: argsJson,
                    allowedDecisions: allowed,
                    isPlanApproval: isPlan,
                    isQuestion: false,
                    riskLevel: risk
                ))
            }
            for item in stage.pendingQuestions {
                guard case let .questionRequest(_, rid, questions, _, _) = item else { continue }
                let key = "\(hostId)::\(agentId)::q::\(rid)"
                guard seenKeys.insert(key).inserted else { continue }
                out.append(OrbitGlobalPendingDecision(
                    id: key,
                    hostId: hostId,
                    hostName: hostName,
                    agentId: agentId,
                    agentTitle: agentTitle,
                    requestId: rid,
                    title: questions.first?.header ?? OrbitL10n.tr("Input Requested"),
                    commandOrPath: questions.first?.question ?? "",
                    toolName: nil,
                    argumentsJson: nil,
                    allowedDecisions: [],
                    isPlanApproval: false,
                    isQuestion: true,
                    riskLevel: .high
                ))
            }
        }

        if let hid = selectedHostId, let aid = selectedAgentId, let stage = agentStage {
            let title = activeAgent?.title ?? stage.agent?.title ?? "Agent"
            appendFromStage(hostId: hid, agentId: aid, agentTitle: title, stage: stage)
        }

        if let core {
            for host in hosts {
                let agents = hostDetailsByHostId[host.hostId]?.agents ?? []
                for agent in agents where agent.isAwaitingPermission && agent.archivedAtRfc3339 == nil {
                    let stage = core.agentStage(hostId: host.hostId, agentId: agent.agentId)
                    appendFromStage(hostId: host.hostId, agentId: agent.agentId, agentTitle: agent.title, stage: stage)
                }
            }
        }
        return out
    }

    public var liveFleetSessions: [(host: FfiHostCard, agent: FfiAgentCard)] {
        var rows: [(host: FfiHostCard, agent: FfiAgentCard)] = []
        for host in hosts {
            let agents = (hostDetailsByHostId[host.hostId]?.agents ?? [])
                .filter { $0.archivedAtRfc3339 == nil && $0.parentAgentId == nil }
            for agent in agents {
                rows.append((host: host, agent: agent))
            }
        }
        return rows.sorted { lhs, rhs in
            if lhs.agent.isAwaitingPermission != rhs.agent.isAwaitingPermission {
                return lhs.agent.isAwaitingPermission
            }
            if lhs.agent.isForegroundRunning != rhs.agent.isForegroundRunning {
                return lhs.agent.isForegroundRunning
            }
            return lhs.agent.updatedAtRfc3339 > rhs.agent.updatedAtRfc3339
        }
    }

    // MARK: - Plan Approval & Safe Low-Risk Batch Approval

    public func resolvePlanApproval(
        hostId: String? = nil,
        agentId: String? = nil,
        requestId: String,
        action: PlanApprovalAction,
        editedPlanMarkdown: String? = nil,
        feedback: String? = nil,
        targetMode: FfiSessionMode? = nil
    ) {
        if requestId.hasPrefix("demo-perm-") {
            resolvePermission(requestId: requestId, decision: action == .approveAndBuild ? "allow" : "deny")
            return
        }
        guard let core,
              let hid = hostId ?? selectedHostId,
              let aid = agentId ?? selectedAgentId else {
            return
        }
        let coreMode = targetMode?.coreSessionMode
        Task { @MainActor [weak self] in
            do {
                try await Task.detached(priority: .userInitiated) {
                    try core.resolvePlanApproval(
                        hostId: hid,
                        agentId: aid,
                        permissionRequestId: requestId,
                        action: action,
                        editedPlanMarkdown: editedPlanMarkdown,
                        feedback: feedback,
                        targetMode: coreMode
                    )
                }.value
                guard let self else { return }
                self.hapticSuccess()
                switch action {
                case .approveAndBuild:
                    self.showToast(OrbitL10n.tr("Approve & Build"))
                case .requestChanges:
                    self.showToast(OrbitL10n.tr("Send Revisions"))
                case .dismiss:
                    self.showToast(OrbitL10n.tr("Dismiss"))
                }
                self.reloadAgentStage(hostId: hid, agentId: aid)
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func approveLowRiskBatch(decisions: [OrbitGlobalPendingDecision]) {
        guard let core else { return }
        let candidates = decisions.map { item in
            OrbitBatchApprovalCandidate(
                hostId: item.hostId,
                agentId: item.agentId,
                requestId: item.requestId,
                title: item.title,
                commandOrPath: item.commandOrPath,
                toolName: item.toolName,
                argumentsJson: item.argumentsJson,
                allowedDecisions: item.allowedDecisions,
                isQuestion: item.isQuestion
            )
        }
        let safe = OrbitBatchApprovalLogic.selectSafeBatchPermissions(candidates)
        guard !safe.isEmpty else {
            showToast(OrbitL10n.tr("Plans & high-risk actions always require individual review."))
            return
        }
        let grouped = Dictionary(grouping: safe, by: \.hostId)
        Task { @MainActor [weak self] in
            let outcome: (UInt64, UInt64, String?) = await Task.detached(priority: .userInitiated) {
                var totalResolved: UInt64 = 0
                var totalSkipped: UInt64 = 0
                var firstError: String? = nil
                for (hid, items) in grouped {
                    let ids = items.map { FfiPermissionRequestId(agentId: $0.agentId, permissionRequestId: $0.requestId) }
                    do {
                        let result = try core.resolvePermissionsBatch(hostId: hid, items: ids, decision: "allow")
                        totalResolved += result.resolvedCount
                        totalSkipped += result.skippedCount
                    } catch {
                        if firstError == nil { firstError = error.localizedDescription }
                    }
                }
                return (totalResolved, totalSkipped, firstError)
            }.value
            guard let self else { return }
            if let err = outcome.2, outcome.0 == 0 {
                self.lastError = err
            } else {
                self.hapticSuccess()
                let msg = outcome.1 > 0
                    ? "Approved \(outcome.0) · Skipped \(outcome.1)"
                    : "Approved \(outcome.0)"
                self.showToast(msg)
            }
            if let hid = self.selectedHostId, let aid = self.selectedAgentId {
                self.reloadAgentStage(hostId: hid, agentId: aid)
            }
        }
    }

    // MARK: - APNs Push Notifications & ActivityKit Live Activity

    public func bindLiveActivityManager() {
        OrbitLiveActivityManager.shared.onLiveActivityTokenUpdated = { [weak self] tokenHex in
            self?.handleLiveActivityPushToken(tokenHex)
        }
        syncLiveActivityFromFleetState()
    }

    public func setLiveActivitiesEnabled(_ enabled: Bool) {
        liveActivitiesEnabled = enabled
        UserDefaults.standard.set(enabled, forKey: "orbit.ios.liveActivitiesEnabled")
        if !enabled {
            OrbitLiveActivityManager.shared.endAllActivities()
        } else {
            syncLiveActivityFromFleetState()
        }
    }

    public func requestPushPermissionsAndRegister() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { [weak self] granted, error in
            DispatchQueue.main.async { [weak self] in
                if let error {
                    self?.lastError = error.localizedDescription
                    return
                }
                guard granted else {
                    self?.showToast("Notifications disabled in iOS Settings")
                    return
                }
                UIApplication.shared.registerForRemoteNotifications()
            }
        }
    }

    public func handleApnsDeviceToken(_ tokenHex: String) {
        apnsDeviceTokenHex = tokenHex
        if let core {
            _ = try? core.registerPushToken(platform: "apns", token: tokenHex)
        }
        registerPushDeviceAcrossOnlineHosts(deviceToken: tokenHex, liveActivityToken: liveActivityTokenHex)
    }

    public func handleApnsRegistrationError(_ error: Error) {
        #if !targetEnvironment(simulator)
        lastError = error.localizedDescription
        #endif
    }

    public func handleLiveActivityPushToken(_ tokenHex: String) {
        liveActivityTokenHex = tokenHex
        guard let core else { return }
        let deviceId = OrbitKeychain.shared.stablePushDeviceId()
        let onlineHosts = hosts.filter { OrbitFormatters.isHostOnline($0.connectionState) }
        Task { @MainActor [weak self] in
            let latestSnap: FfiPushDevicesSnapshot? = await Task.detached(priority: .utility) {
                var snap: FfiPushDevicesSnapshot? = nil
                for host in onlineHosts {
                    if let next = try? core.registerLiveActivityToken(
                        hostId: host.hostId,
                        deviceId: deviceId,
                        liveActivityToken: tokenHex
                    ) {
                        snap = next
                    }
                }
                return snap
            }.value
            if let latestSnap {
                self?.pushDevicesSnapshot = latestSnap
            }
        }
    }

    public func handlePushNotificationTap(userInfo: [AnyHashable: Any]) {
        guard let action = OrbitPushDeepLinkLogic.parseNotificationUserInfo(userInfo) else { return }
        switch action {
        case let .openSession(hostId, agentId):
            navigateToSessionFromDeepLink(hostId: hostId, agentId: agentId)
        case let .pairHost(uri):
            handleScannedOrPastedUri(uri)
        case let .syncLogin(uri):
            handleScannedOrPastedUri(uri)
        }
    }

    public func registerPushDeviceAcrossOnlineHosts(deviceToken: String, liveActivityToken: String?) {
        guard let core else { return }
        let deviceId = OrbitKeychain.shared.stablePushDeviceId()
        let deviceName = UIDevice.current.name
        #if DEBUG
        let isSandbox = true
        #else
        let isSandbox = false
        #endif
        let req = FfiRegisterPushDeviceRequest(
            deviceId: deviceId,
            platform: .apns,
            deviceName: deviceName,
            deviceToken: deviceToken,
            sandbox: isSandbox,
            liveActivityToken: liveActivityToken
        )
        let onlineHosts = hosts.filter { OrbitFormatters.isHostOnline($0.connectionState) }
        Task { @MainActor [weak self] in
            let latestSnap: FfiPushDevicesSnapshot? = await Task.detached(priority: .utility) {
                var snap: FfiPushDevicesSnapshot? = nil
                for host in onlineHosts {
                    if let next = try? core.registerPushDevice(hostId: host.hostId, req: req) {
                        snap = next
                    }
                }
                return snap
            }.value
            if let latestSnap {
                self?.pushDevicesSnapshot = latestSnap
            }
        }
    }

    public func refreshPushDevices() {
        guard let core, let hid = selectedHostId else { return }
        pushBusy = true
        Task { @MainActor [weak self] in
            let snap: FfiPushDevicesSnapshot? = await Task.detached(priority: .userInitiated) {
                try? core.listPushDevices(hostId: hid)
            }.value
            guard let self else { return }
            self.pushBusy = false
            if let snap {
                self.pushDevicesSnapshot = snap
            }
        }
    }

    public func updatePushPreferences(_ prefs: PushPreferences) {
        guard let core, let hid = selectedHostId else { return }
        pushBusy = true
        let req = UpdatePushPreferencesRequest(
            preferences: prefs,
            gatewayAuthToken: nil,
            keepExistingGatewayToken: true
        )
        Task { @MainActor [weak self] in
            do {
                let snap: FfiPushDevicesSnapshot = try await Task.detached(priority: .userInitiated) {
                    try core.updatePushPreferences(hostId: hid, req: req)
                }.value
                guard let self else { return }
                self.pushBusy = false
                self.pushDevicesSnapshot = snap
                self.hapticLight()
            } catch {
                self?.pushBusy = false
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func sendTestPushNotification() {
        guard let core, let hid = selectedHostId else { return }
        pushBusy = true
        let deviceId = OrbitKeychain.shared.stablePushDeviceId()
        Task { @MainActor [weak self] in
            do {
                let res: PushTestResultView = try await Task.detached(priority: .userInitiated) {
                    try core.testPushNotification(hostId: hid, deviceId: deviceId)
                }.value
                guard let self else { return }
                self.pushBusy = false
                self.hapticSuccess()
                self.showToast("\(res.delivered)/\(res.attempted) · \(res.detail)")
            } catch {
                self?.pushBusy = false
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func syncLiveActivityFromFleetState() {
        guard liveActivitiesEnabled else {
            OrbitLiveActivityManager.shared.endAllActivities()
            return
        }
        let prioritySession: (host: FfiHostCard, agent: FfiAgentCard)? = {
            if let host = activeHost, let agent = activeAgent,
               (agent.isForegroundRunning || agent.isAwaitingPermission) {
                return (host, agent)
            }
            return liveFleetSessions.first(where: { $0.agent.isAwaitingPermission || $0.agent.isForegroundRunning })
        }()

        guard let target = prioritySession else {
            if let host = activeHost, let agent = activeAgent {
                OrbitLiveActivityManager.shared.syncSessionState(
                    enabled: true,
                    hostId: host.hostId,
                    hostName: host.displayName,
                    agentId: agent.agentId,
                    agentTitle: agent.title,
                    providerLabel: agent.providerLabel,
                    statusLabel: agent.statusLabel,
                    isRunning: false,
                    pendingDecisionCount: 0
                )
            } else {
                OrbitLiveActivityManager.shared.endAllActivities()
            }
            return
        }

        let pendingCount = Int(max(
            target.agent.isAwaitingPermission ? 1 : 0,
            Int(totalPendingApprovals)
        ))
        OrbitLiveActivityManager.shared.syncSessionState(
            enabled: true,
            hostId: target.host.hostId,
            hostName: target.host.displayName,
            agentId: target.agent.agentId,
            agentTitle: target.agent.title,
            providerLabel: target.agent.providerLabel,
            statusLabel: target.agent.statusLabel,
            isRunning: target.agent.isForegroundRunning,
            pendingDecisionCount: pendingCount
        )
    }

    // MARK: - Transcript Export & Session Usage

    public func exportActiveTranscript(
        format: FfiExportFormat,
        includeThinking: Bool,
        includeToolCalls: Bool,
        includeTimestamps: Bool
    ) async throws -> (exported: FfiExportedTranscript, fileURL: URL) {
        guard let core, let hid = selectedHostId, let aid = selectedAgentId else {
            throw OrbitStorageError.applicationSupportUnavailable
        }
        let options = FfiExportOptions(
            format: format,
            includeThinking: includeThinking,
            includeToolCalls: includeToolCalls,
            includeTimestamps: includeTimestamps
        )
        let exported: FfiExportedTranscript = try await Task.detached(priority: .userInitiated) {
            try core.exportAgentTranscript(hostId: hid, agentId: aid, options: options)
        }.value

        let safeName = (exported.suggestedFilename as NSString).lastPathComponent
        let finalName = safeName.isEmpty ? "orbit-transcript.md" : safeName
        let tempDir = FileManager.default.temporaryDirectory.appendingPathComponent("orbit-exports", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        let fileURL = tempDir.appendingPathComponent(finalName)
        try Data(exported.content.utf8).write(to: fileURL, options: [.atomic])
        return (exported, fileURL)
    }

    public func refreshActiveSessionUsage() {
        guard let core, let hid = selectedHostId, let aid = selectedAgentId else {
            activeSessionUsage = nil
            return
        }
        Task { @MainActor [weak self] in
            let usage: SessionUsage? = await Task.detached(priority: .utility) {
                try? core.getSessionUsage(hostId: hid, agentId: aid)
            }.value
            guard let self, self.selectedHostId == hid, self.selectedAgentId == aid else { return }
            self.activeSessionUsage = usage
        }
    }

    // MARK: - Global Cross-Host Search (Local FTS5 + Remote Hosts)

    public func performGlobalSearch(query rawQuery: String, hostId: String? = nil) {
        let q = rawQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        globalSearchQuery = rawQuery
        guard !q.isEmpty, let core else {
            globalSearchHits = []
            globalSearchRemoteQueried = 0
            globalSearchRemoteFailed = 0
            isSearchingGlobal = false
            return
        }
        isSearchingGlobal = true
        let req = SearchMessagesRequest(query: q, hostId: hostId, agentId: nil, limit: 60)
        Task { @MainActor [weak self] in
            let result: ([ConversationSearchHit], UInt32, UInt32) = await Task.detached(priority: .userInitiated) {
                if let res = try? core.searchMessagesRemote(req: req) {
                    return (res.hits, res.remoteHostsQueried, res.remoteHostsFailed)
                }
                let local = core.searchMessages(query: q, limit: 50)
                let converted = local.map { hit in
                    ConversationSearchHit(
                        hostId: hit.hostId,
                        hostName: hit.hostName,
                        agentId: hit.agentId,
                        seq: hit.seq,
                        turnId: nil,
                        role: hit.role,
                        title: hit.title,
                        projectName: hit.projectName,
                        snippet: hit.snippet,
                        updatedAtRfc3339: hit.updatedAtRfc3339,
                        isRemote: false,
                        isUnloaded: false
                    )
                }
                return (converted, 0, 1)
            }.value
            guard let self, self.globalSearchQuery.trimmingCharacters(in: .whitespacesAndNewlines) == q else { return }
            self.globalSearchHits = result.0
            self.globalSearchRemoteQueried = result.1
            self.globalSearchRemoteFailed = result.2
            self.isSearchingGlobal = false
        }
    }

    // MARK: - Personal Router & Fleet Mesh Management

    public var primaryRouterCandidate: RouterHostCapabilities? {
        routerHostCandidates.first(where: { $0.routerEnabled && $0.online })
            ?? routerHostCandidates.sorted(by: { $0.recommendedScore > $1.recommendedScore }).first
    }

    public func refreshRouterAndMesh() {
        guard let core else { return }
        routerBusy = true
        let hostIds = hosts.map(\.hostId)
        Task { @MainActor [weak self] in
            let fetched: ([RouterHostCapabilities], [String: [ConnectedHostSummary]], [String: RouterStatusSnapshot]) =
                await Task.detached(priority: .userInitiated) {
                    let candidates = (try? core.discoverFleetRouterHosts()) ?? []
                    let mesh = (try? core.getFleetMeshSnapshot()) ?? [:]
                    var statuses: [String: RouterStatusSnapshot] = [:]
                    for hid in hostIds {
                        if let snap = try? core.getRouterConfig(hostId: hid) {
                            statuses[hid] = snap
                        }
                    }
                    return (candidates, mesh, statuses)
                }.value
            guard let self else { return }
            self.routerHostCandidates = fetched.0
            self.fleetMeshSnapshot = fetched.1
            self.routerStatusByHost = fetched.2
            self.routerBusy = false
        }
    }

    public func updatePersonalRouterConfig(
        hostId: String,
        config: PersonalRouterConfig,
        webhookSecret: String? = nil,
        activate: Bool = true
    ) {
        guard let core else { return }
        routerBusy = true
        Task { @MainActor [weak self] in
            do {
                let updated: RouterStatusSnapshot = try await Task.detached(priority: .userInitiated) {
                    try core.updateRouterConfig(
                        hostId: hostId,
                        config: config,
                        externalWebhookSecret: webhookSecret,
                        activate: activate
                    )
                }.value
                guard let self else { return }
                self.routerStatusByHost[hostId] = updated
                self.routerBusy = false
                self.hapticSuccess()
                self.showToast("Updated Personal Agent")
                self.refreshRouterAndMesh()
            } catch {
                self?.routerBusy = false
                self?.lastError = error.localizedDescription
            }
        }
    }

    public func updateFleetMeshPeer(
        hostId: String,
        registrationId: String,
        label: String? = nil,
        enabled: Bool? = nil
    ) {
        guard let core else { return }
        Task { @MainActor [weak self] in
            do {
                try await Task.detached(priority: .userInitiated) {
                    try core.updateMeshPeer(
                        hostId: hostId,
                        registrationId: registrationId,
                        label: label,
                        enabled: enabled
                    )
                }.value
                guard let self else { return }
                self.hapticLight()
                self.refreshRouterAndMesh()
            } catch {
                self?.lastError = error.localizedDescription
            }
        }
    }

    /// Sends a quick prompt from the Personal Agent Overview dock to the active Personal Router agent
    /// (or starts a new session on the recommended host if Personal Router is not yet active).
    public func submitPersonalAgentOverviewPrompt(_ rawText: String) {
        let prompt = rawText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty else { return }
        personalAgentDraftPrompt = ""

        if let candidate = routerHostCandidates.first(where: { $0.routerEnabled && $0.activeRouterAgentId != nil }),
           let routerAgentId = candidate.activeRouterAgentId {
            selectAgent(hostId: candidate.hostId, agentId: routerAgentId)
            core?.sendPrompt(hostId: candidate.hostId, agentId: routerAgentId, prompt: prompt)
            hapticSuccess()
            return
        }

        if let targetHost = primaryRouterCandidate?.hostId ?? selectedHostId ?? hosts.first?.hostId {
            selectHost(targetHost)
            isShowingGlobalOverview = false
            composerText = prompt
            submitComposer()
        } else {
            activeSheet = .qrPair
        }
    }
}
