package dev.orbit.android.store

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Observes Android default network switches (Wi-Fi <-> Cellular <-> VPN) via [ConnectivityManager]
 * and notifies [OrbitMobileCore.onNetworkChanged] so active Noise E2EE sockets reconnect immediately.
 */
class NetworkConnectivityObserver(
    context: Context,
    private val onNetworkSwitch: () -> Unit,
) {
    private val connectivityManager =
        context.applicationContext.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
    private val registered = AtomicBoolean(false)
    private val hasSeenInitialNetwork = AtomicBoolean(false)
    private val lastNetworkHandle = AtomicLong(0L)
    private val lastTransportsMask = AtomicLong(0L)

    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            val prev = lastNetworkHandle.getAndSet(network.networkHandle)
            val alreadyInitialized = !hasSeenInitialNetwork.compareAndSet(false, true)
            if (alreadyInitialized && prev != network.networkHandle) {
                onNetworkSwitch()
            }
        }

        override fun onLost(network: Network) {
            if (lastNetworkHandle.compareAndSet(network.networkHandle, 0L)) {
                onNetworkSwitch()
            }
        }

        override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) {
            var mask = 0L
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) mask = mask or 1L
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)) mask = mask or 2L
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) mask = mask or 4L
            if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) mask = mask or 8L
            val prevMask = lastTransportsMask.getAndSet(mask)
            if (prevMask != 0L && prevMask != mask) {
                onNetworkSwitch()
            }
        }
    }

    fun start() {
        val cm = connectivityManager ?: return
        if (registered.compareAndSet(false, true)) {
            runCatching {
                cm.registerDefaultNetworkCallback(callback)
            }.onFailure {
                registered.set(false)
            }
        }
    }

    fun stop() {
        val cm = connectivityManager ?: return
        if (registered.compareAndSet(true, false)) {
            runCatching {
                cm.unregisterNetworkCallback(callback)
            }
        }
    }
}
