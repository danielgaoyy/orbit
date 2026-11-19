package dev.orbit.android.ui.composer

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.orbit.android.store.OrbitFormatters
import dev.orbit.android.ui.theme.OrbitCompactDropdownMenu
import dev.orbit.android.ui.theme.OrbitMenuOptionRow
import dev.orbit.android.ui.theme.OrbitMenuSectionHeader
import dev.orbit.android.ui.theme.OrbitProviderIcon
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiHostDetail
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiProviderModelSpec
import uniffi.orbit_ffi_uniffi.FfiReasoningEffort

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ModelSelectorSheet(
    currentProvider: FfiProviderKind,
    currentModel: String?,
    currentEffort: FfiReasoningEffort?,
    hostDetail: FfiHostDetail?,
    isNewSession: Boolean = true,
    onSwitchProvider: (FfiProviderKind) -> Unit = {},
    onSelectEffort: (FfiReasoningEffort) -> Unit = {},
    onSelect: (provider: FfiProviderKind, model: String?, effort: FfiReasoningEffort?) -> Unit,
    onRefreshCatalogs: () -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    var selectedProvider by remember(currentProvider) { mutableStateOf(currentProvider) }
    val catalog = remember(hostDetail, selectedProvider) {
        OrbitFormatters.catalogFor(hostDetail, selectedProvider)
    }
    var selectedModelId by remember(selectedProvider, currentModel, catalog) {
        mutableStateOf(
            if (selectedProvider == currentProvider) {
                currentModel
                    ?: catalog.configuredDefaultModel
                    ?: catalog.models.firstOrNull { it.isDefault }?.id
                    ?: catalog.models.firstOrNull()?.id
            } else {
                catalog.configuredDefaultModel
                    ?: catalog.models.firstOrNull { it.isDefault }?.id
                    ?: catalog.models.firstOrNull()?.id
            }
        )
    }
    val activeModelSpec: FfiProviderModelSpec? = remember(catalog, selectedModelId) {
        catalog.models.firstOrNull { it.id == selectedModelId }
            ?: catalog.models.firstOrNull { it.isDefault }
            ?: catalog.models.firstOrNull()
    }
    var selectedEffort by remember(selectedProvider, activeModelSpec, currentEffort) {
        mutableStateOf(
            OrbitFormatters.resolveEffort(
                spec = activeModelSpec,
                desired = if (selectedProvider == currentProvider) currentEffort else null,
                catalogDefault = catalog.configuredDefaultEffort,
            )
        )
    }

    val detectedProviders = remember(hostDetail, currentProvider) {
        hostDetail?.host?.availableProviders
            ?.distinct()
            ?.takeIf { it.isNotEmpty() }
            ?: listOf(currentProvider)
    }
    val thinkingOptions = activeModelSpec?.thinkingOptions.orEmpty()
    var thinkingMenuExpanded by remember { mutableStateOf(false) }

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
                .padding(horizontal = OrbitSpacing.lg)
                .padding(bottom = OrbitSpacing.lg),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
        ) {
            // Compact Header
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Model",
                    style = typography.title,
                    color = colors.fgPrimary,
                )
                IconButton(
                    onClick = {
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        onRefreshCatalogs()
                    },
                    modifier = Modifier.size(36.dp),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Refresh,
                        contentDescription = "Refresh model catalogs",
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconMd),
                    )
                }
            }

            // Segmented Provider Switcher (New session & >1 CLI detected)
            if (isNewSession && detectedProviders.size > 1) {
                SingleChoiceSegmentedButtonRow(
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    detectedProviders.forEachIndexed { index, provider ->
                        val active = provider == selectedProvider
                        SegmentedButton(
                            selected = active,
                            onClick = {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                selectedProvider = provider
                                onSwitchProvider(provider)
                            },
                            shape = SegmentedButtonDefaults.itemShape(
                                index = index,
                                count = detectedProviders.size,
                            ),
                            colors = SegmentedButtonDefaults.colors(
                                activeContainerColor = colors.primary,
                                activeContentColor = colors.onPrimary,
                                inactiveContainerColor = colors.subtleFill,
                                inactiveContentColor = colors.fgSecondary,
                                activeBorderColor = colors.primary,
                                inactiveBorderColor = colors.hairline,
                            ),
                            icon = {},
                        ) {
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                OrbitProviderIcon(
                                    provider = provider,
                                    size = 15.dp,
                                )
                                Text(
                                    text = OrbitFormatters.providerShortLabel(provider),
                                    style = typography.subheadMedium,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                    }
                }
            }

            // Compact Model List
            val listShape = RoundedCornerShape(14.dp)
            LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(max = 320.dp)
                    .clip(listShape)
                    .background(colors.canvas, listShape)
                    .border(OrbitSpacing.hairline, colors.hairline, listShape),
                contentPadding = PaddingValues(vertical = OrbitSpacing.xs),
            ) {
                items(
                    items = catalog.models,
                    key = { "${selectedProvider.name}:${it.id}" },
                ) { spec ->
                    val isSelected = spec.id == (activeModelSpec?.id ?: selectedModelId)
                    val displayLabel = remember(selectedProvider, spec.label) {
                        if (selectedProvider == FfiProviderKind.CLAUDE_CODE) {
                            spec.label.removePrefix("Claude ").trim().ifEmpty { spec.label }
                        } else {
                            spec.label
                        }
                    }
                    val subLabel = remember(spec) {
                        spec.description.trim().takeIf { it.isNotEmpty() }
                            ?: spec.defaultEffort?.let { "Default · ${OrbitFormatters.effortLabel(it)}" }
                            ?: if (spec.isDefault) "Default" else null
                    }
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .background(if (isSelected) colors.subtleFill else Color.Transparent)
                            .clickable {
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                selectedModelId = spec.id
                                val nextEffort = OrbitFormatters.resolveEffort(
                                    spec = spec,
                                    desired = selectedEffort,
                                    catalogDefault = catalog.configuredDefaultEffort,
                                )
                                selectedEffort = nextEffort
                                onSelect(selectedProvider, spec.id, nextEffort)
                                onDismiss()
                            }
                            .padding(horizontal = OrbitSpacing.md, vertical = 10.dp),
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(
                            modifier = Modifier.weight(1f),
                            verticalArrangement = Arrangement.spacedBy(2.dp),
                        ) {
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text(
                                    text = displayLabel,
                                    style = if (isSelected) typography.bodySemiBold else typography.body,
                                    color = colors.fgPrimary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                if (spec.id == catalog.configuredDefaultModel || (catalog.configuredDefaultModel == null && spec.isDefault)) {
                                    Text(
                                        text = "Default",
                                        style = typography.meta,
                                        color = colors.fgMuted,
                                        modifier = Modifier
                                            .background(colors.subtleFill, RoundedCornerShape(6.dp))
                                            .padding(horizontal = 6.dp, vertical = 1.dp),
                                    )
                                }
                            }
                            if (subLabel != null) {
                                Text(
                                    text = subLabel,
                                    style = typography.meta,
                                    color = colors.fgSecondary,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                        if (isSelected) {
                            Icon(
                                imageVector = Icons.Outlined.Check,
                                contentDescription = "Selected",
                                tint = colors.primary,
                                modifier = Modifier.size(OrbitSpacing.iconMd),
                            )
                        }
                    }
                }
            }

            // Pinned Bottom Thinking Effort Controls
            if (thinkingOptions.isNotEmpty()) {
                HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = "Thinking",
                            style = typography.subheadMedium,
                            color = colors.fgSecondary,
                        )
                        selectedEffort?.let { eff ->
                            Text(
                                text = OrbitFormatters.effortDescription(eff),
                                style = typography.meta,
                                color = colors.fgMuted,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }

                    if (thinkingOptions.size <= 5) {
                        SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
                            thinkingOptions.forEachIndexed { index, opt ->
                                val active = opt.effort == selectedEffort
                                SegmentedButton(
                                    selected = active,
                                    onClick = {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        selectedEffort = opt.effort
                                        onSelectEffort(opt.effort)
                                        onSelect(selectedProvider, activeModelSpec?.id, opt.effort)
                                    },
                                    shape = SegmentedButtonDefaults.itemShape(
                                        index = index,
                                        count = thinkingOptions.size,
                                    ),
                                    colors = SegmentedButtonDefaults.colors(
                                        activeContainerColor = colors.primary,
                                        activeContentColor = colors.onPrimary,
                                        inactiveContainerColor = colors.subtleFill,
                                        inactiveContentColor = colors.fgSecondary,
                                        activeBorderColor = colors.primary,
                                        inactiveBorderColor = colors.hairline,
                                    ),
                                    icon = {},
                                ) {
                                    Text(
                                        text = OrbitFormatters.compactEffortLabel(opt.effort),
                                        style = typography.metaMedium,
                                        maxLines = 1,
                                        overflow = TextOverflow.Clip,
                                    )
                                }
                            }
                        }
                    } else {
                        val triggerShape = RoundedCornerShape(10.dp)
                        Box(modifier = Modifier.fillMaxWidth()) {
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clip(triggerShape)
                                    .background(colors.subtleFill, triggerShape)
                                    .border(OrbitSpacing.hairline, colors.hairline, triggerShape)
                                    .clickable { thinkingMenuExpanded = true }
                                    .padding(horizontal = OrbitSpacing.md, vertical = 10.dp),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Text(
                                    text = selectedEffort?.let { OrbitFormatters.effortLabel(it) } ?: "Auto",
                                    style = typography.subheadMedium,
                                    color = colors.fgPrimary,
                                )
                                Icon(
                                    imageVector = Icons.Outlined.KeyboardArrowDown,
                                    contentDescription = null,
                                    tint = colors.fgSecondary,
                                    modifier = Modifier.size(OrbitSpacing.iconSm),
                                )
                            }
                            OrbitCompactDropdownMenu(
                                expanded = thinkingMenuExpanded,
                                onDismissRequest = { thinkingMenuExpanded = false },
                                width = 264.dp,
                            ) {
                                OrbitMenuSectionHeader(title = "Reasoning Effort")
                                thinkingOptions.forEach { opt ->
                                    val isCurrent = opt.effort == selectedEffort
                                    OrbitMenuOptionRow(
                                        title = OrbitFormatters.effortLabel(opt.effort),
                                        subtitle = opt.description?.takeIf { it.isNotBlank() },
                                        selected = isCurrent,
                                        onClick = {
                                            thinkingMenuExpanded = false
                                            selectedEffort = opt.effort
                                            onSelectEffort(opt.effort)
                                            onSelect(selectedProvider, activeModelSpec?.id, opt.effort)
                                        },
                                    )
                                }
                            }
                        }
                    }
                }
            } else {
                Spacer(modifier = Modifier.size(OrbitSpacing.xs))
            }
        }
    }
}
