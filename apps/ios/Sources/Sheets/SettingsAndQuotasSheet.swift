import SwiftUI
import UIKit

struct SettingsAndQuotasSheet: View {
    @ObservedObject var store: OrbitIosStore
    @Binding var appearanceRaw: String

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var cloudClientIdInput = ""
    @State private var cloudTokenInput = ""
    @State private var cloudUseTokenFallback = false
    @State private var cloudPassphraseInput = ""
    @State private var cloudConfirmPassphraseInput = ""
    @State private var cloudGeneratedKey: String? = nil

    private var selectedAppearance: OrbitAppearanceMode {
        OrbitAppearanceMode.fromStorage(appearanceRaw)
    }

    private var groupedBackground: Color {
        palette.isDark ? palette.canvas : palette.drawer
    }

    private var quotas: [FfiProviderQuota] {
        store.currentHostDetail?.quotas ?? []
    }

    private var appVersionLabel: String {
        let short = store.appShortVersion
        let build = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "1"
        return "\(short) (\(build))"
    }

    var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.xl) {
                    appearanceSection
                    generalPreferencesSection
                    cloudSyncSection
                    hostsSection
                    if store.activeHost != nil {
                        quotasSection
                    }
                    aboutSection
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.lg)
            }
            .background(groupedBackground.ignoresSafeArea())
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.primary)
                }
            }
            .navigationDestination(for: String.self) { hostId in
                HostDetailSettingsView(
                    store: store,
                    hostId: hostId,
                    onHostRemoved: {
                        if store.hosts.isEmpty {
                            dismiss()
                        }
                    }
                )
                .environment(\.orbitPalette, palette)
            }
        }
        .onAppear {
            store.refreshCloudSyncStatus()
            if store.activeHost != nil {
                store.refreshQuotas()
            }
        }
        .onChange(of: store.cloudSyncStatus?.state) { nextState in
            if nextState == .ready {
                cloudPassphraseInput = ""
                cloudConfirmPassphraseInput = ""
                cloudGeneratedKey = nil
            }
        }
    }

    // MARK: - 1. Appearance (Strictly System / Light / Dark)

    private var appearanceSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text("Appearance")
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, OrbitSpacing.xs)

            HStack(spacing: 4) {
                ForEach(OrbitAppearanceMode.allCases) { mode in
                    let isSelected = selectedAppearance == mode
                    Button {
                        store.hapticLight()
                        withAnimation(.easeInOut(duration: 0.18)) {
                            appearanceRaw = mode.rawValue
                        }
                    } label: {
                        HStack(spacing: 5) {
                            Image(systemName: mode.systemImage)
                                .font(.system(size: 12.5, weight: .medium))
                            Text(mode.label)
                                .font(OrbitTypography.subhead(isSelected ? .semibold : .medium))
                        }
                        .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgSecondary)
                        .frame(maxWidth: .infinity, minHeight: 36)
                        .background(
                            RoundedRectangle(cornerRadius: 9, style: .continuous)
                                .fill(isSelected ? palette.elevated : Color.clear)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 9, style: .continuous)
                                .strokeBorder(isSelected ? palette.hairline : Color.clear, lineWidth: 0.5)
                        )
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(4)
            .background(palette.isDark ? palette.elevated.opacity(0.65) : palette.subtleFill)
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    // MARK: - 1b. General Preferences (Aligned with Desktop Settings)

    private var generalPreferencesSection: some View {
        let defaultPerm = OrbitIosStore.sessionModeFromRaw(store.defaultPermissionModeRaw)
        return VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text("General")
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, OrbitSpacing.xs)

            VStack(spacing: 0) {
                VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
                    Text("Default Access Permissions")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgSecondary)

                    HStack(spacing: 3) {
                        permissionPrefChip(.default, title: "Always Ask", selected: defaultPerm == .default)
                        permissionPrefChip(.acceptEdits, title: "Accept Edits", selected: defaultPerm == .acceptEdits)
                        permissionPrefChip(.fullAccess, title: "Full Access", selected: defaultPerm == .fullAccess)
                    }
                    .padding(3)
                    .background(palette.subtleFill)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .padding(.horizontal, OrbitSpacing.md)
                .padding(.vertical, OrbitSpacing.md)

                OrbitHairline()
                    .padding(.leading, OrbitSpacing.md)

                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Default Execution Mode")
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgPrimary)
                        Text(store.draftUseWorktree ? "Isolated Git Worktree" : "Direct Local Folder")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                    }
                    Spacer()
                    Picker("", selection: Binding(
                        get: { store.draftUseWorktree },
                        set: { store.setDraftUseWorktree($0) }
                    )) {
                        Text("Local").tag(false)
                        Text("Worktree").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .frame(width: 150)
                }
                .padding(.horizontal, OrbitSpacing.md)
                .padding(.vertical, 10)

                OrbitHairline()
                    .padding(.leading, OrbitSpacing.md)

                Toggle(isOn: Binding(
                    get: { store.coalesceExploration },
                    set: { store.setCoalesceExploration($0) }
                )) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Collapse Thinking & Tool Steps")
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgPrimary)
                        Text("Keep completed exploration folded behind compact summaries")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                    }
                }
                .tint(palette.direct)
                .padding(.horizontal, OrbitSpacing.md)
                .padding(.vertical, 10)
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    private func permissionPrefChip(_ mode: FfiSessionMode, title: String, selected: Bool) -> some View {
        Button {
            store.setDefaultPermissionMode(mode)
        } label: {
            Text(title)
                .font(OrbitTypography.meta(selected ? .semibold : .medium))
                .foregroundStyle(selected ? palette.fgPrimary : palette.fgSecondary)
                .frame(maxWidth: .infinity, minHeight: 30)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(selected ? palette.elevated : Color.clear)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(selected ? palette.hairline : Color.clear, lineWidth: 0.5)
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    // MARK: - 1c. Zero-Server E2EE Cloud Sync (GitHub Secret Gist)

    private var cloudSyncSection: some View {
        let status = store.cloudSyncStatus
        let state = status?.state ?? .signedOut
        let oauthReady = status?.githubOauthConfigured ?? false
        let combinedError = store.cloudSyncError ?? status?.lastError
        let isAuthError = state == .error || (combinedError.map {
            let low = $0.lowercased()
            return low.contains("401") || low.contains("bad credentials") || low.contains("expired") || low.contains("revoked")
        } ?? false)

        return VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            HStack {
                Text("Cloud Sync (GitHub E2EE)")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgMuted)
                Spacer()
                if state == .error || isAuthError {
                    Text("Auth Expired")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.danger)
                } else if state == .ready {
                    Text("Synced")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.direct)
                } else if state == .locked {
                    Text("Locked")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.warning)
                }
            }
            .padding(.horizontal, OrbitSpacing.xs)

            VStack(alignment: .leading, spacing: OrbitSpacing.md) {
                if let flow = store.cloudSyncDeviceFlow {
                    VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                        Text("Enter this code on GitHub")
                            .font(OrbitTypography.subhead(.semibold))
                            .foregroundStyle(palette.fgPrimary)
                        HStack {
                            Text(flow.userCode)
                                .font(.system(.title3, design: .monospaced).weight(.bold))
                                .foregroundStyle(palette.fgPrimary)
                            Spacer()
                            Button("Copy") {
                                UIPasteboard.general.string = flow.userCode
                                store.showToast("Code copied")
                            }
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.accentSky)
                        }
                        .padding(OrbitSpacing.sm)
                        .background(palette.subtleFill)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))

                        HStack(spacing: OrbitSpacing.sm) {
                            Button {
                                UIPasteboard.general.string = flow.userCode
                                if let url = URL(string: flow.verificationUri) {
                                    UIApplication.shared.open(url)
                                }
                            } label: {
                                Text("Copy & Open GitHub")
                                    .font(OrbitTypography.subhead(.semibold))
                                    .foregroundStyle(palette.onPrimary)
                                    .frame(maxWidth: .infinity, minHeight: 38)
                                    .background(palette.primary)
                                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            }
                            .buttonStyle(.plain)

                            Button("Cancel") {
                                store.cancelGithubCloudLogin()
                            }
                            .font(OrbitTypography.subhead(.medium))
                            .foregroundStyle(palette.fgSecondary)
                            .padding(.horizontal, OrbitSpacing.sm)
                        }
                    }
                } else if state == .signedOut {
                    if false {
                        EmptyView()
                    } else {
                        Text("Sync paired hosts & projects across devices via your own encrypted GitHub Secret Gist (`gist` scope only, zero Orbit servers).")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgSecondary)

                        if !cloudUseTokenFallback {
                            if !oauthReady {
                                TextField("GitHub OAuth Client ID (Ov23li...)", text: $cloudClientIdInput)
                                    .textInputAutocapitalization(.never)
                                    .autocorrectionDisabled()
                                    .font(OrbitTypography.code(.regular))
                                    .padding(10)
                                    .background(palette.subtleFill)
                                    .clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
                            }
                            Button {
                                store.startGithubCloudLogin(
                                    clientIdOverride: cloudClientIdInput.isEmpty ? nil : cloudClientIdInput
                                )
                            } label: {
                                HStack(spacing: 6) {
                                    if store.cloudSyncBusy {
                                        ProgressView().tint(palette.onPrimary)
                                    } else {
                                        Image(systemName: "icloud.and.arrow.down")
                                    }
                                    Text("Sign in with GitHub")
                                        .font(OrbitTypography.subhead(.semibold))
                                }
                                .foregroundStyle(palette.onPrimary)
                                .frame(maxWidth: .infinity, minHeight: 40)
                                .background(palette.primary)
                                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            }
                            .buttonStyle(.plain)
                            .disabled(store.cloudSyncBusy)
                        } else {
                            SecureField("GitHub Token (gist scope)", text: $cloudTokenInput)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                                .font(OrbitTypography.code(.regular))
                                .padding(10)
                                .background(palette.subtleFill)
                                .clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))

                            HStack(spacing: OrbitSpacing.sm) {
                                Button {
                                    store.loginCloudSyncWithToken(cloudTokenInput)
                                    cloudTokenInput = ""
                                } label: {
                                    Text(store.cloudSyncBusy ? "Verifying…" : "Continue with Token")
                                        .font(OrbitTypography.subhead(.semibold))
                                        .foregroundStyle(palette.onPrimary)
                                        .frame(maxWidth: .infinity, minHeight: 38)
                                        .background(palette.primary)
                                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                                }
                                .buttonStyle(.plain)
                                .disabled(store.cloudSyncBusy)

                                Button("Create Token") {
                                    if let url = URL(string: "https://github.com/settings/tokens/new?scopes=gist&description=Orbit%20Cloud%20Sync") {
                                        UIApplication.shared.open(url)
                                    }
                                }
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(palette.accentSky)
                            }
                        }

                        Button {
                            cloudUseTokenFallback.toggle()
                            store.cloudSyncError = nil
                        } label: {
                            Text(cloudUseTokenFallback ? "Use GitHub OAuth Device Login instead" : "Or sign in with a Personal Access Token (`gist`)")
                                .font(OrbitTypography.meta(.medium))
                                .foregroundStyle(palette.fgMuted)
                        }
                        .buttonStyle(.plain)

                        OrbitHairline()

                        Button {
                            store.presentSheetSafely(.qrPair)
                        } label: {
                            HStack(spacing: 6) {
                                Image(systemName: "qrcode.viewfinder")
                                Text("Scan 1-Step Device Link QR")
                                    .font(OrbitTypography.subhead(.semibold))
                            }
                            .foregroundStyle(palette.fgPrimary)
                            .frame(maxWidth: .infinity, minHeight: 38)
                            .background(palette.subtleFill)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }
                        .buttonStyle(.plain)
                    }
                } else if state == .locked, let status {
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("@\(status.accountLogin ?? "github")")
                                .font(OrbitTypography.subhead(.semibold))
                                .foregroundStyle(palette.fgPrimary)
                            Text(status.remoteVaultExists
                                ? "Encrypted vault found — enter Sync Passphrase to unlock."
                                : "Create a Sync Passphrase to encrypt your first vault.")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgSecondary)
                        }
                        Spacer()
                        Button("Sign Out") {
                            store.logoutCloudSync(clearLocalHosts: false)
                        }
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.danger)
                    }

                    if let gen = cloudGeneratedKey {
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text("Generated Recovery Key (save it!)")
                                    .font(OrbitTypography.meta(.semibold))
                                    .foregroundStyle(palette.direct)
                                Spacer()
                                Button("Copy") {
                                    UIPasteboard.general.string = gen
                                    store.showToast("Recovery Key copied")
                                }
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(palette.accentSky)
                            }
                            Text(gen)
                                .font(OrbitTypography.code(.semibold))
                                .foregroundStyle(palette.fgPrimary)
                                .textSelection(.enabled)
                        }
                        .padding(10)
                        .background(palette.subtleFill)
                        .clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
                    }

                    SecureField(
                        status.remoteVaultExists ? "Sync Passphrase or Recovery Key" : "Create Sync Passphrase",
                        text: $cloudPassphraseInput
                    )
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .padding(10)
                    .background(palette.subtleFill)
                    .clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))

                    if !status.remoteVaultExists && cloudGeneratedKey == nil {
                        SecureField("Confirm Sync Passphrase", text: $cloudConfirmPassphraseInput)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .padding(10)
                            .background(palette.subtleFill)
                            .clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
                    }

                    HStack(spacing: OrbitSpacing.sm) {
                        Button {
                            if !status.remoteVaultExists && cloudGeneratedKey == nil && cloudPassphraseInput != cloudConfirmPassphraseInput {
                                store.cloudSyncError = "Sync Passphrases do not match."
                                return
                            }
                            let pass = cloudPassphraseInput
                            store.unlockCloudSyncWithPassphrase(pass)
                        } label: {
                            Text(store.cloudSyncBusy ? "Unlocking…" : (status.remoteVaultExists ? "Unlock & Sync" : "Encrypt & Sync"))
                                .font(OrbitTypography.subhead(.semibold))
                                .foregroundStyle(palette.onPrimary)
                                .frame(maxWidth: .infinity, minHeight: 38)
                                .background(palette.primary)
                                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                        }
                        .buttonStyle(.plain)
                        .disabled(store.cloudSyncBusy)

                        if !status.remoteVaultExists {
                            Button("Generate Key") {
                                let rec = store.generateCloudSyncPassphrase()
                                cloudGeneratedKey = rec
                                cloudPassphraseInput = rec
                                cloudConfirmPassphraseInput = rec
                            }
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.accentSky)
                        }
                    }
                } else if let status {
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("@\(status.accountLogin ?? "github")")
                                .font(OrbitTypography.subhead(.semibold))
                                .foregroundStyle(palette.fgPrimary)
                            Text("\(status.syncedHostCount) hosts · \(status.syncedProjectCount) projects · rev #\(status.lastVaultRevision) · \(status.instantUnlockEnabled ? "Instant Unlock" : "Manual Passphrase")")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }
                        Spacer()
                        Button {
                            store.syncCloudNow()
                        } label: {
                            HStack(spacing: 4) {
                                if store.cloudSyncBusy {
                                    ProgressView()
                                } else {
                                    Image(systemName: "arrow.triangle.2.circlepath")
                                }
                                Text("Sync")
                            }
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.onPrimary)
                            .padding(.horizontal, 12)
                            .frame(minHeight: 32)
                            .background(palette.primary)
                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                        }
                        .buttonStyle(.plain)
                        .disabled(store.cloudSyncBusy)
                    }

                    OrbitHairline()

                    Toggle(isOn: Binding(
                        get: { status.autoSyncEnabled },
                        set: { store.setCloudAutoSync($0) }
                    )) {
                        Text("Auto-sync changes")
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgPrimary)
                    }
                    .tint(palette.direct)

                    Toggle(isOn: Binding(
                        get: { !status.instantUnlockEnabled },
                        set: { store.setCloudSyncManualPassphraseMode($0) }
                    )) {
                        VStack(alignment: .leading, spacing: 1) {
                            Text("Require manual passphrase")
                                .font(OrbitTypography.subhead(.regular))
                                .foregroundStyle(palette.fgPrimary)
                            Text("Off = new devices auto-unlock cleanly via Dual-Gist.")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                        }
                    }
                    .tint(palette.direct)

                    HStack {
                        Button("Copy Link URI") {
                            if let uri = store.exportCloudSyncDeviceLinkUri() {
                                UIPasteboard.general.string = uri
                                store.showToast("Device Link URI copied — keep secret")
                            } else {
                                store.showToast("Unlock Cloud Sync first")
                            }
                        }
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.accentSky)

                        if let gistUrl = status.gistHtmlUrl, let url = URL(string: gistUrl), !gistUrl.isEmpty {
                            Text("·").foregroundStyle(palette.fgMuted)
                            Button("View Secret Gist") {
                                UIApplication.shared.open(url)
                            }
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.accentSky)
                        }
                        Spacer()
                        Button("Sign Out") {
                            store.logoutCloudSync(clearLocalHosts: false)
                        }
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.danger)
                    }
                }

                if let err = store.cloudSyncError ?? status?.lastError, !err.isEmpty {
                    VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
                        Text(err)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.danger)

                        if (isAuthError || state == .error) && store.cloudSyncDeviceFlow == nil {
                            Button {
                                store.startGithubCloudLogin(
                                    clientIdOverride: cloudClientIdInput.isEmpty ? nil : cloudClientIdInput
                                )
                            } label: {
                                HStack(spacing: 6) {
                                    if store.cloudSyncBusy {
                                        ProgressView().tint(palette.onPrimary)
                                    } else {
                                        Image(systemName: "arrow.clockwise")
                                    }
                                    Text("Sign In Again with GitHub")
                                        .font(OrbitTypography.meta(.semibold))
                                }
                                .foregroundStyle(palette.onPrimary)
                                .frame(maxWidth: .infinity, minHeight: 34)
                                .background(palette.primary)
                                .clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
                            }
                            .buttonStyle(.plain)
                            .disabled(store.cloudSyncBusy)
                        }
                    }
                }
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

    // MARK: - 2. Hosts Fleet (Progressive Disclosure Drill-Down)

    private var hostsSection: some View {
        let outdatedHosts = store.outdatedOnlineHosts
        return VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            HStack {
                Text("Hosts")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgMuted)
                Spacer()
                if !outdatedHosts.isEmpty {
                    Button {
                        store.upgradeAllOutdatedHosts()
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "arrow.down.circle.fill")
                                .font(.system(size: 11.5))
                            Text(outdatedHosts.count > 1 ? "Update All (\(outdatedHosts.count))" : "Update Host")
                                .font(OrbitTypography.meta(.semibold))
                        }
                        .foregroundStyle(palette.accentSky)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, OrbitSpacing.xs)

            VStack(spacing: 0) {
                if store.hosts.isEmpty {
                    HStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: "network")
                            .font(.system(size: 14, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: 20, alignment: .center)
                        Text("No hosts paired yet")
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgMuted)
                        Spacer()
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(minHeight: 46)

                    OrbitHairline()
                        .padding(.leading, 44)
                } else {
                    ForEach(Array(store.hosts.enumerated()), id: \.element.hostId) { idx, host in
                        NavigationLink(value: host.hostId) {
                            hostSummaryRow(host)
                        }
                        .buttonStyle(.plain)

                        OrbitHairline()
                            .padding(.leading, 44)
                    }
                }

                Button {
                    store.hapticLight()
                    store.presentSheetSafely(.qrPair)
                } label: {
                    HStack(spacing: OrbitSpacing.sm + 2) {
                        Image(systemName: "qrcode.viewfinder")
                            .font(.system(size: 14, weight: .medium))
                            .foregroundStyle(palette.primary)
                            .frame(width: 20, alignment: .center)

                        Text("Pair New Host")
                            .font(OrbitTypography.subhead(.medium))
                            .foregroundStyle(palette.primary)

                        Spacer()
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    private func hostSummaryRow(_ host: FfiHostCard) -> some View {
        let isActive = host.hostId == store.selectedHostId
        let detailParts: [String] = {
            var parts = [OrbitFormatters.compactTopBarConnectionLabel(for: host)]
            if let ping = host.lastPingMs {
                parts.append("\(ping)ms")
            }
            if let ver = host.daemonVersion, !ver.isEmpty {
                parts.append(ver.hasPrefix("v") ? ver : "v\(ver)")
            }
            return parts
        }()

        return HStack(spacing: OrbitSpacing.sm + 2) {
            Circle()
                .fill(palette.connectionColor(host.connectionState))
                .frame(width: 7, height: 7)
                .frame(width: 20, alignment: .center)

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(host.displayName)
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    if isActive && store.hosts.count > 1 {
                        Text("Active")
                            .font(.system(size: 10.5, weight: .medium))
                            .foregroundStyle(palette.fgSecondary)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 1.5)
                            .background(palette.subtleFill, in: Capsule())
                    }

                    if store.hostUpgradeStates[host.hostId]?.isUpgrading == true {
                        OrbitSpinnerRingView(color: palette.accentSky, size: 11)
                    } else if store.isHostDaemonOutdated(host) || store.isHostProtocolBehind(host) {
                        Text("Update")
                            .font(.system(size: 10.5, weight: .semibold))
                            .foregroundStyle(palette.accentSky)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 1.5)
                            .background(palette.accentSky.opacity(0.14), in: Capsule())
                    }
                }

                Text(detailParts.joined(separator: " · "))
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .lineLimit(1)
            }

            Spacer(minLength: OrbitSpacing.sm)

            Image(systemName: "chevron.right")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(palette.fgMuted)
        }
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.vertical, 9)
        .frame(minHeight: 54)
        .contentShape(Rectangle())
    }

    // MARK: - 3. Usage & Quotas

    private var quotasSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            HStack {
                Text("Usage & Quotas")
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgMuted)

                Spacer()

                Button {
                    store.hapticLight()
                    store.refreshQuotas()
                } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "arrow.clockwise")
                            .font(.system(size: 11, weight: .medium))
                        Text("Refresh")
                            .font(OrbitTypography.meta(.medium))
                    }
                    .foregroundStyle(palette.fgSecondary)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, OrbitSpacing.xs)

            if quotas.isEmpty {
                HStack {
                    Text("No active rate-limit windows reported yet.")
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgMuted)
                    Spacer()
                }
                .padding(.horizontal, OrbitSpacing.md)
                .frame(minHeight: 48)
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(quotas.enumerated()), id: \.offset) { idx, quota in
                        quotaBlock(quota)
                        if idx < quotas.count - 1 {
                            OrbitHairline()
                                .padding(.leading, OrbitSpacing.md)
                        }
                    }
                }
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )
            }
        }
    }

    private func quotaBlock(_ quota: FfiProviderQuota) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm + 2) {
            HStack(spacing: OrbitSpacing.sm) {
                OrbitProviderIcon(provider: quota.provider, size: 15)

                Text(quota.providerLabel)
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)

                if let account = quota.accountLabel, !account.isEmpty {
                    Text(account)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }

                Spacer(minLength: OrbitSpacing.xs)

                if let credits = quota.creditsRemaining {
                    Text(String(format: "$%.2f", credits))
                        .font(OrbitTypography.code(.medium))
                        .foregroundStyle(palette.direct)
                }
            }

            if let err = quota.error, !err.isEmpty {
                Text(err)
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.warning)
            }

            if quota.windows.isEmpty && quota.error == nil {
                Text("No active rate-limit windows")
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted)
            } else {
                ForEach(Array(quota.windows.enumerated()), id: \.offset) { _, win in
                    quotaWindowBar(win)
                }
            }
        }
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.vertical, OrbitSpacing.md)
    }

    private func quotaWindowBar(_ window: FfiQuotaWindow) -> some View {
        let clampedPct = min(100.0, max(0.0, Double(window.usedPercent)))
        let barColor: Color = {
            if clampedPct > 90 { return palette.danger }
            if clampedPct >= 70 { return palette.warning }
            return palette.direct
        }()

        return VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: OrbitSpacing.xs) {
                Text(window.label)
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgSecondary)
                    .lineLimit(1)

                Spacer(minLength: OrbitSpacing.xs)

                if let reset = OrbitFormatters.resetCountdown(rfc3339: window.resetsAtRfc3339) {
                    Text(reset)
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                }

                Text("\(Int(clampedPct.rounded()))%")
                    .font(OrbitTypography.code(.medium))
                    .foregroundStyle(barColor)
            }

            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(palette.subtleFill)
                        .frame(height: 3.5)

                    Capsule()
                        .fill(barColor)
                        .frame(
                            width: max(3.5, geo.size.width * CGFloat(clampedPct / 100.0)),
                            height: 3.5
                        )
                }
            }
            .frame(height: 3.5)
        }
    }

    // MARK: - 4. About

    private var aboutSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text("About")
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, OrbitSpacing.xs)

            VStack(spacing: 0) {
                HStack {
                    Text("Version")
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgPrimary)
                    Spacer()
                    Text(appVersionLabel)
                        .font(OrbitTypography.code(.regular))
                        .foregroundStyle(palette.fgMuted)
                }
                .padding(.horizontal, OrbitSpacing.md)
                .frame(minHeight: OrbitSpacing.hitTarget)

                OrbitHairline()
                    .padding(.leading, OrbitSpacing.md)

                HStack {
                    Text("Encryption")
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgPrimary)
                    Spacer()
                    Text("Noise NK · E2EE")
                        .font(OrbitTypography.code(.regular))
                        .foregroundStyle(palette.fgMuted)
                }
                .padding(.horizontal, OrbitSpacing.md)
                .frame(minHeight: OrbitSpacing.hitTarget)
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }
}

