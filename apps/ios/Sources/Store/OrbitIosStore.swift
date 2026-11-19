import Foundation
import Network
import SwiftUI
import UIKit

// MARK: - Navigation & Composer Models

public enum OrbitSheetDestination: String, Identifiable, Equatable {
    case gitDiff
    case pty
    case modelPicker
    case contextPicker
    case qrPair
    case directoryBrowser
    case settings

    public var id: String { rawValue }

    public var defaultDetent: PresentationDetent {
        switch self {
        case .modelPicker, .contextPicker:
            return .medium
        case .gitDiff, .pty, .settings, .directoryBrowser, .qrPair:
            return .large
        }
    }
}

public enum OrbitSlashDirective: String, CaseIterable, Identifiable, Equatable {
    case plan
    case review
    case compact

    public var id: String { rawValue }

    public var badgeLabel: String { "/\(rawValue)" }

    public var title: String {
        switch self {
        case .plan: return "Plan Architecture"
        case .review: return "Review Changes"
        case .compact: return "Compact Context"
        }
    }

    public var subtitle: String {
        switch self {
        case .plan: return "Draft step-by-step plan first"
        case .review: return "Audit working tree changes"
        case .compact: return "Compress session context"
        }
    }

    public var icon: String {
        switch self {
        case .plan: return "list.bullet.clipboard"
        case .review: return "doc.text.magnifyingglass"
        case .compact: return "arrow.down.right.and.arrow.up.left"
        }
    }

    func workflowInstruction() -> String {
        switch self {
        case .plan:
            return """
            <WORKFLOW_INSTRUCTION>
            Create a clear, step-by-step architectural and implementation plan first. Identify affected files, key trade-offs, and verification steps before making code edits.
            </WORKFLOW_INSTRUCTION>
            """
        case .review:
            return """
            <WORKFLOW_INSTRUCTION>
            Review the current repository changes thoroughly for correctness, edge cases, security, error handling, and idiomatic style. Cite exact file paths and line numbers.
            </WORKFLOW_INSTRUCTION>
            """
        case .compact:
            return "/compact"
        }
    }
}

public enum OrbitDrawerFilter: String, CaseIterable, Identifiable, Equatable {
    case all
    case active
    case approvals

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .all: return "All"
        case .active: return "Active"
        case .approvals: return "Approvals"
        }
    }
}

public enum ComposerContextItem: Identifiable, Equatable, Hashable {
    case file(path: String)
    case gitDiff(staged: Bool, label: String)
    case terminalOutput(label: String, content: String)
    case assistantQuote(id: String, label: String, excerpt: String)

    public var id: String {
        switch self {
        case let .file(path):
            return "file:\(path)"
        case let .gitDiff(staged, _):
            return "diff:\(staged ? "staged" : "unstaged")"
        case .terminalOutput:
            return "terminal:active"
        case let .assistantQuote(id, _, _):
            return "quote:\(id)"
        }
    }

    public var chipLabel: String {
        switch self {
        case let .file(path):
            return OrbitFormatters.shortPath(path)
        case let .gitDiff(_, label):
            return label
        case let .terminalOutput(label, _):
            return label
        case let .assistantQuote(_, label, _):
            return label
        }
    }

    public var icon: String {
        switch self {
        case .file:
            return "doc.text"
        case .gitDiff:
            return "arrow.triangle.branch"
        case .terminalOutput:
            return "terminal"
        case .assistantQuote:
            return "text.quote"
        }
    }
}

public struct QuotedDiffSnippet: Identifiable, Equatable, Hashable {
    public let id: String
    public let path: String
    public let lineNumber: UInt32?
    public let kind: FfiDiffLineKind
    public let content: String

    public var chipLabel: String {
        let base = OrbitFormatters.shortPath(path)
        if let line = lineNumber {
            return "\(base):L\(line)"
        }
        return base
    }
}

public struct QueuedPromptItem: Identifiable, Equatable {
    public let id: String
    public var text: String
    public var slashDirective: OrbitSlashDirective?
    public var attachedContexts: [ComposerContextItem]
    public var quotedDiffSnippets: [QuotedDiffSnippet]
    public let createdAt: Date

    public init(
        id: String = UUID().uuidString,
        text: String,
        slashDirective: OrbitSlashDirective?,
        attachedContexts: [ComposerContextItem],
        quotedDiffSnippets: [QuotedDiffSnippet],
        createdAt: Date = Date()
    ) {
        self.id = id
        self.text = text
        self.slashDirective = slashDirective
        self.attachedContexts = attachedContexts
        self.quotedDiffSnippets = quotedDiffSnippets
        self.createdAt = createdAt
    }

    public var summaryPreview: String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            return trimmed
        }
        if let dir = slashDirective {
            return dir.badgeLabel
        }
        if let firstCtx = attachedContexts.first {
            return firstCtx.chipLabel
        }
        if let firstQuote = quotedDiffSnippets.first {
            return firstQuote.chipLabel
        }
        return "Queued follow-up"
    }

    public var attachmentCount: Int {
        (slashDirective != nil ? 1 : 0) + attachedContexts.count + quotedDiffSnippets.count
    }
}

// MARK: - Throttled Core Event Listener Bridge

private final class WeakMobileListenerBridge: MobileCoreListener, @unchecked Sendable {
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

// MARK: - Orbit iOS Store

@MainActor
public final class OrbitIosStore: ObservableObject {
    // Fleet & Host State
    @Published public private(set) var hosts: [FfiHostCard] = []
    @Published public private(set) var projects: [FfiClientProject] = []
    @Published public private(set) var totalPendingApprovals: UInt32 = 0
    @Published public var selectedHostId: String?
    @Published public private(set) var currentHostDetail: FfiHostDetail?

    // Active Session & Stage (`selectedAgentId == nil` means New Session draft)
    @Published public var selectedAgentId: String?
    @Published public private(set) var agentStage: FfiAgentStage?

    // Workspace, Files, Git, Terminal
    @Published public var activeCwd: String = "~"
    @Published public var activeExtraDirs: [String] = []
    @Published public private(set) var workspaceFilesList: [String] = []
    @Published public private(set) var gitReview: FfiGitReview?
    @Published public var gitStagedMode: Bool = false
    @Published public var jumpToDiffFilePath: String?
    @Published public private(set) var terminalScreen: FfiTerminalScreen?
    @Published public private(set) var localDaemonUri: String?
    @Published public private(set) var pasteboardHasStrings: Bool = false

    // Composer State & Live Queueing
    @Published public var composerText: String = ""
    @Published public var selectedProvider: FfiProviderKind = .claudeCode
    @Published public var selectedModel: String?
    @Published public var selectedEffort: FfiReasoningEffort?
    @Published public var selectedMode: FfiSessionMode = .acceptEdits
    @Published public var slashDirective: OrbitSlashDirective?
    @Published public var attachedContexts: [ComposerContextItem] = []
    @Published public var quotedDiffSnippets: [QuotedDiffSnippet] = []
    @Published public var queuedPromptsByAgent: [String: [QueuedPromptItem]] = [:]

    // Shell, Drawer Search & Sheet Navigation
    @Published public private(set) var containerWidth: CGFloat = 0
    @Published public var isDrawerOpen: Bool = false
    @Published public var isPadSidebarCollapsed: Bool = false
    @Published public var drawerSearchQuery: String = ""
    @Published public var drawerFilter: OrbitDrawerFilter = .all
    @Published public var diffFileFilterQuery: String = ""
    @Published public var isPadDiffInspectorVisible: Bool = false
    @Published public var activeSheet: OrbitSheetDestination?
    @Published public var expandFirstTurnByDefault: Bool = false
    @Published public var expandFirstStepByDefault: Bool = false
    @Published public var expandAllWorkGroupsByDefault: Bool = false
    @Published public var keepStageScrolledToTop: Bool = false
    @Published public private(set) var demoPendingPermissions: [FfiTimelineItem] = []
    @Published public private(set) var demoPendingQuestions: [FfiTimelineItem] = []
    @Published public private(set) var demoPrioritizeQuestion: Bool = false
    @Published public private(set) var hasDemoComposerLaunch: Bool = false
    @Published public var lastError: String?
    @Published public var toastMessage: String?

