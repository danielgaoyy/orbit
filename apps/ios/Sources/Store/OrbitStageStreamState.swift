import Foundation
import SwiftUI

/// Isolated high-frequency observable slice for the active conversation transcript & decision dock.
/// Separating this from `OrbitIosStore` prevents 20–40 Hz token deltas from triggering `objectWillChange`
/// across Sidebar, Settings, Composer, Search, and Shell containers.
@MainActor
public final class OrbitStageStreamState: ObservableObject {
    @Published public internal(set) var agentStage: FfiAgentStage? = nil
    @Published public internal(set) var agentStageHostId: String? = nil
    @Published public internal(set) var stageHasOlderTurns: Bool = false
    @Published public internal(set) var isLoadingOlderTurns: Bool = false
    @Published public internal(set) var isStageHydrating: Bool = false
    @Published public internal(set) var activeStageLoadError: String? = nil

    public init() {}

    func setAgentStage(_ next: FfiAgentStage?) {
        if agentStage != next {
            agentStage = next
        }
    }

    func setAgentStageHostId(_ next: String?) {
        if agentStageHostId != next {
            agentStageHostId = next
        }
    }

    func setStageHasOlderTurns(_ next: Bool) {
        if stageHasOlderTurns != next {
            stageHasOlderTurns = next
        }
    }

    func setIsLoadingOlderTurns(_ next: Bool) {
        if isLoadingOlderTurns != next {
            isLoadingOlderTurns = next
        }
    }

    func setIsStageHydrating(_ next: Bool) {
        if isStageHydrating != next {
            isStageHydrating = next
        }
    }

    func setActiveStageLoadError(_ next: String?) {
        if activeStageLoadError != next {
            activeStageLoadError = next
        }
    }
}

/// Isolated high-frequency observable slice for interactive PTY frames and scrollback history.
@MainActor
public final class OrbitTerminalState: ObservableObject {
    @Published public internal(set) var terminalScreen: FfiTerminalScreen? = nil
    /// Per-terminalId rolling scrollback text history (up to 5,000 lines per session).
    @Published public internal(set) var scrollbackByTerminalId: [String: [String]] = [:]

    private let maxScrollbackLines = 5000

    public init() {}

    public func scrollbackLines(for terminalId: String?) -> [String] {
        guard let tid = terminalId, !tid.isEmpty else { return [] }
        return scrollbackByTerminalId[tid] ?? []
    }

    func clearScrollback(for terminalId: String) {
        if scrollbackByTerminalId[terminalId] != nil {
            scrollbackByTerminalId.removeValue(forKey: terminalId)
        }
    }

    func setTerminalScreen(_ next: FfiTerminalScreen?, terminalId: String? = nil, coreScrollback: [String]? = nil) {
        if let screen = next, let tid = terminalId, !tid.isEmpty {
            if let coreScrollback, !coreScrollback.isEmpty {
                ingestCoreScrollback(coreScrollback, screen: screen, terminalId: tid)
            } else {
                ingestScreenIntoScrollback(screen, terminalId: tid)
            }
        }
        if terminalScreen != next {
            terminalScreen = next
        }
    }

    private func ingestCoreScrollback(_ coreScrollback: [String], screen: FfiTerminalScreen, terminalId tid: String) {
        var visibleLines: [String] = screen.rowsData.map { row in
            row.spans
                .map(\.text)
                .joined()
                .replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression)
        }
        while let last = visibleLines.last, last.isEmpty {
            visibleLines.removeLast()
        }
        var combined = coreScrollback
        combined.append(contentsOf: visibleLines)
        if combined.count > maxScrollbackLines {
            combined = Array(combined.suffix(maxScrollbackLines))
        }
        if scrollbackByTerminalId[tid] != combined {
            scrollbackByTerminalId[tid] = combined
        }
    }

    /// Extract visible screen lines (trimmed trailing whitespace) and append any scrolled-off prefix
    /// or novel tail lines to a bounded ring buffer so users can scroll back and copy full history.
    private func ingestScreenIntoScrollback(_ screen: FfiTerminalScreen, terminalId tid: String) {
        guard !tid.isEmpty, !screen.rowsData.isEmpty else { return }

        var visibleLines: [String] = screen.rowsData.map { row in
            row.spans
                .map(\.text)
                .joined()
                .replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression)
        }
        while let last = visibleLines.last, last.isEmpty {
            visibleLines.removeLast()
        }
        guard !visibleLines.isEmpty else { return }

        let prior = scrollbackByTerminalId[tid] ?? []
        let updated = OrbitTerminalScrollbackLogic.merge(
            history: prior,
            visibleLines: visibleLines,
            windowRows: Int(screen.rows),
            maxLines: maxScrollbackLines
        )
        if scrollbackByTerminalId[tid] != updated {
            scrollbackByTerminalId[tid] = updated
        }
    }
}
