import Foundation
import Network
import SwiftUI
import UIKit

// MARK: - Navigation & Composer Models

public enum OrbitSheetDestination: Identifiable, Equatable {
    case gitDiff
    case pty
    case qrPair
    case directoryBrowser
    case settings
    case workspaceBoard(workspaceId: String)
    case sessionOverview
    case sideQuestion
    case workspaceFiles(initialPath: String?)
    case automations
    case archivedThreads
    case spawnPeer(workspaceId: String)
    case createWorkspace(projectRoot: String, projectName: String, isGitRepo: Bool, moveAgentId: String?)

    public var id: String {
        switch self {
        case .gitDiff: return "gitDiff"
        case .pty: return "pty"
        case .qrPair: return "qrPair"
        case .directoryBrowser: return "directoryBrowser"
        case .settings: return "settings"
        case let .workspaceBoard(wsId): return "workspaceBoard:\(wsId)"
        case .sessionOverview: return "sessionOverview"
        case .sideQuestion: return "sideQuestion"
        case let .workspaceFiles(path): return "workspaceFiles:\(path ?? "")"
        case .automations: return "automations"
        case .archivedThreads: return "archivedThreads"
        case let .spawnPeer(wsId): return "spawnPeer:\(wsId)"
        case let .createWorkspace(root, _, _, moveId): return "createWorkspace:\(root):\(moveId ?? "")"
        }
    }

    public var defaultDetent: PresentationDetent {
        switch self {
        case .spawnPeer, .createWorkspace:
            return .large
        default:
            return .large
        }
    }
}

public enum OrbitPadInspectorTab: String, CaseIterable, Identifiable, Equatable {
    case overview
    case diff
    case board
    case files
    case sideQuestion

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .overview: return "Overview"
        case .diff: return "Diff"
        case .board: return "Board"
        case .files: return "Files"
        case .sideQuestion: return "Side Q&A"
        }
    }

    public var systemImage: String {
        switch self {
        case .overview: return "sidebar.right"
        case .diff: return "arrow.left.arrow.right"
        case .board: return "doc.richtext"
        case .files: return "folder"
        case .sideQuestion: return "bubble.left.and.text.bubble.right"
        }
    }
}

public enum OrbitRenameTarget: Identifiable, Equatable {
    case agent(hostId: String, agentId: String, currentTitle: String, currentHandle: String?)
    case workspace(hostId: String, workspaceId: String, currentTitle: String)

    public var id: String {
        switch self {
        case let .agent(hid, aid, _, _): return "agent:\(hid):\(aid)"
        case let .workspace(hid, wid, _): return "ws:\(hid):\(wid)"
        }
    }
}

public struct SideQuestionEntry: Identifiable, Equatable {
    public let id: String
    public var requestId: String
    public let question: String
    public var answer: String
    public var error: String?
    public var isStreaming: Bool
    public let createdAt: Date

    public init(
        id: String = UUID().uuidString,
        requestId: String = "",
        question: String,
        answer: String = "",
        error: String? = nil,
        isStreaming: Bool = true,
        createdAt: Date = Date()
    ) {
        self.id = id
        self.requestId = requestId
        self.question = question
        self.answer = answer
        self.error = error
        self.isStreaming = isStreaming
        self.createdAt = createdAt
    }
}

public struct HostDaemonUpgradeState: Equatable {
    public var isUpgrading: Bool = false
    public var statusMessage: String? = nil
    public var error: String? = nil
}

public enum OrbitSlashDirective: String, CaseIterable, Identifiable, Equatable {
    case plan
    case review
    case compact
    case orbit
    case orbitHandoff = "orbit-handoff"
    case orbitCommittee = "orbit-committee"
    case orbitAdvisor = "orbit-advisor"

    public var id: String { rawValue }

    public var badgeLabel: String { "/\(rawValue)" }

    public var title: String {
        switch self {
        case .plan: return "Plan Architecture"
        case .review: return "Review Changes"
        case .compact: return "Compact Context"
        case .orbit: return "Orbit Multi-Agent"
        case .orbitHandoff: return "Task Handoff"
        case .orbitCommittee: return "Committee Planning"
        case .orbitAdvisor: return "Read-Only Advisor"
        }
    }

    public var subtitle: String {
        switch self {
        case .plan: return "Draft step-by-step plan first"
        case .review: return "Audit working tree changes"
        case .compact: return "Compress session context"
        case .orbit: return "Coordinate specialist agents across profiles"
        case .orbitHandoff: return "Hand off cleanly to a specialist worktree"
        case .orbitCommittee: return "Multi-model planning, implementation & review"
        case .orbitAdvisor: return "Independent read-only second opinion"
        }
    }

    public var icon: String {
        switch self {
        case .plan: return "checklist"
        case .review: return "doc.text.magnifyingglass"
        case .compact: return "rectangle.compress.vertical"
        case .orbit: return "square.3.layers.3d"
        case .orbitHandoff: return "paperplane"
        case .orbitCommittee: return "person.3"
        case .orbitAdvisor: return "sparkles"
        }
    }

    public var ffiValue: FfiSlashDirective {
        switch self {
        case .plan: return .plan
        case .review: return .review
        case .compact: return .compact
        case .orbit: return .orbit
        case .orbitHandoff: return .orbitHandoff
        case .orbitCommittee: return .orbitCommittee
        case .orbitAdvisor: return .orbitAdvisor
        }
    }

    public init?(ffiValue: FfiSlashDirective?) {
        guard let ffiValue else { return nil }
        switch ffiValue {
        case .plan: self = .plan
        case .review: self = .review
        case .compact: self = .compact
        case .orbit: self = .orbit
        case .orbitHandoff: self = .orbitHandoff
        case .orbitCommittee: self = .orbitCommittee
        case .orbitAdvisor: self = .orbitAdvisor
        }
    }
}

public extension OrbitSlashDirective {
    static func iconName(forSlashCommandId id: String) -> String {
        switch id {
        case "plan": return "checklist"
        case "review": return "doc.text.magnifyingglass"
        case "diff": return "arrow.left.arrow.right"
        case "btw": return "bubble.left.and.text.bubble.right"
        case "files": return "folder"
        case "compact": return "rectangle.compress.vertical"
        case "orbit": return "square.3.layers.3d"
        case "orbit-handoff": return "paperplane"
        case "orbit-committee": return "person.3"
        case "orbit-advisor": return "sparkles"
        case "ask": return "shield"
        case "edits": return "checkmark.shield"
        case "full": return "shield.slash"
        case "terminal": return "terminal"
        case "new": return "square.and.pencil"
        case "settings": return "gearshape"
        default: return "command"
        }
    }
}

extension FfiLaunchpadProject: Identifiable {}

extension FfiSidebarConversation: Identifiable {
    public var id: String { "\(hostId)::\(agent.agentId)" }
}

extension FfiSidebarProjectGroup: Identifiable {}

extension FfiSidebarWorkspaceGroup: Identifiable {
    public var id: String { workspaceId }
}

extension FfiScheduleSpec: Identifiable {
    public var id: String { scheduleId }
}

extension FfiScheduleRunRecord: Identifiable {
    public var id: String { runId }
}

extension FfiWorkspaceEvent: Identifiable {
    public var id: String { "\(workspaceId)::\(seq)" }
}

extension FfiProviderSubagentDescriptor: Identifiable {
    public var id: String { subagentId }
}

extension FfiNestedGitRepo: Identifiable {
    public var id: String { path }
}

extension FfiSlashCommand: Identifiable {}

extension FfiMentionCandidate: Identifiable {
    public var id: String { key }
}

public struct InlineMentionRegistration: Equatable, Hashable {
    public let token: String
    public let item: ComposerContextItem

    public init(token: String, item: ComposerContextItem) {
        self.token = token
        self.item = item
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

public enum OrbitSidebarProjectSort: String, CaseIterable, Identifiable, Equatable {
    case recent
    case alphabetical
    case manual

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .recent: return "Recent activity"
        case .alphabetical: return "Alphabetical"
        case .manual: return "Manual order"
        }
    }

    public var ffiValue: FfiSidebarProjectSort {
        switch self {
        case .recent: return .recent
        case .alphabetical: return .alphabetical
        case .manual: return .manual
        }
    }
}

public enum OrbitSidebarThreadFilter: String, CaseIterable, Identifiable, Equatable {
    case all
    case activeUnread

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .all: return "All conversations"
        case .activeUnread: return "Active & unread"
        }
    }

    public var ffiValue: FfiSidebarThreadFilter {
        switch self {
        case .all: return .all
        case .activeUnread: return .activeUnread
        }
    }
}

