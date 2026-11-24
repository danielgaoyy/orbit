package dev.orbit.android

import dev.orbit.android.notifications.*
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
}
