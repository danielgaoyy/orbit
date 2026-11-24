package dev.orbit.android.store

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import java.security.SecureRandom
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Pure binary envelope codec for a hardware-Keystore-wrapped 32-byte master key.
 *
 * Envelope format (Base64-encoded for SharedPreferences storage):
 *   byte[0]       = FORMAT_VERSION (0x01)
 *   byte[1..12]   = 12-byte random AES-GCM IV (96 bits)
 *   byte[13..end] = AES-256-GCM ciphertext + 128-bit authentication tag
 */
object MasterKeyEnvelopeCodec {
    const val FORMAT_VERSION: Byte = 0x01
    const val MASTER_KEY_BYTES = 32
    const val GCM_IV_BYTES = 12
    const val GCM_TAG_BITS = 128
    private const val GCM_TAG_BYTES = GCM_TAG_BITS / 8
    private const val TRANSFORMATION = "AES/GCM/NoPadding"

    data class SealedEnvelope(
        val version: Byte,
        val iv: ByteArray,
        val ciphertext: ByteArray,
    ) {
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is SealedEnvelope) return false
            return version == other.version &&
                iv.contentEquals(other.iv) &&
                ciphertext.contentEquals(other.ciphertext)
        }

        override fun hashCode(): Int {
            var result = version.toInt()
            result = 31 * result + iv.contentHashCode()
            result = 31 * result + ciphertext.contentHashCode()
            return result
        }
    }

    fun generateRandomMasterKey(rng: SecureRandom = SecureRandom()): ByteArray =
        ByteArray(MASTER_KEY_BYTES).also { rng.nextBytes(it) }

    fun encodeEnvelope(iv: ByteArray, ciphertext: ByteArray): String {
        require(iv.size == GCM_IV_BYTES) {
            "Expected $GCM_IV_BYTES-byte GCM IV, got ${iv.size}"
        }
        require(ciphertext.size == MASTER_KEY_BYTES + GCM_TAG_BYTES) {
            "Invalid ciphertext length (${ciphertext.size} bytes)"
        }
        val raw = ByteArray(1 + iv.size + ciphertext.size)
        return try {
            raw[0] = FORMAT_VERSION
            System.arraycopy(iv, 0, raw, 1, iv.size)
            System.arraycopy(ciphertext, 0, raw, 1 + iv.size, ciphertext.size)
            Base64.getEncoder().encodeToString(raw)
        } finally {
            raw.fill(0)
        }
    }

    fun decodeEnvelope(encoded: String): SealedEnvelope {
        val raw = Base64.getDecoder().decode(encoded.trim())
        return try {
            val minLen = 1 + GCM_IV_BYTES + MASTER_KEY_BYTES + GCM_TAG_BYTES
            require(raw.size == minLen) {
                "Corrupt master key envelope (${raw.size} bytes)"
            }
            val version = raw[0]
            require(version == FORMAT_VERSION) {
                "Unsupported master key envelope version: $version"
            }
            val iv = raw.copyOfRange(1, 1 + GCM_IV_BYTES)
            val ciphertext = raw.copyOfRange(1 + GCM_IV_BYTES, raw.size)
            SealedEnvelope(version = version, iv = iv, ciphertext = ciphertext)
        } finally {
            raw.fill(0)
        }
    }

    fun wrapWithKey(plaintextKey: ByteArray, wrappingKey: SecretKey): String {
        require(plaintextKey.size == MASTER_KEY_BYTES) {
            "Master key must be exactly $MASTER_KEY_BYTES bytes"
        }
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, wrappingKey)
        cipher.updateAAD(byteArrayOf(FORMAT_VERSION))
        val iv = cipher.iv
        require(iv != null && iv.size == GCM_IV_BYTES) {
            "AES-GCM provider returned invalid IV"
        }
        val ciphertext = cipher.doFinal(plaintextKey)
        return encodeEnvelope(iv, ciphertext)
    }

    fun unwrapWithKey(encoded: String, wrappingKey: SecretKey): ByteArray {
        val env = decodeEnvelope(encoded)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, wrappingKey, GCMParameterSpec(GCM_TAG_BITS, env.iv))
        cipher.updateAAD(byteArrayOf(env.version))
        val plaintext = cipher.doFinal(env.ciphertext)
        require(plaintext.size == MASTER_KEY_BYTES) {
            "Unwrapped master key length mismatch (${plaintext.size} bytes)"
        }
        return plaintext
    }
}

/**
 * Hardware-backed Android Keystore protector for the 32-byte `OrbitMobileCore` SQLite master key.
 *
 * Uses `AndroidKeyStore` directly with AES-256-GCM (`setRandomizedEncryptionRequired(true)`) so
 * the wrapping key never leaves hardware TEE/StrongBox and there are zero deprecated Jetpack
 * `security-crypto` dependencies. The wrapped envelope is stored in private SharedPreferences
 * excluded from cloud backup & device transfer via `backup_rules.xml` / `data_extraction_rules.xml`.
 */
object OrbitMasterKeyStore {
    private const val KEYSTORE_PROVIDER = "AndroidKeyStore"
    private const val KEY_ALIAS = "dev.orbit.android.master_key_wrap_v1"
    private const val PREFS_NAME = "orbit_keystore_envelope"
    private const val KEY_WRAPPED_MASTER = "wrapped_master_key_v1"

    private val lock = Any()

    /**
     * Load the existing 32-byte master key by unwrapping it through AndroidKeyStore, or generate
     * a fresh 32-byte CSPRNG master key, seal it, and persist the envelope atomically.
     *
     * Fail-closed: if a persisted envelope exists but cannot be unwrapped (e.g. tampering or
     * keystore corruption), throws rather than silently minting a mismatched key.
     */
    fun getOrCreateMasterKey(context: Context): ByteArray = synchronized(lock) {
        val appContext = context.applicationContext
        val prefs = appContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        val existing = prefs.getString(KEY_WRAPPED_MASTER, null)

        if (existing != null) {
            val wrappingKey = loadExistingKeystoreKey()
                ?: throw IllegalStateException(
                    "Android Keystore wrapping key missing for existing sealed master key envelope"
                )
            return MasterKeyEnvelopeCodec.unwrapWithKey(existing, wrappingKey)
        }

        val wrappingKey = getOrCreateKeystoreKey()
        val masterKey = MasterKeyEnvelopeCodec.generateRandomMasterKey()
        val sealed = MasterKeyEnvelopeCodec.wrapWithKey(masterKey, wrappingKey)
        val committed = prefs.edit().putString(KEY_WRAPPED_MASTER, sealed).commit()
        check(committed) { "Failed to persist wrapped master key envelope" }
        masterKey
    }

    private fun loadExistingKeystoreKey(): SecretKey? {
        val keyStore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
        val entry = keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry
        return entry?.secretKey
    }

    private fun getOrCreateKeystoreKey(): SecretKey {
        loadExistingKeystoreKey()?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER)
        val spec = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setRandomizedEncryptionRequired(true)
            .build()
        generator.init(spec)
        return generator.generateKey()
    }
}
