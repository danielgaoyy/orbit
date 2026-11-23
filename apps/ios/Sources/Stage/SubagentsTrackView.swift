import SwiftUI

/// Approval banner surfaced whenever same-host managed subagents are waiting for permission.
struct SubagentApprovalBanner: View {
    @ObservedObject var store: OrbitIosStore
    @Environment(\.orbitPalette) private var palette

    private var pendingChildren: [FfiAgentCard] {
        store.activePendingSubagentApprovals
    }

    var body: some View {
        if !pendingChildren.isEmpty && !store.isDraftNewSession {
            VStack(spacing: 6) {
                ForEach(pendingChildren, id: \.agentId) { child in
                    HStack(spacing: OrbitSpacing.sm) {
                        Circle()
                            .fill(palette.warning)
                            .frame(width: 7, height: 7)

                        VStack(alignment: .leading, spacing: 1) {
                            Text(child.title.isEmpty ? "Subagent awaiting approval" : child.title)
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(palette.fgPrimary)
                                .lineLimit(1)
                            Text("\(child.providerLabel) · Needs permission")
                                .font(.system(size: 11, weight: .regular))
                                .foregroundStyle(palette.warning)
                                .lineLimit(1)
                        }

                        Spacer(minLength: 4)

                        Button {
                            store.subagentInspectorTarget = OrbitSubagentInspectorTarget(
                                hostId: store.selectedHostId ?? "",
                                parentAgentId: store.selectedAgentId ?? "",
                                childAgentId: child.agentId,
                                title: child.title.isEmpty ? "Subagent" : child.title
                            )
                        } label: {
                            Text("Inspect")
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(palette.fgPrimary)
                                .padding(.horizontal, 10)
                                .frame(height: 26)
                                .background(palette.subtleFill, in: Capsule())
                        }
                        .buttonStyle(.plain)

                        Button {
                            if let hid = store.selectedHostId {
                                store.selectAgent(hostId: hid, agentId: child.agentId)
                            }
                        } label: {
                            Text("Open")
                                .font(OrbitTypography.meta(.semibold))
                                .foregroundStyle(palette.warning)
                                .padding(.horizontal, 10)
                                .frame(height: 26)
                                .background(palette.warning.opacity(0.14), in: Capsule())
                        }
                        .buttonStyle(.plain)
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .padding(.vertical, 8)
                    .background(palette.warning.opacity(0.08))
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .strokeBorder(palette.warning.opacity(0.35), lineWidth: 0.5)
                    )
                }
            }
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.top, 4)
        }
    }
}

/// Structured detail sheet for inspecting either a managed child agent or provider native subagent timeline.
struct SubagentInspectorSheet: View {
    @ObservedObject var store: OrbitIosStore
    let target: OrbitSubagentInspectorTarget

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    private var managedChild: FfiAgentCard? {
        guard let childId = target.childAgentId else { return nil }
        return store.detail(forHostId: target.hostId)?.agents.first(where: { $0.agentId == childId })
    }

    private var nativeEvents: [FfiProviderSubagentEvent] {
        guard let subId = target.providerSubagentId else { return [] }
        return store.providerSubagentEvents(parentAgentId: target.parentAgentId, subagentId: subId)
    }

