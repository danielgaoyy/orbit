package dev.orbit.android

import android.Manifest
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.orbit.android.notifications.OrbitNotificationHelper
import dev.orbit.android.store.OrbitAndroidStore
import dev.orbit.android.ui.shell.OrbitAppShell
import dev.orbit.android.ui.theme.OrbitTheme

class MainActivity : ComponentActivity() {
    private val store: OrbitAndroidStore by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        OrbitNotificationHelper.ensureChannels(this)

        if (savedInstanceState == null) {
            store.applyLaunchIntent(intent)
        }

        setContent {
            val ui by store.state.collectAsStateWithLifecycle()
            val lifecycleOwner = LocalLifecycleOwner.current
            val notifPermissionLauncher = rememberLauncherForActivityResult(
                contract = ActivityResultContracts.RequestPermission(),
            ) { _ -> Unit }

            val onlineHosts = ui.hosts.filter { it.connectionState == uniffi.orbit_ffi_uniffi.FfiConnectionState.ONLINE_DIRECT ||
                it.connectionState == uniffi.orbit_ffi_uniffi.FfiConnectionState.ONLINE_RELAY }.map { it.hostId }.toSet()
            LaunchedEffect(onlineHosts) {
                if (onlineHosts.isNotEmpty()) dev.orbit.android.notifications.OrbitFirebaseMessagingBridge
                    .loadPersistedToken(this@MainActivity)?.let { store.registerPushToken("fcm", it) }
            }

            LaunchedEffect(ui.hosts.isNotEmpty()) {
                if (ui.hosts.isNotEmpty() &&
                    Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                    !OrbitNotificationHelper.hasNotificationPermission(this@MainActivity)
                ) {
                    val prefs = getSharedPreferences("orbit_android_prefs", Context.MODE_PRIVATE)
                    if (!prefs.getBoolean("requested_post_notifications_v1", false)) {
                        prefs.edit().putBoolean("requested_post_notifications_v1", true).apply()
                        notifPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                    }
                }
            }

            DisposableEffect(lifecycleOwner) {
                val observer = LifecycleEventObserver { _, event ->
                    when (event) {
                        Lifecycle.Event.ON_START -> store.onAppForeground()
                        Lifecycle.Event.ON_STOP -> store.onAppBackground()
                        else -> Unit
                    }
                }
                lifecycleOwner.lifecycle.addObserver(observer)

                onDispose {
                    lifecycleOwner.lifecycle.removeObserver(observer)
                }
            }

            OrbitTheme(appearanceMode = ui.appearanceMode) {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = OrbitTheme.colors.canvas,
                ) {
                    OrbitAppShell(
                        ui = ui,
                        store = store,
                    )
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        store.applyLaunchIntent(intent)
    }
}
