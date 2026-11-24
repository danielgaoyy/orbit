package dev.orbit.android.notifications

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

class OrbitFirebaseMessagingService : FirebaseMessagingService() {
    override fun onNewToken(token: String) {
        OrbitFirebaseMessagingBridge.onNewFcmToken(this, token)
    }

    override fun onMessageReceived(message: RemoteMessage) {
        val data = message.data.toMutableMap()
        message.notification?.title?.let { data.putIfAbsent("title", it) }
        message.notification?.body?.let { data.putIfAbsent("body", it) }
        OrbitFirebaseMessagingBridge.onRemotePushPayload(this, data)
    }
}
