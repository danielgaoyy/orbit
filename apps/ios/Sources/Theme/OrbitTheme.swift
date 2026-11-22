import SwiftUI
import UIKit

// MARK: - Appearance Mode

public enum OrbitAppearanceMode: String, CaseIterable, Identifiable, Sendable {
    case system
    case light
    case dark

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .system: return "System"
        case .light: return "Light"
        case .dark: return "Dark"
        }
    }

    public var systemImage: String {
        switch self {
        case .system: return "circle.lefthalf.filled"
        case .light: return "sun.max"
        case .dark: return "moon"
        }
    }

    public static func fromStorage(_ raw: String) -> OrbitAppearanceMode {
        switch raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
        case "light":
            return .light
        case "dark", "graphite", "oled", "black":
            return .dark
        default:
            return .system
        }
    }

    public var preferredColorScheme: ColorScheme? {
        switch self {
        case .system: return nil
        case .light: return .light
        case .dark: return .dark
        }
    }

    public var userInterfaceStyle: UIUserInterfaceStyle {
        switch self {
        case .system: return .unspecified
        case .light: return .light
        case .dark: return .dark
        }
    }

    public func resolvedPalette(systemScheme: ColorScheme) -> OrbitPalette {
        switch self {
        case .light:
            return .light
        case .dark:
            return .graphiteDark
        case .system:
            return systemScheme == .dark ? .graphiteDark : .light
        }
    }
}

// MARK: - Quiet Precision Palette

public struct OrbitPalette: Equatable, Sendable {
    public let isDark: Bool
    public let canvas: Color
    public let drawer: Color
    public let elevated: Color
    public let code: Color
    public let fgPrimary: Color
    public let fgSecondary: Color
    public let fgMuted: Color
    public let hairline: Color
    public let subtleFill: Color
    public let activeRowFill: Color

    // Semantic Accents (aligned with Desktop monochrome primary + restrained semantic palette)
    public let primary: Color
    public let onPrimary: Color
    public let direct: Color
    public let added: Color
    public let relay: Color
    public let warning: Color
    public let danger: Color
    public let removed: Color
    public let accentSky: Color
    public let accentViolet: Color

    public static let light = OrbitPalette(
        isDark: false,
        canvas: Color(hex: "#FFFFFF"),
        drawer: Color(hex: "#F4F4F6"),
        elevated: Color(hex: "#FFFFFF"),
        code: Color(hex: "#F4F4F6"),
        fgPrimary: Color(hex: "#121314"),
        fgSecondary: Color(hex: "#5A5C60"),
        fgMuted: Color(hex: "#8B8E94"),
        hairline: Color.black.opacity(0.072),
        subtleFill: Color.black.opacity(0.055),
        activeRowFill: Color(hex: "#ECEDEF"),
        primary: Color(hex: "#141416"),
        onPrimary: Color(hex: "#FFFFFF"),
        direct: Color(hex: "#059669"),
        added: Color(hex: "#059669"),
        relay: Color(hex: "#7C3AED"),
        warning: Color(hex: "#D97706"),
        danger: Color(hex: "#EF4444"),
        removed: Color(hex: "#E11D48"),
        accentSky: Color(hex: "#0EA5E9"),
        accentViolet: Color(hex: "#7C3AED")
    )

    public static let graphiteDark = OrbitPalette(
        isDark: true,
        canvas: Color(hex: "#191A1C"),
        drawer: Color(hex: "#111214"),
        elevated: Color(hex: "#212326"),
        code: Color(hex: "#131416"),
        fgPrimary: Color(hex: "#ECEDEF"),
        fgSecondary: Color(hex: "#989A9D"),
        fgMuted: Color(hex: "#686B70"),
        hairline: Color.white.opacity(0.075),
        subtleFill: Color.white.opacity(0.08),
        activeRowFill: Color(hex: "#2A2C2F"),
        primary: Color(hex: "#F4F4F6"),
        onPrimary: Color(hex: "#141416"),
        direct: Color(hex: "#34D399"),
        added: Color(hex: "#34D399"),
        relay: Color(hex: "#A78BFA"),
        warning: Color(hex: "#FBBF24"),
        danger: Color(hex: "#F87171"),
        removed: Color(hex: "#FB7185"),
        accentSky: Color(hex: "#38BDF8"),
        accentViolet: Color(hex: "#A78BFA")
    )

