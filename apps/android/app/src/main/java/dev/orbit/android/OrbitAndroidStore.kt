package dev.orbit.android

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.orbit_ffi_uniffi.FfiAgentTimeline
import uniffi.orbit_ffi_uniffi.FfiCoreEvent
import uniffi.orbit_ffi_uniffi.FfiGitReview
import uniffi.orbit_ffi_uniffi.FfiHostCard
import uniffi.orbit_ffi_uniffi.FfiHostDetail
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiSessionMode
import uniffi.orbit_ffi_uniffi.FfiTerminalScreen
import uniffi.orbit_ffi_uniffi.FfiTimelineItem
import uniffi.orbit_ffi_uniffi.MobileCoreListener
import uniffi.orbit_ffi_uniffi.OrbitMobileCore
import java.io.File

data class OrbitUiState(
    val hosts: List<FfiHostCard> = emptyList(),
    val totalPendingApprovals: UInt = 0u,
    val selectedHostId: String? = null,
    val hostDetail: FfiHostDetail? = null,
    val selectedAgentId: String? = null,
    val timelineItems: List<FfiTimelineItem> = emptyList(),
    val gitReview: FfiGitReview? = null,
    val terminalScreen: FfiTerminalScreen? = null,
    val lastError: String? = null,
)

class OrbitAndroidStore(application: Application) : AndroidViewModel(application) {
    private val _state = MutableStateFlow(OrbitUiState())
    val state: StateFlow<OrbitUiState> = _state.asStateFlow()

    private var core: OrbitMobileCore? = null

    private val listener = object : MobileCoreListener {
        override fun onCoreEvent(event: FfiCoreEvent) {
            viewModelScope.launch {
                handleCoreEvent(event)
            }
        }
    }

    init {
        val dbFile = File(application.filesDir, "orbit_android_v2.sqlite")
        try {
            val instance = OrbitMobileCore.open(dbFile.absolutePath)
            core = instance
            instance.subscribe(listener)
            reloadFleet()
            instance.connectAllHosts()
        } catch (t: Throwable) {
            _state.update { it.copy(lastError = t.message ?: "Failed to load native Rust core") }
        }
    }

    private fun handleCoreEvent(event: FfiCoreEvent) {
        val cur = _state.value
        when (event) {
            is FfiCoreEvent.FleetChanged -> {
                reloadFleet()
                cur.selectedHostId?.let { reloadHostDetail(it) }
            }
            is FfiCoreEvent.HostChanged -> {
                reloadFleet()
                if (event.hostId == cur.selectedHostId) {
                    reloadHostDetail(event.hostId)
                }
            }
            is FfiCoreEvent.TimelineChanged -> {
                if (event.hostId == cur.selectedHostId && event.agentId == cur.selectedAgentId) {
                    reloadTimeline(event.hostId, event.agentId)
                }
            }
            is FfiCoreEvent.GitChanged -> {
                if (event.hostId == cur.selectedHostId) {
                    val gr = core?.gitReview(event.hostId, event.cwd, null)
                    _state.update { it.copy(gitReview = gr) }
                }
            }
            is FfiCoreEvent.TerminalChanged -> {
                if (event.hostId == cur.selectedHostId) {
                    val scr = core?.terminalScreen(event.hostId)
                    _state.update { it.copy(terminalScreen = scr) }
                }
            }
        }
    }

    fun reloadFleet() {
        val snap = core?.fleetOverview() ?: return
        _state.update { prev ->
            val nextHostId = prev.selectedHostId ?: snap.hosts.firstOrNull()?.hostId
            prev.copy(
                hosts = snap.hosts,
                totalPendingApprovals = snap.totalPendingApprovals,
                selectedHostId = nextHostId,
            )
        }
        _state.value.selectedHostId?.let { reloadHostDetail(it) }
    }

    fun selectHost(hostId: String) {
        _state.update { it.copy(selectedHostId = hostId, selectedAgentId = null, timelineItems = emptyList()) }
        reloadHostDetail(hostId)
        core?.refreshQuotas(hostId)
    }

