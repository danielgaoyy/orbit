package dev.orbit.android

import dev.orbit.android.store.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.orbit_ffi_uniffi.*

class OrbitReducersTest {
    private val read =
        FfiTimelineItem.PermissionRequest(
            2u,
            "req",
            "Read file",
            "README.md",
            FfiToolCallDetail.FileRead("README.md", null, null),
            listOf("allow", "deny"),
            null,
            null,
        )
    private val low =
        GuardianAssessment(
            GuardianRiskLevel.LOW,
            GuardianDecision.REQUIRE_HUMAN,
            1f,
            "local",
            "",
            emptyList(),
            0u,
        )

    @Test
    fun composerHasOneActionForEachState() {
        for (running in listOf(false, true)) for (draft in listOf(false, true)) {
            assertEquals(
                ComposerRightActionState.RECORDING_STOP,
                ComposerActionReducer.resolve(true, running, draft),
            )
        }
        assertEquals(
            ComposerRightActionState.VOICE,
            ComposerActionReducer.resolve(false, false, false),
        )
        assertEquals(
            ComposerRightActionState.STOP,
            ComposerActionReducer.resolve(false, true, false),
        )
        assertEquals(
            ComposerRightActionState.SEND,
            ComposerActionReducer.resolve(false, false, true),
        )
        assertEquals(
            ComposerRightActionState.QUEUE,
            ComposerActionReducer.resolve(false, true, true),
        )
        assertTrue(ComposerDraftState(mentionedFiles = listOf("main.kt")).canSend)
    }

    @Test
    fun onlyExplicitNavigationCommandsCanLeaveDictation() {
        val phrases =
            mapOf(
                dev.orbit.android.ui.composer.VoiceNavigation.SEARCH to "Open global search",
                dev.orbit.android.ui.composer.VoiceNavigation.OVERVIEW to "Open overview",
            )
        fun command(text: String) =
            dev.orbit.android.ui.composer.VoiceNavigation.fromTranscript(text, phrases::getValue)
        assertEquals(
            dev.orbit.android.ui.composer.VoiceNavigation.OVERVIEW,
            command("Open overview."),
        )
        assertEquals(
            dev.orbit.android.ui.composer.VoiceNavigation.SEARCH,
            command("open global search"),
        )
        assertNull(command("Approve all"))
        assertNull(command("Open global search and run a command"))
    }

    @Test
    fun fleetSummaryPrioritizesDecisionsAndCountsBothTransports() {
        val direct =
            FfiHostCard(
                "d",
                "Direct",
                "d",
                null,
                null,
                FfiHostRouteMode.AUTO,
                6868u,
                null,
                false,
                null,
                null,
                null,
                1u,
                null,
                "",
                emptyList(),
                null,
                FfiConnectionState.ONLINE_DIRECT,
                null,
                emptyList(),
                0u,
                0u,
                0u,
                false,
                false,
            )
        val down = direct.copy(hostId = "x", connectionState = FfiConnectionState.DISCONNECTED)
        val fleet =
            listOf(
                direct,
                direct.copy(hostId = "r", connectionState = FfiConnectionState.ONLINE_RELAY),
                down,
            )
        val waiting = OverviewSafetyReducer.summarizeFleet(fleet, 1, 2)
        assertEquals(OverviewHealthTone.ATTENTION, waiting.tone)
        assertEquals(2, waiting.onlineHostCount)
        assertEquals(1, waiting.offlineHostCount)
        assertEquals(
            OverviewHealthTone.ACTIVE,
            OverviewSafetyReducer.summarizeFleet(fleet, 1, 0).tone,
        )
        assertEquals(
            OverviewHealthTone.OFFLINE,
            OverviewSafetyReducer.summarizeFleet(listOf(down), 0, 0).tone,
        )
        assertEquals(
            OverviewHealthTone.CALM,
            OverviewSafetyReducer.summarizeFleet(listOf(direct), 0, 0).tone,
        )
    }

    @Test
    fun providerTypesRoundTripAcrossTheFfiBridge() {
        FfiProviderKind.entries.forEach {
            assertEquals(
                it,
                OrbitFfiTypeBridge.fromProviderKind(OrbitFfiTypeBridge.toProviderKind(it)),
            )
        }
    }

    @Test
    fun batchRequiresAnExplicitLowRiskAssessmentAndReadOnlyRequest() {
        assertTrue(OverviewSafetyReducer.isSafeLowRiskBatchCandidate(read, low))
        for (risk in GuardianRiskLevel.entries.filter { it != GuardianRiskLevel.LOW }) {
            assertFalse(
                OverviewSafetyReducer.isSafeLowRiskBatchCandidate(read, low.copy(riskLevel = risk))
            )
        }
        assertFalse(OverviewSafetyReducer.isSafeLowRiskBatchCandidate(read, null))
        assertFalse(
            OverviewSafetyReducer.isSafeLowRiskBatchCandidate(
                read,
                low.copy(decision = GuardianDecision.AUTO_DENY),
            )
        )
        listOf(
                read.copy(resolvedDecision = "deny"),
                read.copy(title = "Implementation plan"),
                read.copy(allowedDecisions = listOf("request_changes", "approve_and_build")),
                read.copy(allowedDecisions = listOf("deny")),
                read.copy(detail = FfiToolCallDetail.Shell("git status", null, null, null)),
            )
            .forEach { assertFalse(OverviewSafetyReducer.isSafeLowRiskBatchCandidate(it, low)) }
    }

