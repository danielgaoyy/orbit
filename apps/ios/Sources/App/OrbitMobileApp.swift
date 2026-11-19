import SwiftUI

@main
struct OrbitMobileApp: App {
    @StateObject private var store = OrbitIosStore()
    @AppStorage("orbit.appearance") private var appearanceRaw: String = OrbitAppearanceMode.system.rawValue
    @Environment(\.scenePhase) private var scenePhase

    init() {
        let args = CommandLine.arguments
        if let themeIdx = args.firstIndex(of: "-orbitTheme"), themeIdx + 1 < args.count {
            let raw = args[themeIdx + 1].lowercased()
            let resolved: OrbitAppearanceMode? = {
                if raw == "graphite" {
                    return .dark
                }
                return OrbitAppearanceMode(rawValue: raw)
            }()
            if let mode = resolved {
                UserDefaults.standard.set(mode.rawValue, forKey: "orbit.appearance")
            }
        }
    }

    private var appearanceMode: OrbitAppearanceMode {
        OrbitAppearanceMode(rawValue: appearanceRaw) ?? .system
    }

    var body: some Scene {
        WindowGroup {
            OrbitThemedRootContainer(
                store: store,
                appearanceRaw: $appearanceRaw,
                appearanceMode: appearanceMode
            )
            .preferredColorScheme(appearanceMode.preferredColorScheme)
            .onOpenURL { url in
                store.handleDeepLink(url)
            }
            .onAppear {
                store.applyLaunchArguments(CommandLine.arguments) { mode in
                    appearanceRaw = mode.rawValue
                }
            }
        }
        .onChange(of: scenePhase) { newPhase in
            switch newPhase {
            case .active:
                store.onForeground()
            case .background:
                store.onBackground()
            case .inactive:
                break
            @unknown default:
                break
            }
        }
    }
}

private struct OrbitThemedRootContainer: View {
    @ObservedObject var store: OrbitIosStore
    @Binding var appearanceRaw: String
    let appearanceMode: OrbitAppearanceMode
    @Environment(\.colorScheme) private var systemColorScheme

    private var resolvedPalette: OrbitPalette {
        appearanceMode.resolvedPalette(systemScheme: systemColorScheme)
    }

    var body: some View {
        AdaptiveWorkspaceShell(
            store: store,
            appearanceRaw: $appearanceRaw
        )
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(resolvedPalette.canvas.ignoresSafeArea())
        .environment(\.orbitPalette, resolvedPalette)
        .preferredColorScheme(appearanceMode.preferredColorScheme)
        .tint(resolvedPalette.primary)
    }
}
