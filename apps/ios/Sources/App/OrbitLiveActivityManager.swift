import Foundation
import UIKit
#if canImport(ActivityKit)
import ActivityKit
#endif

/// Manages the ActivityKit Live Activity lifecycle and observes remote APNs push token updates
/// so Orbit daemons can update the Lock Screen and Dynamic Island even while the app is backgrounded.
@MainActor
public final class OrbitLiveActivityManager: ObservableObject {
    public static let shared = OrbitLiveActivityManager()

    @Published public private(set) var isActivityActive: Bool = false
    @Published public private(set) var currentLiveActivityTokenHex: String? = nil

    public var onLiveActivityTokenUpdated: ((String) -> Void)?

    #if canImport(ActivityKit)
    private var tokenObservationTask: Task<Void, Never>?
    private var lastSyncedSignature: String = ""
    #endif

    private init() {}

    public var areActivitiesSupportedAndEnabled: Bool {
        #if canImport(ActivityKit)
        if #available(iOS 16.2, *) {
            return ActivityAuthorizationInfo().areActivitiesEnabled
        }
        #endif
        return false
    }

    public func syncSessionState(
        enabled: Bool,
        hostId: String,
        hostName: String,
        agentId: String,
        agentTitle: String,
        providerLabel: String,
        statusLabel: String,
        isRunning: Bool,
        pendingDecisionCount: Int
    ) {
        #if canImport(ActivityKit)
        guard #available(iOS 16.2, *) else { return }
        guard enabled && ActivityAuthorizationInfo().areActivitiesEnabled else {
            endAllActivities()
            return
        }

        let shouldDisplay = isRunning || pendingDecisionCount > 0
        guard shouldDisplay else {
            endActivityGracefully(
                hostId: hostId,
                hostName: hostName,
                agentId: agentId,
                agentTitle: agentTitle,
                statusLabel: statusLabel
            )
            return
        }

        let eventKind = pendingDecisionCount > 0 ? "permission_required" : "running"
        let cleanTitle = agentTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            ? "Orbit Agent"
            : agentTitle
        let bodyText = pendingDecisionCount > 0
            ? (pendingDecisionCount == 1 ? "Decision waiting" : "\(pendingDecisionCount) decisions waiting")
            : statusLabel
        let deepLink = "orbit://open?host=\(hostId)&agent=\(agentId)"

        let signature = "\(hostId)|\(agentId)|\(cleanTitle)|\(statusLabel)|\(isRunning)|\(pendingDecisionCount)"
        if signature == lastSyncedSignature && isActivityActive {
            return
        }
        lastSyncedSignature = signature

        let contentState = OrbitAgentActivityAttributes.ContentState(
            eventKind: eventKind,
            hostId: hostId,
            agentId: agentId,
            title: "\(cleanTitle) · \(hostName)",
            body: bodyText,
            deepLink: deepLink,
            hostName: hostName,
            agentTitle: cleanTitle,
            statusLabel: statusLabel,
            isRunning: isRunning,
            pendingDecisionCount: pendingDecisionCount
        )

        let existingActivities = Activity<OrbitAgentActivityAttributes>.activities
        if let active = existingActivities.first(where: { $0.attributes.hostId == hostId && $0.attributes.agentId == agentId }) {
            isActivityActive = true
            observePushTokens(for: active)
            Task {
                await active.update(ActivityContent(state: contentState, staleDate: Date().addingTimeInterval(1800)))
            }
            return
        }

        // End stale activities for other sessions so Dynamic Island stays calm and singular.
        for stale in existingActivities {
            Task {
                await stale.end(nil, dismissalPolicy: .immediate)
            }
        }

        let attributes = OrbitAgentActivityAttributes(
            hostId: hostId,
            agentId: agentId,
            providerLabel: providerLabel,
            startedAt: Date()
        )
        do {
            let activity = try Activity<OrbitAgentActivityAttributes>.request(
                attributes: attributes,
                content: ActivityContent(state: contentState, staleDate: Date().addingTimeInterval(1800)),
                pushType: .token
            )
            isActivityActive = true
            observePushTokens(for: activity)
        } catch {
            isActivityActive = false
        }
        #endif
    }

    public func endAllActivities() {
        #if canImport(ActivityKit)
        guard #available(iOS 16.2, *) else { return }
        tokenObservationTask?.cancel()
        tokenObservationTask = nil
        lastSyncedSignature = ""
        isActivityActive = false
        for activity in Activity<OrbitAgentActivityAttributes>.activities {
            Task {
                await activity.end(nil, dismissalPolicy: .immediate)
            }
        }
        #endif
    }

    #if canImport(ActivityKit)
    @available(iOS 16.2, *)
    private func endActivityGracefully(
        hostId: String,
        hostName: String,
        agentId: String,
        agentTitle: String,
        statusLabel: String
    ) {
        let activities = Activity<OrbitAgentActivityAttributes>.activities
        guard !activities.isEmpty else {
            isActivityActive = false
            return
        }
        let finalState = OrbitAgentActivityAttributes.ContentState(
            eventKind: "turn_completed",
            hostId: hostId,
            agentId: agentId,
            title: "\(agentTitle) · \(hostName)",
            body: statusLabel,
            deepLink: "orbit://open?host=\(hostId)&agent=\(agentId)",
            hostName: hostName,
            agentTitle: agentTitle,
            statusLabel: statusLabel,
            isRunning: false,
            pendingDecisionCount: 0
        )
        lastSyncedSignature = ""
        isActivityActive = false
        for activity in activities {
            Task {
                await activity.end(
                    ActivityContent(state: finalState, staleDate: nil),
                    dismissalPolicy: .after(Date().addingTimeInterval(120))
                )
            }
        }
    }

    @available(iOS 16.2, *)
    private func observePushTokens(for activity: Activity<OrbitAgentActivityAttributes>) {
        tokenObservationTask?.cancel()
        tokenObservationTask = Task { [weak self] in
            for await tokenData in activity.pushTokenUpdates {
                let hex = tokenData.map { String(format: "%02x", $0) }.joined()
                guard !hex.isEmpty else { continue }
                await MainActor.run {
                    guard let self else { return }
                    if self.currentLiveActivityTokenHex != hex {
                        self.currentLiveActivityTokenHex = hex
                        self.onLiveActivityTokenUpdated?(hex)
                    }
                }
            }
        }
    }
    #endif
}
