package dev.orbit.android.ui.sheets

import android.content.Context
import android.text.InputType
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView

/**
 * Pure VT100 / ANSI control-sequence encoder for Orbit Android raw PTY sessions.
 * Decoupled from Android views so every key chord can be verified in JVM unit tests.
 */
object OrbitVt100Encoder {
    private const val ESC_BYTE: Byte = 0x1B
    private const val DEL_BYTE: Byte = 0x7F

    fun controlByteFor(ch: Char): Byte? {
        val upper = ch.uppercaseChar()
        if (upper in 'A'..'Z') {
            return (upper.code - 64).toByte()
        }
        return when (ch) {
            ' ', '@' -> 0x00
            '[' -> 0x1B
            '\\' -> 0x1C
            ']' -> 0x1D
            '^' -> 0x1E
            '_' -> 0x1F
            '?' -> DEL_BYTE
            else -> null
        }
    }

    fun encodeCommittedText(
        input: CharSequence,
        ctrlLatched: Boolean = false,
        altLatched: Boolean = false,
    ): ByteArray {
        if (input.isEmpty()) return ByteArray(0)
        if (input.length == 1 && (ctrlLatched || altLatched)) {
            val single = input[0]
            if (ctrlLatched) {
                controlByteFor(single)?.let { ctrl ->
                    return if (altLatched) byteArrayOf(ESC_BYTE, ctrl) else byteArrayOf(ctrl)
                }
            }
            if (altLatched) {
                val utf8 = single.toString().toByteArray(Charsets.UTF_8)
                return byteArrayOf(ESC_BYTE) + utf8
            }
        }
        val normalized = input.toString().replace("\r\n", "\r").replace("\n", "\r")
        return normalized.toByteArray(Charsets.UTF_8)
    }

    fun encodeHardwareKey(
        keyCode: Int,
        unicodeCodePoint: Int = 0,
        ctrlActive: Boolean = false,
        altActive: Boolean = false,
    ): ByteArray? = when (keyCode) {
        KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> byteArrayOf('\r'.code.toByte())
        KeyEvent.KEYCODE_DEL -> byteArrayOf(DEL_BYTE)
        KeyEvent.KEYCODE_FORWARD_DEL -> "\u001B[3~".toByteArray(Charsets.UTF_8)
        KeyEvent.KEYCODE_TAB -> byteArrayOf('\t'.code.toByte())
        KeyEvent.KEYCODE_ESCAPE -> byteArrayOf(ESC_BYTE)
        KeyEvent.KEYCODE_DPAD_UP -> "\u001B[A".toByteArray(Charsets.UTF_8)
        KeyEvent.KEYCODE_DPAD_DOWN -> "\u001B[B".toByteArray(Charsets.UTF_8)
        KeyEvent.KEYCODE_DPAD_RIGHT -> "\u001B[C".toByteArray(Charsets.UTF_8)
        KeyEvent.KEYCODE_DPAD_LEFT -> "\u001B[D".toByteArray(Charsets.UTF_8)
        KeyEvent.KEYCODE_MOVE_HOME -> "\u001B[H".toByteArray(Charsets.UTF_8)
        KeyEvent.KEYCODE_MOVE_END -> "\u001B[F".toByteArray(Charsets.UTF_8)
        KeyEvent.KEYCODE_PAGE_UP -> "\u001B[5~".toByteArray(Charsets.UTF_8)
        KeyEvent.KEYCODE_PAGE_DOWN -> "\u001B[6~".toByteArray(Charsets.UTF_8)
        else -> encodePrintableOrChord(keyCode, unicodeCodePoint, ctrlActive, altActive)
    }

    private fun encodePrintableOrChord(
        keyCode: Int,
        unicodeCodePoint: Int,
        ctrlActive: Boolean,
        altActive: Boolean,
    ): ByteArray? {
        if (ctrlActive) {
            val baseChar = if (unicodeCodePoint in 32..126) {
                unicodeCodePoint.toChar()
            } else {
                keyCodeToAsciiChar(keyCode)
            }
            val ctrl = baseChar?.let(::controlByteFor) ?: return null
            return if (altActive) byteArrayOf(ESC_BYTE, ctrl) else byteArrayOf(ctrl)
        }
        if (unicodeCodePoint > 0) {
            val text = String(Character.toChars(unicodeCodePoint))
            return encodeCommittedText(text, ctrlLatched = false, altLatched = altActive)
        }
        return null
    }

