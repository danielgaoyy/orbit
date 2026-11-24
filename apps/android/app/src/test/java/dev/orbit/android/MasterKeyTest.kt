package dev.orbit.android

import dev.orbit.android.store.MasterKeyEnvelopeCodec as Codec
import java.util.Base64
import javax.crypto.AEADBadTagException
import javax.crypto.spec.SecretKeySpec
import org.junit.Assert.*
import org.junit.Test

class MasterKeyTest {
    private val wrapping = SecretKeySpec(ByteArray(32) { it.toByte() }, "AES")

    @Test
    fun freshMasterKeysAndIvsHaveCorrectSizesAndRoundTrip() {
        val key = Codec.generateRandomMasterKey()
        assertEquals(32, key.size)
        assertFalse(key.contentEquals(Codec.generateRandomMasterKey()))
        val encoded = Codec.wrapWithKey(key, wrapping)
        val envelope = Codec.decodeEnvelope(encoded)
        assertEquals(12, envelope.iv.size)
        assertEquals(48, envelope.ciphertext.size)
        assertNotEquals(encoded, Codec.wrapWithKey(key, wrapping))
        assertArrayEquals(key, Codec.unwrapWithKey(encoded, wrapping))
    }

    @Test
    fun rejectsMalformedVersionSizeAndBase64() {
        val bytes = Base64.getDecoder().decode(Codec.wrapWithKey(ByteArray(32), wrapping))
        listOf(bytes.copyOf(60), bytes.copyOf(62), bytes.clone().apply { this[0] = 2 }).forEach {
            assertThrows(IllegalArgumentException::class.java) {
                Codec.decodeEnvelope(Base64.getEncoder().encodeToString(it))
            }
        }
        assertThrows(IllegalArgumentException::class.java) { Codec.decodeEnvelope("*") }
        assertThrows(IllegalArgumentException::class.java) {
            Codec.wrapWithKey(ByteArray(31), wrapping)
        }
        assertThrows(IllegalArgumentException::class.java) {
            Codec.encodeEnvelope(ByteArray(11), ByteArray(48))
        }
    }

    @Test
    fun ciphertextAndWrappingKeyAreAuthenticated() {
        val sealed = Codec.wrapWithKey(Codec.generateRandomMasterKey(), wrapping)
        val env = Codec.decodeEnvelope(sealed)
        env.ciphertext[0] = (env.ciphertext[0].toInt() xor 1).toByte()
        assertThrows(AEADBadTagException::class.java) {
            Codec.unwrapWithKey(Codec.encodeEnvelope(env.iv, env.ciphertext), wrapping)
        }
        assertThrows(AEADBadTagException::class.java) {
            Codec.unwrapWithKey(sealed, SecretKeySpec(ByteArray(32), "AES"))
        }
    }
}
