package dev.orbit.android.store

import uniffi.orbit_ffi_uniffi.FfiAgentStage
import uniffi.orbit_ffi_uniffi.FfiAgentStageDelta
import uniffi.orbit_ffi_uniffi.FfiConnectionState
import uniffi.orbit_ffi_uniffi.FfiHostCard
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiPushPreferences
import uniffi.orbit_ffi_uniffi.FfiReasoningEffort
import uniffi.orbit_ffi_uniffi.FfiSessionMode
import uniffi.orbit_ffi_uniffi.FfiTimelineItem
import uniffi.orbit_ffi_uniffi.FfiToolCallDetail
import uniffi.orbit_ffi_uniffi.GuardianAssessment
import uniffi.orbit_ffi_uniffi.GuardianRiskLevel
import uniffi.orbit_ffi_uniffi.GuardianDecision
import uniffi.orbit_ffi_uniffi.PlanApprovalAction
import uniffi.orbit_ffi_uniffi.ProviderKind
import uniffi.orbit_ffi_uniffi.PushPreferences
import uniffi.orbit_ffi_uniffi.ReasoningEffort
import uniffi.orbit_ffi_uniffi.SessionMode

/**
 * Stationary right-circle composer action state (Codex-style voice reuse: NEVER an extra mic button).
 */
enum class ComposerRightActionState {
    /** Empty draft + idle agent -> tap to start voice dictation. */
    VOICE,
    /** Voice dictation currently active -> tap to stop & commit recognized text. */
    RECORDING_STOP,
    /** Text or context/attachment present + idle agent -> tap to send prompt. */
    SEND,
    /** Agent running + empty draft -> tap to interrupt/stop active turn. */
    STOP,
    /** Agent running + text/attachment present -> tap to enqueue steer/follow-up prompt. */
    QUEUE,
}

object ComposerActionReducer {
    fun resolve(
        isRecordingVoice: Boolean,
        isAgentRunning: Boolean,
        canSend: Boolean,
    ): ComposerRightActionState = when {
        isRecordingVoice -> ComposerRightActionState.RECORDING_STOP
        isAgentRunning && canSend -> ComposerRightActionState.QUEUE
        isAgentRunning -> ComposerRightActionState.STOP
        canSend -> ComposerRightActionState.SEND
        else -> ComposerRightActionState.VOICE
    }
}

/**
 * Pure stage delta merge supporting `auxUnchanged` insight stability and tail-pagination metadata.
 */
data class StageMergeOutcome(
    val stage: FfiAgentStage,
    val hasOlderTurns: Boolean,
    val oldestLoadedSeq: ULong,
    val auxRevision: ULong,
)

object StageDeltaReducer {
    fun merge(
        prevStage: FfiAgentStage?,
        hostId: String,
        agentId: String,
        delta: FfiAgentStageDelta,
    ): StageMergeOutcome {
        val sameStage = prevStage != null &&
            prevStage.hostId == hostId &&
            (prevStage.agent?.agentId ?: agentId) == agentId
        val incremental = sameStage && !delta.reset && delta.replaceFromTurnIndex <= prevStage!!.turns.size.toUInt()
        val previous = prevStage.takeIf { incremental }
        val aux = previous.takeIf { delta.auxUnchanged }
        val mergedStage = FfiAgentStage(
            revision = delta.revision,
            hostId = delta.hostId,
            agent = delta.agent ?: previous?.agent,
            turns = previous?.let { it.turns.take(delta.replaceFromTurnIndex.toInt()) + delta.changedTurns } ?: delta.changedTurns,
            pendingPermissions = aux?.pendingPermissions ?: delta.pendingPermissions,
            pendingQuestions = aux?.pendingQuestions ?: delta.pendingQuestions,
            sessionTurnDiffs = aux?.sessionTurnDiffs ?: delta.sessionTurnDiffs,
            insights = aux?.insights ?: delta.insights,
            latestSeq = delta.latestSeq,
        )
        return StageMergeOutcome(
            stage = mergedStage,
            hasOlderTurns = delta.hasOlderTurns,
            oldestLoadedSeq = delta.oldestLoadedSeq,
            auxRevision = delta.auxRevision,
        )
    }

