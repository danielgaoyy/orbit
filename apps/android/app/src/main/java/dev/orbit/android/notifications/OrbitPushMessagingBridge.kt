package dev.orbit.android.notifications

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.net.URLDecoder

/**
 * Canonical Orbit push event categories emitted by `orbit-daemon` (`crates/orbit-protocol/src/push.rs`).
 */
enum class OrbitPushEventKind(val slug: String) {
    PERMISSION_REQUIRED("permission_required"),
    TURN_COMPLETED("turn_completed"),
    AGENT_ERROR("agent_error"),
    GOAL_FINISHED("goal_finished"),
    TIMER_FIRED("timer_fired");

    companion object {
        fun fromRaw(raw: String?): OrbitPushEventKind = when (raw?.trim()?.lowercase()) {
            "permission_required", "approval", "permission", "question", "plan_approval" -> PERMISSION_REQUIRED
            "agent_error", "error", "failed", "conflict" -> AGENT_ERROR
            "goal_finished", "goal_completed", "goal" -> GOAL_FINISHED
            "timer_fired", "timer", "schedule_fired" -> TIMER_FIRED
            else -> TURN_COMPLETED
        }
    }
}

enum class OrbitPushChannelRoute {
    APPROVALS,
    COMPLETIONS,
    ERRORS,
}

data class OrbitPushRoute(
    val eventKind: OrbitPushEventKind,
    val channel: OrbitPushChannelRoute,
    val hostId: String,
    val agentId: String?,
    val title: String,
    val body: String,
    val deepLink: String,
)

sealed interface OrbitDeepLinkAction {
    data class PairHost(val uri: String) : OrbitDeepLinkAction
    data class ImportDeviceLink(val uri: String) : OrbitDeepLinkAction
    data class OpenThread(
        val hostId: String,
        val agentId: String,
        val eventKind: OrbitPushEventKind? = null,
    ) : OrbitDeepLinkAction
    data class OpenHost(
        val hostId: String,
        val eventKind: OrbitPushEventKind? = null,
    ) : OrbitDeepLinkAction
}

/**
 * Pure, JVM-testable router for canonical FCM push payloads (`event_kind`, `host_id`, `agent_id`,
 * `deep_link`) and `orbit://` deep-link URIs.
 */
object OrbitPushRouter {
    private val SAFE_ID_REGEX = Regex("^[A-Za-z0-9_.:-]{1,128}$")

    fun isSafeIdentifier(raw: String): Boolean = SAFE_ID_REGEX.matches(raw)

    fun channelFor(kind: OrbitPushEventKind): OrbitPushChannelRoute = when (kind) {
        OrbitPushEventKind.PERMISSION_REQUIRED -> OrbitPushChannelRoute.APPROVALS
        OrbitPushEventKind.AGENT_ERROR -> OrbitPushChannelRoute.ERRORS
        OrbitPushEventKind.TURN_COMPLETED,
        OrbitPushEventKind.GOAL_FINISHED,
        OrbitPushEventKind.TIMER_FIRED -> OrbitPushChannelRoute.COMPLETIONS
    }

    fun defaultTitleFor(kind: OrbitPushEventKind): String = when (kind) {
        OrbitPushEventKind.PERMISSION_REQUIRED -> "Approval required"
        OrbitPushEventKind.TURN_COMPLETED -> "Turn completed"
        OrbitPushEventKind.AGENT_ERROR -> "Agent error"
        OrbitPushEventKind.GOAL_FINISHED -> "Goal finished"
        OrbitPushEventKind.TIMER_FIRED -> "Timer fired"
    }

    fun defaultBodyFor(kind: OrbitPushEventKind): String = when (kind) {
        OrbitPushEventKind.PERMISSION_REQUIRED -> "An agent is waiting for your decision."
        OrbitPushEventKind.TURN_COMPLETED -> "An agent finished its background turn."
        OrbitPushEventKind.AGENT_ERROR -> "An agent stopped with an error."
        OrbitPushEventKind.GOAL_FINISHED -> "An autonomous goal reached a terminal state."
        OrbitPushEventKind.TIMER_FIRED -> "A scheduled timer triggered."
    }

