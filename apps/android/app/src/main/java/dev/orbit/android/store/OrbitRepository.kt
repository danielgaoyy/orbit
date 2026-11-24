package dev.orbit.android.store

import android.content.Context
import android.os.Build
import uniffi.orbit_ffi_uniffi.OrbitMobileCore
import java.io.File
import java.util.UUID

/**
 * Thin Android repository boundary responsible for hardware-keyed [OrbitMobileCore] creation
 * and stable device identity persistence. All domain projections, SQLite persistence, and E2EE
 * transport state remain strictly inside Rust (`orbit-client-core`).
 */
object OrbitRepository {
    private const val DB_FILENAME = "orbit_android_v3.sqlite"
    private const val IDENTITY_PREFS = "orbit_device_identity"
    private const val KEY_DEVICE_ID = "stable_push_device_id_v1"

    /**
     * Opens the durable encrypted [OrbitMobileCore] database using a hardware-Keystore-protected
     * 32-byte master key (`OrbitMobileCore.newWithMasterKey`). Zeroes the temporary key array
     * immediately after handing it across UniFFI.
     */
    fun openMobileCore(context: Context): OrbitMobileCore {
        val appContext = context.applicationContext
        val dbFile = File(appContext.filesDir, DB_FILENAME)
        val masterKeyBytes = OrbitMasterKeyStore.getOrCreateMasterKey(appContext)
        return try {
            OrbitMobileCore.newWithMasterKey(dbFile.absolutePath, masterKeyBytes)
        } finally {
            masterKeyBytes.fill(0)
        }
    }

    /**
     * Stable app-scoped device ID used when registering Android FCM push tokens on paired daemons.
     */
    fun getOrCreateDeviceId(context: Context): String {
        val prefs = context.applicationContext.getSharedPreferences(IDENTITY_PREFS, Context.MODE_PRIVATE)
        val existing = prefs.getString(KEY_DEVICE_ID, null)?.trim()?.takeIf { it.isNotEmpty() }
        if (existing != null) return existing
        val created = "android-${UUID.randomUUID()}"
        prefs.edit().putString(KEY_DEVICE_ID, created).apply()
        return created
    }

    fun deviceDisplayName(): String {
        val manufacturer = Build.MANUFACTURER?.trim().orEmpty()
        val model = Build.MODEL?.trim().orEmpty()
        return when {
            model.isEmpty() -> "Android Device"
            manufacturer.isEmpty() || model.startsWith(manufacturer, ignoreCase = true) -> model
            else -> "${manufacturer.replaceFirstChar { it.uppercase() }} $model"
        }
    }
}
