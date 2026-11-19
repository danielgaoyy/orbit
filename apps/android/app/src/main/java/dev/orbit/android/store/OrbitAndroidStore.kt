package dev.orbit.android.store

import android.app.Application
import android.content.Context
import android.content.Intent
import android.os.SystemClock
import androidx.compose.runtime.Immutable
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.orbit.android.BuildConfig
import dev.orbit.android.ui.theme.OrbitAppearanceMode
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.orbit_ffi_uniffi.FfiAgentCard
import uniffi.orbit_ffi_uniffi.FfiAgentStage
import uniffi.orbit_ffi_uniffi.FfiClientProject
import uniffi.orbit_ffi_uniffi.FfiConnectionState
import uniffi.orbit_ffi_uniffi.FfiCoreEvent
import uniffi.orbit_ffi_uniffi.FfiGitReview
import uniffi.orbit_ffi_uniffi.FfiHostCard
import uniffi.orbit_ffi_uniffi.FfiHostDetail
import uniffi.orbit_ffi_uniffi.FfiHostRouteMode
import uniffi.orbit_ffi_uniffi.FfiOrbitException
import uniffi.orbit_ffi_uniffi.FfiPromptBadge
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiProviderModelCatalog
import uniffi.orbit_ffi_uniffi.FfiProviderModelSpec
import uniffi.orbit_ffi_uniffi.FfiQuestionItem
import uniffi.orbit_ffi_uniffi.FfiReasoningEffort
import uniffi.orbit_ffi_uniffi.FfiSessionMode
import uniffi.orbit_ffi_uniffi.FfiTerminalScreen
import uniffi.orbit_ffi_uniffi.FfiTimelineItem
import uniffi.orbit_ffi_uniffi.FfiToolCallDetail
import uniffi.orbit_ffi_uniffi.FfiToolStepDescriptor
import uniffi.orbit_ffi_uniffi.MobileCoreListener
import uniffi.orbit_ffi_uniffi.OrbitMobileCore
import uniffi.orbit_ffi_uniffi.defaultEffortDescription
import uniffi.orbit_ffi_uniffi.fallbackProviderCatalog
import uniffi.orbit_ffi_uniffi.formatCatalogModelLabel
import uniffi.orbit_ffi_uniffi.formatEffortLabel
import uniffi.orbit_ffi_uniffi.resolveSupportedEffortForModel
import java.io.File
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

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

enum class OrbitSlashCommandId {
    PLAN,
    REVIEW,
    DIFF,
    COMPACT,
    SUPERVISED,
    AUTO,
    FULL,
    TERMINAL,
    NEW_SESSION,
    SETTINGS,
}

@Immutable
data class OrbitSlashCommand(
    val id: OrbitSlashCommandId,
    val command: String,
    val label: String,
    val description: String,
    val keywords: List<String>,
) {
    companion object {
        val all: List<OrbitSlashCommand> = listOf(
            OrbitSlashCommand(
                id = OrbitSlashCommandId.PLAN,
                command = "/plan",
                label = "Draft Implementation Plan",
                description = "Prompt the agent to design a step-by-step plan before editing",
                keywords = listOf("plan", "design", "architect", "checklist"),
            ),
            OrbitSlashCommand(
                id = OrbitSlashCommandId.REVIEW,
                command = "/review",
                label = "Code Review Working Tree",
                description = "Attach uncommitted git diff and prepare an engineering review",
                keywords = listOf("review", "audit", "critique", "git"),
            ),
            OrbitSlashCommand(
                id = OrbitSlashCommandId.DIFF,
                command = "/diff",
                label = "Toggle Diff Inspector",
                description = "Open or close the side-by-side Git & Turn Diff panel",
                keywords = listOf("diff", "inspector", "changes"),
            ),
            OrbitSlashCommand(
                id = OrbitSlashCommandId.COMPACT,
                command = "/compact",
                label = "Compact Context",
                description = "Ask active agent to summarize and compact conversation history",
                keywords = listOf("compact", "summarize", "context", "memory"),
            ),
            OrbitSlashCommand(
                id = OrbitSlashCommandId.SUPERVISED,
                command = "/ask",
                label = "Always Ask Permissions",
                description = "Require approval before file edits and shell commands",
                keywords = listOf("ask", "supervised", "default", "safe", "permission"),
            ),
            OrbitSlashCommand(
                id = OrbitSlashCommandId.AUTO,
                command = "/edits",
                label = "Accept Edits Permissions",
                description = "Auto-approve file edits, ask before shell commands",
                keywords = listOf("edits", "auto", "edit", "accept", "permission"),
            ),
            OrbitSlashCommand(
                id = OrbitSlashCommandId.FULL,
                command = "/full",
                label = "Full Access Permissions",
                description = "Edit files and run commands without approval prompts",
                keywords = listOf("full", "autonomous", "bypass", "unrestricted", "permission"),
            ),
            OrbitSlashCommand(
                id = OrbitSlashCommandId.TERMINAL,
                command = "/terminal",
                label = "Toggle PTY Terminal",
                description = "Show or hide the persistent multi-slot terminal dock",
                keywords = listOf("terminal", "pty", "shell", "console"),
            ),
            OrbitSlashCommand(
                id = OrbitSlashCommandId.NEW_SESSION,
                command = "/new",
                label = "New Agent Session",
                description = "Start a fresh isolated session in the stage",
                keywords = listOf("new", "thread", "session", "clear"),
            ),
            OrbitSlashCommand(
                id = OrbitSlashCommandId.SETTINGS,
                command = "/settings",
                label = "Settings",
                description = "Configure hosts, engines, worktrees, and keybindings",
                keywords = listOf("settings", "preferences", "config"),
            ),
        )

        fun filter(query: String): List<OrbitSlashCommand> {
            val q = query.trim().lowercase()
            if (q.isEmpty()) return all
            return all.filter { c ->
                c.command.removePrefix("/").lowercase().contains(q) ||
                    c.label.lowercase().contains(q) ||
                    c.keywords.any { it.contains(q) }
            }
        }
    }
}

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
}

