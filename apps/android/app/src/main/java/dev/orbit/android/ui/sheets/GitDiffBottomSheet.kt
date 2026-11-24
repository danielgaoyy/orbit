package dev.orbit.android.ui.sheets

import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ChevronRight
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiDiffFile
import uniffi.orbit_ffi_uniffi.FfiDiffLine
import uniffi.orbit_ffi_uniffi.FfiDiffLineKind
import uniffi.orbit_ffi_uniffi.FfiGitReview

private const val INITIAL_DIFF_LINE_LIMIT = 120

private val AutoCollapsedLockfileNames = setOf(
    "cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "podfile.lock",
    "gemfile.lock",
    "composer.lock",
    "poetry.lock",
    "bun.lock",
    "bun.lockb",
)

/**
 * Auto-collapses lockfiles and generated bindings by default while keeping
 * handwritten source files expanded for instant review.
 */
internal fun shouldAutoCollapseDiffFile(path: String): Boolean {
    val normalized = path.replace('\\', '/').trim().lowercase()
    val basename = normalized.substringAfterLast('/')
    if (basename in AutoCollapsedLockfileNames || basename.endsWith(".lock")) return true
    if (normalized.contains("uniffi")) return true
    if (normalized.startsWith("generated/") || normalized.contains("/generated/")) return true
    if (normalized.endsWith(".pbxproj")) return true
    return false
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GitDiffBottomSheet(
    cwd: String,
    gitReview: FfiGitReview?,
    sessionTurnDiffs: List<FfiDiffFile>,
    stagedMode: Boolean,
    selectedPath: String?,
    isRefreshing: Boolean,
    onRefresh: () -> Unit,
    onPullRefresh: () -> Unit,
    onWrite: () -> Unit,
    onSetStagedMode: (Boolean) -> Unit,
    onSelectPath: (String?) -> Unit,
    onQuoteLine: (path: String, lineLabel: String, content: String) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    LaunchedEffect(cwd, stagedMode, selectedPath) {
        onRefresh()
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = colors.elevated,
        contentColor = colors.fgPrimary,
    ) {
        DiffInspectorBody(
            cwd = cwd,
            gitReview = gitReview,
            sessionTurnDiffs = sessionTurnDiffs,
            stagedMode = stagedMode,
            selectedPath = selectedPath,
            isRefreshing = isRefreshing,
            onPullRefresh = onPullRefresh,
            onWrite = onWrite,
            onSetStagedMode = onSetStagedMode,
            onSelectPath = onSelectPath,
            onQuoteLine = onQuoteLine,
            onClose = null,
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.86f)
                .navigationBarsPadding(),
        )
    }
}

/**
 * Inline right-hand 360.dp Diff Inspector Pane used on Large Landscape Tablets
 * (>= 1024.dp): PermanentDrawer 260dp + Stage weight(1f) + GitDiffInspector 360dp.
 */
