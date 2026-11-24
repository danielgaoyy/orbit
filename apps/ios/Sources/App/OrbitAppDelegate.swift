import Foundation
import UIKit
import UserNotifications

/// Bridges APNs device token registration and zero-knowledge notification tap deep links into `OrbitIosStore`.
public final class OrbitAppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    @MainActor
    public weak var store: OrbitIosStore? {
        didSet {
            flushPendingEventsIfReady()
        }
    }

    private var pendingDeviceTokenHex: String?
    private var pendingNotificationUserInfo: [AnyHashable: Any]?

    public func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        let center = UNUserNotificationCenter.current()
        center.delegate = self

        // Silently refresh APNs token if user already granted notification permission.
        center.getNotificationSettings { settings in
            guard settings.authorizationStatus == .authorized
                || settings.authorizationStatus == .provisional
                || settings.authorizationStatus == .ephemeral else {
                return
            }
            DispatchQueue.main.async {
                UIApplication.shared.registerForRemoteNotifications()
            }
        }
        return true
    }

    public func application(
        _ application: UIApplication,
        didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data
    ) {
        let hex = deviceToken.map { String(format: "%02x", $0) }.joined()
        guard !hex.isEmpty else { return }
        Task { @MainActor [weak self] in
            guard let self else { return }
            self.pendingDeviceTokenHex = hex
            self.flushPendingEventsIfReady()
        }
    }

    public func application(
        _ application: UIApplication,
        didFailToRegisterForRemoteNotificationsWithError error: Error
    ) {
        Task { @MainActor [weak self] in
            self?.store?.handleApnsRegistrationError(error)
        }
    }

    // MARK: - UNUserNotificationCenterDelegate

    public func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound, .badge])
    }

    public func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let userInfo = response.notification.request.content.userInfo
        Task { @MainActor [weak self] in
            guard let self else { return }
            self.pendingNotificationUserInfo = userInfo
            self.flushPendingEventsIfReady()
        }
        completionHandler()
    }

    @MainActor
    private func flushPendingEventsIfReady() {
        guard let store else { return }
        if let token = pendingDeviceTokenHex {
            pendingDeviceTokenHex = nil
            store.handleApnsDeviceToken(token)
        }
        if let userInfo = pendingNotificationUserInfo {
            pendingNotificationUserInfo = nil
            store.handlePushNotificationTap(userInfo: userInfo)
        }
    }
}
