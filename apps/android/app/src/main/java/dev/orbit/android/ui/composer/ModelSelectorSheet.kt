package dev.orbit.android.ui.composer

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
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
import dev.orbit.android.ui.theme.OrbitSpacing
import dev.orbit.android.ui.theme.OrbitTheme
import uniffi.orbit_ffi_uniffi.FfiHostDetail
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiReasoningEffort

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ModelSelectorSheet(
    currentProvider: FfiProviderKind,
    currentModel: String?,
    currentEffort: FfiReasoningEffort?,
    hostDetail: FfiHostDetail?,
    onSelect: (provider: FfiProviderKind, model: String?, effort: FfiReasoningEffort?) -> Unit,
    onRefreshCatalogs: () -> Unit,
    onDismiss: () -> Unit,
) {
    val colors = OrbitTheme.colors
    val typography = OrbitTheme.typography
    val haptics = LocalHapticFeedback.current
    // Keep ModelSelectorSheet compact/adaptive
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false)

    var selectedProvider by remember(currentProvider) { mutableStateOf(currentProvider) }
    val catalog = remember(hostDetail, selectedProvider) {
        OrbitFormatters.catalogFor(hostDetail, selectedProvider)
    }
    var selectedModel by remember(selectedProvider, currentModel) {
        mutableStateOf(
            if (selectedProvider == currentProvider) {
                currentModel ?: catalog.configuredDefaultModel ?: catalog.models.firstOrNull { it.isDefault }?.id
            } else {
                catalog.configuredDefaultModel ?: catalog.models.firstOrNull { it.isDefault }?.id
            }
        )
    }
    val activeModelSpec = remember(catalog, selectedModel) {
        catalog.models.firstOrNull { it.id == selectedModel } ?: catalog.models.firstOrNull { it.isDefault }
    }
    var selectedEffort by remember(selectedProvider, activeModelSpec, currentEffort) {
        mutableStateOf(
            OrbitFormatters.resolveEffort(
                spec = activeModelSpec,
                desired = currentEffort,
                catalogDefault = catalog.configuredDefaultEffort,
            )
        )
    }

    val availableProviders = hostDetail?.host?.availableProviders.orEmpty()
    val thinkingOptions = activeModelSpec?.thinkingOptions.orEmpty()

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
                .padding(horizontal = OrbitSpacing.lg, vertical = OrbitSpacing.xs),
            verticalArrangement = Arrangement.spacedBy(OrbitSpacing.md),
        ) {
            // Header
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Model & Reasoning",
                    style = typography.title,
                    color = colors.fgPrimary,
                )
                Box(
                    modifier = Modifier
                        .size(OrbitSpacing.touchTarget)
                        .clip(RoundedCornerShape(OrbitSpacing.md))
                        .clickable {
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            onRefreshCatalogs()
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Refresh,
                        contentDescription = "Refresh provider models",
                        tint = colors.fgSecondary,
                        modifier = Modifier.size(OrbitSpacing.iconMd),
                    )
                }
            }

            // 1. PROVIDER Switcher
            Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs)) {
                Text(
                    text = "PROVIDER",
                    style = typography.metaSemiBold,
                    color = colors.fgMuted,
                )
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                ) {
                    FfiProviderKind.entries.forEach { provider ->
                        val isInstalled = availableProviders.isEmpty() || provider in availableProviders
                        val active = provider == selectedProvider
                        val pillShape = RoundedCornerShape(OrbitSpacing.md)

                        Column(
                            modifier = Modifier
                                .weight(1f)
                                .heightIn(min = 44.dp)
                                .clip(pillShape)
                                .background(if (active) colors.subtleFill else colors.code, pillShape)
                                .border(
                                    if (active) 1.dp else OrbitSpacing.hairline,
                                    if (active) colors.primary else colors.hairline,
                                    pillShape
                                )
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    selectedProvider = provider
                                    val nextCat = OrbitFormatters.catalogFor(hostDetail, provider)
                                    val nextMod = nextCat.configuredDefaultModel
                                        ?: nextCat.models.firstOrNull { it.isDefault }?.id
                                        ?: nextCat.models.firstOrNull()?.id
                                    val nextSpec = nextCat.models.firstOrNull { it.id == nextMod }
                                    val nextEff = OrbitFormatters.resolveEffort(
                                        nextSpec,
                                        selectedEffort,
                                        nextCat.configuredDefaultEffort,
                                    )
                                    selectedModel = nextMod
                                    selectedEffort = nextEff
                                    onSelect(provider, nextMod, nextEff)
                                }
                                .padding(horizontal = OrbitSpacing.xs, vertical = OrbitSpacing.xs),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.Center,
                        ) {
                            Text(
                                text = OrbitFormatters.providerLabel(provider),
                                style = typography.subheadSemiBold,
                                color = when {
                                    active -> colors.primary
                                    isInstalled -> colors.fgPrimary
                                    else -> colors.fgMuted
                                },
                                maxLines = 1,
                                softWrap = false,
                            )
                            if (!isInstalled) {
                                Text(
                                    text = "Unavailable",
                                    style = typography.meta,
                                    color = colors.fgMuted,
                                    maxLines = 1,
                                    softWrap = false,
                                )
                            }
                        }
                    }
                }
            }

            // 2. REASONING EFFORT (Immediately below PROVIDER and above MODEL for at-a-glance visibility!)
            if (thinkingOptions.isNotEmpty()) {
                val activeEffortDesc = remember(thinkingOptions, selectedEffort) {
                    selectedEffort?.let { eff ->
                        thinkingOptions.firstOrNull { it.effort == eff }?.description
                            ?: OrbitFormatters.effortDescription(eff)
                    }.orEmpty()
                }

                Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs)) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = "REASONING EFFORT",
                            style = typography.metaSemiBold,
                            color = colors.fgMuted,
                            maxLines = 1,
                            softWrap = false,
                        )
                        if (activeEffortDesc.isNotBlank()) {
                            Text(
                                text = activeEffortDesc,
                                style = typography.meta,
                                color = colors.fgSecondary,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                        }
                    }

                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .horizontalScroll(rememberScrollState()),
                        horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                    ) {
                        thinkingOptions.forEach { opt ->
                            val active = opt.effort == selectedEffort
                            val chipShape = RoundedCornerShape(OrbitSpacing.md)
                            Box(
                                modifier = Modifier
                                    .heightIn(min = 38.dp)
                                    .clip(chipShape)
                                    .background(if (active) colors.subtleFill else colors.code, chipShape)
                                    .border(
                                        if (active) 1.dp else OrbitSpacing.hairline,
                                        if (active) colors.primary else colors.hairline,
                                        chipShape
                                    )
                                    .clickable {
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                        selectedEffort = opt.effort
                                        onSelect(selectedProvider, selectedModel, opt.effort)
                                    }
                                    .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.xs),
                                contentAlignment = Alignment.Center,
                            ) {
                                Text(
                                    text = opt.label.ifBlank { OrbitFormatters.effortLabel(opt.effort) },
                                    style = typography.subheadMedium,
                                    color = if (active) colors.primary else colors.fgPrimary,
                                    maxLines = 1,
                                    softWrap = false,
                                )
                            }
                        }
                    }
                }
            }

            HorizontalDivider(thickness = OrbitSpacing.hairline, color = colors.hairline)

            // 3. MODEL List
            Column(verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs)) {
                Text(
                    text = "MODEL",
                    style = typography.metaSemiBold,
                    color = colors.fgMuted,
                )

                LazyColumn(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(max = 260.dp),
                    verticalArrangement = Arrangement.spacedBy(OrbitSpacing.xs),
                ) {
                    items(catalog.models, key = { it.id }) { spec ->
                        val isSelected = spec.id == selectedModel
                        val cleanedDesc = remember(spec.label, spec.id, spec.description) {
                            stripRedundantModelPrefix(spec.label, spec.id, spec.description)
                        }
                        val rowShape = RoundedCornerShape(OrbitSpacing.md)

                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = OrbitSpacing.touchTarget)
                                .clip(rowShape)
                                .background(if (isSelected) colors.subtleFill else Color.Transparent, rowShape)
                                .border(
                                    if (isSelected) 1.dp else OrbitSpacing.hairline,
                                    if (isSelected) colors.primary else colors.hairline,
                                    rowShape
                                )
                                .clickable {
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    val nextEffort = OrbitFormatters.resolveEffort(
                                        spec,
                                        selectedEffort,
                                        catalog.configuredDefaultEffort,
                                    )
                                    selectedModel = spec.id
                                    selectedEffort = nextEffort
                                    onSelect(selectedProvider, spec.id, nextEffort)
                                    onDismiss()
                                }
                                .padding(horizontal = OrbitSpacing.md, vertical = OrbitSpacing.sm),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            Column(modifier = Modifier.weight(1f)) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(OrbitSpacing.sm),
                                ) {
                                    Text(
                                        text = spec.label,
                                        style = typography.bodyMedium,
                                        color = colors.fgPrimary,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    if (spec.isDefault) {
                                        Text(
                                            text = "Default",
                                            style = typography.subheadMedium,
                                            color = colors.fgMuted,
                                            maxLines = 1,
                                            softWrap = false,
                                        )
                                    }
                                }
                                if (cleanedDesc.isNotBlank()) {
                                    Text(
                                        text = cleanedDesc,
                                        style = typography.meta,
                                        color = colors.fgSecondary,
                                        maxLines = 2,
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
            }

            Spacer(modifier = Modifier.height(OrbitSpacing.xs))
        }
    }
}

private fun stripRedundantModelPrefix(
    label: String,
    modelId: String,
    rawDescription: String,
): String {
    var trimmed = rawDescription.trim()
    if (trimmed.isEmpty()) return ""
    val prefixes = listOf(
        "$label · ",
        "$label - ",
        "$label — ",
        "$modelId · ",
        "$modelId - ",
    )
    for (p in prefixes) {
        if (trimmed.startsWith(p, ignoreCase = true)) {
            trimmed = trimmed.substring(p.length).trim()
            break
        }
    }
    return if (trimmed.equals(label, ignoreCase = true)) "" else trimmed
}
