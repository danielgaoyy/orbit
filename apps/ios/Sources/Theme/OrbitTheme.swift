import SwiftUI
import UIKit

// MARK: - Appearance Mode

public enum OrbitAppearanceMode: String, CaseIterable, Identifiable, Sendable {
    case system
    case light
    case dark
    case oled

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .system: return "System"
        case .light: return "Light"
        case .dark: return "Graphite"
        case .oled: return "OLED"
        }
    }

    public var subtitle: String {
        switch self {
        case .system: return "Match system"
        case .light: return "Daylight paper"
        case .dark: return "Quiet graphite"
        case .oled: return "True pitch black"
        }
    }

    public var preferredColorScheme: ColorScheme? {
        switch self {
        case .system: return nil
        case .light: return .light
        case .dark, .oled: return .dark
        }
    }

    public var userInterfaceStyle: UIUserInterfaceStyle {
        switch self {
        case .system: return .unspecified
        case .light: return .light
        case .dark, .oled: return .dark
        }
    }

    public func resolvedPalette(systemScheme: ColorScheme) -> OrbitPalette {
        switch self {
        case .light:
            return .light
        case .dark:
            return .graphiteDark
        case .oled:
            return .oledPure
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
        removed: Color(hex: "#E11D48")
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
        removed: Color(hex: "#FB7185")
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
        removed: Color(hex: "#FB7185")
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
    public static let drawerWidth: CGFloat = 296
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

    public static func resetCountdown(rfc3339: String?) -> String? {
        guard let raw = rfc3339?.trimmingCharacters(in: .whitespacesAndNewlines), !raw.isEmpty else {
            return nil
        }
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        var date = formatter.date(from: raw) ?? ISO8601DateFormatter().date(from: raw)
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
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let date = formatter.date(from: rfc3339) ?? ISO8601DateFormatter().date(from: rfc3339)
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

    public static func decisionLabel(_ raw: String) -> String {
        let lower = raw.lowercased()
        if lower == "allow" || lower == "accept" {
            return "Allow"
        } else if lower == "allow_always" || lower == "always_allow" || lower == "allow_session" {
            return "Always Allow"
        } else if lower == "deny" || lower == "reject" {
            return "Deny"
        }
        return raw
            .replacingOccurrences(of: "_", with: " ")
            .capitalized
    }
}

// MARK: - Hex Color Extension

public extension Color {
    init(hex: String) {
        let clean = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var rgb: UInt64 = 0
        Scanner(string: clean).scanHexInt64(&rgb)

        switch clean.count {
        case 8:
            let a = Double((rgb >> 24) & 0xFF) / 255.0
            let r = Double((rgb >> 16) & 0xFF) / 255.0
            let g = Double((rgb >> 8) & 0xFF) / 255.0
            let b = Double(rgb & 0xFF) / 255.0
            self.init(red: r, green: g, blue: b, opacity: a)
        default:
            let r = Double((rgb >> 16) & 0xFF) / 255.0
            let g = Double((rgb >> 8) & 0xFF) / 255.0
            let b = Double(rgb & 0xFF) / 255.0
            self.init(red: r, green: g, blue: b)
        }
    }
}