    /**
     * Computes the restored LazyColumn index after older turns are prepended at the top so the
     * user's reading viewport remains anchored to the exact same turn key and pixel offset.
     */
    fun resolvePrependedAnchorIndex(
        anchorTurnKey: String?,
        newTurnKeys: List<String>,
        fallbackPrependDelta: Int,
    ): Int? {
        if (newTurnKeys.isEmpty()) return null
        if (!anchorTurnKey.isNullOrEmpty()) {
            val idx = newTurnKeys.indexOf(anchorTurnKey)
            if (idx >= 0) return idx
        }
        return fallbackPrependDelta.coerceIn(0, newTurnKeys.lastIndex)
    }
}

/**
 * Pure Plan Approval detector & submission payload helper aligned with Desktop `plan-review-card-model.ts`.
 */
data class PlanApprovalSpec(
    val requestId: String,
    val headerTitle: String,
    val planMarkdown: String,
)

data class PlanApprovalSubmitPayload(
    val action: PlanApprovalAction,
    val editedPlanMarkdown: String?,
    val feedback: String?,
    val targetMode: SessionMode?,
)

object PlanApprovalReducer {
    private val GENERIC_PLAN_TITLES = setOf(
        "exitplanmode",
        "exit_plan_mode",
        "plan approval",
        "review implementation plan",
        "approve tool execution",
    )

    fun detectPlanApproval(permission: FfiTimelineItem.PermissionRequest): PlanApprovalSpec? {
        val titleLower = permission.title.trim().lowercase()
        val detailToolLower = (permission.detail as? FfiToolCallDetail.Generic)?.toolName?.trim()?.lowercase().orEmpty()
        val decisionsLower = permission.allowedDecisions.map { it.trim().lowercase() }

        val isPlanGate = titleLower.contains("exitplanmode") ||
            titleLower.contains("exit_plan_mode") ||
            titleLower.contains("plan approval") ||
            titleLower.contains("implementation plan") ||
            detailToolLower == "exitplanmode" ||
            detailToolLower == "exit_plan_mode" ||
            decisionsLower.any { it == "approve_and_build" || it == "request_changes" }

        if (!isPlanGate) return null

        val extractedMarkdown = extractPlanMarkdown(permission)
        val headerTitle = permission.title.trim().let { raw ->
            if (raw.isEmpty() || raw.lowercase() in GENERIC_PLAN_TITLES || raw.lowercase().startsWith("claude wants to use exitplanmode")) {
                "Implementation Plan"
            } else {
                raw
            }
        }
        return PlanApprovalSpec(
            requestId = permission.requestId,
            headerTitle = headerTitle,
            planMarkdown = extractedMarkdown,
        )
    }

    private fun extractPlanMarkdown(permission: FfiTimelineItem.PermissionRequest): String {
        val generic = permission.detail as? FfiToolCallDetail.Generic
        if (generic != null && generic.argumentsJson.isNotBlank()) {
            val rawJson = generic.argumentsJson
            val match = Regex("\"plan\"\\s*:\\s*\"((?:[^\"\\\\]|\\\\.)*)\"").find(rawJson)
            if (match != null) {
                return unescapeJsonString(match.groupValues[1])
            }
        }
        return permission.commandOrPath.ifBlank { permission.title }
    }

    internal fun unescapeJsonString(escaped: String): String {
        val out = StringBuilder(escaped.length)
        var i = 0
        while (i < escaped.length) {
            val ch = escaped[i]
            if (ch == '\\' && i + 1 < escaped.length) {
                when (val next = escaped[i + 1]) {
                    '\\' -> { out.append('\\'); i += 2 }
                    '"' -> { out.append('"'); i += 2 }
                    '/' -> { out.append('/'); i += 2 }
                    'n' -> { out.append('\n'); i += 2 }
                    'r' -> { out.append('\r'); i += 2 }
                    't' -> { out.append('\t'); i += 2 }
                    'b' -> { out.append('\b'); i += 2 }
                    'f' -> { out.append('\u000C'); i += 2 }
                    'u' -> {
                        if (i + 5 < escaped.length) {
                            val code = escaped.substring(i + 2, i + 6).toIntOrNull(16)
                            if (code != null) {
                                out.append(code.toChar())
                                i += 6
                                continue
                            }
                        }
                        out.append("\\u")
                        i += 2
                    }
                    else -> { out.append(next); i += 2 }
                }
            } else {
                out.append(ch)
                i++
            }
        }
        return out.toString()
    }

