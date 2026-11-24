import Foundation
#if canImport(ActivityKit)
import ActivityKit
#endif

#if canImport(ActivityKit)
@available(iOS 16.2, *)
public struct OrbitAgentActivityAttributes: ActivityAttributes {
    public struct ContentState: Codable, Hashable {
        /// Matches Rust daemon `PushDispatchPayload.event_kind` (`permission_required`, `turn_completed`, `agent_error`, `goal_finished`, `timer_fired`, `running`).
        public var eventKind: String
        public var hostId: String
        public var agentId: String?
        public var title: String
        public var body: String
        public var deepLink: String

        /// Optional local presentation fields populated during in-app `Activity.update`.
        public var hostName: String
        public var agentTitle: String
        public var statusLabel: String
        public var isRunning: Bool
        public var pendingDecisionCount: Int
        public var updatedAtMs: Int64

        public init(
            eventKind: String = "running",
            hostId: String,
            agentId: String? = nil,
            title: String,
            body: String,
            deepLink: String,
            hostName: String,
            agentTitle: String,
            statusLabel: String,
            isRunning: Bool,
            pendingDecisionCount: Int,
            updatedAtMs: Int64 = Int64(Date().timeIntervalSince1970 * 1000)
        ) {
            self.eventKind = eventKind
            self.hostId = hostId
            self.agentId = agentId
            self.title = title
            self.body = body
            self.deepLink = deepLink
            self.hostName = hostName
            self.agentTitle = agentTitle
            self.statusLabel = statusLabel
            self.isRunning = isRunning
            self.pendingDecisionCount = pendingDecisionCount
            self.updatedAtMs = updatedAtMs
        }

        private enum CodingKeys: String, CodingKey {
            case eventKind = "event_kind"
            case hostId = "host_id"
            case agentId = "agent_id"
            case title
            case body
            case deepLink = "deep_link"
            case hostName = "host_name"
            case agentTitle = "agent_title"
            case statusLabel = "status_label"
            case isRunning = "is_running"
            case pendingDecisionCount = "pending_decision_count"
            case updatedAtMs = "updated_at_ms"
        }

        public init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            let eventKind = try c.decodeIfPresent(String.self, forKey: .eventKind) ?? "running"
            let hostId = try c.decodeIfPresent(String.self, forKey: .hostId) ?? ""
            let agentId = try c.decodeIfPresent(String.self, forKey: .agentId)
            let title = try c.decodeIfPresent(String.self, forKey: .title) ?? "Orbit"
            let body = try c.decodeIfPresent(String.self, forKey: .body) ?? ""
            let deepLink = try c.decodeIfPresent(String.self, forKey: .deepLink)
                ?? Self.fallbackDeepLink(hostId: hostId, agentId: agentId)

            self.eventKind = eventKind
            self.hostId = hostId
            self.agentId = agentId
            self.title = title
            self.body = body
            self.deepLink = deepLink

            let derivedHostName = title.components(separatedBy: "·").last?.trimmingCharacters(in: .whitespacesAndNewlines)
            self.hostName = try c.decodeIfPresent(String.self, forKey: .hostName)
                ?? (derivedHostName?.isEmpty == false ? derivedHostName! : hostId)
            self.agentTitle = try c.decodeIfPresent(String.self, forKey: .agentTitle) ?? title
            let inferredRunning = (eventKind == "running")
            self.isRunning = try c.decodeIfPresent(Bool.self, forKey: .isRunning) ?? inferredRunning
            let inferredPending = (eventKind == "permission_required") ? 1 : 0
            self.pendingDecisionCount = try c.decodeIfPresent(Int.self, forKey: .pendingDecisionCount) ?? inferredPending
            self.statusLabel = try c.decodeIfPresent(String.self, forKey: .statusLabel)
                ?? Self.defaultStatusLabel(for: eventKind)
            self.updatedAtMs = try c.decodeIfPresent(Int64.self, forKey: .updatedAtMs)
                ?? Int64(Date().timeIntervalSince1970 * 1000)
        }

        public func encode(to encoder: Encoder) throws {
            var c = encoder.container(keyedBy: CodingKeys.self)
            try c.encode(eventKind, forKey: .eventKind)
            try c.encode(hostId, forKey: .hostId)
            try c.encodeIfPresent(agentId, forKey: .agentId)
            try c.encode(title, forKey: .title)
            try c.encode(body, forKey: .body)
            try c.encode(deepLink, forKey: .deepLink)
            try c.encode(hostName, forKey: .hostName)
            try c.encode(agentTitle, forKey: .agentTitle)
            try c.encode(statusLabel, forKey: .statusLabel)
            try c.encode(isRunning, forKey: .isRunning)
            try c.encode(pendingDecisionCount, forKey: .pendingDecisionCount)
            try c.encode(updatedAtMs, forKey: .updatedAtMs)
        }

        public var needsAttention: Bool {
            pendingDecisionCount > 0 || eventKind == "permission_required"
        }

        public var isError: Bool {
            eventKind == "agent_error"
        }

        private static func fallbackDeepLink(hostId: String, agentId: String?) -> String {
            guard !hostId.isEmpty else { return "orbit://open" }
            if let agentId, !agentId.isEmpty {
                return "orbit://open?host=\(hostId)&agent=\(agentId)"
            }
            return "orbit://open?host=\(hostId)"
        }

        private static func defaultStatusLabel(for eventKind: String) -> String {
            switch eventKind {
            case "permission_required": return "Approval needed"
            case "turn_completed": return "Completed"
            case "agent_error": return "Error"
            case "goal_finished": return "Goal finished"
            case "timer_fired": return "Trigger fired"
            default: return "Running"
            }
        }
    }

    public var hostId: String
    public var agentId: String
    public var providerLabel: String
    public var startedAt: Date

    public init(hostId: String, agentId: String, providerLabel: String, startedAt: Date = Date()) {
        self.hostId = hostId
        self.agentId = agentId
        self.providerLabel = providerLabel
        self.startedAt = startedAt
    }
}
#endif