    private fun keyCodeToAsciiChar(keyCode: Int): Char? {
        if (keyCode in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z) {
            return ('A'.code + (keyCode - KeyEvent.KEYCODE_A)).toChar()
        }
        return when (keyCode) {
            KeyEvent.KEYCODE_SPACE -> ' '
            KeyEvent.KEYCODE_LEFT_BRACKET -> '['
            KeyEvent.KEYCODE_BACKSLASH -> '\\'
            KeyEvent.KEYCODE_RIGHT_BRACKET -> ']'
            else -> null
        }
    }
}

/**
 * Custom Android [View] bridging IME text commits and physical key events directly to Orbit's
 * remote shim PTY (`sendTerminalBytes`) with zero line buffering.
 */
class OrbitRawTerminalView(context: Context) : View(context) {
    var ctrlLatched: Boolean = false
    var altLatched: Boolean = false
    var onBytesReady: ((ByteArray) -> Unit)? = null
    var onClearLatchedModifiers: (() -> Unit)? = null

    init {
        isFocusable = true
        isFocusableInTouchMode = true
        isClickable = true
    }

    override fun onCheckIsTextEditor(): Boolean = true

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN or
            EditorInfo.IME_FLAG_NO_EXTRACT_UI or
            EditorInfo.IME_ACTION_NONE

        return object : BaseInputConnection(this, false) {
            override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                val content = text ?: return true
                if (content.isNotEmpty()) {
                    emitBytes(
                        OrbitVt100Encoder.encodeCommittedText(
                            input = content,
                            ctrlLatched = ctrlLatched,
                            altLatched = altLatched,
                        )
                    )
                }
                return true
            }

            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                val count = beforeLength.coerceAtLeast(1)
                onBytesReady?.invoke(ByteArray(count) { 0x7F })
                return true
            }

            override fun sendKeyEvent(event: KeyEvent): Boolean {
                if (event.action == KeyEvent.ACTION_DOWN) {
                    return dispatchKeyDown(event)
                }
                return true
            }
        }
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (dispatchKeyDown(event)) return true
        return super.onKeyDown(keyCode, event)
    }

    private fun dispatchKeyDown(event: KeyEvent): Boolean {
        val encoded = OrbitVt100Encoder.encodeHardwareKey(
            keyCode = event.keyCode,
            unicodeCodePoint = event.unicodeChar,
            ctrlActive = event.isCtrlPressed || ctrlLatched,
            altActive = event.isAltPressed || altLatched,
        ) ?: return false
        emitBytes(encoded)
        return true
    }

    private fun emitBytes(bytes: ByteArray) {
        if (bytes.isEmpty()) return
        onBytesReady?.invoke(bytes)
        if (ctrlLatched || altLatched) {
            ctrlLatched = false
            altLatched = false
            onClearLatchedModifiers?.invoke()
        }
    }

    fun focusAndShowIme() {
        requestFocus()
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
        imm?.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT)
    }
}

@Composable
fun RawTerminalInputBridge(
    focusTrigger: Int,
    ctrlLatched: Boolean,
    altLatched: Boolean,
    onSendBytes: (ByteArray) -> Unit,
    onModifiersCleared: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewRef = remember { arrayOfNulls<OrbitRawTerminalView>(1) }

    LaunchedEffect(focusTrigger) {
        if (focusTrigger > 0) {
            viewRef[0]?.focusAndShowIme()
        }
    }

    AndroidView(
        modifier = modifier,
        factory = { ctx ->
            OrbitRawTerminalView(ctx).apply {
                this.ctrlLatched = ctrlLatched
                this.altLatched = altLatched
                this.onBytesReady = onSendBytes
                this.onClearLatchedModifiers = onModifiersCleared
                viewRef[0] = this
            }
        },
        update = { view ->
            view.ctrlLatched = ctrlLatched
            view.altLatched = altLatched
            view.onBytesReady = onSendBytes
            view.onClearLatchedModifiers = onModifiersCleared
            viewRef[0] = view
        },
    )
}