@Composable
fun GitDiffInspectorPane(
    cwd: String,
    gitReview: FfiGitReview?,
    sessionTurnDiffs: List<FfiDiffFile>,
    stagedMode: Boolean,
    selectedPath: String?,
    isRefreshing: Boolean,
    onRefresh: () -> Unit,
    onPullRefresh: () -> Unit,
    onWrite: () -> Unit,
    onSetStagedMode: (Boolean) -> Unit,
    onSelectPath: (String?) -> Unit,
    onQuoteLine: (path: String, lineLabel: String, content: String) -> Unit,
    onClose: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors

    LaunchedEffect(cwd, stagedMode, selectedPath) {
        onRefresh()
    }

    Row(modifier = modifier.fillMaxHeight()) {
        Box(
            modifier = Modifier
                .width(OrbitSpacing.hairline)
                .fillMaxHeight()
                .background(colors.hairline)
        )
        DiffInspectorBody(
            cwd = cwd,
            gitReview = gitReview,
            sessionTurnDiffs = sessionTurnDiffs,
            stagedMode = stagedMode,
            selectedPath = selectedPath,
            isRefreshing = isRefreshing,
            onPullRefresh = onPullRefresh,
            onWrite = onWrite,
            onSetStagedMode = onSetStagedMode,
            onSelectPath = onSelectPath,
            onQuoteLine = onQuoteLine,
            onClose = onClose,
            modifier = Modifier
                .width(360.dp)
                .fillMaxHeight()
                .background(colors.drawer)
                .statusBarsPadding()
                .navigationBarsPadding(),
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DiffInspectorBody(
    cwd: String,
    gitReview: FfiGitReview?,
    sessionTurnDiffs: List<FfiDiffFile>,
    stagedMode: Boolean,
    selectedPath: String?,
    isRefreshing: Boolean,
    onPullRefresh: () -> Unit,
    onWrite: () -> Unit,
    onSetStagedMode: (Boolean) -> Unit,
    onSelectPath: (String?) -> Unit,
    onQuoteLine: (path: String, lineLabel: String, content: String) -> Unit,
    onClose: (() -> Unit)?,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val pullRefreshState = rememberPullToRefreshState()

    var fileSearchQuery by rememberSaveable(cwd, stagedMode) { mutableStateOf("") }
    val normalizedFileQuery = remember(fileSearchQuery) { fileSearchQuery.trim().lowercase() }

    val repoDiffFiles = gitReview?.diffFiles.orEmpty()
    val effectiveFiles = if (repoDiffFiles.isNotEmpty()) repoDiffFiles else sessionTurnDiffs
    val queryMatchedFiles = remember(effectiveFiles, normalizedFileQuery) {
        if (normalizedFileQuery.isEmpty()) {
            effectiveFiles
        } else {
            effectiveFiles.filter { it.path.lowercase().contains(normalizedFileQuery) }
        }
    }
    val visibleFiles = remember(queryMatchedFiles, selectedPath) {
        if (selectedPath != null) {
            queryMatchedFiles.filter { it.path == selectedPath }
        } else {
            queryMatchedFiles
        }
    }

    // Per-file manual collapse overrides (defaults to shouldAutoCollapseDiffFile)
    val collapsedOverrides = remember(cwd, stagedMode) { mutableStateMapOf<String, Boolean>() }
    // Per-file "Show all N lines" expansion state beyond INITIAL_DIFF_LINE_LIMIT (120)
    val showAllLinesMap = remember(cwd, stagedMode) { mutableStateMapOf<String, Boolean>() }
    val sharedCodeScroll = rememberScrollState()

    val anyVisibleExpanded = visibleFiles.any { f ->
        !(collapsedOverrides[f.path] ?: shouldAutoCollapseDiffFile(f.path))
    }

    Column(modifier = modifier) {
        // Header Row: Branch + Unstaged/Staged + Refresh (+ Close on tablet 3-pane)
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    Text(
                        text = gitReview?.branch ?: OrbitFormatters.shortPath(cwd),
                        style = typography.title,
                        color = colors.fgPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (gitReview != null && (gitReview.ahead > 0u || gitReview.behind > 0u)) {
                        Text(
                            text = "↑${gitReview.ahead} ↓${gitReview.behind}",
                            style = typography.metaMedium,
                            color = colors.fgSecondary,
                            maxLines = 1,
                            softWrap = false,
                        )
                    }
                }
                Text(
                    text = "Tap any diff line to quote into composer",
                    style = typography.meta,
                    color = colors.fgMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }

            Row(verticalAlignment = Alignment.CenterVertically) {
                if (gitReview?.isGitRepo == true) androidx.compose.material3.TextButton(onClick = onWrite) {
                    Text(androidx.compose.ui.res.stringResource(dev.orbit.android.R.string.git_changes))
                }
                Box(
                    modifier = Modifier
                        .size(OrbitSpacing.touchTarget)
                        .clip(RoundedCornerShape(OrbitSpacing.md))
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onPullRefresh()
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Refresh,
                        contentDescription = "Refresh git diff",
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconMd),
                    )
                }
                if (onClose != null) {
                    Box(
                        modifier = Modifier
                            .size(OrbitSpacing.touchTarget)
                            .clip(RoundedCornerShape(OrbitSpacing.md))
                            .clickable { onClose() },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Close,
                            contentDescription = "Close diff inspector",
                            tint = colors.fgSecondary,
                            modifier = Modifier.size(OrbitSpacing.iconMd),
                        )
                    }
                }
            }
        }

        // Unstaged / Staged Quiet Segmented Track
        val trackShape = RoundedCornerShape(OrbitSpacing.md)
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs)
                .background(colors.code, trackShape)
                .border(OrbitSpacing.hairline, colors.hairline, trackShape)
                .padding(OrbitSpacing.xs / 2),
            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
        ) {
            listOf(false to "Unstaged", true to "Staged").forEach { (isStaged, label) ->
                val selected = stagedMode == isStaged
                val segShape = RoundedCornerShape(OrbitSpacing.sm)
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 36.dp)
                        .clip(segShape)
                        .background(if (selected) colors.elevated else Color.Transparent, segShape)
                        .then(
                            if (selected) Modifier.border(OrbitSpacing.hairline, colors.hairline, segShape)
                            else Modifier
                        )
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onSetStagedMode(isStaged)
                        }
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = label,
                        style = typography.subheadSemiBold,
                        color = if (selected) colors.fgPrimary else colors.fgSecondary,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
            }
        }

        // Instant File Filter Search + Collapse All / Expand Code Action + Horizontal File Jump Chips
        if (effectiveFiles.isNotEmpty()) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                val filterShape = RoundedCornerShape(OrbitSpacing.sm)
                Row(
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 44.dp)
                        .clip(filterShape)
                        .background(colors.code, filterShape)
                        .border(OrbitSpacing.hairline, colors.hairline, filterShape)
                        .padding(start = OrbitSpacing.sm),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Search,
                        contentDescription = null,
                        tint = colors.fgMuted,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                    Box(modifier = Modifier.weight(1f)) {
                        if (fileSearchQuery.isEmpty()) {
                            Text(
                                text = "Filter modified files…",
                                style = typography.subhead,
                                color = colors.fgMuted,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        BasicTextField(
                            value = fileSearchQuery,
                            onValueChange = { fileSearchQuery = it },
                            singleLine = true,
                            textStyle = typography.subhead.copy(color = colors.fgPrimary),
                            cursorBrush = SolidColor(colors.primary),
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                    if (fileSearchQuery.isNotEmpty()) {
                        Box(
                            modifier = Modifier
                                .size(44.dp)
                                .clip(CircleShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    fileSearchQuery = ""
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Close,
                                contentDescription = "Clear file filter",
                                tint = colors.fgMuted,
                                modifier = Modifier.size(OrbitSpacing.iconSm),
                            )
                        }
                    } else {
                        Spacer(modifier = Modifier.width(OrbitSpacing.sm))
                    }
                }

                val toggleActionShape = RoundedCornerShape(OrbitSpacing.sm)
                Box(
                    modifier = Modifier
                        .heightIn(min = 44.dp)
                        .clip(toggleActionShape)
                        .background(colors.subtleFill, toggleActionShape)
                        .border(OrbitSpacing.hairline, colors.hairline, toggleActionShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            if (anyVisibleExpanded) {
                                visibleFiles.forEach { f -> collapsedOverrides[f.path] = true }
                            } else {
                                val allAutoCollapsed = visibleFiles.all { shouldAutoCollapseDiffFile(it.path) }
                                visibleFiles.forEach { f ->
                                    collapsedOverrides[f.path] = if (allAutoCollapsed) {
                                        false
                                    } else {
                                        shouldAutoCollapseDiffFile(f.path)
                                    }
                                }
                            }
                        }
                        .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = if (anyVisibleExpanded) "Collapse All" else "Expand Code",
                        style = typography.metaSemiBold,
                        color = colors.fgSecondary,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
            }

            if (queryMatchedFiles.isNotEmpty()) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .horizontalScroll(rememberScrollState())
                        .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs),
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    FileFilterChip(
                        label = "All (${queryMatchedFiles.size})",
                        additions = 0u,
                        deletions = 0u,
                        selected = selectedPath == null,
                        onClick = { onSelectPath(null) },
                    )
                    queryMatchedFiles.forEach { df ->
                        FileFilterChip(
                            label = OrbitFormatters.shortPath(df.path),
                            additions = df.additions,
                            deletions = df.deletions,
                            selected = selectedPath == df.path,
                            onClick = {
                                val nextPath = if (selectedPath == df.path) null else df.path
                                if (nextPath != null) {
                                    collapsedOverrides[nextPath] = false
                                }
                                onSelectPath(nextPath)
                            },
                        )
                    }
                }
            }
        }

        HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

        // Native Pull-to-Refresh + 120fps Flattened Virtualized Diff Hunk List
        PullToRefreshBox(
            isRefreshing = isRefreshing,
            onRefresh = onPullRefresh,
            state = pullRefreshState,
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth(),
            indicator = {
                PullToRefreshDefaults.Indicator(
                    state = pullRefreshState,
                    isRefreshing = isRefreshing,
                    containerColor = colors.elevated,
                    color = colors.primary,
                    modifier = Modifier.align(Alignment.TopCenter),
                )
            },
        ) {
            if (visibleFiles.isEmpty()) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .verticalScroll(rememberScrollState())
                        .padding(OrbitSpacing.xl),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = if (effectiveFiles.isNotEmpty()) {
                            "No matching modified files"
                        } else {
                            "Working tree clean"
                        },
                        style = typography.body,
                        color = colors.fgMuted,
                    )
                }
            } else {
                LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(OrbitSpacing.lg),
                ) {
                    visibleFiles.forEach { file ->
                        val autoCollapsed = shouldAutoCollapseDiffFile(file.path)
                        val isCollapsed = collapsedOverrides[file.path] ?: autoCollapsed
                        val showAll = showAllLinesMap[file.path] == true
                        val totalLines = file.lines.size
                        val displayedLines = if (isCollapsed) {
                            emptyList()
                        } else if (showAll || totalLines <= INITIAL_DIFF_LINE_LIMIT) {
                            file.lines
                        } else {
                            file.lines.subList(0, INITIAL_DIFF_LINE_LIMIT)
                        }
                        val hasShowAllRow = !isCollapsed && !showAll && totalLines > INITIAL_DIFF_LINE_LIMIT

                        // 1. Flattened File Header Item
                        item(
                            key = "hdr:${file.path}",
                            contentType = "diff_header",
                        ) {
                            DiffFileHeaderRow(
                                file = file,
                                isCollapsed = isCollapsed,
                                isAutoGenerated = autoCollapsed,
                                hasBodyBelow = displayedLines.isNotEmpty() || hasShowAllRow,
                                onToggleCollapse = {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    collapsedOverrides[file.path] = !isCollapsed
                                },
                            )
                        }

                        // 2. Flattened Virtualized Diff Line Items
                        if (displayedLines.isNotEmpty()) {
                            itemsIndexed(
                                items = displayedLines,
                                key = { idx, _ -> "ln:${file.path}:$idx" },
                                contentType = { _, _ -> "diff_line" },
                            ) { idx, ln ->
                                val isLastItemInCard = !hasShowAllRow && idx == displayedLines.lastIndex
                                VirtualizedDiffLineRow(
                                    line = ln,
                                    horizontalScrollState = sharedCodeScroll,
                                    isBottomRounded = isLastItemInCard,
                                    onTapQuote = {
                                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                        val lineNum = ln.newLine ?: ln.oldLine
                                        val lineLabel = lineNum?.let { "L$it" } ?: "hunk"
                                        onQuoteLine(file.path, lineLabel, ln.content)
                                    },
                                )
                            }
                        }

                        // 3. Per-File 120-Line Truncation Expansion Row
                        if (hasShowAllRow) {
                            item(
                                key = "more:${file.path}",
                                contentType = "diff_show_all",
                            ) {
                                ShowAllDiffLinesRow(
                                    totalLines = totalLines,
                                    onExpandAll = {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        showAllLinesMap[file.path] = true
                                    },
                                )
                            }
                        }

                        // 4. Inter-File Spacer
                        item(
                            key = "sp:${file.path}",
                            contentType = "diff_spacer",
                        ) {
                            Spacer(modifier = Modifier.height(OrbitSpacing.lg))
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun FileFilterChip(
    label: String,
    additions: UInt,
    deletions: UInt,
    selected: Boolean,
    onClick: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val shape = RoundedCornerShape(OrbitSpacing.sm)

    Row(
        modifier = Modifier
            .heightIn(min = 36.dp)
            .clip(shape)
            .background(if (selected) colors.subtleFill else Color.Transparent, shape)
            .border(
                OrbitSpacing.hairline,
                if (selected) colors.primary else colors.hairline,
                shape
            )
            .clickable { onClick() }
            .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
    ) {
        Text(
            text = label,
            style = typography.codeMedium,
            color = if (selected) colors.primary else colors.fgPrimary,
            maxLines = 1,
            softWrap = false,
        )
        if (additions > 0u) {
            Text("+${additions}", style = typography.metaMedium, color = colors.directAdded, maxLines = 1)
        }
        if (deletions > 0u) {
            Text("-${deletions}", style = typography.metaMedium, color = colors.dangerRemoved, maxLines = 1)
        }
    }
}

@Composable
private fun DiffFileHeaderRow(
    file: FfiDiffFile,
    isCollapsed: Boolean,
    isAutoGenerated: Boolean,
    hasBodyBelow: Boolean,
    onToggleCollapse: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val shape = if (hasBodyBelow) {
        RoundedCornerShape(topStart = OrbitSpacing.md, topEnd = OrbitSpacing.md)
    } else {
        RoundedCornerShape(OrbitSpacing.md)
    }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.code, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape)
            .clickable { onToggleCollapse() }
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = OrbitSpacing.touchTarget)
                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(
                modifier = Modifier.weight(1f),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                Icon(
                    imageVector = if (isCollapsed) Icons.Outlined.ChevronRight else Icons.Outlined.ExpandMore,
                    contentDescription = if (isCollapsed) "Expand file diff" else "Collapse file diff",
                    tint = colors.fgMuted,
                    modifier = Modifier.size(OrbitSpacing.iconSm),
                )
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = file.path,
                        style = typography.codeMedium,
                        color = colors.fgPrimary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (isCollapsed) {
                        val collapsedHint = if (isAutoGenerated) {
                            "Generated / lockfile collapsed · ${file.lines.size} lines"
                        } else {
                            "Collapsed · ${file.lines.size} lines"
                        }
                        Text(
                            text = collapsedHint,
                            style = typography.meta,
                            color = colors.fgMuted,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }

            Row(
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (file.additions > 0u) {
                    Text(
                        text = "+${file.additions}",
                        style = typography.metaMedium,
                        color = colors.directAdded,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
                if (file.deletions > 0u) {
                    Text(
                        text = "-${file.deletions}",
                        style = typography.metaMedium,
                        color = colors.dangerRemoved,
                        maxLines = 1,
                        softWrap = false,
                    )
                }
            }
        }
        if (hasBodyBelow) {
            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
        }
    }
}

@Composable
private fun VirtualizedDiffLineRow(
    line: FfiDiffLine,
    horizontalScrollState: ScrollState,
    isBottomRounded: Boolean,
    onTapQuote: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography

    val tintBg = when (line.kind) {
        FfiDiffLineKind.ADDED -> colors.diffAddedBg
        FfiDiffLineKind.REMOVED -> colors.diffRemovedBg
        FfiDiffLineKind.HUNK_HEADER -> colors.diffHunkBg
        else -> Color.Transparent
    }
    val fg = when (line.kind) {
        FfiDiffLineKind.ADDED -> colors.directAdded
        FfiDiffLineKind.REMOVED -> colors.dangerRemoved
        FfiDiffLineKind.HUNK_HEADER -> colors.primary
        FfiDiffLineKind.META -> colors.fgMuted
        FfiDiffLineKind.CONTEXT -> colors.fgPrimary
    }

    val shape = if (isBottomRounded) {
        RoundedCornerShape(bottomStart = OrbitSpacing.md, bottomEnd = OrbitSpacing.md)
    } else {
        RoundedCornerShape(0.dp)
    }

    val oldNumStr = line.oldLine?.toString()?.padStart(4, ' ') ?: "    "
    val newNumStr = line.newLine?.toString()?.padStart(4, ' ') ?: "    "
    val gutterSign = when (line.kind) {
        FfiDiffLineKind.ADDED -> "+"
        FfiDiffLineKind.REMOVED -> "-"
        FfiDiffLineKind.CONTEXT -> " "
        FfiDiffLineKind.HUNK_HEADER, FfiDiffLineKind.META -> ""
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.code, shape)
            .background(tintBg, shape)
            .border(OrbitSpacing.hairline, colors.hairline.copy(alpha = 0.4f), shape)
            .clickable(enabled = line.kind != FfiDiffLineKind.META) { onTapQuote() }
            .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs / 2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (line.kind != FfiDiffLineKind.META && line.kind != FfiDiffLineKind.HUNK_HEADER) {
            Text(
                text = "$oldNumStr $newNumStr │ $gutterSign ",
                style = typography.code,
                color = if (line.kind == FfiDiffLineKind.CONTEXT) colors.fgMuted else fg,
                maxLines = 1,
                softWrap = false,
            )
        }
        Box(
            modifier = Modifier
                .weight(1f)
                .horizontalScroll(horizontalScrollState),
        ) {
            Text(
                text = line.content,
                style = typography.code,
                color = fg,
                maxLines = 1,
                softWrap = false,
            )
        }
    }
}

@Composable
private fun ShowAllDiffLinesRow(
    totalLines: Int,
    onExpandAll: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val shape = RoundedCornerShape(bottomStart = OrbitSpacing.md, bottomEnd = OrbitSpacing.md)

    Box(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = OrbitSpacing.touchTarget)
            .clip(shape)
            .background(colors.code, shape)
            .border(OrbitSpacing.hairline, colors.hairline, shape)
            .clickable { onExpandAll() }
            .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = "Show all $totalLines lines",
            style = typography.subheadMedium,
            color = colors.primary,
            maxLines = 1,
            softWrap = false,
        )
    }
}
