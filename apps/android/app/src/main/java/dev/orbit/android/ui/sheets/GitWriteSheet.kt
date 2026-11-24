package dev.orbit.android.ui.sheets

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.orbit.android.R
import dev.orbit.android.store.OrbitAndroidStore
import dev.orbit.android.store.OrbitUiState
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.GitConflictResolution

internal fun buildGitPushTerminalCommand(cwd: String): String? {
    val trimmed = cwd.trim()
    if (trimmed.isEmpty() || trimmed.any { it.isISOControl() }) return null
    return when {
        trimmed == "~" -> "git -C ~ push"
        trimmed.startsWith("~/") -> {
            val suffix = trimmed.removePrefix("~/").replace("'", "'\\''")
            "git -C ~/'$suffix' push"
        }
        trimmed.startsWith("/") -> {
            val escaped = trimmed.replace("'", "'\\''")
            "git -C '$escaped' push"
        }
        else -> null
    }
}

@Composable
internal fun GitWriteSheet(
    ui: OrbitUiState,
    store: OrbitAndroidStore,
    onDiff: () -> Unit,
    onPush: (String) -> Unit,
    onClose: () -> Unit,
) =
    key(ui.selectedHostId, ui.activeCwd) {
        var message by rememberSaveable { mutableStateOf("") }
        var title by rememberSaveable { mutableStateOf("") }
        var body by rememberSaveable { mutableStateOf("") }
        var amend by rememberSaveable { mutableStateOf(false) }
        var draft by rememberSaveable { mutableStateOf(true) }
        val ready = !ui.gitMutationBusy && ui.selectedHostId != null
        val review =
            ui.gitReview?.takeIf { it.hostId == ui.selectedHostId && it.cwd == ui.activeCwd }
        LaunchedEffect(Unit) {
            store.loadGitReview()
            store.refreshGitExtras()
        }
        LaunchedEffect(ui.generatedCommitMessage) {
            ui.generatedCommitMessage?.let {
                message = it
                store.consumeGeneratedCommitMessage()
            }
        }
        ToolSheet(R.string.git_changes, onClose) {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Text(
                    listOfNotNull(review?.branch, ui.activeCwd).joinToString(" · "),
                    style = OrbitTheme.typography.code,
                )
                TextButton(onClick = onDiff) { Text(stringResource(R.string.nav_diff_review)) }
                review?.changedFiles.orEmpty().forEach { entry ->
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(entry.path, Modifier.weight(1f), style = OrbitTheme.typography.code)
                        TextButton(
                            enabled = ready,
                            onClick = {
                                if (entry.staged) store.gitUnstagePaths(listOf(entry.path), false)
                                else store.gitStagePaths(listOf(entry.path), false)
                            },
                        ) {
                            Text(
                                stringResource(
                                    if (entry.staged) R.string.git_unstage_file
                                    else R.string.git_stage_file
                                )
                            )
                        }
                    }
                }
                if (review?.changedFiles.orEmpty().isNotEmpty())
                    Row {
                        TextButton(
                            enabled = ready && review?.changedFiles.orEmpty().any { !it.staged },
                            onClick = { store.gitStagePaths(emptyList(), true) },
                        ) {
                            Text(stringResource(R.string.git_stage_all))
                        }
                        TextButton(
                            enabled = ready && review?.changedFiles.orEmpty().any { it.staged },
                            onClick = { store.gitUnstagePaths(emptyList(), true) },
                        ) {
                            Text(stringResource(R.string.git_unstage_all))
                        }
                    }
                OutlinedTextField(
                    message,
                    { message = it },
                    Modifier.fillMaxWidth(),
                    label = { Text(stringResource(R.string.git_commit_placeholder)) },
                )
                ToggleRow(R.string.git_amend, amend) { amend = it }
                Row {
                    TextButton(enabled = ready, onClick = store::gitGenerateCommitMessage) {
                        Text(stringResource(R.string.git_generate_commit))
                    }
                    TextButton(
                        enabled =
                            ready &&
                                message.isNotBlank() &&
                                (amend || review?.changedFiles.orEmpty().any { it.staged }),
                        onClick = {
                            store.gitCommit(message, amend) {
                                message = ""
                                amend = false
                            }
                        },
                    ) {
                        Text(stringResource(R.string.git_commit))
                    }
                }
                val pushCommand = remember(ui.activeCwd) { buildGitPushTerminalCommand(ui.activeCwd) }
                TextButton(
                    enabled = ready && review?.isGitRepo == true && pushCommand != null,
                    onClick = { pushCommand?.let(onPush) },
                ) {
                    Text(stringResource(R.string.git_push_terminal))
                }
                if (ui.gitConflicts.isNotEmpty()) {
                    HorizontalDivider(thickness = 0.5.dp)
                    Column(
                        modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                        verticalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Text(
                            text = stringResource(R.string.git_conflicts_banner, ui.gitConflicts.size),
                            style = OrbitTheme.typography.subheadSemiBold,
                            color = OrbitTheme.colors.dangerRemoved,
                        )
                        ui.gitConflicts.forEach { conflict ->
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(4.dp),
                            ) {
                                Text(
                                    text = conflict.path,
                                    style = OrbitTheme.typography.code,
                                    modifier = Modifier.weight(1f),
                                )
                                TextButton(
                                    enabled = ready && !conflict.isBinary,
                                    onClick = {
                                        store.gitResolveConflict(
                                            conflict.path,
                                            GitConflictResolution.Ours,
                                            conflict.contentSha256,
                                        )
                                    },
                                ) {
                                    Text(stringResource(R.string.git_resolve_ours))
                                }
                                TextButton(
                                    enabled = ready && !conflict.isBinary,
                                    onClick = {
                                        store.gitResolveConflict(
                                            conflict.path,
                                            GitConflictResolution.Theirs,
                                            conflict.contentSha256,
                                        )
                                    },
                                ) {
                                    Text(stringResource(R.string.git_resolve_theirs))
                                }
                            }
                        }
                    }
                }
                HorizontalDivider(thickness = 0.5.dp)
                Column(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text(
                            text = stringResource(R.string.git_stashes_section),
                            style = OrbitTheme.typography.subheadSemiBold,
                        )
                        TextButton(
                            enabled = ready && review?.changedFiles.orEmpty().isNotEmpty(),
                            onClick = { store.gitStashSave() },
                        ) {
                            Text(stringResource(R.string.git_stash_save))
                        }
                    }
                    ui.gitStashes.forEach { stash ->
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Column(Modifier.weight(1f)) {
                                Text(
                                    text = stash.stashRef,
                                    style = OrbitTheme.typography.codeMedium,
                                )
                                Text(
                                    text = stash.message.ifBlank { stash.branch.orEmpty() },
                                    style = OrbitTheme.typography.meta,
                                    color = OrbitTheme.colors.fgSecondary,
                                )
                            }
                            TextButton(
                                enabled = ready,
                                onClick = { store.gitStashPop(stash.stashRef, stash.oid) },
                            ) {
                                Text(stringResource(R.string.git_stash_pop))
                            }
                        }
                    }
                }
                HorizontalDivider(thickness = 0.5.dp)
                ui.gitPrStatus?.let { pr ->
                    pr.title?.let { Text(it, style = OrbitTheme.typography.subheadMedium) }
                }
                OutlinedTextField(
                    title,
                    { title = it },
                    Modifier.fillMaxWidth(),
                    label = { Text(stringResource(R.string.git_pr_title_placeholder)) },
                )
                OutlinedTextField(
                    body,
                    { body = it },
                    Modifier.fillMaxWidth(),
                    label = { Text(stringResource(R.string.git_pr_body_placeholder)) },
                )
                ToggleRow(R.string.git_pr_draft, draft) { draft = it }
                TextButton(
                    enabled = ready && title.isNotBlank(),
                    onClick = {
                        store.gitCreatePr(title, body, null, draft) {
                            title = ""
                            body = ""
                        }
                    },
                ) {
                    Text(stringResource(R.string.git_create_pr))
                }
            }
        }
    }
