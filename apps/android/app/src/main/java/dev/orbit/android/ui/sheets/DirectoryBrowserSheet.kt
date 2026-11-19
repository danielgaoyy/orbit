package dev.orbit.android.ui.sheets

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ArrowUpward
import androidx.compose.material.icons.outlined.BookmarkBorder
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiDirectoryListing

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DirectoryBrowserSheet(
    initialPath: String,
    listing: FfiDirectoryListing?,
    onBrowsePath: (String?) -> Unit,
    onSelectDirectory: (String) -> Unit,
    onPinProject: (name: String, directory: String) -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    LaunchedEffect(initialPath) {
        onBrowsePath(initialPath.takeIf { it.isNotBlank() && it != "~" })
    }

    val currentPath = listing?.currentPath ?: initialPath

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = colors.elevated,
        contentColor = colors.fgPrimary,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .navigationBarsPadding()
                .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.sm),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
        ) {
            Text(
                text = "Browse Remote Directory",
                style = typography.title,
                color = colors.fgPrimary,
            )

            // Current Path Bar + Up Button
            val pathShape = RoundedCornerShape(OrbitSpacing.md)
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(colors.code, pathShape)
                    .border(OrbitSpacing.hairline, colors.hairline, pathShape)
                    .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                listing?.parentPath?.let { parent ->
                    Box(
                        modifier = Modifier
                            .size(36.dp)
                            .clip(RoundedCornerShape(OrbitSpacing.sm))
                            .background(colors.subtleFill, RoundedCornerShape(OrbitSpacing.sm))
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                onBrowsePath(parent)
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Outlined.ArrowUpward,
                            contentDescription = "Parent directory",
                            tint = colors.fgPrimary,
                            modifier = Modifier.size(OrbitSpacing.iconSm),
                        )
                    }
                }
                Text(
                    text = currentPath,
                    style = typography.codeMedium,
                    color = colors.fgPrimary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            // Directory Entries
            LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 180.dp, max = 340.dp),
                verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
            ) {
                val entries = listing?.entries.orEmpty()
                if (entries.isEmpty()) {
                    item {
                        Text(
                            text = if (listing == null) "Loading remote directory..." else "No subdirectories.",
                            style = typography.subhead,
                            color = colors.fgMuted,
                            modifier = Modifier.padding(vertical = OrbitSpacing.md),
                        )
                    }
                } else {
                    items(entries, key = { it.path }) { entry ->
                        val rowShape = RoundedCornerShape(OrbitSpacing.md)
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = OrbitSpacing.touchTarget)
                                .clip(rowShape)
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    onBrowsePath(entry.path)
                                }
                                .padding(horizontal = OrbitSpacing.sm, vertical = OrbitSpacing.xs),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
                        ) {
                            Icon(
                                imageVector = Icons.Outlined.Folder,
                                contentDescription = null,
                                tint = if (entry.isGitRepo) colors.primary else colors.fgSecondary,
                                modifier = Modifier.size(OrbitSpacing.iconMd),
                            )
                            Text(
                                text = entry.name,
                                style = typography.bodyMedium,
                                color = colors.fgPrimary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            if (entry.isGitRepo) {
                                val gitShape = RoundedCornerShape(OrbitSpacing.xs)
                                Box(
                                    modifier = Modifier
                                        .background(colors.subtleFill, gitShape)
                                        .border(OrbitSpacing.hairline, colors.hairline, gitShape)
                                        .padding(horizontal = OrbitSpacing.sm, vertical = 2.dp)
                                ) {
                                    Text(
                                        text = "Git",
                                        style = typography.metaMedium,
                                        color = colors.primary,
                                    )
                                }
                            }
                        }
                    }
                }
            }

            // Action Buttons: Pin Project + Open Here
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
            ) {
                val secShape = RoundedCornerShape(OrbitSpacing.md)
                Row(
                    modifier = Modifier
                        .heightIn(min = OrbitSpacing.touchTarget)
                        .clip(secShape)
                        .background(colors.subtleFill, secShape)
                        .border(OrbitSpacing.hairline, colors.hairline, secShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onPinProject(OrbitFormatters.shortPath(currentPath), currentPath)
                        }
                        .padding(horizontal = OrbitSpacing.lg),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.BookmarkBorder,
                        contentDescription = null,
                        tint = colors.fgPrimary,
                        modifier = Modifier.size(OrbitSpacing.iconSm),
                    )
                    Text(
                        text = "Pin",
                        style = typography.subheadSemiBold,
                        color = colors.fgPrimary,
                    )
                }

                val priShape = RoundedCornerShape(OrbitSpacing.md)
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = OrbitSpacing.touchTarget)
                        .clip(priShape)
                        .background(colors.primary, priShape)
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onSelectDirectory(currentPath)
                            onDismiss()
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "Open ${OrbitFormatters.shortPath(currentPath)}",
                        style = typography.bodySemiBold,
                        color = Color.White,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }

            Spacer(modifier = Modifier.height(OrbitSpacing.xs))
        }
    }
}
