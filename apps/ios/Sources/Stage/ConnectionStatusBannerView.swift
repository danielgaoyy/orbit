import SwiftUI

/// Compact stage-top connection, offline, and daemon-version status banner aligned with Desktop `ConnectionStatusBanner`.
struct ConnectionStatusBannerView: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.orbitPalette) private var palette

    private var activeHost: FfiHostCard? {
        store.activeHost
    }

    private var upgradeState: HostDaemonUpgradeState? {
        guard let hid = activeHost?.hostId else { return nil }
        return store.hostUpgradeStates[hid]
    }

    private var shouldShowVersionBanner: Bool {
        guard let host = activeHost else { return false }
        guard !store.isVersionMismatchDismissed(for: host) else { return false }
        return store.isHostProtocolBehind(host) || store.isHostDaemonOutdated(host)
    }

    var body: some View {
        VStack(spacing: 0) {
            if store.isNetworkOffline {
                bannerRow(
                    tint: palette.warning,
                    icon: "wifi.slash",
                    title: "No Internet Connection",
                    subtitle: "Waiting for network to resume streaming…"
                ) {
                    EmptyView()
                }
            } else if let host = activeHost, let up = upgradeState, up.isUpgrading {
                bannerRow(
                    tint: palette.accentSky,
                    icon: nil,
                    showSpinner: true,
                    title: "Updating \(host.displayName)",
                    subtitle: up.statusMessage ?? "Downloading and restarting daemon…"
                ) {
                    EmptyView()
                }
            } else if let host = activeHost, let upErr = upgradeState?.error, !upErr.isEmpty {
                bannerRow(
                    tint: palette.danger,
                    icon: "exclamationmark.triangle",
                    title: "Daemon update failed",
                    subtitle: upErr
                ) {
                    Button {
                        store.hostUpgradeStates.removeValue(forKey: host.hostId)
                    } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 10.5, weight: .semibold))
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: 24, height: 24)
                    }
                    .buttonStyle(.plain)
                }
            } else if let host = activeHost, host.connectionState == .disconnected {
                bannerRow(
                    tint: palette.danger,
                    icon: "bolt.slash",
                    title: "\(host.displayName) Offline",
                    subtitle: host.lastError?.trimmingCharacters(in: .whitespacesAndNewlines)
                ) {
                    Button {
                        store.reconnectHost(host.hostId)
                    } label: {
                        Text("Reconnect")
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.danger)
                            .padding(.horizontal, 10)
                            .frame(height: 26)
                            .background(palette.danger.opacity(0.14), in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
            } else if let host = activeHost,
                      host.connectionState == .connectingDirect || host.connectionState == .connectingRelay {
                bannerRow(
                    tint: palette.warning,
                    icon: nil,
                    showSpinner: true,
                    title: host.connectionState == .connectingRelay
                        ? "Connecting to \(host.displayName) via Relay…"
                        : "Connecting to \(host.displayName)…",
                    subtitle: nil
                ) {
                    EmptyView()
                }
            } else if let host = activeHost, shouldShowVersionBanner {
                let isBehind = store.isHostProtocolBehind(host)
                let daemonVer = host.daemonVersion.map { $0.hasPrefix("v") ? $0 : "v\($0)" } ?? "older"
                let outdatedCount = store.outdatedOnlineHosts.count

                bannerRow(
                    tint: isBehind ? palette.warning : palette.accentSky,
                    icon: "arrow.down.circle",
                    title: isBehind
                        ? "Daemon protocol update needed"
                        : "\(host.displayName) is on \(daemonVer)",
                    subtitle: "App v\(store.appShortVersion) has newer capabilities"
                ) {
                    HStack(spacing: 6) {
                        Button {
                            if outdatedCount > 1 {
                                store.upgradeAllOutdatedHosts()
                            } else {
                                store.upgradeHostDaemon(host.hostId)
                            }
                        } label: {
                            Text(outdatedCount > 1 ? "Update All (\(outdatedCount))" : "Update")
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(isBehind ? palette.warning : palette.accentSky)
                                .padding(.horizontal, 10)
                                .frame(height: 26)
                                .background(
                                    (isBehind ? palette.warning : palette.accentSky).opacity(0.14),
                                    in: Capsule()
                                )
                        }
                        .buttonStyle(.plain)

                        Button {
                            store.dismissVersionMismatch(for: host.hostId)
                        } label: {
                            Image(systemName: "xmark")
                                .font(.system(size: 10.5, weight: .semibold))
                                .foregroundStyle(palette.fgMuted)
                                .frame(width: 24, height: 24)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Dismiss Update Banner")
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func bannerRow<Trailing: View>(
        tint: Color,
        icon: String?,
        showSpinner: Bool = false,
        title: String,
        subtitle: String?,
        @ViewBuilder trailing: () -> Trailing
    ) -> some View {
        VStack(spacing: 0) {
            HStack(spacing: OrbitSpacing.sm) {
                if showSpinner {
                    OrbitSpinnerRingView(color: tint, size: 12)
                } else if let icon {
                    Image(systemName: icon)
                        .font(.system(size: 12.5, weight: .semibold))
                        .foregroundStyle(tint)
                }

                VStack(alignment: .leading, spacing: 1) {
                    Text(title)
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    if let sub = subtitle, !sub.isEmpty {
                        Text(sub)
                            .font(.system(size: 11, weight: .regular))
                            .foregroundStyle(palette.fgSecondary)
                            .lineLimit(1)
                            .truncationMode(.tail)
                    }
                }
                .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)

                trailing()
            }
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.vertical, 7)
            .background(tint.opacity(palette.isDark ? 0.12 : 0.08))

            OrbitHairline()
        }
    }
}
