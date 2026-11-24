package dev.orbit.android

import android.view.KeyEvent
import androidx.compose.ui.unit.dp
import dev.orbit.android.ui.sheets.OrbitVt100Encoder as Encoder
import dev.orbit.android.ui.shell.*
import org.junit.Assert.*
import org.junit.Test

class AndroidInputTest {
    @Test
    fun physicalKeysEmitVt100Bytes() {
        mapOf(
                KeyEvent.KEYCODE_ESCAPE to "\u001b",
                KeyEvent.KEYCODE_TAB to "\t",
                KeyEvent.KEYCODE_DPAD_UP to "\u001b[A",
                KeyEvent.KEYCODE_DPAD_DOWN to "\u001b[B",
                KeyEvent.KEYCODE_DPAD_RIGHT to "\u001b[C",
                KeyEvent.KEYCODE_DPAD_LEFT to "\u001b[D",
                KeyEvent.KEYCODE_DEL to "\u007f",
                KeyEvent.KEYCODE_FORWARD_DEL to "\u001b[3~",
                KeyEvent.KEYCODE_ENTER to "\r",
            )
            .forEach { (key, text) ->
                assertArrayEquals(text.toByteArray(), Encoder.encodeHardwareKey(key))
            }
        assertNull(Encoder.encodeHardwareKey(KeyEvent.KEYCODE_UNKNOWN))
    }

    @Test
    fun stickyModifiersAndUnicodeSurviveTheIme() {
        assertArrayEquals(byteArrayOf(27, 3), Encoder.encodeCommittedText("c", true, true))
        assertArrayEquals(byteArrayOf(0), Encoder.encodeCommittedText("@", true))
        assertArrayEquals("日本語\r🙂\r".toByteArray(), Encoder.encodeCommittedText("日本語\r\n🙂\n"))
        assertArrayEquals("🙂".toByteArray(), Encoder.encodeHardwareKey(0, 0x1f642))
        assertArrayEquals(
            byteArrayOf(4),
            Encoder.encodeHardwareKey(KeyEvent.KEYCODE_D, ctrlActive = true),
        )
    }

    @Test
    fun bookLayoutUsesPhysicalPositionAndOcclusion() {
        val hinge = OrbitHingeGeometry(true, true, true, 500, 0, 530, 1100)
        assertEquals(
            OrbitFoldablePosture.BookDualPane(250.dp, 15.dp),
            OrbitFoldablePostureMapper.mapGeometry(hinge, 1100, 1100, 2f),
        )
        assertEquals(
            OrbitFoldablePosture.BookDualPane(250.dp, 0.dp),
            OrbitFoldablePostureMapper.mapGeometry(
                hinge.copy(rightPx = 500, isHalfFolded = false, isSeparatingSurface = false),
                1100,
                1100,
                2f,
            ),
        )
    }

    @Test
    fun tabletopAndCompactWindowsRespectBothDeckMinima() {
        val hinge = OrbitHingeGeometry(false, true, true, 0, 600, 900, 624)
        assertEquals(
            OrbitFoldablePosture.TabletopSplit(300.dp, 12.dp),
            OrbitFoldablePostureMapper.mapGeometry(hinge, 900, 1200, 2f),
        )
        listOf(
                OrbitFoldablePostureMapper.mapGeometry(hinge, 900, 800, 2f),
                OrbitFoldablePostureMapper.mapGeometry(null, 900, 1200, 2f),
                OrbitFoldablePostureMapper.mapGeometry(hinge, 900, 1200, 0f),
            )
            .forEach { assertEquals(OrbitFoldablePosture.FlatOrStandard, it) }
    }
}