    public static let oledPure = OrbitPalette(
        isDark: true,
        canvas: Color(hex: "#000000"),
        drawer: Color(hex: "#08090B"),
        elevated: Color(hex: "#121317"),
        code: Color(hex: "#0A0B0E"),
        fgPrimary: Color(hex: "#F4F4F6"),
        fgSecondary: Color(hex: "#989A9D"),
        fgMuted: Color(hex: "#63666B"),
        hairline: Color.white.opacity(0.11),
        subtleFill: Color.white.opacity(0.085),
        activeRowFill: Color.white.opacity(0.10),
        primary: Color(hex: "#F4F4F6"),
        onPrimary: Color(hex: "#000000"),
        direct: Color(hex: "#34D399"),
        added: Color(hex: "#34D399"),
        relay: Color(hex: "#A78BFA"),
        warning: Color(hex: "#FBBF24"),
        danger: Color(hex: "#F87171"),
        removed: Color(hex: "#FB7185"),
        accentSky: Color(hex: "#38BDF8"),
        accentViolet: Color(hex: "#A78BFA")
    )

    public func connectionColor(_ state: FfiConnectionState) -> Color {
        switch state {
        case .onlineDirect:
            return direct
        case .onlineRelay:
            return relay
        case .connectingDirect, .connectingRelay:
            return warning
        case .disconnected:
            return fgMuted
        }
    }

    public func providerAccent(_ provider: FfiProviderKind) -> Color {
        switch provider {
        case .antigravity:
            return Color(hex: "#3186FF")
        case .codex:
            return Color(hex: "#6B8CFF")
        case .claudeCode:
            return Color(hex: "#D97757")
        }
    }
}

private struct OrbitPaletteKey: EnvironmentKey {
    static let defaultValue: OrbitPalette = .light
}

public extension EnvironmentValues {
    var orbitPalette: OrbitPalette {
        get { self[OrbitPaletteKey.self] }
        set { self[OrbitPaletteKey.self] = newValue }
    }
}

// MARK: - Strict 4-Tier Typography Scale (18 / 15 / 13 / 12 ONLY)

public enum OrbitTypography {
    /// Tier 1: 18pt Semibold — Sheet headers, Empty-state greeting, Onboarding headline
    public static let title: Font = .system(size: 18, weight: .semibold, design: .default)

    /// Tier 2: 15pt — Assistant Markdown prose, User prompt text, Composer input, Drawer primary row titles
    public static func body(_ weight: Font.Weight = .regular) -> Font {
        .system(size: 15, weight: weight, design: .default)
    }

    public static func bodyItalic(_ weight: Font.Weight = .regular) -> Font {
        .system(size: 15, weight: weight, design: .default).italic()
    }

    /// Tier 3: 13pt — Top bar session title, Settled step summary row, Button labels, Approval header, Drawer subtitles
    public static func subhead(_ weight: Font.Weight = .regular) -> Font {
        .system(size: 13, weight: weight, design: .default)
    }

    public static func subheadItalic(_ weight: Font.Weight = .regular) -> Font {
        .system(size: 13, weight: weight, design: .default).italic()
    }

    /// Tier 4a: 12pt Proportional with Monospaced Digits — Timestamps, +N -M counts, RTT ms, model pills
    public static func meta(_ weight: Font.Weight = .regular) -> Font {
        .system(size: 12, weight: weight, design: .default).monospacedDigit()
    }

    /// Tier 4b: 12pt Monospaced — Code blocks, inline code, file paths, diffs, shell commands, PTY grid
    public static func code(_ weight: Font.Weight = .regular) -> Font {
        .system(size: 12, weight: weight, design: .monospaced)
    }

    public static func codeItalic(_ weight: Font.Weight = .regular) -> Font {
        .system(size: 12, weight: weight, design: .monospaced).italic()
    }

    /// Refined SF Symbol sizing aligned strictly to the 4-tier scale
    public static let icon: Font = .system(size: 15, weight: .medium, design: .default)
    public static let iconSmall: Font = .system(size: 13, weight: .medium, design: .default)
    public static let iconMicro: Font = .system(size: 12, weight: .semibold, design: .default)

    /// Native UIFont for exact PTY terminal character cell measurement (12pt monospaced)
    public static let terminalUIFont: UIFont = UIFont.monospacedSystemFont(ofSize: 12, weight: .regular)