    /**
     * Parses canonical FCM `RemoteMessage.data` map (`event_kind`, `host_id`, `agent_id`,
     * `deep_link`, `title`, `body`) with backward-compatible fallback to legacy `kind` key
     * and query parameter extraction from `deep_link`.
     */
    fun parsePayload(data: Map<String, String>): OrbitPushRoute? {
        val rawDeepLink = data["deep_link"]?.trim().orEmpty()
        val parsedLink = rawDeepLink.takeIf { it.isNotEmpty() }?.let { parseDeepLink(it) }

        val hostFromData = data["host_id"]?.trim().orEmpty()
        val agentFromData = data["agent_id"]?.trim().orEmpty()

        val resolvedHost = hostFromData.ifEmpty {
            when (parsedLink) {
                is OrbitDeepLinkAction.OpenThread -> parsedLink.hostId
                is OrbitDeepLinkAction.OpenHost -> parsedLink.hostId
                else -> ""
            }
        }
        if (resolvedHost.isEmpty() || !isSafeIdentifier(resolvedHost)) return null

        val resolvedAgent = agentFromData.ifEmpty {
            (parsedLink as? OrbitDeepLinkAction.OpenThread)?.agentId.orEmpty()
        }.takeIf { it.isNotEmpty() && isSafeIdentifier(it) }

        val rawKind = data["event_kind"]?.takeIf { it.isNotBlank() }
            ?: data["kind"]?.takeIf { it.isNotBlank() }
        val eventKind = rawKind?.let(OrbitPushEventKind::fromRaw) ?: when (parsedLink) {
            is OrbitDeepLinkAction.OpenThread -> parsedLink.eventKind
            is OrbitDeepLinkAction.OpenHost -> parsedLink.eventKind
            else -> null
        } ?: OrbitPushEventKind.TURN_COMPLETED

        val canonicalLink = buildCanonicalOpenUri(resolvedHost, resolvedAgent, eventKind)
        val title = data["title"]?.trim().orEmpty().ifBlank { defaultTitleFor(eventKind) }
        val body = data["body"]?.trim().orEmpty().ifBlank { defaultBodyFor(eventKind) }

        return OrbitPushRoute(
            eventKind = eventKind,
            channel = channelFor(eventKind),
            hostId = resolvedHost,
            agentId = resolvedAgent,
            title = title,
            body = body,
            deepLink = canonicalLink,
        )
    }

    fun buildCanonicalOpenUri(
        hostId: String,
        agentId: String?,
        eventKind: OrbitPushEventKind? = null,
    ): String = buildString {
        append("orbit://open?host=")
        append(encodeQueryValue(hostId.trim()))
        agentId?.trim()?.takeIf { it.isNotEmpty() }?.let { aid ->
            append("&agent=")
            append(encodeQueryValue(aid))
        }
        eventKind?.let { kind ->
            append("&event_kind=")
            append(kind.slug)
        }
    }

    /**
     * Pure JVM URI parser (independent of `android.net.Uri`) for `orbit://pair`,
     * `orbit://sync-login`, and `orbit://open`.
     */
    fun parseDeepLink(rawUri: String?): OrbitDeepLinkAction? {
        val trimmed = rawUri?.trim().orEmpty()
        if (trimmed.isEmpty()) return null
        val uri = runCatching { java.net.URI(trimmed) }.getOrNull() ?: return null
        if (!"orbit".equals(uri.scheme, ignoreCase = true) || uri.userInfo != null || uri.port != -1) return null
        when (uri.host?.lowercase()) {
            "pair" -> return OrbitDeepLinkAction.PairHost(trimmed)
            "sync-login" -> return OrbitDeepLinkAction.ImportDeviceLink(trimmed)
            "open" -> if (!uri.path.isNullOrEmpty() && uri.path != "/") return null
            else -> return null
        }
        val queryIdx = trimmed.indexOf('?')
        if (queryIdx < 0 || queryIdx >= trimmed.lastIndex) return null
        val query = trimmed.substring(queryIdx + 1).substringBefore('#')
        val params = parseQueryParams(query)

        val hostId = params["host"] ?: params["host_id"] ?: return null
        if (!isSafeIdentifier(hostId)) return null

        val agentId = (params["agent"] ?: params["agent_id"])?.takeIf { it.isNotEmpty() }
        if (agentId != null && !isSafeIdentifier(agentId)) return null

        val eventKind = (params["event_kind"] ?: params["kind"])
            ?.takeIf { it.isNotBlank() }
            ?.let { OrbitPushEventKind.fromRaw(it) }

        return if (agentId != null) {
            OrbitDeepLinkAction.OpenThread(hostId = hostId, agentId = agentId, eventKind = eventKind)
        } else {
            OrbitDeepLinkAction.OpenHost(hostId = hostId, eventKind = eventKind)
        }
    }