    fun buildSubmitPayload(
        action: PlanApprovalAction,
        originalMarkdown: String,
        draftMarkdown: String,
        feedback: String,
    ): PlanApprovalSubmitPayload {
        if (action == PlanApprovalAction.DISMISS) {
            return PlanApprovalSubmitPayload(
                action = action,
                editedPlanMarkdown = null,
                feedback = null,
                targetMode = null,
            )
        }
        val modified = originalMarkdown.replace("\r\n", "\n").trim() !=
            draftMarkdown.replace("\r\n", "\n").trim()
        val trimmedFeedback = feedback.trim().takeIf { it.isNotEmpty() }
        return PlanApprovalSubmitPayload(
            action = action,
            editedPlanMarkdown = if (modified) draftMarkdown else null,
            feedback = trimmedFeedback,
            targetMode = if (action == PlanApprovalAction.APPROVE_AND_BUILD) SessionMode.Default else null,
        )
    }
}

/**
 * Pure overview telemetry & safe low-risk batch approval policy.
 *
 * Security invariant: NEVER includes plan approvals, shell commands with destructive verbs,
 * or non-read/non-safe operations in batch auto-approve candidates.
 */
enum class OverviewHealthTone {
    CALM,
    ACTIVE,
    ATTENTION,
    OFFLINE,
}

data class OverviewCalmSummary(
    val tone: OverviewHealthTone,
    val onlineHostCount: Int,
    val totalHostCount: Int,
    val runningAgentCount: Int,
    val pendingDecisionCount: Int,
    val offlineHostCount: Int,
)

object OverviewSafetyReducer {
    fun isSafeLowRiskBatchCandidate(
        permission: FfiTimelineItem.PermissionRequest,
        assessment: GuardianAssessment?,
    ): Boolean =
        assessment?.riskLevel == GuardianRiskLevel.LOW &&
            assessment.decision != GuardianDecision.AUTO_DENY &&
            permission.resolvedDecision == null &&
            "allow" in permission.allowedDecisions &&
            PlanApprovalReducer.detectPlanApproval(permission) == null &&
            (permission.detail is FfiToolCallDetail.FileRead || permission.detail is FfiToolCallDetail.Search)

    fun summarizeFleet(
        hosts: List<FfiHostCard>,
        runningAgentCount: Int,
        pendingDecisionCount: Int,
    ): OverviewCalmSummary {
        val online = hosts.count {
            it.connectionState == FfiConnectionState.ONLINE_DIRECT ||
                it.connectionState == FfiConnectionState.ONLINE_RELAY
        }
        val offline = hosts.count { it.connectionState == FfiConnectionState.DISCONNECTED }
        val tone = when {
            pendingDecisionCount > 0 -> OverviewHealthTone.ATTENTION
            hosts.isNotEmpty() && online == 0 -> OverviewHealthTone.OFFLINE
            runningAgentCount > 0 -> OverviewHealthTone.ACTIVE
            else -> OverviewHealthTone.CALM
        }
        return OverviewCalmSummary(
            tone = tone,
            onlineHostCount = online,
            totalHostCount = hosts.size,
            runningAgentCount = runningAgentCount,
            pendingDecisionCount = pendingDecisionCount,
            offlineHostCount = offline,
        )
    }
}

/**
 * Lossless conversions between UniFFI Contract v2 `Ffi*` records and newly exported M2-Core types.
 */
