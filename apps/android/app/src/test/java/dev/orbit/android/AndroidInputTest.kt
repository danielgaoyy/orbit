package dev.orbit.android

import android.view.KeyEvent
import androidx.compose.ui.unit.dp
import dev.orbit.android.ui.sheets.OrbitVt100Encoder as Encoder
import dev.orbit.android.ui.sheets.buildGitPushTerminalCommand
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
    fun developerKeybarAppliesLatchedModifiersAndCtrlZ() {
        assertArrayEquals(byteArrayOf(0x1A), Encoder.encodeDeveloperKey("\u001A"))
        assertArrayEquals(
            "\u001B[1;5C".toByteArray(),
            Encoder.encodeDeveloperKey("\u001B[C", ctrlLatched = true),
        )
        assertArrayEquals(
            "\u001B[1;3D".toByteArray(),
            Encoder.encodeDeveloperKey("\u001B[D", altLatched = true),
        )
        assertArrayEquals(
            "\u001B[1;7A".toByteArray(),
            Encoder.encodeDeveloperKey("\u001B[A", ctrlLatched = true, altLatched = true),
        )
        assertArrayEquals(
            byteArrayOf(0x1B, '\t'.code.toByte()),
            Encoder.encodeDeveloperKey("\t", altLatched = true),
        )
        assertArrayEquals(
            "\u001B~".toByteArray(),
            Encoder.encodeDeveloperKey("~", altLatched = true),
        )
        assertArrayEquals(
            byteArrayOf(0x1F),
            Encoder.encodeDeveloperKey("/", ctrlLatched = true),
        )
        assertArrayEquals(
            "\u001B[1;5D".toByteArray(),
            Encoder.encodeHardwareKey(KeyEvent.KEYCODE_DPAD_LEFT, ctrlActive = true),
        )
    }

    @Test
    fun gitPushTerminalCommandSupportsHomeAndAbsolutePathsSafely() {
        assertEquals("git -C '/Users/dev/orbit' push", buildGitPushTerminalCommand("/Users/dev/orbit"))
        assertEquals("git -C ~/'Code/orbit' push", buildGitPushTerminalCommand("~/Code/orbit"))
        assertEquals("git -C ~ push", buildGitPushTerminalCommand("~"))
        assertEquals(
            "git -C ~/'my '\\''repo'\\''' push",
            buildGitPushTerminalCommand("~/my 'repo'"),
        )
        assertNull(buildGitPushTerminalCommand("relative/path"))
        assertNull(buildGitPushTerminalCommand("/tmp/bad\nrm -rf /"))
    }

    @Test
    fun bookLayoutUsesPhysicalPositionAndRequiresSeparatingOrHalfFolded() {
        val hinge = OrbitHingeGeometry(true, true, true, 500, 0, 530, 1100)
        assertEquals(
            OrbitFoldablePosture.BookDualPane(250.dp, 15.dp),
            OrbitFoldablePostureMapper.mapGeometry(hinge, 1100, 1100, 2f),
        )
        // Half-folded non-separating seamless hinge enters BookDualPane
        assertEquals(
            OrbitFoldablePosture.BookDualPane(250.dp, 0.dp),
            OrbitFoldablePostureMapper.mapGeometry(
                hinge.copy(rightPx = 500, isHalfFolded = true, isSeparatingSurface = false),
                1100,
                1100,
                2f,
            ),
        )
        // Flat seamless unfolded foldable returns FlatOrStandard for collapsible sidebar + 3-pane diff
        assertEquals(
            OrbitFoldablePosture.FlatOrStandard,
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
