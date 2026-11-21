import SwiftUI

/// Desktop Archived Conversations Recovery sheet — Restore or permanently delete archived threads.
struct ArchivedThreadsSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette

    @State private var searchQuery = ""
    @State private var confirmDeleteAgent: FfiAgentCard? = nil

    private var archivedAgents: [FfiAgentCard] {
        let all = store.currentHostDetail?.archivedAgents ?? []
        let q = searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return all }
        return all.filter { ag in
            ag.title.lowercased().contains(q)
                || ag.cwd.lowercased().contains(q)
                || (ag.handle?.lowercased().contains(q) ?? false)
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

                if archivedAgents.isEmpty {
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
                            ForEach(archivedAgents, id: \.agentId) { agent in
                                archivedCard(agent)
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
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
            .confirmationDialog(
                "Permanently delete conversation?",
                isPresented: Binding(
                    get: { confirmDeleteAgent != nil },
                    set: { if !$0 { confirmDeleteAgent = nil } }
                ),
                titleVisibility: .visible
            ) {
                if let agent = confirmDeleteAgent {
                    Button("Delete Forever", role: .destructive) {
                        store.deleteAgentPermanently(agent.agentId)
                        confirmDeleteAgent = nil
                    }
                }
                Button("Cancel", role: .cancel) { confirmDeleteAgent = nil }
            } message: {
                Text("This permanently removes the session history from the host daemon and cannot be undone.")
            }
        }
    }

    private func archivedCard(_ agent: FfiAgentCard) -> some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(alignment: .top, spacing: 8) {
                OrbitProviderIcon(provider: agent.provider, size: 14)
                    .padding(.top, 2)

                VStack(alignment: .leading, spacing: 2) {
                    Text(agent.title.isEmpty ? "Untitled Session" : agent.title)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(2)

                    Text("\(OrbitFormatters.shortPath(agent.cwd)) · \(OrbitFormatters.relativeTimeFromRfc3339(agent.archivedAtRfc3339 ?? agent.updatedAtRfc3339))")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                }

                Spacer(minLength: 4)
            }

            HStack(spacing: OrbitSpacing.sm) {
                Button {
                    store.unarchiveAgent(agent.agentId)
                    dismiss()
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) {
                        store.selectAgent(agent.agentId)
                    }
                } label: {
                    Label("Restore", systemImage: "arrow.uturn.backward")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.onPrimary)
                        .padding(.horizontal, 12)
                        .frame(height: 30)
                        .background(palette.primary, in: Capsule())
                }
                .buttonStyle(.plain)

                Spacer()

                Button {
                    confirmDeleteAgent = agent
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
