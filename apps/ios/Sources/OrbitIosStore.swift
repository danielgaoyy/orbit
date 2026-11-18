import Foundation
import SwiftUI
import UIKit

final class WeakListenerBridge: MobileCoreListener, @unchecked Sendable {
    weak var owner: OrbitIosStore?

    init(owner: OrbitIosStore) {
        self.owner = owner
    }

    func onCoreEvent(event: FfiCoreEvent) {
        Task { @MainActor [weak owner] in
            owner?.handleCoreEvent(event)
        }
    }
}

@MainActor
final class OrbitIosStore: ObservableObject {
    @Published var hosts: [FfiHostCard] = []
    @Published var totalPendingApprovals: UInt32 = 0
    @Published var selectedHostId: String?
    @Published var currentHostDetail: FfiHostDetail?
    @Published var selectedAgentId: String?
    @Published var timelineItems: [FfiTimelineItem] = []
    @Published var gitReview: FfiGitReview?
    @Published var terminalScreen: FfiTerminalScreen?
    @Published var lastError: String?

    private var core: OrbitMobileCore?
    private var listenerRef: WeakListenerBridge?

    init() {
        let docDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
        let dbPath = docDir?.appendingPathComponent("orbit_ios_v2.sqlite").path ?? ":memory:"
        do {
            let instance = try OrbitMobileCore.open(dbPath: dbPath)
            self.core = instance
            let bridge = WeakListenerBridge(owner: self)
            self.listenerRef = bridge
            instance.subscribe(listener: bridge)
            reloadFleet()
            instance.connectAllHosts()
        } catch {
            self.lastError = error.localizedDescription
        }
    }

    func handleCoreEvent(_ event: FfiCoreEvent) {
        switch event {
        case .fleetChanged:
            reloadFleet()
            if let hid = selectedHostId {
                reloadHostDetail(hid)
            }
        case let .hostChanged(_, hostId):
            reloadFleet()
            if hostId == selectedHostId {
                reloadHostDetail(hostId)
            }
        case let .timelineChanged(_, hostId, agentId):
            if hostId == selectedHostId && agentId == selectedAgentId {
                reloadTimeline()
            }
        case let .gitChanged(_, hostId, cwd):
            if hostId == selectedHostId {
                gitReview = core?.gitReview(hostId: hostId, cwd: cwd, path: nil)
            }
        case let .terminalChanged(_, hostId, _):
            if hostId == selectedHostId {
                terminalScreen = core?.terminalScreen(hostId: hostId)
            }
        }
    }

    func reloadFleet() {
        guard let snap = core?.fleetOverview() else { return }
        hosts = snap.hosts
        totalPendingApprovals = snap.totalPendingApprovals
        if selectedHostId == nil, let first = snap.hosts.first {
            selectHost(first.hostId)
        }
    }

    func selectHost(_ hostId: String) {
        selectedHostId = hostId
        reloadHostDetail(hostId)
        core?.refreshQuotas(hostId: hostId)
    }

    func reloadHostDetail(_ hostId: String) {
        currentHostDetail = core?.hostDetail(hostId: hostId)
        if selectedAgentId == nil, let firstAgent = currentHostDetail?.agents.first {
            selectAgent(firstAgent.agentId)
        }
    }

    func selectAgent(_ agentId: String) {
        selectedAgentId = agentId
        if let hid = selectedHostId {
            core?.openAgentTimeline(hostId: hid, agentId: agentId)
            reloadTimeline()
        }
    }

    func reloadTimeline() {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        timelineItems = core?.agentTimeline(hostId: hid, agentId: aid).items ?? []
    }

    func pairHost(uri: String) {
        let trimmed = uri.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        do {
            let card = try core?.pairHost(pairingUri: trimmed)
            lastError = nil
            UIImpactFeedbackGenerator(style: .medium).impactOccurred()
            reloadFleet()
            if let id = card?.hostId {
                selectHost(id)
            }
        } catch {
            lastError = error.localizedDescription
        }
    }

    func removeHost(_ hostId: String) {
        try? core?.removeHost(hostId: hostId)
        if selectedHostId == hostId {
            selectedHostId = nil
            currentHostDetail = nil
        }
        reloadFleet()
    }

    func updateRelay(hostId: String, relayUrl: String?) {
        try? core?.updateHostRelay(hostId: hostId, relayUrl: relayUrl)
    }

    func createAgent(cwd: String, provider: FfiProviderKind, mode: FfiSessionMode, prompt: String) {
        guard let hid = selectedHostId, !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return
        }
        core?.createAgent(
            hostId: hid,
            cwd: cwd,
            provider: provider,
            mode: mode,
            initialPrompt: prompt,
            worktreeBranch: nil
        )
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }

    func sendPrompt(_ text: String) {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        core?.sendPrompt(hostId: hid, agentId: aid, prompt: text)
    }

    func interruptAgent() {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        core?.interruptAgent(hostId: hid, agentId: aid)
        UIImpactFeedbackGenerator(style: .rigid).impactOccurred()
    }

    func resolvePermission(requestId: String, decision: String) {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        core?.resolvePermission(
            hostId: hid,
            agentId: aid,
            permissionRequestId: requestId,
            decision: decision
        )
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }

    func answerQuestion(requestId: String, option: String) {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        core?.answerQuestion(
            hostId: hid,
            agentId: aid,
            questionRequestId: requestId,
            answers: [[option]]
        )
    }

    func loadGitReview(cwd: String) {
        guard let hid = selectedHostId else { return }
        core?.fetchGitStatus(hostId: hid, cwd: cwd)
        core?.fetchGitDiff(hostId: hid, cwd: cwd, path: nil, staged: false)
        gitReview = core?.gitReview(hostId: hid, cwd: cwd, path: nil)
    }

    func startTerminal(cwd: String) {
        guard let hid = selectedHostId else { return }
        core?.startTerminal(hostId: hid, cwd: cwd, cols: 82, rows: 28)
        terminalScreen = core?.terminalScreen(hostId: hid)
    }

    func sendTerminalKeys(_ keys: String) {
        guard let hid = selectedHostId else { return }
        core?.sendTerminalKeys(hostId: hid, keys: keys)
    }
}

extension Color {
    init(hex: String) {
        let clean = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var rgb: UInt64 = 0
        Scanner(string: clean).scanHexInt64(&rgb)
        let r = Double((rgb >> 16) & 0xFF) / 255.0
        let g = Double((rgb >> 8) & 0xFF) / 255.0
        let b = Double(rgb & 0xFF) / 255.0
        self.init(red: r, green: g, blue: b)
    }
}