    private var core: OrbitMobileCore?
    private var listenerBridge: WeakMobileListenerBridge?
    private let pathMonitor = NWPathMonitor()
    private let pathMonitorQueue = DispatchQueue(label: "dev.orbit.ios.nwpath", qos: .utility)
    private var lastNetworkSignature: String?
    private var lastTerminalDimensions: (cols: UInt16, rows: UInt16) = (80, 24)
    private var toastDismissTask: Task<Void, Never>?
    private var lastKnownRunningByAgentKey: [String: Bool] = [:]
    private var autoSelectLatestAgentOnLoad: Bool = false
    private var pendingLaunchPadDiff: Bool = false
    private var explicitDiffSheetRequested: Bool = false
    private var pendingLaunchPtyCommand: String?
    private var ptyLaunchCmdDispatched: Bool = false

    public init() {
        let docDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
        let dbPath = docDir?.appendingPathComponent("orbit_ios_v3.sqlite").path ?? ":memory:"

        do {
            let instance: OrbitMobileCore
            do {
                instance = try OrbitMobileCore.openWithOptions(dbPath: dbPath, autoDiscoverLocal: true)
            } catch let openError {
                NSLog("[OrbitIosStore] Failed to open SQLite at %@: %@ — falling back to in-memory store", dbPath, openError.localizedDescription)
                self.lastError = "Storage fallback active: \(openError.localizedDescription)"
                instance = try OrbitMobileCore.openInMemory()
            }
            self.core = instance

            let bridge = WeakMobileListenerBridge(owner: self)
            self.listenerBridge = bridge
            instance.subscribeThrottled(listener: bridge, intervalMs: 24)

            self.localDaemonUri = instance.discoverLocalDaemonUri()
            self.refreshPasteboardAvailability()

            reloadFleet()
            instance.connectAllHosts()
            startNetworkMonitor()
        } catch {
            self.lastError = error.localizedDescription
        }
    }

    deinit {
        pathMonitor.cancel()
    }

    // MARK: - Computed Helpers

    public var activeHost: FfiHostCard? {
        if let hid = selectedHostId {
            return hosts.first(where: { $0.hostId == hid }) ?? currentHostDetail?.host
        }
        return hosts.first
    }

    public var activeAgent: FfiAgentCard? {
        if let stageAgent = agentStage?.agent {
            return stageAgent
        }
        guard let aid = selectedAgentId else { return nil }
        return currentHostDetail?.agents.first(where: { $0.agentId == aid })
    }

    public var isDraftNewSession: Bool {
        selectedAgentId == nil
    }

    public var isActiveAgentRunning: Bool {
        if let agent = activeAgent, agent.isRunning {
            return true
        }
        if let lastTurn = agentStage?.turns.last, lastTurn.hasActiveItem && lastTurn.terminalReply == nil {
            return true
        }
        return false
    }

    public var canShowInlineDiffInspector: Bool {
        guard UIDevice.current.userInterfaceIdiom == .pad, containerWidth >= 700 else {
            return false
        }
        if containerWidth >= 1024 {
            return true
        }
        let sidebarOccupied: CGFloat = isPadSidebarCollapsed ? 0 : OrbitSpacing.sidebarWidth
        return (containerWidth - sidebarOccupied) >= 780
    }

    public var hasDemoStageOverlay: Bool {
        !demoPendingPermissions.isEmpty || !demoPendingQuestions.isEmpty || hasDemoComposerLaunch
    }

    public var activeAgentQueueKey: String? {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return nil }
        return "\(hid)::\(aid)"
    }

    public var activeQueuedPrompts: [QueuedPromptItem] {
        guard let key = activeAgentQueueKey else { return [] }
        return queuedPromptsByAgent[key] ?? []
    }

    public var activeGitBranch: String? {
        if let branch = gitReview?.branch, !branch.isEmpty {
            return branch
        }
        if let repo = currentHostDetail?.discoveredProjects.first(where: {
            OrbitFormatters.arePathsEquivalent($0.path, activeCwd)
        }), let branch = repo.gitBranch, !branch.isEmpty {
            return branch
        }
        if let ws = currentHostDetail?.workspaces.first(where: {
            OrbitFormatters.arePathsEquivalent($0.cwd, activeCwd)
        }), let branch = ws.gitBranch, !branch.isEmpty {
            return branch
        }
        return nil
    }

    public var diffTotals: (additions: UInt32, deletions: UInt32, fileCount: Int) {
        if let review = gitReview, !review.diffFiles.isEmpty {
            let add = review.diffFiles.reduce(0) { $0 + $1.additions }
            let del = review.diffFiles.reduce(0) { $0 + $1.deletions }
            return (add, del, review.diffFiles.count)
        }
        if let turnDiffs = agentStage?.sessionTurnDiffs, !turnDiffs.isEmpty {
            let add = turnDiffs.reduce(0) { $0 + $1.additions }
            let del = turnDiffs.reduce(0) { $0 + $1.deletions }
            return (add, del, turnDiffs.count)
        }
        return (0, 0, 0)
    }

    public var hostPinnedProjects: [FfiClientProject] {
        guard let hid = selectedHostId else { return [] }
        return projects.filter { $0.hostId == hid }
    }

    public func catalog(for provider: FfiProviderKind) -> FfiProviderModelCatalog {
        if let found = currentHostDetail?.providerCatalogs.first(where: { $0.provider == provider }),
           !found.models.isEmpty {
            return found
        }
        return fallbackProviderCatalog(provider: provider)
    }

    public var activeModelSpec: FfiProviderModelSpec? {
        let cat = catalog(for: selectedProvider)
        if let modelId = selectedModel {
            return cat.models.first(where: { $0.id == modelId })
        }
        if let defId = cat.configuredDefaultModel {
            return cat.models.first(where: { $0.id == defId })
        }
        return cat.models.first(where: { $0.isDefault }) ?? cat.models.first
    }

    public var composerModelSummaryLabel: String {
        let cat = catalog(for: selectedProvider)
        let modelLabel = formatCatalogModelLabel(catalog: cat, rawModelId: selectedModel)
        if let effort = selectedEffort {
            let shortEffort = OrbitFormatters.compactEffortLabel(effort)
            return "\(modelLabel) · \(shortEffort)"
        }
        return modelLabel
    }

    public var canSubmitComposer: Bool {
        guard selectedHostId != nil else { return false }
        let hasText = !composerText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let hasAttachments = !attachedContexts.isEmpty || !quotedDiffSnippets.isEmpty
        if slashDirective == .review || slashDirective == .compact {
            return true
        }
        return hasText || hasAttachments
    }

    // MARK: - App Lifecycle, Network & Launch Arguments

    private func startNetworkMonitor() {
        pathMonitor.pathUpdateHandler = { [weak self] path in
            let sig = "\(path.status)-\(path.usesInterfaceType(.wifi))-\(path.usesInterfaceType(.cellular))-\(path.usesInterfaceType(.wiredEthernet))"
            Task { @MainActor [weak self] in
                guard let self = self else { return }
                if let previous = self.lastNetworkSignature, previous != sig {
                    self.core?.onNetworkChanged()
                }
                self.lastNetworkSignature = sig
            }
        }
        pathMonitor.start(queue: pathMonitorQueue)
    }

    public func onForeground() {
        core?.onAppForeground()
        localDaemonUri = core?.discoverLocalDaemonUri()
        refreshPasteboardAvailability()
        reloadFleet()
        if let hid = selectedHostId {
            reloadHostDetail(hid)
            if let aid = selectedAgentId {
                reloadAgentStage(hostId: hid, agentId: aid)
            }
        }
    }

    public func onBackground() {
        core?.onAppBackground()
    }

    public func refreshPasteboardAvailability() {
        pasteboardHasStrings = UIPasteboard.general.hasStrings || UIPasteboard.general.hasURLs
    }

    public func readPairingUriFromClipboard() -> String? {
        guard let raw = UIPasteboard.general.string?.trimmingCharacters(in: .whitespacesAndNewlines),
              !raw.isEmpty else {
            return nil
        }
        if let range = raw.range(of: "orbit://pair/[^\\s\"']+", options: .regularExpression) {
            return String(raw[range])
        }
        return raw
    }

