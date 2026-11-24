import SwiftUI

/// Global Cross-Host FTS5 + Remote Transcript Search sheet (`searchMessagesRemote` + local fallback).
struct GlobalSearchSheet: View {
    @ObservedObject var store: OrbitIosStore

    @Environment(\.dismiss) private var dismiss
    @Environment(\.orbitPalette) private var palette
    @State private var queryText: String = ""
    @State private var selectedHostFilter: String? = nil
    @State private var debounceTask: Task<Void, Never>? = nil
    @FocusState private var isSearchFieldFocused: Bool

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                searchHeaderBar
                OrbitHairline()
                resultsContent
            }
            .background(palette.canvas.ignoresSafeArea())
            .navigationTitle(OrbitL10n.tr("Global Search"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(OrbitL10n.tr("Done")) { dismiss() }
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.primary)
                }
            }
            .onAppear {
                if !store.globalSearchQuery.isEmpty {
                    queryText = store.globalSearchQuery
                } else if !store.drawerSearchQuery.isEmpty {
                    queryText = store.drawerSearchQuery
                    triggerSearch(store.drawerSearchQuery)
                }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                    isSearchFieldFocused = true
                }
            }
        }
    }

    private var searchHeaderBar: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            HStack(spacing: OrbitSpacing.sm) {
                Image(systemName: "magnifyingglass")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(palette.fgMuted)

                TextField(
                    "",
                    text: $queryText,
                    prompt: Text(OrbitL10n.tr("Search all transcripts…"))
                        .foregroundColor(palette.fgMuted.opacity(0.7))
                )
                .font(OrbitTypography.body(.regular))
                .foregroundStyle(palette.fgPrimary)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .submitLabel(.search)
                .focused($isSearchFieldFocused)
                .onSubmit {
                    triggerSearch(queryText, immediate: true)
                }
                .onChange(of: queryText) { next in
                    triggerSearch(next, immediate: false)
                }

                if store.isSearchingGlobal {
                    OrbitSpinnerRingView(color: palette.fgSecondary, size: 13)
                } else if !queryText.isEmpty {
                    Button {
                        queryText = ""
                        store.performGlobalSearch(query: "")
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(OrbitTypography.iconSmall)
                            .foregroundStyle(palette.fgMuted)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, OrbitSpacing.md)
            .frame(height: 40)
            .background(palette.recessed)
            .clipShape(RoundedRectangle(cornerRadius: 11, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 11, style: .continuous)
                    .strokeBorder(palette.hairline, lineWidth: 0.5)
            )

            if store.hosts.count > 1 {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        hostFilterChip(title: OrbitL10n.tr("Global Fleet"), hostId: nil)
                        ForEach(store.hosts, id: \.hostId) { host in
                            hostFilterChip(title: host.displayName, hostId: host.hostId)
                        }
                    }
                }
            }

            if !queryText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !store.isSearchingGlobal {
                HStack(spacing: 6) {
                    Text("\(store.globalSearchHits.count)")
                        .font(OrbitTypography.meta(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                    Text("hits")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                    if store.globalSearchRemoteQueried > 0 {
                        Text("·")
                            .foregroundStyle(palette.fgFaint)
                        Text("\(store.globalSearchRemoteQueried) remote queried")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                    }
                    if store.globalSearchRemoteFailed > 0 {
                        Text("· \(store.globalSearchRemoteFailed) unreachable")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.warning)
                    }
                }
            }
        }
        .padding(.horizontal, OrbitSpacing.lg)
        .padding(.vertical, OrbitSpacing.md)
    }

    private func hostFilterChip(title: String, hostId: String?) -> some View {
        let selected = selectedHostFilter == hostId
        return Button {
            store.hapticLight()
            selectedHostFilter = hostId
            if !queryText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                triggerSearch(queryText, immediate: true)
            }
        } label: {
            Text(title)
                .font(OrbitTypography.meta(selected ? .semibold : .medium))
                .foregroundStyle(selected ? palette.onPrimary : palette.fgSecondary)
                .padding(.horizontal, 10)
                .frame(height: 28)
                .background(selected ? palette.primary : palette.subtleFill, in: Capsule())
        }
        .buttonStyle(.plain)
    }

    @ViewBuilder
    private var resultsContent: some View {
        if queryText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            VStack(spacing: OrbitSpacing.sm) {
                Spacer()
                Image(systemName: "text.magnifyingglass")
                    .font(.system(size: 26, weight: .medium))
                    .foregroundStyle(palette.fgMuted)
                Text(OrbitL10n.tr("Search across all hosts"))
                    .font(OrbitTypography.subhead(.semibold))
                    .foregroundStyle(palette.fgPrimary)
                Text("Full-history FTS5 search across local cache and online daemons.")
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgMuted)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, OrbitSpacing.xl)
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if store.globalSearchHits.isEmpty && !store.isSearchingGlobal {
            VStack(spacing: OrbitSpacing.sm) {
                Spacer()
                Text(OrbitL10n.tr("No matching results"))
                    .font(OrbitTypography.subhead(.medium))
                    .foregroundStyle(palette.fgSecondary)
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            ScrollView(.vertical, showsIndicators: true) {
                LazyVStack(spacing: OrbitSpacing.sm) {
                    ForEach(Array(store.globalSearchHits.enumerated()), id: \.offset) { _, hit in
                        hitCard(hit)
                    }
                }
                .padding(.horizontal, OrbitSpacing.lg)
                .padding(.vertical, OrbitSpacing.md)
            }
        }
    }

    private func hitCard(_ hit: ConversationSearchHit) -> some View {
        Button {
            store.hapticLight()
            dismiss()
            store.selectAgent(hostId: hit.hostId, agentId: hit.agentId)
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 6) {
                    Text(hit.title.isEmpty ? hit.projectName : hit.title)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    Spacer(minLength: 4)

                    Text(hit.hostName)
                        .font(.system(size: 10.5, weight: .medium))
                        .foregroundStyle(palette.fgSecondary)
                        .padding(.horizontal, 7)
                        .padding(.vertical, 2)
                        .background(palette.subtleFill, in: Capsule())
                }

                Text(hit.snippet)
                    .font(OrbitTypography.meta(.regular))
                    .foregroundStyle(palette.fgSecondary)
                    .lineLimit(3)
                    .multilineTextAlignment(.leading)

                HStack(spacing: 6) {
                    Text(hit.role.capitalized)
                        .font(.system(size: 10.5, weight: .medium))
                        .foregroundStyle(palette.fgMuted)
                    if !hit.projectName.isEmpty {
                        Text("·")
                            .foregroundStyle(palette.fgFaint)
                        Text(hit.projectName)
                            .font(.system(size: 10.5, weight: .regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(1)
                    }
                    Spacer()
                    Text(OrbitFormatters.relativeUpdatedTime(rfc3339: hit.updatedAtRfc3339))
                        .font(.system(size: 10.5, weight: .regular))
                        .foregroundStyle(palette.fgMuted)
                }
            }
            .padding(OrbitSpacing.md)
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

    private func triggerSearch(_ text: String, immediate: Bool = false) {
        debounceTask?.cancel()
        if immediate {
            store.performGlobalSearch(query: text, hostId: selectedHostFilter)
            return
        }
        debounceTask = Task {
            try? await Task.sleep(nanoseconds: 240_000_000)
            guard !Task.isCancelled else { return }
            await MainActor.run {
                store.performGlobalSearch(query: text, hostId: selectedHostFilter)
            }
        }
    }
}
