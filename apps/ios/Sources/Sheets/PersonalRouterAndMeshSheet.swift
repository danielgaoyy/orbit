import SwiftUI

/// Personal Agent Router & Multi-Host Fleet Mesh Management sheet (`discoverFleetRouterHosts`,
/// `getRouterConfig`, `updateRouterConfig`, `getFleetMeshSnapshot`, `updateMeshPeer`).
struct PersonalRouterAndMeshSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette
    @State private var selectedRouterHostId: String? = nil

    private var activeRouterHostId: String? {
        selectedRouterHostId
            ?? store.primaryRouterCandidate?.hostId
            ?? store.selectedHostId
            ?? store.hosts.first?.hostId
    }

    private var activeSnapshot: RouterStatusSnapshot? {
        guard let hid = activeRouterHostId else { return nil }
        return store.routerStatusByHost[hid]
    }

    var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.xl) {
                    routerHostSelectionSection
                    if let hid = activeRouterHostId, let snap = activeSnapshot {
                        routerPolicySection(hostId: hid, snapshot: snap)
                    }
                    fleetMeshPeersSection
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.md)
            }
            .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
            .navigationTitle(OrbitL10n.tr("Fleet Mesh & Personal Agent"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button {
                        store.hapticLight()
                        store.refreshRouterAndMesh()
                    } label: {
                        if store.routerBusy {
                            OrbitSpinnerRingView(color: palette.fgSecondary, size: 13)
                        } else {
                            Image(systemName: "arrow.clockwise")
                                .font(OrbitTypography.iconSmall)
                                .foregroundStyle(palette.fgSecondary)
                        }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Done")) { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
            .onAppear {
                store.refreshRouterAndMesh()
            }
        }
    }

    // MARK: - 1. Primary Router Host Selection

    private var routerHostSelectionSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text(OrbitL10n.tr("Primary Router Host"))
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, 4)

            if store.routerHostCandidates.isEmpty {
                Text("No paired hosts discovered for Personal Agent routing yet.")
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .padding(OrbitSpacing.md)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(store.routerHostCandidates.enumerated()), id: \.element.hostId) { idx, cap in
                        if idx > 0 { OrbitHairline().padding(.leading, 40) }
                        let isSelected = cap.hostId == activeRouterHostId
                        Button {
                            store.hapticLight()
                            selectedRouterHostId = cap.hostId
                        } label: {
                            HStack(spacing: OrbitSpacing.md) {
                                Circle()
                                    .fill(cap.online ? palette.direct : palette.fgMuted)
                                    .frame(width: 7, height: 7)

                                VStack(alignment: .leading, spacing: 2) {
                                    HStack(spacing: 6) {
                                        Text(cap.hostName)
                                            .font(OrbitTypography.subhead(.semibold))
                                            .foregroundStyle(palette.fgPrimary)
                                        if cap.routerEnabled {
                                            Text("Active")
                                                .font(.system(size: 10.5, weight: .semibold))
                                                .foregroundStyle(palette.direct)
                                                .padding(.horizontal, 6)
                                                .padding(.vertical, 1.5)
                                                .background(palette.direct.opacity(0.14), in: Capsule())
                                        }
                                    }
                                    if let reason = cap.recommendationReason, !reason.isEmpty {
                                        Text(reason)
                                            .font(OrbitTypography.meta(.regular))
                                            .foregroundStyle(palette.fgMuted)
                                            .lineLimit(2)
                                    }
                                }

                                Spacer()

                                if isSelected {
                                    Image(systemName: "checkmark")
                                        .font(.system(size: 12, weight: .semibold))
                                        .foregroundStyle(palette.primary)
                                }
                            }
                            .padding(.horizontal, OrbitSpacing.md)
                            .padding(.vertical, 11)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
            }
        }
    }

    // MARK: - 2. Router Policy & Scope Controls

    private func routerPolicySection(hostId: String, snapshot: RouterStatusSnapshot) -> some View {
        let cfg = snapshot.config
        return VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text(OrbitL10n.tr("Personal Agent"))
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, 4)

            VStack(spacing: 0) {
                Toggle(isOn: Binding(
                    get: { cfg.enabled },
                    set: { newVal in
                        var next = cfg
                        next.enabled = newVal
                        store.updatePersonalRouterConfig(hostId: hostId, config: next, activate: newVal)
                    }
                )) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Enable Personal Agent Router")
                            .font(OrbitTypography.subhead(.medium))
                            .foregroundStyle(palette.fgPrimary)
                        Text("\(snapshot.managedWorkersCount) workers · \(snapshot.pendingApprovalsCount) waiting")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                    }
                }
                .tint(palette.direct)
                .padding(OrbitSpacing.md)

                OrbitHairline().padding(.leading, OrbitSpacing.md)

                VStack(alignment: .leading, spacing: 8) {
                    Text(OrbitL10n.tr("Router Scope"))
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgSecondary)
                    HStack(spacing: 4) {
                        scopeChip(
                            title: OrbitL10n.tr("Global Fleet"),
                            selected: cfg.scope == .globalFleet
                        ) {
                            var next = cfg
                            next.scope = .globalFleet
                            store.updatePersonalRouterConfig(hostId: hostId, config: next, activate: cfg.enabled)
                        }
                        scopeChip(
                            title: OrbitL10n.tr("Local Host Only"),
                            selected: cfg.scope == .localHostOnly
                        ) {
                            var next = cfg
                            next.scope = .localHostOnly
                            store.updatePersonalRouterConfig(hostId: hostId, config: next, activate: cfg.enabled)
                        }
                    }
                    .padding(3)
                    .background(palette.recessed, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .padding(OrbitSpacing.md)

                OrbitHairline().padding(.leading, OrbitSpacing.md)

                VStack(alignment: .leading, spacing: 8) {
                    Text(OrbitL10n.tr("Pre-Review Mode"))
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgSecondary)
                    HStack(spacing: 4) {
                        scopeChip(title: "Off", selected: cfg.preReviewMode == .disabled) {
                            var next = cfg
                            next.preReviewMode = .disabled
                            store.updatePersonalRouterConfig(hostId: hostId, config: next, activate: cfg.enabled)
                        }
                        scopeChip(title: "Assist", selected: cfg.preReviewMode == .assistOnly) {
                            var next = cfg
                            next.preReviewMode = .assistOnly
                            store.updatePersonalRouterConfig(hostId: hostId, config: next, activate: cfg.enabled)
                        }
                        scopeChip(title: "Medium Risk", selected: cfg.preReviewMode == .mediumRiskOnly) {
                            var next = cfg
                            next.preReviewMode = .mediumRiskOnly
                            store.updatePersonalRouterConfig(hostId: hostId, config: next, activate: cfg.enabled)
                        }
                    }
                    .padding(3)
                    .background(palette.recessed, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .padding(OrbitSpacing.md)
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
        }
    }

    private func scopeChip(title: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button {
            store.hapticLight()
            action()
        } label: {
            Text(title)
                .font(OrbitTypography.meta(selected ? .semibold : .medium))
                .foregroundStyle(selected ? palette.fgPrimary : palette.fgSecondary)
                .frame(maxWidth: .infinity, minHeight: 30)
                .background(selected ? palette.elevated : Color.clear, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        }
        .buttonStyle(.plain)
    }

    // MARK: - 3. Fleet Mesh Connected Peers

    private var fleetMeshPeersSection: some View {
        let hostId = activeRouterHostId ?? store.selectedHostId ?? ""
        let peers = store.fleetMeshSnapshot[hostId] ?? []
        return VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text(OrbitL10n.tr("Mesh Peers"))
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, 4)

            if peers.isEmpty {
                Text("No mesh peers registered on this host yet.")
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .padding(OrbitSpacing.md)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(peers.enumerated()), id: \.element.registrationId) { idx, peer in
                        if idx > 0 { OrbitHairline().padding(.leading, 40) }
                        HStack(spacing: OrbitSpacing.md) {
                            Circle()
                                .fill(meshStatusColor(peer.status))
                                .frame(width: 7, height: 7)

                            VStack(alignment: .leading, spacing: 2) {
                                Text(peer.label.isEmpty ? (peer.hostname ?? peer.registrationId) : peer.label)
                                    .font(OrbitTypography.subhead(.medium))
                                    .foregroundStyle(palette.fgPrimary)
                                    .lineLimit(1)

                                Text(peer.lastError ?? peer.directEndpoints.first ?? peer.relayUrl ?? "\(peer.status)")
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(peer.lastError != nil ? palette.danger : palette.fgMuted)
                                    .lineLimit(1)
                            }

                            Spacer(minLength: 4)

                            Toggle("", isOn: Binding(
                                get: { peer.enabled },
                                set: { enabled in
                                    store.updateFleetMeshPeer(
                                        hostId: hostId,
                                        registrationId: peer.registrationId,
                                        enabled: enabled
                                    )
                                }
                            ))
                            .labelsHidden()
                            .tint(palette.direct)
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .padding(.vertical, 10)
                    }
                }
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(palette.hairline, lineWidth: 0.5))
            }
        }
    }

    private func meshStatusColor(_ status: ConnectedHostStatus) -> Color {
        switch status {
        case .online: return palette.direct
        case .connecting: return palette.warning
        case .error: return palette.danger
        case .disabled: return palette.fgMuted
        }
    }
}
