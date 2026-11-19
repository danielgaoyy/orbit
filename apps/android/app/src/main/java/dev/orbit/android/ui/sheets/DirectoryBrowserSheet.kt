package dev.orbit.android.ui.sheets

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.outlined.AccountTree
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.PushPin
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiClientProject
import uniffi.orbit_ffi_uniffi.FfiDirectoryListing
import uniffi.orbit_ffi_uniffi.FfiDiscoveredProject

private data class WorkspaceBreadcrumb(
    val id: String,
    val label: String,
    val targetPath: String,
)

private fun buildBreadcrumbs(rawPath: String): List<WorkspaceBreadcrumb> {
    val trimmed = rawPath.trim()
    if (trimmed.isEmpty() || trimmed == "~") {
        return listOf(WorkspaceBreadcrumb("~", "~", "~"))
    }
    if (trimmed == "/") {
        return listOf(WorkspaceBreadcrumb("/", "/", "/"))
    }
    val isAbsolute = trimmed.startsWith("/")
    val parts = trimmed.split('/').filter { it.isNotEmpty() }
    val out = mutableListOf<WorkspaceBreadcrumb>()
    if (isAbsolute) {
        if (parts.size >= 2 && (parts[0] == "Users" || parts[0] == "home")) {
            val homePath = "/${parts[0]}/${parts[1]}"
            out += WorkspaceBreadcrumb(homePath, "~", homePath)
            var running = homePath
            parts.drop(2).forEach { part ->
                running = "$running/$part"
                out += WorkspaceBreadcrumb(running, part, running)
            }
        } else {
            out += WorkspaceBreadcrumb("/", "/", "/")
            var running = ""
            parts.forEach { part ->
                running = "$running/$part"
                out += WorkspaceBreadcrumb(running, part, running)
            }
        }
    } else {
        var running = ""
        parts.forEachIndexed { index, part ->
            running = if (index == 0) part else "$running/$part"
            out += WorkspaceBreadcrumb(running, part, running)
        }
    }
    return out
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DirectoryBrowserSheet(
    initialPath: String,
    hostDisplayName: String?,
    isMultiHost: Boolean,
    listing: FfiDirectoryListing?,
    pinnedProjects: List<FfiClientProject>,
    discoveredProjects: List<FfiDiscoveredProject>,
    onBrowsePath: (String?) -> Unit,
    onSelectDirectory: (String) -> Unit,
    onPinProject: (name: String, directory: String) -> Unit,
    onUnpinProject: (projectId: String) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    val groupedBg = if (colors.isDark) colors.canvas else colors.drawer
    val cardShape = RoundedCornerShape(14.dp)

    val currentPath = (listing?.currentPath ?: initialPath).ifBlank { "~" }
    val currentFolderName = OrbitFormatters.shortPath(currentPath)

    var isEditingPath by remember { mutableStateOf(false) }
    var manualPathInput by remember(currentPath) { mutableStateOf(currentPath) }
    var filterQuery by remember { mutableStateOf("") }

    val normalizedFilter = remember(filterQuery) { filterQuery.trim().lowercase() }
    val breadcrumbs = remember(currentPath) { buildBreadcrumbs(currentPath) }
    val breadcrumbScroll = rememberScrollState()

    val filteredEntries = remember(listing, normalizedFilter) {
        val all = listing?.entries.orEmpty()
        if (normalizedFilter.isEmpty()) all
        else all.filter { it.name.lowercase().contains(normalizedFilter) }
    }

    val filteredRepos = remember(discoveredProjects, normalizedFilter) {
        val base = discoveredProjects.filter { !it.path.contains("/worktrees/") }
        if (normalizedFilter.isEmpty()) base
        else base.filter {
            it.name.lowercase().contains(normalizedFilter) ||
                it.path.lowercase().contains(normalizedFilter) ||
                (it.gitBranch?.lowercase()?.contains(normalizedFilter) == true)
        }
    }

    val pinnedMatch = remember(pinnedProjects, currentPath) {
        pinnedProjects.firstOrNull { proj ->
            proj.directories.any { OrbitFormatters.pathsEquivalent(it, currentPath) }
        }
    }

    LaunchedEffect(initialPath) {
        onBrowsePath(initialPath.takeIf { it.isNotBlank() && it != "~" })
    }

    LaunchedEffect(currentPath) {
        breadcrumbScroll.animateScrollTo(breadcrumbScroll.maxValue)
    }

    fun submitManualPath() {
        val trimmed = manualPathInput.trim()
        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
        filterQuery = ""
        onBrowsePath(trimmed.ifEmpty { null })
        isEditingPath = false
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = groupedBg,
        contentColor = colors.fgPrimary,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.86f)
                .imePadding()
                .navigationBarsPadding(),
        ) {
            // Top Header
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg)
                    .padding(bottom = OrbitSpacing.sm),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = "Choose Workspace",
                        style = typography.title,
                        color = colors.fgPrimary,
                        maxLines = 1,
                    )
                    if (isMultiHost && !hostDisplayName.isNullOrBlank()) {
                        Text(
                            text = hostDisplayName,
                            style = typography.meta,
                            color = colors.fgMuted,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                Box(
                    modifier = Modifier
                        .size(36.dp)
                        .clip(CircleShape)
                        .clickable { onDismiss() },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Close,
                        contentDescription = "Close",
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(18.dp),
                    )
                }
            }

            // Breadcrumb Trail or Manual Path Jump
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrbitSpacing.lg)
                    .padding(bottom = OrbitSpacing.md),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                if (isEditingPath) {
                    val fieldShape = RoundedCornerShape(10.dp)
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(40.dp)
                            .background(colors.elevated, fieldShape)
                            .border(OrbitSpacing.hairline, colors.hairline, fieldShape)
                            .padding(horizontal = OrbitSpacing.md),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.Folder,
                            contentDescription = null,
                            tint = colors.fgMuted,
                            modifier = Modifier.size(15.dp),
                        )
                        Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                            if (manualPathInput.isEmpty()) {
                                Text(
                                    text = "/path/to/workspace",
                                    style = typography.code,
                                    color = colors.fgMuted,
                                    maxLines = 1,
                                )
                            }
                            BasicTextField(
                                value = manualPathInput,
                                onValueChange = { manualPathInput = it },
                                singleLine = true,
                                textStyle = typography.code.copy(color = colors.fgPrimary),
                                cursorBrush = SolidColor(colors.primary),
                                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Go),
                                keyboardActions = KeyboardActions(onGo = { submitManualPath() }),
                                modifier = Modifier.fillMaxWidth(),
                            )
                        }
                        Text(
                            text = "Go",
                            style = typography.metaSemiBold,
                            color = colors.primary,
                            modifier = Modifier
                                .clip(RoundedCornerShape(6.dp))
                                .clickable { submitManualPath() }
                                .padding(horizontal = 6.dp, vertical = 4.dp),
                        )
                        Box(
                            modifier = Modifier
                                .size(28.dp)
                                .clip(CircleShape)
                                .clickable {
                                    isEditingPath = false
                                    manualPathInput = currentPath
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Close,
                                contentDescription = "Cancel path editing",
                                tint = colors.fgMuted,
                                modifier = Modifier.size(15.dp),
                            )
                        }
                    }
                } else {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                    ) {
                        listing?.parentPath?.let { parent ->
                            Box(
                                modifier = Modifier
                                    .size(32.dp)
                                    .clip(CircleShape)
                                    .background(colors.elevated, CircleShape)
                                    .border(OrbitSpacing.hairline, colors.hairline, CircleShape)
                                    .clickable {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        filterQuery = ""
                                        onBrowsePath(parent)
                                    },
                                contentAlignment = Alignment.Center,
                            ) {
                                Icon(
                                    imageVector = Icons.AutoMirrored.Outlined.KeyboardArrowLeft,
                                    contentDescription = "Parent directory",
                                    tint = colors.fgSecondary,
                                    modifier = Modifier.size(16.dp),
                                )
                            }
                        }

                        Row(
                            modifier = Modifier
                                .weight(1f)
                                .horizontalScroll(breadcrumbScroll),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(2.dp),
                        ) {
                            breadcrumbs.forEachIndexed { idx, seg ->
                                val isLast = idx == breadcrumbs.lastIndex
                                if (idx > 0) {
                                    Icon(
                                        imageVector = Icons.AutoMirrored.Outlined.KeyboardArrowRight,
                                        contentDescription = null,
                                        tint = colors.fgMuted,
                                        modifier = Modifier.size(13.dp),
                                    )
                                }
                                Box(
                                    modifier = Modifier
                                        .clip(CircleShape)
                                        .background(if (isLast) colors.elevated else Color.Transparent, CircleShape)
                                        .then(
                                            if (isLast) {
                                                Modifier.border(OrbitSpacing.hairline, colors.hairline, CircleShape)
                                            } else {
                                                Modifier
                                            }
                                        )
                                        .clickable(enabled = !isLast) {
                                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                            filterQuery = ""
                                            onBrowsePath(seg.targetPath)
                                        }
                                        .padding(horizontal = 9.dp, vertical = 5.dp),
                                ) {
                                    Text(
                                        text = seg.label,
                                        style = if (isLast) typography.metaSemiBold else typography.metaMedium,
                                        color = if (isLast) colors.fgPrimary else colors.fgSecondary,
                                        maxLines = 1,
                                    )
                                }
                            }
                        }

                        Box(
                            modifier = Modifier
                                .size(32.dp)
                                .clip(CircleShape)
                                .background(colors.elevated, CircleShape)
                                .border(OrbitSpacing.hairline, colors.hairline, CircleShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    manualPathInput = currentPath
                                    isEditingPath = true
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Edit,
                                contentDescription = "Enter path manually",
                                tint = colors.fgSecondary,
                                modifier = Modifier.size(14.dp),
                            )
                        }
                    }
                }

                // Quiet single-line filter input
                val filterShape = RoundedCornerShape(10.dp)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(38.dp)
                        .background(if (colors.isDark) colors.elevated else colors.subtleFill, filterShape)
                        .padding(horizontal = OrbitSpacing.md),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Search,
                        contentDescription = null,
                        tint = colors.fgMuted,
                        modifier = Modifier.size(16.dp),
                    )
                    Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                        if (filterQuery.isEmpty()) {
                            Text(
                                text = "Filter folders or repositories…",
                                style = typography.subhead,
                                color = colors.fgMuted,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        BasicTextField(
                            value = filterQuery,
                            onValueChange = { filterQuery = it },
                            singleLine = true,
                            textStyle = typography.subhead.copy(color = colors.fgPrimary),
                            cursorBrush = SolidColor(colors.primary),
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                    if (filterQuery.isNotEmpty()) {
                        Box(
                            modifier = Modifier
                                .size(24.dp)
                                .clip(CircleShape)
                                .clickable { filterQuery = "" },
                            contentAlignment = Alignment.Center,
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Close,
                                contentDescription = "Clear filter",
                                tint = colors.fgMuted,
                                modifier = Modifier.size(14.dp),
                            )
                        }
                    }
                }
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            // Main Scrollable Grouped Lists (Folders + Discovered Repositories)
            Column(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.lg),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xl),
            ) {
                // Section 1: Folders
                Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs + 2.dp)) {
                    Text(
                        text = "Folders",
                        style = typography.metaMedium,
                        color = colors.fgMuted,
                        modifier = Modifier.padding(horizontal = OrbitSpacing.xs),
                    )
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(cardShape)
                            .background(colors.elevated, cardShape)
                            .border(OrbitSpacing.hairline, colors.hairline, cardShape),
                    ) {
                        if (listing == null) {
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .heightIn(min = 48.dp)
                                    .padding(horizontal = OrbitSpacing.md),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                            ) {
                                CircularProgressIndicator(
                                    modifier = Modifier.size(16.dp),
                                    strokeWidth = 2.dp,
                                    color = colors.fgSecondary,
                                )
                                Text(
                                    text = "Loading directory…",
                                    style = typography.subhead,
                                    color = colors.fgSecondary,
                                )
                            }
                        } else if (filteredEntries.isEmpty()) {
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .heightIn(min = 48.dp)
                                    .padding(horizontal = OrbitSpacing.md),
                                contentAlignment = Alignment.CenterStart,
                            ) {
                                Text(
                                    text = if (normalizedFilter.isEmpty()) {
                                        "No subfolders in this directory"
                                    } else {
                                        "No matching folders"
                                    },
                                    style = typography.subhead,
                                    color = colors.fgMuted,
                                )
                            }
                        } else {
                            filteredEntries.forEachIndexed { idx, entry ->
                                Row(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .heightIn(min = OrbitSpacing.touchTarget)
                                        .clickable {
                                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                            filterQuery = ""
                                            onBrowsePath(entry.path)
                                        }
                                        .padding(horizontal = OrbitSpacing.md, vertical = 8.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                                ) {
                                    Box(
                                        modifier = Modifier.size(20.dp),
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        Icon(
                                            imageVector = if (entry.isGitRepo) {
                                                Icons.Outlined.AccountTree
                                            } else {
                                                Icons.Outlined.Folder
                                            },
                                            contentDescription = null,
                                            tint = if (entry.isGitRepo) colors.primary else colors.fgSecondary,
                                            modifier = Modifier.size(16.dp),
                                        )
                                    }
                                    Text(
                                        text = entry.name,
                                        style = typography.subheadMedium,
                                        color = colors.fgPrimary,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                        modifier = Modifier.weight(1f, fill = false),
                                    )
                                    if (entry.isGitRepo) {
                                        Box(
                                            modifier = Modifier
                                                .background(colors.subtleFill, CircleShape)
                                                .padding(horizontal = 6.dp, vertical = 1.dp),
                                        ) {
                                            Text(
                                                text = "git",
                                                style = typography.meta,
                                                color = colors.fgSecondary,
                                            )
                                        }
                                    }
                                    Spacer(modifier = Modifier.weight(1f))
                                    Icon(
                                        imageVector = Icons.AutoMirrored.Outlined.KeyboardArrowRight,
                                        contentDescription = null,
                                        tint = colors.fgMuted,
                                        modifier = Modifier.size(16.dp),
                                    )
                                }
                                if (idx < filteredEntries.lastIndex) {
                                    HorizontalDivider(
                                        thickness = OrbitSpacing.hairline,
                                        color = colors.hairline,
                                        modifier = Modifier.padding(start = 42.dp),
                                    )
                                }
                            }
                        }
                    }
                }

                // Section 2: Discovered Repositories (1-Tap Direct Open)
                if (filteredRepos.isNotEmpty()) {
                    Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs + 2.dp)) {
                        Text(
                            text = "Repositories",
                            style = typography.metaMedium,
                            color = colors.fgMuted,
                            modifier = Modifier.padding(horizontal = OrbitSpacing.xs),
                        )
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(cardShape)
                                .background(colors.elevated, cardShape)
                                .border(OrbitSpacing.hairline, colors.hairline, cardShape),
                        ) {
                            filteredRepos.forEachIndexed { idx, repo ->
                                val isCurrent = OrbitFormatters.pathsEquivalent(repo.path, currentPath)
                                Row(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .heightIn(min = 52.dp)
                                        .clickable {
                                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                            onSelectDirectory(repo.path)
                                            onDismiss()
                                        }
                                        .padding(horizontal = OrbitSpacing.md, vertical = 9.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                                ) {
                                    Box(
                                        modifier = Modifier.size(20.dp),
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        Icon(
                                            imageVector = Icons.Outlined.AccountTree,
                                            contentDescription = null,
                                            tint = colors.primary,
                                            modifier = Modifier.size(16.dp),
                                        )
                                    }
                                    Column(
                                        modifier = Modifier.weight(1f),
                                        verticalArrangement = Arrangement.spacedBy(1.dp),
                                    ) {
                                        Text(
                                            text = repo.name,
                                            style = if (isCurrent) typography.subheadSemiBold else typography.subheadMedium,
                                            color = colors.fgPrimary,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                        Text(
                                            text = OrbitFormatters.compactPath(repo.path),
                                            style = typography.code,
                                            color = colors.fgMuted,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                    repo.gitBranch?.takeIf { it.isNotBlank() }?.let { branch ->
                                        Text(
                                            text = branch,
                                            style = typography.meta,
                                            color = colors.fgSecondary,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                            modifier = Modifier.widthIn(max = 104.dp),
                                        )
                                    }
                                }
                                if (idx < filteredRepos.lastIndex) {
                                    HorizontalDivider(
                                        thickness = OrbitSpacing.hairline,
                                        color = colors.hairline,
                                        modifier = Modifier.padding(start = 42.dp),
                                    )
                                }
                            }
                        }
                    }
                }
            }

            // Sticky Bottom Thumb-Zone Action Dock
            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(groupedBg)
                    .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.md),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Folder,
                        contentDescription = null,
                        tint = colors.fgMuted,
                        modifier = Modifier.size(13.dp),
                    )
                    Text(
                        text = OrbitFormatters.compactPath(currentPath),
                        style = typography.code,
                        color = colors.fgSecondary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    val isPinned = pinnedMatch != null
                    val btnShape = RoundedCornerShape(12.dp)

                    Row(
                        modifier = Modifier
                            .height(OrbitSpacing.touchTarget)
                            .clip(btnShape)
                            .background(colors.elevated, btnShape)
                            .border(OrbitSpacing.hairline, colors.hairline, btnShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                if (pinnedMatch != null) {
                                    onUnpinProject(pinnedMatch.projectId)
                                } else {
                                    onPinProject(currentFolderName, currentPath)
                                }
                            }
                            .padding(horizontal = OrbitSpacing.lg),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Icon(
                            imageVector = if (isPinned) Icons.Filled.PushPin else Icons.Outlined.PushPin,
                            contentDescription = null,
                            tint = if (isPinned) colors.primary else colors.fgPrimary,
                            modifier = Modifier.size(15.dp),
                        )
                        Text(
                            text = if (isPinned) "Pinned" else "Pin",
                            style = typography.subheadMedium,
                            color = if (isPinned) colors.primary else colors.fgPrimary,
                        )
                    }

                    Box(
                        modifier = Modifier
                            .weight(1f)
                            .height(OrbitSpacing.touchTarget)
                            .clip(btnShape)
                            .background(colors.primary, btnShape)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onSelectDirectory(currentPath)
                                onDismiss()
                            }
                            .padding(horizontal = OrbitSpacing.md),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = "Open $currentFolderName",
                            style = typography.subheadSemiBold,
                            color = colors.onPrimary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        }
    }
}
