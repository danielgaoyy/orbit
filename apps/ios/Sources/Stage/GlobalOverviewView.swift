import SwiftUI

/// Personal Agent Global Overview — *Orbital Quiet Precision* humanized fleet triage screen:
/// 1. 1-line calm status header (no SaaS KPI dashboard bloat)
/// 2. Conditional "Needs Your Decision" queue (low-risk batch approve, never blind-approving Plan/High-Risk)
/// 3. Live Fleet Sessions across all paired hosts
/// 4. Docked Personal Agent prompt bar with Codex-style single right circle (Voice <-> Send)
struct GlobalOverviewView: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.orbitPalette) private var palette
    @StateObject private var voiceController = OrbitVoiceDictationController()
    @FocusState private var isPromptFocused: Bool

    private var onlineHostCount: Int {
        store.hosts.filter { OrbitFormatters.isHostOnline($0.connectionState) }.count
    }

    private var liveSessions: [(host: FfiHostCard, agent: FfiAgentCard)] {
        store.liveFleetSessions
    }

    private var runningCount: Int {
        liveSessions.filter { $0.agent.isForegroundRunning }.count
    }

    private var pendingDecisions: [OrbitGlobalPendingDecision] {
        store.globalPendingDecisions
    }

    private var safeBatchDecisions: [OrbitGlobalPendingDecision] {
        pendingDecisions.filter(\.isSafeForBatchAllow)
    }

    private var hasPlanOrHighRisk: Bool {
        pendingDecisions.contains { !$0.isSafeForBatchAllow }
    }

    private var calmStatusLine: String {
        var parts: [String] = []
        if !pendingDecisions.isEmpty {
            parts.append(OrbitL10n.fmt("%d waiting", pendingDecisions.count))
        }
        if runningCount > 0 {
            parts.append(OrbitL10n.fmt("%d running", runningCount))
        }
        if onlineHostCount > 0 {
            parts.append(OrbitL10n.fmt(onlineHostCount == 1 ? "%d host online" : "%d hosts online", onlineHostCount))
        }
        if parts.isEmpty {
            return OrbitL10n.tr("All clear")
        }
        return parts.joined(separator: " · ")
    }

    private var statusAccentColor: Color {
        if !pendingDecisions.isEmpty { return palette.warning }
        if runningCount > 0 { return palette.direct }
        return onlineHostCount > 0 ? palette.direct : palette.fgMuted
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.xl) {
                    calmHeaderSection

                    if !pendingDecisions.isEmpty {
                        needsDecisionSection
                    }

                    liveFleetSection
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.top, OrbitSpacing.lg)
                .padding(.bottom, OrbitSpacing.xl)
            }
            .orbitSoftScrollEdgeEffect()
            .scrollDismissesKeyboard(.interactively)
            .refreshable {
                store.refreshRouterAndMesh()
                if let hid = store.selectedHostId {
                    store.reloadHostDetail(hid)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(palette.canvas.ignoresSafeArea())
        .safeAreaInset(edge: .bottom, spacing: 0) {
            personalAgentPromptDock
        }
        .onAppear {
            store.refreshRouterAndMesh()
        }
        .onDisappear {
            if voiceController.isRecording {
                voiceController.stopRecording(discard: false)
            }
        }
    }

    // MARK: - 1. One-Line Calm Status Header

    private var calmHeaderSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(alignment: .center, spacing: OrbitSpacing.sm) {
                if runningCount > 0 || !pendingDecisions.isEmpty {
                    PulsingDotView(color: statusAccentColor, size: 7)
                } else {
                    Circle()
                        .fill(statusAccentColor)
                        .frame(width: 7, height: 7)
                }

                Text(calmStatusLine)
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgSecondary)
                    .lineLimit(1)

                Spacer(minLength: OrbitSpacing.xs)

                Button {
                    store.hapticLight()
                    store.activeSheet = .globalSearch
                } label: {
                    Image(systemName: "magnifyingglass")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 34, height: 34)
                        .background(palette.subtleFill, in: Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(OrbitL10n.tr("Global Search"))

                Button {
                    store.hapticLight()
                    store.activeSheet = .personalRouter
                } label: {
                    Image(systemName: "point.3.connected.trianglepath.dotted")
                        .font(OrbitTypography.iconSmall)
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 34, height: 34)
                        .background(palette.subtleFill, in: Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(OrbitL10n.tr("Fleet Mesh & Personal Agent"))
            }

            Text(OrbitL10n.tr("Overview"))
                .font(.system(size: 24, weight: .semibold, design: .default))
                .tracking(-0.3)
                .foregroundStyle(palette.fgPrimary)
        }
    }

    // MARK: - 2. Conditional "Needs Your Decision" Queue

    private var needsDecisionSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                Text(OrbitL10n.tr("Needs Your Decision"))
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)

                Text("\(pendingDecisions.count)")
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.warning)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 1.5)
                    .background(palette.warning.opacity(0.14), in: Capsule())

                Spacer(minLength: 4)

                if !safeBatchDecisions.isEmpty {
                    Button {
                        store.approveLowRiskBatch(decisions: safeBatchDecisions)
                    } label: {
                        Text(OrbitL10n.fmt("Approve %d Low-Risk", safeBatchDecisions.count))
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.onPrimary)
                            .padding(.horizontal, 11)
                            .frame(height: 28)
                            .background(palette.primary, in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 2)

            VStack(spacing: 0) {
                ForEach(Array(pendingDecisions.enumerated()), id: \.element.id) { idx, item in
                    if idx > 0 {
                        OrbitHairline().padding(.leading, OrbitSpacing.md)
                    }
                    decisionRow(item)
                }
            }
            .background(palette.elevated)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(palette.warning.opacity(0.30), lineWidth: 0.5)
            )

            if hasPlanOrHighRisk {
                Text(OrbitL10n.tr("Plans & high-risk actions always require individual review."))
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .padding(.horizontal, 2)
            }
        }
    }

    private func decisionRow(_ item: OrbitGlobalPendingDecision) -> some View {
        Button {
            store.hapticLight()
            store.selectAgent(hostId: item.hostId, agentId: item.agentId)
        } label: {
            HStack(alignment: .top, spacing: OrbitSpacing.md) {
                Image(systemName: item.isPlanApproval
                      ? "list.bullet.clipboard"
                      : (item.isQuestion ? "questionmark.bubble" : "exclamationmark.shield"))
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(item.isSafeForBatchAllow ? palette.fgSecondary : palette.warning)
                    .frame(width: 22, height: 22)
                    .padding(.top, 2)

                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        Text(item.title.isEmpty ? OrbitL10n.tr("Approval Required") : item.title)
                            .font(OrbitTypography.subhead(.semibold))
                            .foregroundStyle(palette.fgPrimary)
                            .lineLimit(1)

                        riskBadge(item)
                    }

                    if !item.commandOrPath.isEmpty {
                        Text(item.commandOrPath)
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(palette.fgSecondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }

                    Text("\(item.hostName) · \(item.agentTitle)")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                }

                Spacer(minLength: 4)

                Text(OrbitL10n.tr("Review"))
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                    .padding(.horizontal, 10)
                    .frame(height: 28)
                    .background(palette.subtleFill, in: Capsule())
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.vertical, OrbitSpacing.md)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func riskBadge(_ item: OrbitGlobalPendingDecision) -> some View {
        let label: String
        let color: Color
        if item.isPlanApproval {
            label = OrbitL10n.tr("Plan Review")
            color = palette.accentSky
        } else if item.isQuestion {
            label = OrbitL10n.tr("Input Requested")
            color = palette.accentViolet
        } else if !item.isSafeForBatchAllow {
            label = OrbitL10n.tr("High Risk")
            color = palette.danger
        } else {
            label = OrbitL10n.tr("Low Risk")
            color = palette.direct
        }
        return Text(label)
            .font(.system(size: 10.5, weight: .semibold))
            .foregroundStyle(color)
            .padding(.horizontal, 6)
            .padding(.vertical, 1.5)
            .background(color.opacity(0.14), in: Capsule())
    }

    // MARK: - 3. Live Fleet Sessions

    private var liveFleetSection: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack {
                Text(OrbitL10n.tr("Live Fleet Sessions"))
                    .font(OrbitTypography.meta(.semibold))
                    .foregroundStyle(palette.fgMuted)

                Spacer()

                Button {
                    store.hapticLight()
                    store.startNewSessionDraft()
                } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "plus")
                            .font(.system(size: 11, weight: .semibold))
                        Text(OrbitL10n.tr("Start New Session"))
                            .font(OrbitTypography.meta(.semibold))
                    }
                    .foregroundStyle(palette.fgPrimary)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 2)

            if liveSessions.isEmpty {
                VStack(spacing: OrbitSpacing.sm) {
                    Text(OrbitL10n.tr("No active sessions right now"))
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgSecondary)
                    Text(OrbitL10n.tr("Ask your Personal Agent…"))
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, OrbitSpacing.xl)
                .background(palette.elevated)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(liveSessions.prefix(12).enumerated()), id: \.element.agent.agentId) { idx, pair in
                        if idx > 0 {
                            OrbitHairline().padding(.leading, 44)
                        }
                        fleetSessionRow(host: pair.host, agent: pair.agent)
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

    private func fleetSessionRow(host: FfiHostCard, agent: FfiAgentCard) -> some View {
        Button {
            store.hapticLight()
            store.selectAgent(hostId: host.hostId, agentId: agent.agentId)
        } label: {
            HStack(spacing: OrbitSpacing.md) {
                OrbitProviderIcon(provider: agent.provider, size: 16)
                    .frame(width: 20)

                VStack(alignment: .leading, spacing: 2) {
                    Text(agent.title.isEmpty ? OrbitL10n.tr("New Conversation") : agent.title)
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    HStack(spacing: 5) {
                        Text(host.displayName)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(1)
                        let cleanCwd = agent.cwd.trimmingCharacters(in: .whitespacesAndNewlines)
                        if !cleanCwd.isEmpty && cleanCwd != "~" {
                            Text("·")
                                .foregroundStyle(palette.fgFaint)
                            Text(OrbitFormatters.shortPath(cleanCwd))
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .lineLimit(1)
                        }
                    }
                }

                Spacer(minLength: 4)

                if agent.isAwaitingPermission {
                    Circle()
                        .fill(palette.warning)
                        .frame(width: 7, height: 7)
                } else if agent.isForegroundRunning {
                    OrbitSpinnerRingView(color: palette.fgSecondary, size: 12)
                } else {
                    Text(OrbitFormatters.relativeUpdatedTime(rfc3339: agent.updatedAtRfc3339))
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                }
            }
            .padding(.horizontal, OrbitSpacing.md)
            .padding(.vertical, 11)
            .frame(minHeight: 52)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    // MARK: - 4. Docked Personal Agent Prompt Bar (Codex-Style Single Right Circle)

    private var canSubmitOverviewPrompt: Bool {
        !store.personalAgentDraftPrompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var personalAgentPromptDock: some View {
        VStack(spacing: 0) {
            if voiceController.isRecording {
                OrbitVoiceRecordingStrip(controller: voiceController) {
                    store.hapticLight()
                    voiceController.stopRecording(discard: true)
                }
                .padding(.bottom, OrbitSpacing.xs)
                .transition(.move(edge: .bottom).combined(with: .opacity))
            }

            HStack(spacing: OrbitSpacing.sm) {
                TextField(
                    "",
                    text: $store.personalAgentDraftPrompt,
                    prompt: Text(OrbitL10n.tr("Ask your Personal Agent…"))
                        .foregroundColor(palette.fgMuted.opacity(0.7)),
                    axis: .vertical
                )
                .font(OrbitTypography.body(.regular))
                .foregroundStyle(palette.fgPrimary)
                .lineLimit(1...4)
                .focused($isPromptFocused)
                .padding(.leading, OrbitSpacing.md)
                .padding(.vertical, 10)

                // SINGLE stationary right circle: Recording -> Stop | Empty -> Mic | Text -> Send
                if voiceController.isRecording {
                    Button {
                        store.hapticMedium()
                        voiceController.stopRecording(discard: false)
                    } label: {
                        ZStack {
                            Circle()
                                .fill(palette.danger)
                                .frame(width: 32, height: 32)
                            RoundedRectangle(cornerRadius: 2.5, style: .continuous)
                                .fill(Color.white)
                                .frame(width: 10, height: 10)
                        }
                        .frame(width: 38, height: 38)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(OrbitL10n.tr("Stop Recording"))
                } else if canSubmitOverviewPrompt {
                    Button {
                        isPromptFocused = false
                        store.submitPersonalAgentOverviewPrompt(store.personalAgentDraftPrompt)
                    } label: {
                        ZStack {
                            Circle()
                                .fill(palette.primary)
                                .frame(width: 32, height: 32)
                            Image(systemName: "arrow.up")
                                .font(.system(size: 14, weight: .semibold))
                                .foregroundStyle(palette.onPrimary)
                        }
                        .frame(width: 38, height: 38)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(OrbitL10n.tr("Send Prompt"))
                } else {
                    Button {
                        isPromptFocused = false
                        store.hapticLight()
                        voiceController.toggleRecording(
                            currentDraft: store.personalAgentDraftPrompt,
                            onTranscriptChanged: { updated in
                                store.personalAgentDraftPrompt = updated
                            },
                            onPermissionDenied: { msg in
                                store.showToast(msg)
                            }
                        )
                    } label: {
                        ZStack {
                            Circle()
                                .fill(palette.subtleFill)
                                .frame(width: 32, height: 32)
                            Image(systemName: "mic.fill")
                                .font(.system(size: 13.5, weight: .semibold))
                                .foregroundStyle(palette.fgPrimary)
                        }
                        .frame(width: 38, height: 38)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(OrbitL10n.tr("Voice Input"))
                }
            }
            .padding(.trailing, 6)
            .orbitGlassSurface(
                in: RoundedRectangle(cornerRadius: 24, style: .continuous),
                fallbackFill: palette.elevated,
                fallbackBorder: palette.borderDefault,
                fallbackShadowOpacity: palette.isDark ? 0.22 : 0.06
            )
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.top, OrbitSpacing.sm)
            .padding(.bottom, OrbitSpacing.sm)
        }
    }
}