public enum ComposerContextItem: Identifiable, Equatable, Hashable {
    case file(path: String)
    case gitDiff(staged: Bool, label: String)
    case terminalOutput(label: String, content: String)
    case assistantQuote(id: String, label: String, excerpt: String)
    case stagedImage(
        id: String,
        filename: String,
        remotePath: String?,
        previewData: Data,
        sizeBytes: Int,
        isUploading: Bool
    )
    case stagedFile(
        id: String,
        filename: String,
        remotePath: String?,
        sizeBytes: Int,
        isUploading: Bool
    )
    case inlineFile(
        id: String,
        filename: String,
        content: String,
        lineCount: Int,
        sizeBytes: Int
    )

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
        case let .stagedImage(id, _, _, _, _, _):
            return "img:\(id)"
        case let .stagedFile(id, _, _, _, _):
            return "sfile:\(id)"
        case let .inlineFile(id, _, _, _, _):
            return "ifile:\(id)"
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
        case let .stagedImage(_, filename, _, _, _, _):
            return filename
        case let .stagedFile(_, filename, _, _, _):
            return filename
        case let .inlineFile(_, filename, _, _, _):
            return filename
        }
    }

    public var icon: String {
        switch self {
        case let .file(path):
            return OrbitFormatters.semanticFileIcon(for: OrbitFormatters.shortPath(path))
        case let .stagedFile(_, filename, _, _, _):
            return OrbitFormatters.semanticFileIcon(for: filename)
        case let .inlineFile(_, filename, _, _, _):
            return OrbitFormatters.semanticFileIcon(for: filename)
        case let .gitDiff(staged, _):
            return staged ? "arrow.left.arrow.right" : "doc.badge.plus"
        case .terminalOutput:
            return "terminal"
        case .assistantQuote:
            return "text.quote"
        case .stagedImage:
            return "photo"
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
    public var badges: [FfiPromptBadge]
    public let createdAt: Date

    public init(
        id: String = UUID().uuidString,
        text: String,
        slashDirective: OrbitSlashDirective?,
        attachedContexts: [ComposerContextItem],
        quotedDiffSnippets: [QuotedDiffSnippet],
        badges: [FfiPromptBadge] = [],
        createdAt: Date = Date()
    ) {
        self.id = id
        self.text = text
        self.slashDirective = slashDirective
        self.attachedContexts = attachedContexts
        self.quotedDiffSnippets = quotedDiffSnippets
        self.badges = badges
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
        if let firstBadge = badges.first {
            return firstBadge.label
        }
        return "Queued follow-up"
    }

    public var attachmentCount: Int {
        let localCount = (slashDirective != nil ? 1 : 0) + attachedContexts.count + quotedDiffSnippets.count
        return max(localCount, badges.count)
    }
}

// MARK: - Side Question Stream Bridge

private final class WeakSideQuestionListenerBridge: SideQuestionListener, @unchecked Sendable {
    weak var owner: OrbitIosStore?
    let scopeKey: String
    let entryId: String

    init(owner: OrbitIosStore, scopeKey: String, entryId: String) {
        self.owner = owner
        self.scopeKey = scopeKey
        self.entryId = entryId
    }

    func onSideQuestionEvent(event: FfiSideQuestionEvent) {
        let scope = scopeKey
        let entry = entryId
        Task { @MainActor [weak owner] in
            owner?.handleSideQuestionEvent(scopeKey: scope, entryId: entry, event: event)
        }
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
    @Published public private(set) var projectOrder: [String] = []
    @Published public private(set) var totalPendingApprovals: UInt32 = 0
    @Published public var selectedHostId: String?
    @Published public private(set) var currentHostDetail: FfiHostDetail?

    // Active Session & Stage (`selectedAgentId == nil` means New Session draft)
    @Published public var selectedAgentId: String?
    @Published public private(set) var agentStage: FfiAgentStage?
    @Published public private(set) var sidebarView: FfiFleetSidebar?
    @Published public private(set) var slashCommands: [FfiSlashCommand] = []

    // Workspace, Files, Git, Terminal
    @Published public var activeCwd: String = "~"
    @Published public var activeExtraDirs: [String] = []
    @Published public var draftUseWorktree: Bool = false
    @Published public var draftWorkspaceId: String? = nil
    @Published public var draftWorktreeRepoPath: String? = nil
    @Published public private(set) var workspaceFilesList: [String] = []
    @Published public var activeWorkspaceFilePath: String? = nil
    @Published public private(set) var workspaceFileCache: [String: FfiWorkspaceFile] = [:]
    @Published public private(set) var workspaceFileLoadingPath: String? = nil
    @Published public private(set) var workspaceFileError: String? = nil
    @Published public private(set) var gitReview: FfiGitReview?
    @Published public var gitStagedMode: Bool = false
    @Published public var jumpToDiffFilePath: String?
    @Published public private(set) var terminalScreen: FfiTerminalScreen?
    @Published public private(set) var localDaemonUri: String?
    @Published public private(set) var pasteboardHasStrings: Bool = false

    // Mission Workspaces, Side Questions, Subagents, Schedules & Updates
    @Published public var collapsedWorkspaceIds: Set<String> = []
    @Published public var collapsedInboxProjectIds: Set<String> = []
    @Published public var archivedSubagentKeys: Set<String> = []
    @Published public var padInspectorTab: OrbitPadInspectorTab = .overview
    @Published public var sideQuestionsByScope: [String: [SideQuestionEntry]] = [:]
    @Published public var sideQuestionDraftsByScope: [String: String] = [:]
    @Published public var hostUpgradeStates: [String: HostDaemonUpgradeState] = [:]
    @Published public var dismissedVersionMismatchHosts: Set<String> = []
    @Published public private(set) var isNetworkOffline: Bool = false
    @Published public var coalesceExploration: Bool = true
    @Published public var defaultPermissionModeRaw: String = "accept_edits"
    @Published public var renameTarget: OrbitRenameTarget? = nil
    @Published public private(set) var subagentTimelinesCache: [String: [FfiProviderSubagentEvent]] = [:]
    @Published public private(set) var scheduleRunsCache: [String: [FfiScheduleRunRecord]] = [:]

    // Composer State & Live Queueing
    @Published public var composerText: String = ""
    @Published public var selectedProvider: FfiProviderKind = .claudeCode
    @Published public var selectedModel: String?
    @Published public var selectedEffort: FfiReasoningEffort?
    @Published public var selectedMode: FfiSessionMode = .acceptEdits
    @Published public var slashDirective: OrbitSlashDirective?
    private var hasUserCustomizedDraftModel: Bool = false
    private var draftSelectionsByProvider: [FfiProviderKind: (modelId: String?, effort: FfiReasoningEffort?)] = [:]
    private var hasSyncedLiveCatalogDefault: Bool = false
    @Published public var attachedContexts: [ComposerContextItem] = []
    @Published public var registeredInlineMentions: [InlineMentionRegistration] = []
    @Published public var composerFocusNonce: Int = 0
    @Published public var quotedDiffSnippets: [QuotedDiffSnippet] = []
    @Published public var queuedPromptsByAgent: [String: [QueuedPromptItem]] = [:]

    // Shell, Drawer Search & Sheet Navigation
    @Published public private(set) var containerWidth: CGFloat = 0
    @Published public var isDrawerOpen: Bool = false
    @Published public var isPadSidebarCollapsed: Bool = false
    @Published public var drawerSearchQuery: String = "" {
        didSet {
            refreshSidebar()
        }
    }
    @Published public var drawerFilter: OrbitDrawerFilter = .all
    @Published public var sidebarProjectSort: OrbitSidebarProjectSort = .recent {
        didSet {
            refreshSidebar()
        }
    }
    @Published public var sidebarThreadFilter: OrbitSidebarThreadFilter = .all {
        didSet {
            refreshSidebar()
        }
    }
    @Published public var collapsedProjectIds: Set<String> = []
    @Published public var diffFileFilterQuery: String = ""
    @Published public var isPadDiffInspectorVisible: Bool = false
    @Published public var activeSheet: OrbitSheetDestination?
    @Published public var isModelPickerPresented: Bool = false
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
    private var cachedFallbackCatalogs: [FfiProviderModelCatalog] = []
    private var lastMarkedSeenSeqByAgent: [String: UInt64] = [:]
    private var activeSideQuestionListeners: [String: WeakSideQuestionListenerBridge] = [:]

    private static let legacyPinnedThreadKeysDefaultsKey = "orbit.ios.pinnedThreadKeys"
    private static let migratedThreadPrefsDefaultsKey = "orbit.ios.migratedThreadPrefsV1"
    private static let draftUseWorktreeDefaultsKey = "orbit.ios.draftUseWorktree"
    private static let archivedSubagentsDefaultsKey = "orbit.ios.archivedSubagentKeys"
    private static let coalesceExplorationDefaultsKey = "orbit.ios.coalesceExploration"
    private static let defaultPermissionModeDefaultsKey = "orbit.ios.defaultPermissionMode"

    public init() {
        self.draftUseWorktree = UserDefaults.standard.bool(forKey: Self.draftUseWorktreeDefaultsKey)
        if let savedArchived = UserDefaults.standard.stringArray(forKey: Self.archivedSubagentsDefaultsKey) {
            self.archivedSubagentKeys = Set(savedArchived)
        }
        if UserDefaults.standard.object(forKey: Self.coalesceExplorationDefaultsKey) != nil {
            self.coalesceExploration = UserDefaults.standard.bool(forKey: Self.coalesceExplorationDefaultsKey)
        }
        if let savedPerm = UserDefaults.standard.string(forKey: Self.defaultPermissionModeDefaultsKey), !savedPerm.isEmpty {
            self.defaultPermissionModeRaw = savedPerm
            self.selectedMode = Self.sessionModeFromRaw(savedPerm)
        }
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

            if !UserDefaults.standard.bool(forKey: Self.migratedThreadPrefsDefaultsKey) {
                let legacyPinned = UserDefaults.standard.stringArray(forKey: Self.legacyPinnedThreadKeysDefaultsKey) ?? []
                if !legacyPinned.isEmpty {
                    let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
                    let seeds: [FfiThreadPrefSeed] = legacyPinned.enumerated().compactMap { idx, compositeKey in
                        guard let sepRange = compositeKey.range(of: "::") else { return nil }
                        let hostId = String(compositeKey[..<sepRange.lowerBound])
                        let agentId = String(compositeKey[sepRange.upperBound...])
                        guard !hostId.isEmpty, !agentId.isEmpty else { return nil }
                        return FfiThreadPrefSeed(
                            hostId: hostId,
                            agentId: agentId,
                            seenSeq: 0,
                            isPinned: true,
                            pinnedOrder: Int64(idx),
                            updatedAtMs: nowMs
                        )
                    }
                    if !seeds.isEmpty {
                        instance.seedThreadPrefs(seeds: seeds)
                    }
                }
                UserDefaults.standard.set(true, forKey: Self.migratedThreadPrefsDefaultsKey)
                UserDefaults.standard.removeObject(forKey: Self.legacyPinnedThreadKeysDefaultsKey)
            }

            self.cachedFallbackCatalogs = instance.fallbackProviderCatalogs()
            self.slashCommands = instance.slashCommands(platform: .ios, query: nil)

            let bridge = WeakMobileListenerBridge(owner: self)
            self.listenerBridge = bridge
            instance.subscribeThrottled(listener: bridge, intervalMs: 24)

            self.localDaemonUri = instance.discoverLocalDaemonUri()
            self.refreshPasteboardAvailability()

            reloadFleet()
            refreshSidebar()
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
        if let agent = activeAgent {
            return agent.isRunning
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
        guard let key = activeAgentQueueKey, let aid = selectedAgentId else { return [] }
        let localList = queuedPromptsByAgent[key] ?? []
        if currentHostDetail?.host.supportsServerQueue == true {
            // Prefer the freshly refreshed `currentHostDetail` card over `agentStage?.agent`
            // because queue mutations emit `HostChanged` without necessarily emitting `TimelineChanged`.
            let freshCard = currentHostDetail?.agents.first(where: { $0.agentId == aid }) ?? activeAgent
            let serverList = freshCard?.queuedPrompts ?? []
            return serverList.map { q in
                if let cached = localList.first(where: { $0.id == q.queueId }) {
                    return cached
                }
                return QueuedPromptItem(
                    id: q.queueId,
                    text: q.displayText,
                    slashDirective: nil,
                    attachedContexts: [],
                    quotedDiffSnippets: [],
                    badges: q.badges
                )
            }
        }
        return localList
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

    public var launchpadProjects: [FfiLaunchpadProject] {
        guard let hid = selectedHostId else { return [] }
        return (sidebarView?.launchpadProjects ?? []).filter { $0.hostId == hid }
    }

    public func refreshSidebar() {
        guard let core else { return }
        let req = FfiFleetSidebarRequest(
            sort: sidebarProjectSort.ffiValue,
            filter: sidebarThreadFilter.ffiValue,
            query: drawerSearchQuery,
            activeHostId: selectedHostId,
            activeAgentId: isDraftNewSession ? nil : selectedAgentId,
            activeCwd: activeCwd
        )
        sidebarView = core.fleetSidebarRequest(req: req)
    }

    public var otherHostsPendingApprovals: UInt32 {
        hosts.reduce(0) { total, host in
            host.hostId == selectedHostId ? total : total + host.pendingApprovalCount
        }
    }

    public func threadKey(hostId: String, agentId: String) -> String {
        "\(hostId)::\(agentId)"
    }

    public func isThreadPinned(hostId: String, agentId: String) -> Bool {
        (sidebarView?.pinnedConversations ?? []).contains {
            $0.hostId == hostId && $0.agent.agentId == agentId
        }
    }

    public func togglePinThread(hostId: String, agentId: String) {
        _ = core?.togglePinAgent(hostId: hostId, agentId: agentId)
        refreshSidebar()
        hapticLight()
    }

    public func toggleProjectCollapsed(_ projectId: String) {
        if collapsedProjectIds.contains(projectId) {
            collapsedProjectIds.remove(projectId)
        } else {
            collapsedProjectIds.insert(projectId)
        }
        hapticLight()
    }

    public func markAgentSeen(
        hostId: String,
        agentId: String,
        seq: UInt64,
        refreshSidebarAfter: Bool = true
    ) {
        let key = "\(hostId)::\(agentId)"
        if let lastSeq = lastMarkedSeenSeqByAgent[key], lastSeq >= seq {
            return
        }
        lastMarkedSeenSeqByAgent[key] = seq
        core?.markAgentSeen(hostId: hostId, agentId: agentId, seq: seq)
        if refreshSidebarAfter {
            refreshSidebar()
        }
    }

    public func markAgentUnread(hostId: String, agentId: String) {
        lastMarkedSeenSeqByAgent.removeValue(forKey: "\(hostId)::\(agentId)")
        core?.markAgentUnread(hostId: hostId, agentId: agentId)
        refreshSidebar()
        hapticLight()
    }

    public func isThreadUnread(hostId: String, agent: FfiAgentCard) -> Bool {
        let isSelected = selectedHostId == hostId && selectedAgentId == agent.agentId && !isDraftNewSession
        if isSelected || agent.isRunning {
            return false
        }
        if let conv = sidebarView?.pinnedConversations.first(where: {
            $0.hostId == hostId && $0.agent.agentId == agent.agentId
        }) {
            return conv.isUnread
        }
        for group in sidebarView?.projects ?? [] where group.hostId == hostId {
            if let conv = group.conversations.first(where: { $0.agent.agentId == agent.agentId }) {
                return conv.isUnread
            }
        }
        return false
    }

    public func catalog(for provider: FfiProviderKind) -> FfiProviderModelCatalog {
        if let found = currentHostDetail?.providerCatalogs.first(where: { $0.provider == provider }),
           !found.models.isEmpty {
            return found
        }
        if let cached = cachedFallbackCatalogs.first(where: { $0.provider == provider }),
           !cached.models.isEmpty {
            return cached
        }
        return fallbackProviderCatalog(provider: provider)
    }

    public func resolvedModelSelection(
        provider: FfiProviderKind,
        desiredModel: String?,
        desiredEffort: FfiReasoningEffort?
    ) -> FfiResolvedModelSelection? {
        core?.resolveModelSelection(
            req: FfiModelSelectionRequest(
                hostId: selectedHostId,
                provider: provider,
                desiredModel: desiredModel,
                desiredEffort: desiredEffort
            )
        )
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

    public var composerModelDisplayName: String {
        if let resolved = resolvedModelSelection(
            provider: selectedProvider,
            desiredModel: selectedModel,
            desiredEffort: selectedEffort
        ) {
            return resolved.selectedModelLabel
        }
        let cat = catalog(for: selectedProvider)
        return formatCatalogModelLabel(catalog: cat, rawModelId: selectedModel)
    }

    public var composerEffortDisplayLabel: String? {
        if let resolved = resolvedModelSelection(
            provider: selectedProvider,
            desiredModel: selectedModel,
            desiredEffort: selectedEffort
        ) {
            return resolved.selectedEffortLabel
        }
        let cat = catalog(for: selectedProvider)
        guard let spec = activeModelSpec else {
            guard let effort = selectedEffort else { return nil }
            return formatEffortLabel(effort: effort)
        }
        guard !spec.thinkingOptions.isEmpty else { return nil }
        guard let effort = resolveSupportedEffortForModel(
            spec: spec,
            desired: selectedEffort,
            catalogDefault: cat.configuredDefaultEffort
        ) else {
            return nil
        }
        return formatEffortLabel(effort: effort)
    }

    public var composerModelSummaryLabel: String {
        if let effortLabel = composerEffortDisplayLabel {
            return "\(composerModelDisplayName) \(effortLabel)"
        }
        return composerModelDisplayName
    }

    public var hasUploadingAttachments: Bool {
        attachedContexts.contains { item in
            switch item {
            case let .stagedImage(_, _, _, _, _, isUploading):
                return isUploading
            case let .stagedFile(_, _, _, _, isUploading):
                return isUploading
            default:
                return false
            }
        }
    }

    public var canSubmitComposer: Bool {
        guard selectedHostId != nil, !hasUploadingAttachments else { return false }
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
            let isOffline = path.status != .satisfied
            let sig = "\(path.status)-\(path.usesInterfaceType(.wifi))-\(path.usesInterfaceType(.cellular))-\(path.usesInterfaceType(.wiredEthernet))"
            Task { @MainActor [weak self] in
                guard let self = self else { return }
                if self.isNetworkOffline != isOffline {
                    self.isNetworkOffline = isOffline
                }
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
            setAppearance(OrbitAppearanceMode.fromStorage(raw))
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

        if args.contains("-orbitNewSession") {
            autoSelectLatestAgentOnLoad = false
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { [weak self] in
                self?.startNewSessionDraft()
            }
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
                case "model", "models", "modelpicker":
                    self.isModelPickerPresented = true
                case "pair", "qr":
                    self.activeSheet = .qrPair
                case "directory":
                    self.openDirectoryBrowser()
                case "files":
                    self.openWorkspaceFiles()
                case "overview":
                    self.openSessionOverview()
                case "board":
                    if let wsId = self.activeWorkspace?.workspaceId {
                        self.openWorkspaceBoard(workspaceId: wsId)
                    }
                case "sidequestion", "btw":
                    self.openSideQuestion()
                case "automations", "schedules":
                    self.activeSheet = .automations
                case "archived":
                    self.activeSheet = .archivedThreads
                case "settings", "quotas":
                    self.activeSheet = .settings
                default:
                    break
                }
            }
        }

        if let slashIdx = args.firstIndex(of: "-orbitSlash"), slashIdx + 1 < args.count {
            let raw = args[slashIdx + 1].lowercased()
            if let dir = OrbitSlashDirective(rawValue: raw) {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
                    self?.slashDirective = dir
                }
            }
        }

        if let textIdx = args.firstIndex(of: "-orbitComposerText"), textIdx + 1 < args.count {
            let txt = args[textIdx + 1]
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
                guard let self else { return }
                self.composerText = txt
                if txt.contains("@") {
                    self.refreshWorkspaceFiles()
                    self.refreshGitReview()
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
            if hostId == selectedHostId && activeSheet == .pty {
                terminalScreen = core?.terminalScreen(hostId: hostId)
            }
        }
    }

    // MARK: - Fleet, Host & Session Selection

    public func reloadFleet() {
        guard let snap = core?.fleetOverview() else { return }
        hosts = snap.hosts
        projects = snap.projects
        projectOrder = snap.projectOrder
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
        refreshSidebar()
    }

    public func selectHost(_ hostId: String) {
        let changed = selectedHostId != hostId
        selectedHostId = hostId
        if changed {
            selectedAgentId = nil
            agentStage = nil
            gitReview = nil
            terminalScreen = nil
            if !hasUserCustomizedDraftModel {
                hasSyncedLiveCatalogDefault = false
                selectedModel = nil
                selectedEffort = nil
            }
        }
        core?.connectHost(hostId: hostId)
        core?.refreshHost(hostId: hostId)
        core?.refreshProviderModels(hostId: hostId, forceRefresh: false)
        core?.refreshQuotas(hostId: hostId)
        reloadHostDetail(hostId)
        refreshSidebar()
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

        // Advance active thread's seen cursor if currently viewing it
        if selectedHostId == hostId, let currentAid = selectedAgentId, !isDraftNewSession,
           let ag = detail.agents.first(where: { $0.agentId == currentAid }) {
            markAgentSeen(hostId: hostId, agentId: currentAid, seq: ag.latestSeq, refreshSidebarAfter: false)
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
            hasSyncedLiveCatalogDefault = detail.providerCatalogs.contains {
                $0.provider == firstAvailable && !$0.models.isEmpty
            }
            syncDefaultModelAndEffort(for: firstAvailable)
        } else {
            let hasLiveCatalogForSelected = detail.providerCatalogs.contains {
                $0.provider == selectedProvider && !$0.models.isEmpty
            }
            if (selectedModel == nil && selectedEffort == nil)
                || (isDraftNewSession && !hasUserCustomizedDraftModel && hasLiveCatalogForSelected && !hasSyncedLiveCatalogDefault) {
                if isDraftNewSession && hasLiveCatalogForSelected {
                    hasSyncedLiveCatalogDefault = true
                }
                syncDefaultModelAndEffort(for: selectedProvider)
            }
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

        refreshSidebar()
    }

    private func syncDefaultModelAndEffort(for provider: FfiProviderKind) {
        if let resolved = resolvedModelSelection(provider: provider, desiredModel: nil, desiredEffort: nil) {
            selectedModel = resolved.selectedModelId
            selectedEffort = resolved.selectedEffort
            return
        }
        let cat = catalog(for: provider)
        let defaultSpec = cat.configuredDefaultModel.flatMap { id in
            cat.models.first(where: { $0.id.caseInsensitiveCompare(id) == .orderedSame })
        }
            ?? cat.models.first(where: { $0.isDefault })
            ?? cat.models.first
        selectedModel = defaultSpec?.id ?? cat.configuredDefaultModel
        selectedEffort = resolveSupportedEffortForModel(
            spec: defaultSpec,
            desired: nil,
            catalogDefault: cat.configuredDefaultEffort
        )
    }

    public func selectAgent(_ agentId: String) {
        guard let hid = selectedHostId else { return }
        selectedAgentId = agentId
        isDrawerOpen = false
        core?.openAgentTimeline(hostId: hid, agentId: agentId)
        core?.listProviderSubagents(hostId: hid, parentAgentId: agentId)
        reloadAgentStage(hostId: hid, agentId: agentId)

        if let card = currentHostDetail?.agents.first(where: { $0.agentId == agentId }) ?? agentStage?.agent {
            selectedProvider = card.provider
            selectedModel = card.model
            selectedEffort = card.effort
            selectedMode = card.mode
            updateActiveWorkspaceCwd(card.cwd)
            if !card.workspaceId.isEmpty {
                core?.fetchWorkspaceEvents(hostId: hid, workspaceId: card.workspaceId, limit: 40)
                if let ws = currentHostDetail?.workspaces.first(where: { $0.workspaceId == card.workspaceId }),
                   ws.isCurated || ws.boardRevision > 0 {
                    core?.fetchWorkspaceBoard(hostId: hid, workspaceId: card.workspaceId)
                }
            }
            markAgentSeen(hostId: hid, agentId: agentId, seq: max(card.latestSeq, agentStage?.latestSeq ?? 0))
        }
        refreshSidebar()
        hapticLight()
    }

    public func reloadAgentStage(hostId: String, agentId: String) {
        let sameStage = agentStage?.hostId == hostId && (agentStage?.agent?.agentId ?? selectedAgentId) == agentId
        let sinceRev: UInt64 = sameStage ? (agentStage?.revision ?? 0) : 0
        guard let delta = core?.agentStageDelta(hostId: hostId, agentId: agentId, sinceRevision: sinceRev) else {
            return
        }

        let stage: FfiAgentStage
        if !delta.reset,
           sameStage,
           let current = agentStage,
           Int(delta.replaceFromTurnIndex) <= current.turns.count {
            var mergedTurns = Array(current.turns.prefix(Int(delta.replaceFromTurnIndex)))
            mergedTurns.append(contentsOf: delta.changedTurns)
            stage = FfiAgentStage(
                revision: delta.revision,
                hostId: delta.hostId,
                agent: delta.agent ?? current.agent,
                turns: mergedTurns,
                pendingPermissions: delta.pendingPermissions,
                pendingQuestions: delta.pendingQuestions,
                sessionTurnDiffs: delta.sessionTurnDiffs,
                insights: delta.insights,
                latestSeq: delta.latestSeq
            )
        } else if !delta.reset && sinceRev > 0,
                  let full = core?.agentStageDelta(hostId: hostId, agentId: agentId, sinceRevision: 0) {
            stage = FfiAgentStage(
                revision: full.revision,
                hostId: full.hostId,
                agent: full.agent,
                turns: full.changedTurns,
                pendingPermissions: full.pendingPermissions,
                pendingQuestions: full.pendingQuestions,
                sessionTurnDiffs: full.sessionTurnDiffs,
                insights: full.insights,
                latestSeq: full.latestSeq
            )
        } else {
            stage = FfiAgentStage(
                revision: delta.revision,
                hostId: delta.hostId,
                agent: delta.agent,
                turns: delta.changedTurns,
                pendingPermissions: delta.pendingPermissions,
                pendingQuestions: delta.pendingQuestions,
                sessionTurnDiffs: delta.sessionTurnDiffs,
                insights: delta.insights,
                latestSeq: delta.latestSeq
            )
        }

        agentStage = stage
        if let card = stage.agent {
            if selectedProvider != card.provider { selectedProvider = card.provider }
            if let m = card.model, selectedModel != m { selectedModel = m }
            if let e = card.effort, selectedEffort != e { selectedEffort = e }
            if selectedMode != card.mode { selectedMode = card.mode }
            if !card.cwd.isEmpty && card.cwd != activeCwd {
                updateActiveWorkspaceCwd(card.cwd)
            }
            if hostId == selectedHostId && agentId == selectedAgentId && !isDraftNewSession {
                markAgentSeen(
                    hostId: hostId,
                    agentId: agentId,
                    seq: max(card.latestSeq, stage.latestSeq),
                    refreshSidebarAfter: !card.isRunning
                )
            }
        } else if hostId == selectedHostId && agentId == selectedAgentId && !isDraftNewSession {
            markAgentSeen(hostId: hostId, agentId: agentId, seq: stage.latestSeq, refreshSidebarAfter: false)
        }

        if hostId == selectedHostId && agentId == selectedAgentId {
            let awaitingDecision = !stage.pendingPermissions.isEmpty
                || !stage.pendingQuestions.isEmpty
                || (stage.agent?.isAwaitingPermission ?? false)
            let isBusy = isActiveAgentRunning || awaitingDecision
            evaluateAgentQueueDrain(hostId: hostId, agentId: agentId, isBusy: isBusy)
        }
    }

    public func startNewSessionDraft(
        cwd: String? = nil,
        extraDirs: [String] = [],
        provider: FfiProviderKind? = nil,
        workspaceId: String? = nil
    ) {
        let wasViewingAgent = selectedAgentId != nil
        selectedAgentId = nil
        agentStage = nil
        isDrawerOpen = false
        activeExtraDirs = extraDirs
        draftWorkspaceId = workspaceId
        draftWorktreeRepoPath = nil
        selectedMode = Self.sessionModeFromRaw(defaultPermissionModeRaw)
        if let provider = provider {
            selectedProvider = provider
            if let saved = draftSelectionsByProvider[provider] {
                applyModelSelection(provider: provider, modelId: saved.modelId, effort: saved.effort)
            } else {
                hasSyncedLiveCatalogDefault = currentHostDetail?.providerCatalogs.contains {
                    $0.provider == provider && !$0.models.isEmpty
                } ?? false
                syncDefaultModelAndEffort(for: provider)
            }
        } else if wasViewingAgent {
            if let saved = draftSelectionsByProvider[selectedProvider] {
                applyModelSelection(provider: selectedProvider, modelId: saved.modelId, effort: saved.effort)
            } else {
                hasUserCustomizedDraftModel = false
                hasSyncedLiveCatalogDefault = currentHostDetail?.providerCatalogs.contains {
                    $0.provider == selectedProvider && !$0.models.isEmpty
                } ?? false
                syncDefaultModelAndEffort(for: selectedProvider)
            }
        }
        if let cwd = cwd, !cwd.isEmpty {
            updateActiveWorkspaceCwd(cwd)
        } else if let hid = selectedHostId, !activeCwd.isEmpty, activeCwd != "~" {
            core?.requestGitReposInspection(hostId: hid, path: activeCwd, extraDirs: activeExtraDirs)
        }
        refreshSidebar()
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

    public func setDraftUseWorktree(_ useWorktree: Bool) {
        draftUseWorktree = useWorktree
        UserDefaults.standard.set(useWorktree, forKey: Self.draftUseWorktreeDefaultsKey)
        hapticLight()
    }

    public func selectWorkspaceDirectory(_ cwd: String) {
        let clean = cwd.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty else { return }
        updateActiveWorkspaceCwd(clean)
        hapticLight()
    }

    private func updateActiveWorkspaceCwd(_ cwd: String) {
        guard !cwd.isEmpty else { return }
        let changed = activeCwd != cwd
        activeCwd = cwd
        if changed {
            draftWorktreeRepoPath = nil
        }
        if changed, let hid = selectedHostId {
            core?.fetchGitStatus(hostId: hid, cwd: cwd)
            core?.fetchGitDiff(hostId: hid, cwd: cwd, path: nil, staged: gitStagedMode)
            gitReview = core?.gitReview(hostId: hid, cwd: cwd, path: nil)
            core?.fetchWorkspaceFiles(hostId: hid, cwd: cwd)
            workspaceFilesList = core?.workspaceFiles(hostId: hid, cwd: cwd) ?? []
            core?.requestGitReposInspection(hostId: hid, path: cwd, extraDirs: activeExtraDirs)
            refreshSidebar()
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

    public func switchDraftProvider(_ provider: FfiProviderKind) {
        if isDraftNewSession, let saved = draftSelectionsByProvider[provider] {
            applyModelSelection(
                provider: provider,
                modelId: saved.modelId,
                effort: saved.effort
            )
            return
        }
        let nextCat = catalog(for: provider)
        let defaultModelId = nextCat.configuredDefaultModel
            ?? nextCat.models.first(where: { $0.isDefault })?.id
            ?? nextCat.models.first?.id
        applyModelSelection(
            provider: provider,
            modelId: defaultModelId,
            effort: nil
        )
    }

    public func applyModelSelection(
        provider: FfiProviderKind,
        modelId: String?,
        effort: FfiReasoningEffort?
    ) {
        let providerChanged = selectedProvider != provider
        selectedProvider = provider
        if let resolved = resolvedModelSelection(
            provider: provider,
            desiredModel: modelId,
            desiredEffort: effort
        ) {
            selectedModel = resolved.selectedModelId
            selectedEffort = resolved.selectedEffort
        } else {
            let cat = catalog(for: provider)
            let spec = modelId.flatMap { id in
                cat.models.first(where: { $0.id.caseInsensitiveCompare(id) == .orderedSame })
            }
                ?? cat.models.first(where: { $0.isDefault })
                ?? cat.models.first
            selectedModel = spec?.id ?? modelId
            selectedEffort = resolveSupportedEffortForModel(
                spec: spec,
                desired: effort,
                catalogDefault: cat.configuredDefaultEffort
            )
        }

        if isDraftNewSession {
            hasUserCustomizedDraftModel = true
            draftSelectionsByProvider[provider] = (selectedModel, selectedEffort)
        }

        if let hid = selectedHostId, let aid = selectedAgentId {
            if providerChanged {
                core?.switchAgentProvider(
                    hostId: hid,
                    agentId: aid,
                    provider: provider,
                    model: selectedModel,
                    effort: selectedEffort
                )
            } else {
                core?.setAgentConfig(
                    hostId: hid,
                    agentId: aid,
                    model: selectedModel,
                    effort: selectedEffort
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
        attachedContexts.removeAll(where: { $0.id == item.id })
        hapticLight()
    }

    public func removeQuotedSnippet(_ snippet: QuotedDiffSnippet) {
        quotedDiffSnippets.removeAll(where: { $0.id == snippet.id })
        hapticLight()
    }

    private static let maxAttachmentBytes = 10 * 1024 * 1024
    private static let maxInlineTextFileBytes = 64 * 1024

    public func attachImage(filename: String, uploadData: Data, previewData: Data) {
        guard !uploadData.isEmpty else {
            showToast("Selected image is empty")
            return
        }
        guard uploadData.count <= Self.maxAttachmentBytes else {
            showToast("Image exceeds 10 MB limit")
            return
        }

        let itemId = UUID().uuidString
        guard let hid = selectedHostId else {
            if hasDemoStageOverlay {
                attachedContexts.append(
                    .stagedImage(
                        id: itemId,
                        filename: filename,
                        remotePath: "/demo/\(filename)",
                        previewData: previewData,
                        sizeBytes: uploadData.count,
                        isUploading: false
                    )
                )
                hapticLight()
            } else {
                activeSheet = .qrPair
            }
            return
        }

        attachedContexts.append(
            .stagedImage(
                id: itemId,
                filename: filename,
                remotePath: nil,
                previewData: previewData,
                sizeBytes: uploadData.count,
                isUploading: true
            )
        )
        hapticLight()

        performRemoteStage(
            hostId: hid,
            contextId: "img:\(itemId)",
            filename: filename,
            mimeType: "image/jpeg",
            data: uploadData
        ) { staged in
            .stagedImage(
                id: itemId,
                filename: staged.filename,
                remotePath: staged.path,
                previewData: previewData,
                sizeBytes: Int(staged.sizeBytes),
                isUploading: false
            )
        }
    }

    public func attachFile(filename: String, mimeType: String?, data: Data, isImageHint: Bool) {
        guard !data.isEmpty else {
            showToast("\(filename) is empty")
            return
        }
        guard data.count <= Self.maxAttachmentBytes else {
            showToast("\(filename) exceeds 10 MB limit")
            return
        }

        if isImageHint {
            Task { @MainActor [weak self] in
                let normalized = await Task.detached(priority: .userInitiated) {
                    OrbitAttachmentNormalizer.normalizeImageData(data)
                }.value
                guard let self else { return }
                if let normalized {
                    let base = (filename as NSString).deletingPathExtension
                    let jpgName = base.isEmpty ? "image.jpg" : "\(base).jpg"
                    self.attachImage(
                        filename: jpgName,
                        uploadData: normalized.uploadData,
                        previewData: normalized.previewData
                    )
                } else {
                    self.stageOpaqueFile(filename: filename, mimeType: mimeType, data: data)
                }
            }
            return
        }

        if data.count <= Self.maxInlineTextFileBytes,
           !data.contains(0),
           let text = String(data: data, encoding: .utf8) {
            let lineCount = text.split(separator: "\n", omittingEmptySubsequences: false).count
            attachedContexts.append(
                .inlineFile(
                    id: UUID().uuidString,
                    filename: filename,
                    content: text,
                    lineCount: lineCount,
                    sizeBytes: data.count
                )
            )
            hapticLight()
            return
        }

        stageOpaqueFile(filename: filename, mimeType: mimeType, data: data)
    }

    private func stageOpaqueFile(filename: String, mimeType: String?, data: Data) {
        let itemId = UUID().uuidString
        guard let hid = selectedHostId else {
            if hasDemoStageOverlay {
                attachedContexts.append(
                    .stagedFile(
                        id: itemId,
                        filename: filename,
                        remotePath: "/demo/\(filename)",
                        sizeBytes: data.count,
                        isUploading: false
                    )
                )
                hapticLight()
            } else {
                activeSheet = .qrPair
            }
            return
        }

        attachedContexts.append(
            .stagedFile(
                id: itemId,
                filename: filename,
                remotePath: nil,
                sizeBytes: data.count,
                isUploading: true
            )
        )
        hapticLight()

        performRemoteStage(
            hostId: hid,
            contextId: "sfile:\(itemId)",
            filename: filename,
            mimeType: mimeType,
            data: data
        ) { staged in
            .stagedFile(
                id: itemId,
                filename: staged.filename,
                remotePath: staged.path,
                sizeBytes: Int(staged.sizeBytes),
                isUploading: false
            )
        }
    }

    private func performRemoteStage(
        hostId: String,
        contextId: String,
        filename: String,
        mimeType: String?,
        data: Data,
        buildCompletedItem: @escaping @MainActor (FfiStagedAttachment) -> ComposerContextItem
    ) {
        let coreRef = self.core
        Task { @MainActor [weak self] in
            do {
                let staged: FfiStagedAttachment = try await Task.detached(priority: .userInitiated) {
                    guard let coreRef else {
                        throw NSError(
                            domain: "Orbit",
                            code: 1,
                            userInfo: [NSLocalizedDescriptionKey: "Core unavailable"]
                        )
                    }
                    return try coreRef.stageAttachment(
                        hostId: hostId,
                        filename: filename,
                        mimeType: mimeType,
                        bytes: data
                    )
                }.value
                guard let self else { return }
                guard let idx = self.attachedContexts.firstIndex(where: { $0.id == contextId }) else {
                    return
                }
                self.attachedContexts[idx] = buildCompletedItem(staged)
            } catch {
                let errorDetail: String
                if case let FfiOrbitError.Core(detail) = error {
                    errorDetail = detail
                } else {
                    errorDetail = error.localizedDescription
                }
                guard let self else { return }
                self.attachedContexts.removeAll(where: { $0.id == contextId })
                self.hapticWarning()
                self.showToast("Upload failed: \(errorDetail)")
            }
        }
    }

    public func registerInlineMention(token: String, item: ComposerContextItem) {
        if let idx = registeredInlineMentions.firstIndex(where: { $0.token == token }) {
            registeredInlineMentions[idx] = InlineMentionRegistration(token: token, item: item)
        } else {
            registeredInlineMentions.append(InlineMentionRegistration(token: token, item: item))
        }
        hapticLight()
    }

    public func requestComposerFocus() {
        composerFocusNonce &+= 1
    }

    private func mergedContextsResolvingInlineMentions(
        for text: String,
        baseContexts: [ComposerContextItem]
    ) -> [ComposerContextItem] {
        var merged = baseContexts
        for reg in registeredInlineMentions {
            guard text.contains(reg.token) else { continue }
            if !merged.contains(where: { $0.id == reg.item.id }) {
                merged.append(reg.item)
            }
        }
        return merged
    }

    public func applyQuickWorkflow(_ directive: OrbitSlashDirective?, promptText: String = "") {
        slashDirective = directive
        if !promptText.isEmpty {
            composerText = promptText
        }
        requestComposerFocus()
        hapticLight()
    }

    public func filterSlashCommands(query: String) -> [FfiSlashCommand] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return slashCommands }
        return core?.slashCommands(platform: .ios, query: trimmed) ?? slashCommands
    }

    public func rankMentionCandidates(query: String, limit: UInt32 = 11) -> [FfiMentionCandidate] {
        guard let core, let hid = selectedHostId else { return [] }
        return core.rankMentionCandidatesWithWorkspace(
            hostId: hid,
            cwd: activeCwd,
            query: query,
            limit: limit,
            workspaceId: activeWorkspace?.workspaceId,
            activeAgentId: selectedAgentId
        )
    }

    public func submitComposer() {
        guard !hasUploadingAttachments else {
            showToast("Waiting for attachment upload…")
            return
        }
        guard let hid = selectedHostId else {
            activeSheet = .qrPair
            return
        }

        let draftText = composerText
        let draftDirective = slashDirective
        let draftQuotes = quotedDiffSnippets
        let draftContexts = mergedContextsResolvingInlineMentions(
            for: draftText,
            baseContexts: attachedContexts
        )

        if let preview = composePromptDraft(
            hostId: hid,
            rawInput: draftText,
            explicitDirective: draftDirective,
            contexts: draftContexts,
            quotes: draftQuotes
        ) {
            guard !preview.isEmpty else { return }
            if preview.isBtw {
                composerText = ""
                slashDirective = nil
                hapticLight()
                let q = (preview.btwQuestion ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                openSideQuestion(prefillAndSubmit: q.isEmpty ? nil : q)
                return
            }
            if preview.isCompact && selectedAgentId == nil {
                showToast("Start a session first to compact context")
                return
            }
        } else {
            return
        }

        if isActiveAgentRunning, selectedAgentId != nil {
            enqueueComposerPrompt()
            return
        }

        // Reset draft immediately for snappy 60/120fps responsiveness
        composerText = ""
        slashDirective = nil
        attachedContexts.removeAll()
        registeredInlineMentions.removeAll()
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
        guard !hasUploadingAttachments else {
            showToast("Waiting for attachment upload…")
            return
        }
        guard let hid = selectedHostId, let aid = selectedAgentId else {
            submitComposer()
            return
        }
        let mergedContexts = mergedContextsResolvingInlineMentions(
            for: composerText,
            baseContexts: attachedContexts
        )
        guard let preview = composePromptDraft(
            hostId: hid,
            rawInput: composerText,
            explicitDirective: slashDirective,
            contexts: mergedContexts,
            quotes: quotedDiffSnippets
        ), !preview.isEmpty else {
            return
        }
        if preview.isBtw {
            showToast("Side question (/btw) cannot be queued")
            return
        }

        let item = QueuedPromptItem(
            text: composerText.trimmingCharacters(in: .whitespacesAndNewlines),
            slashDirective: OrbitSlashDirective(ffiValue: preview.effectiveDirective),
            attachedContexts: mergedContexts,
            quotedDiffSnippets: quotedDiffSnippets
        )
        let key = "\(hid)::\(aid)"
        let useServerQueue = currentHostDetail?.host.supportsServerQueue == true
        var queue = queuedPromptsByAgent[key] ?? []
        queue.append(item)
        queuedPromptsByAgent[key] = queue
        if !useServerQueue {
            lastKnownRunningByAgentKey[key] = true
        }

        if useServerQueue, let core = core {
            _ = core.enqueuePrompt(
                hostId: hid,
                agentId: aid,
                prompt: preview.wirePrompt,
                displayPrompt: preview.displayPrompt,
                badges: preview.badges,
                queueId: item.id
            )
        }

        composerText = ""
        slashDirective = nil
        attachedContexts.removeAll()
        registeredInlineMentions.removeAll()
        quotedDiffSnippets.removeAll()
        hapticLight()
        showToast("Queued next prompt")
    }

    public func editQueuedPrompt(_ item: QueuedPromptItem) {
        guard let hid = selectedHostId, let aid = selectedAgentId, let key = activeAgentQueueKey else { return }
        let useServerQueue = currentHostDetail?.host.supportsServerQueue == true
        removeQueuedPrompt(item, inKey: key)
        if useServerQueue, let core = core {
            core.removeQueuedPrompt(hostId: hid, agentId: aid, queueId: item.id)
        }
        composerText = item.text
        slashDirective = item.slashDirective
        attachedContexts = item.attachedContexts
        quotedDiffSnippets = item.quotedDiffSnippets
        hapticLight()
    }

    public func removeQueuedPrompt(_ item: QueuedPromptItem) {
        guard let hid = selectedHostId, let aid = selectedAgentId, let key = activeAgentQueueKey else { return }
        let useServerQueue = currentHostDetail?.host.supportsServerQueue == true
        removeQueuedPrompt(item, inKey: key)
        if useServerQueue, let core = core {
            core.removeQueuedPrompt(hostId: hid, agentId: aid, queueId: item.id)
        }
        hapticLight()
    }

    public func sendQueuedPromptNow(_ item: QueuedPromptItem) {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        let key = "\(hid)::\(aid)"
        let useServerQueue = currentHostDetail?.host.supportsServerQueue == true
        removeQueuedPrompt(item, inKey: key)
        hapticLight()
        if useServerQueue, let core = core {
            core.flushQueuedPromptNow(hostId: hid, agentId: aid, queueId: item.id)
            return
        }
        lastKnownRunningByAgentKey[key] = true
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
        if hosts.first(where: { $0.hostId == hostId })?.supportsServerQueue == true {
            return
        }
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

    private func buildFfiComposerInputs(
        contexts: [ComposerContextItem],
        quotes: [QuotedDiffSnippet]
    ) -> (contexts: [FfiComposerContextInput], quotes: [FfiComposerQuoteInput], pastes: [FfiComposerPasteInput]) {
        var ffiContexts: [FfiComposerContextInput] = []
        var ffiPastes: [FfiComposerPasteInput] = []

        for ctx in contexts {
            switch ctx {
            case let .file(path):
                ffiContexts.append(.workspaceFile(path: path))
            case let .gitDiff(staged, label):
                let diffText: String? = (staged == gitStagedMode) ? gitReview?.rawDiff : nil
                ffiContexts.append(.gitDiff(staged: staged, label: label, diffText: diffText))
            case let .terminalOutput(label, content):
                ffiContexts.append(.terminalOutput(label: label, content: content))
            case let .assistantQuote(_, label, excerpt):
                ffiContexts.append(.assistantQuote(label: label, content: excerpt))
            case let .stagedImage(_, filename, remotePath, _, _, _):
                if let path = remotePath, !path.isEmpty {
                    ffiContexts.append(.image(path: path, filename: filename))
                }
            case let .stagedFile(_, filename, remotePath, _, _):
                if let path = remotePath, !path.isEmpty {
                    ffiContexts.append(.stagedFile(path: path, filename: filename))
                }
            case let .inlineFile(_, filename, content, _, _):
                ffiPastes.append(
                    FfiComposerPasteInput(
                        label: filename,
                        filename: filename,
                        content: content
                    )
                )
            }
        }

        let ffiQuotes = quotes.map { q in
            FfiComposerQuoteInput(
                path: q.path,
                lineNumber: q.lineNumber,
                code: q.content
            )
        }

        return (ffiContexts, ffiQuotes, ffiPastes)
    }

    private func composePromptDraft(
        hostId hid: String,
        rawInput: String,
        explicitDirective: OrbitSlashDirective?,
        contexts: [ComposerContextItem],
        quotes: [QuotedDiffSnippet]
    ) -> FfiComposedPrompt? {
        guard let core else { return nil }
        let inputs = buildFfiComposerInputs(contexts: contexts, quotes: quotes)
        return core.composePrompt(
            req: FfiComposePromptRequest(
                provider: selectedProvider,
                rawText: rawInput,
                explicitDirective: explicitDirective?.ffiValue,
                hostId: hid,
                cwd: activeCwd,
                contexts: inputs.contexts,
                quotes: inputs.quotes,
                pastes: inputs.pastes
            )
        )
    }

    private func dispatchPromptInputs(
        hostId hid: String,
        agentId: String?,
        rawInput: String,
        explicitDirective: OrbitSlashDirective?,
        contexts: [ComposerContextItem],
        quotes: [QuotedDiffSnippet]
    ) {
        let needsGitDiff = explicitDirective == .review
            || rawInput.trimmingCharacters(in: .whitespacesAndNewlines).lowercased().hasPrefix("/review")
            || contexts.contains {
                if case .gitDiff = $0 { return true }
                return false
            }
        if needsGitDiff && (gitReview == nil || !OrbitFormatters.arePathsEquivalent(gitReview?.cwd ?? "", activeCwd)) {
            core?.fetchGitStatus(hostId: hid, cwd: activeCwd)
            core?.fetchGitDiff(hostId: hid, cwd: activeCwd, path: nil, staged: gitStagedMode)
            gitReview = core?.gitReview(hostId: hid, cwd: activeCwd, path: nil)
        }

        guard let composed = composePromptDraft(
            hostId: hid,
            rawInput: rawInput,
            explicitDirective: explicitDirective,
            contexts: contexts,
            quotes: quotes
        ), !composed.isEmpty else {
            return
        }

        if composed.isBtw {
            let q = (composed.btwQuestion ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            openSideQuestion(prefillAndSubmit: q.isEmpty ? nil : q)
            return
        }

        if composed.isCompact {
            guard let aid = agentId else {
                showToast("Start a session first to compact context")
                return
            }
            core?.sendPromptWithMeta(
                hostId: hid,
                agentId: aid,
                prompt: composed.wirePrompt,
                displayPrompt: composed.displayPrompt,
                badges: composed.badges
            )
            if aid == selectedAgentId {
                reloadAgentStage(hostId: hid, agentId: aid)
            }
            return
        }

        if let aid = agentId {
            lastKnownRunningByAgentKey["\(hid)::\(aid)"] = true
            core?.sendPromptWithMeta(
                hostId: hid,
                agentId: aid,
                prompt: composed.wirePrompt,
                displayPrompt: composed.displayPrompt,
                badges: composed.badges
            )
            if aid == selectedAgentId {
                reloadAgentStage(hostId: hid, agentId: aid)
            }
        } else {
            let effectiveCwd = (draftUseWorktree && (draftWorktreeRepoPath?.isEmpty == false))
                ? (draftWorktreeRepoPath ?? activeCwd)
                : activeCwd
            let req = FfiCreateSessionRequest(
                hostId: hid,
                cwd: effectiveCwd,
                extraDirs: activeExtraDirs,
                provider: selectedProvider,
                model: selectedModel,
                effort: selectedEffort,
                mode: selectedMode,
                initialPrompt: composed.wirePrompt,
                initialDisplayPrompt: composed.displayPrompt,
                initialBadges: composed.badges,
                worktreeBranch: draftUseWorktree ? "auto" : nil
            )
            let createdId = core?.createSessionInWorkspace(
                req: req,
                workspaceId: draftWorkspaceId,
                handle: nil,
                autoCluster: false
            )
            if let newId = createdId, !newId.isEmpty {
                draftWorkspaceId = nil
                draftWorktreeRepoPath = nil
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
            padInspectorTab = .diff
            withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                isPadDiffInspectorVisible = true
            }
        } else {
            isPadDiffInspectorVisible = false
            activeSheet = .gitDiff
        }
    }

    public func reconnectHost(_ hostId: String) {
        let name = hosts.first(where: { $0.hostId == hostId })?.displayName ?? "host"
        core?.connectHost(hostId: hostId)
        core?.refreshHost(hostId: hostId)
        core?.refreshQuotas(hostId: hostId)
        reloadFleet()
        if selectedHostId == hostId {
            reloadHostDetail(hostId)
        }
        hapticLight()
        showToast("Reconnecting to \(name)…")
    }

    public func reconnectActiveHost() {
        guard let hid = selectedHostId else { return }
        reconnectHost(hid)
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

    // MARK: - Native Diagram Rendering & Bounded Cache

    private var diagramRenderCache: [String: FfiRenderDiagramView] = [:]
    private var diagramRenderCacheOrder: [String] = []
    private let diagramRenderCacheLimit = 96

    public func renderDiagram(
        kind: FfiDiagramKind,
        source: String,
        isDark: Bool
    ) async -> FfiRenderDiagramView? {
        let key = "\(kind)::\(isDark ? "dark" : "light")::\(source)"
        if let hit = diagramRenderCache[key] {
            if let pos = diagramRenderCacheOrder.firstIndex(of: key) {
                diagramRenderCacheOrder.remove(at: pos)
                diagramRenderCacheOrder.append(key)
            }
            return hit
        }
        guard let coreRef = core else { return nil }
        let theme: FfiDiagramTheme = isDark ? .dark : .light
        let result = await Task.detached(priority: .userInitiated) {
            coreRef.renderDiagram(
                req: FfiRenderDiagramRequest(kind: kind, source: source, theme: theme)
            )
        }.value
        if diagramRenderCache[key] == nil,
           diagramRenderCacheOrder.count >= diagramRenderCacheLimit,
           let oldest = diagramRenderCacheOrder.first {
            diagramRenderCacheOrder.removeFirst()
            diagramRenderCache.removeValue(forKey: oldest)
        }
        if let pos = diagramRenderCacheOrder.firstIndex(of: key) {
            diagramRenderCacheOrder.remove(at: pos)
        }
        diagramRenderCacheOrder.append(key)
        diagramRenderCache[key] = result
        return result
    }

    // MARK: - General Preferences (Aligned with Desktop Settings)

    public static func sessionModeFromRaw(_ raw: String) -> FfiSessionMode {
        switch raw.lowercased() {
        case "default", "ask": return .default
        case "full_access", "full": return .fullAccess
        default: return .acceptEdits
        }
    }

    public static func rawFromSessionMode(_ mode: FfiSessionMode) -> String {
        switch OrbitFormatters.normalizePermissionMode(mode) {
        case .default, .plan, .custom: return "default"
        case .acceptEdits: return "accept_edits"
        case .fullAccess: return "full_access"
        }
    }

    public func setCoalesceExploration(_ enabled: Bool) {
        coalesceExploration = enabled
        UserDefaults.standard.set(enabled, forKey: Self.coalesceExplorationDefaultsKey)
        hapticLight()
    }

    public func setDefaultPermissionMode(_ mode: FfiSessionMode) {
        let raw = Self.rawFromSessionMode(mode)
        defaultPermissionModeRaw = raw
        UserDefaults.standard.set(raw, forKey: Self.defaultPermissionModeDefaultsKey)
        if isDraftNewSession {
            selectedMode = OrbitFormatters.normalizePermissionMode(mode)
        }
        hapticLight()
    }

    // MARK: - Mission Workspaces, Peers, Board, Events & Auto-Organize

    public var activeWorkspace: FfiWorkspaceSummary? {
        guard let detail = currentHostDetail else { return nil }
        if let agent = activeAgent, !agent.workspaceId.isEmpty {
            return detail.workspaces.first(where: { $0.workspaceId == agent.workspaceId })
        }
        if let draftWs = draftWorkspaceId {
            return detail.workspaces.first(where: { $0.workspaceId == draftWs })
        }
        return nil
    }

    public var activeWorkspacePeers: [FfiAgentCard] {
        guard let ws = activeWorkspace, let detail = currentHostDetail else { return [] }
        return detail.agents.filter { ag in
            ag.workspaceId == ws.workspaceId
                && ag.archivedAtRfc3339 == nil
                && !(ag.parentAgentId != nil && ag.parentHostId == nil)
        }
    }

    public var activeProjectGroup: FfiSidebarProjectGroup? {
        guard let hid = selectedHostId else { return nil }
        let groups = (sidebarView?.projects ?? []).filter { $0.hostId == hid }
        if let agent = activeAgent {
            if let matched = groups.first(where: { g in
                g.conversations.contains(where: { $0.agent.agentId == agent.agentId })
                    || g.workspaces.contains(where: { $0.workspaceId == agent.workspaceId })
            }) {
                return matched
            }
        }
        return groups.first(where: { g in
            OrbitFormatters.arePathsEquivalent(g.primaryCwd, activeCwd)
                || g.directories.contains(where: { OrbitFormatters.arePathsEquivalent($0, activeCwd) })
        })
    }

    public var activeProjectRoot: String {
        activeWorkspace?.projectRoot ?? activeProjectGroup?.primaryCwd ?? activeCwd
    }

    public var activeProjectCuratedWorkspaces: [FfiWorkspaceSummary] {
        guard let detail = currentHostDetail else { return [] }
        let root = activeProjectRoot
        return detail.workspaces.filter { ws in
            ws.isCurated && (
                OrbitFormatters.arePathsEquivalent(ws.projectRoot ?? ws.sourceCwd ?? ws.cwd, root)
                    || OrbitFormatters.arePathsEquivalent(ws.cwd, root)
            )
        }
    }

    public func workspaceBoard(for workspaceId: String) -> FfiWorkspaceBoardSnapshot? {
        if let snap = currentHostDetail?.workspaceBoards[workspaceId] {
            return snap
        }
        guard let hid = selectedHostId else { return nil }
        return core?.workspaceBoard(hostId: hid, workspaceId: workspaceId)
    }

    public func workspaceEvents(for workspaceId: String) -> [FfiWorkspaceEvent] {
        if let cached = currentHostDetail?.workspaceEvents[workspaceId], !cached.isEmpty {
            return cached
        }
        guard let hid = selectedHostId else { return [] }
        return core?.workspaceEvents(hostId: hid, workspaceId: workspaceId) ?? []
    }

    public var activeWorkspaceConflictEvents: [FfiWorkspaceEvent] {
        guard let ws = activeWorkspace else { return [] }
        return workspaceEvents(for: ws.workspaceId).filter { $0.kind == .conflictWarning }
    }

    public func organizeStatus(forProjectRoot root: String) -> FfiWorkspaceOrganizeStatus? {
        guard let map = currentHostDetail?.workspaceOrganizeStatus else { return nil }
        if let direct = map[root] { return direct }
        return map.first(where: { OrbitFormatters.arePathsEquivalent($0.key, root) })?.value
    }

    public func toggleWorkspaceCollapsed(_ workspaceId: String) {
        if collapsedWorkspaceIds.contains(workspaceId) {
            collapsedWorkspaceIds.remove(workspaceId)
        } else {
            collapsedWorkspaceIds.insert(workspaceId)
        }
        hapticLight()
    }

    public func toggleInboxCollapsed(projectId: String, currentlyCollapsed: Bool) {
        if currentlyCollapsed {
            collapsedInboxProjectIds.remove(projectId)
            collapsedInboxProjectIds.insert("expanded::\(projectId)")
        } else {
            collapsedInboxProjectIds.remove("expanded::\(projectId)")
            collapsedInboxProjectIds.insert(projectId)
        }
        hapticLight()
    }

    public func isInboxCollapsed(for project: FfiSidebarProjectGroup) -> Bool {
        if collapsedInboxProjectIds.contains("expanded::\(project.id)") {
            return false
        }
        if collapsedInboxProjectIds.contains(project.id) {
            return true
        }
        // Auto-expand if there are no curated workspaces or active agent is inside inbox
        let hasActiveInInbox = project.unclassifiedConversations.contains {
            !isDraftNewSession && $0.agent.agentId == selectedAgentId
        }
        if hasActiveInInbox || project.workspaces.isEmpty {
            return false
        }
        return true
    }

    public func fetchWorkspaceBoardAndEvents(_ workspaceId: String) {
        guard let hid = selectedHostId else { return }
        core?.fetchWorkspaceBoard(hostId: hid, workspaceId: workspaceId)
        core?.fetchWorkspaceEvents(hostId: hid, workspaceId: workspaceId, limit: 60)
    }

    public func updateWorkspaceBoard(
        workspaceId: String,
        content: String,
        section: String? = nil
    ) {
        guard let hid = selectedHostId else { return }
        core?.updateWorkspaceBoard(
            hostId: hid,
            workspaceId: workspaceId,
            content: content,
            section: section,
            updatedBy: "user"
        )
        hapticLight()
        showToast("Board updated")
    }

    public func createMissionWorkspace(
        sourceCwd: String,
        title: String,
        missionGoal: String?,
        useWorktree: Bool,
        moveAgentId: String? = nil
    ) {
        guard let hid = selectedHostId else { return }
        let cleanTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleanTitle.isEmpty else { return }
        if let moveId = moveAgentId {
            core?.moveAgentToWorkspaceV2(
                hostId: hid,
                agentId: moveId,
                targetWorkspaceId: nil,
                newWorkspaceTitle: cleanTitle,
                makeUnclassified: false
            )
            hapticSuccess()
            showToast("Created \"\(cleanTitle)\"")
            return
        }
        let cleanGoal = missionGoal?.trimmingCharacters(in: .whitespacesAndNewlines)
        core?.createMissionWorkspace(
            hostId: hid,
            sourceCwd: sourceCwd,
            title: cleanTitle,
            missionGoal: (cleanGoal?.isEmpty == false) ? cleanGoal : nil,
            useWorktree: useWorktree,
            worktreeBranch: nil,
            baseRef: nil
        )
        hapticSuccess()
        showToast("Created workspace \"\(cleanTitle)\"")
    }

    public func renameWorkspace(hostId: String, workspaceId: String, title: String) {
        let clean = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty else { return }
        core?.renameWorkspace(hostId: hostId, workspaceId: workspaceId, title: clean)
        hapticLight()
        showToast("Renamed workspace")
    }

    public func updateWorkspaceGoal(workspaceId: String, goal: String) {
        guard let hid = selectedHostId else { return }
        core?.updateWorkspaceGoal(
            hostId: hid,
            workspaceId: workspaceId,
            goal: goal.trimmingCharacters(in: .whitespacesAndNewlines)
        )
        hapticLight()
        showToast("Updated mission goal")
    }

    public func archiveWorkspace(_ workspaceId: String, keepAgentsUnclassified: Bool) {
        guard let hid = selectedHostId else { return }
        core?.archiveWorkspace(
            hostId: hid,
            workspaceId: workspaceId,
            keepAgentsUnclassified: keepAgentsUnclassified
        )
        if activeWorkspace?.workspaceId == workspaceId && !keepAgentsUnclassified {
            selectedAgentId = nil
            agentStage = nil
        }
        hapticMedium()
        showToast(keepAgentsUnclassified ? "Dissolved workspace" : "Archived workspace")
    }

    public func mergeWorkspaces(sourceWorkspaceId: String, targetWorkspaceId: String) {
        guard let hid = selectedHostId, sourceWorkspaceId != targetWorkspaceId else { return }
        core?.mergeWorkspaces(
            hostId: hid,
            sourceWorkspaceId: sourceWorkspaceId,
            targetWorkspaceId: targetWorkspaceId
        )
        hapticSuccess()
        showToast("Merged workspaces")
    }

    public func moveAgentToWorkspace(
        agentId: String,
        targetWorkspaceId: String?,
        newWorkspaceTitle: String? = nil,
        makeUnclassified: Bool = false
    ) {
        guard let hid = selectedHostId else { return }
        core?.moveAgentToWorkspaceV2(
            hostId: hid,
            agentId: agentId,
            targetWorkspaceId: targetWorkspaceId,
            newWorkspaceTitle: newWorkspaceTitle,
            makeUnclassified: makeUnclassified
        )
        hapticLight()
        if makeUnclassified {
            showToast("Moved to 待归类")
        } else if let title = newWorkspaceTitle, !title.isEmpty {
            showToast("Moved to \"\(title)\"")
        } else {
            showToast("Moved conversation")
        }
    }

    public func organizeProjectWorkspaces(
        projectRoot: String,
        agentIds: [String] = [],
        autoApply: Bool = true
    ) {
        guard let hid = selectedHostId else { return }
        core?.organizeProjectWorkspaces(
            hostId: hid,
            projectRoot: projectRoot,
            agentIds: agentIds,
            autoApply: autoApply
        )
        hapticLight()
        showToast(agentIds.count == 1 ? "AI classifying conversation…" : "AI organizing workspaces…")
    }

    public func summarizeAgent(_ agentId: String) {
        guard let hid = selectedHostId else { return }
        core?.summarizeAgent(hostId: hid, agentId: agentId)
        hapticLight()
        showToast("Generating AI summary…")
    }

    public func dismissClusterSuggestion(_ agentId: String) {
        guard let hid = selectedHostId else { return }
        core?.dismissClusterSuggestion(hostId: hid, agentId: agentId)
        hapticLight()
    }

    public func updateAgentIdentity(
        hostId: String,
        agentId: String,
        handle: String? = nil,
        title: String? = nil,
        taskSummary: String? = nil,
        lockHandle: Bool = false,
        lockTitle: Bool = false
    ) {
        core?.updateAgentIdentity(
            hostId: hostId,
            agentId: agentId,
            handle: handle,
            title: title,
            taskSummary: taskSummary,
            lockHandle: lockHandle,
            lockTitle: lockTitle
        )
        hapticLight()
        showToast("Updated session")
    }

    public func spawnWorkspacePeer(
        workspaceId: String,
        provider: FfiProviderKind,
        model: String?,
        effort: FfiReasoningEffort?,
        mode: FfiSessionMode,
        handle: String?,
        initialPrompt: String,
        useWorktree: Bool
    ) {
        guard let hid = selectedHostId else { return }
        let ws = currentHostDetail?.workspaces.first(where: { $0.workspaceId == workspaceId })
        let cwd = ws?.cwd ?? activeCwd
        let cleanHandle = handle?.trimmingCharacters(in: .whitespacesAndNewlines)
        let req = FfiSpawnWorkspacePeerRequest(
            hostId: hid,
            workspaceId: workspaceId,
            cwd: cwd,
            extraDirs: activeExtraDirs,
            provider: provider,
            model: model,
            effort: effort,
            mode: mode,
            initialPrompt: initialPrompt,
            worktreeBranch: useWorktree ? "auto" : nil,
            handle: (cleanHandle?.isEmpty == false) ? cleanHandle : nil,
            profileId: nil
        )
        let newId = core?.spawnWorkspacePeer(req: req) ?? ""
        if !newId.isEmpty {
            selectedAgentId = newId
            core?.openAgentTimeline(hostId: hid, agentId: newId)
            reloadHostDetail(hid)
            reloadAgentStage(hostId: hid, agentId: newId)
        }
        hapticSuccess()
        showToast("Spawned workspace peer")
    }

    public func unarchiveAgent(_ agentId: String) {
        guard let hid = selectedHostId else { return }
        core?.unarchiveAgent(hostId: hid, agentId: agentId)
        hapticSuccess()
        showToast("Conversation restored")
    }

    public func deleteAgentPermanently(_ agentId: String) {
        guard let hid = selectedHostId else { return }
        core?.deleteAgent(hostId: hid, agentId: agentId)
        hapticMedium()
        showToast("Conversation deleted")
    }

    public func detachAgent(_ agentId: String) {
        guard let hid = selectedHostId else { return }
        core?.detachAgent(hostId: hid, agentId: agentId)
        hapticLight()
        showToast("Detached as standalone session")
    }

    public func resolveChildPermission(
        childHostId: String?,
        childAgentId: String,
        requestId: String,
        decision: String
    ) {
        guard let hid = childHostId ?? selectedHostId else { return }
        core?.resolvePermission(
            hostId: hid,
            agentId: childAgentId,
            permissionRequestId: requestId,
            decision: decision
        )
        hapticMedium()
        showToast("Sent decision: \(OrbitFormatters.decisionLabel(decision))")
    }

    // MARK: - Git Repos Inspection (Non-Git Guard & Nested Repo Picker)

    public func gitReposInspection(for path: String) -> FfiGitReposInspection? {
        if let map = currentHostDetail?.gitReposByCwd {
            if let direct = map[path] { return direct }
            if let matched = map.first(where: { OrbitFormatters.arePathsEquivalent($0.key, path) }) {
                return matched.value
            }
        }
        guard let hid = selectedHostId else { return nil }
        return core?.gitReposInspection(hostId: hid, path: path)
    }

    public func requestGitReposInspection(for path: String) {
        guard let hid = selectedHostId, !path.isEmpty, path != "~" else { return }
        core?.requestGitReposInspection(hostId: hid, path: path, extraDirs: activeExtraDirs)
    }

    // MARK: - Subagents (Managed & Provider Native)

    public var activeManagedSubagents: [FfiAgentCard] {
        guard let aid = selectedAgentId, let detail = currentHostDetail else { return [] }
        return detail.agents.filter { ag in
            ag.parentAgentId == aid && ag.archivedAtRfc3339 == nil
        }
    }

    public var activeProviderSubagents: [FfiProviderSubagentDescriptor] {
        guard let aid = selectedAgentId, let detail = currentHostDetail else { return [] }
        return detail.providerSubagents[aid] ?? []
    }

    public func subagentArchiveKey(kind: String, id: String) -> String {
        let hid = selectedHostId ?? "local"
        return "\(hid)::\(kind)::\(id)"
    }

    public func isSubagentArchived(kind: String, id: String) -> Bool {
        archivedSubagentKeys.contains(subagentArchiveKey(kind: kind, id: id))
    }

    public func archiveManagedSubagent(_ childAgentId: String) {
        archiveSession(childAgentId)
        let key = subagentArchiveKey(kind: "managed", id: childAgentId)
        archivedSubagentKeys.insert(key)
        UserDefaults.standard.set(Array(archivedSubagentKeys), forKey: Self.archivedSubagentsDefaultsKey)
    }

    public func archiveNativeSubagent(_ subagentId: String) {
        let key = subagentArchiveKey(kind: "native", id: subagentId)
        archivedSubagentKeys.insert(key)
        UserDefaults.standard.set(Array(archivedSubagentKeys), forKey: Self.archivedSubagentsDefaultsKey)
        hapticLight()
        showToast("Archived subagent")
    }

    public func archiveFinishedSubagents() {
        var count = 0
        for child in activeManagedSubagents where !child.isRunning && !child.isAwaitingPermission {
            archiveManagedSubagent(child.agentId)
            count += 1
        }
        for native in activeProviderSubagents where native.status != .running {
            let key = subagentArchiveKey(kind: "native", id: native.subagentId)
            if !archivedSubagentKeys.contains(key) {
                archivedSubagentKeys.insert(key)
                count += 1
            }
        }
        UserDefaults.standard.set(Array(archivedSubagentKeys), forKey: Self.archivedSubagentsDefaultsKey)
        if count > 0 {
            hapticLight()
            showToast("Archived \(count) finished \(count == 1 ? "subagent" : "subagents")")
        }
    }

    public var visibleManagedSubagents: [FfiAgentCard] {
        activeManagedSubagents.filter { !isSubagentArchived(kind: "managed", id: $0.agentId) }
    }

    public var visibleProviderSubagents: [FfiProviderSubagentDescriptor] {
        activeProviderSubagents.filter { !isSubagentArchived(kind: "native", id: $0.subagentId) }
    }

    public var activeVisibleSubagentCount: Int {
        visibleManagedSubagents.count + visibleProviderSubagents.count
    }

    public var activeRunningBackgroundTaskCount: Int {
        (agentStage?.insights.backgroundTasks ?? []).filter { $0.status == .running }.count
    }

    public func openProviderSubagentTimeline(parentAgentId: String, subagentId: String) {
        guard let hid = selectedHostId else { return }
        core?.openProviderSubagentTimeline(
            hostId: hid,
            parentAgentId: parentAgentId,
            subagentId: subagentId
        )
        let events = core?.providerSubagentTimeline(
            hostId: hid,
            parentAgentId: parentAgentId,
            subagentId: subagentId
        ) ?? []
        subagentTimelinesCache["\(parentAgentId)::\(subagentId)"] = events
    }

    public func providerSubagentEvents(parentAgentId: String, subagentId: String) -> [FfiProviderSubagentEvent] {
        if let hid = selectedHostId, let live = core?.providerSubagentTimeline(
            hostId: hid,
            parentAgentId: parentAgentId,
            subagentId: subagentId
        ), !live.isEmpty {
            return live
        }
        return subagentTimelinesCache["\(parentAgentId)::\(subagentId)"] ?? []
    }

    // MARK: - Inspector & Sheet Openers (iPad 3rd Column + iPhone Sheets)

    public func openWorkspaceBoard(workspaceId: String) {
        fetchWorkspaceBoardAndEvents(workspaceId)
        hapticLight()
        if canShowInlineDiffInspector && activeWorkspace?.workspaceId == workspaceId {
            padInspectorTab = .board
            withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                isPadDiffInspectorVisible = true
            }
        } else {
            activeSheet = .workspaceBoard(workspaceId: workspaceId)
        }
    }

    public func openSessionOverview() {
        if let aid = selectedAgentId, let hid = selectedHostId {
            core?.listProviderSubagents(hostId: hid, parentAgentId: aid)
        }
        hapticLight()
        if canShowInlineDiffInspector {
            padInspectorTab = .overview
            withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                isPadDiffInspectorVisible = true
            }
        } else {
            activeSheet = .sessionOverview
        }
    }

    public func openWorkspaceFiles(initialPath: String? = nil) {
        refreshWorkspaceFiles()
        if let p = initialPath, !p.isEmpty {
            activeWorkspaceFilePath = p
            loadWorkspaceFile(path: p)
        }
        hapticLight()
        if canShowInlineDiffInspector {
            padInspectorTab = .files
            withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                isPadDiffInspectorVisible = true
            }
        } else {
            activeSheet = .workspaceFiles(initialPath: initialPath)
        }
    }

    public func openSideQuestion(prefillAndSubmit question: String? = nil) {
        hapticLight()
        if canShowInlineDiffInspector {
            padInspectorTab = .sideQuestion
            withAnimation(.spring(response: 0.28, dampingFraction: 0.86)) {
                isPadDiffInspectorVisible = true
            }
        } else {
            activeSheet = .sideQuestion
        }
        if let q = question?.trimmingCharacters(in: .whitespacesAndNewlines), !q.isEmpty {
            submitSideQuestion(q)
        }
    }

    // MARK: - Workspace File Viewer & Quote

    public func loadWorkspaceFile(path: String, forceRefresh: Bool = false) {
        guard let hid = selectedHostId, let coreRef = core else { return }
        let cwd = activeCwd
        let cacheKey = "\(hid)::\(cwd)::\(path)"
        activeWorkspaceFilePath = path
        if !forceRefresh, workspaceFileCache[cacheKey] != nil {
            workspaceFileError = nil
            return
        }
        workspaceFileLoadingPath = path
        workspaceFileError = nil
        Task { @MainActor [weak self] in
            do {
                let file: FfiWorkspaceFile = try await Task.detached(priority: .userInitiated) {
                    try coreRef.readWorkspaceFile(hostId: hid, cwd: cwd, path: path)
                }.value
                guard let self else { return }
                self.workspaceFileCache[cacheKey] = file
                if self.workspaceFileLoadingPath == path {
                    self.workspaceFileLoadingPath = nil
                }
            } catch {
                guard let self else { return }
                if self.workspaceFileLoadingPath == path {
                    self.workspaceFileLoadingPath = nil
                }
                if case let FfiOrbitError.Core(detail) = error {
                    self.workspaceFileError = detail
                } else {
                    self.workspaceFileError = error.localizedDescription
                }
            }
        }
    }

    public func cachedWorkspaceFile(path: String) -> FfiWorkspaceFile? {
        guard let hid = selectedHostId else { return nil }
        return workspaceFileCache["\(hid)::\(activeCwd)::\(path)"]
    }

    public func quoteFileLine(path: String, lineNumber: UInt32, content: String) {
        let snippetId = "\(path):\(lineNumber):\(content.hashValue)"
        guard !quotedDiffSnippets.contains(where: { $0.id == snippetId }) else {
            showToast("Line already quoted")
            return
        }
        let snippet = QuotedDiffSnippet(
            id: snippetId,
            path: path,
            lineNumber: lineNumber,
            kind: .context,
            content: content
        )
        quotedDiffSnippets.append(snippet)
        hapticLight()
        showToast("Quoted \(snippet.chipLabel)")
    }

    public func attachWorkspaceFileToComposer(_ path: String) {
        let item = ComposerContextItem.file(path: path)
        if !attachedContexts.contains(item) {
            attachedContexts.append(item)
        }
        hapticLight()
        showToast("Attached @\(OrbitFormatters.shortPath(path))")
    }

    // MARK: - Checkpoint Rewind

    public func rewindAgent(toBeforeSeq seq: UInt64, restoreFiles: Bool) {
        guard let hid = selectedHostId, let aid = selectedAgentId, let coreRef = core else { return }
        hapticMedium()
        showToast(restoreFiles ? "Rewinding conversation & files…" : "Rewinding conversation…")
        Task { @MainActor [weak self] in
            do {
                let result: FfiRewindAgentResult = try await Task.detached(priority: .userInitiated) {
                    try coreRef.rewindAgent(
                        hostId: hid,
                        agentId: aid,
                        toBeforeSeq: seq,
                        restoreFiles: restoreFiles
                    )
                }.value
                guard let self else { return }
                if let prompt = result.restoredPrompt, !prompt.isEmpty, self.composerText.isEmpty {
                    self.composerText = prompt
                }
                self.reloadHostDetail(hid)
                self.reloadAgentStage(hostId: hid, agentId: aid)
                self.refreshGitReview()
                self.hapticSuccess()
                let msg = result.restoredFiles > 0
                    ? "Rewound (\(result.restoredFiles) files restored)"
                    : (result.notice ?? "Rewound conversation")
                self.showToast(msg)
            } catch {
                guard let self else { return }
                if case let FfiOrbitError.Core(detail) = error {
                    self.lastError = detail
                } else {
                    self.lastError = error.localizedDescription
                }
                self.hapticWarning()
            }
        }
    }

    // MARK: - Side Question (/btw) Streaming

    public var activeSideQuestionScopeKey: String {
        let hid = selectedHostId ?? "none"
        let scope = selectedAgentId ?? "draft::\(activeCwd)"
        return "\(hid)::\(scope)"
    }

    public var activeSideQuestions: [SideQuestionEntry] {
        sideQuestionsByScope[activeSideQuestionScopeKey] ?? []
    }

    public func sideQuestionDraft(for scopeKey: String) -> String {
        sideQuestionDraftsByScope[scopeKey] ?? ""
    }

    public func setSideQuestionDraft(_ text: String, for scopeKey: String) {
        sideQuestionDraftsByScope[scopeKey] = text
    }

    public func submitSideQuestion(_ rawQuestion: String) {
        let question = rawQuestion.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !question.isEmpty, let hid = selectedHostId, let coreRef = core else { return }
        let scopeKey = activeSideQuestionScopeKey
        let entry = SideQuestionEntry(question: question, isStreaming: true)
        var list = sideQuestionsByScope[scopeKey] ?? []
        list.append(entry)
        sideQuestionsByScope[scopeKey] = list
        sideQuestionDraftsByScope[scopeKey] = ""

        let listener = WeakSideQuestionListenerBridge(owner: self, scopeKey: scopeKey, entryId: entry.id)
        activeSideQuestionListeners[entry.id] = listener

        let req = FfiAskSideQuestionRequest(
            hostId: hid,
            agentId: selectedAgentId,
            cwd: activeCwd,
            provider: selectedProvider,
            model: selectedModel,
            question: question,
            contextSummary: ""
        )
        hapticLight()

        Task { @MainActor [weak self] in
            do {
                let reqId: String = try await Task.detached(priority: .userInitiated) {
                    try coreRef.askSideQuestion(req: req, listener: listener)
                }.value
                guard let self else { return }
                if var items = self.sideQuestionsByScope[scopeKey],
                   let idx = items.firstIndex(where: { $0.id == entry.id }) {
                    items[idx].requestId = reqId
                    self.sideQuestionsByScope[scopeKey] = items
                }
            } catch {
                guard let self else { return }
                let detail: String
                if case let FfiOrbitError.Core(msg) = error {
                    detail = msg
                } else {
                    detail = error.localizedDescription
                }
                if var items = self.sideQuestionsByScope[scopeKey],
                   let idx = items.firstIndex(where: { $0.id == entry.id }) {
                    items[idx].isStreaming = false
                    items[idx].error = detail
                    self.sideQuestionsByScope[scopeKey] = items
                }
                self.activeSideQuestionListeners.removeValue(forKey: entry.id)
            }
        }
    }

    func handleSideQuestionEvent(scopeKey: String, entryId: String, event: FfiSideQuestionEvent) {
        guard var items = sideQuestionsByScope[scopeKey],
              let idx = items.firstIndex(where: { $0.id == entryId }) else {
            return
        }
        switch event {
        case let .delta(_, textDelta):
            items[idx].answer.append(textDelta)
        case let .done(_, fullText, error):
            if !fullText.isEmpty {
                items[idx].answer = fullText
            }
            items[idx].error = error
            items[idx].isStreaming = false
            activeSideQuestionListeners.removeValue(forKey: entryId)
        }
        sideQuestionsByScope[scopeKey] = items
    }

    public func clearSideQuestions() {
        sideQuestionsByScope.removeValue(forKey: activeSideQuestionScopeKey)
        hapticLight()
    }

    public func promoteSideQuestionToComposer(_ entry: SideQuestionEntry) {
        let excerpt = entry.answer.trimmingCharacters(in: .whitespacesAndNewlines)
        let prefix = entry.question.trimmingCharacters(in: .whitespacesAndNewlines)
        if !excerpt.isEmpty {
            quoteAssistantReply(turnKey: "btw-\(entry.id)", text: "Q: \(prefix)\n\n\(excerpt)")
        } else if !prefix.isEmpty {
            composerText = composerText.isEmpty ? prefix : "\(composerText)\n\(prefix)"
        }
        activeSheet = nil
        requestComposerFocus()
    }

    // MARK: - Fleet Daemon Version & One-Click Over-the-Wire Update

    public var appShortVersion: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0.7.2"
    }

    public func isHostProtocolBehind(_ host: FfiHostCard) -> Bool {
        guard OrbitFormatters.isHostOnline(host.connectionState),
              let daemonProto = host.daemonProtocolVersion else {
            return false
        }
        return daemonProto < host.clientProtocolVersion
    }

    public func isHostDaemonOutdated(_ host: FfiHostCard) -> Bool {
        guard OrbitFormatters.isHostOnline(host.connectionState),
              let daemonSemver = OrbitFormatters.normalizeSemver(host.daemonVersion),
              let appSemver = OrbitFormatters.normalizeSemver(appShortVersion) else {
            return false
        }
        return OrbitFormatters.compareSemver(daemonSemver, appSemver) < 0
    }

    public var outdatedOnlineHosts: [FfiHostCard] {
        hosts.filter { isHostDaemonOutdated($0) || isHostProtocolBehind($0) }
    }

    public func dismissVersionMismatch(for hostId: String) {
        dismissedVersionMismatchHosts.insert(hostId)
        hapticLight()
    }

    public func upgradeHostDaemon(_ hostId: String, targetVersion: String? = nil) {
        guard let coreRef = core else { return }
        if hostUpgradeStates[hostId]?.isUpgrading == true { return }
        hostUpgradeStates[hostId] = HostDaemonUpgradeState(
            isUpgrading: true,
            statusMessage: "Upgrading daemon…",
            error: nil
        )
        hapticMedium()
        Task { @MainActor [weak self] in
            do {
                let message: String = try await Task.detached(priority: .userInitiated) {
                    try coreRef.upgradeHostDaemonOverWire(hostId: hostId, targetVersion: targetVersion)
                }.value
                guard let self else { return }
                self.hostUpgradeStates[hostId] = HostDaemonUpgradeState(
                    isUpgrading: false,
                    statusMessage: message.isEmpty ? "Upgrade triggered" : message,
                    error: nil
                )
                self.hapticSuccess()
                self.showToast("Daemon upgrade sent")
                try? await Task.sleep(nanoseconds: 2_500_000_000)
                self.reconnectHost(hostId)
            } catch {
                guard let self else { return }
                let detail: String
                if case let FfiOrbitError.Core(msg) = error {
                    detail = msg
                } else {
                    detail = error.localizedDescription
                }
                self.hostUpgradeStates[hostId] = HostDaemonUpgradeState(
                    isUpgrading: false,
                    statusMessage: nil,
                    error: detail
                )
                self.hapticWarning()
            }
        }
    }

    public func upgradeAllOutdatedHosts() {
        for host in outdatedOnlineHosts {
            upgradeHostDaemon(host.hostId)
        }
    }

    // MARK: - Project Reordering

    public func moveProject(sortOrderKey: String, offset: Int, within projects: [FfiSidebarProjectGroup]) {
        let keys = projects.map(\.sortOrderKey)
        guard let idx = keys.firstIndex(of: sortOrderKey) else { return }
        let targetIdx = idx + offset
        guard targetIdx >= 0 && targetIdx < keys.count else { return }
        var reordered = keys
        let moved = reordered.remove(at: idx)
        reordered.insert(moved, at: targetIdx)
        do {
            if let updated = try core?.reorderProjects(orderedKeys: reordered) {
                projectOrder = updated
            }
            sidebarProjectSort = .manual
            refreshSidebar()
            hapticLight()
        } catch {
            lastError = error.localizedDescription
        }
    }

    // MARK: - Automations (Cron Schedules & Heartbeats)

    public func createSchedule(
        name: String,
        cronExpr: String,
        prompt: String,
        cwd: String,
        provider: FfiProviderKind,
        mode: FfiSessionMode,
        worktree: Bool,
        archiveOnFinish: Bool
    ) {
        guard let hid = selectedHostId else { return }
        let req = FfiCreateScheduleRequest(
            hostId: hid,
            name: name.trimmingCharacters(in: .whitespacesAndNewlines),
            cronExpr: cronExpr.trimmingCharacters(in: .whitespacesAndNewlines),
            timezone: TimeZone.current.identifier,
            prompt: prompt.trimmingCharacters(in: .whitespacesAndNewlines),
            cwd: cwd,
            workspaceId: activeWorkspace?.workspaceId,
            provider: provider,
            model: selectedModel,
            effort: selectedEffort,
            mode: mode,
            worktree: worktree,
            archiveOnFinish: archiveOnFinish,
            maxRuns: nil
        )
        core?.createSchedule(req: req)
        hapticSuccess()
        showToast("Created automation")
    }

    public func pauseSchedule(_ scheduleId: String) {
        guard let hid = selectedHostId else { return }
        core?.pauseSchedule(hostId: hid, scheduleId: scheduleId)
        hapticLight()
    }

    public func resumeSchedule(_ scheduleId: String) {
        guard let hid = selectedHostId else { return }
        core?.resumeSchedule(hostId: hid, scheduleId: scheduleId)
        hapticLight()
    }

    public func runScheduleOnce(_ scheduleId: String) {
        guard let hid = selectedHostId else { return }
        core?.runScheduleOnce(hostId: hid, scheduleId: scheduleId)
        hapticLight()
        showToast("Triggered automation run")
    }

    public func deleteSchedule(_ scheduleId: String) {
        guard let hid = selectedHostId else { return }
        core?.deleteSchedule(hostId: hid, scheduleId: scheduleId)
        hapticMedium()
        showToast("Deleted automation")
    }

    public func fetchScheduleRuns(_ scheduleId: String) {
        guard let hid = selectedHostId else { return }
        core?.fetchScheduleRuns(hostId: hid, scheduleId: scheduleId, limit: 20)
        if let runs = core?.scheduleRuns(hostId: hid, scheduleId: scheduleId) {
            scheduleRunsCache[scheduleId] = runs
        }
    }

    public func scheduleRuns(for scheduleId: String) -> [FfiScheduleRunRecord] {
        if let hid = selectedHostId, let live = core?.scheduleRuns(hostId: hid, scheduleId: scheduleId), !live.isEmpty {
            return live
        }
        return scheduleRunsCache[scheduleId] ?? []
    }

    // MARK: - Stop Background Task (Desktop Parity)

    public func stopBackgroundTask(_ task: FfiSessionBackgroundTask) {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        let prompt = "Please stop the running background task \"\(task.title)\" (id: \(task.id))."
        core?.sendPrompt(hostId: hid, agentId: aid, prompt: prompt)
        hapticLight()
        showToast("Requested stop for \(task.title)")
    }
}