    public static let bodyLineSpacing: CGFloat = 5
    public static let subheadLineSpacing: CGFloat = 3
    public static let metaLineSpacing: CGFloat = 2

    public static func terminalCellMetrics() -> (width: CGFloat, height: CGFloat) {
        let sample = ("M" as NSString).size(withAttributes: [.font: terminalUIFont])
        let cellWidth = max(sample.width, 7.2)
        let cellHeight = max(ceil(terminalUIFont.lineHeight) + metaLineSpacing, 16.0)
        return (cellWidth, cellHeight)
    }
}

// MARK: - Strict 4pt Spatial Grid (4 / 8 / 12 / 16 / 24 / 32)

public enum OrbitSpacing {
    public static let xs: CGFloat = 4
    public static let sm: CGFloat = 8
    public static let md: CGFloat = 12
    public static let lg: CGFloat = 16
    public static let xl: CGFloat = 24
    public static let xxl: CGFloat = 32

    public static let hitTarget: CGFloat = 44
    public static let sidebarWidth: CGFloat = 260
    public static let drawerWidth: CGFloat = 316
    public static let inspectorWidth: CGFloat = 360
}

// MARK: - Typography & Touch Target Modifiers

public extension View {
    func orbitTitleStyle() -> some View {
        self.font(OrbitTypography.title)
    }

    func orbitBodyStyle(_ weight: Font.Weight = .regular) -> some View {
        self.font(OrbitTypography.body(weight))
            .lineSpacing(OrbitTypography.bodyLineSpacing)
    }

    func orbitSubheadStyle(_ weight: Font.Weight = .regular) -> some View {
        self.font(OrbitTypography.subhead(weight))
            .lineSpacing(OrbitTypography.subheadLineSpacing)
    }

    func orbitMetaStyle(_ weight: Font.Weight = .regular) -> some View {
        self.font(OrbitTypography.meta(weight))
            .lineSpacing(OrbitTypography.metaLineSpacing)
    }

    func orbitCodeStyle(_ weight: Font.Weight = .regular) -> some View {
        self.font(OrbitTypography.code(weight))
            .lineSpacing(OrbitTypography.metaLineSpacing)
    }

    func orbitHitTarget() -> some View {
        self.frame(minWidth: OrbitSpacing.hitTarget, minHeight: OrbitSpacing.hitTarget)
            .contentShape(Rectangle())
    }

    /// Floating iOS 26+ Liquid Glass surface with an elevated hairline fallback on iOS 16.4–18.
    @ViewBuilder
    func orbitGlassSurface<S: InsettableShape>(
        in shape: S,
        tint: Color? = nil,
        interactive: Bool = false,
        fallbackFill: Color,
        fallbackBorder: Color,
        fallbackShadowOpacity: Double = 0
    ) -> some View {
        if #available(iOS 26.0, *) {
            let glass: Glass = {
                var value = Glass.regular
                if let tint {
                    value = value.tint(tint)
                }
                if interactive {
                    value = value.interactive()
                }
                return value
            }()
            self.glassEffect(glass, in: shape)
        } else {
            self
                .background(fallbackFill)
                .clipShape(shape)
                .overlay(shape.strokeBorder(fallbackBorder, lineWidth: 0.5))
                .shadow(
                    color: fallbackShadowOpacity > 0 ? Color.black.opacity(fallbackShadowOpacity) : .clear,
                    radius: fallbackShadowOpacity > 0 ? 10 : 0,
                    y: fallbackShadowOpacity > 0 ? 2 : 0
                )
        }
    }

    /// Soft edge effect on iOS 26+ so scrolled content dissolves smoothly under floating Liquid Glass chrome.
    @ViewBuilder
    func orbitSoftScrollEdgeEffect() -> some View {
        if #available(iOS 26.0, *) {
            self.scrollEdgeEffectStyle(.soft, for: .all)
        } else {
            self
        }
    }

    /// Subtle symbol replacement transition on iOS 17+ for copy/checkmark toggles.
    @ViewBuilder
    func orbitSymbolReplaceTransition() -> some View {
        if #available(iOS 17.0, *) {
            self.contentTransition(.symbolEffect(.replace))
        } else {
            self
        }
    }
}

