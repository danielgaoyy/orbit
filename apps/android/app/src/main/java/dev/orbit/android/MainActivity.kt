package dev.orbit.android

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.orbit.android.store.OrbitAndroidStore
import dev.orbit.android.ui.shell.AdaptiveOrbitShell
import dev.orbit.android.ui.theme.OrbitTheme

class MainActivity : ComponentActivity() {
    private val store: OrbitAndroidStore by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        if (savedInstanceState == null) {
            store.applyLaunchIntent(intent)
        }

        setContent {
            val ui by store.state.collectAsStateWithLifecycle()
            val lifecycleOwner = LocalLifecycleOwner.current

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
                    AdaptiveOrbitShell(
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
