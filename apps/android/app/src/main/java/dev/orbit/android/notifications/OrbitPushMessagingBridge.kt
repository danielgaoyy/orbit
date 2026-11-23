package dev.orbit.android.notifications

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Push notification token & remote payload bridge ready for Firebase Cloud Messaging (FCM).
 *
 * When `google-services.json` and `firebase-messaging` are enabled in a distribution build,
 * an `OrbitFirebaseMessagingService : FirebaseMessagingService()` subclass simply delegates:
 * - `onNewToken(token)` -> [OrbitFirebaseMessagingBridge.onNewFcmToken]
 * - `onMessageReceived(remoteMessage)` -> [OrbitFirebaseMessagingBridge.onRemotePushPayload]
 *
 * Keeping the bridge decoupled allows debug/FOSS builds to compile with zero Play Services dependency.
 */
object OrbitFirebaseMessagingBridge {
    const val PLATFORM_ANDROID_FCM = "android_fcm"

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
     * Dispatch a decoded push payload (e.g. from FCM `RemoteMessage.data`) to the matching
     * Android notification channel with an `orbit://open?host=...&agent=...` deep link.
     */
    fun onRemotePushPayload(context: Context, data: Map<String, String>) {
        val hostId = data["host_id"]?.trim().orEmpty()
        val agentId = data["agent_id"]?.trim().orEmpty()
        if (hostId.isEmpty() || agentId.isEmpty()) return

        val kind = data["kind"]?.trim()?.lowercase().orEmpty()
        val title = data["title"]?.trim().orEmpty()
        val body = data["body"]?.trim().orEmpty()

        when (kind) {
            "approval", "permission", "question" -> {
                OrbitNotificationHelper.notifyApprovalRequired(
                    context = context,
                    hostId = hostId,
                    agentId = agentId,
                    title = title,
                    body = body,
                )
            }
            "error", "failed", "conflict" -> {
                OrbitNotificationHelper.notifyAgentError(
                    context = context,
                    hostId = hostId,
                    agentId = agentId,
                    title = title,
                    body = body,
                )
            }
            else -> {
                OrbitNotificationHelper.notifyAgentCompleted(
                    context = context,
                    hostId = hostId,
                    agentId = agentId,
                    title = title,
                    body = body,
                )
            }
        }
    }
}