    var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.md) {
                    if let child = managedChild {
                        HStack(spacing: OrbitSpacing.sm) {
                            OrbitProviderIcon(provider: child.provider, size: 16)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(child.title.isEmpty ? "Subagent" : child.title)
                                    .font(OrbitTypography.subhead(.semibold))
                                    .foregroundStyle(palette.fgPrimary)
                                Text("\(child.providerLabel) · \(child.statusLabel)")
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(child.isAwaitingPermission ? palette.warning : palette.fgMuted)
                            }
                            Spacer()
                            Button("Open Thread") {
                                dismiss()
                                store.selectAgent(hostId: target.hostId, agentId: child.agentId)
                            }
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.primary)
                        }
                        .padding(OrbitSpacing.md)
                        .background(palette.elevated)
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                    } else if let subId = target.providerSubagentId {
                        if nativeEvents.isEmpty {
                            Text("No recorded subagent events yet for \(subId).")
                                .font(OrbitTypography.meta(.regular))
                                .foregroundStyle(palette.fgMuted)
                                .padding(OrbitSpacing.md)
                        } else {
                            ForEach(nativeEvents, id: \.seq) { ev in
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(ev.summaryText)
                                        .font(OrbitTypography.subhead(.regular))
                                        .foregroundStyle(palette.fgPrimary)
                                    Text(ev.timestampRfc3339)
                                        .font(OrbitTypography.meta(.regular))
                                        .foregroundStyle(palette.fgMuted)
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(OrbitSpacing.md)
                                .background(palette.elevated)
                                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                            }
                        }
                    }
                }
                .padding(OrbitSpacing.lg)
            }
            .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
            .navigationTitle(target.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
            .onAppear {
                if let subId = target.providerSubagentId {
                    store.openProviderSubagentTimeline(parentAgentId: target.parentAgentId, subagentId: subId)
                }
            }
        }
    }
}

/// Floating Liquid Glass Subagents pill docked above the composer + detail sheet
/// for both Orbit Managed Subagents and Provider Native Subagents.
struct SubagentsTrackPill: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.orbitPalette) private var palette
    @State private var isSheetPresented = false

    private var managed: [FfiAgentCard] { store.visibleManagedSubagents }
    private var native: [FfiProviderSubagentDescriptor] { store.visibleProviderSubagents }
    private var totalCount: Int { managed.count + native.count }

    private var runningCount: Int {
        managed.filter { $0.isForegroundRunning }.count
            + native.filter { $0.status == .running }.count
    }

    private var awaitingCount: Int {
        managed.filter { $0.isAwaitingPermission }.count
    }

    var body: some View {
        if totalCount > 0 && !store.isDraftNewSession {
            HStack {
                Button {
                    store.hapticLight()
                    if let aid = store.selectedAgentId, let hid = store.selectedHostId {
                        store.openProviderSubagentTimelineIfNeeded(parentId: aid, hostId: hid)
                    }
                    isSheetPresented = true
                } label: {
                    HStack(spacing: 6) {
                        if awaitingCount > 0 {
                            Circle()
                                .fill(palette.warning)
                                .frame(width: 6.5, height: 6.5)
                        } else if runningCount > 0 {
                            OrbitSpinnerRingView(color: palette.primary, size: 11.5)
                        } else {
                            Image(systemName: "square.stack.3d.up")
                                .font(.system(size: 11.5, weight: .semibold))
                                .foregroundStyle(palette.fgSecondary)
                        }

                        Text("Subagents · \(totalCount)")
                            .font(OrbitTypography.meta(.semibold))
                            .foregroundStyle(palette.fgPrimary)

                        if awaitingCount > 0 {
                            Text("\(awaitingCount) waiting")
                                .font(.system(size: 11, weight: .semibold))
                                .foregroundStyle(palette.warning)
                        } else if runningCount > 0 {
                            Text("\(runningCount) running")
                                .font(.system(size: 11, weight: .medium))
                                .foregroundStyle(palette.fgSecondary)
                        }

                        Image(systemName: "chevron.up")
                            .font(.system(size: 9.5, weight: .semibold))
                            .foregroundStyle(palette.fgMuted)
                    }
                    .padding(.horizontal, 12)
                    .frame(height: 30)
                    .orbitGlassSurface(
                        in: Capsule(),
                        interactive: true,
                        fallbackFill: palette.elevated,
                        fallbackBorder: awaitingCount > 0 ? palette.warning.opacity(0.45) : palette.hairline,
                        fallbackShadowOpacity: 0.08
                    )
                }
                .buttonStyle(.plain)

                Spacer()
            }
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.top, 4)
            .sheet(isPresented: $isSheetPresented) {
                SubagentsListSheet(store: store)
                    .environment(\.orbitPalette, palette)
                    .presentationDetents([.medium, .large])
                    .presentationDragIndicator(.visible)
            }
        }
    }
}

