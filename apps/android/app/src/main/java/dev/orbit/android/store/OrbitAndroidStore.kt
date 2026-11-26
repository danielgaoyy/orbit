package dev.orbit.android.store

import android.app.Application
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.SystemClock
import android.util.LruCache
import androidx.compose.runtime.Immutable
import androidx.core.content.FileProvider
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.orbit.android.BuildConfig
import dev.orbit.android.notifications.ExternalDeepLinkDisposition
import dev.orbit.android.notifications.OrbitDeepLinkAction
import dev.orbit.android.notifications.OrbitDeepLinkSecurityPolicy
import dev.orbit.android.notifications.OrbitFirebaseMessagingBridge
import dev.orbit.android.notifications.OrbitNotificationHelper
import dev.orbit.android.notifications.OrbitPushRouter
import dev.orbit.android.notifications.PendingDeepLinkConfirmation
import dev.orbit.android.notifications.PendingDeepLinkKind
import dev.orbit.android.ui.sheets.SideQuestionEntry
import dev.orbit.android.ui.theme.OrbitAppearanceMode
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import uniffi.orbit_ffi_uniffi.ConnectedHostSummary
import uniffi.orbit_ffi_uniffi.ConversationSearchHit
import uniffi.orbit_ffi_uniffi.DagStepSpec
import uniffi.orbit_ffi_uniffi.FfiAgentCard
import uniffi.orbit_ffi_uniffi.FfiAgentStage
import uniffi.orbit_ffi_uniffi.FfiAgentStageDelta
import uniffi.orbit_ffi_uniffi.FfiAskSideQuestionRequest
import uniffi.orbit_ffi_uniffi.FfiClientPlatform
import uniffi.orbit_ffi_uniffi.FfiClientProject
import uniffi.orbit_ffi_uniffi.FfiCloudSyncDevicePollStatus
import uniffi.orbit_ffi_uniffi.FfiCloudSyncStatus
import uniffi.orbit_ffi_uniffi.FfiComposePromptRequest
import uniffi.orbit_ffi_uniffi.FfiComposedPrompt
import uniffi.orbit_ffi_uniffi.FfiComposerContextInput
import uniffi.orbit_ffi_uniffi.FfiComposerPasteInput
import uniffi.orbit_ffi_uniffi.FfiComposerQuoteInput
import uniffi.orbit_ffi_uniffi.FfiConnectionState
import uniffi.orbit_ffi_uniffi.FfiCoreEvent
import uniffi.orbit_ffi_uniffi.FfiCreateScheduleRequest
import uniffi.orbit_ffi_uniffi.FfiCreateSessionRequest
import uniffi.orbit_ffi_uniffi.FfiDiagramKind
import uniffi.orbit_ffi_uniffi.FfiDiagramTheme
import uniffi.orbit_ffi_uniffi.FfiExportFormat
import uniffi.orbit_ffi_uniffi.FfiExportOptions
import uniffi.orbit_ffi_uniffi.FfiFastModeSlashAction
import uniffi.orbit_ffi_uniffi.FfiFleetSidebar
import uniffi.orbit_ffi_uniffi.FfiFleetSidebarRequest
import uniffi.orbit_ffi_uniffi.FfiForkAgentRequest
import uniffi.orbit_ffi_uniffi.FfiGitHubDeviceFlowStart
import uniffi.orbit_ffi_uniffi.FfiGitReview
import uniffi.orbit_ffi_uniffi.FfiGoalControlAction
import uniffi.orbit_ffi_uniffi.FfiHostCard
import uniffi.orbit_ffi_uniffi.FfiHostDetail
import uniffi.orbit_ffi_uniffi.FfiHostRouteMode
import uniffi.orbit_ffi_uniffi.FfiLaunchpadProject
import uniffi.orbit_ffi_uniffi.FfiMentionCandidate
import uniffi.orbit_ffi_uniffi.FfiMentionRankRequest
import uniffi.orbit_ffi_uniffi.FfiModelSelectionRequest
import uniffi.orbit_ffi_uniffi.FfiOrbitException
import uniffi.orbit_ffi_uniffi.FfiPermissionRequestId
import uniffi.orbit_ffi_uniffi.FfiPromptBadge
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiProviderModelCatalog
import uniffi.orbit_ffi_uniffi.FfiProviderModelSpec
import uniffi.orbit_ffi_uniffi.FfiPushDevicesSnapshot
import uniffi.orbit_ffi_uniffi.FfiPushPlatform
import uniffi.orbit_ffi_uniffi.FfiPushRegistrationStatus
import uniffi.orbit_ffi_uniffi.FfiQuestionItem
import uniffi.orbit_ffi_uniffi.FfiReasoningEffort
import uniffi.orbit_ffi_uniffi.FfiRegisterPushDeviceRequest
import uniffi.orbit_ffi_uniffi.FfiRenderDiagramRequest
import uniffi.orbit_ffi_uniffi.FfiRenderDiagramView
import uniffi.orbit_ffi_uniffi.FfiResolvedModelSelection
import uniffi.orbit_ffi_uniffi.FfiScheduleRunRecord
import uniffi.orbit_ffi_uniffi.FfiSessionMode
import uniffi.orbit_ffi_uniffi.FfiSideQuestionEvent
import uniffi.orbit_ffi_uniffi.FfiSidebarProjectSort
import uniffi.orbit_ffi_uniffi.FfiSidebarThreadFilter
import uniffi.orbit_ffi_uniffi.FfiSlashCommand
import uniffi.orbit_ffi_uniffi.FfiSlashDirective
import uniffi.orbit_ffi_uniffi.FfiSpawnWorkspacePeerRequest
import uniffi.orbit_ffi_uniffi.FfiTerminalScreen
import uniffi.orbit_ffi_uniffi.FfiThreadPrefSeed
import uniffi.orbit_ffi_uniffi.FfiTimelineItem
import uniffi.orbit_ffi_uniffi.FfiToolCallDetail
import uniffi.orbit_ffi_uniffi.FfiToolStepDescriptor
import uniffi.orbit_ffi_uniffi.FfiWorkspaceFile
import uniffi.orbit_ffi_uniffi.GitConflictFile
import uniffi.orbit_ffi_uniffi.GitConflictResolution
import uniffi.orbit_ffi_uniffi.GitPrStatus
import uniffi.orbit_ffi_uniffi.GitStashEntry
import uniffi.orbit_ffi_uniffi.GuardianAuditEntry
import uniffi.orbit_ffi_uniffi.GuardianConfigSnapshotView
import uniffi.orbit_ffi_uniffi.GuardianPolicyConfig
import uniffi.orbit_ffi_uniffi.GuardianRiskLevel
import uniffi.orbit_ffi_uniffi.MobileCoreListener
import uniffi.orbit_ffi_uniffi.MonitorKind
import uniffi.orbit_ffi_uniffi.OrbitMobileCore
import uniffi.orbit_ffi_uniffi.PermissionRuleRecord
import uniffi.orbit_ffi_uniffi.PersonalRouterConfig
import uniffi.orbit_ffi_uniffi.PlanApprovalAction
import uniffi.orbit_ffi_uniffi.PushPreferences
import uniffi.orbit_ffi_uniffi.RouterHostCapabilities
import uniffi.orbit_ffi_uniffi.RouterStatusSnapshot
import uniffi.orbit_ffi_uniffi.SearchMessagesRequest
import uniffi.orbit_ffi_uniffi.SideQuestionListener
import uniffi.orbit_ffi_uniffi.StartDagWorkflowRequest
import uniffi.orbit_ffi_uniffi.StartGoalRunRequest
import uniffi.orbit_ffi_uniffi.StartMonitorWatchRequest
import uniffi.orbit_ffi_uniffi.TimerCondition
import uniffi.orbit_ffi_uniffi.UpdatePushPreferencesRequest
import uniffi.orbit_ffi_uniffi.UpsertPermissionRuleInput
import uniffi.orbit_ffi_uniffi.defaultEffortDescription
import uniffi.orbit_ffi_uniffi.fallbackProviderCatalog
import uniffi.orbit_ffi_uniffi.formatCatalogModelLabel
import uniffi.orbit_ffi_uniffi.formatEffortLabel
import uniffi.orbit_ffi_uniffi.resolveSupportedEffortForModel
import java.io.File
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

@Immutable
data class QuotedDiffSnippet(
    val id: String = UUID.randomUUID().toString(),
    val path: String,
    val lineLabel: String,
    val content: String,
    val isAssistantExcerpt: Boolean = false,
) {
    val badgeLabel: String
        get() = when {
            isAssistantExcerpt -> lineLabel.ifBlank { "Quoted reply" }
            lineLabel.isBlank() -> OrbitFormatters.shortPath(path)
            else -> "${OrbitFormatters.shortPath(path)}:$lineLabel"
        }
}

@Immutable
data class StagedAttachmentItem(
    val id: String = UUID.randomUUID().toString(),
    val path: String,
    val filename: String,
    val sizeBytes: Long,
    val isImage: Boolean,
    val previewBytes: ByteArray? = null,
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is StagedAttachmentItem) return false
        return id == other.id &&
            path == other.path &&
            filename == other.filename &&
            sizeBytes == other.sizeBytes &&
            isImage == other.isImage
    }

    override fun hashCode(): Int {
        var result = id.hashCode()
        result = 31 * result + path.hashCode()
        result = 31 * result + filename.hashCode()
        result = 31 * result + sizeBytes.hashCode()
        result = 31 * result + isImage.hashCode()
        return result
    }
}

@Immutable
data class InlineMentionRegistration(
    val token: String,
    val kind: String,
    val path: String? = null,
    val staged: Boolean = false,
    val label: String = "",
)

@Immutable
data class QueuedPromptItem(
    val id: String = UUID.randomUUID().toString(),
    val text: String,
    val previewText: String,
    val mentionedFiles: List<String> = emptyList(),
    val stagedAttachments: List<StagedAttachmentItem> = emptyList(),
    val inlineMentions: List<InlineMentionRegistration> = emptyList(),
    val includeGitDiff: Boolean = false,
    val includeStagedDiff: Boolean = false,
    val includeTerminal: Boolean = false,
    val activeSlashCommand: String? = null,
    val quotedSnippets: List<QuotedDiffSnippet> = emptyList(),
)

@Immutable
data class ComposerDraftState(
    val text: String = "",
    val provider: FfiProviderKind = FfiProviderKind.CLAUDE_CODE,
    val model: String? = null,
    val effort: FfiReasoningEffort? = null,
    val fastMode: Boolean = false,
    val mode: FfiSessionMode = FfiSessionMode.AcceptEdits,
    val mentionedFiles: List<String> = emptyList(),
    val stagedAttachments: List<StagedAttachmentItem> = emptyList(),
    val inlineMentions: List<InlineMentionRegistration> = emptyList(),
    val isStagingAttachment: Boolean = false,
    val includeGitDiff: Boolean = false,
    val includeStagedDiff: Boolean = false,
    val includeTerminal: Boolean = false,
    val activeSlashCommand: String? = null,
    val quotedSnippets: List<QuotedDiffSnippet> = emptyList(),
    val extraDirs: List<String> = emptyList(),
    val worktreeBranch: String? = null,
) {
    val hasContextAttachments: Boolean
        get() = mentionedFiles.isNotEmpty() ||
            stagedAttachments.isNotEmpty() ||
            includeGitDiff ||
            includeStagedDiff ||
            includeTerminal ||
            activeSlashCommand != null ||
            quotedSnippets.isNotEmpty()

    val canSend: Boolean
        get() = text.isNotBlank() || hasContextAttachments
}

enum class OrbitSidebarProjectSort(val label: String) {
    RECENT("Recent"),
    NAME("Name"),
    MANUAL("Manual"),
    ;

    fun toFfi(): FfiSidebarProjectSort = when (this) {
        RECENT -> FfiSidebarProjectSort.RECENT
        NAME -> FfiSidebarProjectSort.ALPHABETICAL
        MANUAL -> FfiSidebarProjectSort.MANUAL
    }
}

enum class OrbitSidebarThreadFilter(val label: String) {
    ALL("All Conversations"),
    ACTIVE_AND_UNREAD("Active & Unread"),
    ;

    fun toFfi(): FfiSidebarThreadFilter = when (this) {
        ALL -> FfiSidebarThreadFilter.ALL
        ACTIVE_AND_UNREAD -> FfiSidebarThreadFilter.ACTIVE_UNREAD
    }
}

@Immutable
data class OverviewPendingDecision(
    val hostId: String,
    val hostName: String,
    val agentId: String,
    val agentTitle: String,
    val permission: FfiTimelineItem.PermissionRequest,
    val isSafeLowRisk: Boolean,
    val isPlanApproval: Boolean,
    val riskLevel: GuardianRiskLevel? = null,
)

@Immutable
data class OrbitUiState(
    val appearanceMode: OrbitAppearanceMode = OrbitAppearanceMode.SYSTEM,
    val isPermanentDrawerCollapsed: Boolean = false,
    val hosts: List<FfiHostCard> = emptyList(),
    val projects: List<FfiClientProject> = emptyList(),
    val totalPendingApprovals: UInt = 0u,
    val selectedHostId: String? = null,
    val hostDetail: FfiHostDetail? = null,
    val selectedAgentId: String? = null,
    val isNewSessionMode: Boolean = false,
    val draftUseWorktree: Boolean = false,
    val agentStage: FfiAgentStage? = null,
    val sidebarView: FfiFleetSidebar? = null,
    val slashCommands: List<FfiSlashCommand> = emptyList(),
    val activeCwd: String = "~",
    val workspaceFiles: List<String> = emptyList(),
    val gitReview: FfiGitReview? = null,
    val gitStagedMode: Boolean = false,
    val gitSelectedPath: String? = null,
    val isDiffInspectorOpen: Boolean = false,
    val isRefreshingStage: Boolean = false,
    val isRefreshingGitDiff: Boolean = false,
    val terminalScreen: FfiTerminalScreen? = null,
    val terminalCols: UShort = 84u,
    val terminalRows: UShort = 28u,
    val composer: ComposerDraftState = ComposerDraftState(),
    val queuedPromptsByAgent: Map<String, List<QueuedPromptItem>> = emptyMap(),
    val drawerSearchQuery: String = "",
    val sidebarProjectSort: OrbitSidebarProjectSort = OrbitSidebarProjectSort.RECENT,
    val sidebarThreadFilter: OrbitSidebarThreadFilter = OrbitSidebarThreadFilter.ALL,
    val collapsedProjectIds: Set<String> = emptySet(),
    val harnessRequestId: Long = 0L,
    val harnessSheetTarget: String? = null,
    val pendingPtyCommand: String? = null,
    val demoPermissions: List<FfiTimelineItem> = emptyList(),
    val demoQuestions: List<FfiTimelineItem> = emptyList(),
    val localDaemonUri: String? = null,
    val scheduleRunsById: Map<String, List<FfiScheduleRunRecord>> = emptyMap(),
    val sideQuestionsByScope: Map<String, List<SideQuestionEntry>> = emptyMap(),
    val sideQuestionDraftsByScope: Map<String, String> = emptyMap(),
    val cloudSyncStatus: FfiCloudSyncStatus? = null,
    val cloudSyncDeviceFlow: FfiGitHubDeviceFlowStart? = null,
    val cloudSyncBusy: Boolean = false,
    val cloudSyncError: String? = null,
    val pushRegistrationStatus: FfiPushRegistrationStatus? = null,
    val pushDevicesSnapshot: FfiPushDevicesSnapshot? = null,
    val showGlobalOverview: Boolean = true,
    val fleetSessions: List<Pair<String, FfiAgentCard>> = emptyList(),
    val overviewPendingDecisions: List<OverviewPendingDecision> = emptyList(),
    val stageHasOlderTurns: Boolean = false,
    val stageOldestLoadedSeq: ULong = 0uL,
    val stageAuxRevision: ULong = 0uL,
    val isLoadingOlderTurns: Boolean = false,
    val gitPrStatus: GitPrStatus? = null,
    val gitConflicts: List<GitConflictFile> = emptyList(),
    val gitStashes: List<GitStashEntry> = emptyList(),
    val gitMutationBusy: Boolean = false,
    val generatedCommitMessage: String? = null,
    val routerHosts: List<RouterHostCapabilities> = emptyList(),
    val routerStatus: RouterStatusSnapshot? = null,
    val fleetMeshSnapshot: Map<String, List<ConnectedHostSummary>> = emptyMap(),
    val isLoadingRouterMesh: Boolean = false,
    val globalSearchQuery: String = "",
    val globalSearchHits: List<ConversationSearchHit> = emptyList(),
    val globalSearchRemoteQueried: UInt = 0u,
    val globalSearchRemoteFailed: UInt = 0u,
    val isSearchingGlobal: Boolean = false,
    val permissionRules: List<PermissionRuleRecord> = emptyList(),
    val guardianSnapshot: GuardianConfigSnapshotView? = null,
    val pendingSheetTriggerId: Long = 0L,
    val pendingSheetTrigger: String? = null,
    val pendingDeepLinkConfirmation: PendingDeepLinkConfirmation? = null,
    val lastError: String? = null,
    val toastMessage: String? = null,
) {
    val activeWorkspaceId: String?
        get() = activeAgent?.workspaceId?.takeIf { it.isNotBlank() }
            ?: hostDetail?.workspaces?.firstOrNull {
                OrbitFormatters.pathsEquivalent(it.cwd, activeCwd)
            }?.workspaceId
            ?: hostDetail?.workspaces?.firstOrNull()?.workspaceId

    val activeSideQuestionScopeKey: String
        get() {
            val hid = selectedHostId ?: "none"
            val aid = selectedAgentId?.takeIf { !isNewSessionMode } ?: "draft:$activeCwd"
            return "$hid::$aid"
        }

    val activeSideQuestions: List<SideQuestionEntry>
        get() = sideQuestionsByScope[activeSideQuestionScopeKey].orEmpty()

    val activeSideQuestionDraft: String
        get() = sideQuestionDraftsByScope[activeSideQuestionScopeKey].orEmpty()
    val selectedHost: FfiHostCard?
        get() = hostDetail?.host?.takeIf { it.hostId == selectedHostId } ?: hosts.firstOrNull { it.hostId == selectedHostId }

    val activeAgent: FfiAgentCard?
        get() = if (selectedAgentId == null) {
            null
        } else {
            // Prefer freshly updated `HostChanged` host-detail cards over `agentStage?.agent`
            // so `Running -> Idle / waiting` transitions take effect immediately.
            hostDetail?.agents?.firstOrNull { it.agentId == selectedAgentId }
                ?: agentStage?.agent
        }

    val activeQueuedPrompts: List<QueuedPromptItem>
        get() {
            val hId = selectedHostId ?: return emptyList()
            val aId = selectedAgentId?.takeIf { !isNewSessionMode } ?: return emptyList()
            val key = "$hId::$aId"
            val localList = queuedPromptsByAgent[key].orEmpty()
            if (selectedHost?.supportsServerQueue == true) {
                // Prefer fresh `hostDetail.agents` over `agentStage.agent` because queue mutations
                // broadcast `HostChanged` without necessarily triggering `TimelineChanged`.
                val freshCard = hostDetail?.agents?.firstOrNull { it.agentId == aId } ?: activeAgent
                val serverList = freshCard?.queuedPrompts.orEmpty()
                return serverList.map { q ->
                    val cached = localList.firstOrNull { it.id == q.queueId }
                    if (cached != null) {
                        cached
                    } else {
                        QueuedPromptItem(
                            id = q.queueId,
                            text = q.displayText,
                            previewText = q.displayText.lineSequence().firstOrNull()?.trim().orEmpty()
                                .ifBlank { q.badges.joinToString(" ") { it.label } },
                        )
                    }
                }
            }
            return localList
        }

    val isActiveAgentPersonalAgent: Boolean
        get() = !isNewSessionMode && (activeAgent?.isPersonalRouter == true)

    val isActiveAgentRunning: Boolean
        get() = !isNewSessionMode && (activeAgent?.isForegroundRunning() == true)

    val otherHostsPendingApprovals: UInt
        get() = hosts
            .filter { it.hostId != selectedHostId }
            .sumOf { it.pendingApprovalCount }

    val activeGitBranch: String?
        get() {
            gitReview?.branch?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
            val detail = hostDetail ?: return null
            detail.discoveredProjects
                .firstOrNull { OrbitFormatters.pathsEquivalent(it.path, activeCwd) }
                ?.gitBranch?.trim()?.takeIf { it.isNotEmpty() }
                ?.let { return it }
            detail.workspaces
                .firstOrNull { OrbitFormatters.pathsEquivalent(it.cwd, activeCwd) }
                ?.gitBranch?.trim()?.takeIf { it.isNotEmpty() }
                ?.let { return it }
            return null
        }

    val launchpadProjects: List<FfiLaunchpadProject>
        get() {
            val hid = selectedHostId ?: return emptyList()
            return sidebarView?.launchpadProjects.orEmpty().filter { it.hostId == hid }
        }

    fun threadKey(hostId: String, agentId: String): String = "$hostId::$agentId"

    fun isThreadPinned(hostId: String, agentId: String): Boolean =
        sidebarView?.pinnedConversations.orEmpty().any {
            it.hostId == hostId && it.agent.agentId == agentId
        }

    fun isThreadUnread(hostId: String, agent: FfiAgentCard): Boolean {
        if (selectedHostId == hostId && !isNewSessionMode && selectedAgentId == agent.agentId) {
            return false
        }
        if (agent.isForegroundRunning() || agent.isAwaitingPermission) {
            return false
        }
        sidebarView?.pinnedConversations.orEmpty()
            .firstOrNull { it.hostId == hostId && it.agent.agentId == agent.agentId }
            ?.let { return it.isUnread }
        sidebarView?.projects.orEmpty()
            .filter { it.hostId == hostId }
            .forEach { grp ->
                grp.conversations.firstOrNull { it.agent.agentId == agent.agentId }?.let { return it.isUnread }
            }
        return false
    }
}

object OrbitFormatters {
    fun providerLabel(provider: FfiProviderKind): String = when (provider) {
        FfiProviderKind.ANTIGRAVITY -> "Antigravity"
        FfiProviderKind.CODEX -> "Codex"
        FfiProviderKind.CLAUDE_CODE -> "Claude Code"
        FfiProviderKind.OPEN_CODE -> "OpenCode"
        FfiProviderKind.GEMINI_CLI -> "Gemini CLI"
        FfiProviderKind.CURSOR_AGENT -> "Cursor Agent"
        FfiProviderKind.COPILOT -> "Copilot"
        FfiProviderKind.GOOSE -> "Goose"
        FfiProviderKind.PI -> "Pi"
        FfiProviderKind.KIMI -> "Kimi"
        FfiProviderKind.CLINE -> "Cline"
        FfiProviderKind.MISTRAL_VIBE -> "Mistral Vibe"
        FfiProviderKind.AUGMENT -> "Augment"
        FfiProviderKind.ACP -> "ACP Agent"
    }