enum class OrbitSidebarThreadFilter(val label: String) {
    ALL("All Conversations"),
    ACTIVE_AND_UNREAD("Active & Unread"),
}

@Immutable
data class OrbitLaunchpadProject(
    val id: String,
    val name: String,
    val path: String,
    val branch: String? = null,
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
    val pinnedThreadKeys: Set<String> = emptySet(),
    val seenSeqByThreadKey: Map<String, ULong> = emptyMap(),
    val harnessRequestId: Long = 0L,
    val harnessSheetTarget: String? = null,
    val pendingPtyCommand: String? = null,
    val demoPermissions: List<FfiTimelineItem> = emptyList(),
    val demoQuestions: List<FfiTimelineItem> = emptyList(),
    val localDaemonUri: String? = null,
    val lastError: String? = null,
    val toastMessage: String? = null,
) {
    val selectedHost: FfiHostCard?
        get() = hostDetail?.host ?: hosts.firstOrNull { it.hostId == selectedHostId }

    val activeAgent: FfiAgentCard?
        get() = if (selectedAgentId == null) {
            null
        } else {
            agentStage?.agent ?: hostDetail?.agents?.firstOrNull { it.agentId == selectedAgentId }
        }

    val activeQueuedPrompts: List<QueuedPromptItem>
        get() {
            val hId = selectedHostId ?: return emptyList()
            val aId = selectedAgentId?.takeIf { !isNewSessionMode } ?: return emptyList()
            return queuedPromptsByAgent["$hId::$aId"].orEmpty()
        }

    val isActiveAgentRunning: Boolean
        get() = !isNewSessionMode && (
            agentStage?.agent?.isRunning == true ||
                agentStage?.turns?.any { it.hasActiveItem } == true
            )

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

    val launchpadProjects: List<OrbitLaunchpadProject>
        get() {
            val hid = selectedHostId ?: return emptyList()
            val detail = hostDetail
            val results = mutableListOf<OrbitLaunchpadProject>()

            fun isBareHomePath(path: String): Boolean {
                val trimmed = path.trim().trimEnd('/')
                if (trimmed.isEmpty() || trimmed == "~" || trimmed == "/root") return true
                val parts = trimmed.split('/').filter { it.isNotEmpty() }
                return parts.size == 2 && (parts[0] == "Users" || parts[0] == "home")
            }

            fun resolvedBranch(path: String): String? {
                if (OrbitFormatters.pathsEquivalent(path, activeCwd)) {
                    gitReview?.branch?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
                }
                detail?.discoveredProjects
                    ?.firstOrNull { OrbitFormatters.pathsEquivalent(it.path, path) }
                    ?.gitBranch?.trim()?.takeIf { it.isNotEmpty() }
                    ?.let { return it }
                detail?.workspaces
                    ?.firstOrNull { OrbitFormatters.pathsEquivalent(it.cwd, path) }
                    ?.gitBranch?.trim()?.takeIf { it.isNotEmpty() }
                    ?.let { return it }
                return null
            }

            fun appendIfUnique(id: String, name: String, path: String, branch: String?) {
                val clean = path.trim()
                if (clean.isEmpty()) return
                if (results.any { OrbitFormatters.pathsEquivalent(it.path, clean) }) return
                val resolvedName = name.trim().ifEmpty { OrbitFormatters.shortPath(clean) }
                results += OrbitLaunchpadProject(
                    id = id,
                    name = resolvedName,
                    path = clean,
                    branch = branch?.trim()?.takeIf { it.isNotEmpty() } ?: resolvedBranch(clean),
                )
            }

            projects.filter { it.hostId == hid }.forEach { proj ->
                proj.directories.firstOrNull()?.let { primary ->
                    appendIfUnique(
                        id = proj.projectId,
                        name = proj.name,
                        path = primary,
                        branch = null,
                    )
                }
            }

            detail?.workspaces.orEmpty().forEach { ws ->
                if (!ws.isWorktree && !isBareHomePath(ws.cwd)) {
                    appendIfUnique(
                        id = "$hid::ws::${ws.workspaceId}",
                        name = ws.title.ifBlank { OrbitFormatters.shortPath(ws.cwd) },
                        path = ws.cwd,
                        branch = ws.gitBranch,
                    )
                }
            }

            detail?.discoveredProjects.orEmpty().forEach { dp ->
                if (!isBareHomePath(dp.path) && !dp.path.contains("/worktrees/")) {
                    appendIfUnique(
                        id = "$hid::dp::${dp.path}",
                        name = dp.name,
                        path = dp.path,
                        branch = dp.gitBranch,
                    )
                }
            }

            if (activeCwd.isNotBlank() && !isBareHomePath(activeCwd)) {
                appendIfUnique(
                    id = "$hid::active::$activeCwd",
                    name = OrbitFormatters.shortPath(activeCwd),
                    path = activeCwd,
                    branch = activeGitBranch,
                )
            }

            if (results.isEmpty()) {
                detail?.discoveredProjects?.firstOrNull()?.let { firstDp ->
                    appendIfUnique(
                        id = "$hid::dp::${firstDp.path}",
                        name = firstDp.name,
                        path = firstDp.path,
                        branch = firstDp.gitBranch,
                    )
                }
            }

            return results
        }

    fun threadKey(hostId: String, agentId: String): String = "$hostId::$agentId"

    fun isThreadPinned(hostId: String, agentId: String): Boolean =
        pinnedThreadKeys.contains(threadKey(hostId, agentId))

    fun isThreadUnread(hostId: String, agent: FfiAgentCard): Boolean {
        if (selectedHostId == hostId && !isNewSessionMode && selectedAgentId == agent.agentId) {
            return false
        }
        if (agent.isRunning || agent.isAwaitingPermission) {
            return false
        }
        val seen = seenSeqByThreadKey[threadKey(hostId, agent.agentId)] ?: return false
        return agent.latestSeq > seen
    }
}

