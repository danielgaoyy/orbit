import SwiftUI

struct ComposerMentionCandidate: Identifiable, Equatable {
    enum Kind: Equatable {
        case gitDiff(staged: Bool, label: String, subtitle: String)
        case terminalOutput(label: String, subtitle: String)
        case file(path: String, basename: String, dirPath: String, gitStatus: String?)
    }

    let id: String
    let kind: Kind

    var icon: String {
        switch kind {
        case let .gitDiff(staged, _, _):
            return staged ? "arrow.left.arrow.right" : "doc.badge.plus"
        case .terminalOutput:
            return "terminal"
        case let .file(_, basename, _, _):
            return OrbitFormatters.semanticFileIcon(for: basename)
        }
    }
}

/// Native iOS `List`-backed `/` command panel aligned with Desktop `SlashCommandMenu`.
struct ComposerSlashCommandPanel: View {
    let commands: [OrbitSlashCommand]
    let onSelect: (OrbitSlashCommand) -> Void

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
                    Image(systemName: cmd.icon)
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
    let candidates: [ComposerMentionCandidate]
    let onSelect: (ComposerMentionCandidate) -> Void

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

    @ViewBuilder
    private func rowContent(for item: ComposerMentionCandidate) -> some View {
        switch item.kind {
        case let .file(_, basename, dirPath, gitStatus):
            HStack(spacing: 10) {
                Image(systemName: item.icon)
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: 22, alignment: .center)

                Text(basename)
                    .font(.system(size: 13.5, weight: .medium, design: .monospaced))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)
                    .layoutPriority(1)

                if !dirPath.isEmpty {
                    Text(dirPath)
                        .font(.system(size: 12, weight: .regular, design: .monospaced))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }

                Spacer(minLength: 4)

                if let status = gitStatus {
                    Text(status)
                        .font(.system(size: 10.5, weight: .medium, design: .monospaced))
                        .foregroundStyle(palette.fgSecondary)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(palette.subtleFill)
                        .clipShape(RoundedRectangle(cornerRadius: 4, style: .continuous))
                }
            }
        case let .gitDiff(_, label, subtitle), let .terminalOutput(label, subtitle):
            HStack(spacing: 10) {
                Image(systemName: item.icon)
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: 22, alignment: .center)

                Text(label)
                    .font(.system(size: 13.5, weight: .medium))
                    .foregroundStyle(palette.fgPrimary)
                    .fixedSize(horizontal: true, vertical: false)

                Text(subtitle)
                    .font(.system(size: 12.5, weight: .regular))
                    .foregroundStyle(palette.fgMuted)
                    .lineLimit(1)

                Spacer(minLength: 0)
            }
        }
    }
}
