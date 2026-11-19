package dev.orbit.android.ui.theme

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

private val OrbitMenuShape = RoundedCornerShape(14.dp)
private val OrbitMenuRowShape = RoundedCornerShape(10.dp)

@Composable
fun OrbitCompactDropdownMenu(
    expanded: Boolean,
    onDismissRequest: () -> Unit,
    width: Dp = 252.dp,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    val colors = OrbitTheme.colors
    DropdownMenu(
        expanded = expanded,
        onDismissRequest = onDismissRequest,
        modifier = modifier.width(width),
        shape = OrbitMenuShape,
        containerColor = colors.elevated,
        tonalElevation = 2.dp,
        shadowElevation = 6.dp,
        border = BorderStroke(OrbitSpacing.hairline, colors.hairline),
        content = content,
    )
}

@Composable
fun OrbitMenuSectionHeader(
    title: String,
    trailing: String? = null,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = title,
            style = typography.metaMedium,
            color = colors.fgMuted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f, fill = trailing == null),
        )
        if (!trailing.isNullOrBlank()) {
            Text(
                text = trailing,
                style = typography.meta,
                color = colors.fgMuted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
        }
    }
}

@Composable
fun OrbitMenuScrollContainer(
    maxHeight: Dp = 240.dp,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(max = maxHeight)
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(2.dp),
        content = content,
    )
}

@Composable
fun OrbitMenuOptionRow(
    title: String,
    selected: Boolean,
    onClick: () -> Unit,
    subtitle: String? = null,
    icon: ImageVector? = null,
    leading: (@Composable () -> Unit)? = null,
    trailingBadge: (@Composable () -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current

    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp)
            .clip(OrbitMenuRowShape)
            .background(if (selected) colors.subtleFill else Color.Transparent, OrbitMenuRowShape)
            .clickable {
                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                onClick()
            }
            .heightIn(min = if (subtitle.isNullOrBlank()) 38.dp else 46.dp)
            .padding(horizontal = 10.dp, vertical = if (subtitle.isNullOrBlank()) 6.dp else 8.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leading != null) {
            Box(
                modifier = Modifier.size(18.dp),
                contentAlignment = Alignment.Center,
            ) {
                leading()
            }
        } else if (icon != null) {
            Box(
                modifier = Modifier.size(18.dp),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = icon,
                    contentDescription = null,
                    tint = if (selected) colors.fgPrimary else colors.fgSecondary,
                    modifier = Modifier.size(16.dp),
                )
            }
        }

        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(1.dp),
        ) {
            Text(
                text = title,
                style = if (selected) typography.subheadSemiBold else typography.subheadMedium,
                color = colors.fgPrimary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (!subtitle.isNullOrBlank()) {
                Text(
                    text = subtitle,
                    style = typography.meta,
                    color = colors.fgMuted,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }

        if (trailingBadge != null) {
            trailingBadge()
        }

        if (selected) {
            Icon(
                imageVector = Icons.Outlined.Check,
                contentDescription = null,
                tint = colors.primary,
                modifier = Modifier.size(16.dp),
            )
        }
    }
}

@Composable
fun OrbitMenuActionRow(
    title: String,
    icon: ImageVector,
    onClick: () -> Unit,
    isDestructive: Boolean = false,
    modifier: Modifier = Modifier,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val fg = if (isDestructive) colors.dangerRemoved else colors.fgPrimary
    val iconTint = if (isDestructive) colors.dangerRemoved else colors.fgSecondary

    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp)
            .clip(OrbitMenuRowShape)
            .clickable {
                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                onClick()
            }
            .heightIn(min = 38.dp)
            .padding(horizontal = 10.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier.size(18.dp),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = iconTint,
                modifier = Modifier.size(16.dp),
            )
        }
        Text(
            text = title,
            style = typography.subheadMedium,
            color = fg,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
fun OrbitMenuDivider(modifier: Modifier = Modifier) {
    HorizontalDivider(
        thickness = OrbitSpacing.hairline,
        color = OrbitTheme.colors.hairline,
        modifier = modifier.padding(horizontal = 16.dp, vertical = 4.dp),
    )
}