object OrbitFfiTypeBridge {
    fun toProviderKind(p: FfiProviderKind): ProviderKind = when (p) {
        FfiProviderKind.ANTIGRAVITY -> ProviderKind.ANTIGRAVITY
        FfiProviderKind.CODEX -> ProviderKind.CODEX
        FfiProviderKind.CLAUDE_CODE -> ProviderKind.CLAUDE_CODE
        FfiProviderKind.OPEN_CODE -> ProviderKind.OPEN_CODE
        FfiProviderKind.GEMINI_CLI -> ProviderKind.GEMINI_CLI
        FfiProviderKind.CURSOR_AGENT -> ProviderKind.CURSOR_AGENT
        FfiProviderKind.COPILOT -> ProviderKind.COPILOT
        FfiProviderKind.GOOSE -> ProviderKind.GOOSE
        FfiProviderKind.PI -> ProviderKind.PI
        FfiProviderKind.KIMI -> ProviderKind.KIMI
        FfiProviderKind.CLINE -> ProviderKind.CLINE
        FfiProviderKind.MISTRAL_VIBE -> ProviderKind.MISTRAL_VIBE
        FfiProviderKind.AUGMENT -> ProviderKind.AUGMENT
        FfiProviderKind.ACP -> ProviderKind.ACP
    }

    fun fromProviderKind(p: ProviderKind): FfiProviderKind = when (p) {
        ProviderKind.ANTIGRAVITY -> FfiProviderKind.ANTIGRAVITY
        ProviderKind.CODEX -> FfiProviderKind.CODEX
        ProviderKind.CLAUDE_CODE -> FfiProviderKind.CLAUDE_CODE
        ProviderKind.OPEN_CODE -> FfiProviderKind.OPEN_CODE
        ProviderKind.GEMINI_CLI -> FfiProviderKind.GEMINI_CLI
        ProviderKind.CURSOR_AGENT -> FfiProviderKind.CURSOR_AGENT
        ProviderKind.COPILOT -> FfiProviderKind.COPILOT
        ProviderKind.GOOSE -> FfiProviderKind.GOOSE
        ProviderKind.PI -> FfiProviderKind.PI
        ProviderKind.KIMI -> FfiProviderKind.KIMI
        ProviderKind.CLINE -> FfiProviderKind.CLINE
        ProviderKind.MISTRAL_VIBE -> FfiProviderKind.MISTRAL_VIBE
        ProviderKind.AUGMENT -> FfiProviderKind.AUGMENT
        ProviderKind.ACP -> FfiProviderKind.ACP
    }

    fun toSessionMode(mode: FfiSessionMode): SessionMode = when (mode) {
        FfiSessionMode.Default -> SessionMode.Default
        FfiSessionMode.Auto -> SessionMode.Auto
        FfiSessionMode.Plan -> SessionMode.Plan
        FfiSessionMode.AcceptEdits -> SessionMode.AcceptEdits
        FfiSessionMode.FullAccess -> SessionMode.FullAccess
        is FfiSessionMode.Custom -> SessionMode.Custom(mode.name)
    }

    fun toReasoningEffort(effort: FfiReasoningEffort): ReasoningEffort = when (effort) {
        FfiReasoningEffort.OFF -> ReasoningEffort.OFF
        FfiReasoningEffort.LOW -> ReasoningEffort.LOW
        FfiReasoningEffort.MEDIUM -> ReasoningEffort.MEDIUM
        FfiReasoningEffort.HIGH -> ReasoningEffort.HIGH
        FfiReasoningEffort.XHIGH -> ReasoningEffort.XHIGH
        FfiReasoningEffort.MAX -> ReasoningEffort.MAX
        FfiReasoningEffort.ULTRA -> ReasoningEffort.ULTRA
    }

    fun toPushPreferences(ffi: FfiPushPreferences): PushPreferences = PushPreferences(
        enabled = ffi.enabled,
        notifyPermissionRequired = ffi.notifyPermissionRequired,
        notifyTurnCompleted = ffi.notifyTurnCompleted,
        notifyAgentError = ffi.notifyAgentError,
        notifyGoalFinished = ffi.notifyGoalFinished,
        notifyTimerFired = ffi.notifyTimerFired,
        includeMessagePreview = ffi.includeMessagePreview,
        quietHoursEnabled = ffi.quietHoursEnabled,
        quietStartHour = ffi.quietStartHour,
        quietEndHour = ffi.quietEndHour,
        pushGatewayUrl = ffi.pushGatewayUrl,
        hasGatewayAuthToken = ffi.hasGatewayAuthToken,
    )
}
