import SwiftUI

/// Native iOS `List`-backed `/` command panel aligned with Desktop `SlashCommandMenu`.
struct ComposerSlashCommandPanel: View {
    let commands: [FfiSlashCommand]
    let onSelect: (FfiSlashCommand) -> Void

    @Environment(\.orbitPalette) private var palette

    private var listHeight: CGFloat {
        min(CGFloat(commands.count) * 44 + 4, 240)
    }

    var body: some View {
        List(commands) { cmd in
            Button {
                onSelect(cmd)
            } label: {
                HStack(spacing: 10) {
                    Image(systemName: OrbitComposerLogic.iconName(forSlashCommandId: cmd.id))
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 22, alignment: .center)

                    Text(cmd.command)
                        .font(.system(size: 13.5, weight: .medium, design: .monospaced))
                        .foregroundStyle(palette.fgPrimary)
                        .fixedSize(horizontal: true, vertical: false)

                    Text(cmd.description)
                        .font(.system(size: 12.5, weight: .regular))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)

                    Spacer(minLength: 0)
                }
                .frame(minHeight: 44)
                .contentShape(Rectangle())
            }
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets(top: 0, leading: 12, bottom: 0, trailing: 12))
            .listRowSeparatorTint(palette.hairline)
            .alignmentGuide(.listRowSeparatorLeading) { _ in 32 }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .environment(\.defaultMinListRowHeight, 44)
        .scrollDisabled(commands.count <= 5)
        .frame(height: listHeight)
        .orbitGlassSurface(
            in: RoundedRectangle(cornerRadius: 18, style: .continuous),
            fallbackFill: palette.elevated,
            fallbackBorder: palette.hairline,
            fallbackShadowOpacity: palette.isDark ? 0.28 : 0.08
        )
    }
}

/// Native iOS `List`-backed `@` mention panel aligned with Desktop `MentionMenu`.
struct ComposerMentionPanel: View {
    let candidates: [FfiMentionCandidate]
    let onSelect: (FfiMentionCandidate) -> Void

    @Environment(\.orbitPalette) private var palette

    private var listHeight: CGFloat {
        min(CGFloat(candidates.count) * 44 + 4, 240)
    }

    var body: some View {
        List(candidates) { item in
            Button {
                onSelect(item)
            } label: {
                rowContent(for: item)
                    .frame(minHeight: 44)
                    .contentShape(Rectangle())
            }
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets(top: 0, leading: 12, bottom: 0, trailing: 12))
            .listRowSeparatorTint(palette.hairline)
            .alignmentGuide(.listRowSeparatorLeading) { _ in 32 }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .environment(\.defaultMinListRowHeight, 44)
        .scrollDisabled(candidates.count <= 5)
        .frame(height: listHeight)
        .orbitGlassSurface(
            in: RoundedRectangle(cornerRadius: 18, style: .continuous),
            fallbackFill: palette.elevated,
            fallbackBorder: palette.hairline,
            fallbackShadowOpacity: palette.isDark ? 0.28 : 0.08
        )
    }

    private func iconName(for item: FfiMentionCandidate) -> String {
        if OrbitComposerLogic.isPeerMentionCandidate(kind: item.kind, key: item.key) {
            return OrbitComposerLogic.isAllPeerMention(key: item.key, handle: item.title) ? "person.3" : "at"
        }
        switch item.kind.lowercased() {
        case "staged":
            return "arrow.left.arrow.right"
        case "git_diff":
            return "doc.badge.plus"
        case "terminal":
            return "terminal"
        default:
            return OrbitFormatters.semanticFileIcon(for: item.title)
        }
    }

    @ViewBuilder
    private func rowContent(for item: FfiMentionCandidate) -> some View {
        if item.kind == "file" {
            HStack(spacing: 10) {
                Image(systemName: iconName(for: item))
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: 22, alignment: .center)

                Text(item.title)
                    .font(.system(size: 13.5, weight: .medium, design: .monospaced))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)
                    .layoutPriority(1)

                if !item.subtitle.isEmpty {
                    Text(item.subtitle)
                        .font(.system(size: 12, weight: .regular, design: .monospaced))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }

                Spacer(minLength: 4)

                if !item.gitStatus.isEmpty {
                    Text(item.gitStatus)
                        .font(.system(size: 10.5, weight: .medium, design: .monospaced))
                        .foregroundStyle(palette.fgSecondary)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(palette.subtleFill)
                        .clipShape(RoundedRectangle(cornerRadius: 4, style: .continuous))
                }
            }
        } else {
            HStack(spacing: 10) {
                Image(systemName: iconName(for: item))
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: 22, alignment: .center)

                Text(item.title)
                    .font(.system(size: 13.5, weight: .medium))
                    .foregroundStyle(palette.fgPrimary)
                    .fixedSize(horizontal: true, vertical: false)

                Text(item.subtitle)
                    .font(.system(size: 12.5, weight: .regular))
                    .foregroundStyle(palette.fgMuted)
                    .lineLimit(1)

                Spacer(minLength: 0)
            }
        }
    }
}