private extension OrbitIosStore {
    func openProviderSubagentTimelineIfNeeded(parentId: String, hostId: String) {
        if let first = visibleProviderSubagents.first {
            openProviderSubagentTimeline(parentAgentId: parentId, subagentId: first.subagentId)
        }
    }
}

struct SubagentsListSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette
    @State private var expandedNativeIds: Set<String> = []
    @State private var localInspectorTarget: OrbitSubagentInspectorTarget? = nil

    private var managed: [FfiAgentCard] { store.visibleManagedSubagents }
    private var native: [FfiProviderSubagentDescriptor] { store.visibleProviderSubagents }

    private var hasFinished: Bool {
        managed.contains { !$0.isForegroundRunning && !$0.isAwaitingPermission }
            || native.contains { $0.status != .running }
    }

    var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: true) {
                VStack(alignment: .leading, spacing: OrbitSpacing.lg) {
                    if managed.isEmpty && native.isEmpty {
                        VStack(spacing: OrbitSpacing.sm) {
                            Image(systemName: "square.stack.3d.up")
                                .font(.system(size: 24, weight: .medium))
                                .foregroundStyle(palette.fgMuted)
                            Text("No active subagents")
                                .font(OrbitTypography.subhead(.medium))
                                .foregroundStyle(palette.fgSecondary)
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 40)
                    } else {
                        if !managed.isEmpty {
                            sectionBlock(title: "Orbit Managed Subagents (\(managed.count))") {
                                ForEach(managed, id: \.agentId) { child in
                                    managedSubagentCard(child)
                                }
                            }
                        }

                        if !native.isEmpty {
                            sectionBlock(title: "Provider Native Subagents (\(native.count))") {
                                ForEach(native, id: \.subagentId) { sub in
                                    nativeSubagentCard(sub)
                                }
                            }
                        }
                    }
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.md)
            }
            .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
            .navigationTitle("Subagents")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if hasFinished {
                    ToolbarItem(placement: .topBarLeading) {
                        Button("Archive Finished") {
                            store.archiveFinishedSubagents()
                        }
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.fgSecondary)
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
            .sheet(item: $localInspectorTarget) { target in
                SubagentInspectorSheet(store: store, target: target)
                    .environment(\.orbitPalette, palette)
                    .presentationDetents([.medium, .large])
                    .presentationDragIndicator(.visible)
            }
        }
    }

    private func sectionBlock<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            Text(title)
                .font(OrbitTypography.meta(.semibold))
                .foregroundStyle(palette.fgMuted)
                .padding(.horizontal, 4)

            VStack(spacing: OrbitSpacing.sm) {
                content()
            }
        }
    }

    private func managedSubagentCard(_ child: FfiAgentCard) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                if child.isAwaitingPermission {
                    Circle().fill(palette.warning).frame(width: 7, height: 7)
                } else if child.isForegroundRunning {
                    OrbitSpinnerRingView(color: palette.primary, size: 12.5)
                } else {
                    OrbitProviderIcon(provider: child.provider, size: 14)
                }

                VStack(alignment: .leading, spacing: 2) {
                    Text(child.title.isEmpty ? "Subagent" : child.title)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    Text("\(child.providerLabel) · \(child.statusLabel)")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(child.isAwaitingPermission ? palette.warning : palette.fgMuted)
                        .lineLimit(1)
                }

                Spacer(minLength: 4)

                Button {
                    localInspectorTarget = OrbitSubagentInspectorTarget(
                        hostId: store.selectedHostId ?? "",
                        parentAgentId: store.selectedAgentId ?? "",
                        childAgentId: child.agentId,
                        title: child.title.isEmpty ? "Subagent" : child.title
                    )
                } label: {
                    Text("Inspect")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.fgSecondary)
                        .padding(.horizontal, 10)
                        .frame(height: 28)
                        .background(palette.subtleFill, in: Capsule())
                }
                .buttonStyle(.plain)

                Button {
                    dismiss()
                    store.selectAgent(child.agentId)
                } label: {
                    Text("Open")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.primary)
                        .padding(.horizontal, 10)
                        .frame(height: 28)
                        .background(palette.subtleFill, in: Capsule())
                }
                .buttonStyle(.plain)
            }

            HStack(spacing: OrbitSpacing.sm) {
                Button {
                    store.detachAgent(child.agentId)
                } label: {
                    Label("Detach", systemImage: "arrow.up.right.square")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.fgSecondary)
                }
                .buttonStyle(.plain)

                Spacer()

                Button {
                    store.archiveManagedSubagent(child.agentId)
                } label: {
                    Label("Archive", systemImage: "archivebox")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.danger)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(OrbitSpacing.md)
        .background(palette.elevated)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(child.isAwaitingPermission ? palette.warning.opacity(0.4) : palette.hairline, lineWidth: 0.5)
        )
    }

    private func nativeSubagentCard(_ sub: FfiProviderSubagentDescriptor) -> some View {
        let isExpanded = expandedNativeIds.contains(sub.subagentId)
        let events = store.providerSubagentEvents(
            parentAgentId: sub.parentAgentId,
            subagentId: sub.subagentId
        )

        return VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                statusBadge(sub.status)

                VStack(alignment: .leading, spacing: 2) {
                    Text(sub.title.isEmpty ? sub.subagentId : sub.title)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    if let subtitle = sub.subtitle, !subtitle.isEmpty {
                        Text(subtitle)
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(1)
                    }
                }

                Spacer(minLength: 4)

                Button {
                    store.hapticLight()
                    withAnimation(.easeInOut(duration: 0.18)) {
                        if isExpanded {
                            expandedNativeIds.remove(sub.subagentId)
                        } else {
                            expandedNativeIds.insert(sub.subagentId)
                            store.openProviderSubagentTimeline(
                                parentAgentId: sub.parentAgentId,
                                subagentId: sub.subagentId
                            )
                        }
                    }
                } label: {
                    Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(palette.fgMuted)
                        .frame(width: 28, height: 28)
                        .background(palette.subtleFill, in: Circle())
                }
                .buttonStyle(.plain)

                if sub.status != .running {
                    Button {
                        store.archiveNativeSubagent(sub.subagentId)
                    } label: {
                        Image(systemName: "archivebox")
                            .font(.system(size: 11.5, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: 28, height: 28)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Archive Native Subagent")
                }
            }

            if isExpanded {
                OrbitHairline()
                if events.isEmpty {
                    Text("No live events captured yet.")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                } else {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(Array(events.suffix(12)), id: \.seq) { ev in
                            HStack(alignment: .top, spacing: 6) {
                                Text("#\(ev.seq)")
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.fgMuted)
                                Text(ev.summaryText)
                                    .font(OrbitTypography.meta(.regular))
                                    .foregroundStyle(palette.fgSecondary)
                                    .lineLimit(3)
                            }
                        }
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

    @ViewBuilder
    private func statusBadge(_ status: FfiProviderSubagentStatus) -> some View {
        switch status {
        case .running:
            OrbitSpinnerRingView(color: palette.primary, size: 12.5)
        case .completed:
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 13))
                .foregroundStyle(palette.direct)
        case .failed:
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 13))
                .foregroundStyle(palette.danger)
        case .canceled:
            Image(systemName: "minus.circle")
                .font(.system(size: 13))
                .foregroundStyle(palette.fgMuted)
        }
    }
}
