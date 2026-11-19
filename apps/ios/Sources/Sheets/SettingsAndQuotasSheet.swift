import SwiftUI

struct SettingsAndQuotasSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Binding var appearanceRaw: String

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var customHostName = ""
    @State private var selectedRouteMode: FfiHostRouteMode = .auto
    @State private var customDirectEndpoint = ""
    @State private var relayUrl = ""
    @State private var confirmRemoveHost = false

    private var selectedAppearance: OrbitAppearanceMode {
        OrbitAppearanceMode(rawValue: appearanceRaw) ?? .system
    }

    private var quotas: [FfiProviderQuota] {
        store.currentHostDetail?.quotas ?? []
    }

    private let selectableRouteModes: [FfiHostRouteMode] = [.auto, .directOnly, .relayOnly]

    var body: some View {
        VStack(spacing: 0) {
            sheetHeader
            OrbitHairline()

            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.xl) {
                    appearanceSection
                    quotasSection
                    if let host = store.activeHost {
                        hostNetworkSection(host)
                    }
                }
                .padding(OrbitSpacing.lg)
            }
        }
        .background(palette.canvas.ignoresSafeArea())
        .onAppear {
            syncHostFields()
            store.refreshQuotas()
        }
        .onChange(of: store.selectedHostId) { _ in
            syncHostFields()
        }
        .confirmationDialog(
            "Remove this host from Orbit?",
            isPresented: $confirmRemoveHost,
            titleVisibility: .visible
        ) {
            if let host = store.activeHost {
                Button("Remove \(host.displayName)", role: .destructive) {
                    store.removeHost(host.hostId)
                    dismiss()
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Running remote agent sessions on the host will not be terminated.")
        }
    }

    private func syncHostFields() {
        guard let host = store.activeHost else { return }
        customHostName = host.customName ?? ""
        selectedRouteMode = (host.routeMode == .sshTunnel) ? .auto : host.routeMode
        customDirectEndpoint = host.customDirectEndpoint ?? ""
        relayUrl = host.relayUrl ?? ""
    }

    // MARK: - Header

    private var sheetHeader: some View {
        HStack {
            Text("Settings & Quotas")
                .font(OrbitTypography.title)
                .foregroundStyle(palette.fgPrimary)

            Spacer()

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

    // MARK: - Appearance Selector (System / Light / Graphite / OLED)

    private var appearanceSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            Text("APPEARANCE")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)

            LazyVGrid(
                columns: [
                    GridItem(.flexible(), spacing: OrbitSpacing.sm),
                    GridItem(.flexible(), spacing: OrbitSpacing.sm)
                ],
                spacing: OrbitSpacing.sm
            ) {
                ForEach(OrbitAppearanceMode.allCases) { mode in
                    let isSelected = selectedAppearance == mode
                    Button {
                        store.hapticLight()
                        withAnimation(.easeInOut(duration: 0.18)) {
                            appearanceRaw = mode.rawValue
                        }
                    } label: {
                        HStack(spacing: OrbitSpacing.sm) {
                            swatchCircle(for: mode)

                            VStack(alignment: .leading, spacing: 2) {
                                Text(mode.label)
                                    .font(OrbitTypography.subhead(isSelected ? .semibold : .medium))
                                    .foregroundStyle(palette.fgPrimary)
                                Text(mode.subtitle)
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                    .lineLimit(1)
                            }

                            Spacer(minLength: 0)
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .padding(.vertical, OrbitSpacing.sm)
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .background(isSelected ? palette.activeRowFill : palette.elevated)
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

    @ViewBuilder
    private func swatchCircle(for mode: OrbitAppearanceMode) -> some View {
        switch mode {
        case .system:
            ZStack {
                Circle()
                    .fill(
                        LinearGradient(
                            colors: [Color.white, Color(hex: "#191A1C")],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
                    .frame(width: 20, height: 20)
                Circle()
                    .strokeBorder(palette.hairline, lineWidth: 1)
                    .frame(width: 20, height: 20)
            }
        case .light:
            Circle()
                .fill(Color.white)
                .frame(width: 20, height: 20)
                .overlay(Circle().strokeBorder(Color.black.opacity(0.18), lineWidth: 1))
        case .dark:
            Circle()
                .fill(Color(hex: "#191A1C"))
                .frame(width: 20, height: 20)
                .overlay(Circle().strokeBorder(Color.white.opacity(0.22), lineWidth: 1))
        case .oled:
            Circle()
                .fill(Color.black)
                .frame(width: 20, height: 20)
                .overlay(Circle().strokeBorder(Color.white.opacity(0.32), lineWidth: 1))
        }
    }

    // MARK: - Account Quota Usage Bars

    private var quotasSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack {
                Text("PROVIDER QUOTAS")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)

                Spacer()

                Button {
                    store.hapticLight()
                    store.refreshQuotas()
                } label: {
                    HStack(spacing: OrbitSpacing.xs) {
                        Image(systemName: "arrow.clockwise")
                            .font(OrbitTypography.iconMicro)
                        Text("Refresh")
                            .font(OrbitTypography.meta(.medium))
                    }
                    .foregroundStyle(palette.primary)
                    .frame(minHeight: 32)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }

            if quotas.isEmpty {
                Text(store.selectedHostId == nil
                     ? "Pair a host to inspect provider account quotas."
                     : "No quota telemetry reported by installed providers yet.")
                    .orbitSubheadStyle()
                    .foregroundStyle(palette.fgSecondary)
                    .padding(OrbitSpacing.md)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(palette.elevated)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .strokeBorder(palette.hairline, lineWidth: 0.5)
                    )
            } else {
                VStack(spacing: OrbitSpacing.sm) {
                    ForEach(Array(quotas.enumerated()), id: \.offset) { _, quota in
                        quotaCard(quota)
                    }
                }
            }
        }
    }

    private func quotaCard(_ quota: FfiProviderQuota) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.md) {
            HStack {
                Text(quota.providerLabel)
                    .font(OrbitTypography.body(.semibold))
                    .foregroundStyle(palette.fgPrimary)

                if let account = quota.accountLabel, !account.isEmpty {
                    Text(account)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                }

                Spacer()

                if let credits = quota.creditsRemaining {
                    Text(String(format: "$%.2f left", credits))
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.direct)
                }
            }

            if let err = quota.error, !err.isEmpty {
                Text(err)
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.warning)
            }

            if quota.windows.isEmpty && quota.error == nil {
                Text("No active rate-limit windows.")
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted)
            } else {
                ForEach(Array(quota.windows.enumerated()), id: \.offset) { _, win in
                    quotaWindowBar(win)
                }
            }
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
    }

    private func quotaWindowBar(_ window: FfiQuotaWindow) -> some View {
        let clampedPct = min(100.0, max(0.0, Double(window.usedPercent)))
        let barColor: Color = {
            if clampedPct > 90 { return palette.danger }
            if clampedPct >= 70 { return palette.warning }
            return palette.direct
        }()

        return VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            HStack {
                Text(window.label)
                    .font(OrbitTypography.subhead(.medium))
                    .foregroundStyle(palette.fgPrimary)

                Spacer()

                if let reset = OrbitFormatters.resetCountdown(rfc3339: window.resetsAtRfc3339) {
                    Text(reset)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                }

                Text("\(Int(clampedPct.rounded()))%")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(barColor)
            }

            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(palette.subtleFill)
                        .frame(height: 4)

                    Capsule()
                        .fill(barColor)
                        .frame(width: max(4, geo.size.width * CGFloat(clampedPct / 100.0)), height: 4)
                }
            }
            .frame(height: 4)
        }
    }

    // MARK: - Host Route Mode & E2EE Relay Editor

    private func hostNetworkSection(_ host: FfiHostCard) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.md) {
            Text("HOST ROUTE & RELAY (\(host.displayName.uppercased()))")
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)

            VStack(alignment: .leading, spacing: OrbitSpacing.md) {
                // Telemetry Metadata + 1-Tap Reconnect
                VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
                    HStack(spacing: OrbitSpacing.sm) {
                        Circle()
                            .fill(palette.connectionColor(host.connectionState))
                            .frame(width: 6, height: 6)
                        Text(OrbitFormatters.connectionLabel(host.connectionState))
                            .font(OrbitTypography.subhead(.medium))
                            .foregroundStyle(palette.fgPrimary)

                        if let rtt = host.lastPingMs {
                            Text("· \(rtt)ms")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }

                        Spacer(minLength: OrbitSpacing.xs)

                        Button {
                            store.reconnectActiveHost()
                        } label: {
                            HStack(spacing: OrbitSpacing.xs) {
                                Image(systemName: "arrow.clockwise")
                                    .font(OrbitTypography.iconMicro)
                                Text("Reconnect")
                                    .font(OrbitTypography.meta(.semibold))
                            }
                            .foregroundStyle(palette.primary)
                            .padding(.horizontal, OrbitSpacing.sm + 2)
                            .padding(.vertical, OrbitSpacing.xs)
                            .background(palette.primary.opacity(0.12))
                            .clipShape(Capsule())
                            .overlay(
                                Capsule()
                                    .strokeBorder(palette.primary.opacity(0.28), lineWidth: 0.5)
                            )
                            .frame(minHeight: 36)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Reconnect Host")
                    }

                    HStack(spacing: OrbitSpacing.xs) {
                        Text("Pubkey: \(String(host.daemonPubkeyB64.prefix(20)))…")
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(1)

                        Spacer(minLength: OrbitSpacing.xs)

                        if let ver = host.daemonVersion, !ver.isEmpty {
                            Text("v\(ver)")
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }

                        if let arch = host.osArch {
                            Text(arch)
                                .font(OrbitTypography.code(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }
                    }
                }

                OrbitHairline()

                // Route Mode Selector (Auto / Direct Only / Relay Only)
                VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
                    Text("Route Mode")
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgSecondary)

                    HStack(spacing: OrbitSpacing.xs) {
                        ForEach(selectableRouteModes, id: \.self) { mode in
                            let isSelected = selectedRouteMode == mode
                            Button {
                                store.hapticLight()
                                selectedRouteMode = mode
                            } label: {
                                Text(OrbitFormatters.routeModeLabel(mode))
                                    .font(OrbitTypography.subhead(isSelected ? .semibold : .medium))
                                    .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgSecondary)
                                    .frame(maxWidth: .infinity, minHeight: 38)
                                    .background(
                                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                                            .fill(isSelected ? palette.elevated : Color.clear)
                                    )
                                    .frame(minHeight: OrbitSpacing.hitTarget)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(3)
                    .background(palette.subtleFill)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }

                // Custom Display Name
                labeledMonoField(
                    label: "Display Name (Optional)",
                    placeholder: host.hostname,
                    text: $customHostName
                )

                // Custom Direct Endpoint
                labeledMonoField(
                    label: "Direct Endpoint Override",
                    placeholder: host.directEndpoints.first ?? "ws://192.168.1.x:6868",
                    text: $customDirectEndpoint
                )

                // E2EE Relay URL
                labeledMonoField(
                    label: "Noise E2EE Relay URL",
                    placeholder: "wss://relay.orbit.dev",
                    text: $relayUrl
                )

                Button {
                    store.saveHostConfig(
                        hostId: host.hostId,
                        customName: customHostName,
                        routeMode: selectedRouteMode,
                        customDirectEndpoint: customDirectEndpoint,
                        relayUrl: relayUrl
                    )
                } label: {
                    Text("Save & Reconnect")
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.onPrimary)
                        .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                        .background(palette.primary)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)

                Button {
                    confirmRemoveHost = true
                } label: {
                    Text("Remove Host")
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.danger)
                        .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                        .background(palette.danger.opacity(0.1))
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            .padding(OrbitSpacing.md)
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    private func labeledMonoField(
        label: String,
        placeholder: String,
        text: Binding<String>
    ) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs) {
            Text(label)
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgSecondary)

            TextField(placeholder, text: text)
                .font(OrbitTypography.code(.regular))
                .foregroundStyle(palette.fgPrimary)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .padding(.horizontal, OrbitSpacing.md)
                .frame(minHeight: OrbitSpacing.hitTarget)
                .background(palette.code)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )
        }
    }
}
