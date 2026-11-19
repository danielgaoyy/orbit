import SwiftUI

struct ModelPickerSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var currentProvider: FfiProviderKind = .claudeCode

    private let allProviders: [FfiProviderKind] = [.claudeCode, .codex, .antigravity]

    private var catalog: FfiProviderModelCatalog {
        store.catalog(for: currentProvider)
    }

    private var activeSpec: FfiProviderModelSpec? {
        if let mid = store.selectedModel, currentProvider == store.selectedProvider {
            return catalog.models.first(where: { $0.id == mid })
        }
        if let defId = catalog.configuredDefaultModel {
            return catalog.models.first(where: { $0.id == defId })
        }
        return catalog.models.first(where: { $0.isDefault }) ?? catalog.models.first
    }

    private func activeEffortDescription(spec: FfiProviderModelSpec) -> String {
        let selected = store.selectedEffort ?? spec.defaultEffort ?? catalog.configuredDefaultEffort
        guard let effort = selected else { return "" }
        if let opt = spec.thinkingOptions.first(where: { $0.effort == effort }),
           let desc = opt.description, !desc.isEmpty {
            return desc
        }
        return defaultEffortDescription(effort: effort)
    }

    var body: some View {
        VStack(spacing: 0) {
            sheetHeader
            OrbitHairline()

            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.md) {
                    providerSelectorSection
                    if let spec = activeSpec, !spec.thinkingOptions.isEmpty {
                        reasoningEffortStripSection(spec: spec)
                    }
                    modelsSection
                }
                .padding(OrbitSpacing.lg)
            }
        }
        .background(palette.canvas.ignoresSafeArea())
        .onAppear {
            currentProvider = store.selectedProvider
            store.refreshProviderModels(force: false)
        }
    }

    private var sheetHeader: some View {
        HStack {
            Text("Model & Reasoning")
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)

            Spacer()

            Button {
                store.hapticLight()
                store.refreshProviderModels(force: true)
            } label: {
                Image(systemName: "arrow.clockwise")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: OrbitSpacing.hitTarget, height: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Refresh Model Catalog")

            Button {
                dismiss()
            } label: {
                Text("Done")
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.primary)
                    .frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .frame(height: 52)
    }

    // MARK: - 1. Provider Selector

    private var providerSelectorSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            Text("PROVIDER")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)

            HStack(spacing: OrbitSpacing.sm) {
                ForEach(allProviders, id: \.self) { provider in
                    let isSelected = currentProvider == provider
                    let isInstalled = store.activeHost?.availableProviders.contains(provider) ?? true

                    Button {
                        currentProvider = provider
                        let nextCat = store.catalog(for: provider)
                        let defModel = nextCat.configuredDefaultModel
                            ?? nextCat.models.first(where: { $0.isDefault })?.id
                            ?? nextCat.models.first?.id
                        store.applyModelSelection(
                            provider: provider,
                            modelId: defModel,
                            effort: store.selectedEffort
                        )
                    } label: {
                        HStack(spacing: 6) {
                            OrbitProviderIcon(provider: provider, size: 15)
                                .opacity(isInstalled ? 1.0 : 0.45)

                            VStack(alignment: .leading, spacing: 1) {
                                Text(OrbitFormatters.providerShortLabel(provider))
                                    .font(OrbitTypography.subhead(isSelected ? .medium : .regular))
                                    .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgSecondary)
                                    .lineLimit(1)

                                if !isInstalled {
                                    Text("Offline")
                                        .font(OrbitTypography.meta(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                        .lineLimit(1)
                                }
                            }
                        }
                        .padding(.horizontal, OrbitSpacing.sm)
                        .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                        .background(isSelected ? palette.elevated : palette.subtleFill)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .strokeBorder(
                                    isSelected ? palette.primary : palette.hairline,
                                    lineWidth: isSelected ? 1.0 : 0.5
                                )
                        )
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    // MARK: - 2. Compact Reasoning Effort Strip (Above Model List)

    private func reasoningEffortStripSection(spec: FfiProviderModelSpec) -> some View {
        let activeDesc = activeEffortDescription(spec: spec)
        let effectiveSelected = store.selectedEffort ?? spec.defaultEffort ?? catalog.configuredDefaultEffort

        return VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            HStack(spacing: OrbitSpacing.sm) {
                Text("REASONING")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)

                Spacer(minLength: OrbitSpacing.xs)

                if !activeDesc.isEmpty {
                    Text(activeDesc)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgSecondary)
                        .lineLimit(1)
                }
            }

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: OrbitSpacing.xs) {
                    ForEach(spec.thinkingOptions, id: \.effort) { option in
                        let isSelected = effectiveSelected == option.effort
                        let pillTitle = option.label.isEmpty
                            ? formatEffortLabel(effort: option.effort)
                            : option.label

                        Button {
                            store.applyModelSelection(
                                provider: currentProvider,
                                modelId: spec.id,
                                effort: option.effort
                            )
                        } label: {
                            Text(pillTitle)
                                .font(OrbitTypography.subhead(isSelected ? .semibold : .medium))
                                .foregroundStyle(isSelected ? palette.onPrimary : palette.fgPrimary)
                                .padding(.horizontal, OrbitSpacing.md)
                                .padding(.vertical, 7)
                                .background(
                                    Capsule()
                                        .fill(isSelected ? palette.primary : palette.elevated)
                                )
                                .overlay(
                                    Capsule()
                                        .strokeBorder(
                                            isSelected ? palette.primary : palette.hairline,
                                            lineWidth: 0.5
                                        )
                                )
                                .frame(minHeight: OrbitSpacing.hitTarget)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    // MARK: - 3. Models List

    private var modelsSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            Text("MODEL")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)

            VStack(spacing: OrbitSpacing.xs) {
                ForEach(catalog.models, id: \.id) { spec in
                    let isSelected = (currentProvider == store.selectedProvider)
                        && (store.selectedModel == spec.id || (store.selectedModel == nil && spec.id == activeSpec?.id))
                    let rowEffort: FfiReasoningEffort? = {
                        guard !spec.thinkingOptions.isEmpty else { return nil }
                        if isSelected {
                            return store.selectedEffort ?? spec.defaultEffort ?? catalog.configuredDefaultEffort
                        }
                        return spec.defaultEffort ?? spec.thinkingOptions.first?.effort
                    }()

                    Button {
                        store.applyModelSelection(
                            provider: currentProvider,
                            modelId: spec.id,
                            effort: store.selectedEffort ?? spec.defaultEffort
                        )
                    } label: {
                        HStack(spacing: OrbitSpacing.md) {
                            VStack(alignment: .leading, spacing: 2) {
                                HStack(alignment: .firstTextBaseline, spacing: 6) {
                                    Text(spec.label)
                                        .font(.system(size: 14, weight: isSelected ? .medium : .regular))
                                        .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgPrimary.opacity(0.84))
                                        .lineLimit(1)

                                    if let rowEffort {
                                        Text(formatEffortLabel(effort: rowEffort))
                                            .font(.system(size: 12.5, weight: .regular))
                                            .foregroundStyle(palette.fgMuted)
                                            .lineLimit(1)
                                    }

                                    if spec.isDefault {
                                        Text("Default")
                                            .font(OrbitTypography.meta(.medium))
                                            .foregroundStyle(palette.fgMuted)
                                            .padding(.horizontal, OrbitSpacing.sm)
                                            .padding(.vertical, 1)
                                            .background(palette.subtleFill)
                                            .clipShape(Capsule())
                                    }
                                }

                                if !spec.description.isEmpty {
                                    Text(spec.description)
                                        .font(OrbitTypography.subhead(.regular))
                                        .foregroundStyle(palette.fgSecondary)
                                        .lineLimit(2)
                                }
                            }

                            Spacer()

                            if isSelected {
                                Image(systemName: "checkmark")
                                    .font(.system(size: 13.5, weight: .semibold))
                                    .foregroundStyle(palette.fgPrimary)
                            }
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .padding(.vertical, OrbitSpacing.sm + 2)
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .background(isSelected ? palette.activeRowFill : palette.elevated)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .strokeBorder(
                                    isSelected ? palette.primary.opacity(0.28) : palette.hairline,
                                    lineWidth: 0.5
                                )
                        )
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }
}