    private fun parseQueryParams(query: String): Map<String, String> {
        val result = LinkedHashMap<String, String>()
        query.split('&').forEach { pair ->
            if (pair.isEmpty()) return@forEach
            val eqIdx = pair.indexOf('=')
            val rawKey = if (eqIdx >= 0) pair.substring(0, eqIdx) else pair
            val rawVal = if (eqIdx >= 0) pair.substring(eqIdx + 1) else ""
            val key = runCatching { URLDecoder.decode(rawKey, "UTF-8") }.getOrDefault(rawKey).trim()
            val value = runCatching { URLDecoder.decode(rawVal, "UTF-8") }.getOrDefault(rawVal).trim()
            if (key.isNotEmpty()) {
                result[key] = value
            }
        }
        return result
    }

    private fun encodeQueryValue(raw: String): String {
        val sb = StringBuilder(raw.length)
        for (b in raw.toByteArray(Charsets.UTF_8)) {
            val c = b.toInt() and 0xFF
            val isUnreserved = (c in 'A'.code..'Z'.code) ||
                (c in 'a'.code..'z'.code) ||
                (c in '0'.code..'9'.code) ||
                c == '-'.code || c == '_'.code || c == '.'.code || c == '~'.code
            if (isUnreserved) {
                sb.append(c.toChar())
            } else {
                sb.append(String.format("%%%02X", c))
            }
        }
        return sb.toString()
    }
}

/**
 * Push notification token & remote payload bridge for Firebase Cloud Messaging (FCM).
 *
 * Distribution builds with `google-services.json` delegate from `OrbitFirebaseMessagingService`:
 * - `onNewToken(token)` -> [OrbitFirebaseMessagingBridge.onNewFcmToken]
 * - `onMessageReceived(remoteMessage)` -> [OrbitFirebaseMessagingBridge.onRemotePushPayload]
 *
 * Keeping the bridge decoupled allows debug/FOSS builds to compile cleanly with zero Play Services
 * dependency while still executing the real `registerPushDevice` UniFFI RPC on every connected host.
 */
object OrbitFirebaseMessagingBridge {
    const val PLATFORM_ANDROID_FCM = "fcm"

    private const val PREFS_NAME = "orbit_push_bridge"
    private const val KEY_FCM_TOKEN = "fcm_token"

    private val _cachedToken = MutableStateFlow<String?>(null)
    val cachedToken: StateFlow<String?> = _cachedToken.asStateFlow()

    @Volatile
    private var tokenListener: ((String) -> Unit)? = null

    fun attachTokenListener(context: Context, listener: (String) -> Unit) {
        tokenListener = listener
        val persisted = loadPersistedToken(context)
        if (!persisted.isNullOrBlank()) {
            _cachedToken.value = persisted
            listener(persisted)
        }
    }

    fun detachTokenListener() {
        tokenListener = null
    }

    fun loadPersistedToken(context: Context): String? {
        val prefs = context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        return prefs.getString(KEY_FCM_TOKEN, null)?.trim()?.takeIf { it.isNotEmpty() }
    }

    fun onNewFcmToken(context: Context, token: String) {
        val clean = token.trim()
        if (clean.isEmpty()) return
        val prefs = context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        prefs.edit().putString(KEY_FCM_TOKEN, clean).apply()
        _cachedToken.value = clean
        tokenListener?.invoke(clean)
    }

    fun clearToken(context: Context) {
        val prefs = context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        prefs.edit().remove(KEY_FCM_TOKEN).apply()
        _cachedToken.value = null
    }

    /**
     * Decode canonical FCM payload (`event_kind`, `host_id`, `agent_id`, `deep_link`, `title`, `body`)
     * via [OrbitPushRouter] and post to the matching Android notification channel.
     */
    fun onRemotePushPayload(context: Context, data: Map<String, String>) {
        val route = OrbitPushRouter.parsePayload(data) ?: return
        OrbitNotificationHelper.dispatchPushRoute(context, route)
    }
}
