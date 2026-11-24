package dev.orbit.android

import dev.orbit.android.notifications.*
import java.util.Base64
import org.junit.Assert.*
import org.junit.Test

class PushRoutingTest {
    @Test
    fun everyEventReachesItsNotificationChannelAndThread() {
        val channels =
            mapOf(
                OrbitPushEventKind.PERMISSION_REQUIRED to OrbitNotificationHelper.CHANNEL_APPROVALS,
                OrbitPushEventKind.AGENT_ERROR to OrbitNotificationHelper.CHANNEL_ERRORS,
                OrbitPushEventKind.TURN_COMPLETED to OrbitNotificationHelper.CHANNEL_COMPLETIONS,
                OrbitPushEventKind.GOAL_FINISHED to OrbitNotificationHelper.CHANNEL_COMPLETIONS,
                OrbitPushEventKind.TIMER_FIRED to OrbitNotificationHelper.CHANNEL_COMPLETIONS,
            )
        assertEquals(OrbitPushEventKind.entries.toSet(), channels.keys)
        channels.forEach { (kind, channel) ->
            val route =
                requireNotNull(
                    OrbitPushRouter.parsePayload(
                        mapOf(
                            "event_kind" to kind.slug,
                            "host_id" to "host:1",
                            "agent_id" to "session-2",
                            "kind" to "unknown",
                        )
                    )
                )
            assertEquals(channel, OrbitNotificationHelper.channelIdFor(route.channel))
            assertEquals(
                OrbitDeepLinkAction.OpenThread("host:1", "session-2", kind),
                OrbitPushRouter.parseDeepLink(route.deepLink),
            )
        }
    }

    @Test
    fun hostOnlyLinksPreserveKindEvenWithoutSeparateFcmFields() {
        val route =
            requireNotNull(
                OrbitPushRouter.parsePayload(
                    mapOf("deep_link" to "orbit://open?host=other&event_kind=timer_fired")
                )
            )
        assertEquals(
            OrbitDeepLinkAction.OpenHost("other", OrbitPushEventKind.TIMER_FIRED),
            OrbitPushRouter.parseDeepLink(route.deepLink),
        )
    }

    @Test
    fun acceptsOnlyKnownSchemeAuthoritiesAndValidIdentifiers() {
        listOf(
                "https://open?host=h",
                "orbit://open.invalid?host=h",
                "orbit://sync-login.invalid",
                "orbit://pair@evil/path",
                "orbit://open?host=h&agent=%2Fa",
                "orbit://open?host=",
            )
            .forEach { assertNull(it, OrbitPushRouter.parseDeepLink(it)) }
        assertNull(OrbitPushRouter.parsePayload(emptyMap()))
        assertTrue(
            OrbitPushRouter.parseDeepLink("orbit://pair/abc") is OrbitDeepLinkAction.PairHost
        )
        assertTrue(
            OrbitPushRouter.parseDeepLink("orbit://sync-login?v=1")
                is OrbitDeepLinkAction.ImportDeviceLink
        )
    }

    @Test
    fun externalPairAndSyncLoginIntentsRequireExplicitConfirmation() {
        val pairJson =
            """{"host_id":"host-dev-1","hostname":"mac-studio.local","direct_endpoints":["ws://10.0.0.8:6868"],"relay_url":"wss://relay.orbit.dev","pairing_secret":"SUPER_SECRET_TOKEN"}"""
        val pairUri =
            "orbit://pair/" + Base64.getUrlEncoder().withoutPadding().encodeToString(pairJson.toByteArray())
        val pairDisp = OrbitDeepLinkSecurityPolicy.resolveExternalAction(pairUri)
        assertTrue(pairDisp is ExternalDeepLinkDisposition.RequireConfirmation)
        val pairConfirm = (pairDisp as ExternalDeepLinkDisposition.RequireConfirmation).confirmation
        assertEquals(PendingDeepLinkKind.PAIR_HOST, pairConfirm.kind)
        assertEquals(pairUri, pairConfirm.uri)
        assertTrue(pairConfirm.endpointSummary.contains("mac-studio.local"))
        assertTrue(pairConfirm.endpointSummary.contains("ws://10.0.0.8:6868"))
        assertFalse(pairConfirm.endpointSummary.contains("SUPER_SECRET_TOKEN"))

        val syncJson =
            """{"v":1,"github_access_token":"gho_SECRET_ATTACKER_TOKEN","github_login":"attacker","relay_url":"wss://evil.example"}"""
        val syncUri =
            "orbit://sync-login?v=1&p=" +
                Base64.getUrlEncoder().withoutPadding().encodeToString(syncJson.toByteArray())
        val syncDisp = OrbitDeepLinkSecurityPolicy.resolveExternalAction(syncUri)
        assertTrue(syncDisp is ExternalDeepLinkDisposition.RequireConfirmation)
        val syncConfirm = (syncDisp as ExternalDeepLinkDisposition.RequireConfirmation).confirmation
        assertEquals(PendingDeepLinkKind.IMPORT_CLOUD_SYNC_DEVICE_LINK, syncConfirm.kind)
        assertEquals(syncUri, syncConfirm.uri)
        assertTrue(syncConfirm.endpointSummary.contains("@attacker"))
        assertFalse(syncConfirm.endpointSummary.contains("gho_SECRET_ATTACKER_TOKEN"))

        val openDisp =
            OrbitDeepLinkSecurityPolicy.resolveExternalAction("orbit://open?host=host-1&agent=agent-9")
        assertEquals(
            ExternalDeepLinkDisposition.ImmediateNavigate(
                OrbitDeepLinkAction.OpenThread("host-1", "agent-9", null)
            ),
            openDisp,
        )
        assertNull(OrbitDeepLinkSecurityPolicy.resolveExternalAction("https://evil.example/pair"))
    }
}
