package dev.orbit.android.ui.composer

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ArrowUpward
import androidx.compose.material.icons.outlined.Mic
import androidx.compose.material.icons.outlined.Stop
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import dev.orbit.android.R
import dev.orbit.android.store.ComposerActionReducer
import dev.orbit.android.store.ComposerRightActionState
import dev.orbit.android.ui.theme.OrbitTheme

enum class VoiceNavigation(val phrase: Int) {
    OVERVIEW(R.string.voice_open_overview),
    SEARCH(R.string.voice_open_search);

    companion object {
        fun fromTranscript(text: String, phraseFor: (VoiceNavigation) -> String): VoiceNavigation? {
            val phrase = text.trim().trimEnd('.', '。')
            return entries.firstOrNull { phrase.equals(phraseFor(it), ignoreCase = true) }
        }
    }
}

/** Recognition supplies an editable draft. Sending always requires a separate user gesture. */
@Composable
internal fun ComposerAction(
    hasDraft: Boolean,
    running: Boolean,
    enabled: Boolean,
    onDictated: (String) -> Unit,
    onListening: (Boolean) -> Unit,
    onSubmit: () -> Unit,
    onInterrupt: () -> Unit,
    onNavigate: (VoiceNavigation) -> Unit,
) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val navigate by rememberUpdatedState(onNavigate)
    val dictated by rememberUpdatedState(onDictated)
    val listening by rememberUpdatedState(onListening)
    var session by remember { mutableStateOf<SpeechRecognizer?>(null) }
    var recording by remember { mutableStateOf(false) }

    fun finish() {
        val previous = session
        session = null
        previous?.destroy()
        recording = false
        listening(false)
    }
    fun unavailable() {
        Toast.makeText(context, R.string.composer_voice_unavailable, Toast.LENGTH_SHORT).show()
    }
    fun start() {
        if (session != null || !owner.lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED))
            return
        if (!SpeechRecognizer.isRecognitionAvailable(context)) {
            unavailable()
            return
        }
        try {
            val recognizer = SpeechRecognizer.createSpeechRecognizer(context)
            session = recognizer
            recognizer.setRecognitionListener(
                object : RecognitionListener {
                    override fun onReadyForSpeech(params: Bundle?) {
                        if (session === recognizer) {
                            recording = true
                            listening(true)
                        }
                    }

                    override fun onResults(results: Bundle?) {
                        if (session !== recognizer) return
                        finish()
                        results
                            ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
                            ?.firstOrNull()
                            ?.trim()
                            ?.takeIf(String::isNotEmpty)
                            ?.let { text ->
                                val command =
                                    VoiceNavigation.fromTranscript(text) {
                                        context.getString(it.phrase)
                                    }
                                if (command != null) navigate(command) else dictated(text)
                            }
                    }

                    override fun onEndOfSpeech() {
                        if (session === recognizer) {
                            recording = false
                            listening(false)
                        }
                    }

                    override fun onError(error: Int) {
                        if (session === recognizer) {
                            finish()
                            unavailable()
                        }
                    }

                    override fun onBeginningOfSpeech() = Unit

                    override fun onRmsChanged(rmsdB: Float) = Unit

                    override fun onBufferReceived(buffer: ByteArray?) = Unit

                    override fun onPartialResults(partialResults: Bundle?) = Unit

                    override fun onEvent(eventType: Int, params: Bundle?) = Unit
                }
            )
            recognizer.startListening(
                Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
                    putExtra(
                        RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                        RecognizerIntent.LANGUAGE_MODEL_FREE_FORM,
                    )
                }
            )
        } catch (_: RuntimeException) {
            finish()
            unavailable()
        }
    }

    val permission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted && enabled && !hasDraft && !running) start()
            else if (!granted)
                Toast.makeText(
                        context,
                        R.string.composer_voice_permission_denied,
                        Toast.LENGTH_SHORT,
                    )
                    .show()
        }
    DisposableEffect(owner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) finish()
        }
        owner.lifecycle.addObserver(observer)
        onDispose {
            owner.lifecycle.removeObserver(observer)
            finish()
        }
    }
    val action = ComposerActionReducer.resolve(recording, running, hasDraft)
    val (icon, label) =
        when (action) {
            ComposerRightActionState.VOICE -> Icons.Outlined.Mic to R.string.composer_voice_start
            ComposerRightActionState.RECORDING_STOP ->
                Icons.Outlined.Stop to R.string.composer_voice_stop
            ComposerRightActionState.STOP -> Icons.Outlined.Stop to R.string.composer_stop
            ComposerRightActionState.QUEUE -> Icons.Outlined.ArrowUpward to R.string.composer_queue
            ComposerRightActionState.SEND -> Icons.Outlined.ArrowUpward to R.string.composer_send
        }
    IconButton(
        enabled = enabled && (session == null || recording),
        modifier = Modifier.size(48.dp).background(OrbitTheme.colors.primary, CircleShape),
        onClick = {
            when (action) {
                ComposerRightActionState.VOICE ->
                    if (
                        ContextCompat.checkSelfPermission(
                            context,
                            Manifest.permission.RECORD_AUDIO,
                        ) == PackageManager.PERMISSION_GRANTED
                    )
                        start()
                    else permission.launch(Manifest.permission.RECORD_AUDIO)
                ComposerRightActionState.RECORDING_STOP -> session?.stopListening()
                ComposerRightActionState.STOP -> onInterrupt()
                else -> onSubmit()
            }
        },
    ) {
        Icon(icon, stringResource(label), tint = OrbitTheme.colors.onPrimary)
    }
}