    private fun reloadHostDetail(hostId: String) {
        val detail = core?.hostDetail(hostId)
        _state.update { prev ->
            val nextAgent = prev.selectedAgentId ?: detail?.agents?.firstOrNull()?.agentId
            prev.copy(hostDetail = detail, selectedAgentId = nextAgent)
        }
        val activeAgent = _state.value.selectedAgentId
        if (activeAgent != null) {
            reloadTimeline(hostId, activeAgent)
        }
    }

    fun selectAgent(agentId: String) {
        val hid = _state.value.selectedHostId ?: return
        _state.update { it.copy(selectedAgentId = agentId) }
        core?.openAgentTimeline(hid, agentId)
        reloadTimeline(hid, agentId)
    }

    private fun reloadTimeline(hostId: String, agentId: String) {
        val tl: FfiAgentTimeline = core?.agentTimeline(hostId, agentId) ?: return
        _state.update { it.copy(timelineItems = tl.items) }
    }

    fun pairHost(uri: String) {
        val trimmed = uri.trim()
        if (trimmed.isEmpty()) return
        try {
            val card = core?.pairHost(trimmed)
            _state.update { it.copy(lastError = null) }
            reloadFleet()
            card?.hostId?.let { selectHost(it) }
        } catch (t: Throwable) {
            _state.update { it.copy(lastError = t.message) }
        }
    }

    fun removeHost(hostId: String) {
        runCatching { core?.removeHost(hostId) }
        _state.update {
            it.copy(
                selectedHostId = if (it.selectedHostId == hostId) null else it.selectedHostId,
                hostDetail = if (it.selectedHostId == hostId) null else it.hostDetail,
            )
        }
        reloadFleet()
    }

    fun createAgent(cwd: String, provider: FfiProviderKind, prompt: String) {
        val hid = _state.value.selectedHostId ?: return
        if (prompt.isBlank()) return
        core?.createAgent(hid, cwd.ifBlank { "~" }, provider, FfiSessionMode.AcceptEdits, prompt.trim(), null)
    }

    fun sendPrompt(prompt: String) {
        val cur = _state.value
        val hid = cur.selectedHostId ?: return
        val aid = cur.selectedAgentId ?: return
        if (prompt.isBlank()) return
        core?.sendPrompt(hid, aid, prompt.trim())
    }

    fun interruptAgent() {
        val cur = _state.value
        val hid = cur.selectedHostId ?: return
        val aid = cur.selectedAgentId ?: return
        core?.interruptAgent(hid, aid)
    }

    fun resolvePermission(requestId: String, decision: String) {
        val cur = _state.value
        val hid = cur.selectedHostId ?: return
        val aid = cur.selectedAgentId ?: return
        core?.resolvePermission(hid, aid, requestId, decision)
    }

    fun answerQuestion(requestId: String, option: String) {
        val cur = _state.value
        val hid = cur.selectedHostId ?: return
        val aid = cur.selectedAgentId ?: return
        core?.answerQuestion(hid, aid, requestId, listOf(listOf(option)))
    }

    fun loadGitReview(cwd: String) {
        val hid = _state.value.selectedHostId ?: return
        core?.fetchGitStatus(hid, cwd)
        core?.fetchGitDiff(hid, cwd, null, false)
        _state.update { it.copy(gitReview = core?.gitReview(hid, cwd, null)) }
    }

    fun startTerminal(cwd: String) {
        val hid = _state.value.selectedHostId ?: return
        core?.startTerminal(hid, cwd, 84u.toUShort(), 28u.toUShort())
        _state.update { it.copy(terminalScreen = core?.terminalScreen(hid)) }
    }

    fun sendTerminalKeys(keys: String) {
        val hid = _state.value.selectedHostId ?: return
        core?.sendTerminalKeys(hid, keys)
    }

    override fun onCleared() {
        core?.close()
        super.onCleared()
    }
}