    fun providerShortLabel(provider: FfiProviderKind): String = when (provider) {
        FfiProviderKind.ANTIGRAVITY -> "AGY"
        FfiProviderKind.CODEX -> "Codex"
        FfiProviderKind.CLAUDE_CODE -> "Claude"
        FfiProviderKind.OPEN_CODE -> "OpenCode"
        FfiProviderKind.GEMINI_CLI -> "Gemini"
        FfiProviderKind.CURSOR_AGENT -> "Cursor"
        FfiProviderKind.COPILOT -> "Copilot"
        FfiProviderKind.GOOSE -> "Goose"
        FfiProviderKind.PI -> "Pi"
        FfiProviderKind.KIMI -> "Kimi"
        FfiProviderKind.CLINE -> "Cline"
        FfiProviderKind.MISTRAL_VIBE -> "Mistral"
        FfiProviderKind.AUGMENT -> "Augment"
        FfiProviderKind.ACP -> "ACP"
    }

    fun sessionModeLabel(mode: FfiSessionMode): String = when (mode) {
        FfiSessionMode.Default -> "Always Ask"
        FfiSessionMode.Auto -> "Auto (Guardian)"
        FfiSessionMode.Plan -> "Always Ask"
        FfiSessionMode.AcceptEdits -> "Accept Edits"
        FfiSessionMode.FullAccess -> "Full Access"
        is FfiSessionMode.Custom -> mode.name
    }

    fun normalizeSessionMode(mode: FfiSessionMode): FfiSessionMode = when (mode) {
        FfiSessionMode.Plan -> FfiSessionMode.Default
        else -> mode
    }

    fun routeModeLabel(mode: FfiHostRouteMode): String = when (mode) {
        FfiHostRouteMode.AUTO -> "Auto"
        FfiHostRouteMode.SSH_TUNNEL -> "SSH Tunnel"
        FfiHostRouteMode.DIRECT_ONLY -> "Direct Only"
        FfiHostRouteMode.RELAY_ONLY -> "Relay Only"
    }

    fun connectionLabel(state: FfiConnectionState, viaSsh: Boolean = false): String = when (state) {
        FfiConnectionState.ONLINE_DIRECT -> if (viaSsh) "SSH" else "LAN Direct"
        FfiConnectionState.ONLINE_RELAY -> "Relay"
        FfiConnectionState.CONNECTING_DIRECT -> "Connecting"
        FfiConnectionState.CONNECTING_RELAY -> "Connecting Relay"
        FfiConnectionState.DISCONNECTED -> "Offline"
    }

    fun topBarConnectionLabel(state: FfiConnectionState, viaSsh: Boolean = false): String = when (state) {
        FfiConnectionState.ONLINE_DIRECT -> if (viaSsh) "SSH" else "Direct"
        FfiConnectionState.ONLINE_RELAY -> "Relay"
        FfiConnectionState.CONNECTING_DIRECT, FfiConnectionState.CONNECTING_RELAY -> "Connecting"
        FfiConnectionState.DISCONNECTED -> "Offline"
    }

    fun shortPath(path: String): String {
        val trimmed = path.trimEnd('/')
        if (trimmed.isEmpty() || trimmed == "~") return "~"
        val idx = trimmed.lastIndexOf('/')
        return if (idx >= 0 && idx < trimmed.length - 1) trimmed.substring(idx + 1) else trimmed
    }

    fun compactPath(path: String): String {
        val trimmed = path.trim()
        if (trimmed.isEmpty() || trimmed == "~") return "~"
        val clean = if (trimmed.length > 1) trimmed.trimEnd('/') else trimmed
        val parts = clean.split('/').filter { it.isNotEmpty() }
        if (clean.startsWith("/") && parts.size >= 2 && (parts[0] == "Users" || parts[0] == "home")) {
            val rest = parts.drop(2)
            return if (rest.isEmpty()) "~" else "~/" + rest.joinToString("/")
        }
        return clean
    }

    fun pathsEquivalent(first: String, second: String): Boolean {
        val a = first.trim().let { if (it.length > 1) it.trimEnd('/') else it }
        val b = second.trim().let { if (it.length > 1) it.trimEnd('/') else it }
        if (a == b) return true
        if (b.startsWith("~/")) {
            val suffix = "/" + b.removePrefix("~/")
            if (a.endsWith(suffix)) return true
        }
        if (a.startsWith("~/")) {
            val suffix = "/" + a.removePrefix("~/")
            if (b.endsWith(suffix)) return true
        }
        return false
    }

    fun catalogFor(detail: FfiHostDetail?, provider: FfiProviderKind): FfiProviderModelCatalog =
        detail?.providerCatalogs?.firstOrNull { it.provider == provider }
            ?: fallbackProviderCatalog(provider)

    fun modelLabel(catalog: FfiProviderModelCatalog, rawModelId: String?): String =
        formatCatalogModelLabel(catalog, rawModelId)

    fun effortLabel(effort: FfiReasoningEffort): String =
        formatEffortLabel(effort)

    fun compactEffortLabel(effort: FfiReasoningEffort): String {
        val full = effortLabel(effort)
        return when {
            effort == FfiReasoningEffort.MEDIUM || full.equals("Medium", ignoreCase = true) -> "Med"
            effort == FfiReasoningEffort.XHIGH || full.equals("Extra High", ignoreCase = true) -> "XHigh"
            else -> full
        }
    }

    fun effortDescription(effort: FfiReasoningEffort): String =
        defaultEffortDescription(effort)

    fun resolveEffort(
        spec: FfiProviderModelSpec?,
        desired: FfiReasoningEffort?,
        catalogDefault: FfiReasoningEffort?,
    ): FfiReasoningEffort? = resolveSupportedEffortForModel(spec, desired, catalogDefault)

    fun relativeTimeLabel(unixSecs: Long): String {
        if (unixSecs <= 0L) return ""
        val nowSecs = System.currentTimeMillis() / 1000L
        val delta = (nowSecs - unixSecs).coerceAtLeast(0L)
        return when {
            delta < 60L -> "just now"
            delta < 3_600L -> "${delta / 60L}m ago"
            delta < 86_400L -> "${delta / 3_600L}h ago"
            else -> "${delta / 86_400L}d ago"
        }
    }

    fun stripWorkspacePrefix(path: String, cwd: String): String {
        val cleanPath = path.trim()
        val cleanCwd = cwd.trim().trimEnd('/')
        if (cleanPath.isEmpty() || cleanCwd.isEmpty() || cleanCwd == "~") return cleanPath
        return when {
            cleanPath == cleanCwd -> "."
            cleanPath.startsWith("$cleanCwd/") -> cleanPath.removePrefix("$cleanCwd/")
            else -> cleanPath
        }
    }

    fun parseRfc3339EpochMs(rfc3339: String): Long? {
        val trimmed = rfc3339.trim()
        if (trimmed.isEmpty()) return null
        return runCatching { java.time.Instant.parse(trimmed).toEpochMilli() }.getOrNull()
            ?: runCatching { java.time.OffsetDateTime.parse(trimmed).toInstant().toEpochMilli() }.getOrNull()
    }

    fun compactRelativeTime(rfc3339: String): String {
        val epochMs = parseRfc3339EpochMs(rfc3339) ?: return ""
        val deltaSecs = ((System.currentTimeMillis() - epochMs) / 1000L).coerceAtLeast(0L)
        return when {
            deltaSecs < 60L -> "now"
            deltaSecs < 3600L -> "${deltaSecs / 60L}m"
            deltaSecs < 86_400L -> "${deltaSecs / 3600L}h"
            deltaSecs < 604_800L -> "${deltaSecs / 86_400L}d"
            else -> "${deltaSecs / 604_800L}w"
        }
    }
}

fun FfiAgentCard.isBackgroundWaiting(): Boolean {
    if (hasBackgroundTasks) return true
    val lower = statusLabel.trim().lowercase()
    return lower.startsWith("waiting for ")
        || lower == "waiting_background"
        || lower == "background task"
        || (lower.contains("background task") && lower.startsWith("running ("))
}

fun FfiAgentCard.isForegroundRunning(): Boolean = isRunning && !isBackgroundWaiting()

class OrbitAndroidStore(application: Application) : AndroidViewModel(application) {
    private val prefs = application.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    private val runningAgentKeys: MutableSet<String> = ConcurrentHashMap.newKeySet()
    private val drainingAgentKeys: MutableSet<String> = ConcurrentHashMap.newKeySet()
    private val notifiedApprovalAgentKeys: MutableSet<String> = ConcurrentHashMap.newKeySet()
    private val draftSelectionsByProvider: MutableMap<FfiProviderKind, Pair<String?, FfiReasoningEffort?>> =
        ConcurrentHashMap()
    @Volatile
    private var cloudSyncPollJob: Job? = null
    @Volatile
    private var isAppInForeground: Boolean = true
    @Volatile
    private var pendingDeepLink: Pair<String, String>? = null
    @Volatile
    private var hasUserCustomizedDraftModel: Boolean = false
    private val coreReady = CompletableDeferred<OrbitMobileCore?>()
    @Volatile
    private var pendingSelectLatestSession: Boolean = false

    private val _state = MutableStateFlow(
        OrbitUiState(
            appearanceMode = OrbitAppearanceMode.fromStorage(prefs.getString(KEY_APPEARANCE, null)),
            isPermanentDrawerCollapsed = prefs.getBoolean(KEY_DRAWER_COLLAPSED, false),
            draftUseWorktree = prefs.getBoolean(KEY_DRAFT_USE_WORKTREE, false),
            activeCwd = prefs.getString(KEY_LAST_CWD, "~") ?: "~",
        )
    )
    val state: StateFlow<OrbitUiState> = _state.asStateFlow()

    @Volatile
    private var core: OrbitMobileCore? = null

    @Volatile
    private var cachedFallbackCatalogs: List<FfiProviderModelCatalog> = emptyList()