    public func detectValidPairUriInClipboard() -> String? {
        guard UIPasteboard.general.hasStrings || UIPasteboard.general.hasURLs else {
            return nil
        }
        guard let raw = UIPasteboard.general.string?.trimmingCharacters(in: .whitespacesAndNewlines),
              !raw.isEmpty else {
            return nil
        }
        if let range = raw.range(of: "orbit://pair/[^\\s\"']+", options: [.regularExpression, .caseInsensitive]) {
            return String(raw[range])
        }
        if raw.lowercased().hasPrefix("orbit://pair/") {
            return raw
        }
        return nil
    }

    public func handleDeepLink(_ url: URL) {
        let raw = url.absoluteString.trimmingCharacters(in: .whitespacesAndNewlines)
        guard raw.lowercased().hasPrefix("orbit://") else { return }
        pairHost(uri: raw)
        if activeSheet == .qrPair {
            activeSheet = nil
        }
    }

    public func applyLaunchArguments(_ args: [String], setAppearance: (OrbitAppearanceMode) -> Void) {
        #if DEBUG
        if let themeIdx = args.firstIndex(of: "-orbitTheme"), themeIdx + 1 < args.count {
            let raw = args[themeIdx + 1].lowercased()
            if raw == "graphite" {
                setAppearance(.dark)
            } else if let mode = OrbitAppearanceMode(rawValue: raw) {
                setAppearance(mode)
            }
        }

        if args.contains("-orbitExpandFirstTurn") {
            expandFirstTurnByDefault = true
        }

        if args.contains("-orbitExpandFirstStep") {
            expandFirstStepByDefault = true
            expandFirstTurnByDefault = true
        }

        if args.contains("-orbitExpandWork") {
            expandAllWorkGroupsByDefault = true
        }

        if args.contains("-orbitScrollTop") {
            keepStageScrolledToTop = true
        }

        if args.contains("-orbitSelectLatestAgent") {
            autoSelectLatestAgentOnLoad = true
        }

        if args.contains("-orbitCollapseSidebar") {
            isPadSidebarCollapsed = true
        }

        if args.contains("-orbitOpenPadDiff") {
            pendingLaunchPadDiff = true
            if canShowInlineDiffInspector {
                isPadDiffInspectorVisible = true
            } else {
                isPadDiffInspectorVisible = false
                activeSheet = .gitDiff
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
                guard let self = self else { return }
                self.pendingLaunchPadDiff = false
                self.openGitDiff()
            }
        }

        if let drawerSearchIdx = args.firstIndex(of: "-orbitDrawerSearch"), drawerSearchIdx + 1 < args.count {
            drawerSearchQuery = args[drawerSearchIdx + 1]
            isDrawerOpen = true
        }

        if let diffFilterIdx = args.firstIndex(of: "-orbitDiffFilter"), diffFilterIdx + 1 < args.count {
            diffFileFilterQuery = args[diffFilterIdx + 1]
        }

        if args.contains("-orbitDemoApproval") || args.contains("-orbitDemoQuestion") {
            let prioritizeQuestion = args.contains("-orbitDemoQuestion")
            demoPrioritizeQuestion = prioritizeQuestion
            demoPendingPermissions = [
                .permissionRequest(
                    seq: prioritizeQuestion ? 900_002 : 900_001,
                    requestId: "demo-perm-cargo-test",
                    title: "Run Workspace Release Tests",
                    commandOrPath: "cargo test --workspace --release",
                    detail: .shell(
                        command: "cargo test --workspace --release",
                        cwd: activeCwd == "~" ? "~/Code/orbit" : activeCwd,
                        exitCode: nil,
                        outputPreview: nil
                    ),
                    allowedDecisions: ["deny", "allow_always", "allow"],
                    resolvedDecision: nil,
                    timestampMs: nil
                )
            ]
            demoPendingQuestions = [
                .questionRequest(
                    seq: prioritizeQuestion ? 900_001 : 900_002,
                    requestId: "demo-question-test-scope",
                    questions: [
                        FfiQuestionItem(
                            id: "q-test-scope",
                            header: "Verification Strategy",
                            question: "Which test suite should run before packaging?",
                            options: [
                                "(Recommended) Full workspace + FFI smoke suite",
                                "Changed crates only"
                            ],
                            isMultiSelect: false
                        )
                    ],
                    resolvedAnswers: nil,
                    timestampMs: nil
                )
            ]
        }

        if let demoCompIdx = args.firstIndex(of: "-orbitDemoComposer"), demoCompIdx + 1 < args.count {
            hasDemoComposerLaunch = true
            composerText = args[demoCompIdx + 1]
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
                self?.refreshWorkspaceFiles()
            }
        }

        if let execPtyIdx = args.firstIndex(of: "-orbitExecPty"), execPtyIdx + 1 < args.count {
            let cmd = args[execPtyIdx + 1]
            if !cmd.isEmpty {
                pendingLaunchPtyCommand = cmd
                ptyLaunchCmdDispatched = false
            }
        } else if let ptyCmdIdx = args.firstIndex(of: "-orbitPtyCmd"), ptyCmdIdx + 1 < args.count {
            let cmd = args[ptyCmdIdx + 1]
            if !cmd.isEmpty {
                pendingLaunchPtyCommand = cmd
                ptyLaunchCmdDispatched = false
            }
        }

        if let pairIdx = args.firstIndex(of: "-orbitPairUri"), pairIdx + 1 < args.count {
            let uri = args[pairIdx + 1]
            if !uri.isEmpty {
                _ = pairHost(uri: uri)
            }
        }

        if autoSelectLatestAgentOnLoad, selectedAgentId == nil,
           let firstAgent = currentHostDetail?.agents.first {
            autoSelectLatestAgentOnLoad = false
            selectAgent(firstAgent.agentId)
        }

