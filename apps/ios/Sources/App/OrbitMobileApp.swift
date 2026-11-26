import SwiftUI
import UIKit

@main
struct OrbitMobileApp: App {
    @UIApplicationDelegateAdaptor(OrbitAppDelegate.self) private var appDelegate
    @StateObject private var store = OrbitIosStore()
    @AppStorage("orbit.appearance") private var appearanceRaw: String = OrbitAppearanceMode.system.rawValue
    @State private var debugAppearanceOverride: OrbitAppearanceMode?
    @Environment(\.scenePhase) private var scenePhase

    private var appearanceMode: OrbitAppearanceMode {
        debugAppearanceOverride ?? OrbitAppearanceMode.fromStorage(appearanceRaw)
    }

    private var appearanceBinding: Binding<String> {
        Binding(
            get: { appearanceMode.rawValue },
            set: { newValue in
                debugAppearanceOverride = nil
                appearanceRaw = newValue
            }
        )
    }

    var body: some Scene {
        WindowGroup {
            OrbitThemedRootContainer(
                store: store,
                appearanceRaw: appearanceBinding,
                appearanceMode: appearanceMode
            )
            .onOpenURL { url in
                store.handleDeepLink(url)
            }
            .onAppear {
                appDelegate.store = store
                store.bindLiveActivityManager()
                store.applyLaunchArguments(CommandLine.arguments) { mode in
                    debugAppearanceOverride = mode
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
        .environment(\.locale, store.preferredLocale)
        .id(store.preferredLanguageCode)
        .environment(\.orbitDiagramRenderer) { kind, source, isDark in
            await store.renderDiagram(kind: kind, source: source, isDark: isDark)
        }
        .preferredColorScheme(appearanceMode.preferredColorScheme)
        .tint(resolvedPalette.primary)
        .onAppear {
            syncWindowInterfaceStyle(appearanceMode.userInterfaceStyle)
        }
        .onChange(of: appearanceMode) { newMode in
            syncWindowInterfaceStyle(newMode.userInterfaceStyle)
        }
    }

    private func syncWindowInterfaceStyle(_ style: UIUserInterfaceStyle) {
        for scene in UIApplication.shared.connectedScenes {
            guard let windowScene = scene as? UIWindowScene else { continue }
            for window in windowScene.windows where window.overrideUserInterfaceStyle != style {
                window.overrideUserInterfaceStyle = style
            }
        }
    }
}
