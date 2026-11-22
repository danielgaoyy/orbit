import SwiftUI

/// Desktop Archived Conversations Recovery sheet — Restore or permanently delete archived threads.
struct ArchivedThreadsSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var searchQuery = ""
    @State private var confirmDeleteItem: (host: FfiHostCard, agent: FfiAgentCard)? = nil
    @State private var confirmEmptyArchive = false

    private var archivedItems: [(host: FfiHostCard, agent: FfiAgentCard)] {
        let all = store.allArchivedAgentsAcrossHosts
        let q = searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return all }
        return all.filter { item in
            item.agent.title.lowercased().contains(q)
                || item.agent.cwd.lowercased().contains(q)
                || (item.agent.handle?.lowercased().contains(q) ?? false)
                || item.host.displayName.lowercased().contains(q)
        }
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass")
                        .font(.system(size: 13))
                        .foregroundStyle(palette.fgMuted)
                    TextField("Search archived conversations…", text: $searchQuery)
                        .font(OrbitTypography.subhead(.regular))
                        .foregroundStyle(palette.fgPrimary)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                }
                .padding(.horizontal, 12)
                .frame(height: 36)
                .background(palette.subtleFill, in: Capsule())
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.sm)

                OrbitHairline()

                if archivedItems.isEmpty {
                    VStack(spacing: OrbitSpacing.sm) {
                        Image(systemName: "archivebox")
                            .font(.system(size: 26, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                        Text(searchQuery.isEmpty ? "No Archived Conversations" : "No Matching Archived Conversations")
                            .font(OrbitTypography.subhead(.medium))
                            .foregroundStyle(palette.fgSecondary)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ScrollView(.vertical, showsIndicators: true) {
                        LazyVStack(spacing: OrbitSpacing.sm) {
                            ForEach(archivedItems, id: \.agent.agentId) { item in
                                archivedCard(host: item.host, agent: item.agent)
                            }
                        }
                        .padding(.horizontal, OrbitSpacing.lg)
                        .padding(.vertical, OrbitSpacing.md)
                    }
                }
            }
            .background((palette.isDark ? palette.canvas : palette.drawer).ignoresSafeArea())
            .navigationTitle("Archived Conversations")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if !archivedItems.isEmpty {
                    ToolbarItem(placement: .topBarLeading) {
                        Button("Empty Archive", role: .destructive) {
                            confirmEmptyArchive = true
                        }
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.danger)
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
            .confirmationDialog(
                "Empty archive across all hosts?",
                isPresented: $confirmEmptyArchive,
                titleVisibility: .visible
            ) {
                Button("Delete All Archived", role: .destructive) {
                    store.emptyArchive()
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("This permanently removes all archived session histories and cannot be undone.")
            }
            .confirmationDialog(
                "Permanently delete conversation?",
                isPresented: Binding(
                    get: { confirmDeleteItem != nil },
                    set: { if !$0 { confirmDeleteItem = nil } }
                ),
                titleVisibility: .visible
            ) {
                if let item = confirmDeleteItem {
                    Button("Delete Forever", role: .destructive) {
                        store.deleteAgentPermanently(item.agent.agentId, hostId: item.host.hostId)
                        confirmDeleteItem = nil
                    }
                }
                Button("Cancel", role: .cancel) { confirmDeleteItem = nil }
            } message: {
                Text("This permanently removes the session history from the host daemon and cannot be undone.")
            }
        }
    }

    private func archivedCard(host: FfiHostCard, agent: FfiAgentCard) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(alignment: .top, spacing: 8) {
                OrbitProviderIcon(provider: agent.provider, size: 14)
                    .padding(.top, 2)

                VStack(alignment: .leading, spacing: 2) {
                    Text(agent.title.isEmpty ? "Untitled Session" : agent.title)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(2)

                    let hostPrefix = store.hosts.count > 1 ? "\(host.displayName) · " : ""
                    Text("\(hostPrefix)\(OrbitFormatters.shortPath(agent.cwd)) · \(OrbitFormatters.relativeTimeFromRfc3339(agent.archivedAtRfc3339 ?? agent.updatedAtRfc3339))")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                }

                Spacer(minLength: 4)
            }

            HStack(spacing: OrbitSpacing.sm) {
                Button {
                    store.unarchiveAgent(agent.agentId, hostId: host.hostId)
                } label: {
                    Label("Restore", systemImage: "arrow.uturn.backward")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.onPrimary)
                        .padding(.horizontal, 12)
                        .frame(height: 30)
                        .background(palette.primary, in: Capsule())
                }
                .buttonStyle(.plain)

                Button {
                    store.unarchiveAgent(agent.agentId, hostId: host.hostId)
                    dismiss()
                    store.selectAgent(hostId: host.hostId, agentId: agent.agentId)
                } label: {
                    Text("Restore & Open")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.primary)
                        .padding(.horizontal, 10)
                        .frame(height: 30)
                        .background(palette.subtleFill, in: Capsule())
                }
                .buttonStyle(.plain)

                Spacer()

                Button {
                    confirmDeleteItem = (host: host, agent: agent)
                } label: {
                    Label("Delete", systemImage: "trash")
                        .font(OrbitTypography.meta(.medium))
                        .foregroundStyle(palette.danger)
                        .padding(.horizontal, 10)
                        .frame(height: 30)
                        .background(palette.danger.opacity(0.10), in: Capsule())
                }
                .buttonStyle(.plain)
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