    private val networkObserver = NetworkConnectivityObserver(application) {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.onNetworkChanged() }
        }
    }

    private val terminalInput = kotlinx.coroutines.channels.Channel<Triple<String, UByte, ByteArray>>(256)
    init {
        viewModelScope.launch(Dispatchers.IO) {
            for ((host, slot, bytes) in terminalInput) {
                runCatching {
                    val activeCore = checkNotNull(core ?: coreReady.await()) { "Orbit core is not ready" }
                    activeCore.sendTerminalBytes(host, slot, bytes)
                }.onFailure { postToast(extractErrorMessage(it, "Terminal input failed")) }
            }
        }
    }

    private val listener = object : MobileCoreListener {
        override fun onCoreEvent(event: FfiCoreEvent) {
            viewModelScope.launch(Dispatchers.IO) {
                handleCoreEvent(event)
            }
        }
    }

    init {
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val instance = OrbitRepository.openMobileCore(application)
                core = instance

                if (!prefs.getBoolean(KEY_MIGRATED_THREAD_PREFS_V1, false)) {
                    val legacyPinned = prefs.getStringSet(KEY_PINNED_THREAD_KEYS, emptySet()).orEmpty()
                    if (legacyPinned.isNotEmpty()) {
                        val nowMs = System.currentTimeMillis()
                        val seeds = legacyPinned.mapIndexedNotNull { idx, composite ->
                            val sepIdx = composite.indexOf("::")
                            if (sepIdx <= 0 || sepIdx + 2 >= composite.length) {
                                null
                            } else {
                                FfiThreadPrefSeed(
                                    hostId = composite.substring(0, sepIdx),
                                    agentId = composite.substring(sepIdx + 2),
                                    seenSeq = 0uL,
                                    isPinned = true,
                                    pinnedOrder = idx.toLong(),
                                    updatedAtMs = nowMs,
                                )
                            }
                        }
                        if (seeds.isNotEmpty()) {
                            runCatching { instance.seedThreadPrefs(seeds) }
                        }
                    }
                    prefs.edit()
                        .putBoolean(KEY_MIGRATED_THREAD_PREFS_V1, true)
                        .remove(KEY_PINNED_THREAD_KEYS)
                        .apply()
                }

                cachedFallbackCatalogs = runCatching { instance.fallbackProviderCatalogs() }
                    .getOrDefault(emptyList())
                val commands = runCatching { instance.slashCommands(FfiClientPlatform.ANDROID, null) }
                    .getOrDefault(emptyList())
                _state.update { it.copy(slashCommands = commands) }

                instance.subscribeThrottled(listener, 24u)
                networkObserver.start()
                val discovered = runCatching { instance.discoverLocalDaemonUri() }.getOrNull()
                if (discovered != null) {
                    _state.update { it.copy(localDaemonUri = discovered) }
                }
                reloadFleetInternal()
                refreshSidebarInternal()
                val initialSync = runCatching { instance.cloudSyncStatus() }.getOrNull()
                val initialPush = runCatching { instance.pushRegistrationStatus() }.getOrNull()
                _state.update {
                    it.copy(
                        cloudSyncStatus = initialSync ?: it.cloudSyncStatus,
                        pushRegistrationStatus = initialPush ?: it.pushRegistrationStatus,
                    )
                }
                OrbitFirebaseMessagingBridge.attachTokenListener(application) { fcmToken ->
                    registerPushToken(OrbitFirebaseMessagingBridge.PLATFORM_ANDROID_FCM, fcmToken)
                }
                coreReady.complete(instance)
                instance.connectAllHosts()
                _state.value.selectedHostId?.let { reloadHostDetailInternal(it) }
            } catch (t: Throwable) {
                if (!coreReady.isCompleted) {
                    coreReady.complete(null)
                }
                _state.update {
                    it.copy(lastError = extractErrorMessage(t, "Failed to initialize Orbit native core"))
                }
            }
        }
    }

    private fun extractErrorMessage(t: Throwable, fallback: String): String = when (t) {
        is FfiOrbitException.Core -> t.detail
        else -> t.message?.takeIf { it.isNotBlank() } ?: fallback
    }

    private fun handleCoreEvent(event: FfiCoreEvent) {
        val cur = _state.value
        when (event) {
            is FfiCoreEvent.FleetChanged -> {
                reloadFleetInternal()
                _state.value.selectedHostId?.let { reloadHostDetailInternal(it) }
            }
            is FfiCoreEvent.HostChanged -> {
                reloadFleetInternal()
                if (event.hostId == _state.value.selectedHostId) {
                    reloadHostDetailInternal(event.hostId)
                } else {
                    checkHostNotifications(event.hostId)
                }
            }
            is FfiCoreEvent.TimelineChanged -> {
                val latest = _state.value
                if (event.hostId == latest.selectedHostId && event.agentId == latest.selectedAgentId) {
                    reloadStageInternal(event.hostId, event.agentId)
                } else {
                    val queueKey = agentQueueKey(event.hostId, event.agentId)
                    if (latest.queuedPromptsByAgent[queueKey]?.isNotEmpty() == true) {
                        val bgDelta = runCatching {
                            core?.agentStageDelta(event.hostId, event.agentId, 0uL)
                        }.getOrNull()
                        if (bgDelta != null) {
                            val ag = bgDelta.agent
                            val busy = (ag?.isForegroundRunning() == true) ||
                                (ag?.isAwaitingPermission == true)
                            if (busy) {
                                runningAgentKeys.add(queueKey)
                            } else if (runningAgentKeys.remove(queueKey)) {
                                drainNextQueuedPromptIfSettled(event.hostId, event.agentId)
                            }
                        }
                    }
                }
            }
            is FfiCoreEvent.GitChanged -> {
                val latest = _state.value
                if (event.hostId == latest.selectedHostId) {
                    val activeCwd = latest.activeCwd
                    val review = runCatching {
                        core?.gitReview(event.hostId, activeCwd, null)
                    }.getOrNull()
                    val files = runCatching {
                        core?.workspaceFiles(event.hostId, activeCwd)
                    }.getOrNull()
                    _state.update { prev ->
                        prev.copy(
                            gitReview = review ?: prev.gitReview,
                            workspaceFiles = files ?: prev.workspaceFiles,
                        )
                    }
                }
            }
            is FfiCoreEvent.TerminalChanged -> {
                if (event.hostId == cur.selectedHostId) {
                    val scr = runCatching { core?.terminalScreen(event.hostId) }.getOrNull()
                    _state.update { it.copy(terminalScreen = scr) }
                }
            }
        }
    }

    fun onAppForeground() {
        isAppInForeground = true
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.onAppForeground() }
            reloadFleetInternal()
            _state.value.selectedHostId?.let { reloadHostDetailInternal(it) }
        }
    }

    fun onAppBackground() {
        isAppInForeground = false
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.onAppBackground() }
        }
    }

    fun setAppearanceMode(mode: OrbitAppearanceMode) {
        prefs.edit().putString(KEY_APPEARANCE, mode.name).apply()
        _state.update { it.copy(appearanceMode = mode) }
    }

    fun togglePermanentDrawerCollapsed() {
        setPermanentDrawerCollapsed(!_state.value.isPermanentDrawerCollapsed)
    }

    fun setPermanentDrawerCollapsed(collapsed: Boolean) {
        prefs.edit().putBoolean(KEY_DRAWER_COLLAPSED, collapsed).apply()
        _state.update { it.copy(isPermanentDrawerCollapsed = collapsed) }
    }

    fun setDraftUseWorktree(useWorktree: Boolean) {
        prefs.edit().putBoolean(KEY_DRAFT_USE_WORKTREE, useWorktree).apply()
        _state.update { it.copy(draftUseWorktree = useWorktree) }
    }

    fun updateDrawerSearchQuery(query: String) {
        _state.update { it.copy(drawerSearchQuery = query) }
        viewModelScope.launch(Dispatchers.IO) {
            refreshSidebarInternal()
        }
    }

    fun setSidebarProjectSort(sort: OrbitSidebarProjectSort) {
        _state.update { it.copy(sidebarProjectSort = sort) }
        viewModelScope.launch(Dispatchers.IO) {
            refreshSidebarInternal()
        }
    }

    fun setSidebarThreadFilter(filter: OrbitSidebarThreadFilter) {
        _state.update { it.copy(sidebarThreadFilter = filter) }
        viewModelScope.launch(Dispatchers.IO) {
            refreshSidebarInternal()
        }
    }

    fun toggleProjectCollapsed(projectId: String) {
        _state.update { prev ->
            val next = if (prev.collapsedProjectIds.contains(projectId)) {
                prev.collapsedProjectIds - projectId
            } else {
                prev.collapsedProjectIds + projectId
            }
            prev.copy(collapsedProjectIds = next)
        }
    }

    fun togglePinThread(hostId: String, agentId: String) {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.togglePinAgent(hostId, agentId) }
            refreshSidebarInternal()
        }
    }

    fun markAgentUnread(hostId: String, agentId: String) {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.markAgentUnread(hostId, agentId) }
            refreshSidebarInternal()
        }
    }

    fun consumeHarnessSheetTarget() {
        _state.update { it.copy(harnessSheetTarget = null) }
    }

    fun consumePendingPtyCommand(): String? {
        val cmd = _state.value.pendingPtyCommand
        if (cmd != null) {
            _state.update { it.copy(pendingPtyCommand = null) }
        }
        return cmd
    }

    fun applyLaunchIntent(intent: Intent?) {
        if (intent == null) return

        // 1. Deep-link pairing URI (`orbit://pair...`)
        if (intent.action == Intent.ACTION_VIEW) {
            intent.dataString?.let { handleDeepLinkUri(it) }
        }

        // Debug/automation launch extras are strictly restricted to debug builds so third-party apps
        // cannot inject PTY commands or UI state into the exported MainActivity in release builds.
        if (intent.hasExtra("google.message_id") || intent.hasExtra("gcm.message_id")) {
            val fields = listOf("event_kind", "host_id", "agent_id", "deep_link", "kind")
                .mapNotNull { key -> intent.getStringExtra(key)?.let { key to it } }.toMap()
            OrbitPushRouter.parsePayload(fields)?.let { handleDeepLinkUri(it.deepLink) }
        }
        if (!BuildConfig.DEBUG) return

        // 2. Immediate zero-flash theme (`--es orbit_theme <system|light|dark>`)
        intent.getStringExtra(EXTRA_THEME)?.trim()?.lowercase()?.takeIf { it.isNotEmpty() }?.let { rawTheme ->
            setAppearanceMode(OrbitAppearanceMode.fromStorage(rawTheme))
        }

        // 3. Auto-select latest session (`--ez orbit_select_latest true`)
        if (intent.hasExtra(EXTRA_SELECT_LATEST)) {
            val selectLatest = intent.getBooleanExtra(EXTRA_SELECT_LATEST, false)
            if (selectLatest) {
                pendingSelectLatestSession = true
                val existingLatest = _state.value.hostDetail?.agents
                    ?.maxByOrNull { it.updatedAtRfc3339 }
                    ?: _state.value.hostDetail?.agents?.firstOrNull()
                if (existingLatest != null) {
                    pendingSelectLatestSession = false
                    selectAgent(existingLatest.agentId)
                } else {
                    _state.update { it.copy(isNewSessionMode = false) }
                }
            }
        }

        // 4. Pair host URI (`--es orbit_pair_uri <uri>`)
        intent.getStringExtra(EXTRA_PAIR_URI)?.trim()?.takeIf { it.isNotEmpty() }?.let { uri ->
            pairHost(uri)
        }

        // 5. Drawer search query (`--es orbit_drawer_search <query>`)
        val drawerSearchExtra = intent.getStringExtra(EXTRA_DRAWER_SEARCH)
        if (drawerSearchExtra != null) {
            _state.update {
                it.copy(
                    drawerSearchQuery = drawerSearchExtra,
                    harnessRequestId = it.harnessRequestId + 1L,
                    harnessSheetTarget = "drawer",
                )
            }
        }

        // 6. Demo composer text (`--es orbit_demo_composer <text>`)
        if (intent.hasExtra(EXTRA_DEMO_COMPOSER)) {
            val demoText = intent.getStringExtra(EXTRA_DEMO_COMPOSER).orEmpty()
            _state.update { prev ->
                prev.copy(composer = prev.composer.copy(text = demoText))
            }
        }

        // 7. Demo approval & question cards (`--ez orbit_demo_approval` / `--ez orbit_demo_question`)
        if (intent.hasExtra(EXTRA_DEMO_APPROVAL)) {
            val enabled = intent.getBooleanExtra(EXTRA_DEMO_APPROVAL, false)
            val perms = if (enabled) listOf(buildDemoPermissionRequest()) else emptyList()
            _state.update { it.copy(demoPermissions = perms) }
        }
        if (intent.hasExtra(EXTRA_DEMO_QUESTION)) {
            val enabled = intent.getBooleanExtra(EXTRA_DEMO_QUESTION, false)
            val questions = if (enabled) listOf(buildDemoQuestionRequest()) else emptyList()
            _state.update { it.copy(demoQuestions = questions) }
        }

        // 8. PTY command (`--es orbit_pty_cmd <cmd>`) & sheet target (`--es orbit_open_sheet <...>`)
        val ptyCmd = intent.getStringExtra(EXTRA_PTY_CMD)?.takeIf { it.isNotBlank() }
        val explicitSheet = intent.getStringExtra(EXTRA_OPEN_SHEET)?.trim()?.lowercase()?.takeIf { it.isNotEmpty() }
        if (ptyCmd != null || explicitSheet != null) {
            val resolvedSheet = explicitSheet ?: if (ptyCmd != null) "pty" else null
            _state.update { prev ->
                prev.copy(
                    pendingPtyCommand = ptyCmd ?: prev.pendingPtyCommand,
                    harnessRequestId = prev.harnessRequestId + 1L,
                    harnessSheetTarget = resolvedSheet ?: prev.harnessSheetTarget,
                )
            }
        }
    }

    private fun buildDemoPermissionRequest(): FfiTimelineItem.PermissionRequest =
        FfiTimelineItem.PermissionRequest(
            seq = 900_001uL,
            requestId = "demo_perm_cargo_test",
            title = "Run Shell Command",
            commandOrPath = "cargo test --workspace --release",
            detail = FfiToolCallDetail.Shell(
                command = "cargo test --workspace --release",
                cwd = "~/Code/orbit",
                exitCode = null,
                outputPreview = null,
            ),
            allowedDecisions = listOf("allow", "allow_session", "deny"),
            resolvedDecision = null,
            timestampMs = System.currentTimeMillis(),
        )

    private fun buildDemoQuestionRequest(): FfiTimelineItem.QuestionRequest =
        FfiTimelineItem.QuestionRequest(
            seq = 900_002uL,
            requestId = "demo_question_arch",
            questions = listOf(
                FfiQuestionItem(
                    id = "q_transport",
                    header = "Architecture Decision",
                    question = "Which transport fallback policy should this session use for remote verification?",
                    options = listOf(
                        "(Recommended) Auto — Direct LAN with Noise NK relay fallback",
                        "SSH Tunnel — Loopback port forward only",
                        "Relay Only — Strict zero-knowledge E2EE relay",
                    ),
                    isMultiSelect = false,
                )
            ),
            resolvedAnswers = null,
            timestampMs = System.currentTimeMillis(),
        )

    fun dismissError() {
        _state.update { it.copy(lastError = null) }
    }

    fun clearToast() {
        _state.update { it.copy(toastMessage = null) }
    }

    fun showToast(msg: String) {
        postToast(msg)
    }

    private fun postToast(msg: String) {
        _state.update { it.copy(toastMessage = msg) }
    }

    fun handleDeepLinkUri(uriString: String?) {
        val disposition = OrbitDeepLinkSecurityPolicy.resolveExternalAction(uriString) ?: run {
            val trimmed = uriString?.trim().orEmpty()
            if (trimmed.startsWith("orbit://open", ignoreCase = true)) {
                postToast("Invalid session link")
            }
            return
        }
        when (disposition) {
            is ExternalDeepLinkDisposition.RequireConfirmation -> {
                _state.update { it.copy(pendingDeepLinkConfirmation = disposition.confirmation) }
            }
            is ExternalDeepLinkDisposition.ImmediateNavigate -> when (val action = disposition.action) {
                is OrbitDeepLinkAction.OpenThread -> openThreadFromDeepLink(action.hostId, action.agentId)
                is OrbitDeepLinkAction.OpenHost -> openHostFromDeepLink(action.hostId, action.eventKind)
                is OrbitDeepLinkAction.PairHost,
                is OrbitDeepLinkAction.ImportDeviceLink -> Unit
            }
        }
    }

    fun confirmPendingDeepLink() {
        var confirmed: PendingDeepLinkConfirmation? = null
        _state.update { prev ->
            confirmed = prev.pendingDeepLinkConfirmation
            prev.copy(pendingDeepLinkConfirmation = null)
        }
        val pending = confirmed ?: return
        when (pending.kind) {
            PendingDeepLinkKind.PAIR_HOST -> pairHost(pending.uri)
            PendingDeepLinkKind.IMPORT_CLOUD_SYNC_DEVICE_LINK -> importCloudSyncDeviceLinkUri(pending.uri)
        }
    }

    fun dismissPendingDeepLink() {
        _state.update { it.copy(pendingDeepLinkConfirmation = null) }
    }

    private fun openHostFromDeepLink(hostId: String, kind: dev.orbit.android.notifications.OrbitPushEventKind?) {
        viewModelScope.launch(Dispatchers.IO) {
            val readyCore = core ?: coreReady.await() ?: return@launch
            reloadFleetInternal()
            val known = _state.value.hosts.any { it.hostId == hostId }
            if (!known) {
                postToast("Host not found")
                return@launch
            }
            pendingDeepLink = null
            selectHost(hostId)
            if (kind in setOf(dev.orbit.android.notifications.OrbitPushEventKind.GOAL_FINISHED,
                    dev.orbit.android.notifications.OrbitPushEventKind.TIMER_FIRED)) requestSheet("automations")
            else openGlobalOverview()
            runCatching { readyCore.connectHost(hostId) }
        }
    }

    fun openThreadFromDeepLink(hostId: String, agentId: String) {
        viewModelScope.launch(Dispatchers.IO) {
            val readyCore = core ?: coreReady.await() ?: return@launch
            reloadFleetInternal()
            val knownHost = _state.value.hosts.any { it.hostId == hostId }
            if (!knownHost) {
                pendingDeepLink = null
                postToast("Host not found")
                return@launch
            }
            pendingDeepLink = hostId to agentId
            prefs.edit().putString(KEY_LAST_HOST_ID, hostId).apply()
            _state.update { prev ->
                prev.copy(
                    selectedHostId = hostId,
                    showGlobalOverview = false,
                    hostDetail = prev.hostDetail?.takeIf { it.host.hostId == hostId },
                    gitReview = null,
                    terminalScreen = null,
                    pushDevicesSnapshot = null,
                    agentStage = null,
                    selectedAgentId = agentId,
                    isNewSessionMode = false,
                    lastError = null,
                )
            }
            runCatching { readyCore.connectHost(hostId) }
            reloadHostDetailInternal(hostId)
            // `reloadHostDetailInternal` already opens and loads the stage when the deep-link target matches.
            val stillPending = pendingDeepLink == (hostId to agentId)
            val hasAgent = _state.value.hostDetail?.agents?.any { it.agentId == agentId } == true
            if (stillPending && hasAgent) {
                pendingDeepLink = null
                runCatching { readyCore.openAgentTimeline(hostId, agentId) }
                reloadStageInternal(hostId, agentId)
            }
            refreshSidebarInternal()
        }
    }

    fun consumePendingSheetTrigger() {
        _state.update { it.copy(pendingSheetTrigger = null) }
    }

    fun requestSheet(target: String) {
        _state.update {
            it.copy(
                pendingSheetTriggerId = it.pendingSheetTriggerId + 1L,
                pendingSheetTrigger = target,
            )
        }
    }

    fun reloadFleet() {
        viewModelScope.launch(Dispatchers.IO) {
            reloadFleetInternal()
        }
    }

    private fun refreshSidebarInternal() {
        val instance = core ?: return
        val cur = _state.value
        val req = FfiFleetSidebarRequest(
            sort = cur.sidebarProjectSort.toFfi(),
            filter = cur.sidebarThreadFilter.toFfi(),
            query = cur.drawerSearchQuery,
            activeHostId = cur.selectedHostId,
            activeAgentId = if (cur.isNewSessionMode) null else cur.selectedAgentId,
            activeCwd = cur.activeCwd,
        )
        val sidebar = runCatching { instance.fleetSidebarRequest(req) }.getOrNull() ?: return
        _state.update { it.copy(sidebarView = sidebar) }
    }

    private fun reloadFleetInternal() {
        val snap = runCatching { core?.fleetOverview() }.getOrNull() ?: return
        val savedHostId = prefs.getString(KEY_LAST_HOST_ID, null)
        _state.update { prev ->
            val currentValid = prev.selectedHostId?.takeIf { id -> snap.hosts.any { it.hostId == id } }
            val fallbackSaved = savedHostId?.takeIf { id -> snap.hosts.any { it.hostId == id } }
            val nextHostId = currentValid ?: fallbackSaved ?: snap.hosts.firstOrNull()?.hostId
            prev.copy(
                hosts = snap.hosts,
                projects = snap.projects,
                totalPendingApprovals = snap.totalPendingApprovals,
                selectedHostId = nextHostId,
            )
        }
        refreshSidebarInternal()
    }

    private fun reloadHostDetailInternal(hostId: String) {
        val detail = runCatching { core?.hostDetail(hostId) }.getOrNull() ?: return
        var autoOpenedLatestAgentId: String? = null
        var deepLinkMissingToast = false
        val activeDeepLink = pendingDeepLink?.takeIf { it.first == hostId }
        _state.update { prev ->
            if (prev.selectedHostId != hostId) return@update prev
            // Remap optimistic agent IDs once daemon confirms real session ID
            val remappedAgentId = prev.selectedAgentId?.let { currentId ->
                detail.replacedOptimisticAgents[currentId] ?: currentId
            }
            val latestAgentByTime = detail.agents.maxByOrNull { it.updatedAtRfc3339 }
                ?: detail.agents.firstOrNull()
            val shouldForceLatest = pendingSelectLatestSession && latestAgentByTime != null
            if (shouldForceLatest) {
                pendingSelectLatestSession = false
                autoOpenedLatestAgentId = latestAgentByTime.agentId
            }
            val isHostOnline = detail.host.connectionState == FfiConnectionState.ONLINE_DIRECT ||
                detail.host.connectionState == FfiConnectionState.ONLINE_RELAY
            val deepLinkAgentId = activeDeepLink?.second
            val deepLinkMatched = deepLinkAgentId != null &&
                detail.agents.any { it.agentId == deepLinkAgentId }
            if (deepLinkMatched) {
                pendingDeepLink = null
                autoOpenedLatestAgentId = deepLinkAgentId
            } else if (deepLinkAgentId != null &&
                isHostOnline &&
                detail.revision > 0uL &&
                detail.host.agentCount <= detail.agents.size.toUInt()
            ) {
                pendingDeepLink = null
                deepLinkMissingToast = true
            }
            val nextSelectedAgentId = when {
                deepLinkMatched -> deepLinkAgentId
                deepLinkAgentId != null && !deepLinkMissingToast -> deepLinkAgentId
                shouldForceLatest -> latestAgentByTime.agentId
                prev.isNewSessionMode -> null
                remappedAgentId != null && (remappedAgentId.startsWith("opt_") || detail.agents.any { it.agentId == remappedAgentId }) -> remappedAgentId
                else -> latestAgentByTime?.agentId
            }
            val nextNewSessionMode = if (shouldForceLatest || deepLinkMatched) false else prev.isNewSessionMode
            val matchedAgent = detail.agents.firstOrNull { it.agentId == nextSelectedAgentId }
            val nextCwd = matchedAgent?.cwd
                ?: prev.activeCwd.takeIf { it.isNotBlank() && it != "~" }
                ?: detail.workspaces.firstOrNull()?.cwd
                ?: detail.discoveredProjects.firstOrNull()?.path
                ?: "~"

            val preferredProvider = matchedAgent?.provider
                ?: prev.composer.provider.takeIf { detail.host.availableProviders.isEmpty() || it in detail.host.availableProviders }
                ?: detail.host.availableProviders.firstOrNull()
                ?: prev.composer.provider

            val savedSelection = draftSelectionsByProvider[preferredProvider]
            val (defaultDraftModel, defaultDraftEffort) = syncDefaultModelAndEffort(
                detail = detail,
                provider = preferredProvider,
                desiredModel = if (hasUserCustomizedDraftModel && prev.composer.provider == preferredProvider) {
                    prev.composer.model
                } else {
                    savedSelection?.first
                },
                desiredEffort = if (hasUserCustomizedDraftModel && prev.composer.provider == preferredProvider) {
                    prev.composer.effort
                } else {
                    savedSelection?.second
                },
            )
            val resolvedForAgent = matchedAgent?.let { ag ->
                resolvedModelSelection(
                    hostId = hostId,
                    provider = preferredProvider,
                    desiredModel = ag.model ?: defaultDraftModel,
                    desiredEffort = ag.effort,
                )
            }
            val resolvedModel = resolvedForAgent?.selectedModelId ?: matchedAgent?.model ?: defaultDraftModel
            val resolvedEffort = resolvedForAgent?.selectedEffort ?: run {
                val catalog = OrbitFormatters.catalogFor(detail, preferredProvider)
                val modelSpec = catalog.models.firstOrNull { it.id == resolvedModel }
                    ?: catalog.models.firstOrNull { it.isDefault }
                    ?: catalog.models.firstOrNull()
                matchedAgent?.let { ag ->
                    OrbitFormatters.resolveEffort(
                        spec = modelSpec,
                        desired = ag.effort,
                        catalogDefault = catalog.configuredDefaultEffort,
                    )
                } ?: defaultDraftEffort
            }
            val resolvedMode = OrbitFormatters.normalizeSessionMode(matchedAgent?.mode ?: prev.composer.mode)

            val files = runCatching { core?.workspaceFiles(hostId, nextCwd) }.getOrNull() ?: prev.workspaceFiles

            var migratedQueues = prev.queuedPromptsByAgent
            detail.replacedOptimisticAgents.forEach { (optimisticId, confirmedId) ->
                val oldKey = agentQueueKey(hostId, optimisticId)
                val newKey = agentQueueKey(hostId, confirmedId)
                val pending = migratedQueues[oldKey]
                if (!pending.isNullOrEmpty() && oldKey != newKey) {
                    migratedQueues = (migratedQueues - oldKey) +
                        (newKey to (migratedQueues[newKey].orEmpty() + pending))
                }
                if (runningAgentKeys.remove(oldKey)) {
                    runningAgentKeys.add(newKey)
                }
            }

            prev.copy(
                hostDetail = detail,
                selectedAgentId = nextSelectedAgentId,
                isNewSessionMode = nextNewSessionMode,
                activeCwd = nextCwd,
                workspaceFiles = files,
                queuedPromptsByAgent = migratedQueues,
                composer = prev.composer.copy(
                    provider = preferredProvider,
                    model = resolvedModel,
                    effort = resolvedEffort,
                    fastMode = matchedAgent?.fastMode ?: prev.composer.fastMode,
                    mode = resolvedMode,
                ),
            )
        }

        val stateSnapshot = _state.value
        val activeAgentId = stateSnapshot.selectedAgentId
        if (!stateSnapshot.isNewSessionMode && activeAgentId != null) {
            detail.agents.firstOrNull { it.agentId == activeAgentId }?.let { activeCard ->
                runCatching { core?.markAgentSeen(hostId, activeAgentId, activeCard.latestSeq) }
            }
        }

        if (deepLinkMissingToast) {
            postToast("Session not found")
        }

        autoOpenedLatestAgentId?.let { latestId ->
            runCatching { core?.openAgentTimeline(hostId, latestId) }
        }

        dispatchAgentNotificationsAndQueues(
            hostId = hostId,
            agents = detail.agents,
            activeAgentId = activeAgentId,
        )
        refreshSidebarInternal()
        if (activeAgentId != null && !stateSnapshot.isNewSessionMode) {
            reloadStageInternal(hostId, activeAgentId)
        }
    }

    private fun reloadHostDetailIfSelected(hostId: String) {
        if (_state.value.selectedHostId == hostId) {
            reloadHostDetailInternal(hostId)
        }
    }

    private fun checkHostNotifications(hostId: String) {
        val detail = runCatching { core?.hostDetail(hostId) }.getOrNull() ?: return
        dispatchAgentNotificationsAndQueues(
            hostId = hostId,
            agents = detail.agents,
            activeAgentId = null,
        )
    }

    private fun dispatchAgentNotificationsAndQueues(
        hostId: String,
        agents: List<FfiAgentCard>,
        activeAgentId: String?,
    ) {
        agents.forEach { ag ->
            val qKey = agentQueueKey(hostId, ag.agentId)
            val busy = ag.isForegroundRunning() || ag.isAwaitingPermission
            if (ag.isAwaitingPermission) {
                val shouldNotify = !isAppInForeground || ag.agentId != activeAgentId
                if (shouldNotify && notifiedApprovalAgentKeys.add(qKey)) {
                    OrbitNotificationHelper.notifyApprovalRequired(
                        context = getApplication(),
                        hostId = hostId,
                        agentId = ag.agentId,
                        title = ag.title.ifBlank { OrbitFormatters.providerLabel(ag.provider) },
                        body = "Approval needed in ${OrbitFormatters.shortPath(ag.cwd)}",
                    )
                }
            } else {
                notifiedApprovalAgentKeys.remove(qKey)
            }
            if (busy) {
                runningAgentKeys.add(qKey)
            } else if (runningAgentKeys.remove(qKey)) {
                if (!isAppInForeground) {
                    notifyAgentSettledInBackground(hostId, ag)
                }
                if (ag.agentId != activeAgentId &&
                    _state.value.queuedPromptsByAgent[qKey]?.isNotEmpty() == true
                ) {
                    drainNextQueuedPromptIfSettled(hostId, ag.agentId)
                }
            }
        }
    }

    private fun notifyAgentSettledInBackground(hostId: String, ag: FfiAgentCard) {
        val title = ag.title.ifBlank { OrbitFormatters.providerLabel(ag.provider) }
        if (ag.attentionLabel == "error") {
            OrbitNotificationHelper.notifyAgentError(
                context = getApplication(),
                hostId = hostId,
                agentId = ag.agentId,
                title = title,
                body = ag.statusLabel.ifBlank { "Agent run ended with an error." },
            )
        } else {
            OrbitNotificationHelper.notifyAgentCompleted(
                context = getApplication(),
                hostId = hostId,
                agentId = ag.agentId,
                title = title,
                body = ag.taskSummary?.takeIf { it.isNotBlank() } ?: "Run completed.",
            )
        }
    }

    private fun reloadStageInternal(hostId: String, agentId: String) {
        val prevState = _state.value
        val prevStage = prevState.agentStage
        val sameStage = prevStage != null &&
            prevStage.hostId == hostId &&
            (prevStage.agent?.agentId ?: prevState.selectedAgentId) == agentId
        val sinceRev = if (sameStage) prevStage!!.revision else 0uL
        val sinceAuxRev = if (sameStage && prevState.stageAuxRevision > 0uL) prevState.stageAuxRevision else null
        var delta = runCatching {
            core?.agentStageDeltaWithAux(hostId, agentId, sinceRev, sinceAuxRev)
        }.getOrNull() ?: return
        if (!delta.reset && sinceRev > 0uL && prevStage != null && delta.replaceFromTurnIndex.toInt() > prevStage.turns.size) {
            delta = runCatching { core?.agentStageDeltaWithAux(hostId, agentId, 0uL, null) }.getOrNull() ?: delta
        }
        val merged = StageDeltaReducer.merge(prevStage, hostId, agentId, delta)
        val stage = merged.stage

        _state.update { prev ->
            if (prev.selectedHostId != hostId || prev.selectedAgentId != agentId || prev.isNewSessionMode) return@update prev
            if (prev.agentStage?.hostId == hostId && prev.agentStage.agent?.agentId == agentId &&
                prev.agentStage.revision > stage.revision) return@update prev
            val ag = stage.agent
            val nextComposer = if (ag != null) {
                prev.composer.copy(
                    provider = ag.provider,
                    model = ag.model ?: prev.composer.model,
                    effort = ag.effort ?: prev.composer.effort,
                    fastMode = ag.fastMode,
                    mode = OrbitFormatters.normalizeSessionMode(ag.mode),
                )
            } else {
                prev.composer
            }
            prev.copy(
                agentStage = stage,
                stageHasOlderTurns = merged.hasOlderTurns,
                stageOldestLoadedSeq = merged.oldestLoadedSeq,
                stageAuxRevision = merged.auxRevision,
                isLoadingOlderTurns = prev.isLoadingOlderTurns && merged.hasOlderTurns && merged.oldestLoadedSeq == prev.stageOldestLoadedSeq,
                activeCwd = ag?.cwd ?: prev.activeCwd,
                composer = nextComposer,
            )
        }

        val curAfter = _state.value
        if (!curAfter.isNewSessionMode && curAfter.selectedHostId == hostId && curAfter.selectedAgentId == agentId) {
            val seenSeq = maxOf(stage.agent?.latestSeq ?: 0uL, stage.latestSeq)
            runCatching { core?.markAgentSeen(hostId, agentId, seenSeq) }
            if (stage.agent?.isForegroundRunning() != true) {
                refreshSidebarInternal()
            }
        }

        val queueKey = agentQueueKey(hostId, agentId)
        val ag = stage.agent
        val isBusy = (ag?.isForegroundRunning() == true) ||
            (ag?.isAwaitingPermission == true)
        if (isBusy) {
            runningAgentKeys.add(queueKey)
        } else if (runningAgentKeys.remove(queueKey)) {
            if (!isAppInForeground && ag != null) {
                notifyAgentSettledInBackground(hostId, ag)
            }
            if (_state.value.queuedPromptsByAgent[queueKey]?.isNotEmpty() == true) {
                drainNextQueuedPromptIfSettled(hostId, agentId)
            }
        }
    }

    fun selectHost(hostId: String) {
        pendingDeepLink = null
        prefs.edit().putString(KEY_LAST_HOST_ID, hostId).apply()
        _state.update {
            it.copy(
                selectedHostId = hostId,
                selectedAgentId = null,
                isNewSessionMode = false,
                agentStage = null,
                gitReview = null,
                hostDetail = null,
                workspaceFiles = emptyList(),
                pushDevicesSnapshot = null,
                activeCwd = "~",
                terminalScreen = null,
                lastError = null,
            )
        }
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.connectHost(hostId) }
            reloadHostDetailInternal(hostId)
            runCatching { core?.refreshProviderModels(hostId, false) }
            runCatching { core?.refreshQuotas(hostId) }
            val cwd = _state.value.activeCwd
            runCatching { core?.fetchGitStatus(hostId, cwd) }
            runCatching { core?.fetchGitDiff(hostId, cwd, null, _state.value.gitStagedMode) }
            runCatching { core?.fetchWorkspaceFiles(hostId, cwd) }
        }
    }

    fun refreshSelectedHost() {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.refreshHost(hostId) }
            runCatching { core?.refreshProviderModels(hostId, true) }
            runCatching { core?.refreshQuotas(hostId) }
            reloadHostDetailInternal(hostId)
            val cwd = _state.value.activeCwd
            runCatching { core?.fetchGitStatus(hostId, cwd) }
            runCatching { core?.fetchGitDiff(hostId, cwd, null, _state.value.gitStagedMode) }
            runCatching { core?.fetchWorkspaceFiles(hostId, cwd) }
        }
    }

    fun reconnectHost(hostId: String) {
        val name = _state.value.hosts.firstOrNull { it.hostId == hostId }?.displayName ?: "host"
        postToast("Reconnecting to $name…")
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.connectHost(hostId) }
            runCatching { core?.refreshHost(hostId) }
            runCatching { core?.refreshProviderModels(hostId, false) }
            runCatching { core?.refreshQuotas(hostId) }
            reloadFleetInternal()
            if (_state.value.selectedHostId == hostId) {
                reloadHostDetailInternal(hostId)
            }
        }
    }

    fun reconnectSelectedHost() {
        val hostId = _state.value.selectedHostId ?: return
        reconnectHost(hostId)
    }

    fun refreshStageAndHost() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: run {
            reloadFleet()
            return
        }
        if (cur.isRefreshingStage) return
        _state.update { it.copy(isRefreshingStage = true) }
        viewModelScope.launch(Dispatchers.IO) {
            val startedAt = SystemClock.elapsedRealtime()
            try {
                runCatching { core?.connectHost(hostId) }
                runCatching { core?.refreshHost(hostId) }
                reloadFleetInternal()
                reloadHostDetailInternal(hostId)
                val latest = _state.value
                val agentId = latest.selectedAgentId?.takeIf { !latest.isNewSessionMode }
                if (agentId != null) {
                    runCatching { core?.openAgentTimeline(hostId, agentId) }
                    reloadStageInternal(hostId, agentId)
                }
                val cwd = _state.value.activeCwd
                val staged = _state.value.gitStagedMode
                runCatching { core?.fetchGitStatus(hostId, cwd) }
                runCatching { core?.fetchGitDiff(hostId, cwd, null, staged) }
                runCatching { core?.fetchWorkspaceFiles(hostId, cwd) }
                val review = runCatching { core?.gitReview(hostId, cwd, null) }.getOrNull()
                val files = runCatching { core?.workspaceFiles(hostId, cwd) }.getOrNull()
                _state.update { prev ->
                    prev.copy(
                        gitReview = review ?: prev.gitReview,
                        workspaceFiles = files ?: prev.workspaceFiles,
                    )
                }
                val elapsed = SystemClock.elapsedRealtime() - startedAt
                if (elapsed < 350L) {
                    delay(350L - elapsed)
                }
            } finally {
                _state.update { it.copy(isRefreshingStage = false) }
            }
        }
    }

    fun discoverLocalDaemon() {
        viewModelScope.launch(Dispatchers.IO) {
            val uri = runCatching { core?.discoverLocalDaemonUri() }.getOrNull()
            if (uri != null) {
                _state.update { it.copy(localDaemonUri = uri) }
                pairHost(uri)
            } else {
                postToast("No local daemon discovered on this device")
            }
        }
    }

    fun pairHost(uri: String, onSuccess: (() -> Unit)? = null) {
        val trimmed = uri.trim()
        if (trimmed.isEmpty()) return
        if (trimmed.startsWith("orbit://sync-login", ignoreCase = true)) {
            importCloudSyncDeviceLinkUri(trimmed, onSuccess)
            return
        }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val readyCore = core ?: coreReady.await()
                if (readyCore == null) {
                    _state.update { it.copy(lastError = "Orbit core is not ready yet") }
                    return@launch
                }
                val card = readyCore.pairHost(trimmed)
                _state.update { it.copy(lastError = null) }
                reloadFleetInternal()
                selectHost(card.hostId)
                OrbitFirebaseMessagingBridge.loadPersistedToken(getApplication())?.let { token ->
                    registerPushToken(OrbitFirebaseMessagingBridge.PLATFORM_ANDROID_FCM, token)
                }
                postToast("Paired ${card.displayName}")
                onSuccess?.let { viewModelScope.launch(Dispatchers.Main) { it() } }
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to pair host")) }
            }
        }
    }

    fun removeHost(hostId: String) {
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.removeHost(hostId)
                _state.update { prev ->
                    if (prev.selectedHostId == hostId) {
                        prev.copy(
                            selectedHostId = null,
                            hostDetail = null,
                            selectedAgentId = null,
                            agentStage = null,
                            gitReview = null,
                            isDiffInspectorOpen = false,
                            terminalScreen = null,
                        )
                    } else {
                        prev
                    }
                }
                reloadFleetInternal()
                _state.value.selectedHostId?.let { reloadHostDetailInternal(it) }
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to remove host")) }
            }
        }
    }

    fun updateHostConfiguration(
        hostId: String,
        customName: String?,
        routeMode: FfiHostRouteMode?,
        sshAlias: String?,
        remoteDaemonPort: UShort?,
        customDirectEndpoint: String?,
        relayUrl: String?,
        reconnect: Boolean = true,
    ) {
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.updateHostConfig(
                    hostId = hostId,
                    customName = customName?.trim()?.ifEmpty { null },
                    routeMode = routeMode,
                    sshAlias = sshAlias?.trim()?.ifEmpty { null },
                    remoteDaemonPort = remoteDaemonPort,
                    customDirectEndpoint = customDirectEndpoint?.trim()?.ifEmpty { null },
                    relayUrl = relayUrl?.trim()?.ifEmpty { null },
                    reconnect = reconnect,
                )
                reloadFleetInternal()
                if (hostId == _state.value.selectedHostId) {
                    reloadHostDetailInternal(hostId)
                }
                postToast("Host configuration updated")
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to update host settings")) }
            }
        }
    }

    fun startNewSession(cwd: String? = null) {
        startNewSessionInWorkspace(cwd = cwd, provider = null)
    }

    fun startNewSessionInWorkspace(cwd: String? = null, provider: FfiProviderKind? = null) {
        pendingDeepLink = null
        val nextCwd = cwd?.trim()?.takeIf { it.isNotBlank() } ?: _state.value.activeCwd
        prefs.edit().putString(KEY_LAST_CWD, nextCwd).apply()
        val prevSnapshot = _state.value
        val targetProvider = provider ?: prevSnapshot.composer.provider
        val saved = draftSelectionsByProvider[targetProvider]
        val desiredModel = if (hasUserCustomizedDraftModel && prevSnapshot.composer.provider == targetProvider) {
            prevSnapshot.composer.model
        } else {
            saved?.first
        }
        val desiredEffort = if (hasUserCustomizedDraftModel && prevSnapshot.composer.provider == targetProvider) {
            prevSnapshot.composer.effort
        } else {
            saved?.second
        }
        val selectionGen = modelSelectionGeneration.incrementAndGet()
        _state.update { prev ->
            val nextComposer = prev.composer.copy(
                provider = targetProvider,
                model = desiredModel,
                effort = desiredEffort,
            )
            prev.copy(
                selectedAgentId = null,
                isNewSessionMode = true,
                showGlobalOverview = false,
                agentStage = null,
                stageHasOlderTurns = false,
                stageOldestLoadedSeq = 0uL,
                stageAuxRevision = 0uL,
                activeCwd = nextCwd,
                composer = nextComposer,
                lastError = null,
            )
        }
        val hostId = _state.value.selectedHostId
        viewModelScope.launch(Dispatchers.IO) {
            val (resolvedModel, resolvedEffort) = syncDefaultModelAndEffort(
                detail = _state.value.hostDetail,
                provider = targetProvider,
                desiredModel = desiredModel,
                desiredEffort = desiredEffort,
            )
            if (modelSelectionGeneration.get() == selectionGen) {
                draftSelectionsByProvider[targetProvider] = resolvedModel to resolvedEffort
                _state.update { prev ->
                    if (!prev.isNewSessionMode || prev.composer.provider != targetProvider) {
                        prev
                    } else {
                        prev.copy(
                            composer = prev.composer.copy(
                                model = resolvedModel,
                                effort = resolvedEffort,
                            )
                        )
                    }
                }
            }
            if (hostId == null) return@launch
            runCatching { core?.fetchWorkspaceFiles(hostId, nextCwd) }
            runCatching { core?.fetchGitStatus(hostId, nextCwd) }
            runCatching { core?.fetchGitDiff(hostId, nextCwd, null, _state.value.gitStagedMode) }
            val files = runCatching { core?.workspaceFiles(hostId, nextCwd) }.getOrNull().orEmpty()
            val review = runCatching { core?.gitReview(hostId, nextCwd, null) }.getOrNull()
            _state.update { it.copy(workspaceFiles = files, gitReview = review) }
            refreshSidebarInternal()
        }
    }

    fun inspectGitDiffAtCwd(cwd: String) {
        val cleanCwd = cwd.trim().ifEmpty { "~" }
        prefs.edit().putString(KEY_LAST_CWD, cleanCwd).apply()
        _state.update {
            it.copy(
                activeCwd = cleanCwd,
                isDiffInspectorOpen = true,
                gitSelectedPath = null,
            )
        }
        loadGitReview(cwd = cleanCwd, path = null)
    }

    fun openTerminalAtCwd(cwd: String) {
        val cleanCwd = cwd.trim().ifEmpty { "~" }
        prefs.edit().putString(KEY_LAST_CWD, cleanCwd).apply()
        _state.update { it.copy(activeCwd = cleanCwd) }
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            val detail = runCatching { core?.hostDetail(hostId) }.getOrNull()
            val matching = detail?.terminals?.firstOrNull { term ->
                OrbitFormatters.pathsEquivalent(term.cwd, cleanCwd)
            }
            if (matching != null) {
                runCatching { core?.attachTerminal(hostId, matching.terminalId, 1u) }
            } else {
                runCatching { core?.startTerminal(hostId, cleanCwd, cur.terminalCols, cur.terminalRows) }
            }
            reloadHostDetailInternal(hostId)
            val scr = runCatching { core?.terminalScreen(hostId) }.getOrNull()
            _state.update { it.copy(terminalScreen = scr) }
        }
    }

    fun selectAgent(agentId: String) {
        pendingDeepLink = null
        val hostId = _state.value.selectedHostId ?: return
        _state.update { prev ->
            prev.copy(
                selectedAgentId = agentId,
                isNewSessionMode = false,
                showGlobalOverview = false,
                agentStage = prev.agentStage?.takeIf { it.hostId == hostId && it.agent?.agentId == agentId },
                isLoadingOlderTurns = false,
                stageHasOlderTurns = false,
                stageOldestLoadedSeq = 0uL,
                stageAuxRevision = 0uL,
                lastError = null,
            )
        }
        viewModelScope.launch(Dispatchers.IO) {
            val matchedSeq = _state.value.hostDetail?.agents?.firstOrNull { it.agentId == agentId }?.latestSeq ?: 0uL
            runCatching { core?.markAgentSeen(hostId, agentId, matchedSeq) }
            runCatching { core?.openAgentTimeline(hostId, agentId) }
            reloadStageInternal(hostId, agentId)
            val cwd = _state.value.activeCwd
            runCatching { core?.fetchWorkspaceFiles(hostId, cwd) }
            runCatching { core?.fetchGitStatus(hostId, cwd) }
            runCatching { core?.fetchGitDiff(hostId, cwd, null, _state.value.gitStagedMode) }
            val files = runCatching { core?.workspaceFiles(hostId, cwd) }.getOrNull().orEmpty()
            val review = runCatching { core?.gitReview(hostId, cwd, null) }.getOrNull()
            _state.update { it.copy(workspaceFiles = files, gitReview = review) }
            refreshSidebarInternal()
        }
    }

    fun archiveAgent(agentId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.archiveAgent(hostId, agentId) }
            if (_state.value.selectedAgentId == agentId) {
                _state.update {
                    it.copy(
                        selectedAgentId = null,
                        isNewSessionMode = true,
                        agentStage = null,
                    )
                }
            }
            reloadHostDetailInternal(hostId)
        }
    }

    fun selectWorkspaceCwd(cwd: String) {
        val clean = cwd.trim().ifEmpty { "~" }
        prefs.edit().putString(KEY_LAST_CWD, clean).apply()
        startNewSession(clean)
    }

    fun browseDirectory(path: String? = null) {
        val hostId = _state.value.selectedHostId ?: return
        val target = path ?: _state.value.activeCwd.takeIf { it != "~" }
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.listDirectory(hostId, target) }
        }
    }

    fun pinProject(name: String, directory: String) {
        val hostId = _state.value.selectedHostId ?: return
        val cleanName = name.trim().ifEmpty { OrbitFormatters.shortPath(directory) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.upsertProject(null, hostId, cleanName, listOf(directory))
                reloadFleetInternal()
                postToast("Pinned $cleanName")
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to pin project")) }
            }
        }
    }

    fun removeProject(projectId: String) {
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.removeProject(projectId)
                reloadFleetInternal()
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to remove project")) }
            }
        }
    }

    fun refreshWorkspaceFiles() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.fetchWorkspaceFiles(hostId, cur.activeCwd) }
            val files = runCatching { core?.workspaceFiles(hostId, cur.activeCwd) }.getOrNull().orEmpty()
            _state.update { if (it.selectedHostId == hostId && it.activeCwd == cur.activeCwd) it.copy(workspaceFiles = files) else it }
        }
    }

    // --- Composer & Agent Config ---

    fun updateComposerText(text: String) {
        _state.update { it.copy(composer = it.composer.copy(text = text)) }
    }

    private fun resolvedModelSelection(
        hostId: String?,
        provider: FfiProviderKind,
        desiredModel: String?,
        desiredEffort: FfiReasoningEffort?,
    ): FfiResolvedModelSelection? = runCatching {
        core?.resolveModelSelection(
            FfiModelSelectionRequest(
                hostId = hostId,
                provider = provider,
                desiredModel = desiredModel,
                desiredEffort = desiredEffort,
            )
        )
    }.getOrNull()

    private fun syncDefaultModelAndEffort(
        detail: FfiHostDetail?,
        provider: FfiProviderKind,
        desiredModel: String? = null,
        desiredEffort: FfiReasoningEffort? = null,
    ): Pair<String?, FfiReasoningEffort?> {
        resolvedModelSelection(
            hostId = detail?.host?.hostId ?: _state.value.selectedHostId,
            provider = provider,
            desiredModel = desiredModel,
            desiredEffort = desiredEffort,
        )?.let { resolved ->
            return resolved.selectedModelId to resolved.selectedEffort
        }
        val catalog = OrbitFormatters.catalogFor(detail, provider)
        val resolvedModel = desiredModel?.takeIf { id ->
            catalog.models.isEmpty() || catalog.models.any { it.id == id }
        }
            ?: catalog.configuredDefaultModel
            ?: catalog.models.firstOrNull { it.isDefault }?.id
            ?: catalog.models.firstOrNull()?.id
        val spec = catalog.models.firstOrNull { it.id == resolvedModel }
            ?: catalog.models.firstOrNull { it.isDefault }
            ?: catalog.models.firstOrNull()
        val resolvedEffort = OrbitFormatters.resolveEffort(
            spec = spec,
            desired = desiredEffort,
            catalogDefault = catalog.configuredDefaultEffort,
        )
        return resolvedModel to resolvedEffort
    }

    private val modelSelectionGeneration = AtomicLong(0L)

    fun switchDraftProvider(provider: FfiProviderKind) {
        val prev = _state.value
        draftSelectionsByProvider[prev.composer.provider] =
            prev.composer.model to prev.composer.effort
        hasUserCustomizedDraftModel = true
        val saved = draftSelectionsByProvider[provider]
        selectProviderModelAndEffort(provider, saved?.first, saved?.second)
    }

    fun selectEffort(effort: FfiReasoningEffort) {
        val cur = _state.value
        selectProviderModelAndEffort(cur.composer.provider, cur.composer.model, effort)
    }

    fun selectProviderModelAndEffort(
        provider: FfiProviderKind,
        model: String?,
        effort: FfiReasoningEffort?,
    ) {
        val prev = _state.value
        val gen = modelSelectionGeneration.incrementAndGet()
        val isDraft = prev.isNewSessionMode || prev.selectedAgentId == null
        if (isDraft) {
            hasUserCustomizedDraftModel = true
            draftSelectionsByProvider[provider] = model to effort
        }

        _state.update {
            it.copy(
                composer = it.composer.copy(
                    provider = provider,
                    model = model,
                    effort = effort,
                )
            )
        }

        val hostId = prev.selectedHostId
        val agentId = prev.selectedAgentId
        val activeProvider = prev.activeAgent?.provider
        val detailSnapshot = prev.hostDetail

        viewModelScope.launch(Dispatchers.IO) {
            val (resolvedModel, resolvedEffort) = syncDefaultModelAndEffort(
                detail = detailSnapshot,
                provider = provider,
                desiredModel = model,
                desiredEffort = effort,
            )
            if (modelSelectionGeneration.get() != gen) return@launch
            if (isDraft) {
                draftSelectionsByProvider[provider] = resolvedModel to resolvedEffort
            }
            _state.update {
                it.copy(
                    composer = it.composer.copy(
                        provider = provider,
                        model = resolvedModel,
                        effort = resolvedEffort,
                    )
                )
            }
            if (hostId == null || agentId == null || prev.isNewSessionMode) return@launch
            if (activeProvider != null && activeProvider != provider) {
                runCatching { core?.switchAgentProvider(hostId, agentId, provider, resolvedModel, resolvedEffort, null) }
            } else {
                runCatching { core?.setAgentConfig(hostId, agentId, resolvedModel, resolvedEffort) }
            }
            reloadHostDetailInternal(hostId)
        }
    }

    fun toggleFastMode() {
        setFastMode(!_state.value.composer.fastMode)
    }

    fun setFastMode(enabled: Boolean) {
        val prev = _state.value
        _state.update {
            it.copy(
                composer = it.composer.copy(fastMode = enabled),
                toastMessage = if (enabled) "Fast mode enabled" else "Fast mode disabled",
            )
        }
        val hostId = prev.selectedHostId ?: return
        val agentId = prev.selectedAgentId ?: return
        if (prev.isNewSessionMode) return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.setAgentFastMode(hostId, agentId, enabled) }
            reloadStageInternal(hostId, agentId)
        }
    }

    fun selectSessionMode(mode: FfiSessionMode) {
        val normalized = OrbitFormatters.normalizeSessionMode(mode)
        val prev = _state.value
        _state.update { it.copy(composer = it.composer.copy(mode = normalized)) }
        if (prev.isNewSessionMode || prev.selectedAgentId == null) {
            viewModelScope.launch(Dispatchers.IO) {
                runCatching { core?.setAccountDefaultMode(normalized, System.currentTimeMillis()) }
            }
            return
        }
        val hostId = prev.selectedHostId ?: return
        val agentId = prev.selectedAgentId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.setSessionMode(hostId, agentId, normalized) }
            reloadStageInternal(hostId, agentId)
        }
    }

    fun attachImageBytes(filename: String, mimeType: String?, bytes: ByteArray) {
        stageAttachmentBytes(
            filename = filename,
            mimeType = mimeType ?: "image/png",
            bytes = bytes,
            isImage = true,
        )
    }

    fun attachFileBytes(filename: String, mimeType: String?, bytes: ByteArray) {
        stageAttachmentBytes(
            filename = filename,
            mimeType = mimeType,
            bytes = bytes,
            isImage = false,
        )
    }

    private fun stageAttachmentBytes(
        filename: String,
        mimeType: String?,
        bytes: ByteArray,
        isImage: Boolean,
    ) {
        val hostId = _state.value.selectedHostId ?: run {
            postToast("Select a host first to attach files")
            return
        }
        val safeName = filename.trim().ifEmpty { if (isImage) "image.png" else "attachment.bin" }
        _state.update { it.copy(composer = it.composer.copy(isStagingAttachment = true)) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val staged = core?.stageAttachment(hostId, safeName, mimeType, bytes)
                    ?: throw IllegalStateException("Core is not ready")
                val item = StagedAttachmentItem(
                    path = staged.path,
                    filename = staged.filename.ifBlank { safeName },
                    sizeBytes = staged.sizeBytes.toLong(),
                    isImage = isImage,
                    previewBytes = if (isImage) bytes else null,
                )
                _state.update { prev ->
                    prev.copy(
                        composer = prev.composer.copy(
                            stagedAttachments = prev.composer.stagedAttachments + item,
                            isStagingAttachment = false,
                        )
                    )
                }
            } catch (t: Throwable) {
                _state.update { it.copy(composer = it.composer.copy(isStagingAttachment = false)) }
                postToast(extractErrorMessage(t, "Failed to stage attachment"))
            }
        }
    }

    fun removeStagedAttachment(id: String) {
        _state.update { prev ->
            prev.copy(
                composer = prev.composer.copy(
                    stagedAttachments = prev.composer.stagedAttachments.filterNot { it.id == id }
                )
            )
        }
    }

    fun registerInlineMention(
        token: String,
        kind: String,
        path: String? = null,
        staged: Boolean = false,
        label: String = "",
    ) {
        if (token.isBlank()) return
        val reg = InlineMentionRegistration(
            token = token,
            kind = kind,
            path = path,
            staged = staged,
            label = label,
        )
        _state.update { prev ->
            val filtered = prev.composer.inlineMentions.filterNot { it.token == token }
            prev.copy(
                composer = prev.composer.copy(inlineMentions = filtered + reg)
            )
        }
        if (kind == "terminal") {
            val cur = _state.value
            val hostId = cur.selectedHostId ?: return
            viewModelScope.launch(Dispatchers.IO) {
                val scr = runCatching { core?.terminalScreen(hostId) }.getOrNull()
                if (scr != null) {
                    _state.update { it.copy(terminalScreen = scr) }
                }
            }
        }
    }

    suspend fun filterSlashCommands(query: String): List<FfiSlashCommand> {
        val q = query.trim()
        if (q.isEmpty()) return _state.value.slashCommands
        return withContext(Dispatchers.IO) {
            runCatching { core?.slashCommands(FfiClientPlatform.ANDROID, q) }.getOrNull()
                ?: _state.value.slashCommands
        }
    }

    suspend fun rankMentionCandidates(query: String, limit: UInt = 11u): List<FfiMentionCandidate> {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return emptyList()
        val cwd = cur.activeCwd
        return withContext(Dispatchers.IO) {
            runCatching {
                core?.rankMentionCandidates(
                    FfiMentionRankRequest(
                        hostId = hostId,
                        cwd = cwd,
                        query = query,
                        limit = limit,
                    )
                )
            }.getOrNull().orEmpty()
        }
    }

    fun executeSlashCommand(
        cmd: FfiSlashCommand,
        onOpenTerminal: () -> Unit = {},
        onOpenSettings: () -> Unit = {},
        onOpenSideQuestion: () -> Unit = {},
        onOpenAutomations: () -> Unit = {},
        onOpenWorkspaceBoard: () -> Unit = {},
        onOpenArchived: () -> Unit = {},
        onOpenFork: () -> Unit = {},
        onExportTranscript: () -> Unit = {},
    ) {
        when (cmd.id) {
            "plan" -> setSlashCommand("/plan")
            "review" -> setSlashCommand("/review")
            "diff" -> toggleDiffInspector()
            "compact" -> submitSlashCompact("")
            "btw" -> onOpenSideQuestion()
            "fast" -> toggleFastMode()
            "fork" -> onOpenFork()
            "automations" -> onOpenAutomations()
            "board" -> onOpenWorkspaceBoard()
            "archived" -> onOpenArchived()
            "export" -> onExportTranscript()
            "supervised", "ask" -> {
                selectSessionMode(FfiSessionMode.Default)
                postToast("Permissions: Always Ask")
            }
            "auto" -> {
                if (cmd.command.equals("/edits", ignoreCase = true)) {
                    selectSessionMode(FfiSessionMode.AcceptEdits)
                    postToast("Permissions: Accept Edits")
                } else {
                    selectSessionMode(FfiSessionMode.Auto)
                    postToast("Permissions: Auto (Guardian)")
                }
            }
            "edits" -> {
                selectSessionMode(FfiSessionMode.AcceptEdits)
                postToast("Permissions: Accept Edits")
            }
            "full" -> {
                selectSessionMode(FfiSessionMode.FullAccess)
                postToast("Permissions: Full Access")
            }
            "terminal" -> onOpenTerminal()
            "new" -> startNewSession()
            "settings" -> onOpenSettings()
        }
    }

    fun applyQuickWorkflow(slash: String) {
        when (slash.trim().lowercase()) {
            "/compact" -> submitSlashCompact("")
            "/plan" -> setSlashCommand("/plan")
            "/review" -> setSlashCommand("/review")
            else -> setSlashCommand(slash)
        }
    }

    fun submitSlashCompact(extraFocus: String = "") {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: run {
            postToast("Start an active session first to compact context")
            return
        }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val composed = composePromptViaCore(
                    hostId = hostId,
                    activeCwd = cur.activeCwd,
                    provider = cur.activeAgent?.provider ?: cur.composer.provider,
                    gitReview = cur.gitReview,
                    gitStagedMode = cur.gitStagedMode,
                    terminalScreen = cur.terminalScreen,
                    text = extraFocus.trim(),
                    mentionedFiles = emptyList(),
                    stagedAttachments = emptyList(),
                    inlineMentions = emptyList(),
                    includeGitDiff = false,
                    includeStagedDiff = false,
                    includeTerminal = false,
                    activeSlashCommand = "/compact",
                    quotedSnippets = emptyList(),
                ) ?: return@launch
                runningAgentKeys.add(agentQueueKey(hostId, agentId))
                core?.sendPromptWithMeta(
                    hostId = hostId,
                    agentId = agentId,
                    prompt = composed.wirePrompt,
                    displayPrompt = composed.displayPrompt,
                    badges = composed.badges,
                )
                reloadStageInternal(hostId, agentId)
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to compact context")) }
            }
        }
    }

    fun addFileMention(path: String) {
        _state.update { prev ->
            if (path in prev.composer.mentionedFiles) prev
            else prev.copy(composer = prev.composer.copy(mentionedFiles = prev.composer.mentionedFiles + path))
        }
    }

    fun removeFileMention(path: String) {
        _state.update { prev ->
            prev.copy(composer = prev.composer.copy(mentionedFiles = prev.composer.mentionedFiles - path))
        }
    }

    fun toggleGitDiffContext() {
        _state.update { prev ->
            prev.copy(composer = prev.composer.copy(includeGitDiff = !prev.composer.includeGitDiff))
        }
    }

    fun attachGitDiffContext() {
        _state.update { prev ->
            if (prev.composer.includeGitDiff) prev
            else prev.copy(composer = prev.composer.copy(includeGitDiff = true))
        }
    }

    fun toggleStagedDiffContext() {
        _state.update { prev ->
            prev.copy(composer = prev.composer.copy(includeStagedDiff = !prev.composer.includeStagedDiff))
        }
    }

    fun attachStagedDiffContext() {
        _state.update { prev ->
            if (prev.composer.includeStagedDiff) prev
            else prev.copy(composer = prev.composer.copy(includeStagedDiff = true))
        }
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.fetchGitStatus(hostId, cur.activeCwd) }
        }
    }

    fun toggleTerminalContext() {
        _state.update { prev ->
            prev.copy(composer = prev.composer.copy(includeTerminal = !prev.composer.includeTerminal))
        }
    }

    fun attachTerminalContext() {
        _state.update { prev ->
            if (prev.composer.includeTerminal) prev
            else prev.copy(composer = prev.composer.copy(includeTerminal = true))
        }
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            val scr = runCatching { core?.terminalScreen(hostId) }.getOrNull()
            if (scr != null) {
                _state.update { it.copy(terminalScreen = scr) }
            }
        }
    }

    fun quoteTerminalToComposer() {
        _state.update { prev ->
            prev.copy(
                composer = prev.composer.copy(includeTerminal = true),
                toastMessage = "Attached @terminal to composer",
            )
        }
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            val scr = runCatching { core?.terminalScreen(hostId) }.getOrNull()
            if (scr != null) {
                _state.update { it.copy(terminalScreen = scr) }
            }
        }
    }

    fun setSlashCommand(slash: String?) {
        _state.update { prev ->
            prev.copy(composer = prev.composer.copy(activeSlashCommand = slash))
        }
    }

    fun addQuotedDiffLine(path: String, lineLabel: String, content: String) {
        val snippet = QuotedDiffSnippet(
            path = path,
            lineLabel = lineLabel,
            content = content,
        )
        _state.update { prev ->
            prev.copy(
                composer = prev.composer.copy(quotedSnippets = prev.composer.quotedSnippets + snippet),
                toastMessage = "Quoted ${OrbitFormatters.shortPath(path)}:$lineLabel",
            )
        }
    }

    fun addQuotedAssistantReply(rawReply: String) {
        val cleaned = rawReply.trim()
        if (cleaned.isEmpty()) return
        val firstMeaningfulLine = cleaned
            .lineSequence()
            .map { it.trim() }
            .firstOrNull { it.isNotEmpty() && !it.startsWith("```") }
            ?.trimStart('#', '>', '-', '*', ' ')
            .orEmpty()
        val shortPreview = if (firstMeaningfulLine.length > 22) {
            firstMeaningfulLine.take(22).trimEnd() + "…"
        } else {
            firstMeaningfulLine
        }
        val chipLabel = if (shortPreview.isNotBlank()) "Reply · $shortPreview" else "Quoted reply"
        val excerpt = cleaned.take(1_600)
        val snippet = QuotedDiffSnippet(
            path = "assistant",
            lineLabel = chipLabel,
            content = excerpt,
            isAssistantExcerpt = true,
        )
        _state.update { prev ->
            prev.copy(
                composer = prev.composer.copy(quotedSnippets = prev.composer.quotedSnippets + snippet),
                toastMessage = "Quoted assistant reply",
            )
        }
    }

    fun removeQuotedSnippet(id: String) {
        _state.update { prev ->
            prev.copy(
                composer = prev.composer.copy(
                    quotedSnippets = prev.composer.quotedSnippets.filterNot { it.id == id }
                )
            )
        }
    }

    private fun agentQueueKey(hostId: String, agentId: String): String = "$hostId::$agentId"

    private fun parseSlashDirectiveEnum(raw: String?): FfiSlashDirective? =
        when (raw?.trim()?.lowercase()?.removePrefix("/")) {
            "plan" -> FfiSlashDirective.PLAN
            "goal" -> FfiSlashDirective.GOAL
            "review" -> FfiSlashDirective.REVIEW
            "compact" -> FfiSlashDirective.COMPACT
            "orbit" -> FfiSlashDirective.ORBIT
            "orbit-handoff" -> FfiSlashDirective.ORBIT_HANDOFF
            "orbit-committee" -> FfiSlashDirective.ORBIT_COMMITTEE
            "orbit-advisor" -> FfiSlashDirective.ORBIT_ADVISOR
            else -> null
        }

    private fun composePromptViaCore(
        hostId: String,
        activeCwd: String,
        provider: FfiProviderKind,
        gitReview: FfiGitReview?,
        gitStagedMode: Boolean,
        terminalScreen: FfiTerminalScreen?,
        text: String,
        mentionedFiles: List<String>,
        stagedAttachments: List<StagedAttachmentItem>,
        inlineMentions: List<InlineMentionRegistration>,
        includeGitDiff: Boolean,
        includeStagedDiff: Boolean,
        includeTerminal: Boolean,
        activeSlashCommand: String?,
        quotedSnippets: List<QuotedDiffSnippet>,
    ): FfiComposedPrompt? {
        val instance = core ?: return null
        val contexts = mutableListOf<FfiComposerContextInput>()
        val seenFiles = linkedSetOf<String>()
        var hasWorkingDiff = includeGitDiff
        var hasStagedDiff = includeStagedDiff
        var hasTerminal = includeTerminal

        inlineMentions.forEach { mention ->
            if (!text.contains(mention.token)) return@forEach
            when (mention.kind) {
                "file" -> mention.path?.let { p -> if (seenFiles.add(p)) contexts += FfiComposerContextInput.WorkspaceFile(p) }
                "git_diff" -> if (mention.staged) hasStagedDiff = true else hasWorkingDiff = true
                "terminal" -> hasTerminal = true
            }
        }

        mentionedFiles.forEach { path ->
            if (seenFiles.add(path)) {
                contexts += FfiComposerContextInput.WorkspaceFile(path)
            }
        }

        stagedAttachments.forEach { att ->
            contexts += if (att.isImage) {
                FfiComposerContextInput.Image(path = att.path, filename = att.filename)
            } else {
                FfiComposerContextInput.StagedFile(path = att.path, filename = att.filename)
            }
        }

        if (hasWorkingDiff) {
            val diffText = if (!gitStagedMode) gitReview?.rawDiff?.takeIf { it.isNotBlank() } else null
            contexts += FfiComposerContextInput.GitDiff(
                staged = false,
                label = "Working Tree Diff",
                diffText = diffText,
            )
        }
        if (hasStagedDiff) {
            val diffText = if (gitStagedMode) gitReview?.rawDiff?.takeIf { it.isNotBlank() } else null
            contexts += FfiComposerContextInput.GitDiff(
                staged = true,
                label = "Staged Diff",
                diffText = diffText,
            )
        }
        if (hasTerminal) {
            val scr = terminalScreen ?: runCatching { instance.terminalScreen(hostId) }.getOrNull()
            val termLines = scr?.rowsData.orEmpty()
                .map { row -> row.spans.joinToString("") { it.text }.trimEnd() }
                .dropLastWhile { it.isBlank() }
                .takeLast(60)
                .joinToString("\n")
                .take(6_000)
            contexts += FfiComposerContextInput.TerminalOutput(
                label = "Terminal",
                content = termLines,
            )
        }

        val quotes = mutableListOf<FfiComposerQuoteInput>()
        quotedSnippets.forEach { q ->
            if (q.isAssistantExcerpt) {
                contexts += FfiComposerContextInput.AssistantQuote(
                    label = q.badgeLabel,
                    content = q.content,
                )
            } else {
                val parsedLine = q.lineLabel.trim().removePrefix("L").toUIntOrNull()
                quotes += FfiComposerQuoteInput(
                    path = q.path,
                    lineNumber = parsedLine,
                    code = q.content,
                )
            }
        }

        return runCatching {
            instance.composePrompt(
                FfiComposePromptRequest(
                    provider = provider,
                    rawText = text,
                    explicitDirective = parseSlashDirectiveEnum(activeSlashCommand),
                    hostId = hostId,
                    cwd = activeCwd,
                    contexts = contexts,
                    quotes = quotes,
                    pastes = emptyList(),
                )
            )
        }.getOrNull()
    }

    private val isDispatchingPrompt = AtomicBoolean(false)

    private fun clearSubmittedDraftIfUnchanged(
        draft: ComposerDraftState,
        clearAttachments: Boolean,
        toast: String? = null,
    ) {
        _state.update { prev ->
            val nextComposer = if (prev.composer.text == draft.text && prev.composer.activeSlashCommand == draft.activeSlashCommand) {
                if (clearAttachments) {
                    prev.composer.copy(
                        text = "",
                        mentionedFiles = emptyList(),
                        stagedAttachments = emptyList(),
                        inlineMentions = emptyList(),
                        includeGitDiff = false,
                        includeStagedDiff = false,
                        includeTerminal = false,
                        activeSlashCommand = null,
                        quotedSnippets = emptyList(),
                    )
                } else {
                    prev.composer.copy(text = "", activeSlashCommand = null)
                }
            } else {
                prev.composer
            }
            if (toast != null) prev.copy(composer = nextComposer, toastMessage = toast)
            else prev.copy(composer = nextComposer)
        }
    }

    fun enqueueComposerPrompt() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val draft = cur.composer
        if (!draft.canSend) return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: run {
            submitComposerPrompt()
            return
        }
        if (!isDispatchingPrompt.compareAndSet(false, true)) return

        viewModelScope.launch(Dispatchers.IO) {
            try {
                val preview = composePromptViaCore(
                    hostId = hostId,
                    activeCwd = cur.activeCwd,
                    provider = cur.activeAgent?.provider ?: draft.provider,
                    gitReview = cur.gitReview,
                    gitStagedMode = cur.gitStagedMode,
                    terminalScreen = cur.terminalScreen,
                    text = draft.text,
                    mentionedFiles = draft.mentionedFiles,
                    stagedAttachments = draft.stagedAttachments,
                    inlineMentions = draft.inlineMentions,
                    includeGitDiff = draft.includeGitDiff,
                    includeStagedDiff = draft.includeStagedDiff,
                    includeTerminal = draft.includeTerminal,
                    activeSlashCommand = draft.activeSlashCommand,
                    quotedSnippets = draft.quotedSnippets,
                ) ?: return@launch
                if (preview.isEmpty) return@launch
                if (preview.isBtw) {
                    clearSubmittedDraftIfUnchanged(draft, clearAttachments = false)
                    requestSheet("btw")
                    preview.btwQuestion?.trim()?.takeIf { it.isNotEmpty() }?.let { submitSideQuestion(it) }
                    return@launch
                }
                preview.fastModeAction?.let { action ->
                    clearSubmittedDraftIfUnchanged(draft, clearAttachments = false)
                    when (action) {
                        FfiFastModeSlashAction.TOGGLE -> toggleFastMode()
                        FfiFastModeSlashAction.ENABLE -> setFastMode(true)
                        FfiFastModeSlashAction.DISABLE -> setFastMode(false)
                    }
                    return@launch
                }
                if (preview.isFork) {
                    clearSubmittedDraftIfUnchanged(draft, clearAttachments = false)
                    forkActiveAgent(
                        worktree = preview.forkWorktree,
                        branch = preview.forkBranch,
                        title = null,
                        initialPrompt = preview.forkPrompt,
                    )
                    return@launch
                }
                if (preview.isCompact) {
                    clearSubmittedDraftIfUnchanged(draft, clearAttachments = false)
                    submitSlashCompact(preview.displayPrompt)
                    return@launch
                }

                val item = QueuedPromptItem(
                    text = draft.text,
                    previewText = preview.displayPrompt.lineSequence().firstOrNull()?.trim().orEmpty()
                        .ifBlank { preview.badges.joinToString(" ") { it.label } },
                    mentionedFiles = draft.mentionedFiles,
                    stagedAttachments = draft.stagedAttachments,
                    inlineMentions = draft.inlineMentions,
                    includeGitDiff = draft.includeGitDiff,
                    includeStagedDiff = draft.includeStagedDiff,
                    includeTerminal = draft.includeTerminal,
                    activeSlashCommand = draft.activeSlashCommand,
                    quotedSnippets = draft.quotedSnippets,
                )

                val key = agentQueueKey(hostId, agentId)
                val useServerQueue = cur.selectedHost?.supportsServerQueue == true
                if (!useServerQueue) {
                    runningAgentKeys.add(key)
                }

                _state.update { prev ->
                    val updatedList = prev.queuedPromptsByAgent[key].orEmpty() + item
                    val nextComposer = if (prev.composer.text == draft.text && prev.composer.activeSlashCommand == draft.activeSlashCommand) {
                        prev.composer.copy(
                            text = "",
                            mentionedFiles = emptyList(),
                            stagedAttachments = emptyList(),
                            inlineMentions = emptyList(),
                            includeGitDiff = false,
                            includeStagedDiff = false,
                            includeTerminal = false,
                            activeSlashCommand = null,
                            quotedSnippets = emptyList(),
                        )
                    } else {
                        prev.composer
                    }
                    prev.copy(
                        queuedPromptsByAgent = prev.queuedPromptsByAgent + (key to updatedList),
                        composer = nextComposer,
                        toastMessage = "Queued next prompt",
                    )
                }

                if (useServerQueue) {
                    runCatching {
                        core?.enqueuePrompt(
                            hostId = hostId,
                            agentId = agentId,
                            prompt = preview.wirePrompt,
                            displayPrompt = preview.displayPrompt,
                            badges = preview.badges,
                            queueId = item.id,
                        )
                    }
                }
            } finally {
                isDispatchingPrompt.set(false)
            }
        }
    }

    fun removeQueuedPrompt(itemId: String) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: return
        val key = agentQueueKey(hostId, agentId)
        val useServerQueue = cur.selectedHost?.supportsServerQueue == true
        _state.update { prev ->
            val remaining = prev.queuedPromptsByAgent[key].orEmpty().filterNot { it.id == itemId }
            val nextMap = if (remaining.isEmpty()) {
                prev.queuedPromptsByAgent - key
            } else {
                prev.queuedPromptsByAgent + (key to remaining)
            }
            prev.copy(queuedPromptsByAgent = nextMap)
        }
        if (useServerQueue) {
            viewModelScope.launch(Dispatchers.IO) {
                runCatching { core?.removeQueuedPrompt(hostId, agentId, itemId) }
            }
        }
    }

    fun editQueuedPrompt(itemId: String) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: return
        val key = agentQueueKey(hostId, agentId)
        val useServerQueue = cur.selectedHost?.supportsServerQueue == true
        val target = cur.activeQueuedPrompts.firstOrNull { it.id == itemId } ?: return
        _state.update { prev ->
            val remaining = prev.queuedPromptsByAgent[key].orEmpty().filterNot { it.id == itemId }
            val nextMap = if (remaining.isEmpty()) {
                prev.queuedPromptsByAgent - key
            } else {
                prev.queuedPromptsByAgent + (key to remaining)
            }
            prev.copy(
                queuedPromptsByAgent = nextMap,
                composer = prev.composer.copy(
                    text = target.text,
                    mentionedFiles = target.mentionedFiles,
                    stagedAttachments = target.stagedAttachments,
                    inlineMentions = target.inlineMentions,
                    includeGitDiff = target.includeGitDiff,
                    includeStagedDiff = target.includeStagedDiff,
                    includeTerminal = target.includeTerminal,
                    activeSlashCommand = target.activeSlashCommand,
                    quotedSnippets = target.quotedSnippets,
                ),
            )
        }
        if (useServerQueue) {
            viewModelScope.launch(Dispatchers.IO) {
                runCatching { core?.removeQueuedPrompt(hostId, agentId, itemId) }
            }
        }
    }

    fun sendQueuedPromptNow(itemId: String) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: return
        val key = agentQueueKey(hostId, agentId)
        val useServerQueue = cur.selectedHost?.supportsServerQueue == true
        val target = cur.activeQueuedPrompts.firstOrNull { it.id == itemId } ?: return
        _state.update { prev ->
            val remaining = prev.queuedPromptsByAgent[key].orEmpty().filterNot { it.id == itemId }
            val nextMap = if (remaining.isEmpty()) {
                prev.queuedPromptsByAgent - key
            } else {
                prev.queuedPromptsByAgent + (key to remaining)
            }
            prev.copy(queuedPromptsByAgent = nextMap)
        }
        viewModelScope.launch(Dispatchers.IO) {
            if (useServerQueue) {
                runCatching { core?.flushQueuedPromptNow(hostId, agentId, itemId) }
            } else {
                dispatchQueuedPromptItem(hostId, agentId, target)
            }
        }
    }

    private fun drainNextQueuedPromptIfSettled(hostId: String, agentId: String) {
        val host = _state.value.hosts.firstOrNull { it.hostId == hostId }
        if (host?.supportsServerQueue == true) return
        val key = agentQueueKey(hostId, agentId)
        if (!drainingAgentKeys.add(key)) return
        viewModelScope.launch(Dispatchers.IO) {
            try {
                var popped: QueuedPromptItem? = null
                _state.update { prev ->
                    val currentQueue = prev.queuedPromptsByAgent[key].orEmpty()
                    if (currentQueue.isEmpty()) {
                        prev
                    } else {
                        popped = currentQueue.first()
                        val remaining = currentQueue.drop(1)
                        val nextMap = if (remaining.isEmpty()) {
                            prev.queuedPromptsByAgent - key
                        } else {
                            prev.queuedPromptsByAgent + (key to remaining)
                        }
                        prev.copy(queuedPromptsByAgent = nextMap)
                    }
                }
                val nextItem = popped ?: return@launch
                dispatchQueuedPromptItem(hostId, agentId, nextItem)
            } finally {
                drainingAgentKeys.remove(key)
            }
        }
    }

    private fun matchesLeadingSlashCommand(text: String, command: String): Boolean {
        val trimmed = text.trim()
        if (!trimmed.startsWith(command, ignoreCase = true)) return false
        return trimmed.length == command.length || trimmed[command.length].isWhitespace()
    }

    private fun dispatchQueuedPromptItem(hostId: String, agentId: String, item: QueuedPromptItem) {
        val cur = _state.value
        val key = agentQueueKey(hostId, agentId)
        val needsGitDiff = item.activeSlashCommand.equals("/review", ignoreCase = true) ||
            matchesLeadingSlashCommand(item.text, "/review") ||
            item.includeGitDiff || item.includeStagedDiff ||
            item.inlineMentions.any { it.kind == "git_diff" && item.text.contains(it.token) }
        var effectiveReview = cur.gitReview
        if (needsGitDiff && (effectiveReview == null || !OrbitFormatters.pathsEquivalent(effectiveReview.cwd, cur.activeCwd))) {
            runCatching { core?.fetchGitStatus(hostId, cur.activeCwd) }
            runCatching { core?.fetchGitDiff(hostId, cur.activeCwd, null, item.includeStagedDiff || cur.gitStagedMode) }
            effectiveReview = runCatching { core?.gitReview(hostId, cur.activeCwd, null) }.getOrNull() ?: effectiveReview
        }
        val composed = composePromptViaCore(
            hostId = hostId,
            activeCwd = cur.activeCwd,
            provider = cur.activeAgent?.provider ?: cur.composer.provider,
            gitReview = effectiveReview,
            gitStagedMode = cur.gitStagedMode,
            terminalScreen = cur.terminalScreen,
            text = item.text,
            mentionedFiles = item.mentionedFiles,
            stagedAttachments = item.stagedAttachments,
            inlineMentions = item.inlineMentions,
            includeGitDiff = item.includeGitDiff,
            includeStagedDiff = item.includeStagedDiff,
            includeTerminal = item.includeTerminal,
            activeSlashCommand = item.activeSlashCommand,
            quotedSnippets = item.quotedSnippets,
        ) ?: return
        if (composed.isEmpty || composed.isBtw) return
        try {
            runningAgentKeys.add(key)
            core?.sendPromptWithMeta(
                hostId = hostId,
                agentId = agentId,
                prompt = composed.wirePrompt,
                displayPrompt = composed.displayPrompt,
                badges = composed.badges,
            )
            if (cur.selectedHostId == hostId && cur.selectedAgentId == agentId) {
                reloadStageInternal(hostId, agentId)
            }
        } catch (t: Throwable) {
            _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to send queued prompt")) }
        }
    }

    fun submitComposerPrompt() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val draft = cur.composer
        if (!draft.canSend) return
        if (!isDispatchingPrompt.compareAndSet(false, true)) return

        viewModelScope.launch(Dispatchers.IO) {
            try {
                val preview = composePromptViaCore(
                    hostId = hostId,
                    activeCwd = cur.activeCwd,
                    provider = cur.activeAgent?.provider ?: draft.provider,
                    gitReview = cur.gitReview,
                    gitStagedMode = cur.gitStagedMode,
                    terminalScreen = cur.terminalScreen,
                    text = draft.text,
                    mentionedFiles = draft.mentionedFiles,
                    stagedAttachments = draft.stagedAttachments,
                    inlineMentions = draft.inlineMentions,
                    includeGitDiff = draft.includeGitDiff,
                    includeStagedDiff = draft.includeStagedDiff,
                    includeTerminal = draft.includeTerminal,
                    activeSlashCommand = draft.activeSlashCommand,
                    quotedSnippets = draft.quotedSnippets,
                ) ?: return@launch
                if (preview.isEmpty) return@launch
                if (preview.isBtw) {
                    clearSubmittedDraftIfUnchanged(draft, clearAttachments = false)
                    requestSheet("btw")
                    preview.btwQuestion?.trim()?.takeIf { it.isNotEmpty() }?.let { submitSideQuestion(it) }
                    return@launch
                }
                preview.fastModeAction?.let { action ->
                    clearSubmittedDraftIfUnchanged(draft, clearAttachments = false)
                    when (action) {
                        FfiFastModeSlashAction.TOGGLE -> toggleFastMode()
                        FfiFastModeSlashAction.ENABLE -> setFastMode(true)
                        FfiFastModeSlashAction.DISABLE -> setFastMode(false)
                    }
                    return@launch
                }
                if (preview.isFork) {
                    clearSubmittedDraftIfUnchanged(draft, clearAttachments = false)
                    forkActiveAgent(
                        worktree = preview.forkWorktree,
                        branch = preview.forkBranch,
                        title = null,
                        initialPrompt = preview.forkPrompt,
                    )
                    return@launch
                }
                if (preview.isCompact) {
                    clearSubmittedDraftIfUnchanged(draft, clearAttachments = false)
                    submitSlashCompact(preview.displayPrompt)
                    return@launch
                }

                // Clear composer text & transient attachments before network dispatch
                clearSubmittedDraftIfUnchanged(draft, clearAttachments = true)

                val needsGitDiff = draft.activeSlashCommand.equals("/review", ignoreCase = true) ||
                    matchesLeadingSlashCommand(draft.text, "/review") ||
                    draft.includeGitDiff || draft.includeStagedDiff ||
                    draft.inlineMentions.any { it.kind == "git_diff" && draft.text.contains(it.token) }
                var effectiveReview = cur.gitReview
                val composed = if (needsGitDiff && (effectiveReview == null || !OrbitFormatters.pathsEquivalent(effectiveReview.cwd, cur.activeCwd))) {
                    runCatching { core?.fetchGitStatus(hostId, cur.activeCwd) }
                    runCatching { core?.fetchGitDiff(hostId, cur.activeCwd, null, draft.includeStagedDiff || cur.gitStagedMode) }
                    effectiveReview = runCatching { core?.gitReview(hostId, cur.activeCwd, null) }.getOrNull() ?: effectiveReview
                    composePromptViaCore(
                        hostId = hostId,
                        activeCwd = cur.activeCwd,
                        provider = cur.activeAgent?.provider ?: draft.provider,
                        gitReview = effectiveReview,
                        gitStagedMode = cur.gitStagedMode,
                        terminalScreen = cur.terminalScreen,
                        text = draft.text,
                        mentionedFiles = draft.mentionedFiles,
                        stagedAttachments = draft.stagedAttachments,
                        inlineMentions = draft.inlineMentions,
                        includeGitDiff = draft.includeGitDiff,
                        includeStagedDiff = draft.includeStagedDiff,
                        includeTerminal = draft.includeTerminal,
                        activeSlashCommand = draft.activeSlashCommand,
                        quotedSnippets = draft.quotedSnippets,
                    ) ?: preview
                } else {
                    preview
                }

                val targetAgentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode }
                if (targetAgentId == null) {
                    val createdId = core?.createSession(
                        FfiCreateSessionRequest(
                            hostId = hostId,
                            cwd = cur.activeCwd.ifBlank { "~" },
                            extraDirs = draft.extraDirs,
                            provider = draft.provider,
                            acpAgentId = null,
                            model = draft.model,
                            effort = draft.effort,
                            fastMode = draft.fastMode,
                            mode = draft.mode,
                            initialPrompt = composed.wirePrompt,
                            initialDisplayPrompt = composed.displayPrompt,
                            initialBadges = composed.badges,
                            worktreeBranch = draft.worktreeBranch?.takeIf { it.isNotBlank() }
                                ?: if (cur.draftUseWorktree) "auto" else null,
                        )
                    )
                    if (!createdId.isNullOrBlank()) {
                        runningAgentKeys.add(agentQueueKey(hostId, createdId))
                        _state.update {
                            it.copy(
                                selectedAgentId = createdId,
                                isNewSessionMode = false,
                            )
                        }
                        runCatching { core?.openAgentTimeline(hostId, createdId) }
                        reloadHostDetailInternal(hostId)
                        reloadStageInternal(hostId, createdId)
                    }
                } else if (cur.isActiveAgentRunning || cur.activeAgent?.isAwaitingPermission == true) {
                    val queueId = "q_${System.currentTimeMillis()}_${java.util.UUID.randomUUID().toString().take(6)}"
                    core?.enqueuePrompt(
                        hostId = hostId,
                        agentId = targetAgentId,
                        prompt = composed.wirePrompt,
                        displayPrompt = composed.displayPrompt,
                        badges = composed.badges,
                        queueId = queueId,
                    )
                    reloadHostDetailInternal(hostId)
                    reloadStageInternal(hostId, targetAgentId)
                } else {
                    runningAgentKeys.add(agentQueueKey(hostId, targetAgentId))
                    core?.sendPromptWithMeta(
                        hostId = hostId,
                        agentId = targetAgentId,
                        prompt = composed.wirePrompt,
                        displayPrompt = composed.displayPrompt,
                        badges = composed.badges,
                    )
                    reloadStageInternal(hostId, targetAgentId)
                }
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to send prompt")) }
            } finally {
                isDispatchingPrompt.set(false)
            }
        }
    }

    fun interruptActiveAgent() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.interruptAgent(hostId, agentId) }
        }
    }

    fun resolvePermission(requestId: String, decision: String) {
        if (requestId.startsWith("demo_")) {
            _state.update { prev ->
                prev.copy(
                    demoPermissions = prev.demoPermissions.filterNot {
                        (it as? FfiTimelineItem.PermissionRequest)?.requestId == requestId
                    },
                    toastMessage = "Resolved approval ($decision)",
                )
            }
            return
        }
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.resolvePermission(hostId, agentId, requestId, decision) }
            reloadStageInternal(hostId, agentId)
        }
    }

    fun answerQuestion(requestId: String, answers: List<List<String>>) {
        if (requestId.startsWith("demo_")) {
            _state.update { prev ->
                prev.copy(
                    demoQuestions = prev.demoQuestions.filterNot {
                        (it as? FfiTimelineItem.QuestionRequest)?.requestId == requestId
                    },
                    toastMessage = "Submitted decision response",
                )
            }
            return
        }
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.answerQuestion(hostId, agentId, requestId, answers) }
            reloadStageInternal(hostId, agentId)
        }
    }

    private val stepDescriptorCache = LruCache<String, FfiToolStepDescriptor>(256)

    suspend fun describeStep(item: FfiTimelineItem): FfiToolStepDescriptor? {
        if (item is FfiTimelineItem.ToolCall && item.stepDescriptor != null) {
            return item.stepDescriptor
        }
        val cwd = _state.value.activeCwd
        val cacheKey = if (item is FfiTimelineItem.ToolCall) {
            "$cwd::${item.callId}::${item.seq}::${item.status.name}"
        } else {
            null
        }
        if (cacheKey != null) {
            stepDescriptorCache.get(cacheKey)?.let { return it }
        }
        val described = withContext(Dispatchers.IO) {
            runCatching { core?.describeToolItem(item, cwd) }.getOrNull()
        }
        if (cacheKey != null && described != null) {
            stepDescriptorCache.put(cacheKey, described)
        }
        return described
    }

    // --- Git Diff Inspector ---

    fun loadGitReview(
        cwd: String = _state.value.activeCwd,
        staged: Boolean = _state.value.gitStagedMode,
        path: String? = _state.value.gitSelectedPath,
    ) {
        val hostId = _state.value.selectedHostId ?: return
        _state.update { it.copy(gitStagedMode = staged, gitSelectedPath = path) }
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.fetchGitStatus(hostId, cwd) }
            runCatching { core?.fetchGitDiff(hostId, cwd, null, staged) }
            val review = runCatching { core?.gitReview(hostId, cwd, null) }.getOrNull()
            if (review != null) {
                _state.update { if (it.selectedHostId == hostId && it.activeCwd == cwd) it.copy(gitReview = review) else it }
            }
        }
    }

    fun refreshGitDiff() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        if (cur.isRefreshingGitDiff) return
        _state.update { it.copy(isRefreshingGitDiff = true) }
        viewModelScope.launch(Dispatchers.IO) {
            val startedAt = SystemClock.elapsedRealtime()
            try {
                val cwd = _state.value.activeCwd
                val staged = _state.value.gitStagedMode
                runCatching { core?.fetchGitStatus(hostId, cwd) }
                runCatching { core?.fetchGitDiff(hostId, cwd, null, staged) }
                runCatching { core?.fetchWorkspaceFiles(hostId, cwd) }
                val review = runCatching { core?.gitReview(hostId, cwd, null) }.getOrNull()
                val files = runCatching { core?.workspaceFiles(hostId, cwd) }.getOrNull()
                _state.update { prev ->
                    prev.copy(
                        gitReview = review ?: prev.gitReview,
                        workspaceFiles = files ?: prev.workspaceFiles,
                    )
                }
                val elapsed = SystemClock.elapsedRealtime() - startedAt
                if (elapsed < 350L) {
                    delay(350L - elapsed)
                }
            } finally {
                _state.update { it.copy(isRefreshingGitDiff = false) }
            }
        }
    }

    fun setGitStagedMode(staged: Boolean) {
        loadGitReview(staged = staged)
    }

    fun selectGitFileFilter(path: String?) {
        loadGitReview(path = path)
    }

    fun toggleDiffInspector() {
        val opening = !_state.value.isDiffInspectorOpen
        _state.update { it.copy(isDiffInspectorOpen = opening) }
        if (opening) {
            loadGitReview()
        }
    }

    fun openDiffInspector(path: String? = null) {
        _state.update { it.copy(isDiffInspectorOpen = true, gitSelectedPath = path) }
        loadGitReview(path = path)
    }

    fun closeDiffInspector() {
        _state.update { it.copy(isDiffInspectorOpen = false) }
    }

    // --- Interactive PTY Terminal ---

    fun ensureTerminalStarted(cwd: String = _state.value.activeCwd) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            val detail = runCatching { core?.hostDetail(hostId) }.getOrNull()
            if (detail != null && detail.terminals.isNotEmpty()) {
                val targetId = detail.activeTerminalId ?: detail.terminals.first().terminalId
                runCatching { core?.attachTerminal(hostId, targetId, 1u) }
            } else {
                runCatching { core?.startTerminal(hostId, cwd.ifBlank { "~" }, cur.terminalCols, cur.terminalRows) }
            }
            val scr = runCatching { core?.terminalScreen(hostId) }.getOrNull()
            _state.update { it.copy(terminalScreen = scr) }
        }
    }

    fun spawnNewTerminal(cwd: String = _state.value.activeCwd) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.startTerminal(hostId, cwd.ifBlank { "~" }, cur.terminalCols, cur.terminalRows) }
            reloadHostDetailInternal(hostId)
            val scr = runCatching { core?.terminalScreen(hostId) }.getOrNull()
            _state.update { it.copy(terminalScreen = scr) }
        }
    }

    fun attachTerminalSession(terminalId: String, slot: UByte = 1u) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.attachTerminal(hostId, terminalId, slot) }
            reloadHostDetailInternal(hostId)
            val scr = runCatching { core?.terminalScreen(hostId) }.getOrNull()
            _state.update { it.copy(terminalScreen = scr) }
        }
    }

    fun killTerminalSession(terminalId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.killTerminal(hostId, terminalId) }
            reloadHostDetailInternal(hostId)
            val scr = runCatching { core?.terminalScreen(hostId) }.getOrNull()
            _state.update { it.copy(terminalScreen = scr) }
        }
    }

    fun onTerminalViewportMeasured(cols: Int, rows: Int) {
        val clampedCols = cols.coerceIn(24, 240).toUShort()
        val clampedRows = rows.coerceIn(6, 120).toUShort()
        val cur = _state.value
        if (cur.terminalCols == clampedCols && cur.terminalRows == clampedRows) return
        _state.update { it.copy(terminalCols = clampedCols, terminalRows = clampedRows) }
        val hostId = cur.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.resizeTerminal(hostId, clampedCols, clampedRows) }
            val scr = runCatching { core?.terminalScreen(hostId) }.getOrNull()
            if (scr != null) {
                _state.update { it.copy(terminalScreen = scr) }
            }
        }
    }

    fun sendTerminalKeys(keys: String) {
        val hostId = _state.value.selectedHostId ?: return
        if (keys.isEmpty()) return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.sendTerminalKeys(hostId, keys) }
        }
    }

    fun sendTerminalBytes(bytes: ByteArray) {
        val current = _state.value
        val hostId = current.selectedHostId ?: return
        val slot: UByte = current.hostDetail?.takeIf { it.host.hostId == hostId }?.activeTerminalSlot?.coerceAtLeast(1u) ?: 1u
        if (bytes.isEmpty()) return
        if (terminalInput.trySend(Triple(hostId, slot, bytes)).isFailure) {
            postToast(getApplication<Application>().getString(dev.orbit.android.R.string.terminal_busy))
        }
    }

    fun refreshQuotasAndModels(forceModels: Boolean = false) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.refreshQuotas(hostId) }
            runCatching { core?.refreshProviderModels(hostId, forceModels) }
            reloadHostDetailInternal(hostId)
        }
    }

    private val diagramCache = LruCache<String, FfiRenderDiagramView>(96)

    suspend fun renderDiagram(
        kind: FfiDiagramKind,
        source: String,
        isDark: Boolean,
    ): FfiRenderDiagramView? {
        val cacheKey = "${kind.name}::${if (isDark) "dark" else "light"}::$source"
        diagramCache.get(cacheKey)?.let { return it }
        val activeCore = core ?: coreReady.await() ?: return null
        val theme = if (isDark) FfiDiagramTheme.DARK else FfiDiagramTheme.LIGHT
        val rendered = withContext(Dispatchers.Default) {
            runCatching {
                activeCore.renderDiagram(
                    FfiRenderDiagramRequest(
                        kind = kind,
                        source = source,
                        theme = theme,
                    )
                )
            }.getOrNull()
        } ?: return null
        diagramCache.put(cacheKey, rendered)
        return rendered
    }

    // --- Automations (Schedules, Timers, Goals, Monitors, Workflows) ---

    fun refreshAutomations() {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.refreshHost(hostId) }
            runCatching { core?.listConditionalTimers(hostId, null, true) }
            runCatching { core?.listGoalRuns(hostId, null) }
            runCatching { core?.listMonitorWatches(hostId, null) }
            runCatching { core?.listWorkflowRuns(hostId, true) }
            reloadHostDetailIfSelected(hostId)
        }
        refreshPermissionRulesAndGuardian()
    }

    fun createSchedule(
        name: String,
        cronExpr: String,
        prompt: String,
        cwd: String,
        provider: FfiProviderKind,
        mode: FfiSessionMode,
        worktree: Boolean,
        archiveOnFinish: Boolean,
    ) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.createSchedule(
                    FfiCreateScheduleRequest(
                        hostId = hostId,
                        name = name,
                        cronExpr = cronExpr,
                        timezone = null,
                        prompt = prompt,
                        cwd = cwd.ifBlank { "~" },
                        workspaceId = _state.value.activeWorkspaceId,
                        provider = provider,
                        acpAgentId = null,
                        model = _state.value.composer.model,
                        effort = _state.value.composer.effort,
                        fastMode = _state.value.composer.fastMode,
                        mode = mode,
                        worktree = worktree,
                        archiveOnFinish = archiveOnFinish,
                        maxRuns = null,
                    )
                )
            }.onSuccess {
                postToast("Created automation \"$name\"")
                reloadHostDetailIfSelected(hostId)
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to create automation"))
            }
        }
    }

    fun pauseSchedule(scheduleId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.pauseSchedule(hostId, scheduleId)
            }.onSuccess {
                reloadHostDetailIfSelected(hostId)
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to pause automation"))
            }
        }
    }

    fun resumeSchedule(scheduleId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.resumeSchedule(hostId, scheduleId)
            }.onSuccess {
                reloadHostDetailIfSelected(hostId)
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to resume automation"))
            }
        }
    }

    fun runScheduleOnce(scheduleId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.runScheduleOnce(hostId, scheduleId)
            }.onSuccess {
                postToast("Triggered automation run")
                reloadHostDetailIfSelected(hostId)
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to trigger automation"))
            }
        }
    }

    fun deleteSchedule(scheduleId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.deleteSchedule(hostId, scheduleId)
            }.onSuccess {
                postToast("Deleted automation")
                reloadHostDetailIfSelected(hostId)
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to delete automation"))
            }
        }
    }

    fun fetchScheduleRuns(scheduleId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.fetchScheduleRuns(hostId, scheduleId, 16u) }
                .onFailure { t -> postToast(extractErrorMessage(t, "Failed to fetch run history")) }
            delay(250)
            if (_state.value.selectedHostId != hostId) return@launch
            val runs = runCatching { core?.scheduleRuns(hostId, scheduleId) }.getOrNull().orEmpty()
            _state.update { prev ->
                prev.copy(scheduleRunsById = prev.scheduleRunsById + (scheduleId to runs))
            }
        }
    }

    fun cancelConditionalTimer(timerId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.cancelConditionalTimer(hostId, timerId)
                postToast("Timer canceled")
                reloadHostDetailIfSelected(hostId)
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to cancel timer"))
            }
        }
    }

    fun fireConditionalTimerNow(timerId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.fireConditionalTimerNow(hostId, timerId)
                postToast("Timer fired")
                reloadHostDetailIfSelected(hostId)
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to fire timer"))
            }
        }
    }

    fun controlGoalRun(goalId: String, action: FfiGoalControlAction) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.controlGoalRun(hostId, goalId, action)
                val label = when (action) {
                    FfiGoalControlAction.PAUSE -> "Goal paused"
                    FfiGoalControlAction.RESUME -> "Goal resumed"
                    FfiGoalControlAction.CANCEL -> "Goal stopped"
                    FfiGoalControlAction.VERIFY_NOW -> "Verification triggered"
                }
                postToast(label)
                reloadHostDetailIfSelected(hostId)
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to update goal"))
            }
        }
    }

    fun stopMonitorWatch(monitorId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.stopMonitorWatch(hostId, monitorId)
                postToast("Monitor stopped")
                reloadHostDetailIfSelected(hostId)
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to stop monitor"))
            }
        }
    }

    fun cancelWorkflowRun(runId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.cancelWorkflowRun(hostId, runId)
                postToast("Workflow canceled")
                reloadHostDetailIfSelected(hostId)
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to cancel workflow"))
            }
        }
    }

    // --- Workspace Shared Board & Sibling Peers ---

    fun fetchWorkspaceBoardAndEvents(workspaceId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.fetchWorkspaceBoard(hostId, workspaceId) }
            runCatching { core?.fetchWorkspaceEvents(hostId, workspaceId, 40u) }
            delay(200)
            reloadHostDetailIfSelected(hostId)
        }
    }

    fun updateWorkspaceGoal(workspaceId: String, goal: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.updateWorkspaceGoal(hostId, workspaceId, goal)
            }.onSuccess {
                postToast("Mission goal updated")
                reloadHostDetailIfSelected(hostId)
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to update mission goal"))
            }
        }
    }

    fun updateWorkspaceBoard(workspaceId: String, content: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.updateWorkspaceBoard(hostId, workspaceId, content, null, "android")
            }.onSuccess {
                postToast("Board saved")
                reloadHostDetailIfSelected(hostId)
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to save board"))
            }
        }
    }

    fun spawnWorkspacePeer(
        workspaceId: String,
        handle: String,
        prompt: String,
        worktree: Boolean,
    ) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val wsCwd = cur.hostDetail?.workspaces?.firstOrNull { it.workspaceId == workspaceId }?.cwd
            ?: cur.activeCwd
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.spawnWorkspacePeer(
                    FfiSpawnWorkspacePeerRequest(
                        hostId = hostId,
                        workspaceId = workspaceId,
                        cwd = wsCwd.ifBlank { "~" },
                        extraDirs = emptyList(),
                        provider = cur.composer.provider,
                        acpAgentId = null,
                        model = cur.composer.model,
                        effort = cur.composer.effort,
                        fastMode = cur.composer.fastMode,
                        mode = cur.composer.mode,
                        initialPrompt = prompt,
                        worktreeBranch = if (worktree) "auto" else null,
                        handle = handle.trim().ifEmpty { null },
                        profileId = null,
                    )
                )
            }.onSuccess { spawnedId ->
                if (spawnedId.isNotBlank()) {
                    postToast("Spawned peer ${handle.ifBlank { "session" }}")
                    reloadHostDetailIfSelected(hostId)
                }
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to spawn peer"))
            }
        }
    }

    // --- Side Question (`/btw`) ---

    fun setSideQuestionDraft(text: String) {
        val scopeKey = _state.value.activeSideQuestionScopeKey
        _state.update { prev ->
            prev.copy(sideQuestionDraftsByScope = prev.sideQuestionDraftsByScope + (scopeKey to text))
        }
    }

    fun clearSideQuestions() {
        val scopeKey = _state.value.activeSideQuestionScopeKey
        _state.update { prev ->
            prev.copy(sideQuestionsByScope = prev.sideQuestionsByScope - scopeKey)
        }
    }

    fun promoteSideQuestionToComposer(entry: SideQuestionEntry) {
        val excerpt = buildString {
            appendLine("Side Q: ${entry.question}")
            append(entry.answer.trim())
        }
        addQuotedAssistantReply(excerpt)
    }

    fun submitSideQuestion(question: String) {
        val trimmed = question.trim()
        if (trimmed.isEmpty()) return
        val cur = _state.value
        val hostId = cur.selectedHostId ?: run {
            postToast("Select a host first to ask a side question")
            return
        }
        val scopeKey = cur.activeSideQuestionScopeKey
        val entryId = UUID.randomUUID().toString()
        val initialEntry = SideQuestionEntry(
            id = entryId,
            question = trimmed,
            answer = "",
            isStreaming = true,
        )
        _state.update { prev ->
            val list = prev.sideQuestionsByScope[scopeKey].orEmpty() + initialEntry
            prev.copy(sideQuestionsByScope = prev.sideQuestionsByScope + (scopeKey to list))
        }

        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode }
        val cwd = cur.activeCwd.ifBlank { "~" }
        val provider = cur.activeAgent?.provider ?: cur.composer.provider
        val model = cur.activeAgent?.model ?: cur.composer.model

        viewModelScope.launch(Dispatchers.IO) {
            val sideListener = object : SideQuestionListener {
                override fun onSideQuestionEvent(event: FfiSideQuestionEvent) {
                    when (event) {
                        is FfiSideQuestionEvent.Delta -> {
                            _state.update { prev ->
                                val list = prev.sideQuestionsByScope[scopeKey].orEmpty().map { item ->
                                    if (item.id == entryId) {
                                        item.copy(answer = item.answer + event.textDelta, isStreaming = true)
                                    } else {
                                        item
                                    }
                                }
                                prev.copy(sideQuestionsByScope = prev.sideQuestionsByScope + (scopeKey to list))
                            }
                        }
                        is FfiSideQuestionEvent.Done -> {
                            _state.update { prev ->
                                val list = prev.sideQuestionsByScope[scopeKey].orEmpty().map { item ->
                                    if (item.id == entryId) {
                                        item.copy(
                                            answer = event.fullText.ifBlank { item.answer },
                                            isStreaming = false,
                                            error = event.error?.takeIf { it.isNotBlank() },
                                        )
                                    } else {
                                        item
                                    }
                                }
                                prev.copy(sideQuestionsByScope = prev.sideQuestionsByScope + (scopeKey to list))
                            }
                        }
                    }
                }
            }

            try {
                val contextSummary = if (agentId != null) {
                    runCatching { core?.buildSideQuestionContext(hostId, agentId, 3000u) }.getOrNull().orEmpty()
                } else {
                    ""
                }
                val answer = core?.askSideQuestion(
                    FfiAskSideQuestionRequest(
                        hostId = hostId,
                        agentId = agentId,
                        cwd = cwd,
                        provider = provider,
                        model = model,
                        question = trimmed,
                        contextSummary = contextSummary,
                    ),
                    sideListener,
                )
                if (answer != null) {
                    _state.update { prev ->
                        val list = prev.sideQuestionsByScope[scopeKey].orEmpty().map { item ->
                            if (item.id == entryId) {
                                item.copy(answer = answer.ifBlank { item.answer }, isStreaming = false)
                            } else {
                                item
                            }
                        }
                        prev.copy(sideQuestionsByScope = prev.sideQuestionsByScope + (scopeKey to list))
                    }
                }
            } catch (t: Throwable) {
                val msg = extractErrorMessage(t, "Side question failed")
                _state.update { prev ->
                    val list = prev.sideQuestionsByScope[scopeKey].orEmpty().map { item ->
                        if (item.id == entryId) {
                            item.copy(isStreaming = false, error = msg)
                        } else {
                            item
                        }
                    }
                    prev.copy(sideQuestionsByScope = prev.sideQuestionsByScope + (scopeKey to list))
                }
            }
        }
    }

    // --- Archived Sessions ---

    fun unarchiveAgent(agentId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.unarchiveAgent(hostId, agentId)
            }.onSuccess {
                if (_state.value.selectedHostId == hostId) {
                    reloadHostDetailInternal(hostId)
                    selectAgent(agentId)
                }
                postToast("Session restored")
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to restore session"))
            }
        }
    }

    fun deleteAgentPermanently(agentId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                activeCore.deleteAgent(hostId, agentId)
            }.onSuccess {
                reloadHostDetailIfSelected(hostId)
                postToast("Session deleted")
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to delete session"))
            }
        }
    }

    // --- Fork / Branch Conversation & Markdown Export ---

    fun forkActiveAgent(
        worktree: Boolean,
        branch: String?,
        title: String?,
        initialPrompt: String?,
    ) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val sourceAgentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: run {
            postToast("Open an active session first to fork")
            return
        }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                val forked = activeCore.forkAgentRequest(
                    FfiForkAgentRequest(
                        hostId = hostId,
                        sourceAgentId = sourceAgentId,
                        upToSeq = null,
                        worktree = worktree,
                        branch = branch?.trim()?.ifEmpty { null },
                        baseRef = null,
                        title = title?.trim()?.ifEmpty { null },
                        initialPrompt = initialPrompt?.trim()?.ifEmpty { null },
                    )
                )
                if (_state.value.selectedHostId == hostId) {
                    reloadHostDetailInternal(hostId)
                    selectAgent(forked.agent.agentId)
                }
                postToast("Forked conversation")
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to fork conversation")) }
            }
        }
    }

    fun exportActiveTranscriptMarkdown() {
        exportActiveTranscript(FfiExportFormat.MARKDOWN)
    }

    fun exportActiveTranscript(format: FfiExportFormat) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: run {
            postToast("Open an active session first to export")
            return
        }
        val app = getApplication<Application>()
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                val exported = activeCore.exportAgentTranscript(
                    hostId = hostId,
                    agentId = agentId,
                    options = FfiExportOptions(
                        format = format,
                        includeThinking = false,
                        includeToolCalls = true,
                        includeTimestamps = true,
                    ),
                )
                val exportDir = File(app.cacheDir, "orbit_exports").apply { mkdirs() }
                val nowMs = System.currentTimeMillis()
                exportDir.listFiles()
                    ?.sortedByDescending { it.lastModified() }
                    ?.forEachIndexed { idx, existing ->
                        if (idx >= 8 || (nowMs - existing.lastModified()) > 3_600_000L) {
                            runCatching { existing.delete() }
                        }
                    }

                val ext = when (format) {
                    FfiExportFormat.MARKDOWN -> "md"
                    FfiExportFormat.JSON -> "json"
                    FfiExportFormat.HTML -> "html"
                }
                val sanitizedStem = exported.suggestedFilename
                    .substringAfterLast('/')
                    .substringAfterLast('\\')
                    .replace(Regex("[^A-Za-z0-9._-]"), "_")
                    .trimStart('.')
                    .removeSuffix(".$ext")
                    .ifBlank { "orbit_transcript" }
                    .take(80)
                val safeName = "$sanitizedStem.$ext"
                val outFile = File(exportDir, safeName)
                require(outFile.canonicalFile.parentFile == exportDir.canonicalFile) {
                    "Refused unsafe export path"
                }
                outFile.writeText(exported.content, Charsets.UTF_8)
                val uri = FileProvider.getUriForFile(
                    app,
                    "${app.packageName}.fileprovider",
                    outFile,
                )
                val fallbackMime = when (format) {
                    FfiExportFormat.MARKDOWN -> "text/markdown"
                    FfiExportFormat.JSON -> "application/json"
                    FfiExportFormat.HTML -> "text/html"
                }
                val shareIntent = Intent(Intent.ACTION_SEND).apply {
                    type = exported.mimeType.ifBlank { fallbackMime }
                    putExtra(Intent.EXTRA_STREAM, uri)
                    putExtra(Intent.EXTRA_SUBJECT, safeName)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                withContext(Dispatchers.Main) {
                    app.startActivity(
                        Intent.createChooser(shareIntent, "Export Transcript")
                            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    )
                }
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to export transcript"))
            }
        }
    }

    // --- Cloud Sync (GitHub Secret Gist E2EE) ---

    fun refreshCloudSyncStatus() {
        viewModelScope.launch(Dispatchers.IO) {
            val st = runCatching { core?.cloudSyncStatus() }.getOrNull()
            if (st != null) {
                _state.update { it.copy(cloudSyncStatus = st) }
            }
        }
    }

    suspend fun generateCloudSyncPassphrase(): String = withContext(Dispatchers.IO) {
        runCatching { core?.cloudSyncGeneratePassphrase() }.getOrNull().orEmpty()
    }

    fun startGithubCloudLogin(clientIdOverride: String?) {
        cloudSyncPollJob?.cancel()
        _state.update { it.copy(cloudSyncBusy = true, cloudSyncError = null) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                if (!clientIdOverride.isNullOrBlank()) {
                    runCatching { core?.cloudSyncSetGithubClientId(clientIdOverride) }
                }
                val flow = core?.cloudSyncStartGithubDeviceFlow(clientIdOverride)
                    ?: throw IllegalStateException("Core not ready")
                _state.update { it.copy(cloudSyncDeviceFlow = flow, cloudSyncBusy = false) }
                cloudSyncPollJob = viewModelScope.launch(Dispatchers.IO) {
                    var intervalMs = (flow.intervalSecs.toLong().coerceAtLeast(5L)) * 1000L
                    val deadlineMs = SystemClock.elapsedRealtime() + flow.expiresInSecs.toLong() * 1000L
                    var completed = false
                    while (SystemClock.elapsedRealtime() < deadlineMs) {
                        delay(intervalMs)
                        val poll = runCatching {
                            core?.cloudSyncPollGithubDeviceFlow(flow.deviceCode, clientIdOverride)
                        }.getOrNull() ?: continue
                        when (poll.status) {
                            FfiCloudSyncDevicePollStatus.PENDING -> Unit
                            FfiCloudSyncDevicePollStatus.SLOW_DOWN -> intervalMs += 5_000L
                            FfiCloudSyncDevicePollStatus.AUTHORIZED -> {
                                completed = true
                                val nextStatus = poll.syncStatus ?: runCatching { core?.cloudSyncStatus() }.getOrNull()
                                _state.update {
                                    it.copy(
                                        cloudSyncDeviceFlow = null,
                                        cloudSyncStatus = nextStatus ?: it.cloudSyncStatus,
                                        cloudSyncError = null,
                                    )
                                }
                                postToast("Signed in to GitHub")
                                break
                            }
                            FfiCloudSyncDevicePollStatus.EXPIRED,
                            FfiCloudSyncDevicePollStatus.DENIED -> {
                                completed = true
                                _state.update {
                                    it.copy(
                                        cloudSyncDeviceFlow = null,
                                        cloudSyncError = poll.error ?: "GitHub authorization did not complete.",
                                    )
                                }
                                break
                            }
                        }
                    }
                    if (!completed) {
                        _state.update {
                            it.copy(
                                cloudSyncDeviceFlow = null,
                                cloudSyncError = "GitHub authorization code expired. Please try again.",
                            )
                        }
                    }
                }
            } catch (t: Throwable) {
                _state.update {
                    it.copy(
                        cloudSyncBusy = false,
                        cloudSyncError = extractErrorMessage(t, "Failed to start GitHub login"),
                    )
                }
            }
        }
    }

    fun cancelGithubCloudLogin() {
        cloudSyncPollJob?.cancel()
        cloudSyncPollJob = null
        _state.update { it.copy(cloudSyncDeviceFlow = null, cloudSyncBusy = false) }
    }

    fun loginCloudSyncWithToken(token: String) {
        val clean = token.trim()
        if (clean.isEmpty()) return
        _state.update { it.copy(cloudSyncBusy = true, cloudSyncError = null) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val st = core?.cloudSyncLoginWithToken(clean)
                _state.update { it.copy(cloudSyncStatus = st ?: it.cloudSyncStatus, cloudSyncBusy = false) }
                postToast("GitHub token verified")
            } catch (t: Throwable) {
                _state.update {
                    it.copy(
                        cloudSyncBusy = false,
                        cloudSyncError = extractErrorMessage(t, "Failed to verify GitHub token"),
                    )
                }
            }
        }
    }

    fun unlockCloudSyncWithPassphrase(passphrase: String) {
        val clean = passphrase.trim()
        if (clean.isEmpty()) return
        _state.update { it.copy(cloudSyncBusy = true, cloudSyncError = null) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val res = core?.cloudSyncUnlockWithPassphrase(clean)
                val st = runCatching { core?.cloudSyncStatus() }.getOrNull()
                _state.update { it.copy(cloudSyncStatus = st ?: it.cloudSyncStatus, cloudSyncBusy = false) }
                reloadFleetInternal()
                _state.value.selectedHostId?.let { reloadHostDetailInternal(it) }
                val summary = res?.let { "Synced rev #${it.vaultRevision}" } ?: "Cloud vault unlocked"
                postToast(summary)
            } catch (t: Throwable) {
                _state.update {
                    it.copy(
                        cloudSyncBusy = false,
                        cloudSyncError = extractErrorMessage(t, "Failed to unlock cloud vault"),
                    )
                }
            }
        }
    }

    fun syncCloudNow() {
        _state.update { it.copy(cloudSyncBusy = true, cloudSyncError = null) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val res = core?.cloudSyncNow()
                val st = runCatching { core?.cloudSyncStatus() }.getOrNull()
                _state.update { it.copy(cloudSyncStatus = st ?: it.cloudSyncStatus, cloudSyncBusy = false) }
                reloadFleetInternal()
                _state.value.selectedHostId?.let { reloadHostDetailInternal(it) }
                val summary = res?.let { "Synced · rev #${it.vaultRevision}" } ?: "Cloud sync complete"
                postToast(summary)
            } catch (t: Throwable) {
                _state.update {
                    it.copy(
                        cloudSyncBusy = false,
                        cloudSyncError = extractErrorMessage(t, "Cloud sync failed"),
                    )
                }
            }
        }
    }

    fun setCloudAutoSync(enabled: Boolean) {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.cloudSyncSetAutoSync(enabled) }
                .onSuccess { st ->
                    if (st != null) {
                        _state.update { it.copy(cloudSyncStatus = st) }
                    }
                }
        }
    }

    fun logoutCloudSync(clearLocalHosts: Boolean = false) {
        cancelGithubCloudLogin()
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val st = core?.cloudSyncLogout(clearLocalHosts)
                _state.update {
                    it.copy(
                        cloudSyncStatus = st ?: it.cloudSyncStatus,
                        cloudSyncError = null,
                    )
                }
                if (clearLocalHosts) {
                    reloadFleetInternal()
                }
                postToast("Disconnected Cloud Sync")
            } catch (t: Throwable) {
                _state.update { it.copy(cloudSyncError = extractErrorMessage(t, "Failed to disconnect")) }
            }
        }
    }

    // --- Zero-Passphrase Cloud Sync & 1-Scan Device Link QR ---

    fun setCloudManualPassphraseMode(requireManual: Boolean) {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.cloudSyncSetManualPassphraseMode(requireManual) }
                .onSuccess { st ->
                    if (st != null) {
                        _state.update { it.copy(cloudSyncStatus = st) }
                    }
                }
                .onFailure { t ->
                    _state.update { it.copy(cloudSyncError = extractErrorMessage(t, "Failed to update unlock mode")) }
                }
        }
    }

    suspend fun exportCloudSyncDeviceLinkUri(): String = withContext(Dispatchers.IO) {
        checkNotNull(core ?: coreReady.await()).cloudSyncExportDeviceLinkUri()
    }

    fun importCloudSyncDeviceLinkUri(uri: String, onSuccess: (() -> Unit)? = null) {
        val clean = uri.trim()
        if (clean.isEmpty()) return
        _state.update { it.copy(cloudSyncBusy = true, cloudSyncError = null) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val readyCore = core ?: coreReady.await() ?: throw IllegalStateException("Orbit core not ready")
                val res = readyCore.cloudSyncImportDeviceLinkUri(clean)
                val st = runCatching { readyCore.cloudSyncStatus() }.getOrNull()
                _state.update {
                    it.copy(
                        cloudSyncStatus = st ?: it.cloudSyncStatus,
                        cloudSyncBusy = false,
                        cloudSyncError = null,
                    )
                }
                reloadFleetInternal()
                _state.value.selectedHostId?.let { reloadHostDetailInternal(it) }
                postToast("Device linked · rev #${res.vaultRevision}")
                onSuccess?.let { viewModelScope.launch(Dispatchers.Main) { it() } }
            } catch (t: Throwable) {
                val msg = extractErrorMessage(t, "Failed to import Device Link URI")
                _state.update { it.copy(cloudSyncBusy = false, cloudSyncError = msg, lastError = msg) }
            }
        }
    }

    // --- Real Push Notification Device Registration (`registerPushDevice` UniFFI RPC) ---

    private val registrationMutex = kotlinx.coroutines.sync.Mutex()
    private val registeredHostTokens = mutableMapOf<String, String>()

    fun registerPushToken(platform: String, token: String) {
        val cleanToken = token.trim()
        if (cleanToken.isEmpty()) return
        viewModelScope.launch(Dispatchers.IO) {
            val activeCore = core ?: coreReady.await() ?: return@launch
            val app = getApplication<Application>()
            val deviceId = OrbitRepository.getOrCreateDeviceId(app)
            val deviceName = OrbitRepository.deviceDisplayName()
            // Persist local push token metadata first
            runCatching { activeCore.registerPushToken(platform, cleanToken) }
                .onSuccess { st -> _state.update { it.copy(pushRegistrationStatus = st) } }
            // Register across all paired hosts via canonical daemon RPC `registerPushDevice`
            val hostIds = _state.value.hosts.map { it.hostId }
            val req = FfiRegisterPushDeviceRequest(
                deviceId = deviceId,
                platform = FfiPushPlatform.FCM,
                deviceName = deviceName,
                deviceToken = cleanToken,
                sandbox = BuildConfig.DEBUG,
                liveActivityToken = null,
            )
            registrationMutex.withLock {
                hostIds.forEach { hostId ->
                    if (registeredHostTokens[hostId] != cleanToken) {
                        runCatching { activeCore.registerPushDevice(hostId, req) }.onSuccess { snapshot ->
                            registeredHostTokens[hostId] = cleanToken
                            _state.update { if (it.selectedHostId == hostId) it.copy(pushDevicesSnapshot = snapshot) else it }
                        }
                    }
                }
            }
        }
    }

    fun refreshPushDevices(hostId: String? = _state.value.selectedHostId) {
        val targetHost = hostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            val activeCore = core ?: return@launch
            runCatching { activeCore.listPushDevices(targetHost) }
                .onSuccess { snap -> _state.update { if (it.selectedHostId == targetHost) it.copy(pushDevicesSnapshot = snap) else it } }
        }
    }

    fun updateHostPushPreferences(prefsUpdate: PushPreferences) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            val activeCore = core ?: return@launch
            runCatching {
                activeCore.updatePushPreferences(
                    hostId = hostId,
                    req = UpdatePushPreferencesRequest(
                        preferences = prefsUpdate,
                        gatewayAuthToken = null,
                        keepExistingGatewayToken = true,
                    ),
                )
            }.onSuccess { snap ->
                _state.update { it.copy(pushDevicesSnapshot = snap) }
                postToast("Push preferences saved")
            }.onFailure { t ->
                postToast(extractErrorMessage(t, "Failed to save push preferences"))
            }
        }
    }

    fun sendTestPushNotification() {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            val activeCore = core ?: return@launch
            val deviceId = OrbitRepository.getOrCreateDeviceId(getApplication())
            runCatching { activeCore.testPushNotification(hostId, deviceId) }
                .onSuccess { res -> postToast(res.detail.ifBlank { "Test push sent (${res.delivered}/${res.attempted})" }) }
                .onFailure { t -> postToast(extractErrorMessage(t, "Test push failed")) }
        }
    }

    fun unregisterPushToken() {
        viewModelScope.launch(Dispatchers.IO) {
            val activeCore = core
            val deviceId = OrbitRepository.getOrCreateDeviceId(getApplication())
            _state.value.hosts.forEach { host ->
                runCatching { activeCore?.unregisterPushDevice(host.hostId, deviceId) }
            }
            registrationMutex.withLock { registeredHostTokens.clear() }
            runCatching { activeCore?.unregisterPushToken(OrbitFirebaseMessagingBridge.PLATFORM_ANDROID_FCM) }
            OrbitFirebaseMessagingBridge.clearToken(getApplication())
            val st = runCatching { activeCore?.pushRegistrationStatus() }.getOrNull()
            _state.update { it.copy(pushRegistrationStatus = st, pushDevicesSnapshot = null) }
        }
    }

    // --- Global Overview, Safe Batch Approvals & Personal Agent Dispatch ---

    fun openGlobalOverview() {
        _state.update { it.copy(showGlobalOverview = true, lastError = null) }
        refreshRouterAndMesh()
        refreshOverviewPendingDecisions()
    }

    fun closeGlobalOverview() {
        _state.update { it.copy(showGlobalOverview = false) }
    }

    private val overviewGeneration = AtomicLong()
    private val guardianAuditCache = ConcurrentHashMap<String, Pair<Long, List<GuardianAuditEntry>>>()

    fun refreshOverviewPendingDecisions() {
        val generation = overviewGeneration.incrementAndGet()
        viewModelScope.launch(Dispatchers.IO) {
            val activeCore = core ?: return@launch
            val cur = _state.value
            val hostNameById = cur.hosts.associate { it.hostId to it.displayName }

            val perHostResults = coroutineScope {
                cur.hosts.map { host ->
                    async(Dispatchers.IO) {
                        val detail = runCatching { activeCore.hostDetail(host.hostId) }.getOrNull()
                            ?: return@async (emptyList<Pair<String, FfiAgentCard>>() to emptyList<OverviewPendingDecision>())
                        val hostSessions = detail.agents
                            .filter { it.archivedAtRfc3339 == null }
                            .map { host.hostId to it }
                        val awaitingAgents = detail.agents.filter { it.isAwaitingPermission }
                        val audits = if (awaitingAgents.isNotEmpty()) {
                            val nowMs = SystemClock.elapsedRealtime()
                            val cached = guardianAuditCache[host.hostId]
                            if (cached != null && (nowMs - cached.first) < GUARDIAN_AUDIT_CACHE_TTL_MS) {
                                cached.second
                            } else {
                                val fetched = withTimeoutOrNull(GUARDIAN_CONFIG_RPC_TIMEOUT_MS) {
                                    runCatching { activeCore.getGuardianConfig(host.hostId).recentAudits }.getOrNull()
                                }
                                if (fetched != null) {
                                    guardianAuditCache[host.hostId] = nowMs to fetched
                                    fetched
                                } else {
                                    cached?.second.orEmpty()
                                }
                            }
                        } else {
                            emptyList()
                        }

                        val hostDecisions = mutableListOf<OverviewPendingDecision>()
                        awaitingAgents.forEach { agent ->
                            val delta = runCatching {
                                activeCore.agentStageDelta(host.hostId, agent.agentId, 0uL)
                            }.getOrNull() ?: return@forEach
                            delta.pendingPermissions
                                .filterIsInstance<FfiTimelineItem.PermissionRequest>()
                                .filter { it.resolvedDecision == null }
                                .forEach { perm ->
                                    val assessment = audits
                                        .filter { it.agentId == agent.agentId && it.requestId == perm.requestId }
                                        .maxByOrNull { it.decidedAtMs }
                                        ?.assessment
                                    hostDecisions.add(
                                        OverviewPendingDecision(
                                            hostId = host.hostId,
                                            hostName = hostNameById[host.hostId] ?: host.hostId,
                                            agentId = agent.agentId,
                                            agentTitle = agent.title.ifBlank {
                                                OrbitFormatters.providerLabel(agent.provider)
                                            },
                                            permission = perm,
                                            isSafeLowRisk = OverviewSafetyReducer.isSafeLowRiskBatchCandidate(
                                                perm,
                                                assessment,
                                            ),
                                            isPlanApproval = PlanApprovalReducer.detectPlanApproval(perm) != null,
                                            riskLevel = assessment?.riskLevel,
                                        )
                                    )
                                }
                        }
                        hostSessions to hostDecisions
                    }
                }.awaitAll()
            }

            val sessions = mutableListOf<Pair<String, FfiAgentCard>>()
            val collected = mutableListOf<OverviewPendingDecision>()
            val seenKeys = HashSet<Triple<String, String, String>>()
            perHostResults.forEach { (hostSessions, hostDecisions) ->
                sessions.addAll(hostSessions)
                hostDecisions.forEach { decision ->
                    val key = Triple(decision.hostId, decision.agentId, decision.permission.requestId)
                    if (seenKeys.add(key)) {
                        collected.add(decision)
                    }
                }
            }
            val recent = sessions.sortedByDescending { it.second.updatedAtRfc3339 }
            _state.update {
                if (overviewGeneration.get() == generation) {
                    it.copy(overviewPendingDecisions = collected, fleetSessions = recent)
                } else {
                    it
                }
            }
        }
    }

    fun resolvePermissionForHost(
        hostId: String,
        agentId: String,
        requestId: String,
        decision: String,
    ) {
        viewModelScope.launch(Dispatchers.IO) {
            try {
                guardianAuditCache.remove(hostId)
                core?.resolvePermission(hostId, agentId, requestId, decision)
                reloadFleetInternal()
                reloadHostDetailInternal(hostId)
                if (_state.value.selectedHostId == hostId && _state.value.selectedAgentId == agentId) {
                    reloadStageInternal(hostId, agentId)
                }
                refreshOverviewPendingDecisions()
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to resolve approval"))
            }
        }
    }

    fun selectHostAndAgent(hostId: String, agentId: String) {
        _state.update { it.copy(showGlobalOverview = false) }
        if (_state.value.selectedHostId == hostId) {
            selectAgent(agentId)
        } else {
            openThreadFromDeepLink(hostId, agentId)
        }
    }

    fun resolvePermissionsBatchSafe(
        hostId: String,
        permissionsByAgent: List<Pair<String, FfiTimelineItem.PermissionRequest>>,
    ) {
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val activeCore = core ?: return@launch
                guardianAuditCache.remove(hostId)
                val audits = activeCore.getGuardianConfig(hostId).recentAudits
                val safeItems = permissionsByAgent.groupBy { it.first }.flatMap { (agentId, rows) ->
                    val requested = rows.map { it.second.requestId }.toSet()
                    activeCore.agentStageDelta(hostId, agentId, 0uL).pendingPermissions
                        .filterIsInstance<FfiTimelineItem.PermissionRequest>()
                        .filter { p ->
                            p.requestId in requested && OverviewSafetyReducer.isSafeLowRiskBatchCandidate(p,
                                audits.filter { it.agentId == agentId && it.requestId == p.requestId }
                                    .maxByOrNull { it.decidedAtMs }?.assessment)
                        }.map { FfiPermissionRequestId(agentId, it.requestId) }
                }
                if (safeItems.isEmpty()) {
                    refreshOverviewPendingDecisions()
                    return@launch
                }
                val res = activeCore.resolvePermissionsBatch(hostId, safeItems, "allow")
                reloadFleetInternal()
                reloadHostDetailInternal(hostId)
                _state.value.selectedAgentId?.let { aid ->
                    if (_state.value.selectedHostId == hostId) reloadStageInternal(hostId, aid)
                }
                refreshOverviewPendingDecisions()
                postToast("Approved ${res.resolvedCount} safe requests")
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Batch approval failed"))
            }
        }
    }

    fun resolvePlanApproval(
        requestId: String,
        action: PlanApprovalAction,
        originalMarkdown: String,
        draftMarkdown: String,
        feedback: String,
        buildMode: uniffi.orbit_ffi_uniffi.SessionMode = uniffi.orbit_ffi_uniffi.SessionMode.Default,
    ) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId ?: return
        val payload = PlanApprovalReducer.buildSubmitPayload(
            action = action,
            originalMarkdown = originalMarkdown,
            draftMarkdown = draftMarkdown,
            feedback = feedback,
        )
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.resolvePlanApproval(
                    hostId = hostId,
                    agentId = agentId,
                    permissionRequestId = requestId,
                    action = payload.action,
                    editedPlanMarkdown = payload.editedPlanMarkdown,
                    feedback = payload.feedback,
                    targetMode = payload.targetMode?.let { buildMode },
                )
                reloadStageInternal(hostId, agentId)
                reloadHostDetailInternal(hostId)
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to resolve plan approval")) }
            }
        }
    }

    fun sendPersonalAgentPrompt(promptText: String, onSent: () -> Unit) {
        val trimmed = promptText.trim()
        if (trimmed.isEmpty()) return
        val cur = _state.value
        val routerHost = cur.routerHosts.firstOrNull { it.activeRouterAgentId != null && it.online }
            ?: cur.routerHosts.firstOrNull { it.routerEnabled && it.online }
        val activeRouterAgentId = routerHost?.activeRouterAgentId

        viewModelScope.launch(Dispatchers.IO) {
            try {
                val activeCore = core ?: throw IllegalStateException("Orbit core not ready")
                if (routerHost == null || activeRouterAgentId == null) {
                    postToast(
                        getApplication<Application>()
                            .getString(dev.orbit.android.R.string.overview_router_setup_toast)
                    )
                    requestSheet("router")
                    return@launch
                }
                activeCore.sendPrompt(routerHost.hostId, activeRouterAgentId, trimmed)
                withContext(Dispatchers.Main) {
                    onSent()
                    selectHostAndAgent(routerHost.hostId, activeRouterAgentId)
                }
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to send to Personal Agent")) }
            }
        }
    }

    // --- Tail Pagination & Delivery Interruption ---

    fun loadOlderTimeline(limitEvents: UInt = 80u) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: return
        if (cur.isLoadingOlderTurns || !cur.stageHasOlderTurns) return
        _state.update { it.copy(isLoadingOlderTurns = true) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val loadedMore = core?.loadOlderAgentTimeline(hostId, agentId, limitEvents) ?: false
                reloadStageInternal(hostId, agentId)
                if (loadedMore) delay(8_000) // Remote pages arrive via TimelineChanged after the RPC returns.
                _state.update {
                    if (it.selectedHostId == hostId && it.selectedAgentId == agentId && it.stageOldestLoadedSeq == cur.stageOldestLoadedSeq)
                        it.copy(isLoadingOlderTurns = false) else it
                }
            } catch (t: Throwable) {
                _state.update { it.copy(isLoadingOlderTurns = false) }
                postToast(extractErrorMessage(t, "Failed to load earlier history"))
            }
        }
    }

    fun interruptAgentAndContinue() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.interruptAgentAndContinue(hostId, agentId) }
                .onSuccess {
                    reloadStageInternal(hostId, agentId)
                    postToast("Interrupted & delivering queued message")
                }
                .onFailure { t ->
                    postToast(extractErrorMessage(t, "Failed to interrupt & deliver"))
                }
        }
    }

    // --- File Explorer & Syntax-Highlighted Code Viewer ---

    suspend fun readFile(host: String, cwd: String, path: String): FfiWorkspaceFile = withContext(Dispatchers.IO) {
        checkNotNull(core ?: coreReady.await()).readWorkspaceFile(host, cwd, path)
    }

    suspend fun createProjectHooks(host: String, cwd: String) = withContext(Dispatchers.IO) {
        checkNotNull(core).createWorkspaceEntry(uniffi.orbit_ffi_uniffi.FfiCreateWorkspaceEntryRequest(
            host, cwd, ".orbit/hooks.json", false, """{"version":1,"hooks":[]}"""))
    }

    suspend fun providerTimeline(host: String, parent: String, id: String, open: Boolean) = withContext(Dispatchers.IO) {
        val activeCore = checkNotNull(core)
        if (open) activeCore.openProviderSubagentTimeline(host, parent, id)
        activeCore.providerSubagentTimeline(host, parent, id)
    }

    suspend fun saveFile(file: FfiWorkspaceFile, text: String): Boolean = withContext(Dispatchers.IO) {
        require(file.writable && !file.truncated && !file.etag.isNullOrEmpty())
        val result = checkNotNull(core).writeWorkspaceFile(uniffi.orbit_ffi_uniffi.FfiWriteWorkspaceFileRequest(
            file.hostId, file.cwd, file.path, text, file.etag, false))
        result.conflict == null
    }

    // --- Git Write Operations (Stage, Unstage, Commit, Generate Message, PR, Conflicts, Stash) ---

    fun refreshGitExtras() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val cwd = cur.activeCwd
        viewModelScope.launch(Dispatchers.IO) {
            val activeCore = core ?: return@launch
            val pr = runCatching { activeCore.gitPrStatus(hostId, cwd) }.getOrNull()
            val conflicts = runCatching { activeCore.gitConflicts(hostId, cwd).files }.getOrDefault(emptyList())
            val stashes = runCatching { activeCore.gitStashList(hostId, cwd).entries }.getOrDefault(emptyList())
            _state.update {
                if (it.selectedHostId != hostId || it.activeCwd != cwd) return@update it
                it.copy(
                    gitPrStatus = pr,
                    gitConflicts = conflicts,
                    gitStashes = stashes,
                )
            }
        }
    }

    fun gitStagePaths(paths: List<String>, all: Boolean) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val cwd = cur.activeCwd
        _state.update { it.copy(gitMutationBusy = true) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                checkNotNull(core).gitStagePaths(hostId, cwd, paths, all)
                loadGitReview(cwd = cwd, path = cur.gitSelectedPath)
                postToast(if (all) "Staged all changes" else "Staged ${paths.size} file(s)")
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to stage files"))
            } finally {
                _state.update { it.copy(gitMutationBusy = false) }
            }
        }
    }

    fun gitUnstagePaths(paths: List<String>, all: Boolean) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val cwd = cur.activeCwd
        _state.update { it.copy(gitMutationBusy = true) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                checkNotNull(core).gitUnstagePaths(hostId, cwd, paths, all)
                loadGitReview(cwd = cwd, path = cur.gitSelectedPath)
                postToast(if (all) "Unstaged all changes" else "Unstaged ${paths.size} file(s)")
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to unstage files"))
            } finally {
                _state.update { it.copy(gitMutationBusy = false) }
            }
        }
    }

    fun gitGenerateCommitMessage() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val cwd = cur.activeCwd
        _state.update { it.copy(gitMutationBusy = true) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val res = checkNotNull(core).gitGenerateCommitMessage(hostId, cwd)
                _state.update { if (it.selectedHostId == hostId && it.activeCwd == cwd) it.copy(generatedCommitMessage = res.message, gitMutationBusy = false) else it.copy(gitMutationBusy = false) }
            } catch (t: Throwable) {
                _state.update { it.copy(gitMutationBusy = false) }
                postToast(extractErrorMessage(t, "Failed to generate commit message"))
            }
        }
    }

    fun consumeGeneratedCommitMessage() {
        _state.update { it.copy(generatedCommitMessage = null) }
    }

    fun gitCommit(message: String, amend: Boolean = false, onCommitted: () -> Unit = {}) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val cwd = cur.activeCwd
        val trimmed = message.trim()
        if (trimmed.isEmpty()) return
        _state.update { it.copy(gitMutationBusy = true) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val res = checkNotNull(core).gitCommit(hostId, cwd, trimmed, amend)
                loadGitReview(cwd = cwd, path = cur.gitSelectedPath)
                refreshGitExtras()
                postToast("Committed ${res.shortSha}")
                withContext(Dispatchers.Main) { onCommitted() }
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Git commit failed"))
            } finally {
                _state.update { it.copy(gitMutationBusy = false) }
            }
        }
    }

    fun gitCreatePr(title: String, body: String, base: String?, draft: Boolean, onCreated: () -> Unit = {}) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val cwd = cur.activeCwd
        if (title.isBlank()) return
        _state.update { it.copy(gitMutationBusy = true) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val res = checkNotNull(core).gitCreatePr(hostId, cwd, title.trim(), body.trim(), base?.trim()?.ifEmpty { null }, draft)
                refreshGitExtras()
                postToast("Created PR #${res.number}".trim())
                withContext(Dispatchers.Main) { onCreated() }
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to create Pull Request"))
            } finally {
                _state.update { it.copy(gitMutationBusy = false) }
            }
        }
    }

    fun gitResolveConflict(path: String, resolution: GitConflictResolution, expectedSha256: String?) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val cwd = cur.activeCwd
        _state.update { it.copy(gitMutationBusy = true) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.gitResolveConflict(hostId, cwd, path, null, resolution, true, expectedSha256)
                loadGitReview(cwd = cwd, path = cur.gitSelectedPath)
                refreshGitExtras()
                postToast("Resolved conflict in ${OrbitFormatters.shortPath(path)}")
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to resolve conflict"))
            } finally {
                _state.update { it.copy(gitMutationBusy = false) }
            }
        }
    }

    fun gitStashSave(message: String? = null, includeUntracked: Boolean = true) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val cwd = cur.activeCwd
        _state.update { it.copy(gitMutationBusy = true) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.gitStashSave(hostId, cwd, message?.trim()?.ifEmpty { null }, includeUntracked)
                loadGitReview(cwd = cwd, path = cur.gitSelectedPath)
                refreshGitExtras()
                postToast("Saved working tree to stash")
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to stash changes"))
            } finally {
                _state.update { it.copy(gitMutationBusy = false) }
            }
        }
    }

    fun gitStashPop(stashRef: String, expectedOid: String?) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val cwd = cur.activeCwd
        _state.update { it.copy(gitMutationBusy = true) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.gitStashPop(hostId, cwd, stashRef, expectedOid)
                loadGitReview(cwd = cwd, path = cur.gitSelectedPath)
                refreshGitExtras()
                postToast("Popped $stashRef")
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to pop stash"))
            } finally {
                _state.update { it.copy(gitMutationBusy = false) }
            }
        }
    }

    // --- Automations Creation, Permission Rules (Hooks) & Guardian ---

    fun startGoalRun(objective: String, criteria: List<String>, verifyCommand: String?, maxIterations: UInt?) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId ?: cur.hostDetail?.agents?.firstOrNull()?.agentId ?: run {
            postToast("Open or create a session first to start a Goal")
            return
        }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.startGoal(
                    StartGoalRunRequest(
                        hostId = hostId,
                        agentId = agentId,
                        objective = objective.trim(),
                        criteria = criteria.filter { it.isNotBlank() },
                        verifyCommand = verifyCommand?.trim()?.ifEmpty { null },
                        maxIterations = maxIterations,
                        maxDurationSecs = null,
                        replaceExisting = false,
                        dispatchInitialPrompt = null,
                    )
                )
                refreshAutomations()
                postToast("Goal started")
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to start goal"))
            }
        }
    }

    fun createMonitorWatch(name: String, kind: MonitorKind, expiresInSecs: ULong? = null) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId ?: cur.hostDetail?.agents?.firstOrNull()?.agentId ?: run {
            postToast("Open or create a session first to add a Monitor")
            return
        }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.createMonitor(
                    StartMonitorWatchRequest(
                        hostId = hostId,
                        agentId = agentId,
                        name = name.trim(),
                        kind = kind,
                        expiresInSecs = expiresInSecs,
                    )
                )
                refreshAutomations()
                postToast("Monitor created")
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to create monitor"))
            }
        }
    }

    fun createConditionalTimer(durationSecs: ULong, prompt: String, condition: TimerCondition = TimerCondition.Never) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId ?: cur.hostDetail?.agents?.firstOrNull()?.agentId ?: run {
            postToast("Open a session first to arm a timer")
            return
        }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.createTimer(hostId, agentId, durationSecs, prompt.trim(), condition)
                refreshAutomations()
                postToast("Timer armed")
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to create timer"))
            }
        }
    }

    fun startSimpleDagWorkflow(name: String, prompt: String) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val step = DagStepSpec(
                    stepId = "step_1",
                    name = name.trim(),
                    dependsOn = emptyList(),
                    promptTemplate = prompt.trim(),
                    provider = OrbitFfiTypeBridge.toProviderKind(cur.composer.provider),
                    model = cur.composer.model,
                    isolatedWorktree = false,
                    retryCount = 0u,
                    timeoutSecs = 900uL,
                )
                core?.startWorkflow(
                    StartDagWorkflowRequest(
                        hostId = hostId,
                        name = name.trim(),
                        cwd = cur.activeCwd,
                        steps = listOf(step),
                        defaultProvider = OrbitFfiTypeBridge.toProviderKind(cur.composer.provider),
                        defaultMode = OrbitFfiTypeBridge.toSessionMode(cur.composer.mode),
                        maxConcurrency = 1u,
                    )
                )
                refreshAutomations()
                postToast("Workflow launched")
            } catch (t: Throwable) {
                postToast(extractErrorMessage(t, "Failed to launch workflow"))
            }
        }
    }

    fun refreshPermissionRulesAndGuardian() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            val rules = runCatching { core?.listPermissionRules(hostId, cur.activeCwd) }.getOrNull().orEmpty()
            val guardian = runCatching { core?.getGuardianConfig(hostId) }.getOrNull()
            _state.update { it.copy(permissionRules = rules, guardianSnapshot = guardian) }
        }
    }

    fun upsertPermissionRule(rule: UpsertPermissionRuleInput) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.upsertPermissionRule(hostId, rule) }
                .onSuccess {
                    refreshPermissionRulesAndGuardian()
                    postToast("Rule saved")
                }
                .onFailure { t -> postToast(extractErrorMessage(t, "Failed to save rule")) }
        }
    }

    fun deletePermissionRule(ruleId: String) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.deletePermissionRule(hostId, ruleId) }
                .onSuccess {
                    refreshPermissionRulesAndGuardian()
                    postToast("Rule removed")
                }
                .onFailure { t -> postToast(extractErrorMessage(t, "Failed to remove rule")) }
        }
    }

    fun updateGuardianConfig(config: GuardianPolicyConfig) {
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.updateGuardianConfig(hostId, config) }
                .onSuccess { snap ->
                    guardianAuditCache.remove(hostId)
                    _state.update { it.copy(guardianSnapshot = snap) }
                    postToast("Guardian policy updated")
                }
                .onFailure { t -> postToast(extractErrorMessage(t, "Failed to update Guardian policy")) }
        }
    }

    // --- Personal Router & Fleet Mesh ---

    fun refreshRouterAndMesh(requestedHost: String? = null) {
        _state.update { it.copy(isLoadingRouterMesh = true) }
        viewModelScope.launch(Dispatchers.IO) {
            val activeCore = core ?: run {
                _state.update { it.copy(isLoadingRouterMesh = false) }
                return@launch
            }
            val routerHosts = runCatching { activeCore.discoverFleetRouterHosts() }.getOrDefault(emptyList())
            val primaryHostId = requestedHost ?: routerHosts.firstOrNull { it.routerEnabled && it.online }?.hostId
                ?: _state.value.selectedHostId
                ?: routerHosts.firstOrNull()?.hostId
            val routerStatus = primaryHostId?.let { hid ->
                runCatching { activeCore.getRouterConfig(hid) }.getOrNull()
            }
            val mesh = runCatching { activeCore.getFleetMeshSnapshot() }.getOrDefault(emptyMap())
            _state.update {
                it.copy(
                    routerHosts = routerHosts,
                    routerStatus = routerStatus,
                    fleetMeshSnapshot = mesh,
                    isLoadingRouterMesh = false,
                )
            }
        }
    }

    fun updatePersonalRouterConfig(
        hostId: String,
        config: PersonalRouterConfig,
        activate: Boolean,
    ) {
        _state.update { it.copy(isLoadingRouterMesh = true) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val snap = core?.updateRouterConfig(hostId, config, null, activate)
                _state.update { it.copy(routerStatus = snap ?: it.routerStatus, isLoadingRouterMesh = false) }
                refreshRouterAndMesh()
                postToast(if (config.enabled) "Personal Router updated" else "Personal Router paused")
            } catch (t: Throwable) {
                _state.update { it.copy(isLoadingRouterMesh = false) }
                postToast(extractErrorMessage(t, "Failed to update Personal Router"))
            }
        }
    }

    fun toggleMeshPeer(hostId: String, registrationId: String, label: String?, enabled: Boolean) {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.updateMeshPeer(hostId, registrationId, label, enabled) }
                .onSuccess {
                    refreshRouterAndMesh()
                    postToast(if (enabled) "Mesh peer enabled" else "Mesh peer disabled")
                }
                .onFailure { t -> postToast(extractErrorMessage(t, "Failed to update mesh peer")) }
        }
    }

    // --- Global Cross-Host Search (Local FTS5 + Remote Daemons) ---

    private val searchGeneration = java.util.concurrent.atomic.AtomicLong()

    fun runGlobalSearch(query: String, includeRemote: Boolean = true) {
        val generation = searchGeneration.incrementAndGet()
        val trimmed = query.trim()
        _state.update { it.copy(globalSearchQuery = query) }
        if (trimmed.length < 2) {
            _state.update {
                it.copy(
                    globalSearchHits = emptyList(),
                    globalSearchRemoteQueried = 0u,
                    globalSearchRemoteFailed = 0u,
                    isSearchingGlobal = false,
                )
            }
            return
        }
        _state.update { it.copy(isSearchingGlobal = true, globalSearchHits = emptyList()) }
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val activeCore = checkNotNull(core ?: coreReady.await())
                if (includeRemote) {
                    val res = activeCore.searchMessagesRemote(
                        SearchMessagesRequest(
                            query = trimmed,
                            hostId = null,
                            agentId = null,
                            limit = 40u,
                        )
                    )
                    _state.update {
                        if (searchGeneration.get() != generation) return@update it
                        it.copy(
                            globalSearchHits = res.hits,
                            globalSearchRemoteQueried = res.remoteHostsQueried,
                            globalSearchRemoteFailed = res.remoteHostsFailed,
                            isSearchingGlobal = false,
                        )
                    }
                } else {
                    val localHits = activeCore.searchMessages(trimmed, 40u).map { h ->
                        ConversationSearchHit(
                            hostId = h.hostId,
                            hostName = h.hostName,
                            agentId = h.agentId,
                            seq = h.seq,
                            turnId = null,
                            role = h.role,
                            title = h.title,
                            projectName = h.projectName,
                            snippet = h.snippet,
                            updatedAtRfc3339 = h.updatedAtRfc3339,
                            isRemote = false,
                            isUnloaded = false,
                        )
                    }
                    _state.update {
                        if (searchGeneration.get() != generation) return@update it
                        it.copy(
                            globalSearchHits = localHits,
                            globalSearchRemoteQueried = 0u,
                            globalSearchRemoteFailed = 0u,
                            isSearchingGlobal = false,
                        )
                    }
                }
            } catch (t: Throwable) {
                if (searchGeneration.get() != generation) return@launch
                _state.update { it.copy(isSearchingGlobal = false) }
                postToast(extractErrorMessage(t, "Search failed"))
            }
        }
    }

    override fun onCleared() {
        terminalInput.close()
        cloudSyncPollJob?.cancel()
        OrbitFirebaseMessagingBridge.detachTokenListener()
        networkObserver.stop()
        runCatching { core?.close() }
        super.onCleared()
    }

    companion object {
        private const val PREFS_NAME = "orbit_android_prefs"
        private const val KEY_APPEARANCE = "appearance_mode"
        private const val KEY_DRAWER_COLLAPSED = "permanent_drawer_collapsed"
        private const val KEY_LAST_HOST_ID = "last_host_id"
        private const val KEY_LAST_CWD = "last_cwd"
        private const val KEY_PINNED_THREAD_KEYS = "pinned_thread_keys"
        private const val KEY_MIGRATED_THREAD_PREFS_V1 = "migrated_thread_prefs_v1"
        private const val KEY_DRAFT_USE_WORKTREE = "draft_use_worktree"
        private const val GUARDIAN_AUDIT_CACHE_TTL_MS = 15_000L
        private const val GUARDIAN_CONFIG_RPC_TIMEOUT_MS = 2_500L
        private val SAFE_DEEP_LINK_ID_REGEX = Regex("^[A-Za-z0-9_.:-]{1,128}$")

        const val EXTRA_PAIR_URI = "orbit_pair_uri"
        const val EXTRA_SELECT_LATEST = "orbit_select_latest"
        const val EXTRA_THEME = "orbit_theme"
        const val EXTRA_OPEN_SHEET = "orbit_open_sheet"
        const val EXTRA_DRAWER_SEARCH = "orbit_drawer_search"
        const val EXTRA_DEMO_COMPOSER = "orbit_demo_composer"
        const val EXTRA_DEMO_APPROVAL = "orbit_demo_approval"
        const val EXTRA_DEMO_QUESTION = "orbit_demo_question"
        const val EXTRA_PTY_CMD = "orbit_pty_cmd"
    }
}