public struct OrbitGlassContainer<Content: View>: View {
    private let spacing: CGFloat
    private let content: Content

    public init(spacing: CGFloat = OrbitSpacing.sm, @ViewBuilder content: () -> Content) {
        self.spacing = spacing
        self.content = content()
    }

    public var body: some View {
        if #available(iOS 26.0, *) {
            GlassEffectContainer(spacing: spacing) {
                content
            }
        } else {
            content
        }
    }
}

// MARK: - Shared Components & Formatters

public struct OrbitHairline: View {
    @Environment(\.orbitPalette) private var palette
    public var axis: Axis = .horizontal

    public init(axis: Axis = .horizontal) {
        self.axis = axis
    }

    public var body: some View {
        if axis == .horizontal {
            Rectangle()
                .fill(palette.hairline)
                .frame(height: 0.5)
        } else {
            Rectangle()
                .fill(palette.hairline)
                .frame(width: 0.5)
        }
    }
}

public struct PulsingDotView: View {
    public let color: Color
    public var size: CGFloat = 6
    @State private var isPulsing = false

    public init(color: Color, size: CGFloat = 6) {
        self.color = color
        self.size = size
    }

    public var body: some View {
        Circle()
            .fill(color)
            .frame(width: size, height: size)
            .opacity(isPulsing ? 0.38 : 1.0)
            .animation(
                .easeInOut(duration: 0.9).repeatForever(autoreverses: true),
                value: isPulsing
            )
            .onAppear {
                isPulsing = true
            }
    }
}

public struct OrbitSpinnerRingView: View {
    public let color: Color
    public var size: CGFloat = 12.5
    @State private var isSpinning = false

    public init(color: Color, size: CGFloat = 12.5) {
        self.color = color
        self.size = size
    }

    public var body: some View {
        Circle()
            .trim(from: 0, to: 0.72)
            .stroke(color, style: StrokeStyle(lineWidth: 1.5, lineCap: .round))
            .frame(width: size, height: size)
            .rotationEffect(.degrees(isSpinning ? 360 : 0))
            .animation(
                .linear(duration: 0.9).repeatForever(autoreverses: false),
                value: isSpinning
            )
            .onAppear {
                isSpinning = true
            }
    }
}

public enum OrbitFormatters {
    public static func connectionLabel(_ state: FfiConnectionState) -> String {
        switch state {
        case .onlineDirect:
            return "LAN Direct"
        case .onlineRelay:
            return "E2EE Relay"
        case .connectingDirect:
            return "Connecting Direct…"
        case .connectingRelay:
            return "Connecting Relay…"
        case .disconnected:
            return "Offline"
        }
    }

    public static func compactTopBarConnectionLabel(for host: FfiHostCard) -> String {
        switch host.connectionState {
        case .onlineDirect:
            return (host.viaSshTunnel || host.routeMode == .sshTunnel) ? "SSH" : "Direct"
        case .onlineRelay:
            return "Relay"
        case .connectingDirect, .connectingRelay:
            return "Connecting"
        case .disconnected:
            return "Offline"
        }
    }

    public static func providerLabel(_ provider: FfiProviderKind) -> String {
        switch provider {
        case .antigravity:
            return "Antigravity"
        case .codex:
            return "Codex"
        case .claudeCode:
            return "Claude Code"
        }
    }

    public static func providerShortLabel(_ provider: FfiProviderKind) -> String {
        switch provider {
        case .antigravity:
            return "Antigravity"
        case .codex:
            return "Codex"
        case .claudeCode:
            return "Claude"
        }
    }

    public static func compactEffortLabel(_ effort: FfiReasoningEffort) -> String {
        switch effort {
        case .off:
            return "Off"
        case .low:
            return "Low"
        case .medium:
            return "Med"
        case .high:
            return "High"
        case .xhigh:
            return "XHigh"
        case .max:
            return "Max"
        case .ultra:
            return "Ultra"
        }
    }

    public static func normalizePermissionMode(_ mode: FfiSessionMode) -> FfiSessionMode {
        if case .plan = mode {
            return .default
        }
        return mode
    }

    public static func sessionModeLabel(_ mode: FfiSessionMode) -> String {
        switch normalizePermissionMode(mode) {
        case .default, .plan:
            return "Always Ask"
        case .acceptEdits:
            return "Accept Edits"
        case .fullAccess:
            return "Full Access"
        case let .custom(name):
            return name
        }
    }