    @Test
    fun revisionAndBuildUseDistinctPlanRequests() {
        val plan =
            read.copy(detail = FfiToolCallDetail.Generic("ExitPlanMode", """{"plan":"One\nTwo"}"""))
        assertEquals("One\nTwo", PlanApprovalReducer.detectPlanApproval(plan)?.planMarkdown)
        assertNull(PlanApprovalReducer.detectPlanApproval(read))
        val approve =
            PlanApprovalReducer.buildSubmitPayload(
                PlanApprovalAction.APPROVE_AND_BUILD,
                "A\r\nB",
                "A\nB",
                " ",
            )
        assertNull(approve.editedPlanMarkdown)
        assertEquals(SessionMode.Default, approve.targetMode)
        val revise =
            PlanApprovalReducer.buildSubmitPayload(
                PlanApprovalAction.REQUEST_CHANGES,
                "A",
                "B",
                " revise ",
            )
        assertEquals("revise", revise.feedback)
        assertEquals("B", revise.editedPlanMarkdown)
        assertNull(revise.targetMode)

        val dismiss =
            PlanApprovalReducer.buildSubmitPayload(
                PlanApprovalAction.DISMISS,
                "A",
                "B",
                "ignored",
            )
        assertNull(dismiss.editedPlanMarkdown)
        assertNull(dismiss.feedback)
        assertNull(dismiss.targetMode)
    }

    @Test
    fun planJsonUnescaperHandlesEscapedBackslashesAndUnicode() {
        // Double-backslash followed by n must decode to literal "\n", NOT a newline
        val raw = """Literal \\n plus real\nline, quote \"ok\", slash \/, tab\t, cr\r, bs\b, ff\f, uni \u003c\u4e2d\u6587\u003e \uD83D\uDE80"""
        val expected = "Literal \\n plus real\nline, quote \"ok\", slash /, tab\t, cr\r, bs\b, ff\u000C, uni <中文> 🚀"
        assertEquals(expected, PlanApprovalReducer.unescapeJsonString(raw))

        val plan =
            read.copy(
                detail = FfiToolCallDetail.Generic(
                    "ExitPlanMode",
                    """{"plan":"Use \\\\n in regex\nNext: \"escaped\" \u0026 ready"}""",
                )
            )
        assertEquals(
            "Use \\\\n in regex\nNext: \"escaped\" & ready",
            PlanApprovalReducer.detectPlanApproval(plan)?.planMarkdown,
        )
    }

    @Test
    fun benignSpeechErrorsAreClassifiedCorrectly() {
        assertTrue(dev.orbit.android.ui.composer.isBenignSpeechError(android.speech.SpeechRecognizer.ERROR_NO_MATCH))
        assertTrue(dev.orbit.android.ui.composer.isBenignSpeechError(android.speech.SpeechRecognizer.ERROR_SPEECH_TIMEOUT))
        assertTrue(dev.orbit.android.ui.composer.isBenignSpeechError(android.speech.SpeechRecognizer.ERROR_CLIENT))
        assertFalse(dev.orbit.android.ui.composer.isBenignSpeechError(android.speech.SpeechRecognizer.ERROR_AUDIO))
        assertFalse(dev.orbit.android.ui.composer.isBenignSpeechError(android.speech.SpeechRecognizer.ERROR_NETWORK))
    }

    @Test
    fun unchangedAuxKeepsDecisionsOnlyWithinTheSameStage() {
        val insights =
            FfiSessionInsights(emptyList(), emptyList(), emptyList(), listOf("localhost"))
        val prev =
            FfiAgentStage(
                2u,
                "alpha",
                null,
                emptyList(),
                listOf(read),
                emptyList(),
                emptyList(),
                insights,
                5u,
            )
        val next =
            FfiAgentStageDelta(
                3u,
                "alpha",
                null,
                0u,
                emptyList(),
                0u,
                emptyList(),
                emptyList(),
                emptyList(),
                insights.copy(detectedLocalUrls = emptyList()),
                6u,
                false,
                1u,
                true,
                0u,
                true,
                2u,
            )
        val merged = StageDeltaReducer.merge(prev, "alpha", "agent", next)
        assertSame(insights, merged.stage.insights)
        assertEquals(listOf(read), merged.stage.pendingPermissions)
        assertTrue(merged.hasOlderTurns)
        assertEquals(2uL, merged.oldestLoadedSeq)
        assertTrue(
            StageDeltaReducer.merge(prev, "beta", "agent", next.copy(hostId = "beta"))
                .stage
                .pendingPermissions
                .isEmpty()
        )
        assertTrue(
            StageDeltaReducer.merge(prev, "alpha", "agent", next.copy(reset = true))
                .stage
                .pendingPermissions
                .isEmpty()
        )
    }

    @Test
    fun prependedPagesKeepAnchorAndClampMissingAnchor() {
        val keys = listOf("older", "old", "current")
        assertEquals(2, StageDeltaReducer.resolvePrependedAnchorIndex("current", keys, 0))
        assertEquals(2, StageDeltaReducer.resolvePrependedAnchorIndex(null, keys, 10))
        assertNull(StageDeltaReducer.resolvePrependedAnchorIndex("current", emptyList(), 3))
    }
}