object OrbitFormatters {
    fun providerLabel(provider: FfiProviderKind): String = when (provider) {
        FfiProviderKind.ANTIGRAVITY -> "Antigravity"
        FfiProviderKind.CODEX -> "Codex"
        FfiProviderKind.CLAUDE_CODE -> "Claude Code"
    }

    fun providerShortLabel(provider: FfiProviderKind): String = when (provider) {
        FfiProviderKind.ANTIGRAVITY -> "AGY"
        FfiProviderKind.CODEX -> "Codex"
        FfiProviderKind.CLAUDE_CODE -> "Claude"
    }

    fun sessionModeLabel(mode: FfiSessionMode): String = when (mode) {
        FfiSessionMode.Default -> "Always Ask"
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

class OrbitAndroidStore(application: Application) : AndroidViewModel(application) {
    private val prefs = application.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    private val runningAgentKeys: MutableSet<String> = ConcurrentHashMap.newKeySet()
    private val drainingAgentKeys: MutableSet<String> = ConcurrentHashMap.newKeySet()
    private val draftSelectionsByProvider: MutableMap<FfiProviderKind, Pair<String?, FfiReasoningEffort?>> =
        ConcurrentHashMap()
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
            pinnedThreadKeys = prefs.getStringSet(KEY_PINNED_THREAD_KEYS, emptySet())?.toSet().orEmpty(),
        )
    )
    val state: StateFlow<OrbitUiState> = _state.asStateFlow()

    @Volatile
    private var core: OrbitMobileCore? = null

