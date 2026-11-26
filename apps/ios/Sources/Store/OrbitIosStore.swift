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
    case worktreeHandoff(workspaceId: String)
    case globalSearch
    case personalRouter
    case transcriptExport

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
        case let .worktreeHandoff(wsId): return "worktreeHandoff:\(wsId)"
        case .globalSearch: return "globalSearch"
        case .personalRouter: return "personalRouter"
        case .transcriptExport: return "transcriptExport"
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
    case project(projectId: String, currentName: String)

    public var id: String {
        switch self {
        case let .agent(hid, aid, _, _): return "agent:\(hid):\(aid)"
        case let .workspace(hid, wid, _): return "ws:\(hid):\(wid)"
        case let .project(pid, _): return "proj:\(pid)"
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
    case goal
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
        case .goal: return "Autonomous Goal"
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
        case .goal: return "Work autonomously until the goal is achieved"
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
        case .goal: return "target"
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
        case .goal: return .goal
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
        case .goal: self = .goal
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
        case "goal": return "target"
        case "review": return "doc.text.magnifyingglass"
        case "diff": return "arrow.left.arrow.right"
        case "btw": return "bubble.left.and.text.bubble.right"
        case "files": return "folder"
        case "compact": return "rectangle.compress.vertical"
        case "orbit": return "square.3.layers.3d"
        case "orbit-handoff": return "paperplane"
        case "orbit-committee": return "person.3"
        case "orbit-advisor": return "sparkles"
        case "ask", "supervised": return "shield"
        case "edits", "auto": return "checkmark.shield"
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

public enum OrbitBusySubmitMode: String, CaseIterable, Identifiable, Equatable {
    case queue
    case steer

    public var id: String { rawValue }
}

private struct PersistedComposerContextDTO: Codable {
    let kind: String
    let id: String?
    let label: String?
    let secondary: String?
    let content: String?
    let staged: Bool?
    let lineNumber: UInt32?
    let sizeBytes: Int?
    let lineCount: Int?
}

private struct PersistedQuotedDiffDTO: Codable {
    let id: String
    let path: String
    let lineNumber: UInt32?
    let kindTag: String
    let content: String
}

private struct PersistedComposerDraftDTO: Codable {
    let text: String
    let slashDirectiveRaw: String?
    let contexts: [PersistedComposerContextDTO]
    let quotes: [PersistedQuotedDiffDTO]
}

public struct OrbitScopeComposerDraft: Equatable {
    public var text: String
    public var slashDirective: OrbitSlashDirective?
    public var attachedContexts: [ComposerContextItem]
    public var quotedDiffSnippets: [QuotedDiffSnippet]
    public var registeredInlineMentions: [InlineMentionRegistration]
}

private struct OrbitFailedAttachmentPayload {
    let hostId: String
    let contextId: String
    let filename: String
    let mimeType: String?
    let data: Data
    let completedItem: @MainActor (FfiStagedAttachment) -> ComposerContextItem
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
    // High-frequency isolated observable slices (prevent stage/PTY ticks from invalidating unrelated UI)
    public let stageState = OrbitStageStreamState()
    public let terminalState = OrbitTerminalState()

    // Fleet & Host State
    @Published public private(set) var hosts: [FfiHostCard] = []
    @Published public private(set) var projects: [FfiClientProject] = []
    @Published public private(set) var projectOrder: [String] = []
    @Published public private(set) var totalPendingApprovals: UInt32 = 0
    @Published public var selectedHostId: String?
    @Published public private(set) var currentHostDetail: FfiHostDetail?
    @Published public private(set) var hostDetailsByHostId: [String: FfiHostDetail] = [:]
    public private(set) var agentStageHostId: String? {
        get { stageState.agentStageHostId }
        set { stageState.setAgentStageHostId(newValue) }
    }
    private var agentStageScopeKey: String? = nil

    // Active Session & Stage (`selectedAgentId == nil` means New Session draft)
    @Published public var selectedAgentId: String?
    public private(set) var agentStage: FfiAgentStage? {
        get { stageState.agentStage }
        set { stageState.setAgentStage(newValue) }
    }
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
    @Published public var activeWorkspaceFileLine: Int? = nil
    @Published public private(set) var workspaceFileCache: [String: FfiWorkspaceFile] = [:]
    @Published public private(set) var workspaceFileLoadingPath: String? = nil
    @Published public private(set) var workspaceFileError: String? = nil
    @Published public private(set) var gitReview: FfiGitReview?
    @Published public private(set) var gitStagedDiffCache: [String: [FfiDiffFile]] = [:]
    @Published public private(set) var gitUnstagedDiffCache: [String: [FfiDiffFile]] = [:]
    @Published public var gitStagedMode: Bool = false
    @Published public var gitReviewScope: OrbitGitReviewScope = .workingTree
    @Published public var jumpToDiffFilePath: String?
    public private(set) var terminalScreen: FfiTerminalScreen? {
        get { terminalState.terminalScreen }
        set {
            let tid = currentHostDetail?.activeTerminalId ?? currentHostDetail?.terminals.first?.terminalId
            terminalState.setTerminalScreen(newValue, terminalId: tid)
        }
    }
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
    @Published public var preferredLanguageCode: String = ""
    public var preferredLocale: Locale {
        preferredLanguageCode.isEmpty ? .autoupdatingCurrent : Locale(identifier: preferredLanguageCode)
    }
    @Published public var toastIsWarning: Bool = false
    @Published public var renameTarget: OrbitRenameTarget? = nil
    @Published public private(set) var subagentTimelinesCache: [String: [FfiProviderSubagentEvent]] = [:]
    @Published public private(set) var scheduleRunsCache: [String: [FfiScheduleRunRecord]] = [:]
    @Published public var resolvedChildDecisionsByRequestId: [String: String] = [:]
    @Published public var pendingArchiveUndo: OrbitPendingArchiveUndo? = nil
    @Published public var composerUploadStates: [String: OrbitComposerUploadState] = [:]
    @Published public var isGitRepoCache: [String: Bool] = [:]
    public var isStageHydrating: Bool {
        get { stageState.isStageHydrating }
        set { stageState.setIsStageHydrating(newValue) }
    }
    public var activeStageLoadError: String? {
        get { stageState.activeStageLoadError }
        set { stageState.setActiveStageLoadError(newValue) }
    }
    @Published public var subagentInspectorTarget: OrbitSubagentInspectorTarget? = nil
    @Published public var pendingSheetDestination: OrbitSheetDestination? = nil
    @Published public var isDemoChatMode: Bool = false
    @Published public var stageFindQuery: String = ""
    @Published public var isStageFindBarVisible: Bool = false
    @Published public var jumpToTurnKey: String? = nil
    private var activeSideQuestionRequestIds: Set<String> = []
    private var workspaceFileCacheOrder: [String] = []
    private var isApplyingCloudPrefs: Bool = false
    private var composerDraftsByScope: [String: OrbitScopeComposerDraft] = [:]
    private var activeComposerScopeKey: String = ""
    private var failedUploadPayloadsByContextId: [String: OrbitFailedAttachmentPayload] = [:]

    // Composer State & Live Queueing
    @Published public var composerText: String = "" {
        didSet {
            if oldValue != composerText && !isRestoringDraft {
                scheduleSaveAndPersistComposerDraft()
            }
        }
    }
    @Published public var selectedProvider: FfiProviderKind = .claudeCode
    @Published public var selectedModel: String?
    @Published public var selectedEffort: FfiReasoningEffort?
    @Published public var fastModeEnabled: Bool = false
    @Published public var selectedMode: FfiSessionMode = .acceptEdits
    @Published public var composerBusySubmitMode: OrbitBusySubmitMode = .queue
    @Published public var slashDirective: OrbitSlashDirective? {
        didSet {
            if !isRestoringDraft && oldValue != slashDirective {
                scheduleSaveAndPersistComposerDraft()
            }
        }
    }
    private var hasUserCustomizedDraftModel: Bool = false
    private var draftSelectionsByHostProvider: [String: (modelId: String?, effort: FfiReasoningEffort?)] = [:]
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
    @Published public var drawerFilter: OrbitDrawerFilter = .all {
        didSet {
            UserDefaults.standard.set(drawerFilter.rawValue, forKey: Self.drawerFilterDefaultsKey)
            pushCloudSyncPreferencesPatch(["drawerFilter": drawerFilter.rawValue])
            refreshSidebar()
        }
    }
    @Published public var sidebarProjectSort: OrbitSidebarProjectSort = .recent {
        didSet {
            UserDefaults.standard.set(sidebarProjectSort.rawValue, forKey: Self.sidebarProjectSortDefaultsKey)
            pushCloudSyncPreferencesPatch(["sidebarProjectSort": sidebarProjectSort.rawValue])
            refreshSidebar()
        }
    }
    @Published public var sidebarThreadFilter: OrbitSidebarThreadFilter = .all {
        didSet {
            UserDefaults.standard.set(sidebarThreadFilter.rawValue, forKey: Self.sidebarThreadFilterDefaultsKey)
            pushCloudSyncPreferencesPatch(["sidebarThreadFilter": sidebarThreadFilter.rawValue])
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

    // Zero-Server E2EE Cloud Sync (GitHub Device Flow + Secret Gist)
    @Published public private(set) var cloudSyncStatus: FfiCloudSyncStatus?
    @Published public private(set) var cloudSyncBusy: Bool = false
    @Published public var cloudSyncError: String?
    @Published public private(set) var cloudSyncDeviceFlow: FfiGitHubDeviceFlowStart?
    private var cloudDeviceFlowTask: Task<Void, Never>?

    // 5.0 Parity State — Global Overview, Keychain, Push/LiveActivity, 6-Tab Automations, Git Write, Search, Router & Mesh
    @Published public var isShowingGlobalOverview: Bool = false
    @Published public private(set) var masterKeySource: OrbitKeychain.MasterKeySource? = nil
    @Published public var liveActivitiesEnabled: Bool = true
    @Published public internal(set) var apnsDeviceTokenHex: String? = nil
    @Published public internal(set) var liveActivityTokenHex: String? = nil
    @Published public internal(set) var pushDevicesSnapshot: FfiPushDevicesSnapshot? = nil
    @Published public internal(set) var pushBusy: Bool = false
    @Published public internal(set) var goalRuns: [FfiGoalRunRecord] = []
    @Published public internal(set) var monitorWatches: [FfiMonitorWatchRecord] = []
    @Published public internal(set) var conditionalTimers: [FfiConditionalTimerRecord] = []
    @Published public internal(set) var workflowRuns: [FfiWorkflowRunRecord] = []
    @Published public internal(set) var permissionRules: [PermissionRuleRecord] = []
    @Published public internal(set) var guardianConfigSnapshot: GuardianConfigSnapshotView? = nil
    @Published public internal(set) var automationsLoading: Bool = false
    @Published public internal(set) var gitPrStatus: GitPrStatus? = nil
    @Published public internal(set) var gitStashes: [GitStashEntry] = []
    @Published public internal(set) var gitConflicts: [GitConflictFile] = []
    @Published public internal(set) var gitRebaseStatus: GitRebaseStatus? = nil
    @Published public internal(set) var gitWriteBusy: Bool = false
    @Published public var gitWriteError: String? = nil
    @Published public var globalSearchQuery: String = ""
    @Published public internal(set) var globalSearchHits: [ConversationSearchHit] = []
    @Published public internal(set) var globalSearchRemoteQueried: UInt32 = 0
    @Published public internal(set) var globalSearchRemoteFailed: UInt32 = 0
    @Published public internal(set) var isSearchingGlobal: Bool = false
    @Published public internal(set) var routerHostCandidates: [RouterHostCapabilities] = []
    @Published public internal(set) var routerStatusByHost: [String: RouterStatusSnapshot] = [:]
    @Published public internal(set) var fleetMeshSnapshot: [String: [ConnectedHostSummary]] = [:]
    @Published public internal(set) var routerBusy: Bool = false
    @Published public internal(set) var activeSessionUsage: SessionUsage? = nil
    @Published public internal(set) var personalAgentDraftPrompt: String = ""

    // Windowed tail-first pagination & off-main-thread core read/write pipeline
    public private(set) var stageHasOlderTurns: Bool {
        get { stageState.stageHasOlderTurns }
        set { stageState.setStageHasOlderTurns(newValue) }
    }
    public private(set) var isLoadingOlderTurns: Bool {
        get { stageState.isLoadingOlderTurns }
        set { stageState.setIsLoadingOlderTurns(newValue) }
    }
    private let coreReadQueue = DispatchQueue(label: "dev.orbit.ios.core-read", qos: .userInitiated)
    private let coreWriteQueue = DispatchQueue(label: "dev.orbit.ios.core-write", qos: .userInitiated)
    private let draftPersistQueue = DispatchQueue(label: "dev.orbit.ios.draft-persist", qos: .utility)
    private var stageFetchEpoch: UInt64 = 0
    private var stageAuxRevision: UInt64 = 0
    private var stageTurnLimit: UInt32 = 15
    private var stageWindowStartTurnIndex: UInt32? = nil
    private var pendingTimelineRefreshWorkItem: DispatchWorkItem?
    private var isTimelineRefreshCoalescing = false
    private var isStageFetchInFlight = false
    private var hasPendingStageFetch = false

    private var fleetFetchEpoch: UInt64 = 0
    private var sidebarFetchEpoch: UInt64 = 0
    private var hostDetailFetchEpochs: [String: UInt64] = [:]
    private var gitFetchEpoch: UInt64 = 0
    private var terminalFetchEpoch: UInt64 = 0
    private var pendingFleetHostCoalesceWorkItem: DispatchWorkItem?
    private var pendingFleetReloadNeeded = false
    private var pendingHostDetailRefreshIds: Set<String> = []
    private var pendingCloudSyncRefreshNeeded = false
    private var pendingGitRefreshWorkItem: DispatchWorkItem?
    private var pendingTerminalRefreshWorkItem: DispatchWorkItem?
    private var isGitReviewStale = false
    private var isWorkspaceFilesStale = false
    private var composerDraftFileURL: URL? = nil
    private var pendingDraftPersistWorkItem: DispatchWorkItem?
    private var isRestoringDraft = false
    private var pendingArchiveUndoDismissTask: Task<Void, Never>?
    private var pendingRestoreAgentId: String? = nil
    private var didConsumeInitialSelection = false

    var core: OrbitMobileCore?
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
    private static let drawerFilterDefaultsKey = "orbit.ios.drawerFilter"
    private static let sidebarProjectSortDefaultsKey = "orbit.ios.sidebarProjectSort"
    private static let sidebarThreadFilterDefaultsKey = "orbit.ios.sidebarThreadFilter"
    private static let lastSelectedHostIdDefaultsKey = "orbit.ios.lastSelectedHostId"
    private static let lastSelectedAgentIdDefaultsKey = "orbit.ios.lastSelectedAgentId"
    private static let preferredLanguageDefaultsKey = "orbit.ios.preferredLanguage"

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
        if let savedDrawerFilter = UserDefaults.standard.string(forKey: Self.drawerFilterDefaultsKey),
           let df = OrbitDrawerFilter(rawValue: savedDrawerFilter) {
            self.drawerFilter = df
        }
        if let savedSort = UserDefaults.standard.string(forKey: Self.sidebarProjectSortDefaultsKey),
           let s = OrbitSidebarProjectSort(rawValue: savedSort) {
            self.sidebarProjectSort = s
        }
        if let savedThreadFilter = UserDefaults.standard.string(forKey: Self.sidebarThreadFilterDefaultsKey),
           let tf = OrbitSidebarThreadFilter(rawValue: savedThreadFilter) {
            self.sidebarThreadFilter = tf
        }
        if let savedLang = UserDefaults.standard.string(forKey: Self.preferredLanguageDefaultsKey) {
            self.preferredLanguageCode = savedLang
        }
        do {
            guard let appSupportDir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
                throw OrbitStorageError.applicationSupportUnavailable
            }
            let dbURL = try OrbitStorageProtection.prepareProtectedDatabaseURL(in: appSupportDir)
            if let draftURL = try? OrbitStorageProtection.prepareProtectedDatabaseURL(
                in: appSupportDir,
                fileName: "composer_drafts.json"
            ) {
                self.composerDraftFileURL = draftURL
                self.loadPersistedComposerDrafts(from: draftURL)
            }
            if UserDefaults.standard.object(forKey: "orbit.ios.liveActivitiesEnabled") != nil {
                self.liveActivitiesEnabled = UserDefaults.standard.bool(forKey: "orbit.ios.liveActivitiesEnabled")
            }
            let instance: OrbitMobileCore
            let storageDir = dbURL.deletingLastPathComponent()
            do {
                let keyResult = try OrbitKeychain.shared.loadOrCreateMasterKey(
                    fallbackDirectory: storageDir
                )
                do {
                    instance = try OrbitMobileCore.newWithMasterKey(
                        dbPath: dbURL.path,
                        masterKeyBytes: keyResult.key
                    )
                    self.masterKeySource = keyResult.source
                } catch {
                    if let fallbackKey = OrbitKeychain.shared.readExistingProtectedFallbackKey(in: storageDir),
                       fallbackKey != keyResult.key {
                        instance = try OrbitMobileCore.newWithMasterKey(
                            dbPath: dbURL.path,
                            masterKeyBytes: fallbackKey
                        )
                        self.masterKeySource = .protectedFileFallback
                    } else {
                        throw error
                    }
                }
            } catch let masterErr {
                // Only fall back to unkeyed open for legacy unencrypted sandboxes without a fallback key file;
                // otherwise preserve the exact vault error so callers know why decryption failed.
                if OrbitKeychain.shared.readExistingProtectedFallbackKey(in: storageDir) == nil,
                   let legacyCore = try? OrbitMobileCore.openWithOptions(dbPath: dbURL.path, autoDiscoverLocal: false) {
                    instance = legacyCore
                } else {
                    throw masterErr
                }
            }
            self.core = instance

            let savedHostId = UserDefaults.standard.string(forKey: Self.lastSelectedHostIdDefaultsKey)
            let savedAgentId = UserDefaults.standard.string(forKey: Self.lastSelectedAgentIdDefaultsKey)
            if let savedHostId, !savedHostId.isEmpty {
                self.selectedHostId = savedHostId
                self.pendingRestoreAgentId = (savedAgentId?.isEmpty == false) ? savedAgentId : nil
            }

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

            let bridge = WeakMobileListenerBridge(owner: self)
            self.listenerBridge = bridge
            instance.subscribeThrottled(listener: bridge, intervalMs: 40)

            self.reloadFleet()
            self.refreshSidebar()
            self.startNetworkMonitor()
            let initialPermRaw = self.defaultPermissionModeRaw
            coreReadQueue.async { [weak self] in
                let catalogs = instance.fallbackProviderCatalogs()
                let cmds = instance.slashCommands(platform: .ios, query: nil)
                let localUri = instance.discoverLocalDaemonUri()
                let syncStatus = instance.cloudSyncStatus()
                instance.setAccountDefaultMode(
                    mode: Self.sessionModeFromRaw(initialPermRaw),
                    updatedAtMs: 0
                )
                instance.connectAllHosts()
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    if self.cachedFallbackCatalogs.isEmpty {
                        self.cachedFallbackCatalogs = catalogs
                    }
                    if self.slashCommands.isEmpty {
                        self.slashCommands = cmds
                    }
                    self.refreshPasteboardAvailability()
                    if self.localDaemonUri != localUri {
                        self.localDaemonUri = localUri
                    }
                    if self.cloudSyncStatus != syncStatus {
                        self.cloudSyncStatus = syncStatus
                    }
                    self.hydrateCloudSyncPreferences()
                }
            }
        } catch {
            self.lastError = OrbitErrorFormatter.humanize(error)
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
        guard let aid = selectedAgentId else { return nil }
        // Prefer freshly updated `HostChanged` host-detail cards over `agentStage?.agent`
        // (matching Desktop `stream-stage.tsx`) so status transitions (`Running -> Idle / waiting`)
        // reflect immediately without waiting for another `TimelineChanged` event.
        if let selectedHostId,
           let matched = hostDetailsByHostId[selectedHostId]?.agents.first(where: { $0.agentId == aid }) {
            return matched
        }
        if let matched = currentHostDetail?.agents.first(where: { $0.agentId == aid }) {
            return matched
        }
        if let stageAgent = agentStage?.agent,
           stageAgent.agentId == aid,
           (agentStageHostId == nil || agentStageHostId == selectedHostId) {
            return stageAgent
        }
        return nil
    }

    public var isDraftNewSession: Bool {
        selectedAgentId == nil
    }

    /// Single source of truth for foreground execution (matching Desktop `isAgentForegroundRunning`).
    /// Background-only waiting states never mark a finished turn pending or show the red Stop button.
    public var isActiveAgentRunning: Bool {
        isActiveAgentTurnBusy
    }

    /// True while an active foreground turn or autonomous wakeup is executing, OR while a pending
    /// tool permission / clarifying question is blocking completion (so follow-ups enqueue safely).
    public var isActiveAgentTurnBusy: Bool {
        if let stage = agentStage,
           stage.hostId == selectedHostId,
           stage.agent?.agentId == selectedAgentId,
           (!stage.pendingPermissions.isEmpty || !stage.pendingQuestions.isEmpty) {
            return true
        }
        guard let agent = activeAgent else { return false }
        return agent.isForegroundRunning || agent.isAwaitingPermission
    }

    /// True when the currently active session is the host's canonical Personal Agent.
    public var isActiveAgentPersonalAgent: Bool {
        guard !isDraftNewSession, let agent = activeAgent else { return false }
        return agent.isPersonalRouter
    }

    /// True when no foreground turn is active, yet the agent still has running background tasks.
    public var isActiveAgentBackgroundWaiting: Bool {
        guard let agent = activeAgent else { return false }
        let hasBgTasks = agent.hasBackgroundTasks || !(agentStage?.insights.backgroundTasks.isEmpty ?? true)
        return OrbitAgentLogic.isAgentBackgroundWaiting(
            statusRaw: agent.statusLabel,
            hasActiveTurn: agent.isForegroundRunning,
            hasBackgroundTasks: hasBgTasks
        )
    }

    public func detail(forHostId hostId: String?) -> FfiHostDetail? {
        guard let hostId, !hostId.isEmpty else { return currentHostDetail }
        if let cached = hostDetailsByHostId[hostId] {
            return cached
        }
        if currentHostDetail?.host.hostId == hostId {
            return currentHostDetail
        }
        return nil
    }

    private static func canonicalizeSymlinkPath(_ path: String) -> String {
        let norm = OrbitPathLogic.normalizePath(path)
        if norm.hasPrefix("/private/var/") {
            return "/var/" + norm.dropFirst("/private/var/".count)
        }
        if norm == "/private/var" { return "/var" }
        if norm.hasPrefix("/private/tmp/") {
            return "/tmp/" + norm.dropFirst("/private/tmp/".count)
        }
        if norm == "/private/tmp" { return "/tmp" }
        return norm
    }

    public func isActiveFolderGitRepo(hostId: String? = nil, cwd: String? = nil) -> Bool {
        let hid = hostId ?? selectedHostId ?? ""
        let rawTarget = OrbitPathLogic.normalizePath(cwd ?? activeCwd)
        let canonTarget = Self.canonicalizeSymlinkPath(rawTarget)
        if let cached = isGitRepoCache["\(hid)::\(rawTarget)"] ?? isGitRepoCache["\(hid)::\(canonTarget)"] {
            return cached
        }
        if let detail = detail(forHostId: hid),
           let inspected = detail.gitReposByCwd.first(where: {
               OrbitFormatters.arePathsEquivalent($0.key, rawTarget)
                   || Self.canonicalizeSymlinkPath($0.key) == canonTarget
           })?.value {
            return inspected.isGitRepo || !inspected.nestedGitRepos.isEmpty
        }
        if let review = gitReview,
           (OrbitFormatters.arePathsEquivalent(review.cwd, rawTarget)
            || Self.canonicalizeSymlinkPath(review.cwd) == canonTarget) {
            return review.isGitRepo
        }
        if let repo = detail(forHostId: hid)?.discoveredProjects.first(where: {
            OrbitFormatters.arePathsEquivalent($0.path, rawTarget)
                || Self.canonicalizeSymlinkPath($0.path) == canonTarget
        }) {
            return (repo.gitBranch?.isEmpty == false)
        }
        return false
    }

    public var isActiveCwdGitRepo: Bool {
        isActiveFolderGitRepo(hostId: selectedHostId, cwd: activeCwd)
    }

    public func overviewBadgeTone(forAgent agent: FfiAgentCard, hostId: String) -> OrbitBadgeTone {
        let subagentApproval = pendingSubagentApprovalCount(forParentHostId: hostId, parentAgentId: agent.agentId) > 0
        let isError = agent.statusLabel.lowercased().contains("error") || agent.statusLabel.lowercased().contains("fail")
        return OrbitAgentLogic.overviewBadgeTone(
            hasPendingApproval: agent.isAwaitingPermission,
            hasSubagentApproval: subagentApproval,
            isError: isError,
            hasUnread: isThreadUnread(hostId: hostId, agent: agent),
            hasBackgroundTasks: agent.hasBackgroundTasks,
            isRunning: agent.isForegroundRunning
        )
    }

    public func pendingSubagentApprovalCount(forParentHostId hostId: String, parentAgentId: String) -> Int {
        guard let detail = detail(forHostId: hostId) else { return 0 }
        return detail.agents.filter { child in
            let sameHost = (child.parentHostId ?? hostId) == hostId
            return sameHost && child.parentAgentId == parentAgentId && child.isAwaitingPermission && child.archivedAtRfc3339 == nil
        }.count
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
        sidebarFetchEpoch &+= 1
        let epoch = sidebarFetchEpoch
        let req = FfiFleetSidebarRequest(
            sort: sidebarProjectSort.ffiValue,
            filter: sidebarThreadFilter.ffiValue,
            query: drawerSearchQuery,
            activeHostId: selectedHostId,
            activeAgentId: isDraftNewSession ? nil : selectedAgentId,
            activeCwd: activeCwd
        )
        coreReadQueue.async { [weak self] in
            let nextSidebar = core.fleetSidebarRequest(req: req)
            Task { @MainActor [weak self] in
                guard let self, self.sidebarFetchEpoch == epoch else { return }
                if self.sidebarView != nextSidebar {
                    self.sidebarView = nextSidebar
                }
            }
        }
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
        force: Bool = false,
        refreshSidebarAfter: Bool = true
    ) {
        let key = "\(hostId)::\(agentId)"
        guard OrbitAgentLogic.shouldDispatchMarkAgentSeen(
            lastMarkedSeq: lastMarkedSeenSeqByAgent[key],
            nextSeq: seq,
            force: force
        ) else {
            return
        }
        lastMarkedSeenSeqByAgent[key] = max(lastMarkedSeenSeqByAgent[key] ?? 0, seq)
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

    public func markAgentRead(hostId: String, agentId: String, latestSeq: UInt64 = UInt64.max / 2) {
        let resolvedSeq = detail(forHostId: hostId)?.agents.first(where: { $0.agentId == agentId })?.latestSeq ?? latestSeq
        lastMarkedSeenSeqByAgent.removeValue(forKey: "\(hostId)::\(agentId)")
        markAgentSeen(hostId: hostId, agentId: agentId, seq: max(1, resolvedSeq), refreshSidebarAfter: true)
        hapticLight()
    }

    public func isThreadUnread(hostId: String, agent: FfiAgentCard) -> Bool {
        let isSelected = selectedHostId == hostId && selectedAgentId == agent.agentId && !isDraftNewSession
        if isSelected || agent.isForegroundRunning {
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
                    if let core = self.core {
                        self.coreWriteQueue.async {
                            core.onNetworkChanged()
                        }
                    }
                }
                self.lastNetworkSignature = sig
            }
        }
        pathMonitor.start(queue: pathMonitorQueue)
    }

    public func onForeground() {
        refreshPasteboardAvailability()
        if let core {
            coreWriteQueue.async { [weak self] in
                core.onAppForeground()
                let localUri = core.discoverLocalDaemonUri()
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    if self.localDaemonUri != localUri {
                        self.localDaemonUri = localUri
                    }
                    self.reloadFleet()
                    if let hid = self.selectedHostId {
                        self.reloadHostDetail(hid)
                        if let aid = self.selectedAgentId {
                            self.reloadAgentStage(hostId: hid, agentId: aid)
                        }
                    }
                }
            }
        }
    }

    public func onBackground() {
        saveCurrentComposerDraft()
        flushPersistedComposerDraftsNow()
        if let core {
            coreWriteQueue.async {
                core.onAppBackground()
            }
        }
    }

    public func refreshPasteboardAvailability() {
        pasteboardHasStrings = UIPasteboard.general.hasStrings || UIPasteboard.general.hasURLs
    }

    public func readPairingUriFromClipboard() -> String? {
        guard let raw = UIPasteboard.general.string?.trimmingCharacters(in: .whitespacesAndNewlines),
              !raw.isEmpty else {
            return nil
        }
        if let range = raw.range(of: "orbit://(pair|sync-login)[^\\s\"']+", options: .regularExpression) {
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
        if let range = raw.range(of: "orbit://(pair|sync-login)[^\\s\"']+", options: [.regularExpression, .caseInsensitive]) {
            return String(raw[range])
        }
        let lower = raw.lowercased()
        if lower.hasPrefix("orbit://pair/") || lower.hasPrefix("orbit://sync-login") {
            return raw
        }
        return nil
    }

    public func handleDeepLink(_ url: URL) {
        let raw = url.absoluteString.trimmingCharacters(in: .whitespacesAndNewlines)
        guard raw.lowercased().hasPrefix("orbit://") else { return }
        if let parsed = OrbitPushDeepLinkLogic.parseDeepLink(raw) {
            switch parsed {
            case let .openSession(hostId, agentId):
                navigateToSessionFromDeepLink(hostId: hostId, agentId: agentId)
                return
            case let .pairHost(pairUri):
                if pairHost(uri: pairUri) {
                    if activeSheet == .qrPair {
                        activeSheet = nil
                    }
                } else {
                    showToast(lastError ?? "Invalid pairing link.")
                }
                return
            case .syncLogin:
                handleScannedOrPastedUri(raw)
                if activeSheet == .qrPair {
                    activeSheet = nil
                }
                return
            }
        }
        hapticWarning()
        let msg = "Unrecognized Orbit link."
        lastError = msg
        showToast(msg)
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

        if args.contains("-orbitDemoChat") {
            seedDemoChatShowcase()
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
            scheduleCoalescedFleetHostReload(
                fleet: true,
                hostId: selectedHostId,
                cloudSync: true
            )
            syncLiveActivityFromFleetState()

        case let .hostChanged(_, hostId):
            scheduleCoalescedFleetHostReload(
                fleet: true,
                hostId: hostId == selectedHostId ? hostId : nil,
                cloudSync: false
            )
            syncLiveActivityFromFleetState()

        case let .timelineChanged(_, hostId, agentId):
            if hostId == selectedHostId, agentId == selectedAgentId {
                scheduleCoalescedStageReload(hostId: hostId, agentId: agentId)
            }
            syncLiveActivityFromFleetState()

        case let .gitChanged(_, hostId, _):
            if hostId == selectedHostId {
                scheduleCoalescedGitAndFilesRefresh(hostId: hostId)
            }

        case let .terminalChanged(_, hostId, _):
            if hostId == selectedHostId && activeSheet == .pty {
                scheduleCoalescedTerminalRefresh(hostId: hostId)
            }
        }
    }

    private func scheduleCoalescedFleetHostReload(
        fleet: Bool,
        hostId: String?,
        cloudSync: Bool
    ) {
        if fleet { pendingFleetReloadNeeded = true }
        if let hid = hostId { pendingHostDetailRefreshIds.insert(hid) }
        if cloudSync { pendingCloudSyncRefreshNeeded = true }
        guard pendingFleetHostCoalesceWorkItem == nil else { return }
        let work = DispatchWorkItem { [weak self] in
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.pendingFleetHostCoalesceWorkItem = nil
                let needFleet = self.pendingFleetReloadNeeded
                let hostIds = self.pendingHostDetailRefreshIds
                let needCloud = self.pendingCloudSyncRefreshNeeded
                self.pendingFleetReloadNeeded = false
                self.pendingHostDetailRefreshIds.removeAll()
                self.pendingCloudSyncRefreshNeeded = false
                if needFleet {
                    self.reloadFleet()
                }
                for hid in hostIds {
                    self.reloadHostDetail(hid)
                }
                if needCloud, let core = self.core {
                    self.coreReadQueue.async { [weak self] in
                        let nextSync = core.cloudSyncStatus()
                        Task { @MainActor [weak self] in
                            guard let self else { return }
                            if self.cloudSyncStatus != nextSync {
                                self.cloudSyncStatus = nextSync
                            }
                        }
                    }
                }
            }
        }
        pendingFleetHostCoalesceWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(65), execute: work)
    }

    private func scheduleCoalescedGitAndFilesRefresh(hostId: String) {
        let gitVisible = isPadDiffInspectorVisible || activeSheet == .gitDiff
        let filesVisible = (isPadDiffInspectorVisible && padInspectorTab == .files)
            || { if case .workspaceFiles = activeSheet { return true }; return false }()
        if !gitVisible { isGitReviewStale = true }
        if !filesVisible { isWorkspaceFilesStale = true }
        guard gitVisible || filesVisible else { return }
        guard pendingGitRefreshWorkItem == nil else { return }
        let work = DispatchWorkItem { [weak self] in
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.pendingGitRefreshWorkItem = nil
                guard self.selectedHostId == hostId, let core = self.core else { return }
                let cwd = self.activeCwd
                let needGit = self.isPadDiffInspectorVisible || self.activeSheet == .gitDiff
                let needFiles = (self.isPadDiffInspectorVisible && self.padInspectorTab == .files)
                    || { if case .workspaceFiles = self.activeSheet { return true }; return false }()
                self.gitFetchEpoch &+= 1
                let epoch = self.gitFetchEpoch
                self.coreReadQueue.async { [weak self] in
                    let nextReview = needGit ? core.gitReview(hostId: hostId, cwd: cwd, path: nil) : nil
                    let nextFiles = needFiles ? core.workspaceFiles(hostId: hostId, cwd: cwd) : nil
                    Task { @MainActor [weak self] in
                        guard let self,
                              self.gitFetchEpoch == epoch,
                              self.selectedHostId == hostId,
                              self.activeCwd == cwd else { return }
                        if let nextReview {
                            self.applyGitReviewSnapshot(nextReview)
                        }
                        if let nextFiles, self.workspaceFilesList != nextFiles {
                            self.workspaceFilesList = nextFiles
                        }
                    }
                }
            }
        }
        pendingGitRefreshWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(80), execute: work)
    }

    private func scheduleCoalescedTerminalRefresh(hostId: String) {
        guard pendingTerminalRefreshWorkItem == nil else { return }
        let work = DispatchWorkItem { [weak self] in
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.pendingTerminalRefreshWorkItem = nil
                guard self.selectedHostId == hostId,
                      self.activeSheet == .pty,
                      let core = self.core else { return }
                self.terminalFetchEpoch &+= 1
                let epoch = self.terminalFetchEpoch
                self.coreReadQueue.async { [weak self] in
                    let nextScreen = core.terminalScreen(hostId: hostId)
                    let scrollback = core.terminalScrollback(hostId: hostId)
                    Task { @MainActor [weak self] in
                        guard let self,
                              self.terminalFetchEpoch == epoch,
                              self.selectedHostId == hostId else { return }
                        let tid = self.currentHostDetail?.activeTerminalId ?? self.currentHostDetail?.terminals.first?.terminalId
                        self.terminalState.setTerminalScreen(nextScreen, terminalId: tid, coreScrollback: scrollback)
                    }
                }
            }
        }
        pendingTerminalRefreshWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(50), execute: work)
    }

    // MARK: - Fleet, Host & Session Selection

    private func seedDemoChatShowcase() {
        isDemoChatMode = true
        let demoHostId = "demo-host"
        let demoAgentId = "demo-agent-ios-parity"
        let demoCwd = "~/Code/orbit"
        let demoHost = FfiHostCard(
            hostId: demoHostId,
            displayName: "Devbox Studio",
            hostname: "devbox.local",
            customName: nil,
            sshAlias: nil,
            routeMode: .auto,
            remoteDaemonPort: 7474,
            customDirectEndpoint: nil,
            viaSshTunnel: false,
            lastPingMs: 14,
            daemonVersion: appShortVersion,
            daemonProtocolVersion: 1,
            clientProtocolVersion: 1,
            osArch: "aarch64-apple-darwin",
            daemonPubkeyB64: "demoPubKeyOrbit082",
            directEndpoints: ["ws://127.0.0.1:7474"],
            relayUrl: nil,
            connectionState: .onlineDirect,
            lastError: nil,
            availableProviders: [.claudeCode, .codex],
            workspaceCount: 1,
            agentCount: 1,
            pendingApprovalCount: 0,
            supportsServerQueue: true,
            isLocal: false
        )
        let demoCard = FfiAgentCard(
            agentId: demoAgentId,
            workspaceId: "demo-ws-1",
            cwd: demoCwd,
            title: "Align iOS Chat & Sidebar with Desktop v0.8.2",
            provider: .claudeCode,
            providerLabel: "Claude Code",
            model: "claude-sonnet-4-6",
            effort: .high,
            fastMode: false,
            mode: .acceptEdits,
            statusLabel: "Working",
            isRunning: true,
            hasBackgroundTasks: false,
            hasArmedTimers: false,
            isAwaitingPermission: false,
            parentAgentId: nil,
            parentHostId: nil,
            archivedAtRfc3339: nil,
            handle: "ios-parity",
            clusterSuggestion: nil,
            worktreeBranch: "feat/ios-parity",
            taskSummary: "Verifying markdown rendering & queue reorder",
            forkedFromAgentId: nil,
            forkedAtSeq: nil,
            acpAgentId: nil,
            attentionLabel: nil,
            latestSeq: 22,
            createdAtRfc3339: nil,
            updatedAtRfc3339: "2026-11-22T06:00:00Z",
            isPinned: true,
            seenSeq: 22,
            isUnread: false,
            queuedPrompts: [
                FfiQueuedPrompt(queueId: "demo-q1", displayText: "Run XCTest suite & capture iOS screenshots", badges: [], createdAtRfc3339: "2026-11-22T06:01:00Z")
            ],
            labels: [:],
            isPersonalRouter: false
        )
        let demoDetail = FfiHostDetail(
            revision: 1,
            host: demoHost,
            discoveredProjects: [],
            directoryListing: nil,
            providerCatalogs: cachedFallbackCatalogs,
            workspaces: [
                FfiWorkspaceSummary(
                    workspaceId: "demo-ws-1",
                    title: "Orbit iOS Parity",
                    cwd: demoCwd,
                    gitBranch: "feat/ios-parity",
                    isWorktree: true,
                    missionGoal: "Ship v0.8.2 parity across stage, composer, sidebar, and sheets.",
                    projectRoot: demoCwd,
                    sourceCwd: demoCwd,
                    boardRevision: 2,
                    isCurated: true,
                    updatedAtRfc3339: "2026-11-22T06:00:00Z"
                )
            ],
            agents: [demoCard],
            archivedAgents: [],
            lastCreatedAgentId: nil,
            replacedOptimisticAgents: [:],
            quotas: [],
            terminals: [],
            activeTerminalId: nil,
            activeTerminalSlot: 1,
            schedules: [],
            providerSubagents: [:],
            gitReposByCwd: [:],
            workspaceBoards: [:],
            workspaceEvents: [:],
            workspaceOrganizeStatus: [:],
            acpAgents: [],
            artifacts: [],
            conditionalTimers: [],
            goalRuns: [],
            monitorWatches: [],
            workflowRuns: []
        )
        hosts = [demoHost]
        hostDetailsByHostId[demoHostId] = demoDetail
        currentHostDetail = demoDetail
        selectedHostId = demoHostId
        selectedAgentId = demoAgentId
        activeCwd = demoCwd
        let counts1 = FfiTurnWorkCounts(
            editedFileCount: 2, createdFileCount: 1, commandCount: 1, readFileCount: 3, searchCount: 1,
            subagentCount: 1, mcpServerCount: 0, otherToolCount: 0, thoughtCount: 1, narrationCount: 1,
            actionSteps: 4, totalSteps: 5, failedCount: 0, deniedCount: 0, unrecoveredFailureCount: 0,
            additions: 48, deletions: 12
        )
        let summary1 = FfiWorkSummaryResult(counts: counts1, label: "5 steps · 2 edited", liveLabel: "Running", liveDetail: nil, labelIncludesFailure: false)
        let replyMd = """
        All **iOS v0.8.2** fixes are wired in [OrbitIosLogic.swift](Sources/Store/OrbitIosLogic.swift#L42):

        | Layer | Status | Verification |
        | --- | --- | --- |
        | Stage & Markdown | Aligned | Unit tested |
        | Sidebar & Modals | Aligned | Zero duplicate IDs |
        """
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        let startMs = nowMs - 18_000
        let replyItem1 = FfiTimelineItem.assistantMessage(
            seq: 10, itemId: "demo-msg-1", text: replyMd, streaming: false,
            blocks: parseMarkdownPreview(markdown: replyMd, streaming: false), timestampMs: nowMs
        )
        let turn1 = FfiStageTurn(
            key: "demo-turn-1", turnIndex: 0, anchorSeq: 1,
            userPrompt: .userPrompt(seq: 1, messageId: "demo-u1", text: "Align iOS chat, markdown tables, and sidebar with Desktop v0.8.2", badges: [], blocks: [], timestampMs: startMs),
            agentMessage: nil, isCompactionCommand: false, foldedSteps: [],
            workCounts: counts1, workSummaryLabel: summary1.label, workSummaryLiveLabel: summary1.liveLabel,
            workSummaryLiveDetail: nil, workSummaryIncludesFailure: false,
            thoughtDurationMs: 2_400, workDurationMs: 6_800, turnDurationMs: 9_200,
            terminalReply: replyItem1, lastAssistantReply: replyItem1, notices: [], compactions: [],
            liveSegments: [
                .stepGroup(key: "demo-sg-1", firstSeq: 2, items: [
                    .assistantThinking(seq: 2, itemId: "demo-t1", text: "Auditing FFI bindings and iOS view lifecycles.", headline: "Auditing iOS vs Desktop parity", timestampMs: startMs + 2_000)
                ], summary: summary1, thoughtDurationMs: 2_400, workDurationMs: 6_800, startedAtMs: startMs, latestActivityMs: nowMs, hasFollowingContent: true, hasRunningStep: false),
                .assistantMessage(key: "demo-am-1", item: replyItem1)
            ],
            turnDiffs: [], hasError: false, hasUnrecoveredError: false, hasActiveItem: false,
            startedAtMs: startMs, completedAtMs: nowMs,
            latestActivityMs: nowMs, latestVisibleProgressMs: nowMs
        )
        agentStage = FfiAgentStage(
            revision: 1, hostId: demoHostId, agent: demoCard, turns: [turn1],
            pendingPermissions: [], pendingQuestions: [], sessionTurnDiffs: [],
            insights: FfiSessionInsights(touchedFiles: [], backgroundTasks: [], subagents: [], detectedLocalUrls: []),
            latestSeq: 22
        )
        agentStageHostId = demoHostId
        agentStageScopeKey = OrbitAgentLogic.stageScopeKey(hostId: demoHostId, agentId: demoAgentId)
    }

    public func reloadFleet() {
        guard let core, !isDemoChatMode else { return }
        fleetFetchEpoch &+= 1
        let epoch = fleetFetchEpoch
        coreReadQueue.async { [weak self] in
            let snap = core.fleetOverview()
            Task { @MainActor [weak self] in
                guard let self, self.fleetFetchEpoch == epoch, !self.isDemoChatMode else { return }
                if self.hosts != snap.hosts { self.hosts = snap.hosts }
                if self.projects != snap.projects { self.projects = snap.projects }
                if self.projectOrder != snap.projectOrder { self.projectOrder = snap.projectOrder }
                if self.totalPendingApprovals != snap.totalPendingApprovals {
                    self.totalPendingApprovals = snap.totalPendingApprovals
                }

                if self.selectedHostId == nil, let firstHost = snap.hosts.first {
                    self.selectHost(firstHost.hostId)
                } else if let hid = self.selectedHostId, !snap.hosts.contains(where: { $0.hostId == hid }) {
                    if let nextId = snap.hosts.first?.hostId {
                        self.selectHost(nextId)
                    } else {
                        self.selectedHostId = nil
                        self.currentHostDetail = nil
                        self.selectedAgentId = nil
                        self.agentStage = nil
                        self.agentStageHostId = nil
                        self.agentStageScopeKey = nil
                        self.activeCwd = "~"
                        self.draftWorkspaceId = nil
                        self.workspaceFilesList = []
                    }
                } else if !self.didConsumeInitialSelection, let hid = self.selectedHostId {
                    self.didConsumeInitialSelection = true
                    self.selectHost(hid)
                }
                self.refreshSidebar()
            }
        }
    }

    private func currentComposerScopeKey() -> String {
        OrbitComposerLogic.scopedDraftKey(
            hostId: selectedHostId,
            agentId: selectedAgentId,
            draftWorkspaceId: draftWorkspaceId,
            draftCwd: activeCwd
        )
    }

    private func scheduleSaveAndPersistComposerDraft() {
        saveCurrentComposerDraft()
        pendingDraftPersistWorkItem?.cancel()
        let work = DispatchWorkItem { [weak self] in
            Task { @MainActor [weak self] in
                self?.flushPersistedComposerDraftsNow()
            }
        }
        pendingDraftPersistWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(300), execute: work)
    }

    private func saveCurrentComposerDraft() {
        let key = activeComposerScopeKey.isEmpty ? currentComposerScopeKey() : activeComposerScopeKey
        let hasContent = !composerText.isEmpty
            || slashDirective != nil
            || !attachedContexts.isEmpty
            || !quotedDiffSnippets.isEmpty
            || !registeredInlineMentions.isEmpty
        if hasContent {
            composerDraftsByScope[key] = OrbitScopeComposerDraft(
                text: composerText,
                slashDirective: slashDirective,
                attachedContexts: attachedContexts,
                quotedDiffSnippets: quotedDiffSnippets,
                registeredInlineMentions: registeredInlineMentions
            )
        } else {
            composerDraftsByScope.removeValue(forKey: key)
        }
    }

    private func restoreComposerDraftForCurrentScope() {
        let nextKey = currentComposerScopeKey()
        activeComposerScopeKey = nextKey
        isRestoringDraft = true
        defer { isRestoringDraft = false }
        if let saved = composerDraftsByScope[nextKey] {
            composerText = saved.text
            slashDirective = saved.slashDirective
            attachedContexts = saved.attachedContexts
            quotedDiffSnippets = saved.quotedDiffSnippets
            registeredInlineMentions = saved.registeredInlineMentions
        } else {
            composerText = ""
            slashDirective = nil
            attachedContexts = []
            quotedDiffSnippets = []
            registeredInlineMentions = []
        }
    }

    private func flushPersistedComposerDraftsNow() {
        guard let draftURL = composerDraftFileURL else { return }
        pendingDraftPersistWorkItem?.cancel()
        pendingDraftPersistWorkItem = nil
        var encodedMap: [String: PersistedComposerDraftDTO] = [:]
        for (scopeKey, draft) in composerDraftsByScope {
            let ctxs: [PersistedComposerContextDTO] = draft.attachedContexts.compactMap { item in
                switch item {
                case let .file(path):
                    return PersistedComposerContextDTO(kind: "file", id: nil, label: path, secondary: nil, content: nil, staged: nil, lineNumber: nil, sizeBytes: nil, lineCount: nil)
                case let .gitDiff(staged, label):
                    return PersistedComposerContextDTO(kind: "gitDiff", id: nil, label: label, secondary: nil, content: nil, staged: staged, lineNumber: nil, sizeBytes: nil, lineCount: nil)
                case let .terminalOutput(label, content):
                    return PersistedComposerContextDTO(kind: "terminalOutput", id: nil, label: label, secondary: nil, content: content, staged: nil, lineNumber: nil, sizeBytes: nil, lineCount: nil)
                case let .assistantQuote(id, label, excerpt):
                    return PersistedComposerContextDTO(kind: "assistantQuote", id: id, label: label, secondary: nil, content: excerpt, staged: nil, lineNumber: nil, sizeBytes: nil, lineCount: nil)
                case let .stagedImage(id, filename, remotePath, _, sizeBytes, isUploading) where !isUploading && remotePath != nil:
                    return PersistedComposerContextDTO(kind: "stagedImage", id: id, label: filename, secondary: remotePath, content: nil, staged: nil, lineNumber: nil, sizeBytes: sizeBytes, lineCount: nil)
                case let .stagedFile(id, filename, remotePath, sizeBytes, isUploading) where !isUploading && remotePath != nil:
                    return PersistedComposerContextDTO(kind: "stagedFile", id: id, label: filename, secondary: remotePath, content: nil, staged: nil, lineNumber: nil, sizeBytes: sizeBytes, lineCount: nil)
                case let .inlineFile(id, filename, content, lineCount, sizeBytes):
                    return PersistedComposerContextDTO(kind: "inlineFile", id: id, label: filename, secondary: nil, content: content, staged: nil, lineNumber: nil, sizeBytes: sizeBytes, lineCount: lineCount)
                default:
                    return nil
                }
            }
            let quotes: [PersistedQuotedDiffDTO] = draft.quotedDiffSnippets.map { q in
                let kindTag: String = switch q.kind {
                case .added: "added"
                case .removed: "removed"
                case .context: "context"
                case .hunkHeader: "hunkHeader"
                case .meta: "meta"
                }
                return PersistedQuotedDiffDTO(id: q.id, path: q.path, lineNumber: q.lineNumber, kindTag: kindTag, content: q.content)
            }
            encodedMap[scopeKey] = PersistedComposerDraftDTO(
                text: draft.text,
                slashDirectiveRaw: draft.slashDirective?.rawValue,
                contexts: ctxs,
                quotes: quotes
            )
        }
        draftPersistQueue.async {
            guard let data = try? JSONEncoder().encode(encodedMap) else { return }
            try? data.write(to: draftURL, options: [.atomic])
        }
    }

    private func loadPersistedComposerDrafts(from url: URL) {
        guard let data = try? Data(contentsOf: url),
              let decoded = try? JSONDecoder().decode([String: PersistedComposerDraftDTO].self, from: data) else {
            return
        }
        var restored: [String: OrbitScopeComposerDraft] = [:]
        for (scopeKey, dto) in decoded {
            let contexts: [ComposerContextItem] = dto.contexts.compactMap { c in
                switch c.kind {
                case "file":
                    guard let path = c.label else { return nil }
                    return .file(path: path)
                case "gitDiff":
                    guard let label = c.label else { return nil }
                    return .gitDiff(staged: c.staged ?? false, label: label)
                case "terminalOutput":
                    return .terminalOutput(label: c.label ?? "@terminal", content: c.content ?? "")
                case "assistantQuote":
                    return .assistantQuote(id: c.id ?? UUID().uuidString, label: c.label ?? "Quote", excerpt: c.content ?? "")
                case "stagedImage":
                    return .stagedImage(id: c.id ?? UUID().uuidString, filename: c.label ?? "image.png", remotePath: c.secondary, previewData: Data(), sizeBytes: c.sizeBytes ?? 0, isUploading: false)
                case "stagedFile":
                    return .stagedFile(id: c.id ?? UUID().uuidString, filename: c.label ?? "file", remotePath: c.secondary, sizeBytes: c.sizeBytes ?? 0, isUploading: false)
                case "inlineFile":
                    return .inlineFile(id: c.id ?? UUID().uuidString, filename: c.label ?? "file", content: c.content ?? "", lineCount: c.lineCount ?? 0, sizeBytes: c.sizeBytes ?? 0)
                default:
                    return nil
                }
            }
            let quotes: [QuotedDiffSnippet] = dto.quotes.map { q in
                let kind: FfiDiffLineKind = switch q.kindTag {
                case "added", "add": .added
                case "removed", "remove": .removed
                case "hunkHeader": .hunkHeader
                case "meta": .meta
                default: .context
                }
                return QuotedDiffSnippet(id: q.id, path: q.path, lineNumber: q.lineNumber, kind: kind, content: q.content)
            }
            restored[scopeKey] = OrbitScopeComposerDraft(
                text: dto.text,
                slashDirective: dto.slashDirectiveRaw.flatMap { OrbitSlashDirective(rawValue: $0) },
                attachedContexts: contexts,
                quotedDiffSnippets: quotes,
                registeredInlineMentions: []
            )
        }
        self.composerDraftsByScope = restored
    }

    private func draftModelKey(hostId: String? = nil, for provider: FfiProviderKind) -> String {
        "\(hostId ?? selectedHostId ?? "")::\(provider)"
    }

    public func selectHost(_ hostId: String) {
        didConsumeInitialSelection = true
        let changed = selectedHostId != hostId
        if changed {
            saveCurrentComposerDraft()
            flushPersistedComposerDraftsNow()
            selectedAgentId = nil
            UserDefaults.standard.removeObject(forKey: Self.lastSelectedAgentIdDefaultsKey)
            agentStage = nil
            agentStageHostId = nil
            agentStageScopeKey = nil
            activeStageLoadError = nil
            isStageHydrating = false
            activeCwd = "~"
            activeExtraDirs = []
            draftUseWorktree = UserDefaults.standard.bool(forKey: Self.draftUseWorktreeDefaultsKey)
            draftWorkspaceId = nil
            draftWorktreeRepoPath = nil
            workspaceFilesList = []
            activeWorkspaceFilePath = nil
            activeWorkspaceFileLine = nil
            gitReview = nil
            terminalScreen = nil
            if !hasUserCustomizedDraftModel {
                hasSyncedLiveCatalogDefault = false
                selectedModel = nil
                selectedEffort = nil
            }
        }
        selectedHostId = hostId
        UserDefaults.standard.set(hostId, forKey: Self.lastSelectedHostIdDefaultsKey)
        if changed {
            currentHostDetail = hostDetailsByHostId[hostId]
            restoreComposerDraftForCurrentScope()
        }
        if let core {
            coreWriteQueue.async {
                core.connectHost(hostId: hostId)
                core.refreshHost(hostId: hostId)
                core.refreshProviderModels(hostId: hostId, forceRefresh: false)
                core.refreshQuotas(hostId: hostId)
            }
        }
        reloadHostDetail(hostId)
        refreshSidebar()
    }

    public func reloadHostDetail(_ hostId: String) {
        if isDemoChatMode && hostId == "demo-host" {
            return
        }
        guard let core else { return }
        let nextEpoch = (hostDetailFetchEpochs[hostId] ?? 0) &+ 1
        hostDetailFetchEpochs[hostId] = nextEpoch
        coreReadQueue.async { [weak self] in
            guard let detail = core.hostDetail(hostId: hostId) else { return }
            Task { @MainActor [weak self] in
                guard let self, self.hostDetailFetchEpochs[hostId] == nextEpoch else { return }
                self.applyReloadedHostDetail(hostId: hostId, detail: detail)
            }
        }
    }

    private func applyReloadedHostDetail(hostId: String, detail: FfiHostDetail) {
        if hostDetailsByHostId[hostId] != detail {
            hostDetailsByHostId[hostId] = detail
        }

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

        // Invalidate cached seen-seq whenever the host reports an agent as unread
        for agent in detail.agents where agent.isUnread {
            lastMarkedSeenSeqByAgent.removeValue(forKey: "\(hostId)::\(agent.agentId)")
        }

        // Evaluate running -> settled transitions for background sessions with queued prompts
        for agent in detail.agents where !(hostId == selectedHostId && agent.agentId == selectedAgentId) {
            let busy = agent.isForegroundRunning || agent.isAwaitingPermission
            evaluateAgentQueueDrain(hostId: hostId, agentId: agent.agentId, isBusy: busy)
        }

        guard selectedHostId == hostId else {
            refreshSidebar()
            return
        }

        if currentHostDetail != detail {
            currentHostDetail = detail
        }

        // Restore previously active session after cold restart if valid and not overridden by launch args
        if let pendingAid = pendingRestoreAgentId {
            pendingRestoreAgentId = nil
            if selectedAgentId == nil,
               detail.agents.contains(where: { $0.agentId == pendingAid && $0.archivedAtRfc3339 == nil }) {
                selectAgent(hostId: hostId, agentId: pendingAid)
                return
            }
        }

        // Advance active thread's seen cursor if currently viewing it
        if let currentAid = selectedAgentId, !isDraftNewSession,
           let ag = detail.agents.first(where: { $0.agentId == currentAid }) {
            markAgentSeen(
                hostId: hostId,
                agentId: currentAid,
                seq: ag.latestSeq,
                force: ag.isUnread,
                refreshSidebarAfter: false
            )
        }

        // Seamlessly migrate optimistic agent ID when daemon assigns permanent ID
        if let currentAid = selectedAgentId,
           let replacementAid = detail.replacedOptimisticAgents[currentAid] {
            selectedAgentId = replacementAid
            UserDefaults.standard.set(replacementAid, forKey: Self.lastSelectedAgentIdDefaultsKey)
            if let core {
                coreWriteQueue.async {
                    core.openAgentTimeline(hostId: hostId, agentId: replacementAid)
                }
            }
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

        if terminalScreen == nil, !detail.terminals.isEmpty, activeSheet == .pty, let core {
            coreReadQueue.async { [weak self] in
                let nextScreen = core.terminalScreen(hostId: hostId)
                let scrollback = core.terminalScrollback(hostId: hostId)
                Task { @MainActor [weak self] in
                    guard let self, self.selectedHostId == hostId else { return }
                    let tid = self.currentHostDetail?.activeTerminalId ?? self.currentHostDetail?.terminals.first?.terminalId
                    self.terminalState.setTerminalScreen(nextScreen, terminalId: tid, coreScrollback: scrollback)
                }
            }
        }

        if autoSelectLatestAgentOnLoad, selectedAgentId == nil,
           let firstAgent = detail.agents.first {
            autoSelectLatestAgentOnLoad = false
            selectAgent(hostId: hostId, agentId: firstAgent.agentId)
        }

        if (isPadDiffInspectorVisible || activeSheet == .gitDiff),
           (detail.host.connectionState == .onlineDirect || detail.host.connectionState == .onlineRelay),
           (gitReview == nil || !OrbitFormatters.arePathsEquivalent(gitReview?.cwd ?? "", activeCwd)) {
            refreshGitReview()
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
        selectAgent(hostId: hid, agentId: agentId)
    }

    public func selectAgent(hostId: String, agentId: String) {
        isShowingGlobalOverview = false
        didConsumeInitialSelection = true
        pendingRestoreAgentId = nil
        let hostChanged = selectedHostId != hostId
        let agentChanged = selectedAgentId != agentId || hostChanged
        if agentChanged {
            saveCurrentComposerDraft()
            flushPersistedComposerDraftsNow()
        }
        if hostChanged {
            selectedHostId = hostId
            UserDefaults.standard.set(hostId, forKey: Self.lastSelectedHostIdDefaultsKey)
            currentHostDetail = hostDetailsByHostId[hostId]
            gitReview = nil
            terminalScreen = nil
            if let core {
                coreWriteQueue.async {
                    core.connectHost(hostId: hostId)
                    core.refreshHost(hostId: hostId)
                    core.refreshProviderModels(hostId: hostId, forceRefresh: false)
                    core.refreshQuotas(hostId: hostId)
                }
            }
            reloadHostDetail(hostId)
        }
        if agentChanged {
            pendingTimelineRefreshWorkItem?.cancel()
            pendingTimelineRefreshWorkItem = nil
            isStageFetchInFlight = false
            hasPendingStageFetch = false
            stageFetchEpoch &+= 1
            stageAuxRevision = 0
            stageTurnLimit = 15
            stageWindowStartTurnIndex = nil
            stageHasOlderTurns = false
            isLoadingOlderTurns = false
            agentStage = nil
            agentStageHostId = nil
            agentStageScopeKey = nil
            activeStageLoadError = nil
            isStageHydrating = true
            draftWorkspaceId = nil
            draftWorktreeRepoPath = nil
            activeWorkspaceFilePath = nil
            activeWorkspaceFileLine = nil
        }
        selectedAgentId = agentId
        UserDefaults.standard.set(agentId, forKey: Self.lastSelectedAgentIdDefaultsKey)
        isDrawerOpen = false

        let cardSnapshot = detail(forHostId: hostId)?.agents.first(where: { $0.agentId == agentId }) ?? activeAgent
        let fetchWorkspaceId = cardSnapshot?.workspaceId
        let shouldFetchBoard: Bool = fetchWorkspaceId.flatMap { wsId in
            detail(forHostId: hostId)?.workspaces.first(where: { $0.workspaceId == wsId })
                .map { $0.isCurated || $0.boardRevision > 0 }
        } ?? false

        if let core {
            coreWriteQueue.async {
                core.openAgentTimeline(hostId: hostId, agentId: agentId)
                core.listProviderSubagents(hostId: hostId, parentAgentId: agentId)
                if let wsId = fetchWorkspaceId, !wsId.isEmpty {
                    core.fetchWorkspaceEvents(hostId: hostId, workspaceId: wsId, limit: 40)
                    if shouldFetchBoard {
                        core.fetchWorkspaceBoard(hostId: hostId, workspaceId: wsId)
                    }
                }
            }
        }
        reloadAgentStage(hostId: hostId, agentId: agentId)

        if let card = cardSnapshot {
            selectedProvider = card.provider
            selectedModel = card.model
            selectedEffort = card.effort
            fastModeEnabled = card.fastMode
            selectedMode = card.mode
            updateActiveWorkspaceCwd(card.cwd)
            markAgentSeen(
                hostId: hostId,
                agentId: agentId,
                seq: max(card.latestSeq, agentStage?.latestSeq ?? 0),
                force: true,
                refreshSidebarAfter: false
            )
        } else {
            markAgentSeen(
                hostId: hostId,
                agentId: agentId,
                seq: agentStage?.latestSeq ?? 0,
                force: true,
                refreshSidebarAfter: false
            )
        }
        if agentChanged {
            restoreComposerDraftForCurrentScope()
        }
        refreshSidebar()
        hapticLight()
    }

    private func scheduleCoalescedStageReload(hostId: String, agentId: String) {
        if isStageFetchInFlight {
            hasPendingStageFetch = true
            return
        }
        guard pendingTimelineRefreshWorkItem == nil else { return }
        let work = DispatchWorkItem { [weak self] in
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.pendingTimelineRefreshWorkItem = nil
                self.reloadAgentStage(hostId: hostId, agentId: agentId)
            }
        }
        pendingTimelineRefreshWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(50), execute: work)
    }

    public func loadOlderStageTurns() {
        guard !isLoadingOlderTurns,
              stageHasOlderTurns,
              let hostId = selectedHostId,
              let agentId = selectedAgentId,
              let core = self.core else {
            return
        }
        isLoadingOlderTurns = true
        stageTurnLimit &+= 20

        if (stageWindowStartTurnIndex ?? 0) > 0 {
            reloadAgentStage(hostId: hostId, agentId: agentId)
            return
        }

        let epoch = stageFetchEpoch
        coreReadQueue.async { [weak self] in
            let dispatched = core.loadOlderAgentTimeline(
                hostId: hostId,
                agentId: agentId,
                limitEvents: 200
            )
            if !dispatched {
                Task { @MainActor [weak self] in
                    guard let self,
                          self.stageFetchEpoch == epoch,
                          self.selectedHostId == hostId,
                          self.selectedAgentId == agentId else {
                        return
                    }
                    self.isLoadingOlderTurns = false
                }
            }
        }
    }

    public func reloadAgentStage(hostId: String, agentId: String) {
        guard hostId == selectedHostId, agentId == selectedAgentId else { return }
        if isDemoChatMode && hostId == "demo-host" {
            return
        }
        guard let core = self.core else {
            isStageHydrating = false
            isLoadingOlderTurns = false
            return
        }
        let targetScope = OrbitAgentLogic.stageScopeKey(hostId: hostId, agentId: agentId)
        let sameStage = targetScope != nil
            && agentStageScopeKey == targetScope
            && agentStage?.hostId == hostId
            && agentStageHostId == hostId
            && agentStage?.agent?.agentId == agentId
        let sinceRev: UInt64 = sameStage ? (agentStage?.revision ?? 0) : 0
        let sinceAuxRev: UInt64? = sameStage ? stageAuxRevision : nil
        let turnLimit = stageTurnLimit
        let sinceWindowStart: UInt32? = sameStage ? stageWindowStartTurnIndex : nil
        let epoch = stageFetchEpoch
        isStageFetchInFlight = true
        hasPendingStageFetch = false

        coreReadQueue.async { [weak self] in
            let fetchedDelta = core.agentStageDeltaRequest(
                req: FfiAgentStageDeltaRequest(
                    hostId: hostId,
                    agentId: agentId,
                    sinceRevision: sinceRev,
                    sinceAuxRevision: sinceAuxRev,
                    turnLimit: turnLimit,
                    sinceWindowStartTurnIndex: sinceWindowStart
                )
            )
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.isStageFetchInFlight = false
                let shouldFollowUp = self.hasPendingStageFetch
                self.hasPendingStageFetch = false
                guard self.stageFetchEpoch == epoch,
                      self.selectedHostId == hostId,
                      self.selectedAgentId == agentId else {
                    return
                }
                if shouldFollowUp {
                    self.scheduleCoalescedStageReload(hostId: hostId, agentId: agentId)
                }
                var effectiveDelta = fetchedDelta
                let currentSameStage = targetScope != nil
                    && self.agentStageScopeKey == targetScope
                    && self.agentStage?.hostId == hostId
                    && self.agentStageHostId == hostId
                    && self.agentStage?.agent?.agentId == agentId
                let existingTurnsCount = self.agentStage?.turns.count ?? 0

                if !effectiveDelta.reset && sinceRev > 0 && (!currentSameStage || Int(effectiveDelta.replaceFromTurnIndex) > existingTurnsCount) {
                    let full = core.agentStageDeltaRequest(
                        req: FfiAgentStageDeltaRequest(
                            hostId: hostId,
                            agentId: agentId,
                            sinceRevision: 0,
                            sinceAuxRevision: nil,
                            turnLimit: turnLimit,
                            sinceWindowStartTurnIndex: nil
                        )
                    )
                    effectiveDelta = full
                }

                let delta = effectiveDelta

                let stage: FfiAgentStage
                let canIncremental = !delta.reset
                    && currentSameStage
                    && self.agentStage != nil
                    && Int(delta.replaceFromTurnIndex) <= existingTurnsCount
                if canIncremental, let current = self.agentStage {
                    var mergedTurns = Array(current.turns.prefix(Int(delta.replaceFromTurnIndex)))
                    mergedTurns.append(contentsOf: delta.changedTurns)
                    let reuseAux = delta.auxUnchanged
                    stage = FfiAgentStage(
                        revision: delta.revision,
                        hostId: delta.hostId,
                        agent: delta.agent ?? current.agent,
                        turns: mergedTurns,
                        pendingPermissions: reuseAux ? current.pendingPermissions : delta.pendingPermissions,
                        pendingQuestions: reuseAux ? current.pendingQuestions : delta.pendingQuestions,
                        sessionTurnDiffs: reuseAux ? current.sessionTurnDiffs : delta.sessionTurnDiffs,
                        insights: reuseAux ? current.insights : delta.insights,
                        latestSeq: delta.latestSeq
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

                self.stageAuxRevision = delta.auxRevision
                self.stageWindowStartTurnIndex = delta.windowStartTurnIndex
                self.stageHasOlderTurns = delta.hasOlderTurns
                self.isLoadingOlderTurns = false
                self.agentStage = stage
                self.agentStageHostId = hostId
                self.agentStageScopeKey = targetScope
                self.isStageHydrating = false
                self.activeStageLoadError = nil
                if let card = stage.agent {
                    if self.selectedProvider != card.provider { self.selectedProvider = card.provider }
                    if let m = card.model, self.selectedModel != m { self.selectedModel = m }
                    if let e = card.effort, self.selectedEffort != e { self.selectedEffort = e }
                    if self.selectedMode != card.mode { self.selectedMode = card.mode }
                    if !card.cwd.isEmpty && card.cwd != self.activeCwd {
                        self.updateActiveWorkspaceCwd(card.cwd)
                    }
                    if hostId == self.selectedHostId && agentId == self.selectedAgentId && !self.isDraftNewSession {
                        self.markAgentSeen(
                            hostId: hostId,
                            agentId: agentId,
                            seq: max(card.latestSeq, stage.latestSeq),
                            force: card.isUnread,
                            refreshSidebarAfter: !card.isForegroundRunning
                        )
                    }
                } else if hostId == self.selectedHostId && agentId == self.selectedAgentId && !self.isDraftNewSession {
                    self.markAgentSeen(hostId: hostId, agentId: agentId, seq: stage.latestSeq, refreshSidebarAfter: false)
                }

                if hostId == self.selectedHostId && agentId == self.selectedAgentId {
                    let awaitingDecision = !stage.pendingPermissions.isEmpty
                        || !stage.pendingQuestions.isEmpty
                        || (stage.agent?.isAwaitingPermission ?? false)
                    let isBusy = self.isActiveAgentTurnBusy || awaitingDecision
                    self.evaluateAgentQueueDrain(hostId: hostId, agentId: agentId, isBusy: isBusy)
                }
            }
        }
    }

    public func startNewSessionDraft(
        cwd: String? = nil,
        extraDirs: [String] = [],
        provider: FfiProviderKind? = nil,
        workspaceId: String? = nil
    ) {
        isShowingGlobalOverview = false
        saveCurrentComposerDraft()
        flushPersistedComposerDraftsNow()
        let wasViewingAgent = selectedAgentId != nil
        selectedAgentId = nil
        pendingRestoreAgentId = nil
        UserDefaults.standard.removeObject(forKey: Self.lastSelectedAgentIdDefaultsKey)
        agentStage = nil
        agentStageHostId = nil
        agentStageScopeKey = nil
        isStageHydrating = false
        activeStageLoadError = nil
        activeWorkspaceFilePath = nil
        activeWorkspaceFileLine = nil
        isDrawerOpen = false
        if isPadDiffInspectorVisible {
            isPadDiffInspectorVisible = false
        }
        activeExtraDirs = extraDirs
        draftWorkspaceId = workspaceId
        draftWorktreeRepoPath = nil
        draftUseWorktree = UserDefaults.standard.bool(forKey: Self.draftUseWorktreeDefaultsKey)
        selectedMode = Self.sessionModeFromRaw(defaultPermissionModeRaw)
        if let provider = provider {
            selectedProvider = provider
            if let saved = draftSelectionsByHostProvider[draftModelKey(for: provider)] {
                applyModelSelection(provider: provider, modelId: saved.modelId, effort: saved.effort)
            } else {
                hasSyncedLiveCatalogDefault = currentHostDetail?.providerCatalogs.contains {
                    $0.provider == provider && !$0.models.isEmpty
                } ?? false
                syncDefaultModelAndEffort(for: provider)
            }
        } else if wasViewingAgent {
            if let saved = draftSelectionsByHostProvider[draftModelKey(for: selectedProvider)] {
                applyModelSelection(provider: selectedProvider, modelId: saved.modelId, effort: saved.effort)
            } else {
                hasUserCustomizedDraftModel = false
                hasSyncedLiveCatalogDefault = currentHostDetail?.providerCatalogs.contains {
                    $0.provider == selectedProvider && !$0.models.isEmpty
                } ?? false
                syncDefaultModelAndEffort(for: selectedProvider)
            }
        }
        activeComposerScopeKey = ""
        if let cwd = cwd, !cwd.isEmpty {
            updateActiveWorkspaceCwd(cwd)
        } else if let hid = selectedHostId, !activeCwd.isEmpty, activeCwd != "~", let core {
            let cwdSnapshot = activeCwd
            let dirsSnapshot = activeExtraDirs
            coreWriteQueue.async {
                core.requestGitReposInspection(hostId: hid, path: cwdSnapshot, extraDirs: dirsSnapshot)
            }
        }
        restoreComposerDraftForCurrentScope()
        refreshSidebar()
        hapticLight()
    }

    public func openTerminal(inCwd cwd: String? = nil) {
        guard let hid = selectedHostId else {
            presentSheetSafely(.qrPair)
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
        presentSheetSafely(.pty)
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
        pushCloudSyncPreferencesPatch(["draftUseWorktree": useWorktree])
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
        if changed, let hid = selectedHostId, let core {
            let staged = gitStagedMode
            let dirs = activeExtraDirs
            let loadGitNow = isPadDiffInspectorVisible || activeSheet == .gitDiff
            let loadFilesNow = (isPadDiffInspectorVisible && padInspectorTab == .files)
                || { if case .workspaceFiles = activeSheet { return true }; return false }()
            if !loadGitNow { isGitReviewStale = true }
            if !loadFilesNow { isWorkspaceFilesStale = true }
            coreReadQueue.async { [weak self] in
                core.fetchGitStatus(hostId: hid, cwd: cwd)
                core.fetchGitDiff(hostId: hid, cwd: cwd, path: nil, staged: staged)
                core.fetchWorkspaceFiles(hostId: hid, cwd: cwd)
                core.requestGitReposInspection(hostId: hid, path: cwd, extraDirs: dirs)
                let nextReview = loadGitNow ? core.gitReview(hostId: hid, cwd: cwd, path: nil) : nil
                let nextFiles = loadFilesNow ? core.workspaceFiles(hostId: hid, cwd: cwd) : nil
                Task { @MainActor [weak self] in
                    guard let self, self.selectedHostId == hid, self.activeCwd == cwd else { return }
                    if let nextReview {
                        self.applyGitReviewSnapshot(nextReview)
                    }
                    if let nextFiles, self.workspaceFilesList != nextFiles {
                        self.workspaceFilesList = nextFiles
                    }
                    self.refreshSidebar()
                }
            }
        }
    }

    public func archiveSession(_ agentId: String, hostId: String? = nil) {
        guard let hid = hostId ?? selectedHostId else { return }
        let title = detail(forHostId: hid)?.agents.first(where: { $0.agentId == agentId })?.title
            ?? activeAgent?.title
            ?? "Conversation"
        if let core {
            coreWriteQueue.async {
                core.archiveAgent(hostId: hid, agentId: agentId)
            }
        }
        if selectedAgentId == agentId && hid == selectedHostId {
            selectedAgentId = nil
            UserDefaults.standard.removeObject(forKey: Self.lastSelectedAgentIdDefaultsKey)
            agentStage = nil
            agentStageHostId = nil
            agentStageScopeKey = nil
        }
        pendingArchiveUndoDismissTask?.cancel()
        pendingArchiveUndo = OrbitPendingArchiveUndo(hostId: hid, agentId: agentId, title: title)
        pendingArchiveUndoDismissTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 4_500_000_000)
            guard !Task.isCancelled else { return }
            self?.pendingArchiveUndo = nil
        }
        reloadHostDetail(hid)
        refreshSidebar()
        hapticLight()
        showToast("Session archived")
    }

    public func undoPendingArchive() {
        guard let undo = pendingArchiveUndo else { return }
        pendingArchiveUndoDismissTask?.cancel()
        pendingArchiveUndoDismissTask = nil
        pendingArchiveUndo = nil
        if let core {
            coreWriteQueue.async { [weak self] in
                core.unarchiveAgent(hostId: undo.hostId, agentId: undo.agentId)
                Task { @MainActor [weak self] in
                    self?.reloadHostDetail(undo.hostId)
                    self?.refreshSidebar()
                }
            }
        }
        hapticSuccess()
        showToast("Restored \(undo.title)")
    }

    public func dismissArchiveUndo() {
        pendingArchiveUndoDismissTask?.cancel()
        pendingArchiveUndoDismissTask = nil
        pendingArchiveUndo = nil
    }

    public func addProjectDirectory(hostId: String? = nil, name: String? = nil, path: String) {
        let cleanPath = path.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let hid = hostId ?? selectedHostId, !cleanPath.isEmpty else { return }
        let cleanName: String = {
            let trimmed = (name ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty { return trimmed }
            let base = OrbitPathLogic.basename(cleanPath)
            return base.isEmpty ? cleanPath : base
        }()
        do {
            _ = try core?.upsertProject(
                projectId: nil,
                hostId: hid,
                name: cleanName,
                directories: [cleanPath]
            )
            reloadFleet()
            hapticSuccess()
            showToast("Added project \(cleanName)")
        } catch {
            lastError = OrbitErrorFormatter.humanize(error)
            hapticWarning()
        }
    }

    public func renameProject(projectId: String, newName: String) {
        let clean = newName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty,
              let proj = projects.first(where: { $0.projectId == projectId }) else { return }
        do {
            _ = try core?.upsertProject(
                projectId: proj.projectId,
                hostId: proj.hostId,
                name: clean,
                directories: proj.directories
            )
            reloadFleet()
            hapticLight()
            showToast("Renamed project to \(clean)")
        } catch {
            lastError = OrbitErrorFormatter.humanize(error)
            hapticWarning()
        }
    }

    public func deleteProject(_ projectId: String) {
        unpinProject(projectId)
        showToast("Removed project")
    }

    public func copyPromptText(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        UIPasteboard.general.string = trimmed
        refreshPasteboardAvailability()
        hapticLight()
        showToast("Copied prompt")
    }

    public func copyThreadTranscript(hostId: String? = nil, agentId: String? = nil) {
        guard let hid = hostId ?? selectedHostId,
              let aid = agentId ?? selectedAgentId else { return }
        let stage: FfiAgentStage? = (hid == agentStageHostId && aid == agentStage?.agent?.agentId)
            ? agentStage
            : core?.agentStage(hostId: hid, agentId: aid)
        guard let stage else { return }
        var sections: [String] = []
        for turn in stage.turns {
            if case let .userPrompt(_, _, text, _, _, _)? = turn.userPrompt {
                let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
                if !t.isEmpty {
                    sections.append("## User\n\(t)")
                }
            }
            if case let .assistantMessage(_, _, text, _, _, _)? = (turn.terminalReply ?? turn.lastAssistantReply) {
                let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
                if !t.isEmpty {
                    sections.append("## Assistant\n\(t)")
                }
            }
        }
        let body = sections.joined(separator: "\n\n---\n\n")
        guard !body.isEmpty else {
            showToast("No transcript content yet")
            return
        }
        UIPasteboard.general.string = body
        refreshPasteboardAvailability()
        hapticLight()
        showToast("Copied thread transcript")
    }

    public var allArchivedAgentsAcrossHosts: [(host: FfiHostCard, agent: FfiAgentCard)] {
        var collected: [(host: FfiHostCard, agent: FfiAgentCard)] = []
        for host in hosts {
            let detail = detail(forHostId: host.hostId) ?? core?.hostDetail(hostId: host.hostId)
            for agent in detail?.archivedAgents ?? [] {
                collected.append((host: host, agent: agent))
            }
        }
        return collected.sorted { lhs, rhs in
            let lTime = lhs.agent.archivedAtRfc3339 ?? lhs.agent.updatedAtRfc3339
            let rTime = rhs.agent.archivedAtRfc3339 ?? rhs.agent.updatedAtRfc3339
            return lTime > rTime
        }
    }

    public func emptyArchive(hostId: String? = nil) {
        let targets: [(hostId: String, agentId: String)] = allArchivedAgentsAcrossHosts.compactMap { item in
            if let hostId, item.host.hostId != hostId { return nil }
            return (item.host.hostId, item.agent.agentId)
        }
        guard !targets.isEmpty else { return }
        for target in targets {
            core?.deleteAgent(hostId: target.hostId, agentId: target.agentId)
        }
        for hid in Set(targets.map(\.hostId)) {
            reloadHostDetail(hid)
        }
        refreshSidebar()
        hapticMedium()
        showToast("Emptied \(targets.count) archived conversation\(targets.count == 1 ? "" : "s")")
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
            lastError = OrbitErrorFormatter.humanize(error)
        }
    }

    public func unpinProject(_ projectId: String) {
        do {
            try core?.removeProject(projectId: projectId)
            reloadFleet()
            hapticLight()
        } catch {
            lastError = OrbitErrorFormatter.humanize(error)
        }
    }

    public func refreshWorkspaceFiles() {
        guard let hid = selectedHostId, let core else { return }
        let cwd = activeCwd
        isWorkspaceFilesStale = false
        coreReadQueue.async { [weak self] in
            core.fetchWorkspaceFiles(hostId: hid, cwd: cwd)
            let nextFiles = core.workspaceFiles(hostId: hid, cwd: cwd)
            Task { @MainActor [weak self] in
                guard let self, self.selectedHostId == hid, self.activeCwd == cwd else { return }
                if self.workspaceFilesList != nextFiles {
                    self.workspaceFilesList = nextFiles
                }
            }
        }
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
            lastError = OrbitErrorFormatter.humanize(error)
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
        let nextCustomName = customName.map { OrbitAgentLogic.hostConfigFieldValue(edited: $0, original: targetHost?.customName) } ?? nil
        let nextDirectEndpoint = customDirectEndpoint.map { OrbitAgentLogic.hostConfigFieldValue(edited: $0, original: targetHost?.customDirectEndpoint) } ?? nil
        let nextRelayUrl = relayUrl.map { OrbitAgentLogic.hostConfigFieldValue(edited: $0, original: targetHost?.relayUrl) } ?? nil
        guard let core else { return }
        coreWriteQueue.async { [weak self] in
            do {
                _ = try core.updateHostConfig(
                    hostId: hostId,
                    customName: nextCustomName,
                    routeMode: routeMode,
                    sshAlias: targetHost?.sshAlias,
                    remoteDaemonPort: targetHost?.remoteDaemonPort,
                    customDirectEndpoint: nextDirectEndpoint,
                    relayUrl: nextRelayUrl,
                    reconnect: true
                )
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    self.lastError = nil
                    self.reloadFleet()
                    if hostId == self.selectedHostId {
                        self.reloadHostDetail(hostId)
                    }
                    self.hapticSuccess()
                    self.showToast("Updated host route settings")
                }
            } catch {
                let msg = OrbitErrorFormatter.humanize(error)
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    self.lastError = msg
                    self.hapticWarning()
                }
            }
        }
    }

    public func applyRelayToFleet(_ rawRelayUrl: String) {
        guard let core = self.core else { return }
        let trimmed = rawRelayUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let count = try await Task.detached(priority: .userInitiated) {
                    try core.applyRelayToFleet(rawRelayUrl: trimmed)
                }.value
                self.lastError = nil
                self.reloadFleet()
                self.hapticSuccess()
                self.showToast("Shared relay across \(count) host\(count == 1 ? "" : "s")")
            } catch {
                self.lastError = OrbitErrorFormatter.humanize(error)
                self.hapticWarning()
            }
        }
    }

    public func removeHost(_ hostId: String) {
        do {
            try core?.removeHost(hostId: hostId)
            reloadFleet()
            hapticMedium()
            showToast("Removed host")
        } catch {
            lastError = OrbitErrorFormatter.humanize(error)
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
        guard OrbitAgentLogic.canSwitchProvider(
            selectedAgentId: selectedAgentId,
            activeAgentExists: activeAgent != nil,
            supportsProviderSwitch: true
        ) else {
            showToast("Wait for thread to finish loading before switching provider")
            return
        }
        if isDraftNewSession, let saved = draftSelectionsByHostProvider[draftModelKey(for: provider)] {
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
        if providerChanged && !OrbitAgentLogic.canSwitchProvider(
            selectedAgentId: selectedAgentId,
            activeAgentExists: activeAgent != nil,
            supportsProviderSwitch: true
        ) {
            showToast("Wait for thread to finish loading before switching provider")
            return
        }
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
            draftSelectionsByHostProvider[draftModelKey(for: provider)] = (selectedModel, selectedEffort)
        }

        if let hid = selectedHostId, let aid = selectedAgentId {
            if providerChanged {
                core?.switchAgentProvider(
                    hostId: hid,
                    agentId: aid,
                    provider: provider,
                    model: selectedModel,
                    effort: selectedEffort,
                    acpAgentId: nil
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
        if isDraftNewSession || selectedAgentId == nil {
            if mode != .plan {
                setDefaultPermissionMode(mode)
            }
            return
        }
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
        composerUploadStates.removeValue(forKey: item.id)
        failedUploadPayloadsByContextId.removeValue(forKey: item.id)
        hapticLight()
    }

    public func removeQuotedSnippet(_ snippet: QuotedDiffSnippet) {
        quotedDiffSnippets.removeAll(where: { $0.id == snippet.id })
        hapticLight()
    }

    private static let maxAttachmentBytes = OrbitComposerLogic.maxAttachmentBytes
    private static let maxInlineTextFileBytes = 64 * 1024

    public func attachImage(filename: String, uploadData: Data, previewData: Data) {
        guard !uploadData.isEmpty else {
            showToast("Selected image is empty")
            return
        }
        guard !OrbitComposerLogic.attachmentExceedsLimit(uploadData.count, maxBytes: Self.maxAttachmentBytes) else {
            showToast("Image exceeds 25 MB limit")
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

        let mime = filename.lowercased().hasSuffix(".png") ? "image/png" : "image/jpeg"
        performRemoteStage(
            hostId: hid,
            contextId: "img:\(itemId)",
            filename: filename,
            mimeType: mime,
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
        guard !OrbitComposerLogic.attachmentExceedsLimit(data.count, maxBytes: Self.maxAttachmentBytes) else {
            showToast("\(filename) exceeds 25 MB limit")
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
        composerUploadStates[contextId] = .uploading
        failedUploadPayloadsByContextId.removeValue(forKey: contextId)
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
                self.composerUploadStates[contextId] = .uploaded
                self.failedUploadPayloadsByContextId.removeValue(forKey: contextId)
                guard let idx = self.attachedContexts.firstIndex(where: { $0.id == contextId }) else {
                    return
                }
                self.attachedContexts[idx] = buildCompletedItem(staged)
            } catch {
                let errorDetail: String
                if case let FfiOrbitError.Core(detail) = error {
                    errorDetail = detail
                } else {
                    errorDetail = OrbitErrorFormatter.humanize(error)
                }
                guard let self else { return }
                self.composerUploadStates[contextId] = .failed(errorDetail)
                self.failedUploadPayloadsByContextId[contextId] = OrbitFailedAttachmentPayload(
                    hostId: hostId,
                    contextId: contextId,
                    filename: filename,
                    mimeType: mimeType,
                    data: data,
                    completedItem: buildCompletedItem
                )
                if let idx = self.attachedContexts.firstIndex(where: { $0.id == contextId }) {
                    switch self.attachedContexts[idx] {
                    case let .stagedImage(id, fname, remotePath, previewData, sizeBytes, _):
                        self.attachedContexts[idx] = .stagedImage(
                            id: id,
                            filename: fname,
                            remotePath: remotePath,
                            previewData: previewData,
                            sizeBytes: sizeBytes,
                            isUploading: false
                        )
                    case let .stagedFile(id, fname, remotePath, sizeBytes, _):
                        self.attachedContexts[idx] = .stagedFile(
                            id: id,
                            filename: fname,
                            remotePath: remotePath,
                            sizeBytes: sizeBytes,
                            isUploading: false
                        )
                    default:
                        break
                    }
                }
                self.hapticWarning()
                self.showToast("Upload failed (tap chip to retry): \(errorDetail)")
            }
        }
    }

    public func retryAttachmentUpload(contextId: String) {
        guard let payload = failedUploadPayloadsByContextId[contextId] else { return }
        if let idx = attachedContexts.firstIndex(where: { $0.id == contextId }) {
            switch attachedContexts[idx] {
            case let .stagedImage(id, fname, remotePath, previewData, sizeBytes, _):
                attachedContexts[idx] = .stagedImage(
                    id: id,
                    filename: fname,
                    remotePath: remotePath,
                    previewData: previewData,
                    sizeBytes: sizeBytes,
                    isUploading: true
                )
            case let .stagedFile(id, fname, remotePath, sizeBytes, _):
                attachedContexts[idx] = .stagedFile(
                    id: id,
                    filename: fname,
                    remotePath: remotePath,
                    sizeBytes: sizeBytes,
                    isUploading: true
                )
            default:
                break
            }
        }
        performRemoteStage(
            hostId: payload.hostId,
            contextId: payload.contextId,
            filename: payload.filename,
            mimeType: payload.mimeType,
            data: payload.data,
            buildCompletedItem: payload.completedItem
        )
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
            guard OrbitComposerLogic.containsWholeToken(text, token: reg.token) else { continue }
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

        if (isActiveAgentTurnBusy || (activeAgent?.isAwaitingPermission == true)), selectedAgentId != nil {
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

    public func mergeQueuedPromptWithNext(_ item: QueuedPromptItem) {
        let list = activeQueuedPrompts
        guard let idx = list.firstIndex(where: { $0.id == item.id }),
              idx + 1 < list.count,
              let hid = selectedHostId,
              let aid = selectedAgentId,
              let key = activeAgentQueueKey else { return }
        let nextItem = list[idx + 1]
        let mergedText = OrbitComposerLogic.mergeQueuedPrompts(item.text, nextItem.text)
        let mergedContexts = item.attachedContexts + nextItem.attachedContexts.filter { cand in
            !item.attachedContexts.contains(where: { $0.id == cand.id })
        }
        let mergedQuotes = item.quotedDiffSnippets + nextItem.quotedDiffSnippets.filter { cand in
            !item.quotedDiffSnippets.contains(where: { $0.id == cand.id })
        }
        let combined = QueuedPromptItem(
            id: item.id,
            text: mergedText,
            slashDirective: item.slashDirective ?? nextItem.slashDirective,
            attachedContexts: mergedContexts,
            quotedDiffSnippets: mergedQuotes,
            badges: item.badges + nextItem.badges
        )
        var updated = list
        updated[idx] = combined
        updated.remove(at: idx + 1)
        queuedPromptsByAgent[key] = updated

        if currentHostDetail?.host.supportsServerQueue == true, let core = core {
            core.removeQueuedPrompt(hostId: hid, agentId: aid, queueId: nextItem.id)
            if let composed = composePromptDraft(
                hostId: hid,
                rawInput: combined.text,
                explicitDirective: combined.slashDirective,
                contexts: combined.attachedContexts,
                quotes: combined.quotedDiffSnippets
            ) {
                core.removeQueuedPrompt(hostId: hid, agentId: aid, queueId: item.id)
                _ = core.enqueuePrompt(
                    hostId: hid,
                    agentId: aid,
                    prompt: composed.wirePrompt,
                    displayPrompt: composed.displayPrompt,
                    badges: composed.badges,
                    queueId: combined.id
                )
            }
        }
        hapticLight()
        showToast("Merged queued prompts")
    }

    public func reorderQueuedPrompts(fromIndex: Int, toOffset: Int) {
        guard let hid = selectedHostId, let aid = selectedAgentId, let key = activeAgentQueueKey else { return }
        let current = activeQueuedPrompts
        let reorderedIds = OrbitComposerLogic.reorderQueueIds(current.map(\.id), fromIndex: fromIndex, toOffset: toOffset)
        var byId: [String: QueuedPromptItem] = [:]
        for item in current { byId[item.id] = item }
        queuedPromptsByAgent[key] = reorderedIds.compactMap { byId[$0] }
        if currentHostDetail?.host.supportsServerQueue == true {
            core?.reorderQueuedPrompts(hostId: hid, agentId: aid, queueIds: reorderedIds)
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
            coreWriteQueue.async {
                core.flushQueuedPromptNow(hostId: hid, agentId: aid, queueId: item.id)
            }
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

    public func flushAllQueuedPrompts() {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        let items = activeQueuedPrompts
        guard !items.isEmpty else { return }
        let key = "\(hid)::\(aid)"
        let useServerQueue = currentHostDetail?.host.supportsServerQueue == true
        queuedPromptsByAgent.removeValue(forKey: key)
        if useServerQueue, let core = core {
            coreWriteQueue.async {
                for item in items {
                    core.flushQueuedPromptNow(hostId: hid, agentId: aid, queueId: item.id)
                }
            }
        } else {
            let combinedText = items.map(\.text).filter { !$0.isEmpty }.joined(separator: "\n\n")
            let firstDir = items.compactMap(\.slashDirective).first
            var allContexts: [ComposerContextItem] = []
            for item in items {
                for ctx in item.attachedContexts where !allContexts.contains(where: { $0.id == ctx.id }) {
                    allContexts.append(ctx)
                }
            }
            var allQuotes: [QuotedDiffSnippet] = []
            for item in items {
                for q in item.quotedDiffSnippets where !allQuotes.contains(where: { $0.id == q.id }) {
                    allQuotes.append(q)
                }
            }
            lastKnownRunningByAgentKey[key] = true
            dispatchPromptInputs(
                hostId: hid,
                agentId: aid,
                rawInput: combinedText,
                explicitDirective: firstDir,
                contexts: allContexts,
                quotes: allQuotes
            )
        }
        hapticMedium()
        showToast("Flushed all queued prompts")
    }

    public func setFastMode(_ enabled: Bool) {
        fastModeEnabled = enabled
        if let hid = selectedHostId, let aid = selectedAgentId, let core {
            coreWriteQueue.async {
                core.setAgentFastMode(hostId: hid, agentId: aid, fastMode: enabled)
            }
        }
        hapticLight()
        showToast(enabled ? "Fast Mode enabled" : "Fast Mode disabled")
    }

    public func setPreferredLanguage(_ code: String) {
        preferredLanguageCode = code
        UserDefaults.standard.set(code, forKey: Self.preferredLanguageDefaultsKey)
        if code.isEmpty {
            UserDefaults.standard.removeObject(forKey: "AppleLanguages")
        } else {
            UserDefaults.standard.set([code], forKey: "AppleLanguages")
        }
        hapticLight()
    }

    public func addExtraDirectory(_ dir: String) {
        let trimmed = OrbitPathLogic.normalizePath(dir)
        guard !trimmed.isEmpty, !activeExtraDirs.contains(trimmed) else { return }
        activeExtraDirs.append(trimmed)
        requestGitReposInspection(for: activeCwd)
        hapticLight()
    }

    public func removeExtraDirectory(_ dir: String) {
        activeExtraDirs.removeAll { $0 == dir }
        hapticLight()
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
            || OrbitComposerLogic.matchesLeadingSlashCommand(rawInput, command: "/review")
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

        if composed.effectiveDirective == .plan, selectedMode != .plan {
            applySessionMode(.plan)
        }

        if composed.isCompact {
            guard let aid = agentId else {
                hapticWarning()
                showToast("Start a session first to compact context")
                return
            }
            if let core {
                coreWriteQueue.async { [weak self] in
                    core.sendPromptWithMeta(
                        hostId: hid,
                        agentId: aid,
                        prompt: composed.wirePrompt,
                        displayPrompt: composed.displayPrompt,
                        badges: composed.badges
                    )
                    Task { @MainActor [weak self] in
                        if aid == self?.selectedAgentId {
                            self?.reloadAgentStage(hostId: hid, agentId: aid)
                        }
                    }
                }
            }
            return
        }

        if let aid = agentId {
            lastKnownRunningByAgentKey["\(hid)::\(aid)"] = true
            appendOptimisticUserPromptIfNeeded(
                hostId: hid,
                agentId: aid,
                text: composed.displayPrompt,
                badges: composed.badges
            )
            if let core {
                coreWriteQueue.async { [weak self] in
                    core.sendPromptWithMeta(
                        hostId: hid,
                        agentId: aid,
                        prompt: composed.wirePrompt,
                        displayPrompt: composed.displayPrompt,
                        badges: composed.badges
                    )
                    Task { @MainActor [weak self] in
                        if aid == self?.selectedAgentId {
                            self?.reloadAgentStage(hostId: hid, agentId: aid)
                        }
                    }
                }
            }
        } else {
            let canWorktree = OrbitComposerLogic.canCreateWorktreeSession(
                isWorktreeMode: draftUseWorktree,
                isGitRepo: isActiveCwdGitRepo
            )
            if draftUseWorktree && !canWorktree {
                showToast("Git repository required for Worktree mode — using local folder")
                draftUseWorktree = false
            }
            let effectiveCwd = (canWorktree && draftUseWorktree && (draftWorktreeRepoPath?.isEmpty == false))
                ? (draftWorktreeRepoPath ?? activeCwd)
                : activeCwd
            let req = FfiCreateSessionRequest(
                hostId: hid,
                cwd: effectiveCwd,
                extraDirs: activeExtraDirs,
                provider: selectedProvider,
                acpAgentId: nil,
                model: selectedModel,
                effort: selectedEffort,
                fastMode: fastModeEnabled ? true : nil,
                mode: selectedMode,
                initialPrompt: composed.wirePrompt,
                initialDisplayPrompt: composed.displayPrompt,
                initialBadges: composed.badges,
                worktreeBranch: (canWorktree && draftUseWorktree) ? "auto" : nil
            )
            let targetWsId = draftWorkspaceId
            isStageHydrating = true
            if let core {
                coreWriteQueue.async { [weak self] in
                    let createdId = core.createSessionInWorkspace(
                        req: req,
                        workspaceId: targetWsId,
                        handle: nil,
                        autoCluster: false
                    )
                    Task { @MainActor [weak self] in
                        guard let self else { return }
                        let newId = createdId
                        if !newId.isEmpty {
                            self.draftWorkspaceId = nil
                            self.draftWorktreeRepoPath = nil
                            self.lastKnownRunningByAgentKey["\(hid)::\(newId)"] = true
                            self.selectedAgentId = newId
                            UserDefaults.standard.set(newId, forKey: Self.lastSelectedAgentIdDefaultsKey)
                            self.appendOptimisticUserPromptIfNeeded(
                                hostId: hid,
                                agentId: newId,
                                text: composed.displayPrompt,
                                badges: composed.badges
                            )
                            self.coreWriteQueue.async {
                                core.openAgentTimeline(hostId: hid, agentId: newId)
                            }
                            self.reloadHostDetail(hid)
                            self.reloadAgentStage(hostId: hid, agentId: newId)
                        } else {
                            self.isStageHydrating = false
                        }
                    }
                }
            }
        }
    }

    private func appendOptimisticUserPromptIfNeeded(
        hostId: String,
        agentId: String,
        text: String,
        badges: [FfiPromptBadge]
    ) {
        guard hostId == selectedHostId, agentId == selectedAgentId else { return }
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        let optSeq = (agentStage?.latestSeq ?? 0) + 1
        let turnIdx = UInt32(agentStage?.turns.count ?? 0)
        let promptItem = FfiTimelineItem.userPrompt(
            seq: optSeq,
            messageId: "opt-\(UUID().uuidString)",
            text: text,
            badges: badges,
            blocks: [],
            timestampMs: nowMs
        )
        let optTurn = FfiStageTurn(
            key: "opt-turn-\(optSeq)",
            turnIndex: turnIdx,
            anchorSeq: optSeq,
            userPrompt: promptItem,
            agentMessage: nil,
            isCompactionCommand: false,
            foldedSteps: [],
            workCounts: FfiTurnWorkCounts(
                editedFileCount: 0, createdFileCount: 0, commandCount: 0, readFileCount: 0,
                searchCount: 0, subagentCount: 0, mcpServerCount: 0, otherToolCount: 0,
                thoughtCount: 0, narrationCount: 0, actionSteps: 0, totalSteps: 0,
                failedCount: 0, deniedCount: 0, unrecoveredFailureCount: 0, additions: 0, deletions: 0
            ),
            workSummaryLabel: "",
            workSummaryLiveLabel: "Thinking",
            workSummaryLiveDetail: nil,
            workSummaryIncludesFailure: false,
            thoughtDurationMs: nil,
            workDurationMs: nil,
            turnDurationMs: nil,
            terminalReply: nil,
            lastAssistantReply: nil,
            notices: [],
            compactions: [],
            liveSegments: [],
            turnDiffs: [],
            hasError: false,
            hasUnrecoveredError: false,
            hasActiveItem: true,
            startedAtMs: nowMs,
            completedAtMs: nil,
            latestActivityMs: nowMs,
            latestVisibleProgressMs: nowMs
        )
        if var cur = agentStage, cur.hostId == hostId, cur.agent?.agentId == agentId {
            cur.turns.append(optTurn)
            cur.latestSeq = optSeq
            agentStage = cur
        } else {
            agentStage = FfiAgentStage(
                revision: 0,
                hostId: hostId,
                agent: activeAgent,
                turns: [optTurn],
                pendingPermissions: [],
                pendingQuestions: [],
                sessionTurnDiffs: [],
                insights: FfiSessionInsights(touchedFiles: [], backgroundTasks: [], subagents: [], detectedLocalUrls: []),
                latestSeq: optSeq
            )
        }
        agentStageHostId = hostId
        agentStageScopeKey = OrbitAgentLogic.stageScopeKey(hostId: hostId, agentId: agentId)
        isStageHydrating = false
    }

    public func interruptActiveAgent() {
        interruptAgent()
    }

    public func interruptAgent() {
        guard let hid = selectedHostId, let aid = selectedAgentId else { return }
        interruptAgent(agentId: aid, hostId: hid)
    }

    public func interruptAgent(agentId: String, hostId: String? = nil) {
        guard let hid = hostId ?? selectedHostId, let core else { return }
        coreWriteQueue.async {
            core.interruptAgent(hostId: hid, agentId: agentId)
        }
        hapticRigid()
    }

    public func previewWorktreeHandoff(
        workspaceId: String,
        sourceAgentId: String? = nil,
        direction: FfiWorktreeHandoffDirection,
        includeCommitted: Bool
    ) async throws -> FfiWorktreeHandoffPreview {
        guard let core, let hid = selectedHostId else {
            throw OrbitStorageError.applicationSupportUnavailable
        }
        let req = FfiPreviewWorktreeHandoffRequest(
            hostId: hid,
            workspaceId: workspaceId,
            sourceAgentId: sourceAgentId,
            direction: direction,
            targetWorkspaceId: nil,
            includeCommitted: includeCommitted
        )
        return try await Task.detached(priority: .userInitiated) {
            try core.previewWorktreeHandoff(req: req)
        }.value
    }

    public func executeWorktreeHandoff(
        workspaceId: String,
        sourceAgentId: String? = nil,
        direction: FfiWorktreeHandoffDirection,
        mode: FfiWorktreeHandoffMode,
        includeCommitted: Bool
    ) async throws -> FfiWorktreeHandoffOutcome {
        guard let core, let hid = selectedHostId else {
            throw OrbitStorageError.applicationSupportUnavailable
        }
        let req = FfiExecuteWorktreeHandoffRequest(
            hostId: hid,
            workspaceId: workspaceId,
            sourceAgentId: sourceAgentId,
            direction: direction,
            mode: mode,
            targetWorkspaceId: nil,
            includeCommitted: includeCommitted,
            branch: nil,
            allowThreeWayConflicts: true
        )
        let outcome = try await Task.detached(priority: .userInitiated) {
            try core.executeWorktreeHandoff(req: req)
        }.value
        hapticSuccess()
        showToast(outcome.summary.isEmpty ? "Handoff completed" : outcome.summary)
        refreshGitReview()
        return outcome
    }

    public func interruptAgentAndContinue() {
        guard let hid = selectedHostId, let aid = selectedAgentId, let core else { return }
        coreWriteQueue.async {
            core.interruptAgentAndContinue(hostId: hid, agentId: aid)
        }
        hapticRigid()
    }

    // MARK: - Tool Step Descriptor & Approvals / Questions

    public func describeTool(_ item: FfiTimelineItem) -> FfiToolStepDescriptor? {
        core?.describeToolItem(item: item, cwd: activeCwd)
    }

    public func resolvePermission(
        requestId: String,
        decision: String,
        onComplete: ((Bool, String?) -> Void)? = nil
    ) {
        if requestId.hasPrefix("demo-perm-") {
            demoPendingPermissions.removeAll { item in
                if case let .permissionRequest(_, rid, _, _, _, _, _, _) = item {
                    return rid == requestId
                }
                return false
            }
            hapticMedium()
            showToast("Decision: \(OrbitFormatters.decisionLabel(decision))")
            onComplete?(true, nil)
            return
        }
        guard let hid = selectedHostId, let aid = selectedAgentId, let core else {
            onComplete?(false, "Host or conversation is no longer active.")
            return
        }
        hapticMedium()
        coreWriteQueue.async { [weak self] in
            core.resolvePermission(
                hostId: hid,
                agentId: aid,
                permissionRequestId: requestId,
                decision: decision
            )
            Task { @MainActor [weak self] in
                guard let self else {
                    onComplete?(false, nil)
                    return
                }
                self.reloadAgentStage(hostId: hid, agentId: aid)
                // If core returned an immediate error via lastError or rule rejection, still unlock UI
                onComplete?(true, nil)
            }
        }
    }

    public func answerQuestion(
        requestId: String,
        answers: [[String]],
        onComplete: ((Bool, String?) -> Void)? = nil
    ) {
        if requestId.hasPrefix("demo-question-") {
            demoPendingQuestions.removeAll { item in
                if case let .questionRequest(_, rid, _, _, _) = item {
                    return rid == requestId
                }
                return false
            }
            hapticMedium()
            showToast("Submitted response")
            onComplete?(true, nil)
            return
        }
        guard let hid = selectedHostId, let aid = selectedAgentId, let core else {
            onComplete?(false, "Host or conversation is no longer active.")
            return
        }
        hapticMedium()
        coreWriteQueue.async { [weak self] in
            core.answerQuestion(
                hostId: hid,
                agentId: aid,
                questionRequestId: requestId,
                answers: answers
            )
            Task { @MainActor [weak self] in
                guard let self else {
                    onComplete?(false, nil)
                    return
                }
                self.reloadAgentStage(hostId: hid, agentId: aid)
                onComplete?(true, nil)
            }
        }
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
            presentSheetSafely(.gitDiff)
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
        let uniqueId = OrbitComposerLogic.nextUniqueQuoteId(
            existingIds: attachedContexts.map(\.id),
            baseTurnKey: turnKey
        )
        let item = ComposerContextItem.assistantQuote(id: uniqueId, label: chipLabel, excerpt: excerpt)
        attachedContexts.append(item)
        requestComposerFocus()
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

    private func gitDiffCacheKey(hostId: String, cwd: String) -> String {
        "\(hostId)::\(OrbitPathLogic.normalizePath(cwd))"
    }

    func applyGitReviewSnapshot(_ snapshot: FfiGitReview, stagedHint: Bool? = nil) {
        if gitReview != snapshot {
            gitReview = snapshot
        }
        let cacheKey = gitDiffCacheKey(hostId: snapshot.hostId, cwd: snapshot.cwd)
        let isStaged = stagedHint ?? gitStagedMode
        if !snapshot.diffFiles.isEmpty {
            if isStaged {
                gitStagedDiffCache[cacheKey] = snapshot.diffFiles
            } else {
                gitUnstagedDiffCache[cacheKey] = snapshot.diffFiles
            }
        } else if snapshot.rawDiff.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            // Clear only the active slot if its status list confirms zero entries in that slot
            if isStaged && !snapshot.changedFiles.contains(where: { $0.staged }) {
                gitStagedDiffCache[cacheKey] = []
            } else if !isStaged && !snapshot.changedFiles.contains(where: { !$0.staged && !$0.untracked }) {
                gitUnstagedDiffCache[cacheKey] = []
            }
        }
    }

    /// Merges staged, unstaged, and untracked changes accurately per `OrbitGitReviewScope`.
    public func effectiveGitDiffFiles(for scope: OrbitGitReviewScope) -> [FfiDiffFile] {
        guard let review = gitReview else {
            if scope != .staged, let sessionDiffs = agentStage?.sessionTurnDiffs, !sessionDiffs.isEmpty {
                return sessionDiffs
            }
            return []
        }
        let key = gitDiffCacheKey(hostId: review.hostId, cwd: review.cwd)
        let stagedFiles = gitStagedDiffCache[key] ?? (gitStagedMode ? review.diffFiles : [])
        let unstagedFiles = gitUnstagedDiffCache[key] ?? (!gitStagedMode ? review.diffFiles : [])

        func syntheticEntry(for entry: FfiGitFileEntry) -> FfiDiffFile {
            let label = entry.untracked ? "Untracked file — stage or open in Files to preview" : "Changed file (\(entry.status))"
            return FfiDiffFile(
                path: entry.path,
                additions: entry.untracked ? 1 : 0,
                deletions: 0,
                lines: [
                    FfiDiffLine(
                        kind: .hunkHeader,
                        oldLine: nil,
                        newLine: nil,
                        content: label
                    )
                ]
            )
        }

        switch scope {
        case .staged:
            var seen = Set(stagedFiles.map(\.path))
            var merged = stagedFiles
            for entry in review.changedFiles where entry.staged && !seen.contains(entry.path) {
                seen.insert(entry.path)
                merged.append(syntheticEntry(for: entry))
            }
            return merged
        case .workingTree:
            var seen = Set(unstagedFiles.map(\.path))
            var merged = unstagedFiles
            for entry in review.changedFiles where (!entry.staged || entry.untracked) && !seen.contains(entry.path) {
                seen.insert(entry.path)
                merged.append(syntheticEntry(for: entry))
            }
            return merged
        case .all:
            var mergedByPath: [String: FfiDiffFile] = [:]
            var orderedPaths: [String] = []
            for file in unstagedFiles {
                mergedByPath[file.path] = file
                orderedPaths.append(file.path)
            }
            for file in stagedFiles {
                if let existing = mergedByPath[file.path] {
                    mergedByPath[file.path] = FfiDiffFile(
                        path: file.path,
                        additions: existing.additions + file.additions,
                        deletions: existing.deletions + file.deletions,
                        lines: existing.lines + file.lines
                    )
                } else {
                    mergedByPath[file.path] = file
                    orderedPaths.append(file.path)
                }
            }
            for entry in review.changedFiles where mergedByPath[entry.path] == nil {
                mergedByPath[entry.path] = syntheticEntry(for: entry)
                orderedPaths.append(entry.path)
            }
            let out = orderedPaths.compactMap { mergedByPath[$0] }
            if out.isEmpty, let sessionDiffs = agentStage?.sessionTurnDiffs, !sessionDiffs.isEmpty {
                return sessionDiffs
            }
            return out
        }
    }

    public func pullToRefreshGitDiff() async {
        hapticLight()
        refreshGitReview()
        try? await Task.sleep(nanoseconds: 250_000_000)
        if let hid = selectedHostId, let core {
            let cwd = activeCwd
            let isStaged = gitStagedMode
            coreReadQueue.async { [weak self] in
                let nextReview = core.gitReview(hostId: hid, cwd: cwd, path: nil)
                Task { @MainActor [weak self] in
                    self?.applyGitReviewSnapshot(nextReview, stagedHint: isStaged)
                }
            }
        }
    }

    public func setGitStagedFilter(_ staged: Bool) {
        gitStagedMode = staged
        refreshGitReview()
        hapticLight()
    }

    public func refreshGitReview() {
        guard let hid = selectedHostId, let core else { return }
        isGitReviewStale = false
        let cwd = activeCwd
        let scope = gitReviewScope
        let stagedFlag = gitStagedMode
        coreReadQueue.async { [weak self] in
            core.fetchGitStatus(hostId: hid, cwd: cwd)
            if scope == .all {
                // Fetch staged diff first to populate staged cache, then unstaged so workingTree stays warm
                core.fetchGitDiff(hostId: hid, cwd: cwd, path: nil, staged: true)
                let stagedReview = core.gitReview(hostId: hid, cwd: cwd, path: nil)
                core.fetchGitDiff(hostId: hid, cwd: cwd, path: nil, staged: false)
                let unstagedReview = core.gitReview(hostId: hid, cwd: cwd, path: nil)
                Task { @MainActor [weak self] in
                    guard let self, self.selectedHostId == hid, self.activeCwd == cwd else { return }
                    self.applyGitReviewSnapshot(stagedReview, stagedHint: true)
                    self.applyGitReviewSnapshot(unstagedReview, stagedHint: false)
                }
            } else {
                core.fetchGitDiff(hostId: hid, cwd: cwd, path: nil, staged: stagedFlag)
                let nextReview = core.gitReview(hostId: hid, cwd: cwd, path: nil)
                Task { @MainActor [weak self] in
                    guard let self, self.selectedHostId == hid, self.activeCwd == cwd else { return }
                    self.applyGitReviewSnapshot(nextReview, stagedHint: stagedFlag)
                }
            }
        }
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
        guard let hid = selectedHostId, let core else { return }
        lastTerminalDimensions = (cols, rows)
        let targetCwd = activeCwd.trimmingCharacters(in: .whitespacesAndNewlines)
        let detail = currentHostDetail
        let pendingCmd = pendingLaunchPtyCommand
        let shouldDispatchLaunchCmd = !ptyLaunchCmdDispatched && pendingCmd != nil
        if shouldDispatchLaunchCmd { ptyLaunchCmdDispatched = true }

        coreWriteQueue.async { [weak self] in
            if let detail = detail, !detail.terminals.isEmpty {
                let matchedTid: String? = {
                    if !targetCwd.isEmpty && targetCwd != "~" {
                        if let activeTid = detail.activeTerminalId,
                           let activeTerm = detail.terminals.first(where: { $0.terminalId == activeTid }),
                           OrbitFormatters.arePathsEquivalent(activeTerm.cwd, targetCwd) {
                            return activeTid
                        }
                        return detail.terminals.first(where: {
                            OrbitFormatters.arePathsEquivalent($0.cwd, targetCwd)
                        })?.terminalId
                    } else {
                        return detail.activeTerminalId ?? detail.terminals.first?.terminalId
                    }
                }()
                if let tid = matchedTid {
                    core.attachTerminal(hostId: hid, terminalId: tid, slot: 1)
                    core.resizeTerminal(hostId: hid, cols: cols, rows: rows)
                } else {
                    core.startTerminal(hostId: hid, cwd: targetCwd.isEmpty ? "~" : targetCwd, cols: cols, rows: rows)
                }
            } else {
                core.startTerminal(hostId: hid, cwd: targetCwd.isEmpty ? "~" : targetCwd, cols: cols, rows: rows)
            }
            let screen = core.terminalScreen(hostId: hid)
            let scrollback = core.terminalScrollback(hostId: hid)
            Task { @MainActor [weak self] in
                guard let self, self.selectedHostId == hid else { return }
                let tid = self.currentHostDetail?.activeTerminalId ?? self.currentHostDetail?.terminals.first?.terminalId
                self.terminalState.setTerminalScreen(screen, terminalId: tid, coreScrollback: scrollback)
            }
        }

        #if DEBUG
        if let cmd = pendingCmd, shouldDispatchLaunchCmd {
            Task { @MainActor [weak self] in
                try? await Task.sleep(nanoseconds: 600_000_000)
                guard let self else { return }
                if let hid = self.selectedHostId {
                    self.reloadHostDetail(hid)
                    if let detail = self.currentHostDetail,
                       let targetTid = detail.activeTerminalId ?? detail.terminals.first?.terminalId {
                        self.coreWriteQueue.async {
                            core.attachTerminal(hostId: hid, terminalId: targetTid, slot: 1)
                        }
                    }
                }
                if (self.currentHostDetail?.terminals.isEmpty ?? true) && self.terminalScreen == nil {
                    try? await Task.sleep(nanoseconds: 500_000_000)
                    if let hid = self.selectedHostId {
                        self.reloadHostDetail(hid)
                        if let detail = self.currentHostDetail,
                           let targetTid = detail.activeTerminalId ?? detail.terminals.first?.terminalId {
                            self.coreWriteQueue.async {
                                core.attachTerminal(hostId: hid, terminalId: targetTid, slot: 1)
                            }
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
        guard let hid = selectedHostId, let core else { return }
        let dims = lastTerminalDimensions
        let cwd = activeCwd
        coreWriteQueue.async { [weak self] in
            core.startTerminal(hostId: hid, cwd: cwd, cols: dims.cols, rows: dims.rows)
            let nextScreen = core.terminalScreen(hostId: hid)
            let scrollback = core.terminalScrollback(hostId: hid)
            Task { @MainActor [weak self] in
                guard let self, self.selectedHostId == hid else { return }
                self.reloadHostDetail(hid)
                let tid = self.currentHostDetail?.activeTerminalId ?? self.currentHostDetail?.terminals.first?.terminalId
                self.terminalState.setTerminalScreen(nextScreen, terminalId: tid, coreScrollback: scrollback)
            }
        }
        hapticLight()
    }

    public func attachTerminal(_ terminalId: String) {
        guard let hid = selectedHostId, let core else { return }
        let dims = lastTerminalDimensions
        coreWriteQueue.async { [weak self] in
            core.attachTerminal(hostId: hid, terminalId: terminalId, slot: 1)
            core.resizeTerminal(hostId: hid, cols: dims.cols, rows: dims.rows)
            let nextScreen = core.terminalScreen(hostId: hid)
            let scrollback = core.terminalScrollback(hostId: hid)
            Task { @MainActor [weak self] in
                guard let self, self.selectedHostId == hid else { return }
                self.reloadHostDetail(hid)
                self.terminalState.setTerminalScreen(nextScreen, terminalId: terminalId, coreScrollback: scrollback)
            }
        }
        hapticLight()
    }

    public func killTerminal(_ terminalId: String) {
        guard let hid = selectedHostId, let core else { return }
        let activeTid = currentHostDetail?.activeTerminalId ?? currentHostDetail?.terminals.first?.terminalId
        terminalState.clearScrollback(for: terminalId)
        if activeTid == terminalId {
            terminalState.setTerminalScreen(nil, terminalId: nil)
        }
        let nextLiveId = currentHostDetail?.terminals.first(where: { $0.terminalId != terminalId })?.terminalId
        let dims = lastTerminalDimensions
        coreWriteQueue.async { [weak self] in
            core.killTerminal(hostId: hid, terminalId: terminalId)
            if let nextLiveId {
                core.attachTerminal(hostId: hid, terminalId: nextLiveId, slot: 1)
                core.resizeTerminal(hostId: hid, cols: dims.cols, rows: dims.rows)
            }
            let nextScreen = nextLiveId != nil ? core.terminalScreen(hostId: hid) : nil
            let scrollback = nextLiveId != nil ? core.terminalScrollback(hostId: hid) : []
            Task { @MainActor [weak self] in
                guard let self, self.selectedHostId == hid else { return }
                self.reloadHostDetail(hid)
                self.terminalState.setTerminalScreen(nextScreen, terminalId: nextLiveId, coreScrollback: scrollback)
            }
        }
        hapticMedium()
    }

    public func resizeTerminalIfNeeded(cols: UInt16, rows: UInt16) {
        let clampedCols = max(24, min(220, cols))
        let clampedRows = max(6, min(100, rows))
        guard clampedCols != lastTerminalDimensions.cols || clampedRows != lastTerminalDimensions.rows else {
            return
        }
        lastTerminalDimensions = (clampedCols, clampedRows)
        guard let hid = selectedHostId, let core else { return }
        coreWriteQueue.async { [weak self] in
            core.resizeTerminal(hostId: hid, cols: clampedCols, rows: clampedRows)
            let nextScreen = core.terminalScreen(hostId: hid)
            let scrollback = core.terminalScrollback(hostId: hid)
            Task { @MainActor [weak self] in
                guard let self, self.selectedHostId == hid else { return }
                let tid = self.currentHostDetail?.activeTerminalId ?? self.currentHostDetail?.terminals.first?.terminalId
                self.terminalState.setTerminalScreen(nextScreen, terminalId: tid, coreScrollback: scrollback)
            }
        }
    }

    public func sendTerminalKeys(_ keys: String) {
        guard let hid = selectedHostId, !keys.isEmpty, let core else { return }
        coreWriteQueue.async {
            core.sendTerminalKeys(hostId: hid, keys: keys)
        }
    }

    public func sendTerminalBytes(_ bytes: Data, slot: UInt8 = 1) {
        guard let hid = selectedHostId, !bytes.isEmpty, let core else { return }
        coreWriteQueue.async {
            core.sendTerminalBytes(hostId: hid, slot: slot, bytes: bytes)
        }
    }

    public func captureTerminalSnapshotText() -> String? {
        if let activeTid = currentHostDetail?.activeTerminalId ?? currentHostDetail?.terminals.first?.terminalId {
            let fullScrollback = terminalState.scrollbackLines(for: activeTid)
            if !fullScrollback.isEmpty {
                let text = fullScrollback.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
                if !text.isEmpty { return text }
            }
        }
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

    public func showToast(_ message: String, isWarning: Bool = false) {
        toastDismissTask?.cancel()
        toastIsWarning = isWarning
        withAnimation(.easeOut(duration: 0.18)) {
            toastMessage = message
        }
        toastDismissTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 2_200_000_000)
            guard !Task.isCancelled else { return }
            withAnimation(.easeIn(duration: 0.2)) {
                self?.toastMessage = nil
                self?.toastIsWarning = false
            }
        }
    }

    public func clearError() {
        lastError = nil
    }

    public func resetLocalVaultAndReinitialize() {
        core = nil
        hosts = []
        selectedHostId = nil
        selectedAgentId = nil
        agentStage = nil
        sidebarView = nil
        currentHostDetail = nil
        hostDetailsByHostId = [:]

        guard let appSupportDir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            lastError = "Application Support directory unavailable."
            return
        }
        let orbitDir = appSupportDir.appendingPathComponent("Orbit", isDirectory: true)
        let fm = FileManager.default
        if let items = try? fm.contentsOfDirectory(at: orbitDir, includingPropertiesForKeys: nil) {
            for url in items where url.lastPathComponent.hasPrefix("orbit_ios_") || url.lastPathComponent == ".orbit_master_key.bin" {
                try? fm.removeItem(at: url)
            }
        }
        do {
            let dbURL = try OrbitStorageProtection.prepareProtectedDatabaseURL(in: appSupportDir)
            let keyResult = try OrbitKeychain.shared.loadOrCreateMasterKey(fallbackDirectory: dbURL.deletingLastPathComponent())
            self.masterKeySource = keyResult.source
            let instance = try OrbitMobileCore.newWithMasterKey(dbPath: dbURL.path, masterKeyBytes: keyResult.key)
            self.core = instance
            let bridge = WeakMobileListenerBridge(owner: self)
            self.listenerBridge = bridge
            instance.subscribeThrottled(listener: bridge, intervalMs: 40)
            self.lastError = nil
            self.reloadFleet()
            self.refreshSidebar()
            self.showToast(OrbitL10n.tr("Reset Local Store"))
        } catch {
            self.lastError = OrbitErrorFormatter.humanize(error)
        }
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
        case "plan": return .plan
        case "auto", "guardian": return .auto
        case "full_access", "full": return .fullAccess
        default: return .acceptEdits
        }
    }

    public static func rawFromSessionMode(_ mode: FfiSessionMode) -> String {
        switch OrbitFormatters.normalizePermissionMode(mode) {
        case .default, .custom: return "default"
        case .plan: return "plan"
        case .acceptEdits: return "accept_edits"
        case .auto: return "auto"
        case .fullAccess: return "full_access"
        }
    }

    public func setCoalesceExploration(_ enabled: Bool) {
        coalesceExploration = enabled
        UserDefaults.standard.set(enabled, forKey: Self.coalesceExplorationDefaultsKey)
        pushCloudSyncPreferencesPatch(["coalesceExploration": enabled])
        hapticLight()
    }

    public func setDefaultPermissionMode(_ mode: FfiSessionMode) {
        let normalized = OrbitFormatters.normalizePermissionMode(mode)
        guard normalized != .plan else { return }
        let raw = Self.rawFromSessionMode(normalized)
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        defaultPermissionModeRaw = raw
        UserDefaults.standard.set(raw, forKey: Self.defaultPermissionModeDefaultsKey)
        if isDraftNewSession {
            selectedMode = normalized
        }
        core?.setAccountDefaultMode(mode: normalized, updatedAtMs: nowMs)
        pushCloudSyncPreferencesPatch(["draftMode": raw, "draftModeUpdatedAtMs": nowMs])
        hapticLight()
    }

    // MARK: - Mission Workspaces, Peers, Board, Events & Auto-Organize

    public var activeWorkspace: FfiWorkspaceSummary? {
        guard let detail = detail(forHostId: selectedHostId) else { return nil }
        if let agent = activeAgent, !agent.workspaceId.isEmpty {
            return detail.workspaces.first(where: { $0.workspaceId == agent.workspaceId })
        }
        if let draftWs = draftWorkspaceId {
            return detail.workspaces.first(where: { $0.workspaceId == draftWs })
        }
        return nil
    }

    public var activeWorkspacePeers: [FfiAgentCard] {
        guard let ws = activeWorkspace,
              let hid = selectedHostId,
              let detail = detail(forHostId: hid) else { return [] }
        return detail.agents.filter { ag in
            let isSameHostChild = ag.parentAgentId != nil && (ag.parentHostId == nil || ag.parentHostId == hid)
            return ag.workspaceId == ws.workspaceId
                && ag.archivedAtRfc3339 == nil
                && !isSameHostChild
        }
    }

    public var activePendingSubagentApprovals: [FfiAgentCard] {
        guard let hid = selectedHostId,
              let parentId = selectedAgentId,
              let detail = detail(forHostId: hid) else { return [] }
        return detail.agents.filter { child in
            let sameHostParent = (child.parentHostId == nil || child.parentHostId == hid)
            return sameHostParent
                && child.parentAgentId == parentId
                && child.isAwaitingPermission
                && child.archivedAtRfc3339 == nil
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
        let cleanGoal = missionGoal?.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalizedGoal = (cleanGoal?.isEmpty == false) ? cleanGoal : nil
        if let moveId = moveAgentId {
            let previousWorkspaceId = currentHostDetail?.agents.first(where: { $0.agentId == moveId })?.workspaceId
            core?.moveAgentToWorkspaceV2(
                hostId: hid,
                agentId: moveId,
                targetWorkspaceId: nil,
                newWorkspaceTitle: cleanTitle,
                makeUnclassified: false
            )
            if let goalToApply = normalizedGoal {
                Task { @MainActor [weak self] in
                    guard let self else { return }
                    for attempt in 0..<4 {
                        self.reloadHostDetail(hid)
                        if let createdWsId = self.currentHostDetail?.agents.first(where: {
                            $0.agentId == moveId && $0.workspaceId != previousWorkspaceId
                        })?.workspaceId ?? self.currentHostDetail?.workspaces.first(where: {
                            $0.title == cleanTitle
                        })?.workspaceId {
                            self.core?.updateWorkspaceGoal(hostId: hid, workspaceId: createdWsId, goal: goalToApply)
                            break
                        }
                        if attempt < 3 {
                            try? await Task.sleep(nanoseconds: 180_000_000)
                        }
                    }
                }
            }
            hapticSuccess()
            showToast("Created \"\(cleanTitle)\"")
            return
        }
        core?.createMissionWorkspace(
            hostId: hid,
            sourceCwd: sourceCwd,
            title: cleanTitle,
            missionGoal: normalizedGoal,
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
        let archivedWs = currentHostDetail?.workspaces.first(where: { $0.workspaceId == workspaceId })
        core?.archiveWorkspace(
            hostId: hid,
            workspaceId: workspaceId,
            keepAgentsUnclassified: keepAgentsUnclassified
        )
        if activeWorkspace?.workspaceId == workspaceId && !keepAgentsUnclassified {
            selectedAgentId = nil
            agentStage = nil
            if let wsCwd = archivedWs?.cwd, !wsCwd.isEmpty {
                let fallback = projects.first(where: {
                    $0.hostId == hid && $0.directories.contains(where: { wsCwd.hasPrefix($0) })
                })?.directories.first ?? "~"
                if activeCwd.contains("/worktrees/") || OrbitFormatters.arePathsEquivalent(activeCwd, wsCwd) {
                    activeCwd = fallback
                }
            }
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
            showToast("Moved to Unassigned")
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
            acpAgentId: nil,
            model: model,
            effort: effort,
            fastMode: nil,
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

    public func unarchiveAgent(_ agentId: String, hostId: String? = nil) {
        guard let hid = hostId ?? selectedHostId else { return }
        core?.unarchiveAgent(hostId: hid, agentId: agentId)
        if pendingArchiveUndo?.agentId == agentId && pendingArchiveUndo?.hostId == hid {
            pendingArchiveUndo = nil
        }
        reloadHostDetail(hid)
        refreshSidebar()
        hapticSuccess()
        showToast("Conversation restored")
    }

    public func deleteAgentPermanently(_ agentId: String, hostId: String? = nil) {
        guard let hid = hostId ?? selectedHostId else { return }
        core?.deleteAgent(hostId: hid, agentId: agentId)
        if selectedAgentId == agentId && selectedHostId == hid {
            selectedAgentId = nil
            agentStage = nil
            agentStageHostId = nil
            agentStageScopeKey = nil
        }
        reloadHostDetail(hid)
        refreshSidebar()
        hapticMedium()
        showToast("Conversation deleted")
    }

    public func detachAgent(_ agentId: String, hostId: String? = nil) {
        guard let hid = hostId ?? selectedHostId else { return }
        core?.detachAgent(hostId: hid, agentId: agentId)
        reloadHostDetail(hid)
        refreshSidebar()
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
        resolvedChildDecisionsByRequestId[requestId] = decision
        core?.resolvePermission(
            hostId: hid,
            agentId: childAgentId,
            permissionRequestId: requestId,
            decision: decision
        )
        reloadHostDetail(hid)
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
        for child in activeManagedSubagents where !child.isForegroundRunning && !child.isAwaitingPermission {
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
            presentSheetSafely(.workspaceBoard(workspaceId: workspaceId))
        }
    }

    public func openSessionOverview() {
        hapticLight()
        if let aid = selectedAgentId, let hid = selectedHostId, let core {
            coreReadQueue.async {
                core.listProviderSubagents(hostId: hid, parentAgentId: aid)
            }
        }
        if canShowInlineDiffInspector {
            padInspectorTab = .overview
            withAnimation(.easeOut(duration: 0.2)) {
                isPadDiffInspectorVisible = true
            }
        } else {
            presentSheetSafely(.sessionOverview)
        }
    }

    public func openWorkspaceFiles(initialPath: String? = nil, line: Int? = nil) {
        refreshWorkspaceFiles()
        activeWorkspaceFileLine = line
        if let p = initialPath, !p.isEmpty {
            let resolved = OrbitPathLogic.resolveWorkspaceFilePath(p, rootCwd: activeCwd)
            activeWorkspaceFilePath = resolved
            loadWorkspaceFile(path: resolved)
        }
        hapticLight()
        if canShowInlineDiffInspector {
            padInspectorTab = .files
            withAnimation(.easeOut(duration: 0.2)) {
                isPadDiffInspectorVisible = true
            }
        } else {
            presentSheetSafely(.workspaceFiles(initialPath: activeWorkspaceFilePath))
        }
    }

    public func setGitReviewScope(_ scope: OrbitGitReviewScope) {
        gitReviewScope = scope
        let nextStaged = (scope == .staged)
        if gitStagedMode != nextStaged {
            gitStagedMode = nextStaged
        }
        refreshGitReview()
        hapticLight()
    }

    public func presentSheetSafely(_ destination: OrbitSheetDestination) {
        if activeSheet == nil {
            activeSheet = destination
        } else if activeSheet != destination {
            pendingSheetDestination = destination
            activeSheet = nil
        }
    }

    public func openSideQuestion(prefillAndSubmit question: String? = nil) {
        hapticLight()
        if canShowInlineDiffInspector {
            padInspectorTab = .sideQuestion
            withAnimation(.easeOut(duration: 0.2)) {
                isPadDiffInspectorVisible = true
            }
        } else {
            presentSheetSafely(.sideQuestion)
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
                if self.workspaceFileCache.count >= 30,
                   !self.workspaceFileCacheOrder.isEmpty {
                    let evicted = self.workspaceFileCacheOrder.removeFirst()
                    if evicted != cacheKey {
                        self.workspaceFileCache.removeValue(forKey: evicted)
                    }
                }
                self.workspaceFileCacheOrder.removeAll(where: { $0 == cacheKey })
                self.workspaceFileCacheOrder.append(cacheKey)
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
                    self.workspaceFileError = OrbitErrorFormatter.humanize(error)
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
                    self.lastError = OrbitErrorFormatter.humanize(error)
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
                    detail = OrbitErrorFormatter.humanize(error)
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
            let trimmedFull = fullText.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmedFull.isEmpty {
                items[idx].answer = fullText
            }
            items[idx].error = error
            items[idx].isStreaming = false
            activeSideQuestionListeners.removeValue(forKey: entryId)
            if !items[idx].requestId.isEmpty {
                activeSideQuestionRequestIds.remove(items[idx].requestId)
            }
        }
        sideQuestionsByScope[scopeKey] = items
    }

    public func clearSideQuestions() {
        let key = activeSideQuestionScopeKey
        if let existing = sideQuestionsByScope.removeValue(forKey: key) {
            for entry in existing {
                activeSideQuestionListeners.removeValue(forKey: entry.id)
                if !entry.requestId.isEmpty {
                    activeSideQuestionRequestIds.remove(entry.requestId)
                }
            }
        }
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
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0.11.11"
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

    public func isVersionMismatchDismissed(for host: FfiHostCard) -> Bool {
        let key = OrbitAgentLogic.dismissedVersionKey(hostId: host.hostId, daemonVersion: host.daemonVersion)
        return dismissedVersionMismatchHosts.contains(key)
    }

    public var outdatedOnlineHosts: [FfiHostCard] {
        hosts.filter { isHostDaemonOutdated($0) || isHostProtocolBehind($0) }
    }

    public var bannerOutdatedOnlineHosts: [FfiHostCard] {
        outdatedOnlineHosts.filter { !isVersionMismatchDismissed(for: $0) }
    }

    public func dismissVersionMismatch(for hostId: String) {
        let daemonVersion = hosts.first(where: { $0.hostId == hostId })?.daemonVersion
        let key = OrbitAgentLogic.dismissedVersionKey(hostId: hostId, daemonVersion: daemonVersion)
        dismissedVersionMismatchHosts.insert(key)
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
                    detail = OrbitErrorFormatter.humanize(error)
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
        let insertionOffset = offset > 0 ? targetIdx + 1 : targetIdx
        let updatedOrder = OrbitAgentLogic.reorderProjectKeys(
            existingOrder: projectOrder,
            targetHostProjectKeys: keys,
            movedFromIndex: idx,
            movedToOffset: insertionOffset
        )
        do {
            if let updated = try core?.reorderProjects(orderedKeys: updatedOrder) {
                projectOrder = updated
            } else {
                projectOrder = updatedOrder
            }
            sidebarProjectSort = .manual
            refreshSidebar()
            hapticLight()
        } catch {
            lastError = OrbitErrorFormatter.humanize(error)
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
            acpAgentId: nil,
            model: selectedModel,
            effort: selectedEffort,
            fastMode: false,
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
        core?.interruptAgent(hostId: hid, agentId: aid)
        hapticMedium()
        showToast("Interrupting \(task.title)…")
    }

    // MARK: - Zero-Server E2EE Cloud Sync (GitHub Device Flow + Secret Gist)

    public func refreshCloudSyncStatus() {
        cloudSyncStatus = core?.cloudSyncStatus()
    }

    public func generateCloudSyncPassphrase() -> String {
        core?.cloudSyncGeneratePassphrase() ?? ""
    }

    public func startGithubCloudLogin(clientIdOverride: String? = nil) {
        guard let core = self.core else { return }
        cloudDeviceFlowTask?.cancel()
        cloudSyncBusy = true
        cloudSyncError = nil
        let cleanCid = clientIdOverride?.trimmingCharacters(in: .whitespacesAndNewlines)
        let effectiveCid = (cleanCid?.isEmpty == false) ? cleanCid : nil

        cloudDeviceFlowTask = Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let started: FfiGitHubDeviceFlowStart = try await Task.detached(priority: .userInitiated) {
                    try core.cloudSyncStartGithubDeviceFlow(clientIdOverride: effectiveCid)
                }.value
                guard !Task.isCancelled else { return }
                self.cloudSyncBusy = false
                self.cloudSyncDeviceFlow = started
                self.cloudSyncError = nil

                var intervalSecs = max(UInt64(2), started.intervalSecs)
                let deadline = Date().addingTimeInterval(TimeInterval(started.expiresInSecs))

                while !Task.isCancelled && Date() < deadline {
                    try? await Task.sleep(nanoseconds: intervalSecs * 1_000_000_000)
                    guard !Task.isCancelled else { return }
                    let polled: FfiCloudSyncDevicePoll = try await Task.detached(priority: .userInitiated) {
                        try core.cloudSyncPollGithubDeviceFlow(
                            deviceCode: started.deviceCode,
                            clientIdOverride: effectiveCid
                        )
                    }.value
                    guard !Task.isCancelled else { return }
                    switch polled.status {
                    case .pending:
                        continue
                    case .slowDown:
                        intervalSecs = max(intervalSecs + 5, polled.intervalSecs ?? 10)
                    case .authorized:
                        let nextStatus = polled.syncStatus ?? core.cloudSyncStatus()
                        self.cloudSyncDeviceFlow = nil
                        self.cloudSyncStatus = nextStatus
                        self.cloudSyncError = nil
                        self.hapticSuccess()
                        if let login = nextStatus.accountLogin {
                            self.showToast("Signed in to GitHub as @\(login)")
                        } else {
                            self.showToast("Signed in to GitHub")
                        }
                        if nextStatus.state == .ready {
                            self.syncCloudNow()
                        }
                        return
                    case .expired, .denied:
                        self.cloudSyncDeviceFlow = nil
                        self.cloudSyncError = polled.error ?? "GitHub authorization did not complete."
                        return
                    }
                }
                if !Task.isCancelled {
                    self.cloudSyncDeviceFlow = nil
                    self.cloudSyncError = "GitHub verification code expired — please try again."
                }
            } catch {
                if !Task.isCancelled {
                    self.cloudSyncBusy = false
                    self.cloudSyncDeviceFlow = nil
                    self.cloudSyncError = Self.describeCloudSyncError(error)
                }
            }
        }
    }

    public func cancelGithubCloudLogin() {
        cloudDeviceFlowTask?.cancel()
        cloudDeviceFlowTask = nil
        cloudSyncBusy = false
        cloudSyncDeviceFlow = nil
        cloudSyncError = nil
    }

    public func loginCloudSyncWithToken(_ token: String) {
        guard let core = self.core else { return }
        let trimmed = token.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            cloudSyncError = "GitHub token cannot be empty."
            return
        }
        cloudSyncBusy = true
        cloudSyncError = nil
        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let status: FfiCloudSyncStatus = try await Task.detached(priority: .userInitiated) {
                    try core.cloudSyncLoginWithToken(token: trimmed)
                }.value
                self.cloudSyncBusy = false
                self.cloudSyncStatus = status
                self.cloudSyncError = nil
                self.hapticSuccess()
                if let login = status.accountLogin {
                    self.showToast("Signed in to GitHub as @\(login)")
                }
                if status.state == .ready {
                    self.syncCloudNow()
                }
            } catch {
                self.cloudSyncBusy = false
                self.cloudSyncError = Self.describeCloudSyncError(error)
            }
        }
    }

    public func unlockCloudSyncWithPassphrase(_ passphrase: String) {
        guard let core = self.core else { return }
        let trimmed = passphrase.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            cloudSyncError = "Sync Passphrase cannot be empty."
            return
        }
        cloudSyncBusy = true
        cloudSyncError = nil
        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let result: FfiCloudSyncResult = try await Task.detached(priority: .userInitiated) {
                    try core.cloudSyncUnlockWithPassphrase(passphrase: trimmed)
                }.value
                self.cloudSyncBusy = false
                self.cloudSyncStatus = core.cloudSyncStatus()
                self.cloudSyncError = nil
                self.hydrateCloudSyncPreferences()
                self.reloadFleet()
                self.refreshSidebar()
                self.hapticSuccess()
                self.showToast(Self.formatCloudSyncResult(result))
            } catch {
                self.cloudSyncBusy = false
                self.cloudSyncStatus = core.cloudSyncStatus()
                self.cloudSyncError = Self.describeCloudSyncError(error)
            }
        }
    }

    public func syncCloudNow() {
        guard let core = self.core else { return }
        cloudSyncBusy = true
        cloudSyncError = nil
        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let result: FfiCloudSyncResult = try await Task.detached(priority: .userInitiated) {
                    try core.cloudSyncNow()
                }.value
                self.cloudSyncBusy = false
                self.cloudSyncStatus = core.cloudSyncStatus()
                self.cloudSyncError = nil
                self.hydrateCloudSyncPreferences()
                self.reloadFleet()
                self.refreshSidebar()
                self.hapticSuccess()
                self.showToast(Self.formatCloudSyncResult(result))
            } catch {
                self.cloudSyncBusy = false
                self.cloudSyncStatus = core.cloudSyncStatus()
                self.cloudSyncError = Self.describeCloudSyncError(error)
            }
        }
    }

    public func hydrateCloudSyncPreferences() {
        guard let raw = core?.cloudSyncGetPreferencesJson(),
              let data = raw.data(using: .utf8),
              let dict = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return
        }
        isApplyingCloudPrefs = true
        defer { isApplyingCloudPrefs = false }

        if let val = dict["coalesceExploration"] as? Bool {
            coalesceExploration = val
            UserDefaults.standard.set(val, forKey: Self.coalesceExplorationDefaultsKey)
        }
        if let val = dict["draftUseWorktree"] as? Bool {
            draftUseWorktree = val
            UserDefaults.standard.set(val, forKey: Self.draftUseWorktreeDefaultsKey)
        }
        if let val = dict["draftMode"] as? String, !val.isEmpty, val != "plan" {
            let mode = Self.sessionModeFromRaw(val)
            let normalizedMode = OrbitFormatters.normalizePermissionMode(mode)
            let norm = Self.rawFromSessionMode(normalizedMode)
            defaultPermissionModeRaw = norm
            UserDefaults.standard.set(norm, forKey: Self.defaultPermissionModeDefaultsKey)
            if isDraftNewSession {
                selectedMode = normalizedMode
            }
            let updatedAtMs = (dict["draftModeUpdatedAtMs"] as? NSNumber)?.int64Value ?? 0
            core?.setAccountDefaultMode(mode: normalizedMode, updatedAtMs: updatedAtMs)
        }
        if let sortRaw = dict["sidebarProjectSort"] as? String,
           let sortVal = OrbitSidebarProjectSort(rawValue: sortRaw) {
            sidebarProjectSort = sortVal
        }
        if let filterRaw = dict["sidebarThreadFilter"] as? String,
           let filterVal = OrbitSidebarThreadFilter(rawValue: filterRaw) {
            sidebarThreadFilter = filterVal
        }
    }

    public func pushCloudSyncPreferencesPatch(_ patch: [String: Any]) {
        guard !isApplyingCloudPrefs,
              let core,
              let data = try? JSONSerialization.data(withJSONObject: patch, options: []),
              let json = String(data: data, encoding: .utf8) else {
            return
        }
        try? core.cloudSyncUpdatePreferencesJson(patchJson: json)
    }

    public func setCloudAutoSync(_ enabled: Bool) {
        guard let core = self.core else { return }
        do {
            cloudSyncStatus = try core.cloudSyncSetAutoSync(enabled: enabled)
        } catch {
            cloudSyncError = Self.describeCloudSyncError(error)
        }
    }

    public func logoutCloudSync(clearLocalHosts: Bool = false) {
        guard let core = self.core else { return }
        cloudDeviceFlowTask?.cancel()
        cloudDeviceFlowTask = nil
        do {
            cloudSyncStatus = try core.cloudSyncLogout(clearLocalHosts: clearLocalHosts)
            cloudSyncDeviceFlow = nil
            cloudSyncError = nil
            if clearLocalHosts {
                reloadFleet()
                refreshSidebar()
            }
            hapticLight()
            showToast("Signed out of Cloud Sync")
        } catch {
            cloudSyncError = Self.describeCloudSyncError(error)
        }
    }

    @discardableResult
    public func handleScannedOrPastedUri(_ rawUri: String) -> Bool {
        let trimmed = rawUri.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.lowercased().hasPrefix("orbit://sync-login") {
            return importCloudSyncDeviceLink(uri: trimmed)
        }
        return pairHost(uri: trimmed)
    }

    @discardableResult
    public func importCloudSyncDeviceLink(uri: String) -> Bool {
        guard let core = self.core else { return false }
        let trimmed = uri.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.lowercased().hasPrefix("orbit://sync-login") else {
            cloudSyncError = "Invalid Orbit device link URI."
            return false
        }
        cloudSyncBusy = true
        cloudSyncError = nil
        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let result: FfiCloudSyncResult = try await Task.detached(priority: .userInitiated) {
                    try core.cloudSyncImportDeviceLinkUri(uri: trimmed)
                }.value
                self.cloudSyncBusy = false
                self.cloudSyncStatus = core.cloudSyncStatus()
                self.cloudSyncDeviceFlow = nil
                self.cloudSyncError = nil
                self.hydrateCloudSyncPreferences()
                self.reloadFleet()
                self.refreshSidebar()
                self.hapticSuccess()
                self.showToast(Self.formatCloudSyncResult(result))
            } catch {
                self.cloudSyncBusy = false
                self.cloudSyncStatus = core.cloudSyncStatus()
                self.cloudSyncError = Self.describeCloudSyncError(error)
            }
        }
        return true
    }

    public func exportCloudSyncDeviceLinkUri() -> String? {
        guard let core = self.core else { return nil }
        return try? core.cloudSyncExportDeviceLinkUri()
    }

    public func setCloudSyncManualPassphraseMode(_ requireManual: Bool) {
        guard let core = self.core else { return }
        cloudSyncBusy = true
        cloudSyncError = nil
        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let status: FfiCloudSyncStatus = try await Task.detached(priority: .userInitiated) {
                    try core.cloudSyncSetManualPassphraseMode(requireManual: requireManual)
                }.value
                self.cloudSyncBusy = false
                self.cloudSyncStatus = status
                self.cloudSyncError = nil
                self.hapticLight()
                self.showToast(
                    status.instantUnlockEnabled
                        ? "Instant zero-passphrase auto-unlock enabled"
                        : "Manual passphrase required on new devices"
                )
            } catch {
                self.cloudSyncBusy = false
                self.cloudSyncError = Self.describeCloudSyncError(error)
            }
        }
    }

    private static func describeCloudSyncError(_ error: Error) -> String {
        if case let FfiOrbitError.Core(detail) = error {
            return detail
        }
        return OrbitErrorFormatter.humanize(error)
    }

    private static func formatCloudSyncResult(_ res: FfiCloudSyncResult) -> String {
        var parts: [String] = []
        if res.hostsAdded > 0 { parts.append("+\(res.hostsAdded) host\(res.hostsAdded == 1 ? "" : "s")") }
        if res.hostsUpdated > 0 { parts.append("\(res.hostsUpdated) updated") }
        if res.hostsRemoved > 0 { parts.append("-\(res.hostsRemoved) removed") }
        if res.projectsAdded > 0 { parts.append("+\(res.projectsAdded) project\(res.projectsAdded == 1 ? "" : "s")") }
        if parts.isEmpty {
            return res.pushedRemote ? "Cloud Sync uploaded (rev #\(res.vaultRevision))" : "Cloud Sync up to date"
        }
        return "Cloud Synced: \(parts.joined(separator: ", "))"
    }
}

// MARK: - FfiAgentCard Foreground-Running Semantics (Desktop Parity)

public extension FfiAgentCard {
    /// True when the agent is idle in the foreground and only waiting on background tasks / timers.
    var isBackgroundWaiting: Bool {
        if hasBackgroundTasks { return true }
        return OrbitAgentLogic.isBackgroundWaitingStatusLabel(statusLabel)
    }

    /// True ONLY when a foreground turn or active autonomous wakeup is executing.
    var isForegroundRunning: Bool {
        isRunning && !isBackgroundWaiting
    }
}
