import SwiftUI

private struct ModelPickerRowData: Identifiable, Equatable {
    let id: String
    let label: String
    let defaultEffort: FfiReasoningEffort?
    let thinkingOptions: [FfiThinkingOptionSpec]
}

/// Compact native iOS 26 Liquid Glass Popover for selecting Agent Engine, Model, and Thinking Effort.
struct ModelPickerPopoverView: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette
    @State private var browsedProvider: FfiProviderKind = .claudeCode

    private static let allProviders: [FfiProviderKind] = [.claudeCode, .codex, .antigravity]

    private static let defaultCustomThinkingOptions: [FfiThinkingOptionSpec] = [
        .low, .medium, .high, .xhigh, .max
    ].map { effort in
        FfiThinkingOptionSpec(
            effort: effort,
            label: formatEffortLabel(effort: effort),
            description: defaultEffortDescription(effort: effort),
            isDefault: effort == .medium
        )
    }

    private var detectedProviders: [FfiProviderKind] {
        let installed = Set(store.activeHost?.availableProviders ?? [])
        let filtered = Self.allProviders.filter { installed.contains($0) }
        return filtered.isEmpty ? [store.selectedProvider] : filtered
    }

    private var canSwitchAgentEngine: Bool {
        store.isDraftNewSession || store.activeAgent == nil
    }

    private var showAgentSwitcher: Bool {
        canSwitchAgentEngine && detectedProviders.count > 1
    }

    private var effectiveBrowsedProvider: FfiProviderKind {
        guard canSwitchAgentEngine else { return store.selectedProvider }
        if detectedProviders.contains(browsedProvider) {
            return browsedProvider
        }
        return detectedProviders.first ?? store.selectedProvider
    }

    private var catalog: FfiProviderModelCatalog {
        store.catalog(for: effectiveBrowsedProvider)
    }

    private var browsedRows: [ModelPickerRowData] {
        var rows: [ModelPickerRowData] = []
        let catalogModels = catalog.models

        if effectiveBrowsedProvider == store.selectedProvider,
           let rawModel = store.selectedModel,
           !rawModel.isEmpty,
           !catalogModels.contains(where: { $0.id.caseInsensitiveCompare(rawModel) == .orderedSame }) {
            rows.append(
                ModelPickerRowData(
                    id: rawModel,
                    label: formatCatalogModelLabel(catalog: catalog, rawModelId: rawModel),
                    defaultEffort: .medium,
                    thinkingOptions: Self.defaultCustomThinkingOptions
                )
            )
        }

        for spec in catalogModels {
            let cleanLabel: String
            if effectiveBrowsedProvider == .claudeCode,
               spec.label.lowercased().hasPrefix("claude ") {
                cleanLabel = String(spec.label.dropFirst("claude ".count))
            } else {
                cleanLabel = spec.label
            }
            rows.append(
                ModelPickerRowData(
                    id: spec.id,
                    label: cleanLabel,
                    defaultEffort: spec.defaultEffort,
                    thinkingOptions: spec.thinkingOptions
                )
            )
        }
        return rows
    }

    private var resolvedSelectedModelIdOnBrowsed: String? {
        if effectiveBrowsedProvider == store.selectedProvider,
           let selected = store.selectedModel,
           let matched = browsedRows.first(where: { $0.id.caseInsensitiveCompare(selected) == .orderedSame }) {
            return matched.id
        }
        if effectiveBrowsedProvider == store.selectedProvider,
           let selected = store.selectedModel,
           !selected.isEmpty {
            return selected
        }
        if let defId = catalog.configuredDefaultModel,
           let matched = browsedRows.first(where: { $0.id.caseInsensitiveCompare(defId) == .orderedSame }) {
            return matched.id
        }
        if let defSpec = catalog.models.first(where: { $0.isDefault }) {
            return defSpec.id
        }
        return browsedRows.first?.id
    }

    private var activeRowForThinking: ModelPickerRowData? {
        guard let targetId = resolvedSelectedModelIdOnBrowsed else { return browsedRows.first }
        return browsedRows.first(where: { $0.id.caseInsensitiveCompare(targetId) == .orderedSame })
            ?? browsedRows.first
    }

    private var activeSpecForThinking: FfiProviderModelSpec? {
        guard let targetId = activeRowForThinking?.id else { return nil }
        return catalog.models.first(where: { $0.id.caseInsensitiveCompare(targetId) == .orderedSame })
    }

    private func resolvedEffort(for row: ModelPickerRowData) -> FfiReasoningEffort? {
        guard !row.thinkingOptions.isEmpty else { return nil }
        let isActiveOnProvider = (effectiveBrowsedProvider == store.selectedProvider)
            && (resolvedSelectedModelIdOnBrowsed?.caseInsensitiveCompare(row.id) == .orderedSame)
        let desired = isActiveOnProvider ? store.selectedEffort : row.defaultEffort
        if let resolved = store.resolvedModelSelection(
            provider: effectiveBrowsedProvider,
            desiredModel: row.id,
            desiredEffort: desired
        )?.selectedEffort {
            return resolved
        }
        let spec = catalog.models.first(where: { $0.id.caseInsensitiveCompare(row.id) == .orderedSame })
        if let supported = resolveSupportedEffortForModel(
            spec: spec,
            desired: desired,
            catalogDefault: catalog.configuredDefaultEffort
        ) {
            return supported
        }
        if let d = desired, row.thinkingOptions.contains(where: { $0.effort == d }) {
            return d
        }
        if let def = row.defaultEffort, row.thinkingOptions.contains(where: { $0.effort == def }) {
            return def
        }
        return row.thinkingOptions.first?.effort
    }

    private var providerSelectionBinding: Binding<FfiProviderKind> {
        Binding(
            get: { effectiveBrowsedProvider },
            set: { nextProvider in
                browsedProvider = nextProvider
                store.switchDraftProvider(nextProvider)
            }
        )
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if showAgentSwitcher {
                Picker("Agent", selection: providerSelectionBinding) {
                    ForEach(detectedProviders, id: \.self) { provider in
                        Image(uiImage: OrbitProviderIcon.uiImage(for: provider, size: 17))
                            .accessibilityLabel(OrbitFormatters.providerLabel(provider))
                            .tag(provider)
                    }
                }
                .pickerStyle(.segmented)
                .padding(.horizontal, 12)
            }

            providerHeaderRow
                .padding(.horizontal, 12)

            ViewThatFits(in: .vertical) {
                modelRowsStack
                ScrollView(.vertical, showsIndicators: true) {
                    modelRowsStack
                }
                .frame(height: 268)
            }
            .frame(maxHeight: 268)

            if let row = activeRowForThinking, !row.thinkingOptions.isEmpty {
                OrbitHairline()
                    .padding(.horizontal, 12)
                thinkingSection(for: row)
                    .padding(.horizontal, 12)
            }
        }
        .padding(.vertical, 12)
        .frame(width: 304)
        .onAppear {
            browsedProvider = store.selectedProvider
            store.refreshProviderModels(force: false)
        }
    }

    // MARK: - Provider Header Row (matches Desktop 24pt header + subtle refresh)

    private var providerHeaderRow: some View {
        HStack(spacing: 6) {
            if !showAgentSwitcher {
                OrbitProviderIcon(provider: effectiveBrowsedProvider, size: 14)
            }

            Text(OrbitFormatters.providerLabel(effectiveBrowsedProvider))
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(palette.fgMuted)
                .lineLimit(1)

            Spacer(minLength: 8)

            Button {
                store.hapticLight()
                store.refreshProviderModels(force: true)
            } label: {
                Image(systemName: "arrow.clockwise")
                    .font(.system(size: 11.5, weight: .medium))
                    .foregroundStyle(palette.fgMuted)
                    .frame(width: 24, height: 24, alignment: .trailing)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Refresh Model Catalog")
        }
        .frame(height: 24)
        .padding(.horizontal, 10)
    }

    // MARK: - Compact Desktop-Style Model List

    private var modelRowsStack: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(browsedRows) { row in
                let isModelActive = (effectiveBrowsedProvider == store.selectedProvider)
                    && (resolvedSelectedModelIdOnBrowsed?.caseInsensitiveCompare(row.id) == .orderedSame)
                let rowEffort = resolvedEffort(for: row)

                Button {
                    if isModelActive {
                        store.hapticLight()
                        dismiss()
                    } else {
                        store.applyModelSelection(
                            provider: effectiveBrowsedProvider,
                            modelId: row.id,
                            effort: resolvedEffort(for: row)
                        )
                        if row.thinkingOptions.isEmpty {
                            dismiss()
                        }
                    }
                } label: {
                    HStack(spacing: 8) {
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            Text(row.label)
                                .font(.system(size: 14, weight: isModelActive ? .medium : .regular))
                                .foregroundStyle(isModelActive ? palette.fgPrimary : palette.fgPrimary.opacity(0.86))
                                .lineLimit(1)

                            if let rowEffort {
                                Text(formatEffortLabel(effort: rowEffort))
                                    .font(.system(size: 12.5, weight: .regular))
                                    .foregroundStyle(palette.fgPrimary.opacity(0.46))
                                    .lineLimit(1)
                            }
                        }

                        Spacer(minLength: 6)

                        if isModelActive {
                            Image(systemName: "checkmark")
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(palette.fgPrimary)
                        }
                    }
                    .padding(.horizontal, 10)
                    .frame(height: 38)
                    .background(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(isModelActive ? palette.activeRowFill.opacity(0.85) : Color.clear)
                    )
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 12)
    }

    // MARK: - Pinned Native Thinking Control

    private func thinkingSection(for row: ModelPickerRowData) -> some View {
        let options = row.thinkingOptions
        let activeEffort = resolvedEffort(for: row) ?? options[0].effort
        let effortDesc = activeEffortDescription(options: options, effort: activeEffort)
        let useCompactSegmentLabel = options.count >= 4

        let effortBinding = Binding<FfiReasoningEffort>(
            get: { activeEffort },
            set: { nextEffort in
                store.applyModelSelection(
                    provider: effectiveBrowsedProvider,
                    modelId: row.id,
                    effort: nextEffort
                )
            }
        )

        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text("Thinking")
                    .font(.system(size: 11.5, weight: .medium))
                    .foregroundStyle(palette.fgMuted)

                Spacer(minLength: 6)

                if !effortDesc.isEmpty {
                    Text(effortDesc)
                        .font(.system(size: 11.5, weight: .regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                }
            }
            .padding(.horizontal, 10)

            if options.count <= 5 {
                Picker("Thinking", selection: effortBinding) {
                    ForEach(options, id: \.effort) { opt in
                        Text(thinkingSegmentTitle(for: opt, compact: useCompactSegmentLabel))
                            .tag(opt.effort)
                    }
                }
                .pickerStyle(.segmented)
            } else {
                Menu {
                    Picker("Thinking", selection: effortBinding) {
                        ForEach(options, id: \.effort) { opt in
                            Text(thinkingSegmentTitle(for: opt, compact: false))
                                .tag(opt.effort)
                        }
                    }
                } label: {
                    HStack {
                        Text(formatEffortLabel(effort: activeEffort))
                            .font(.system(size: 13.5, weight: .medium))
                            .foregroundStyle(palette.fgPrimary)
                        Spacer()
                        Image(systemName: "chevron.up.chevron.down")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(palette.fgMuted)
                    }
                    .padding(.horizontal, 10)
                    .frame(height: 34)
                    .background(
                        RoundedRectangle(cornerRadius: 9, style: .continuous)
                            .fill(palette.subtleFill)
                    )
                    .contentShape(Rectangle())
                }
            }
        }
    }

    private func thinkingSegmentTitle(for option: FfiThinkingOptionSpec, compact: Bool) -> String {
        if compact {
            return OrbitFormatters.compactEffortLabel(option.effort)
        }
        return option.label.isEmpty ? formatEffortLabel(effort: option.effort) : option.label
    }

    private func activeEffortDescription(
        options: [FfiThinkingOptionSpec],
        effort: FfiReasoningEffort
    ) -> String {
        if let matched = options.first(where: { $0.effort == effort }),
           let desc = matched.description, !desc.isEmpty {
            return desc
        }
        return defaultEffortDescription(effort: effort)
    }
}