    public static func sessionModeIcon(_ mode: FfiSessionMode) -> String {
        switch normalizePermissionMode(mode) {
        case .default, .plan, .custom:
            return "shield"
        case .acceptEdits:
            return "checkmark.shield"
        case .fullAccess:
            return "shield.slash"
        }
    }

    public static func routeModeLabel(_ mode: FfiHostRouteMode) -> String {
        switch mode {
        case .auto:
            return "Auto"
        case .directOnly:
            return "Direct Only"
        case .relayOnly:
            return "Relay Only"
        case .sshTunnel:
            return "SSH Tunnel"
        }
    }

    public static func formatDurationMs(_ ms: Int64?) -> String? {
        guard let ms = ms, ms > 0 else { return nil }
        let seconds = Double(ms) / 1000.0
        if seconds < 1.0 {
            return "\(ms)ms"
        } else if seconds < 60.0 {
            let rounded = Int(seconds.rounded())
            return "\(max(1, rounded))s"
        } else {
            let mins = Int(seconds) / 60
            let secs = Int(seconds) % 60
            return secs > 0 ? "\(mins)m \(secs)s" : "\(mins)m"
        }
    }

    public static func shortPath(_ rawPath: String) -> String {
        let trimmed = rawPath.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "~" }
        if trimmed == "~" || trimmed == "/" { return trimmed }
        let parts = trimmed.split(separator: "/").map(String.init)
        if let last = parts.last, !last.isEmpty {
            return last
        }
        return trimmed
    }

    public static func compactPath(_ rawPath: String) -> String {
        let trimmed = rawPath.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "~" }
        let parts = trimmed.split(separator: "/").map(String.init)
        if parts.count >= 2, parts[0] == "Users" {
            let rest = parts.dropFirst(2)
            return rest.isEmpty ? "~" : "~/" + rest.joined(separator: "/")
        }
        if parts.count >= 2, parts[0] == "home" {
            let rest = parts.dropFirst(2)
            return rest.isEmpty ? "~" : "~/" + rest.joined(separator: "/")
        }
        return trimmed
    }

    public static func arePathsEquivalent(_ lhs: String, _ rhs: String) -> Bool {
        let normLhs = normalizeTildePath(lhs)
        let normRhs = normalizeTildePath(rhs)
        return !normLhs.isEmpty && normLhs == normRhs
    }

    private static func normalizeTildePath(_ rawPath: String) -> String {
        var compact = compactPath(rawPath)
        while compact.count > 1 && compact.hasSuffix("/") {
            compact.removeLast()
        }
        return compact
    }

    private static let isoFormatterWithFractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static let isoFormatterPlain: ISO8601DateFormatter = {
        ISO8601DateFormatter()
    }()

    public static func resetCountdown(rfc3339: String?) -> String? {
        guard let raw = rfc3339?.trimmingCharacters(in: .whitespacesAndNewlines), !raw.isEmpty else {
            return nil
        }
        var date = isoFormatterWithFractional.date(from: raw) ?? isoFormatterPlain.date(from: raw)
        if date == nil, let epoch = Double(raw) {
            let secs = epoch > 10_000_000_000 ? epoch / 1000.0 : epoch
            date = Date(timeIntervalSince1970: secs)
        }
        guard let target = date else { return nil }
        let delta = Int(target.timeIntervalSinceNow)
        if delta <= 0 {
            return "Resets soon"
        }
        let hours = delta / 3600
        let minutes = (delta % 3600) / 60
        if hours >= 24 {
            let days = hours / 24
            let remHours = hours % 24
            return remHours > 0 ? "Resets in \(days)d \(remHours)h" : "Resets in \(days)d"
        } else if hours > 0 {
            return "Resets in \(hours)h \(minutes)m"
        } else {
            return "Resets in \(max(1, minutes))m"
        }
    }

    public static func relativeUpdatedTime(rfc3339: String) -> String {
        guard !rfc3339.isEmpty else { return "" }
        let date = isoFormatterWithFractional.date(from: rfc3339) ?? isoFormatterPlain.date(from: rfc3339)
        guard let parsed = date else { return "" }
        let elapsed = max(0, Int(Date().timeIntervalSince(parsed)))
        if elapsed < 45 {
            return "now"
        } else if elapsed < 3600 {
            return "\(elapsed / 60)m"
        } else if elapsed < 86400 {
            return "\(elapsed / 3600)h"
        } else {
            return "\(elapsed / 86400)d"
        }
    }

    private static let shortDateSameYearFormatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.setLocalizedDateFormatFromTemplate("MMMd")
        return f
    }()

    private static let shortDateWithYearFormatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.setLocalizedDateFormatFromTemplate("MMMdyyyy")
        return f
    }()

    public static func relativeTimeLabel(
        timestampMs: Int64,
        nowMs: Int64 = Int64(Date().timeIntervalSince1970 * 1000)
    ) -> String {
        let diffSec = max(0, Int((nowMs - timestampMs) / 1000))
        if diffSec < 60 {
            return "just now"
        }
        let mins = diffSec / 60
        if mins < 60 {
            return "\(mins)m ago"
        }
        let hours = mins / 60
        if hours < 24 {
            return "\(hours)h ago"
        }
        let days = hours / 24
        if days < 30 {
            return "\(days)d ago"
        }
        let date = Date(timeIntervalSince1970: Double(timestampMs) / 1000.0)
        let nowDate = Date(timeIntervalSince1970: Double(nowMs) / 1000.0)
        let cal = Calendar.current
        let sameYear = cal.component(.year, from: date) == cal.component(.year, from: nowDate)
        return (sameYear ? shortDateSameYearFormatter : shortDateWithYearFormatter).string(from: date)
    }

    public static func stripWorkspacePrefix(_ rawPath: String, cwd: String) -> String {
        OrbitPathLogic.stripWorkspacePrefix(rawPath, cwd: cwd)
    }

    public static func decisionLabel(_ raw: String) -> String {
        OrbitDecisionLogic.decisionLabel(raw)
    }

    private static let lockfileNames: Set<String> = [
        "cargo.lock", "package-lock.json", "pnpm-lock.yaml", "yarn.lock",
        "bun.lock", "bun.lockb", "flake.lock", "poetry.lock", "uv.lock"
    ]

    private static let codeExtensions: Set<String> = [
        "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "swift",
        "kt", "kts", "java", "c", "h", "cc", "cpp", "hpp", "cs", "rb",
        "php", "lua", "zig", "nix", "proto", "graphql", "gql", "css",
        "scss", "less", "html", "vue", "svelte", "astro"
    ]

    /// Semantic monochrome SF Symbol aligned 1:1 with Desktop Lucide file icons.
    public static func semanticFileIcon(for basename: String) -> String {
        let lower = basename.lowercased()
        if lockfileNames.contains(lower) || lower.hasSuffix(".lock") {
            return "lock.doc"
        }
        if lower == "dockerfile" || lower.hasPrefix("dockerfile.")
            || lower.hasPrefix(".env") || lower.contains(".config.") {
            return "doc.badge.gearshape"
        }
        if lower == "makefile" || lower == "justfile" {
            return "terminal"
        }
        if lower == "license" || lower.hasPrefix("readme") || lower.hasPrefix("changelog") {
            return "doc.text"
        }
        let ext = (lower as NSString).pathExtension
        switch ext {
        case "json", "jsonc", "jsonl":
            return "curlybraces"
        case "toml", "yaml", "yml", "ini", "conf", "plist":
            return "doc.badge.gearshape"
        case "sh", "zsh", "bash", "fish", "ps1":
            return "terminal"
        case "md", "mdx", "txt", "rst", "adoc":
            return "doc.text"
        case "png", "jpg", "jpeg", "webp", "gif", "svg", "ico", "icns", "bmp", "heic":
            return "photo"
        case "sql", "sqlite", "db", "prisma":
            return "cylinder.split.1x2"
        default:
            return codeExtensions.contains(ext) ? "chevron.left.forwardslash.chevron.right" : "doc"
        }
    }

    public static func isHostOnline(_ state: FfiConnectionState) -> Bool {
        switch state {
        case .onlineDirect, .onlineRelay:
            return true
        case .connectingDirect, .connectingRelay, .disconnected:
            return false
        }
    }

    public static func isGenericHarnessHandle(_ raw: String?) -> Bool {
        OrbitAgentLogic.isGenericHarnessHandle(raw)
    }

    public static func semanticAgentTabLabel(
        handle: String?,
        title: String,
        taskSummary: String?,
        agentId: String
    ) -> String {
        if let rawHandle = handle?.trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "@")),
           !rawHandle.isEmpty,
           !isGenericHarnessHandle(rawHandle) {
            return rawHandle
        }
        let cleanTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        if !cleanTitle.isEmpty, cleanTitle.lowercased() != "new conversation" {
            return cleanTitle.count > 26 ? String(cleanTitle.prefix(25)) + "…" : cleanTitle
        }
        if let summary = taskSummary?.trimmingCharacters(in: .whitespacesAndNewlines), !summary.isEmpty {
            return summary.count > 26 ? String(summary.prefix(25)) + "…" : summary
        }
        if let rawHandle = handle?.trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "@")),
           !rawHandle.isEmpty {
            return rawHandle
        }
        return String(agentId.prefix(10))
    }

    public static func normalizeSemver(_ raw: String?) -> String? {
        guard let raw else { return nil }
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        let stripped = trimmed.hasPrefix("v") || trimmed.hasPrefix("V")
            ? String(trimmed.dropFirst())
            : trimmed
        let corePart = stripped.split(separator: "-").first.map(String.init) ?? stripped
        let parts = corePart.split(separator: ".")
        guard parts.count >= 2, parts.allSatisfy({ Int($0) != nil }) else { return nil }
        return corePart
    }

    public static func compareSemver(_ lhs: String, _ rhs: String) -> Int {
        let a = lhs.split(separator: ".").compactMap { Int($0) }
        let b = rhs.split(separator: ".").compactMap { Int($0) }
        let maxCount = max(a.count, b.count)
        for idx in 0..<maxCount {
            let av = idx < a.count ? a[idx] : 0
            let bv = idx < b.count ? b[idx] : 0
            if av < bv { return -1 }
            if av > bv { return 1 }
        }
        return 0
    }

    public static func shortFileLabel(_ path: String, rootCwd: String) -> String {
        OrbitPathLogic.shortFileLabel(path, rootCwd: rootCwd)
    }

    public static func formatDurationCompact(_ milliseconds: UInt64) -> String {
        OrbitTimeLogic.formatDurationCompact(milliseconds)
    }

    public static func isAgentForegroundRunning(statusRaw: String?, hasActiveTurn: Bool) -> Bool {
        OrbitAgentLogic.isAgentForegroundRunning(statusRaw: statusRaw, hasActiveTurn: hasActiveTurn)
    }

    public static func isAgentBackgroundWaiting(statusRaw: String?, hasActiveTurn: Bool, hasBackgroundTasks: Bool) -> Bool {
        OrbitAgentLogic.isAgentBackgroundWaiting(
            statusRaw: statusRaw,
            hasActiveTurn: hasActiveTurn,
            hasBackgroundTasks: hasBackgroundTasks
        )
    }

    public static func iconName(forSlashCommandId id: String) -> String {
        OrbitComposerLogic.iconName(forSlashCommandId: id)
    }

    public static func relativeTimeFromRfc3339(_ rfc3339: String?) -> String {
        guard let raw = rfc3339?.trimmingCharacters(in: .whitespacesAndNewlines), !raw.isEmpty else {
            return ""
        }
        let parsed = isoFormatterWithFractional.date(from: raw) ?? isoFormatterPlain.date(from: raw)
        guard let date = parsed else { return "" }
        let ms = Int64(date.timeIntervalSince1970 * 1000.0)
        return relativeTimeLabel(timestampMs: ms)
    }
}

