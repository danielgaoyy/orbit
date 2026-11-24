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
                TextButton(
                    enabled =
                        ready &&
                            review?.isGitRepo == true &&
                            ui.activeCwd.startsWith("/") &&
                            ui.activeCwd.none { it.isISOControl() },
                    onClick = { onPush("git -C '" + ui.activeCwd.replace("'", "'\\''") + "' push") },
                ) {
                    Text(stringResource(R.string.git_push_terminal))
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
