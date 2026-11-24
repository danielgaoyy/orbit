package dev.orbit.android.ui.sheets

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.orbit.android.R
import dev.orbit.android.store.OrbitAndroidStore
import dev.orbit.android.store.OrbitUiState
import dev.orbit.android.ui.stage.buildHighlightedLine
import dev.orbit.android.ui.theme.OrbitTheme
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import uniffi.orbit_ffi_uniffi.FfiWorkspaceFile

@Composable
internal fun WorkspaceFileSheet(ui: OrbitUiState, store: OrbitAndroidStore, onClose: () -> Unit) {
    val host = ui.selectedHostId ?: return
    key(host, ui.activeCwd) {
        var directory by rememberSaveable { mutableStateOf("") }
        var path by rememberSaveable { mutableStateOf<String?>(null) }
        var filter by rememberSaveable { mutableStateOf("") }
        LaunchedEffect(Unit) { store.refreshWorkspaceFiles() }
        val back = {
            if (path != null) path = null else directory = directory.substringBeforeLast('/', "")
        }
        ToolSheet(R.string.files_title, onClose) {
            BackHandler(path != null || directory.isNotEmpty(), back)
            if (path != null || directory.isNotEmpty())
                TextButton(onClick = back) {
                    Text(stringResource(R.string.nav_back) + " · " + (path ?: directory))
                }
            val selected = path
            if (selected != null) {
                WorkspaceFileContent(host, ui.activeCwd, selected, store)
            } else {
                OutlinedTextField(
                    filter,
                    { filter = it },
                    Modifier.fillMaxWidth(),
                    singleLine = true,
                    placeholder = { Text(stringResource(R.string.files_search_placeholder)) },
                )
                val entries =
                    remember(directory, filter, ui.workspaceFiles) {
                        val prefix = if (directory.isEmpty()) "" else "$directory/"
                        ui.workspaceFiles
                            .asSequence()
                            .filter {
                                it.startsWith(prefix) && it.contains(filter, ignoreCase = true)
                            }
                            .map { it.removePrefix(prefix) }
                            .filter(String::isNotEmpty)
                            .map { it.substringBefore('/') to it.contains('/') }
                            .distinct()
                            .sortedWith(
                                compareByDescending<Pair<String, Boolean>> { it.second }
                                    .thenBy { it.first }
                            )
                            .toList()
                    }
                if (entries.isEmpty()) Text(stringResource(R.string.files_empty))
                LazyColumn {
                    items(entries) { (name, folder) ->
                        TextButton(
                            onClick = {
                                val next =
                                    listOf(directory, name)
                                        .filter(String::isNotEmpty)
                                        .joinToString("/")
                                if (folder) directory = next else path = next
                            }
                        ) {
                            Text(name + if (folder) "/" else "", style = OrbitTheme.typography.code)
                        }
                    }
                }
            }
        }
    }
}

@Composable
internal fun WorkspaceFileContent(
    host: String,
    cwd: String,
    path: String,
    store: OrbitAndroidStore,
) =
    key(host, cwd, path) {
        var file by remember { mutableStateOf<FfiWorkspaceFile?>(null) }
        var error by remember { mutableStateOf<String?>(null) }
        var edits by rememberSaveable { mutableStateOf<String?>(null) }
        var busy by remember { mutableStateOf(false) }
        var revision by remember { mutableIntStateOf(0) }
        val scope = rememberCoroutineScope()
        val conflictText = stringResource(R.string.files_conflict)
        LaunchedEffect(revision) {
            error = null
            try {
                file = store.readFile(host, cwd, path)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                error = e.localizedMessage ?: e.toString()
            }
        }
        error?.let { Text(it, color = OrbitTheme.colors.dangerRemoved) }
        val current = file
        if (current == null) {
            if (error != null && path == ".orbit/hooks.json") {
                TextButton(
                    enabled = !busy,
                    onClick = {
                        busy = true
                        scope.launch {
                            try {
                                store.createProjectHooks(host, cwd)
                                revision++
                            } catch (e: CancellationException) {
                                throw e
                            } catch (e: Exception) {
                                error = e.localizedMessage ?: e.toString()
                            } finally {
                                busy = false
                            }
                        }
                    },
                ) {
                    Text(stringResource(R.string.create_hooks_file))
                }
            }
            if (error == null) Text(stringResource(R.string.files_loading))
            else TextButton(onClick = { revision++ }) { Text(stringResource(R.string.nav_refresh)) }
        } else if (current.contentKind != "text") {
            Text(current.mimeType ?: current.contentKind, style = OrbitTheme.typography.meta)
        } else {
            if (current.truncated)
                Text(stringResource(R.string.files_truncated, current.content.toByteArray().size))
            val draft = edits
            if (draft == null) {
                if (current.writable && !current.truncated && current.etag != null) {
                    TextButton(onClick = { edits = current.content }) {
                        Text(stringResource(R.string.plan_tab_edit))
                    }
                }
                val lines = remember(current) { current.content.lines() }
                val highlighted =
                    remember(current) {
                        current.highlightedLines.associateBy { it.lineNumber.toInt() }
                    }
                val scroll = rememberScrollState()
                LazyColumn(
                    Modifier.fillMaxWidth()
                        .heightIn(max = 520.dp)
                        .background(OrbitTheme.colors.code)
                        .horizontalScroll(scroll)
                        .padding(8.dp)
                ) {
                    items(lines.size) { index ->
                        val fg = OrbitTheme.colors.fgPrimary
                        val tokens = highlighted[index + 1]
                        if (tokens == null) Text(lines[index], style = OrbitTheme.typography.code)
                        else
                            Text(
                                remember(tokens, fg) { buildHighlightedLine(tokens, fg) },
                                style = OrbitTheme.typography.code,
                            )
                    }
                }
            } else {
                OutlinedTextField(
                    draft,
                    { edits = it },
                    Modifier.fillMaxWidth(),
                    maxLines = 10,
                    textStyle = OrbitTheme.typography.code,
                )
                Row {
                    TextButton(onClick = { edits = null }, enabled = !busy) {
                        Text(stringResource(R.string.nav_cancel))
                    }
                    TextButton(
                        enabled = !busy && draft != current.content,
                        onClick = {
                            busy = true
                            scope.launch {
                                try {
                                    if (store.saveFile(current, draft)) {
                                        edits = null
                                        revision++
                                    } else error = conflictText
                                } catch (e: CancellationException) {
                                    throw e
                                } catch (e: Exception) {
                                    error = e.localizedMessage ?: e.toString()
                                } finally {
                                    busy = false
                                }
                            }
                        },
                    ) {
                        Text(stringResource(R.string.nav_save))
                    }
                }
            }
        }
    }