    private val networkObserver = NetworkConnectivityObserver(application) {
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.onNetworkChanged() }
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
        val dbFile = File(application.filesDir, "orbit_android_v3.sqlite")
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val instance = OrbitMobileCore.openWithOptions(dbFile.absolutePath, true)
                core = instance
                instance.subscribeThrottled(listener, 24u)
                networkObserver.start()
                val discovered = runCatching { instance.discoverLocalDaemonUri() }.getOrNull()
                if (discovered != null) {
                    _state.update { it.copy(localDaemonUri = discovered) }
                }
                reloadFleetInternal()
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
                }
            }
            is FfiCoreEvent.TimelineChanged -> {
                val latest = _state.value
                if (event.hostId == latest.selectedHostId && event.agentId == latest.selectedAgentId) {
                    reloadStageInternal(event.hostId, event.agentId)
                } else {
                    val queueKey = agentQueueKey(event.hostId, event.agentId)
                    if (latest.queuedPromptsByAgent[queueKey]?.isNotEmpty() == true) {
                        val bgStage = runCatching { core?.agentStage(event.hostId, event.agentId) }.getOrNull()
                        if (bgStage != null) {
                            val ag = bgStage.agent
                            val busy = (ag?.isRunning == true) ||
                                (ag?.isAwaitingPermission == true) ||
                                bgStage.turns.any { it.hasActiveItem }
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
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.onAppForeground() }
            reloadFleetInternal()
            _state.value.selectedHostId?.let { reloadHostDetailInternal(it) }
        }
    }

    fun onAppBackground() {
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
    }

    fun setSidebarProjectSort(sort: OrbitSidebarProjectSort) {
        _state.update { it.copy(sidebarProjectSort = sort) }
    }

    fun setSidebarThreadFilter(filter: OrbitSidebarThreadFilter) {
        _state.update { it.copy(sidebarThreadFilter = filter) }
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
        val key = "$hostId::$agentId"
        var updated: Set<String> = emptySet()
        _state.update { prev ->
            val next = if (prev.pinnedThreadKeys.contains(key)) {
                prev.pinnedThreadKeys - key
            } else {
                prev.pinnedThreadKeys + key
            }
            updated = next
            prev.copy(pinnedThreadKeys = next)
        }
        prefs.edit().putStringSet(KEY_PINNED_THREAD_KEYS, updated).apply()
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
        val trimmed = uriString?.trim().orEmpty()
        if (trimmed.startsWith("orbit://pair", ignoreCase = true)) {
            pairHost(trimmed)
        }
    }

    fun reloadFleet() {
        viewModelScope.launch(Dispatchers.IO) {
            reloadFleetInternal()
        }
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
    }

    private fun reloadHostDetailInternal(hostId: String) {
        val detail = runCatching { core?.hostDetail(hostId) }.getOrNull() ?: return
        var autoOpenedLatestAgentId: String? = null
        var updatedPinnedToPersist: Set<String>? = null
        _state.update { prev ->
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
            val nextSelectedAgentId = when {
                shouldForceLatest -> latestAgentByTime.agentId
                prev.isNewSessionMode -> null
                remappedAgentId != null && (remappedAgentId.startsWith("opt_") || detail.agents.any { it.agentId == remappedAgentId }) -> remappedAgentId
                else -> latestAgentByTime?.agentId
            }
            val nextNewSessionMode = if (shouldForceLatest) false else prev.isNewSessionMode
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

            val catalog = OrbitFormatters.catalogFor(detail, preferredProvider)
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
            val resolvedModel = matchedAgent?.model ?: defaultDraftModel
            val modelSpec = catalog.models.firstOrNull { it.id == resolvedModel }
                ?: catalog.models.firstOrNull { it.isDefault }
                ?: catalog.models.firstOrNull()
            val resolvedEffort = matchedAgent?.let { ag ->
                OrbitFormatters.resolveEffort(
                    spec = modelSpec,
                    desired = ag.effort,
                    catalogDefault = catalog.configuredDefaultEffort,
                )
            } ?: defaultDraftEffort
            val resolvedMode = OrbitFormatters.normalizeSessionMode(matchedAgent?.mode ?: prev.composer.mode)

            val files = runCatching { core?.workspaceFiles(hostId, nextCwd) }.getOrNull() ?: prev.workspaceFiles

            var migratedQueues = prev.queuedPromptsByAgent
            var migratedPinned = prev.pinnedThreadKeys
            val migratedSeen = prev.seenSeqByThreadKey.toMutableMap()
            detail.replacedOptimisticAgents.forEach { (optimisticId, confirmedId) ->
                val oldKey = agentQueueKey(hostId, optimisticId)
                val newKey = agentQueueKey(hostId, confirmedId)
                val pending = migratedQueues[oldKey]
                if (!pending.isNullOrEmpty() && oldKey != newKey) {
                    migratedQueues = (migratedQueues - oldKey) +
                        (newKey to (migratedQueues[newKey].orEmpty() + pending))
                }
                if (oldKey != newKey && migratedPinned.contains(oldKey)) {
                    migratedPinned = (migratedPinned - oldKey) + newKey
                    updatedPinnedToPersist = migratedPinned
                }
                if (oldKey != newKey) {
                    val prevSeen = migratedSeen.remove(oldKey)
                    if (prevSeen != null) {
                        migratedSeen[newKey] = maxOf(migratedSeen[newKey] ?: 0uL, prevSeen)
                    }
                }
                if (runningAgentKeys.remove(oldKey)) {
                    runningAgentKeys.add(newKey)
                }
            }
            detail.agents.forEach { ag ->
                val key = agentQueueKey(hostId, ag.agentId)
                if (!nextNewSessionMode && ag.agentId == nextSelectedAgentId) {
                    migratedSeen[key] = maxOf(migratedSeen[key] ?: 0uL, ag.latestSeq)
                } else if (!migratedSeen.containsKey(key)) {
                    migratedSeen[key] = ag.latestSeq
                }
            }

            prev.copy(
                hostDetail = detail,
                selectedAgentId = nextSelectedAgentId,
                isNewSessionMode = nextNewSessionMode,
                activeCwd = nextCwd,
                workspaceFiles = files,
                queuedPromptsByAgent = migratedQueues,
                pinnedThreadKeys = migratedPinned,
                seenSeqByThreadKey = migratedSeen,
                composer = prev.composer.copy(
                    provider = preferredProvider,
                    model = resolvedModel,
                    effort = resolvedEffort,
                    mode = resolvedMode,
                ),
            )
        }
        updatedPinnedToPersist?.let {
            prefs.edit().putStringSet(KEY_PINNED_THREAD_KEYS, it).apply()
        }

        autoOpenedLatestAgentId?.let { latestId ->
            runCatching { core?.openAgentTimeline(hostId, latestId) }
        }

        val current = _state.value
        val activeAgentId = current.selectedAgentId
        detail.agents.forEach { ag ->
            val qKey = agentQueueKey(hostId, ag.agentId)
            val busy = ag.isRunning || ag.isAwaitingPermission
            if (busy) {
                runningAgentKeys.add(qKey)
            } else if (ag.agentId != activeAgentId && runningAgentKeys.remove(qKey)) {
                if (_state.value.queuedPromptsByAgent[qKey]?.isNotEmpty() == true) {
                    drainNextQueuedPromptIfSettled(hostId, ag.agentId)
                }
            }
        }
        if (activeAgentId != null && !current.isNewSessionMode) {
            reloadStageInternal(hostId, activeAgentId)
        }
    }

    private fun reloadStageInternal(hostId: String, agentId: String) {
        val stage = runCatching { core?.agentStage(hostId, agentId) }.getOrNull() ?: return
        _state.update { prev ->
            val ag = stage.agent
            val nextComposer = if (ag != null) {
                prev.composer.copy(
                    provider = ag.provider,
                    model = ag.model ?: prev.composer.model,
                    effort = ag.effort ?: prev.composer.effort,
                    mode = OrbitFormatters.normalizeSessionMode(ag.mode),
                )
            } else {
                prev.composer
            }
            val nextSeen = if (ag != null && !prev.isNewSessionMode && prev.selectedHostId == hostId && prev.selectedAgentId == agentId) {
                val key = agentQueueKey(hostId, agentId)
                val prevSeen = prev.seenSeqByThreadKey[key] ?: 0uL
                if (ag.latestSeq > prevSeen) {
                    prev.seenSeqByThreadKey + (key to ag.latestSeq)
                } else {
                    prev.seenSeqByThreadKey
                }
            } else {
                prev.seenSeqByThreadKey
            }
            prev.copy(
                agentStage = stage,
                activeCwd = ag?.cwd ?: prev.activeCwd,
                seenSeqByThreadKey = nextSeen,
                composer = nextComposer,
            )
        }

        val queueKey = agentQueueKey(hostId, agentId)
        val ag = stage.agent
        val isBusy = (ag?.isRunning == true) ||
            (ag?.isAwaitingPermission == true) ||
            stage.turns.any { it.hasActiveItem }
        if (isBusy) {
            runningAgentKeys.add(queueKey)
        } else if (runningAgentKeys.remove(queueKey)) {
            if (_state.value.queuedPromptsByAgent[queueKey]?.isNotEmpty() == true) {
                drainNextQueuedPromptIfSettled(hostId, agentId)
            }
        }
    }

    fun selectHost(hostId: String) {
        prefs.edit().putString(KEY_LAST_HOST_ID, hostId).apply()
        _state.update {
            it.copy(
                selectedHostId = hostId,
                selectedAgentId = null,
                isNewSessionMode = false,
                agentStage = null,
                gitReview = null,
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
        val nextCwd = cwd?.trim()?.takeIf { it.isNotBlank() } ?: _state.value.activeCwd
        prefs.edit().putString(KEY_LAST_CWD, nextCwd).apply()
        _state.update { prev ->
            val targetProvider = provider ?: prev.composer.provider
            val saved = draftSelectionsByProvider[targetProvider]
            val (nextModel, nextEffort) = syncDefaultModelAndEffort(
                detail = prev.hostDetail,
                provider = targetProvider,
                desiredModel = if (hasUserCustomizedDraftModel && prev.composer.provider == targetProvider) {
                    prev.composer.model
                } else {
                    saved?.first
                },
                desiredEffort = if (hasUserCustomizedDraftModel && prev.composer.provider == targetProvider) {
                    prev.composer.effort
                } else {
                    saved?.second
                },
            )
            val nextComposer = prev.composer.copy(
                provider = targetProvider,
                model = nextModel,
                effort = nextEffort,
            )
            prev.copy(
                selectedAgentId = null,
                isNewSessionMode = true,
                agentStage = null,
                activeCwd = nextCwd,
                composer = nextComposer,
                lastError = null,
            )
        }
        val hostId = _state.value.selectedHostId ?: return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.fetchWorkspaceFiles(hostId, nextCwd) }
            runCatching { core?.fetchGitStatus(hostId, nextCwd) }
            runCatching { core?.fetchGitDiff(hostId, nextCwd, null, _state.value.gitStagedMode) }
            val files = runCatching { core?.workspaceFiles(hostId, nextCwd) }.getOrNull().orEmpty()
            val review = runCatching { core?.gitReview(hostId, nextCwd, null) }.getOrNull()
            _state.update { it.copy(workspaceFiles = files, gitReview = review) }
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
        val hostId = _state.value.selectedHostId ?: return
        _state.update { prev ->
            val key = agentQueueKey(hostId, agentId)
            val matchedSeq = prev.hostDetail?.agents?.firstOrNull { it.agentId == agentId }?.latestSeq ?: 0uL
            val nextSeen = maxOf(prev.seenSeqByThreadKey[key] ?: 0uL, matchedSeq)
            prev.copy(
                selectedAgentId = agentId,
                isNewSessionMode = false,
                seenSeqByThreadKey = prev.seenSeqByThreadKey + (key to nextSeen),
                lastError = null,
            )
        }
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.openAgentTimeline(hostId, agentId) }
            reloadStageInternal(hostId, agentId)
            val cwd = _state.value.activeCwd
            runCatching { core?.fetchWorkspaceFiles(hostId, cwd) }
            runCatching { core?.fetchGitStatus(hostId, cwd) }
            runCatching { core?.fetchGitDiff(hostId, cwd, null, _state.value.gitStagedMode) }
            val files = runCatching { core?.workspaceFiles(hostId, cwd) }.getOrNull().orEmpty()
            val review = runCatching { core?.gitReview(hostId, cwd, null) }.getOrNull()
            _state.update { it.copy(workspaceFiles = files, gitReview = review) }
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
            _state.update { it.copy(workspaceFiles = files) }
        }
    }

    // --- Composer & Agent Config ---

    fun updateComposerText(text: String) {
        _state.update { it.copy(composer = it.composer.copy(text = text)) }
    }

    private fun syncDefaultModelAndEffort(
        detail: FfiHostDetail?,
        provider: FfiProviderKind,
        desiredModel: String? = null,
        desiredEffort: FfiReasoningEffort? = null,
    ): Pair<String?, FfiReasoningEffort?> {
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

    fun switchDraftProvider(provider: FfiProviderKind) {
        val prev = _state.value
        draftSelectionsByProvider[prev.composer.provider] =
            prev.composer.model to prev.composer.effort
        hasUserCustomizedDraftModel = true
        val saved = draftSelectionsByProvider[provider]
        val (nextModel, nextEffort) = syncDefaultModelAndEffort(
            detail = prev.hostDetail,
            provider = provider,
            desiredModel = saved?.first,
            desiredEffort = saved?.second,
        )
        draftSelectionsByProvider[provider] = nextModel to nextEffort
        selectProviderModelAndEffort(provider, nextModel, nextEffort)
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
        val catalog = OrbitFormatters.catalogFor(prev.hostDetail, provider)
        val spec = catalog.models.firstOrNull { it.id == model }
            ?: catalog.models.firstOrNull { it.isDefault }
            ?: catalog.models.firstOrNull()
        val resolvedEffort = OrbitFormatters.resolveEffort(spec, effort, catalog.configuredDefaultEffort)
        if (prev.isNewSessionMode || prev.selectedAgentId == null) {
            hasUserCustomizedDraftModel = true
            draftSelectionsByProvider[provider] = model to resolvedEffort
        }

        _state.update {
            it.copy(
                composer = it.composer.copy(
                    provider = provider,
                    model = model,
                    effort = resolvedEffort,
                )
            )
        }

        val hostId = prev.selectedHostId ?: return
        val agentId = prev.selectedAgentId ?: return
        if (prev.isNewSessionMode) return

        viewModelScope.launch(Dispatchers.IO) {
            val activeProvider = prev.activeAgent?.provider
            if (activeProvider != null && activeProvider != provider) {
                runCatching { core?.switchAgentProvider(hostId, agentId, provider, model, resolvedEffort) }
            } else {
                runCatching { core?.setAgentConfig(hostId, agentId, model, resolvedEffort) }
            }
            reloadHostDetailInternal(hostId)
        }
    }

    fun selectSessionMode(mode: FfiSessionMode) {
        val normalized = OrbitFormatters.normalizeSessionMode(mode)
        val prev = _state.value
        _state.update { it.copy(composer = it.composer.copy(mode = normalized)) }
        val hostId = prev.selectedHostId ?: return
        val agentId = prev.selectedAgentId ?: return
        if (prev.isNewSessionMode) return
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

    fun executeSlashCommand(
        cmd: OrbitSlashCommand,
        onOpenTerminal: () -> Unit = {},
        onOpenSettings: () -> Unit = {},
    ) {
        when (cmd.id) {
            OrbitSlashCommandId.PLAN -> setSlashCommand("/plan")
            OrbitSlashCommandId.REVIEW -> setSlashCommand("/review")
            OrbitSlashCommandId.DIFF -> toggleDiffInspector()
            OrbitSlashCommandId.COMPACT -> submitSlashCompact("")
            OrbitSlashCommandId.SUPERVISED -> {
                selectSessionMode(FfiSessionMode.Default)
                postToast("Permissions: Always Ask")
            }
            OrbitSlashCommandId.AUTO -> {
                selectSessionMode(FfiSessionMode.AcceptEdits)
                postToast("Permissions: Accept Edits")
            }
            OrbitSlashCommandId.FULL -> {
                selectSessionMode(FfiSessionMode.FullAccess)
                postToast("Permissions: Full Access")
            }
            OrbitSlashCommandId.TERMINAL -> onOpenTerminal()
            OrbitSlashCommandId.NEW_SESSION -> startNewSession()
            OrbitSlashCommandId.SETTINGS -> onOpenSettings()
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
        val provider = cur.activeAgent?.provider ?: cur.composer.provider
        val cleanFocus = extraFocus.trim()
        val badges = listOf(FfiPromptBadge(kind = "slash", label = "/compact"))
        val wirePrompt = if (provider == FfiProviderKind.CLAUDE_CODE || provider == FfiProviderKind.ANTIGRAVITY) {
            if (cleanFocus.isNotEmpty()) "/compact $cleanFocus" else "/compact"
        } else {
            if (cleanFocus.isNotEmpty()) {
                "${buildHiddenWorkflowInstruction("compact")}\n\n<USER_REQUEST>\n$cleanFocus\n</USER_REQUEST>"
            } else {
                buildHiddenWorkflowInstruction("compact")
            }
        }
        runningAgentKeys.add(agentQueueKey(hostId, agentId))
        viewModelScope.launch(Dispatchers.IO) {
            try {
                core?.sendPromptWithMeta(
                    hostId = hostId,
                    agentId = agentId,
                    prompt = wirePrompt,
                    displayPrompt = cleanFocus,
                    badges = badges,
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

    private fun parseLeadingSlashDirective(rawText: String): Pair<String?, String> {
        val trimmed = rawText.trim()
        val match = Regex("""^/(plan|review|compact)(?:\s+([\s\S]*))?$""", RegexOption.IGNORE_CASE)
            .matchEntire(trimmed)
            ?: return null to trimmed
        val directive = match.groupValues[1].lowercase()
        val rest = match.groupValues[2].trim()
        return "/$directive" to rest
    }

    private fun buildHiddenWorkflowInstruction(directive: String): String =
        when (directive.lowercase().removePrefix("/")) {
            "plan" -> listOf(
                "<WORKFLOW_INSTRUCTION command=\"/plan\">",
                "You are invoked with the /plan workflow.",
                "First inspect and verify the relevant codebase files, architecture, and constraints.",
                "Clarify any critical ambiguities if needed, then present a concise, structured, step-by-step implementation plan.",
                "Do NOT modify files or run mutating commands until the user approves the plan.",
                "</WORKFLOW_INSTRUCTION>",
            ).joinToString("\n")
            "review" -> listOf(
                "<WORKFLOW_INSTRUCTION command=\"/review\">",
                "You are invoked with the /review workflow.",
                "Perform a thorough engineering code review of the attached changes.",
                "Focus on correctness bugs, regressions, edge cases, concurrency/state hazards, security, and adherence to project conventions.",
                "Cite exact file paths and line numbers, prioritize findings by severity, and do NOT modify files unless asked.",
                "</WORKFLOW_INSTRUCTION>",
            ).joinToString("\n")
            else -> listOf(
                "<WORKFLOW_INSTRUCTION command=\"/compact\">",
                "Summarize and compact our conversation context into a concise, high-signal checkpoint:",
                "1. Core goal & user preferences",
                "2. Key architectural decisions & completed changes (with file paths)",
                "3. Open issues & immediate next steps",
                "</WORKFLOW_INSTRUCTION>",
            ).joinToString("\n")
        }

    private fun buildPromptPayload(
        hostId: String,
        activeCwd: String,
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
    ): Triple<String, String, List<FfiPromptBadge>> {
        val rawTrimmed = text.trim()
        val (inlineSlash, parsedRest) = if (activeSlashCommand == null) {
            parseLeadingSlashDirective(rawTrimmed)
        } else {
            null to rawTrimmed
        }
        val effectiveSlash = activeSlashCommand ?: inlineSlash
        val cleanUserText = parsedRest

        val mergedFiles = mentionedFiles.toMutableList()
        var mergedWorkingDiff = includeGitDiff
        var mergedStagedDiff = includeStagedDiff
        var mergedTerminal = includeTerminal

        inlineMentions.forEach { mention ->
            if (!cleanUserText.contains(mention.token)) return@forEach
            when (mention.kind) {
                "file" -> mention.path?.let { p -> if (p !in mergedFiles) mergedFiles += p }
                "git_diff" -> if (mention.staged) mergedStagedDiff = true else mergedWorkingDiff = true
                "terminal" -> mergedTerminal = true
            }
        }

        val autoReviewWorkingDiff = effectiveSlash.equals("/review", ignoreCase = true) &&
            !mergedWorkingDiff &&
            !mergedStagedDiff
        if (autoReviewWorkingDiff) {
            mergedWorkingDiff = true
        }

        val badges = mutableListOf<FfiPromptBadge>()
        effectiveSlash?.let { cmd ->
            badges += FfiPromptBadge(kind = "slash", label = cmd)
        }
        stagedAttachments.forEach { att ->
            badges += FfiPromptBadge(
                kind = if (att.isImage) "image" else "file",
                label = att.filename,
            )
        }
        mergedFiles.forEach { path ->
            val shortName = OrbitFormatters.shortPath(path)
            if (!cleanUserText.contains("@$shortName")) {
                badges += FfiPromptBadge(kind = "file", label = "@$shortName")
            }
        }
        if (mergedWorkingDiff &&
            !autoReviewWorkingDiff &&
            !cleanUserText.contains("@working-diff")
        ) {
            badges += FfiPromptBadge(kind = "git_diff", label = "Working Tree Diff")
        }
        if (mergedStagedDiff && !cleanUserText.contains("@staged-diff")) {
            badges += FfiPromptBadge(kind = "git_diff", label = "Staged Diff")
        }
        if (mergedTerminal && !cleanUserText.contains("@terminal")) {
            badges += FfiPromptBadge(kind = "terminal", label = "Terminal")
        }
        quotedSnippets.forEach { q ->
            badges += FfiPromptBadge(kind = "quote", label = q.badgeLabel)
        }

        val displayPrompt = cleanUserText.ifBlank { "" }

        val imageAttachments = stagedAttachments.filter { it.isImage }
        val fileAttachments = stagedAttachments.filterNot { it.isImage }

        val fullPrompt = buildString {
            effectiveSlash?.let { cmd ->
                appendLine(buildHiddenWorkflowInstruction(cmd))
                appendLine()
            }
            if (imageAttachments.isNotEmpty()) {
                imageAttachments.forEach { img ->
                    appendLine("Attached image file: `${img.path}`")
                }
                appendLine()
            }
            val combinedFileRefs = mergedFiles + fileAttachments.map { it.path }
            if (combinedFileRefs.isNotEmpty()) {
                append("Referenced workspace files: ")
                appendLine(combinedFileRefs.joinToString(", ") { "`@$it`" })
                appendLine()
            }
            if (mergedWorkingDiff) {
                val diffText = if (!gitStagedMode) gitReview?.rawDiff?.takeIf { it.isNotBlank() } else null
                if (diffText != null) {
                    appendLine("Working Tree Diff:")
                    appendLine("```diff")
                    appendLine(diffText.take(14_000))
                    appendLine("```")
                    appendLine()
                } else {
                    appendLine("Context: Inspect working tree git changes in $activeCwd.")
                    appendLine()
                }
            }
            if (mergedStagedDiff) {
                val stagedPaths = gitReview?.changedFiles.orEmpty().filter { it.staged }.map { it.path }
                val stagedPatch = if (gitStagedMode) gitReview?.rawDiff?.takeIf { it.isNotBlank() } else null
                if (stagedPatch != null) {
                    appendLine("Staged Diff:")
                    appendLine("```diff")
                    appendLine(stagedPatch.take(14_000))
                    appendLine("```")
                    appendLine()
                } else {
                    appendLine("Context: Inspect staged git changes in $activeCwd.")
                    if (stagedPaths.isNotEmpty()) {
                        stagedPaths.forEach { sp -> appendLine("- $sp") }
                    }
                    appendLine()
                }
            }
            if (mergedTerminal) {
                val scr = terminalScreen ?: runCatching { core?.terminalScreen(hostId) }.getOrNull()
                val termLines = scr?.rowsData.orEmpty()
                    .map { row -> row.spans.joinToString("") { it.text }.trimEnd() }
                    .dropLastWhile { it.isBlank() }
                if (termLines.isNotEmpty()) {
                    appendLine("Active terminal output:")
                    appendLine("```text")
                    appendLine(termLines.takeLast(60).joinToString("\n").take(6_000))
                    appendLine("```")
                    appendLine()
                } else {
                    appendLine("[Inspect active PTY terminal session in $activeCwd]")
                    appendLine()
                }
            }
            if (quotedSnippets.isNotEmpty()) {
                val diffSnippets = quotedSnippets.filterNot { it.isAssistantExcerpt }
                val replyExcerpts = quotedSnippets.filter { it.isAssistantExcerpt }
                if (diffSnippets.isNotEmpty()) {
                    diffSnippets.forEach { q ->
                        appendLine("Reference `${q.path}:${q.lineLabel}`:")
                        appendLine("```")
                        appendLine(q.content)
                        appendLine("```")
                    }
                    appendLine()
                }
                if (replyExcerpts.isNotEmpty()) {
                    appendLine("Quoted assistant context:")
                    replyExcerpts.forEach { q ->
                        q.content.lineSequence().forEach { ln ->
                            appendLine("> $ln")
                        }
                        appendLine()
                    }
                }
            }
            if (cleanUserText.isNotEmpty()) {
                append(cleanUserText)
            }
        }.trim()

        return Triple(displayPrompt, fullPrompt, badges)
    }

    fun enqueueComposerPrompt() {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val draft = cur.composer
        if (!draft.canSend) return

        val (inlineSlash, cleanRest) = if (draft.activeSlashCommand == null) {
            parseLeadingSlashDirective(draft.text)
        } else {
            null to draft.text.trim()
        }
        val effectiveSlash = draft.activeSlashCommand ?: inlineSlash
        if (effectiveSlash == "/compact") {
            _state.update { prev ->
                prev.copy(
                    composer = prev.composer.copy(
                        text = "",
                        activeSlashCommand = null,
                    )
                )
            }
            submitSlashCompact(cleanRest)
            return
        }

        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: run {
            submitComposerPrompt()
            return
        }

        val (displayPrompt, _, badges) = buildPromptPayload(
            hostId = hostId,
            activeCwd = cur.activeCwd,
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
        )

        val item = QueuedPromptItem(
            text = draft.text,
            previewText = displayPrompt.lineSequence().firstOrNull()?.trim().orEmpty()
                .ifBlank { badges.joinToString(" ") { it.label } },
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
        runningAgentKeys.add(key)

        _state.update { prev ->
            val updatedList = prev.queuedPromptsByAgent[key].orEmpty() + item
            prev.copy(
                queuedPromptsByAgent = prev.queuedPromptsByAgent + (key to updatedList),
                composer = prev.composer.copy(
                    text = "",
                    mentionedFiles = emptyList(),
                    stagedAttachments = emptyList(),
                    inlineMentions = emptyList(),
                    includeGitDiff = false,
                    includeStagedDiff = false,
                    includeTerminal = false,
                    activeSlashCommand = null,
                    quotedSnippets = emptyList(),
                ),
                toastMessage = "Queued next prompt",
            )
        }
    }

    fun removeQueuedPrompt(itemId: String) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: return
        val key = agentQueueKey(hostId, agentId)
        _state.update { prev ->
            val remaining = prev.queuedPromptsByAgent[key].orEmpty().filterNot { it.id == itemId }
            val nextMap = if (remaining.isEmpty()) {
                prev.queuedPromptsByAgent - key
            } else {
                prev.queuedPromptsByAgent + (key to remaining)
            }
            prev.copy(queuedPromptsByAgent = nextMap)
        }
    }

    fun editQueuedPrompt(itemId: String) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: return
        val key = agentQueueKey(hostId, agentId)
        val target = cur.queuedPromptsByAgent[key].orEmpty().firstOrNull { it.id == itemId } ?: return
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
    }

    fun sendQueuedPromptNow(itemId: String) {
        val cur = _state.value
        val hostId = cur.selectedHostId ?: return
        val agentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode } ?: return
        val key = agentQueueKey(hostId, agentId)
        val target = cur.queuedPromptsByAgent[key].orEmpty().firstOrNull { it.id == itemId } ?: return
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
            dispatchQueuedPromptItem(hostId, agentId, target)
        }
    }

    private fun drainNextQueuedPromptIfSettled(hostId: String, agentId: String) {
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

    private fun dispatchQueuedPromptItem(hostId: String, agentId: String, item: QueuedPromptItem) {
        val cur = _state.value
        val key = agentQueueKey(hostId, agentId)
        val (displayPrompt, fullPrompt, badges) = buildPromptPayload(
            hostId = hostId,
            activeCwd = cur.activeCwd,
            gitReview = cur.gitReview,
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
        )
        try {
            runningAgentKeys.add(key)
            core?.sendPromptWithMeta(
                hostId = hostId,
                agentId = agentId,
                prompt = fullPrompt,
                displayPrompt = displayPrompt,
                badges = badges,
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

        val (inlineSlash, cleanRest) = if (draft.activeSlashCommand == null) {
            parseLeadingSlashDirective(draft.text)
        } else {
            null to draft.text.trim()
        }
        val effectiveSlash = draft.activeSlashCommand ?: inlineSlash
        if (effectiveSlash == "/compact") {
            _state.update { prev ->
                prev.copy(
                    composer = prev.composer.copy(
                        text = "",
                        activeSlashCommand = null,
                    )
                )
            }
            submitSlashCompact(cleanRest)
            return
        }

        val (displayPrompt, fullPrompt, badges) = buildPromptPayload(
            hostId = hostId,
            activeCwd = cur.activeCwd,
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
        )

        // Clear composer text & transient attachments immediately for responsive feel
        _state.update { prev ->
            prev.copy(
                composer = prev.composer.copy(
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
            )
        }

        viewModelScope.launch(Dispatchers.IO) {
            try {
                val targetAgentId = cur.selectedAgentId?.takeIf { !cur.isNewSessionMode }
                if (targetAgentId == null) {
                    val createdId = core?.createAgentFull(
                        hostId = hostId,
                        cwd = cur.activeCwd.ifBlank { "~" },
                        extraDirs = draft.extraDirs,
                        provider = draft.provider,
                        model = draft.model,
                        effort = draft.effort,
                        mode = draft.mode,
                        initialPrompt = fullPrompt,
                        initialDisplayPrompt = displayPrompt,
                        initialBadges = badges,
                        worktreeBranch = draft.worktreeBranch?.takeIf { it.isNotBlank() }
                            ?: if (cur.draftUseWorktree) "auto" else null,
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
                } else {
                    runningAgentKeys.add(agentQueueKey(hostId, targetAgentId))
                    core?.sendPromptWithMeta(
                        hostId = hostId,
                        agentId = targetAgentId,
                        prompt = fullPrompt,
                        displayPrompt = displayPrompt,
                        badges = badges,
                    )
                    reloadStageInternal(hostId, targetAgentId)
                }
            } catch (t: Throwable) {
                _state.update { it.copy(lastError = extractErrorMessage(t, "Failed to send prompt")) }
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

    fun describeStep(item: FfiTimelineItem): FfiToolStepDescriptor? {
        if (item is FfiTimelineItem.ToolCall && item.stepDescriptor != null) {
            return item.stepDescriptor
        }
        return runCatching { core?.describeToolItem(item, _state.value.activeCwd) }.getOrNull()
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
                _state.update { it.copy(gitReview = review) }
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

    fun sendTerminalBytes(bytes: ByteArray, slot: UByte = 1u) {
        val hostId = _state.value.selectedHostId ?: return
        if (bytes.isEmpty()) return
        viewModelScope.launch(Dispatchers.IO) {
            runCatching { core?.sendTerminalBytes(hostId, slot, bytes) }
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

    override fun onCleared() {
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
        private const val KEY_DRAFT_USE_WORKTREE = "draft_use_worktree"

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