// MARK: - Drill-Down Host Detail Settings View

private struct HostDetailSettingsView: View {
    @ObservedObject var store: OrbitIosStore
    let hostId: String
    let onHostRemoved: () -> Void

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var customHostName = ""
    @State private var selectedRouteMode: FfiHostRouteMode = .auto
    @State private var customDirectEndpoint = ""
    @State private var relayUrl = ""
    @State private var confirmRemoveHost = false

    private let selectableRouteModes: [FfiHostRouteMode] = [.auto, .directOnly, .relayOnly]

    private var groupedBackground: Color {
        palette.isDark ? palette.canvas : palette.drawer
    }

    private var host: FfiHostCard? {
        store.hosts.first { $0.hostId == hostId }
    }

    var body: some View {
        ScrollView(.vertical, showsIndicators: true) {
            if let host = host {
                VStack(alignment: .leading, spacing: OrbitSpacing.xl) {
                    diagnosticsSection(host)
                    connectionSection(host)
                    dangerSection(host)
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.lg)
            } else {
                Text("Host is no longer available.")
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .padding(OrbitSpacing.xl)
            }
        }
        .scrollDismissesKeyboard(.interactively)
        .background(groupedBackground.ignoresSafeArea())
        .navigationTitle(host?.displayName ?? "Host")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            syncFields()
        }
        .confirmationDialog(
            "Remove this host from Orbit?",
            isPresented: $confirmRemoveHost,
            titleVisibility: .visible
        ) {
            if let host = host {
                Button("Remove \(host.displayName)", role: .destructive) {
                    store.removeHost(host.hostId)
                    dismiss()
                    onHostRemoved()
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Running remote agent sessions on the host will not be terminated.")
        }
    }

    private func syncFields() {
        guard let host = host else { return }
        customHostName = host.customName ?? ""
        selectedRouteMode = (host.routeMode == .sshTunnel) ? .auto : host.routeMode
        customDirectEndpoint = host.customDirectEndpoint ?? ""
        relayUrl = host.relayUrl ?? ""
    }

    private func diagnosticsSection(_ host: FfiHostCard) -> some View {
        let daemonSummary: String = {
            var parts: [String] = []
            if let ver = host.daemonVersion, !ver.isEmpty {
                parts.append(ver.hasPrefix("v") ? ver : "v\(ver)")
            }
            if let arch = host.osArch, !arch.isEmpty {
                parts.append(arch)
            }
            return parts.isEmpty ? "—" : parts.joined(separator: " · ")
        }()

        return VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
            Text("Diagnostics")
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, OrbitSpacing.xs)

            VStack(spacing: 0) {
                HStack(spacing: OrbitSpacing.sm) {
                    Circle()
                        .fill(palette.connectionColor(host.connectionState))
                        .frame(width: 7, height: 7)

                    Text(OrbitFormatters.connectionLabel(host.connectionState))
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgPrimary)

                    Spacer(minLength: OrbitSpacing.xs)

                    Button {
                        store.reconnectHost(host.hostId)
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "arrow.clockwise")
                                .font(.system(size: 11, weight: .semibold))
                            Text("Reconnect")
                                .font(OrbitTypography.meta(.semibold))
                        }
                        .foregroundStyle(palette.primary)
                        .padding(.horizontal, 10)
                        .frame(height: 28)
                        .background(palette.subtleFill, in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, OrbitSpacing.md)
                .frame(minHeight: OrbitSpacing.hitTarget)

                OrbitHairline()
                    .padding(.leading, OrbitSpacing.md)

                metricRow(
                    label: "Latency",
                    value: host.lastPingMs.map { "\($0) ms" } ?? "—"
                )

                OrbitHairline()
                    .padding(.leading, OrbitSpacing.md)

                metricRow(label: "Daemon", value: daemonSummary)

                if OrbitFormatters.isHostOnline(host.connectionState) {
                    let upState = store.hostUpgradeStates[host.hostId]
                    let isOutdated = store.isHostDaemonOutdated(host) || store.isHostProtocolBehind(host)

                    OrbitHairline()
                        .padding(.leading, OrbitSpacing.md)

                    HStack(spacing: OrbitSpacing.sm) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Daemon Update")
                                .font(OrbitTypography.subhead(.regular))
                                .foregroundStyle(palette.fgPrimary)
                            if let err = upState?.error, !err.isEmpty {
                                Text(err)
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.danger)
                                    .lineLimit(2)
                            } else if let msg = upState?.statusMessage, !msg.isEmpty {
                                Text(msg)
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.accentSky)
                                    .lineLimit(1)
                            } else {
                                Text(isOutdated ? "Update to v\(store.appShortVersion) available" : "Up to date (v\(store.appShortVersion))")
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(isOutdated ? palette.accentSky : palette.fgMuted)
                            }
                        }

                        Spacer(minLength: OrbitSpacing.xs)

                        if upState?.isUpgrading == true {
                            OrbitSpinnerRingView(color: palette.accentSky, size: 14)
                        } else {
                            Button {
                                store.upgradeHostDaemon(host.hostId)
                            } label: {
                                Text(isOutdated ? "Update Now" : "Re-sync")
                                    .font(OrbitTypography.meta(.semibold))
                                    .foregroundStyle(isOutdated ? palette.onPrimary : palette.fgPrimary)
                                    .padding(.horizontal, 10)
                                    .frame(height: 28)
                                    .background(isOutdated ? palette.accentSky : palette.subtleFill, in: Capsule())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .padding(.vertical, 8)
                }

                OrbitHairline()
                    .padding(.leading, OrbitSpacing.md)

                Button {
                    UIPasteboard.general.string = host.daemonPubkeyB64
                    store.hapticLight()
                    store.showToast("Copied public key")
                } label: {
                    HStack(spacing: OrbitSpacing.sm) {
                        Text("Public Key")
                            .font(OrbitTypography.subhead(.regular))
                            .foregroundStyle(palette.fgPrimary)

                        Spacer(minLength: OrbitSpacing.sm)

                        Text("\(String(host.daemonPubkeyB64.prefix(16)))…")
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(1)

                        Image(systemName: "doc.on.doc")
                            .font(.system(size: 11.5, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(minHeight: OrbitSpacing.hitTarget)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)

                if let err = host.lastError, !err.isEmpty {
                    OrbitHairline()
                        .padding(.leading, OrbitSpacing.md)

                    Text(err)
                        .font(OrbitTypography.code(.regular))
                        .foregroundStyle(palette.danger)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, OrbitSpacing.md)
                        .padding(.vertical, OrbitSpacing.sm)
                }

                if !OrbitFormatters.isHostOnline(host.connectionState),
                   (host.relayUrl ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    OrbitHairline()
                        .padding(.leading, OrbitSpacing.md)

                    Text("No E2EE Relay configured for this host. Configure & share a relay on Orbit Desktop (Settings -> Connection -> Share with All Hosts), then tap Sync in Cloud Sync above to reach it off-LAN.")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.warning)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, OrbitSpacing.md)
                        .padding(.vertical, OrbitSpacing.sm)
                }

                if host.hostId != store.selectedHostId {
                    OrbitHairline()
                        .padding(.leading, OrbitSpacing.md)

                    Button {
                        store.hapticLight()
                        store.selectHost(host.hostId)
                        dismiss()
                    } label: {
                        HStack {
                            Text("Switch to This Host")
                                .font(OrbitTypography.subhead(.medium))
                                .foregroundStyle(palette.primary)
                            Spacer()
                        }
                        .padding(.horizontal, OrbitSpacing.md)
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )
        }
    }

    private func metricRow(label: String, value: String) -> some View {
        HStack {
            Text(label)
                .font(OrbitTypography.subhead(.regular))
                .foregroundStyle(palette.fgPrimary)
            Spacer(minLength: OrbitSpacing.sm)
            Text(value)
                .font(OrbitTypography.code(.regular))
                .foregroundStyle(palette.fgMuted)
                .lineLimit(1)
        }
        .padding(.horizontal, OrbitSpacing.md)
        .frame(minHeight: OrbitSpacing.hitTarget)
    }

    private func connectionSection(_ host: FfiHostCard) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            Text("Connection")
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, OrbitSpacing.xs)

            VStack(spacing: 0) {
                VStack(alignment: .leading, spacing: OrbitSpacing.xs + 2) {
                    Text("Route Mode")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgSecondary)

                    HStack(spacing: 3) {
                        ForEach(selectableRouteModes, id: \.self) { mode in
                            let isSelected = selectedRouteMode == mode
                            Button {
                                store.hapticLight()
                                selectedRouteMode = mode
                            } label: {
                                Text(OrbitFormatters.routeModeLabel(mode))
                                    .font(OrbitTypography.meta(isSelected ? .semibold : .medium))
                                    .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgSecondary)
                                    .frame(maxWidth: .infinity, minHeight: 32)
                                    .background(
                                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                                            .fill(isSelected ? palette.elevated : Color.clear)
                                    )
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                                            .strokeBorder(isSelected ? palette.hairline : Color.clear, lineWidth: 0.5)
                                    )
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(3)
                    .background(palette.subtleFill)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .padding(.horizontal, OrbitSpacing.md)
                .padding(.vertical, OrbitSpacing.md)

                OrbitHairline()
                    .padding(.leading, OrbitSpacing.md)

                groupedInputRow(
                    label: "Display Name",
                    placeholder: host.hostname,
                    text: $customHostName,
                    isMono: false
                )

                OrbitHairline()
                    .padding(.leading, OrbitSpacing.md)

                groupedInputRow(
                    label: "Direct Endpoint",
                    placeholder: host.directEndpoints.first ?? "ws://192.168.1.x:6868",
                    text: $customDirectEndpoint,
                    isMono: true
                )

                OrbitHairline()
                    .padding(.leading, OrbitSpacing.md)

                groupedInputRow(
                    label: "Relay URL",
                    placeholder: "wss://relay.orbit.dev",
                    text: $relayUrl,
                    isMono: true
                )
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )

            Button {
                store.hapticLight()
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
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .padding(.top, OrbitSpacing.xs)

            if store.hosts.count > 1,
               !relayUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                Button {
                    store.hapticLight()
                    store.applyRelayToFleet(relayUrl)
                } label: {
                    Text("Share Relay with All Hosts")
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.primary)
                        .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                        .background(palette.elevated)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .strokeBorder(palette.hairline, lineWidth: 0.5)
                        )
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func groupedInputRow(
        label: String,
        placeholder: String,
        text: Binding<String>,
        isMono: Bool
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(OrbitTypography.meta(.medium))
                .foregroundStyle(palette.fgSecondary)

            TextField(placeholder, text: text)
                .font(isMono ? OrbitTypography.code(.regular) : OrbitTypography.subhead(.regular))
                .foregroundStyle(palette.fgPrimary)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .frame(minHeight: 28)
        }
        .padding(.horizontal, OrbitSpacing.md)
        .padding(.vertical, 10)
    }

    private func dangerSection(_ host: FfiHostCard) -> some View {
        Button {
            store.hapticLight()
            confirmRemoveHost = true
        } label: {
            Text("Remove Host")
                .font(OrbitTypography.subhead(.medium))
                .foregroundStyle(palette.danger)
                .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
