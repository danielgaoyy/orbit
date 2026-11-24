package dev.orbit.android.notifications

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import dev.orbit.android.MainActivity
import dev.orbit.android.R

/**
 * Android notification channel provisioning and canonical deep-link notification dispatcher.
 *
 * Channels:
 * - [CHANNEL_APPROVALS]: High-priority tool approvals, plan approvals, and agent questions.
 * - [CHANNEL_COMPLETIONS]: Default-priority turn completions, goals, and fired timers.
 * - [CHANNEL_ERRORS]: High-priority agent / connection error alerts.
 */
object OrbitNotificationHelper {
    const val CHANNEL_APPROVALS = "orbit_approvals"
    const val CHANNEL_COMPLETIONS = "orbit_completions"
    const val CHANNEL_ERRORS = "orbit_errors"

    fun ensureChannels(context: Context) {
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
            ?: return

        val approvals = NotificationChannel(
            CHANNEL_APPROVALS,
            "Approvals & Decisions",
            NotificationManager.IMPORTANCE_HIGH,
        ).apply {
            description = "Tool permission requests, plan approvals, and agent questions requiring your input."
            enableVibration(true)
        }

        val completions = NotificationChannel(
            CHANNEL_COMPLETIONS,
            "Completed Runs",
            NotificationManager.IMPORTANCE_DEFAULT,
        ).apply {
            description = "Notifications when an autonomous agent, goal, or scheduled timer finishes."
        }

        val errors = NotificationChannel(
            CHANNEL_ERRORS,
            "Errors & Alerts",
            NotificationManager.IMPORTANCE_HIGH,
        ).apply {
            description = "Agent runtime failures, verify failures, and conflict alerts."
            enableVibration(true)
        }

        manager.createNotificationChannels(listOf(approvals, completions, errors))
    }

    fun hasNotificationPermission(context: Context): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val granted = ContextCompat.checkSelfPermission(
                context,
                Manifest.permission.POST_NOTIFICATIONS,
            ) == PackageManager.PERMISSION_GRANTED
            if (!granted) return false
        }
        return NotificationManagerCompat.from(context).areNotificationsEnabled()
    }

    fun buildOpenThreadUri(
        hostId: String,
        agentId: String?,
        eventKind: OrbitPushEventKind? = null,
    ): Uri = Uri.parse(OrbitPushRouter.buildCanonicalOpenUri(hostId, agentId, eventKind))

    fun buildOpenThreadIntent(
        context: Context,
        hostId: String,
        agentId: String?,
        eventKind: OrbitPushEventKind? = null,
    ): Intent =
        Intent(
            Intent.ACTION_VIEW,
            buildOpenThreadUri(hostId, agentId, eventKind),
            context,
            MainActivity::class.java,
        ).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }

    internal fun channelIdFor(channel: OrbitPushChannelRoute): String = when (channel) {
            OrbitPushChannelRoute.APPROVALS -> CHANNEL_APPROVALS
            OrbitPushChannelRoute.COMPLETIONS -> CHANNEL_COMPLETIONS
            OrbitPushChannelRoute.ERRORS -> CHANNEL_ERRORS
        }

    fun dispatchPushRoute(context: Context, route: OrbitPushRoute) {
        val channelId = channelIdFor(route.channel)
        val priority = when (route.channel) {
            OrbitPushChannelRoute.APPROVALS, OrbitPushChannelRoute.ERRORS -> NotificationCompat.PRIORITY_HIGH
            OrbitPushChannelRoute.COMPLETIONS -> NotificationCompat.PRIORITY_DEFAULT
        }
        val key = "${route.eventKind.slug}:${route.hostId}:${route.agentId.orEmpty()}"
        postNotification(
            context = context,
            channelId = channelId,
            notificationId = key.hashCode(),
            hostId = route.hostId,
            agentId = route.agentId,
            eventKind = route.eventKind,
            title = route.title,
            body = route.body,
            priority = priority,
        )
    }

    fun notifyApprovalRequired(
        context: Context,
        hostId: String,
        agentId: String,
        title: String,
        body: String,
    ) {
        postNotification(
            context = context,
            channelId = CHANNEL_APPROVALS,
            notificationId = ("approval:$hostId:$agentId").hashCode(),
            hostId = hostId,
            agentId = agentId,
            eventKind = OrbitPushEventKind.PERMISSION_REQUIRED,
            title = title.ifBlank { "Approval required" },
            body = body.ifBlank { "An agent is waiting for your decision." },
            priority = NotificationCompat.PRIORITY_HIGH,
        )
    }

    fun notifyAgentCompleted(
        context: Context,
        hostId: String,
        agentId: String,
        title: String,
        body: String,
    ) {
        postNotification(
            context = context,
            channelId = CHANNEL_COMPLETIONS,
            notificationId = ("completed:$hostId:$agentId").hashCode(),
            hostId = hostId,
            agentId = agentId,
            eventKind = OrbitPushEventKind.TURN_COMPLETED,
            title = title.ifBlank { "Turn completed" },
            body = body.ifBlank { "Agent finished its task." },
            priority = NotificationCompat.PRIORITY_DEFAULT,
        )
    }

    fun notifyAgentError(
        context: Context,
        hostId: String,
        agentId: String,
        title: String,
        body: String,
    ) {
        postNotification(
            context = context,
            channelId = CHANNEL_ERRORS,
            notificationId = ("error:$hostId:$agentId").hashCode(),
            hostId = hostId,
            agentId = agentId,
            eventKind = OrbitPushEventKind.AGENT_ERROR,
            title = title.ifBlank { "Agent error" },
            body = body.ifBlank { "An agent encountered an error." },
            priority = NotificationCompat.PRIORITY_HIGH,
        )
    }

    private fun postNotification(
        context: Context,
        channelId: String,
        notificationId: Int,
        hostId: String,
        agentId: String?,
        eventKind: OrbitPushEventKind,
        title: String,
        body: String,
        priority: Int,
    ) {
        ensureChannels(context)
        if (!hasNotificationPermission(context)) return

        val pendingIntent = PendingIntent.getActivity(
            context,
            notificationId,
            buildOpenThreadIntent(context, hostId, agentId, eventKind),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        val notification = NotificationCompat.Builder(context, channelId)
            .setSmallIcon(R.drawable.ic_orbit_notification)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setPriority(priority)
            .setAutoCancel(true)
            .setContentIntent(pendingIntent)
            .build()

        try {
            NotificationManagerCompat.from(context).notify(notificationId, notification)
        } catch (_: SecurityException) {
            // Notification permission can be revoked between the check and delivery.
        }
    }
}