        if let sheetIdx = args.firstIndex(of: "-orbitOpenSheet"), sheetIdx + 1 < args.count {
            let target = args[sheetIdx + 1].lowercased()
            if target == "diff" || target == "gitdiff" {
                explicitDiffSheetRequested = true
                openGitDiff(preferSheet: true)
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
                guard let self = self else { return }
                switch target {
                case "drawer":
                    self.isDrawerOpen = true
                case "diff", "gitdiff":
                    self.openGitDiff(preferSheet: true)
                case "pty", "terminal":
                    self.activeSheet = .pty
                case "model", "models":
                    self.activeSheet = .modelPicker
                case "context":
                    self.activeSheet = .contextPicker
                case "pair", "qr":
                    self.activeSheet = .qrPair
                case "directory", "files":
                    self.openDirectoryBrowser()
                case "settings", "quotas":
                    self.activeSheet = .settings
                default:
                    break
                }
            }
        }
        #endif
    }

    // MARK: - Core Event Handling

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
            if hostId == selectedHostId, agentId == selectedAgentId {
                reloadAgentStage(hostId: hostId, agentId: agentId)
            }

        case let .gitChanged(_, hostId, _):
            if hostId == selectedHostId {
                gitReview = core?.gitReview(hostId: hostId, cwd: activeCwd, path: nil)
                workspaceFilesList = core?.workspaceFiles(hostId: hostId, cwd: activeCwd) ?? workspaceFilesList
            }

        case let .terminalChanged(_, hostId, _):
            if hostId == selectedHostId {
                terminalScreen = core?.terminalScreen(hostId: hostId)
            }
        }
    }

    // MARK: - Fleet, Host & Session Selection

    public func reloadFleet() {
        guard let snap = core?.fleetOverview() else { return }
        hosts = snap.hosts
        projects = snap.projects
        totalPendingApprovals = snap.totalPendingApprovals

        if selectedHostId == nil, let firstHost = snap.hosts.first {
            selectHost(firstHost.hostId)
        } else if let hid = selectedHostId, !snap.hosts.contains(where: { $0.hostId == hid }) {
            selectedHostId = snap.hosts.first?.hostId
            if let nextId = selectedHostId {
                selectHost(nextId)
            } else {
                currentHostDetail = nil
                selectedAgentId = nil
                agentStage = nil
            }
        }
    }

    public func selectHost(_ hostId: String) {
        let changed = selectedHostId != hostId
        selectedHostId = hostId
        if changed {
            selectedAgentId = nil
            agentStage = nil
            gitReview = nil
            terminalScreen = nil
        }
        core?.connectHost(hostId: hostId)
        core?.refreshHost(hostId: hostId)
        core?.refreshProviderModels(hostId: hostId, forceRefresh: false)
        core?.refreshQuotas(hostId: hostId)
        reloadHostDetail(hostId)
    }

    public func reloadHostDetail(_ hostId: String) {
        guard let detail = core?.hostDetail(hostId: hostId) else { return }
        currentHostDetail = detail

        // Migrate queued prompts and running state across optimistic -> permanent agent IDs
        for (oldAid, newAid) in detail.replacedOptimisticAgents {
            let oldKey = "\(hostId)::\(oldAid)"
            let newKey = "\(hostId)::\(newAid)"
            if let queued = queuedPromptsByAgent.removeValue(forKey: oldKey), !queued.isEmpty {
                var existing = queuedPromptsByAgent[newKey] ?? []
                existing.append(contentsOf: queued)
                queuedPromptsByAgent[newKey] = existing
            }
            if let wasRun = lastKnownRunningByAgentKey.removeValue(forKey: oldKey) {
                lastKnownRunningByAgentKey[newKey] = wasRun
            }
        }

        // Seamlessly migrate optimistic agent ID when daemon assigns permanent ID
        if let currentAid = selectedAgentId,
           let replacementAid = detail.replacedOptimisticAgents[currentAid] {
            selectedAgentId = replacementAid
            core?.openAgentTimeline(hostId: hostId, agentId: replacementAid)
            reloadAgentStage(hostId: hostId, agentId: replacementAid)
        }

        // Sync default workspace CWD if unset or "~"
        if activeCwd == "~" || activeCwd.isEmpty {
            if let firstPinned = projects.first(where: { $0.hostId == hostId })?.directories.first {
                updateActiveWorkspaceCwd(firstPinned)
            } else if let firstWs = detail.workspaces.first?.cwd {
                updateActiveWorkspaceCwd(firstWs)
            } else if let firstRepo = detail.discoveredProjects.first?.path {
                updateActiveWorkspaceCwd(firstRepo)
            }
        }

        // Ensure selectedProvider defaults to an available provider on host when in draft mode
        if isDraftNewSession, !detail.host.availableProviders.isEmpty,
           !detail.host.availableProviders.contains(selectedProvider),
           let firstAvailable = detail.host.availableProviders.first {
            selectedProvider = firstAvailable
            syncDefaultModelAndEffort(for: firstAvailable)
        } else if selectedModel == nil && selectedEffort == nil {
            syncDefaultModelAndEffort(for: selectedProvider)
        }

        if terminalScreen == nil, !detail.terminals.isEmpty {
            terminalScreen = core?.terminalScreen(hostId: hostId)
        }

        if autoSelectLatestAgentOnLoad, selectedAgentId == nil,
           let firstAgent = detail.agents.first {
            autoSelectLatestAgentOnLoad = false
            selectAgent(firstAgent.agentId)
        }

        if (isPadDiffInspectorVisible || activeSheet == .gitDiff),
           (detail.host.connectionState == .onlineDirect || detail.host.connectionState == .onlineRelay),
           (gitReview == nil || !OrbitFormatters.arePathsEquivalent(gitReview?.cwd ?? "", activeCwd)) {
            refreshGitReview()
        }

        // Evaluate running -> settled transitions for background sessions with queued prompts
        for agent in detail.agents where agent.agentId != selectedAgentId {
            let busy = agent.isRunning || agent.isAwaitingPermission
            evaluateAgentQueueDrain(hostId: hostId, agentId: agent.agentId, isBusy: busy)
        }
    }

    private func syncDefaultModelAndEffort(for provider: FfiProviderKind) {
        let cat = catalog(for: provider)
        let defaultSpec = cat.configuredDefaultModel.flatMap { id in cat.models.first(where: { $0.id == id }) }
            ?? cat.models.first(where: { $0.isDefault })
            ?? cat.models.first
        selectedModel = defaultSpec?.id
        selectedEffort = resolveSupportedEffortForModel(
            spec: defaultSpec,
            desired: selectedEffort,
            catalogDefault: cat.configuredDefaultEffort
        )
    }

    public func selectAgent(_ agentId: String) {
        guard let hid = selectedHostId else { return }
        selectedAgentId = agentId
        isDrawerOpen = false
        core?.openAgentTimeline(hostId: hid, agentId: agentId)
        reloadAgentStage(hostId: hid, agentId: agentId)

        if let card = currentHostDetail?.agents.first(where: { $0.agentId == agentId }) ?? agentStage?.agent {
            selectedProvider = card.provider
            selectedModel = card.model
            selectedEffort = card.effort
            selectedMode = card.mode
            updateActiveWorkspaceCwd(card.cwd)
        }
        hapticLight()
    }

    public func reloadAgentStage(hostId: String, agentId: String) {
        let stage = core?.agentStage(hostId: hostId, agentId: agentId)
        agentStage = stage
        if let card = stage?.agent {
            selectedProvider = card.provider
            if let m = card.model { selectedModel = m }
            if let e = card.effort { selectedEffort = e }
            selectedMode = card.mode
            if !card.cwd.isEmpty && card.cwd != activeCwd {
                updateActiveWorkspaceCwd(card.cwd)
            }
        }

        if hostId == selectedHostId && agentId == selectedAgentId {
            let awaitingDecision = !(stage?.pendingPermissions.isEmpty ?? true)
                || !(stage?.pendingQuestions.isEmpty ?? true)
                || (stage?.agent?.isAwaitingPermission ?? false)
            let isBusy = isActiveAgentRunning || awaitingDecision
            evaluateAgentQueueDrain(hostId: hostId, agentId: agentId, isBusy: isBusy)
        }
    }

    public func startNewSessionDraft(
        cwd: String? = nil,
        extraDirs: [String] = [],
        provider: FfiProviderKind? = nil
    ) {
        selectedAgentId = nil
        agentStage = nil
        isDrawerOpen = false
        activeExtraDirs = extraDirs
        if let provider = provider {
            selectedProvider = provider
            syncDefaultModelAndEffort(for: provider)
        }
        if let cwd = cwd, !cwd.isEmpty {
            updateActiveWorkspaceCwd(cwd)
        }
        hapticLight()
    }

    public func openTerminal(inCwd cwd: String? = nil) {
        guard let hid = selectedHostId else {
            activeSheet = .qrPair
            return
        }
        isDrawerOpen = false
        if let cwd = cwd, !cwd.isEmpty {
            let changedCwd = !OrbitFormatters.arePathsEquivalent(activeCwd, cwd)
            updateActiveWorkspaceCwd(cwd)
            if changedCwd {
                let dims = lastTerminalDimensions
                core?.startTerminal(hostId: hid, cwd: cwd, cols: dims.cols, rows: dims.rows)
                reloadHostDetail(hid)
                terminalScreen = core?.terminalScreen(hostId: hid)
            }
        }
        hapticLight()
        activeSheet = .pty
    }

    public func inspectGitDiff(inCwd cwd: String) {
        isDrawerOpen = false
        if !cwd.isEmpty {
            updateActiveWorkspaceCwd(cwd)
        }
        hapticLight()
        openGitDiff()
    }

    public func copyPathToClipboard(_ path: String) {
        let trimmed = path.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        UIPasteboard.general.string = trimmed
        refreshPasteboardAvailability()
        hapticLight()
        showToast("Copied path")
    }

    private func updateActiveWorkspaceCwd(_ cwd: String) {
        guard !cwd.isEmpty else { return }
        let changed = activeCwd != cwd
        activeCwd = cwd
        if changed, let hid = selectedHostId {
            core?.fetchGitStatus(hostId: hid, cwd: cwd)
            core?.fetchGitDiff(hostId: hid, cwd: cwd, path: nil, staged: gitStagedMode)
            gitReview = core?.gitReview(hostId: hid, cwd: cwd, path: nil)
            core?.fetchWorkspaceFiles(hostId: hid, cwd: cwd)
            workspaceFilesList = core?.workspaceFiles(hostId: hid, cwd: cwd) ?? []
        }
    }

    public func archiveSession(_ agentId: String) {
        guard let hid = selectedHostId else { return }
        core?.archiveAgent(hostId: hid, agentId: agentId)
        if selectedAgentId == agentId {
            selectedAgentId = nil
            agentStage = nil
        }
        reloadHostDetail(hid)
        hapticLight()
        showToast("Session archived")
    }

    // MARK: - Projects & Remote Directory Browser

    public func openDirectoryBrowser(initialPath: String? = nil) {
        guard let hid = selectedHostId else {
            activeSheet = .qrPair
            return
        }
        let target = initialPath ?? (activeCwd == "~" ? nil : activeCwd)
        core?.listDirectory(hostId: hid, path: target)
        activeSheet = .directoryBrowser
    }

    public func listRemoteDirectory(_ path: String?) {
        guard let hid = selectedHostId else { return }
        core?.listDirectory(hostId: hid, path: path)
    }

    public func pinProject(name: String, directories: [String]) {
        guard let hid = selectedHostId, !directories.isEmpty else { return }
        do {
            _ = try core?.upsertProject(
                projectId: nil,
                hostId: hid,
                name: name,
                directories: directories
            )
            reloadFleet()
            hapticSuccess()
            showToast("Pinned \(name)")
        } catch {
            lastError = error.localizedDescription
        }
    }

    public func unpinProject(_ projectId: String) {
        do {
            try core?.removeProject(projectId: projectId)
            reloadFleet()
            hapticLight()
        } catch {
            lastError = error.localizedDescription
        }
    }

    public func refreshWorkspaceFiles() {
        guard let hid = selectedHostId else { return }
        core?.fetchWorkspaceFiles(hostId: hid, cwd: activeCwd)
        workspaceFilesList = core?.workspaceFiles(hostId: hid, cwd: activeCwd) ?? []
    }

    // MARK: - Pairing & Host Config

    @discardableResult
    public func pairHost(uri: String) -> Bool {
        let trimmed = uri.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return false }
        guard let core else {
            lastError = "Orbit core is not ready."
            hapticWarning()
            return false
        }
        do {
            let card = try core.pairHost(pairingUri: trimmed)
            lastError = nil
            hapticSuccess()
            reloadFleet()
            selectHost(card.hostId)
            showToast("Connected to \(card.displayName)")
            return true
        } catch {
            lastError = error.localizedDescription
            hapticWarning()
            return false
        }
    }

    public func connectLocalSimulatorHost() -> Bool {
        guard let uri = localDaemonUri ?? core?.discoverLocalDaemonUri() else {
            lastError = "No local Orbit daemon detected on this host."
            return false
        }
        return pairHost(uri: uri)
    }

    public func saveHostConfig(
        hostId: String,
        customName: String?,
        routeMode: FfiHostRouteMode,
        customDirectEndpoint: String?,
        relayUrl: String?
    ) {
        let targetHost = hosts.first(where: { $0.hostId == hostId })
        let cleanName = customName?.trimmingCharacters(in: .whitespacesAndNewlines)
        let cleanDirect = customDirectEndpoint?.trimmingCharacters(in: .whitespacesAndNewlines)
        let cleanRelay = relayUrl?.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            _ = try core?.updateHostConfig(
                hostId: hostId,
                customName: (cleanName?.isEmpty == false) ? cleanName : nil,
                routeMode: routeMode,
                sshAlias: targetHost?.sshAlias,
                remoteDaemonPort: targetHost?.remoteDaemonPort,
                customDirectEndpoint: (cleanDirect?.isEmpty == false) ? cleanDirect : nil,
                relayUrl: (cleanRelay?.isEmpty == false) ? cleanRelay : nil,
                reconnect: true
            )
            lastError = nil
            reloadFleet()
            if hostId == selectedHostId {
                reloadHostDetail(hostId)
            }
            hapticSuccess()
            showToast("Updated host route settings")
        } catch {
            lastError = error.localizedDescription
            hapticWarning()
        }
    }

    public func removeHost(_ hostId: String) {
        do {
            try core?.removeHost(hostId: hostId)
            reloadFleet()
            hapticMedium()
            showToast("Removed host")
        } catch {
            lastError = error.localizedDescription
        }
    }

    public func refreshProviderModels(force: Bool = true) {
        guard let hid = selectedHostId else { return }
        core?.refreshProviderModels(hostId: hid, forceRefresh: force)
    }

    public func refreshQuotas() {
        guard let hid = selectedHostId else { return }
        core?.refreshQuotas(hostId: hid)
    }

    // MARK: - Model, Provider & Mode Configuration

    public func applyModelSelection(
        provider: FfiProviderKind,
        modelId: String?,
        effort: FfiReasoningEffort?
    ) {
        let cat = catalog(for: provider)
        let spec = modelId.flatMap { id in cat.models.first(where: { $0.id == id }) }
            ?? cat.models.first(where: { $0.isDefault })
            ?? cat.models.first
        let resolvedEffort = resolveSupportedEffortForModel(
            spec: spec,
            desired: effort,
            catalogDefault: cat.configuredDefaultEffort
        )

        let providerChanged = selectedProvider != provider
        selectedProvider = provider
        selectedModel = spec?.id ?? modelId
        selectedEffort = resolvedEffort

        if let hid = selectedHostId, let aid = selectedAgentId {
            if providerChanged {
                core?.switchAgentProvider(
                    hostId: hid,
                    agentId: aid,
                    provider: provider,
                    model: selectedModel,
                    effort: resolvedEffort
                )
            } else {
                core?.setAgentConfig(
                    hostId: hid,
                    agentId: aid,
                    model: selectedModel,
                    effort: resolvedEffort
                )
            }
            reloadAgentStage(hostId: hid, agentId: aid)
        }
        hapticLight()
    }

    public func applySessionMode(_ mode: FfiSessionMode) {
        selectedMode = mode
        if let hid = selectedHostId, let aid = selectedAgentId {
            core?.setSessionMode(hostId: hid, agentId: aid, mode: mode)
            reloadAgentStage(hostId: hid, agentId: aid)
        }
        hapticLight()
    }

    // MARK: - Composer Context & Prompt Submission

    public func toggleContextItem(_ item: ComposerContextItem) {
        if let idx = attachedContexts.firstIndex(of: item) {
            attachedContexts.remove(at: idx)
        } else {
            attachedContexts.append(item)
        }
        hapticLight()
    }

    public func removeContextItem(_ item: ComposerContextItem) {
        attachedContexts.removeAll(where: { $0 == item })
        hapticLight()
    }

    public func removeQuotedSnippet(_ snippet: QuotedDiffSnippet) {
        quotedDiffSnippets.removeAll(where: { $0.id == snippet.id })
        hapticLight()
    }

    public func applyQuickWorkflow(_ directive: OrbitSlashDirective?, promptText: String = "") {
        slashDirective = directive
        if directive == .plan {
            selectedMode = .plan
        }
        if !promptText.isEmpty {
            composerText = promptText
        }
        hapticLight()
    }

    public func submitComposer() {
        guard let hid = selectedHostId else {
            activeSheet = .qrPair
            return
        }

        if isActiveAgentRunning, selectedAgentId != nil {
            enqueueComposerPrompt()
            return
        }

        let draftText = composerText
        let draftDirective = slashDirective
        let draftContexts = attachedContexts
        let draftQuotes = quotedDiffSnippets

        let parsed = parseInlineDirective(rawInput: draftText, explicitDirective: draftDirective)
        if parsed.directive == .compact && selectedAgentId == nil {
            showToast("Start a session first to compact context")
            return
        }
        let hasContent = !parsed.cleanText.isEmpty
            || !draftContexts.isEmpty
            || !draftQuotes.isEmpty
            || parsed.directive == .review
            || parsed.directive == .compact
        guard hasContent else { return }

        // Reset draft immediately for snappy 60/120fps responsiveness
        composerText = ""
        slashDirective = nil
        attachedContexts.removeAll()
        quotedDiffSnippets.removeAll()
        hapticLight()

        dispatchPromptInputs(
            hostId: hid,
            agentId: selectedAgentId,
            rawInput: draftText,
            explicitDirective: draftDirective,
            contexts: draftContexts,
            quotes: draftQuotes
        )
    }

    public func enqueueComposerPrompt() {
        guard let hid = selectedHostId, let aid = selectedAgentId else {
            submitComposer()
            return
        }
        let parsed = parseInlineDirective(rawInput: composerText, explicitDirective: slashDirective)
        let hasContent = !parsed.cleanText.isEmpty
            || !attachedContexts.isEmpty
            || !quotedDiffSnippets.isEmpty
            || parsed.directive == .review
            || parsed.directive == .compact
        guard hasContent else { return }

        let item = QueuedPromptItem(
            text: parsed.cleanText,
            slashDirective: parsed.directive,
            attachedContexts: attachedContexts,
            quotedDiffSnippets: quotedDiffSnippets
        )
        let key = "\(hid)::\(aid)"
        var queue = queuedPromptsByAgent[key] ?? []
        queue.append(item)
        queuedPromptsByAgent[key] = queue
        lastKnownRunningByAgentKey[key] = true

        composerText = ""
        slashDirective = nil
        attachedContexts.removeAll()
        quotedDiffSnippets.removeAll()
        hapticLight()
        showToast("Queued next prompt")
    }

    public func editQueuedPrompt(_ item: QueuedPromptItem) {
        guard let key = activeAgentQueueKey else { return }
        removeQueuedPrompt(item, inKey: key)
        composerText = item.text
        slashDirective = item.slashDirective
        attachedContexts = item.attachedContexts
        quotedDiffSnippets = item.quotedDiffSnippets
        hapticLight()
    }

    public func removeQueuedPrompt(_ item: QueuedPromptItem) {
        guard let key = activeAgentQueueKey else { return }
        removeQueuedPrompt(item, inKey: key)
        hapticLight()
    }

    public func sendQueuedPromptNow(_ item: QueuedPromptItem) {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        let key = "\(hid)::\(aid)"
        removeQueuedPrompt(item, inKey: key)
        lastKnownRunningByAgentKey[key] = true
        hapticLight()
        dispatchPromptInputs(
            hostId: hid,
            agentId: aid,
            rawInput: item.text,
            explicitDirective: item.slashDirective,
            contexts: item.attachedContexts,
            quotes: item.quotedDiffSnippets
        )
    }

    private func removeQueuedPrompt(_ item: QueuedPromptItem, inKey key: String) {
        guard var list = queuedPromptsByAgent[key] else { return }
        list.removeAll(where: { $0.id == item.id })
        if list.isEmpty {
            queuedPromptsByAgent.removeValue(forKey: key)
        } else {
            queuedPromptsByAgent[key] = list
        }
    }

    private func evaluateAgentQueueDrain(hostId: String, agentId: String, isBusy: Bool) {
        let key = "\(hostId)::\(agentId)"
        let wasRunning = lastKnownRunningByAgentKey[key] ?? false
        lastKnownRunningByAgentKey[key] = isBusy

        if wasRunning && !isBusy {
            guard var queue = queuedPromptsByAgent[key], !queue.isEmpty else { return }
            let nextItem = queue.removeFirst()
            if queue.isEmpty {
                queuedPromptsByAgent.removeValue(forKey: key)
            } else {
                queuedPromptsByAgent[key] = queue
            }
            lastKnownRunningByAgentKey[key] = true
            dispatchPromptInputs(
                hostId: hostId,
                agentId: agentId,
                rawInput: nextItem.text,
                explicitDirective: nextItem.slashDirective,
                contexts: nextItem.attachedContexts,
                quotes: nextItem.quotedDiffSnippets
            )
            showToast("Sent queued prompt")
        }
    }

    private func parseInlineDirective(
        rawInput: String,
        explicitDirective: OrbitSlashDirective?
    ) -> (cleanText: String, directive: OrbitSlashDirective?) {
        var rawText = rawInput.trimmingCharacters(in: .whitespacesAndNewlines)
        var effectiveDirective = explicitDirective

        if effectiveDirective == nil {
            for candidate in OrbitSlashDirective.allCases {
                let prefix = "/\(candidate.rawValue)"
                if rawText == prefix {
                    effectiveDirective = candidate
                    rawText = ""
                    break
                } else if rawText.hasPrefix(prefix + " ") {
                    effectiveDirective = candidate
                    rawText = String(rawText.dropFirst(prefix.count)).trimmingCharacters(in: .whitespacesAndNewlines)
                    break
                }
            }
        }
        return (rawText, effectiveDirective)
    }

    private func dispatchPromptInputs(
        hostId hid: String,
        agentId: String?,
        rawInput: String,
        explicitDirective: OrbitSlashDirective?,
        contexts: [ComposerContextItem],
        quotes: [QuotedDiffSnippet]
    ) {
        let parsed = parseInlineDirective(rawInput: rawInput, explicitDirective: explicitDirective)
        let rawText = parsed.cleanText
        let effectiveDirective = parsed.directive

        // Handle `/compact` on an active session
        if effectiveDirective == .compact {
            guard let aid = agentId else {
                showToast("Start a session first to compact context")
                return
            }
            let badges = [FfiPromptBadge(kind: "slash", label: "/compact")]
            let wire: String
            if selectedProvider == .claudeCode || selectedProvider == .antigravity {
                wire = rawText.isEmpty ? "/compact" : "/compact \(rawText)"
            } else {
                wire = rawText.isEmpty ? "Summarize and compact the conversation context." : "Summarize and compact context focusing on: \(rawText)"
            }
            core?.sendPromptWithMeta(
                hostId: hid,
                agentId: aid,
                prompt: wire,
                displayPrompt: rawText.isEmpty ? "/compact" : rawText,
                badges: badges
            )
            if aid == selectedAgentId {
                reloadAgentStage(hostId: hid, agentId: aid)
            }
            return
        }

        let hasContent = !rawText.isEmpty || !contexts.isEmpty || !quotes.isEmpty || effectiveDirective == .review
        guard hasContent else { return }

        // 1. Build clean user-facing badges
        var badges: [FfiPromptBadge] = []
        if let dir = effectiveDirective {
            badges.append(FfiPromptBadge(kind: "slash", label: dir.badgeLabel))
        }
        for ctx in contexts {
            switch ctx {
            case let .file(path):
                badges.append(FfiPromptBadge(kind: "file", label: OrbitFormatters.shortPath(path)))
            case let .gitDiff(_, label):
                badges.append(FfiPromptBadge(kind: "git_diff", label: label))
            case let .terminalOutput(label, _):
                badges.append(FfiPromptBadge(kind: "terminal", label: label))
            case let .assistantQuote(_, label, _):
                badges.append(FfiPromptBadge(kind: "quote", label: label))
            }
        }
        for quote in quotes {
            badges.append(FfiPromptBadge(kind: "quote", label: quote.chipLabel))
        }

        // 2. Build structured wire prompt
        var wireSections: [String] = []
        if let dir = effectiveDirective {
            wireSections.append(dir.workflowInstruction())
        }

        let filePaths = contexts.compactMap { item -> String? in
            if case let .file(path) = item { return path }
            return nil
        }
        if !filePaths.isEmpty {
            let formatted = filePaths.map { "`\($0)`" }.joined(separator: ", ")
            wireSections.append("Referenced workspace files: \(formatted)")
        }

        var includeDiff = contexts.contains(where: {
            if case .gitDiff = $0 { return true }
            return false
        })
        if effectiveDirective == .review {
            includeDiff = true
        }
        if includeDiff {
            let review = gitReview ?? core?.gitReview(hostId: hid, cwd: activeCwd, path: nil)
            if let rawDiff = review?.rawDiff, !rawDiff.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                let capped = String(rawDiff.prefix(24_000))
                wireSections.append("<GIT_DIFF>\n\(capped)\n</GIT_DIFF>")
            } else {
                wireSections.append("Inspect the current git working tree changes with `git status` and `git diff`.")
            }
        }

        for ctx in contexts {
            if case let .terminalOutput(_, content) = ctx, !content.isEmpty {
                wireSections.append("<TERMINAL_OUTPUT>\n\(content)\n</TERMINAL_OUTPUT>")
            } else if case let .assistantQuote(_, _, excerpt) = ctx, !excerpt.isEmpty {
                wireSections.append("<QUOTED_EXCERPT>\n\(excerpt)\n</QUOTED_EXCERPT>")
            }
        }

        if !quotes.isEmpty {
            let quoteLines = quotes.map { q -> String in
                let loc = q.lineNumber.map { "\(q.path):\($0)" } ?? q.path
                return "File `\(loc)`:\n```\n\(q.content)\n```"
            }.joined(separator: "\n\n")
            wireSections.append("<QUOTED_DIFF_LINES>\n\(quoteLines)\n</QUOTED_DIFF_LINES>")
        }

        let cleanDisplayPrompt: String
        if !rawText.isEmpty {
            cleanDisplayPrompt = rawText
            wireSections.append(rawText)
        } else if effectiveDirective == .review {
            cleanDisplayPrompt = "Review uncommitted changes"
            wireSections.append("Please review all uncommitted changes in this workspace.")
        } else {
            cleanDisplayPrompt = "Analyze attached context"
            wireSections.append("Please inspect the attached context above.")
        }

        let finalWirePrompt = wireSections.joined(separator: "\n\n")

        if let aid = agentId {
            lastKnownRunningByAgentKey["\(hid)::\(aid)"] = true
            core?.sendPromptWithMeta(
                hostId: hid,
                agentId: aid,
                prompt: finalWirePrompt,
                displayPrompt: cleanDisplayPrompt,
                badges: badges
            )
            if aid == selectedAgentId {
                reloadAgentStage(hostId: hid, agentId: aid)
            }
        } else {
            let createdId = core?.createAgentFull(
                hostId: hid,
                cwd: activeCwd,
                extraDirs: activeExtraDirs,
                provider: selectedProvider,
                model: selectedModel,
                effort: selectedEffort,
                mode: selectedMode,
                initialPrompt: finalWirePrompt,
                initialDisplayPrompt: cleanDisplayPrompt,
                initialBadges: badges,
                worktreeBranch: nil
            )
            if let newId = createdId, !newId.isEmpty {
                lastKnownRunningByAgentKey["\(hid)::\(newId)"] = true
                selectedAgentId = newId
                core?.openAgentTimeline(hostId: hid, agentId: newId)
                reloadHostDetail(hid)
                reloadAgentStage(hostId: hid, agentId: newId)
            }
        }
    }

    public func interruptActiveAgent() {
        interruptAgent()
    }

    public func interruptAgent() {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        core?.interruptAgent(hostId: hid, agentId: aid)
        hapticRigid()
    }

    // MARK: - Tool Step Descriptor & Approvals / Questions

    public func describeTool(_ item: FfiTimelineItem) -> FfiToolStepDescriptor? {
        core?.describeToolItem(item: item, cwd: activeCwd)
    }

    public func resolvePermission(requestId: String, decision: String) {
        if requestId.hasPrefix("demo-perm-") {
            demoPendingPermissions.removeAll { item in
                if case let .permissionRequest(_, rid, _, _, _, _, _, _) = item {
                    return rid == requestId
                }
                return false
            }
            hapticMedium()
            showToast("Decision: \(OrbitFormatters.decisionLabel(decision))")
            return
        }
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        core?.resolvePermission(
            hostId: hid,
            agentId: aid,
            permissionRequestId: requestId,
            decision: decision
        )
        reloadAgentStage(hostId: hid, agentId: aid)
        hapticMedium()
    }

    public func answerQuestion(requestId: String, answers: [[String]]) {
        if requestId.hasPrefix("demo-question-") {
            demoPendingQuestions.removeAll { item in
                if case let .questionRequest(_, rid, _, _, _) = item {
                    return rid == requestId
                }
                return false
            }
            hapticMedium()
            showToast("Submitted response")
            return
        }
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        core?.answerQuestion(
            hostId: hid,
            agentId: aid,
            questionRequestId: requestId,
            answers: answers
        )
        reloadAgentStage(hostId: hid, agentId: aid)
        hapticMedium()
    }

    // MARK: - Responsive Geometry, Git Diff & Tap-to-Quote

    public func togglePadSidebarCollapsed() {
        isPadSidebarCollapsed.toggle()
        if canShowInlineDiffInspector && activeSheet == .gitDiff && !explicitDiffSheetRequested {
            activeSheet = nil
            isPadDiffInspectorVisible = true
        }
    }

    public func updateContainerWidth(_ width: CGFloat) {
        guard width > 0 else { return }
        if abs(containerWidth - width) > 0.5 {
            containerWidth = width
        }
        if canShowInlineDiffInspector {
            if activeSheet == .gitDiff && !explicitDiffSheetRequested {
                activeSheet = nil
                withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                    isPadDiffInspectorVisible = true
                }
            } else if pendingLaunchPadDiff {
                pendingLaunchPadDiff = false
                isPadDiffInspectorVisible = true
            }
        } else if pendingLaunchPadDiff && isPadDiffInspectorVisible {
            pendingLaunchPadDiff = false
            isPadDiffInspectorVisible = false
            activeSheet = .gitDiff
        }
    }

    public func openGitDiff(focusFilePath: String? = nil, preferSheet: Bool = false) {
        jumpToDiffFilePath = focusFilePath
        if !preferSheet {
            explicitDiffSheetRequested = false
        }
        refreshGitReview()
        if !preferSheet && canShowInlineDiffInspector {
            if activeSheet == .gitDiff {
                activeSheet = nil
            }
            withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                isPadDiffInspectorVisible = true
            }
        } else {
            isPadDiffInspectorVisible = false
            activeSheet = .gitDiff
        }
    }

    public func reconnectActiveHost() {
        guard let hid = selectedHostId else { return }
        let name = activeHost?.displayName ?? "host"
        core?.connectHost(hostId: hid)
        core?.refreshHost(hostId: hid)
        core?.refreshQuotas(hostId: hid)
        reloadFleet()
        reloadHostDetail(hid)
        hapticLight()
        showToast("Reconnecting to \(name)…")
    }

    public func copyAssistantReply(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        UIPasteboard.general.string = trimmed
        refreshPasteboardAvailability()
        hapticLight()
        showToast("Copied response")
    }

    public func quoteAssistantReply(turnKey: String, text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        let firstLine = trimmed
            .components(separatedBy: .newlines)
            .map {
                $0.trimmingCharacters(in: .whitespacesAndNewlines)
                    .replacingOccurrences(of: "^#+\\s*", with: "", options: .regularExpression)
                    .replacingOccurrences(of: "[*`_]", with: "", options: .regularExpression)
            }
            .first(where: { !$0.isEmpty }) ?? "Assistant reply"
        let shortPreview = firstLine.count > 28 ? String(firstLine.prefix(28)) + "…" : firstLine
        let chipLabel = "\"\(shortPreview)\""
        let excerpt = trimmed.count > 800 ? String(trimmed.prefix(800)) + "…" : trimmed
        let item = ComposerContextItem.assistantQuote(id: turnKey, label: chipLabel, excerpt: excerpt)
        if !attachedContexts.contains(item) {
            attachedContexts.append(item)
        }
        hapticLight()
        showToast("Quoted into composer")
    }

    public func pullToRefreshStage() async {
        hapticLight()
        reloadFleet()
        if let hid = selectedHostId {
            core?.refreshHost(hostId: hid)
            if let aid = selectedAgentId {
                core?.openAgentTimeline(hostId: hid, agentId: aid)
            }
            refreshGitReview()
            try? await Task.sleep(nanoseconds: 250_000_000)
            reloadHostDetail(hid)
            if let aid = selectedAgentId {
                reloadAgentStage(hostId: hid, agentId: aid)
            }
        }
    }

    public func pullToRefreshGitDiff() async {
        hapticLight()
        refreshGitReview()
        try? await Task.sleep(nanoseconds: 220_000_000)
        if let hid = selectedHostId {
            gitReview = core?.gitReview(hostId: hid, cwd: activeCwd, path: nil)
        }
    }

    public func setGitStagedFilter(_ staged: Bool) {
        gitStagedMode = staged
        guard let hid = selectedHostId else { return }
        core?.fetchGitStatus(hostId: hid, cwd: activeCwd)
        core?.fetchGitDiff(hostId: hid, cwd: activeCwd, path: nil, staged: staged)
        gitReview = core?.gitReview(hostId: hid, cwd: activeCwd, path: nil)
        hapticLight()
    }

    public func refreshGitReview() {
        guard let hid = selectedHostId else { return }
        core?.fetchGitStatus(hostId: hid, cwd: activeCwd)
        core?.fetchGitDiff(hostId: hid, cwd: activeCwd, path: nil, staged: gitStagedMode)
        gitReview = core?.gitReview(hostId: hid, cwd: activeCwd, path: nil)
    }

    public func quoteDiffLine(path: String, line: FfiDiffLine) {
        let lineNo = line.newLine ?? line.oldLine
        let snippetId = "\(path):\(lineNo ?? 0):\(line.content.hashValue)"
        guard !quotedDiffSnippets.contains(where: { $0.id == snippetId }) else {
            showToast("Line already quoted")
            return
        }
        let snippet = QuotedDiffSnippet(
            id: snippetId,
            path: path,
            lineNumber: lineNo,
            kind: line.kind,
            content: line.content
        )
        quotedDiffSnippets.append(snippet)
        hapticLight()
        showToast("Quoted \(snippet.chipLabel) into composer")
    }

    // MARK: - Native PTY Terminal

    public func ensureTerminalReady(cols: UInt16, rows: UInt16) {
        guard let hid = selectedHostId else { return }
        lastTerminalDimensions = (cols, rows)
        if let detail = currentHostDetail, !detail.terminals.isEmpty {
            if let activeTid = detail.activeTerminalId {
                core?.attachTerminal(hostId: hid, terminalId: activeTid, slot: 1)
            } else if let first = detail.terminals.first {
                core?.attachTerminal(hostId: hid, terminalId: first.terminalId, slot: 1)
            }
            core?.resizeTerminal(hostId: hid, cols: cols, rows: rows)
        } else {
            core?.startTerminal(hostId: hid, cwd: activeCwd, cols: cols, rows: rows)
        }
        terminalScreen = core?.terminalScreen(hostId: hid)

        #if DEBUG
        if let cmd = pendingLaunchPtyCommand, !ptyLaunchCmdDispatched {
            ptyLaunchCmdDispatched = true
            Task { @MainActor [weak self] in
                try? await Task.sleep(nanoseconds: 600_000_000)
                guard let self = self else { return }
                if let hid = self.selectedHostId {
                    self.reloadHostDetail(hid)
                    if let detail = self.currentHostDetail,
                       let targetTid = detail.activeTerminalId ?? detail.terminals.first?.terminalId {
                        self.core?.attachTerminal(hostId: hid, terminalId: targetTid, slot: 1)
                    }
                }
                if (self.currentHostDetail?.terminals.isEmpty ?? true) && self.terminalScreen == nil {
                    try? await Task.sleep(nanoseconds: 500_000_000)
                    if let hid = self.selectedHostId {
                        self.reloadHostDetail(hid)
                        if let detail = self.currentHostDetail,
                           let targetTid = detail.activeTerminalId ?? detail.terminals.first?.terminalId {
                            self.core?.attachTerminal(hostId: hid, terminalId: targetTid, slot: 1)
                        }
                    }
                }
                self.sendTerminalKeys(cmd + "\n")
                self.pendingLaunchPtyCommand = nil
            }
        }
        #endif
    }

    public func startNewTerminal() {
        guard let hid = selectedHostId else { return }
        let dims = lastTerminalDimensions
        core?.startTerminal(hostId: hid, cwd: activeCwd, cols: dims.cols, rows: dims.rows)
        reloadHostDetail(hid)
        terminalScreen = core?.terminalScreen(hostId: hid)
        hapticLight()
    }

    public func attachTerminal(_ terminalId: String) {
        guard let hid = selectedHostId else { return }
        core?.attachTerminal(hostId: hid, terminalId: terminalId, slot: 1)
        core?.resizeTerminal(hostId: hid, cols: lastTerminalDimensions.cols, rows: lastTerminalDimensions.rows)
        reloadHostDetail(hid)
        terminalScreen = core?.terminalScreen(hostId: hid)
        hapticLight()
    }

    public func killTerminal(_ terminalId: String) {
        guard let hid = selectedHostId else { return }
        core?.killTerminal(hostId: hid, terminalId: terminalId)
        reloadHostDetail(hid)
        terminalScreen = core?.terminalScreen(hostId: hid)
        hapticMedium()
    }

    public func resizeTerminalIfNeeded(cols: UInt16, rows: UInt16) {
        let clampedCols = max(24, min(220, cols))
        let clampedRows = max(6, min(100, rows))
        guard clampedCols != lastTerminalDimensions.cols || clampedRows != lastTerminalDimensions.rows else {
            return
        }
        lastTerminalDimensions = (clampedCols, clampedRows)
        guard let hid = selectedHostId else { return }
        core?.resizeTerminal(hostId: hid, cols: clampedCols, rows: clampedRows)
        terminalScreen = core?.terminalScreen(hostId: hid)
    }

    public func sendTerminalKeys(_ keys: String) {
        guard let hid = selectedHostId, !keys.isEmpty else { return }
        core?.sendTerminalKeys(hostId: hid, keys: keys)
    }

    public func sendTerminalBytes(_ bytes: Data, slot: UInt8 = 1) {
        guard let hid = selectedHostId, !bytes.isEmpty else { return }
        core?.sendTerminalBytes(hostId: hid, slot: slot, bytes: bytes)
    }

    public func captureTerminalSnapshotText() -> String? {
        guard let screen = terminalScreen, !screen.rowsData.isEmpty else { return nil }
        let lines = screen.rowsData.map { row in
            row.spans.map(\.text).joined().replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression)
        }
        let nonEmpty = lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        return nonEmpty.isEmpty ? nil : nonEmpty
    }

    @discardableResult
    public func copyTerminalOutput() -> Bool {
        guard let snapshot = captureTerminalSnapshotText(), !snapshot.isEmpty else {
            hapticWarning()
            showToast("Terminal output is empty")
            return false
        }
        UIPasteboard.general.string = snapshot
        refreshPasteboardAvailability()
        hapticLight()
        showToast("Copied terminal output")
        return true
    }

    public func quoteTerminalOutputToComposer() {
        let rawSnapshot = captureTerminalSnapshotText() ?? ""
        let nonEmptyLines = rawSnapshot
            .components(separatedBy: .newlines)
            .map { $0.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression) }
            .filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
        let tailLines = Array(nonEmptyLines.suffix(35))
        let content = tailLines.isEmpty
            ? "Inspect active terminal output."
            : tailLines.joined(separator: "\n")

        attachedContexts.removeAll { item in
            if case .terminalOutput = item { return true }
            return false
        }
        attachedContexts.append(.terminalOutput(label: "@terminal", content: content))
        hapticLight()
        showToast("Attached @terminal to composer")
    }

    // MARK: - Toast & Subtle Haptics

    public func showToast(_ message: String) {
        toastDismissTask?.cancel()
        withAnimation(.easeOut(duration: 0.18)) {
            toastMessage = message
        }
        toastDismissTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 2_200_000_000)
            guard !Task.isCancelled else { return }
            withAnimation(.easeIn(duration: 0.2)) {
                self?.toastMessage = nil
            }
        }
    }

    public func clearError() {
        lastError = nil
    }

    public func hapticLight() {
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }

    public func hapticMedium() {
        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
    }

    public func hapticRigid() {
        UIImpactFeedbackGenerator(style: .rigid).impactOccurred()
    }

    public func hapticSuccess() {
        UINotificationFeedbackGenerator().notificationOccurred(.success)
    }

    public func hapticWarning() {
        UINotificationFeedbackGenerator().notificationOccurred(.warning)
    }
}