// MARK: - Compact Liquid Glass Popover Primitives

public struct OrbitPopoverSectionHeader: View {
    public let title: String
    public let trailing: String?

    @Environment(\.orbitPalette) private var palette

    public init(title: String, trailing: String? = nil) {
        self.title = title
        self.trailing = trailing
    }

    public var body: some View {
        HStack(spacing: 8) {
            Text(title)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(palette.fgMuted)
                .lineLimit(1)

            Spacer(minLength: 8)

            if let trailing, !trailing.isEmpty {
                Text(trailing)
                    .font(.system(size: 11.5, weight: .regular))
                    .foregroundStyle(palette.fgMuted)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
        .frame(height: 22)
        .padding(.horizontal, 10)
    }
}

public struct OrbitPopoverLeadingIcon: View {
    public let systemName: String?
    public let color: Color?

    @Environment(\.orbitPalette) private var palette

    public init(systemName: String?, color: Color? = nil) {
        self.systemName = systemName
        self.color = color
    }

    @ViewBuilder
    public var body: some View {
        if let systemName {
            Image(systemName: systemName)
                .font(.system(size: 13.5, weight: .medium))
                .foregroundStyle(color ?? palette.fgSecondary)
                .frame(width: 16, alignment: .center)
        }
    }
}

public struct OrbitPopoverOptionRow<Leading: View, TrailingBadge: View>: View {
    public let title: String
    public let subtitle: String?
    public let isSelected: Bool
    public let action: () -> Void
    private let leading: Leading
    private let trailingBadge: TrailingBadge

    @Environment(\.orbitPalette) private var palette

    public init(
        title: String,
        subtitle: String? = nil,
        isSelected: Bool,
        action: @escaping () -> Void,
        @ViewBuilder leading: () -> Leading,
        @ViewBuilder trailingBadge: () -> TrailingBadge
    ) {
        self.title = title
        self.subtitle = subtitle
        self.isSelected = isSelected
        self.action = action
        self.leading = leading()
        self.trailingBadge = trailingBadge()
    }

    public var body: some View {
        Button(action: action) {
            HStack(alignment: subtitle == nil ? .center : .top, spacing: 8) {
                leading
                    .padding(.top, subtitle == nil ? 0 : 2)

                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.system(size: 13.5, weight: isSelected ? .medium : .regular))
                        .foregroundStyle(isSelected ? palette.fgPrimary : palette.fgPrimary.opacity(0.88))
                        .lineLimit(1)
                        .truncationMode(.middle)

                    if let subtitle, !subtitle.isEmpty {
                        Text(subtitle)
                            .font(.system(size: 11.5, weight: .regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(2)
                    }
                }
                .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)

                trailingBadge

                if isSelected {
                    Image(systemName: "checkmark")
                        .font(.system(size: 12.5, weight: .semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .padding(.top, subtitle == nil ? 0 : 2)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, subtitle == nil ? 0 : 7)
            .frame(minHeight: subtitle == nil ? 36 : 46)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(isSelected ? palette.activeRowFill.opacity(0.85) : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

extension OrbitPopoverOptionRow where Leading == OrbitPopoverLeadingIcon, TrailingBadge == EmptyView {
    public init(
        title: String,
        subtitle: String? = nil,
        systemImage: String? = nil,
        iconColor: Color? = nil,
        isSelected: Bool,
        action: @escaping () -> Void
    ) {
        self.init(
            title: title,
            subtitle: subtitle,
            isSelected: isSelected,
            action: action,
            leading: {
                OrbitPopoverLeadingIcon(systemName: systemImage, color: iconColor)
            },
            trailingBadge: { EmptyView() }
        )
    }
}

public struct OrbitPopoverActionRow: View {
    public let title: String
    public let systemImage: String
    public let action: () -> Void

    @Environment(\.orbitPalette) private var palette

    public init(title: String, systemImage: String, action: @escaping () -> Void) {
        self.title = title
        self.systemImage = systemImage
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: systemImage)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(palette.fgSecondary)
                    .frame(width: 16, alignment: .center)

                Text(title)
                    .font(.system(size: 13.5, weight: .regular))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(1)

                Spacer(minLength: 4)
            }
            .padding(.horizontal, 10)
            .frame(height: 36)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

public extension View {
    func orbitCompactPopover<PopoverContent: View>(
        isPresented: Binding<Bool>,
        arrowEdge: Edge = .top,
        palette: OrbitPalette,
        @ViewBuilder content: @escaping () -> PopoverContent
    ) -> some View {
        self.popover(
            isPresented: isPresented,
            attachmentAnchor: .rect(.bounds),
            arrowEdge: arrowEdge
        ) {
            content()
                .environment(\.orbitPalette, palette)
                .presentationCompactAdaptation(.popover)
        }
    }
}

// MARK: - Hex Color Extension

public extension Color {
    init(hex: String) {
        if let parsed = OrbitTimeLogic.parseHexColor(hex) {
            self.init(red: parsed.red, green: parsed.green, blue: parsed.blue, opacity: parsed.alpha)
        } else {
            self.init(red: 0, green: 0, blue: 0)
        }
    }
}
