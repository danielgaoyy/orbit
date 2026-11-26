import Foundation

// MARK: - Path & File Target Logic

public enum OrbitPathLogic {
    /// Normalizes path separators, collapses repeated slashes, and drops trailing slashes except for `/`.
    public static func normalizePath(_ raw: String) -> String {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "" }

        var result = trimmed.replacingOccurrences(of: "\\", with: "/")
        // Preserve URL scheme slashes when normalizing file URLs elsewhere; here collapse repeated slashes.
        while result.contains("//") {
            result = result.replacingOccurrences(of: "//", with: "/")
        }
        if result.count > 1, result.hasSuffix("/") {
            result.removeLast()
        }
        return result
    }

    /// Abbreviates `/Users/<name>` or `/home/<name>` prefixes to `~`.
    public static func compactPath(_ raw: String) -> String {
        let norm = normalizePath(raw)
        guard !norm.isEmpty else { return "" }

        for homePrefix in ["/Users/", "/home/"] {
            if norm.hasPrefix(homePrefix) {
                let rest = String(norm.dropFirst(homePrefix.count))
                if let slashIdx = rest.firstIndex(of: "/") {
                    let tail = String(rest[slashIdx...])
                    return "~" + tail
                }
                if !rest.isEmpty {
                    return "~"
                }
            }
        }
        return norm
    }

    /// Shortens long paths while keeping up to the trailing two path components for orientation.
    public static func shortPath(_ raw: String) -> String {
        let compacted = compactPath(raw)
        guard !compacted.isEmpty else { return "" }
        if compacted == "/" || compacted == "~" {
            return compacted
        }

        let parts = compacted.split(separator: "/", omittingEmptySubsequences: true)
        if parts.count <= 2 {
            return compacted
        }
        if compacted.hasPrefix("~/"), parts.count == 3 {
            return compacted
        }
        return ".../" + parts.suffix(2).joined(separator: "/")
    }

    /// Returns the relative path inside `cwd` when `rawPath` lies inside `cwd` (handling `~` vs `/Users/<name>`),
    /// `.` when equal to `cwd`, or `shortPath(rawPath)` when outside `cwd`.
    public static func stripWorkspacePrefix(_ rawPath: String, cwd: String) -> String {
        let trimmedRaw = rawPath.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedRaw.isEmpty else { return "" }

        let normPath = normalizePath(trimmedRaw)
        let normCwd = normalizePath(cwd)
        let compactRaw = compactPath(normPath)
        let compactCwd = compactPath(normCwd)

        if !normCwd.isEmpty {
            if normPath == normCwd || compactRaw == compactCwd {
                return "."
            }
            for (candidatePath, candidateCwd) in [(normPath, normCwd), (compactRaw, compactCwd)] where !candidateCwd.isEmpty {
                let prefix = candidateCwd == "/" ? "/" : candidateCwd + "/"
                if candidatePath.hasPrefix(prefix) {
                    let rel = String(candidatePath.dropFirst(prefix.count))
                    return rel.isEmpty ? "." : rel
                }
            }
        }

        if normPath.hasPrefix("./") {
            return String(normPath.dropFirst(2))
        }
        if !normPath.hasPrefix("/") && !normPath.hasPrefix("~/") && normPath != "~" {
            return normPath
        }
        return shortPath(normPath)
    }

    /// Returns the workspace-relative path for display labels, keeping relative directory structure intact.
    public static func shortFileLabel(_ path: String, rootCwd: String) -> String {
        stripWorkspacePrefix(path, cwd: rootCwd)
    }

    /// Resolves a board or file reference into a full or workspace-relative path without discarding parent folders.
    public static func resolveWorkspaceFilePath(_ reference: String, rootCwd: String?) -> String {
        let parsed = parseFileTargetAndLine(reference, cwd: nil)
        let path = parsed.path
        guard let rootCwd, !rootCwd.isEmpty else {
            return path
        }
        let normPath = normalizePath(path)
        let normRoot = normalizePath(rootCwd)
        let compactRoot = compactPath(normRoot)
        let compactTarget = compactPath(normPath)
        if normPath.hasPrefix("/") || normPath.hasPrefix("~") {
            let prefix = normRoot == "/" ? "/" : normRoot + "/"
            if normPath.hasPrefix(prefix) {
                return String(normPath.dropFirst(prefix.count))
            }
            let compactPrefix = compactRoot == "/" ? "/" : compactRoot + "/"
            if compactTarget.hasPrefix(compactPrefix) {
                return String(compactTarget.dropFirst(compactPrefix.count))
            }
            return normPath
        }
        if normPath.hasPrefix("./") {
            return String(normPath.dropFirst(2))
        }
        return normPath
    }

    public static func basename(_ path: String) -> String {
        let norm = normalizePath(path)
        guard !norm.isEmpty else { return "" }
        if norm == "/" || norm == "~" { return norm }
        return (norm as NSString).lastPathComponent
    }

    public static func pathExtension(_ path: String) -> String {
        let parsed = parseFileTargetAndLine(path, cwd: nil)
        return (parsed.path as NSString).pathExtension.lowercased()
    }

    public static func isPreviewableImagePath(_ path: String) -> Bool {
        switch pathExtension(path) {
        case "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "heic", "tiff", "tif":
            return true
        default:
            return false
        }
    }

    public static func isMarkdownPath(_ path: String) -> Bool {
        switch pathExtension(path) {
        case "md", "mdx", "markdown", "rmd":
            return true
        default:
            return false
        }
    }

    public struct FileTarget: Equatable, Sendable {
        public let path: String
        public let line: Int?
        public let endLine: Int?
    }

    /// Parses file references such as `file:///path/to/file.swift#L12-L20`, `src/foo.rs:42`, or `src/foo.rs:42:5`.
    public static func parseFileTargetAndLine(_ rawTarget: String, cwd: String? = nil) -> FileTarget {
        var working = rawTarget.trimmingCharacters(in: .whitespacesAndNewlines)
        if working.hasPrefix("file://") {
            if let url = URL(string: working), url.isFileURL {
                let pathPart = url.path
                if let fragment = url.fragment, !fragment.isEmpty {
                    working = pathPart + "#" + fragment
                } else {
                    working = pathPart
                }
            } else {
                working = String(working.dropFirst("file://".count))
                if let decoded = working.removingPercentEncoding {
                    working = decoded
                }
            }
        }

        var line: Int?
        var endLine: Int?

        if let hashRange = working.range(of: "#", options: .backwards) {
            let fragment = String(working[hashRange.upperBound...])
            let base = String(working[..<hashRange.lowerBound])
            let parsedRange = parseLineFragment(fragment)
            if parsedRange.line != nil {
                line = parsedRange.line
                endLine = parsedRange.endLine
                working = base
            }
        }

        if line == nil {
            // Match trailing `:line` or `:line:column` without breaking Windows drive prefixes (`C:\...`).
            let parts = working.split(separator: ":", omittingEmptySubsequences: false)
            if parts.count >= 2, let lastNum = Int(parts[parts.count - 1]), lastNum > 0 {
                if parts.count >= 3, let prevNum = Int(parts[parts.count - 2]), prevNum > 0 {
                    // `:line:col`
                    let prefixParts = parts.dropLast(2)
                    let joined = prefixParts.joined(separator: ":")
                    if !joined.isEmpty {
                        working = joined
                        line = prevNum
                    }
                } else {
                    let prefixParts = parts.dropLast(1)
                    let joined = prefixParts.joined(separator: ":")
                    if !joined.isEmpty {
                        working = joined
                        line = lastNum
                    }
                }
            }
        }

        let normalized = normalizePath(working)
        let resolved: String
        if let cwd, !cwd.isEmpty {
            resolved = stripWorkspacePrefix(normalized, cwd: cwd)
        } else {
            resolved = normalized
        }
        return FileTarget(path: resolved, line: line, endLine: endLine)
    }

    private static func parseLineFragment(_ fragment: String) -> (line: Int?, endLine: Int?) {
        let upper = fragment.uppercased()
        guard upper.hasPrefix("L") else { return (nil, nil) }
        let body = String(upper.dropFirst())
        if let dashIdx = body.firstIndex(of: "-") {
            let startStr = String(body[..<dashIdx]).replacingOccurrences(of: "L", with: "")
            let endStr = String(body[body.index(after: dashIdx)...]).replacingOccurrences(of: "L", with: "")
            if let start = Int(startStr), start > 0 {
                let end = Int(endStr)
                return (start, (end ?? 0) > 0 ? end : nil)
            }
            return (nil, nil)
        }
        if let single = Int(body), single > 0 {
            return (single, nil)
        }
        return (nil, nil)
    }
}

// MARK: - Markdown Links & Table Guard Logic

public enum OrbitMarkdownLinkLogic {
    public enum ParsedLink: Equatable, Sendable {
        case externalWeb(URL)
        case mailto(URL)
        case filePath(OrbitPathLogic.FileTarget)
        case anchor(String)
        case unknown(String)
    }

    public static func parseLink(_ raw: String, workspaceCwd: String? = nil) -> ParsedLink {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return .unknown(raw) }

        if trimmed.hasPrefix("#") {
            return .anchor(String(trimmed.dropFirst()))
        }

        let lower = trimmed.lowercased()
        if lower.hasPrefix("http://") || lower.hasPrefix("https://") {
            if let url = URL(string: trimmed) {
                return .externalWeb(url)
            }
        }
        if lower.hasPrefix("mailto:") {
            if let url = URL(string: trimmed) {
                return .mailto(url)
            }
        }
        if lower.hasPrefix("file://")
            || trimmed.hasPrefix("/")
            || trimmed.hasPrefix("./")
            || trimmed.hasPrefix("../")
            || trimmed.hasPrefix("~/")
            || looksLikeWorkspaceFilePath(trimmed) {
            return .filePath(OrbitPathLogic.parseFileTargetAndLine(trimmed, cwd: workspaceCwd))
        }
        if let url = URL(string: trimmed), let scheme = url.scheme, !scheme.isEmpty {
            return .externalWeb(url)
        }
        return .unknown(trimmed)
    }

    private static func looksLikeWorkspaceFilePath(_ raw: String) -> Bool {
        let noFrag = raw.split(separator: "#", maxSplits: 1).first.map(String.init) ?? raw
        let noLine = OrbitPathLogic.parseFileTargetAndLine(noFrag, cwd: nil).path
        if noLine.contains("/") {
            return !noLine.contains(" ")
        }
        let ext = (noLine as NSString).pathExtension
        return !ext.isEmpty && !noLine.contains(" ")
    }

    /// Returns true if lines use Unicode box-drawing characters (like `│`, `┌`, `─`) rather than Markdown `|---|` tables.
    public static func isASCIIBoxTableFalsePositive(_ lines: [String]) -> Bool {
        let boxChars: Set<Character> = [
            "│", "┃", "┌", "┐", "└", "┘", "├", "┤", "┬", "┴", "┼",
            "─", "━", "═", "║", "╔", "╗", "╚", "╝", "╠", "╣", "╦", "╩", "╬"
        ]
        var hasBoxDrawing = false
        var hasStandardPipeSeparator = false

        for line in lines {
            if line.contains(where: { boxChars.contains($0) }) {
                hasBoxDrawing = true
            }
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.contains("|") && trimmed.contains("---") {
                let allowed = CharacterSet(charactersIn: "|-: \t")
                if trimmed.unicodeScalars.allSatisfy({ allowed.contains($0) }) {
                    hasStandardPipeSeparator = true
                }
            }
        }

        return hasBoxDrawing && !hasStandardPipeSeparator
    }
}

// MARK: - Agent Status, Scope, Models, and Badges Logic

public enum OrbitBadgeTone: String, Equatable, Sendable {
    case none
    case muted
    case accent
    case critical
}

public enum OrbitAgentLogic {
    private static let genericHandles: Set<String> = [
        "codex", "claude", "claude_code", "claude code", "agy", "antigravity",
        "gemini", "opencode", "cursor", "agent", "assistant", "session",
        "peer", "worker", "default", "orbit"
    ]

    /// True if a handle is a generic provider/harness placeholder that should not hide a richer title.
    public static func isGenericHarnessHandle(_ raw: String?) -> Bool {
        guard let raw else { return true }
        let trimmed = raw
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "@"))
            .lowercased()
        if trimmed.isEmpty { return true }
        if genericHandles.contains(trimmed) { return true }

        let prefixes = [
            "claude-code", "claude_code", "claude code",
            "antigravity", "opencode", "assistant",
            "codex", "claude", "gemini", "cursor",
            "session", "worker", "agent", "peer", "agy"
        ]
        for prefix in prefixes where trimmed.hasPrefix(prefix) {
            let suffix = trimmed.dropFirst(prefix.count)
            let stripped = suffix.drop(while: { $0 == "-" || $0 == "_" || $0 == " " })
            if stripped.isEmpty || stripped.allSatisfy({ $0.isNumber }) {
                return true
            }
        }
        return false
    }

    private static let foregroundRunningStatuses: Set<String> = [
        "running", "working", "thinking", "streaming", "starting",
        "busy", "executing", "initializing", "connecting", "resuming"
    ]

    /// True when a status label describes a background-only waiting state (matching Desktop `isAgentBackgroundWaiting`).
    public static func isBackgroundWaitingStatusLabel(_ statusRaw: String?) -> Bool {
        let normalized = (statusRaw ?? "").trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if normalized.isEmpty { return false }
        if normalized == "waiting_background"
            || normalized == "background task"
            || normalized.hasPrefix("waiting for ") {
            return true
        }
        return normalized.range(of: #"^running\s*\(.*background task"#, options: [.regularExpression, .caseInsensitive]) != nil
    }

    /// True only when the agent has an active foreground turn/execution, excluding idle background-waiting states.
    public static func isAgentForegroundRunning(
        statusRaw: String?,
        hasActiveTurn: Bool
    ) -> Bool {
        let normalized = (statusRaw ?? "").trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if isBackgroundWaitingStatusLabel(normalized) {
            return false
        }
        if normalized == "idle" || normalized == "completed" || normalized == "failed" || normalized == "error" || normalized == "stopped" || normalized == "closed" {
            return hasActiveTurn
        }
        if hasActiveTurn { return true }
        return foregroundRunningStatuses.contains(normalized)
    }

    /// True when the agent is not actively streaming in the foreground, yet still has live background tasks.
    public static func isAgentBackgroundWaiting(
        statusRaw: String?,
        hasActiveTurn: Bool,
        hasBackgroundTasks: Bool
    ) -> Bool {
        if isBackgroundWaitingStatusLabel(statusRaw) {
            return hasBackgroundTasks || !hasActiveTurn
        }
        guard hasBackgroundTasks else { return false }
        return !isAgentForegroundRunning(statusRaw: statusRaw, hasActiveTurn: hasActiveTurn)
    }

    /// Returns true when a wakeup segment represents a transient `Waiting for ...` state.
    public static func isWaitingWakeupLabel(_ label: String?) -> Bool {
        guard let trimmed = label?.trimmingCharacters(in: .whitespacesAndNewlines), !trimmed.isEmpty else {
            return false
        }
        return trimmed.range(of: #"^waiting for\b"#, options: [.regularExpression, .caseInsensitive]) != nil
    }

    /// Returns true when a wakeup segment should be hidden from the main conversation stream
    /// (matching Desktop `isHiddenWakeupLabel`: hides `Waiting for ...` and `Resumed from background task`).
    public static func isHiddenWakeupLabel(_ label: String?) -> Bool {
        guard let trimmed = label?.trimmingCharacters(in: .whitespacesAndNewlines), !trimmed.isEmpty else {
            return true
        }
        return isWaitingWakeupLabel(trimmed)
            || trimmed.range(of: #"^resumed from background task\b"#, options: [.regularExpression, .caseInsensitive]) != nil
    }

    /// Single source of truth for whether a step group is actively executing in the foreground
    /// (matching Desktop `TurnStageSection`: once prose/notice follows, or the foreground turn settles,
    /// the group folds cleanly even if it spawned a background `Schedule` / `Shell` task).
    public static func isStepGroupLiveActive(
        isTurnActive: Bool,
        hasFollowingContent: Bool,
        hasRunningStep: Bool,
        isLastVisibleSegment: Bool,
        unrecoveredFailureCount: UInt32
    ) -> Bool {
        guard isTurnActive && !hasFollowingContent else { return false }
        let isTrailingFailedGroup = !hasRunningStep && unrecoveredFailureCount > 0
        return hasRunningStep || (isLastVisibleSegment && !isTrailingFailedGroup)
    }

    /// Single source of truth for whether a `markAgentSeen` RPC should be dispatched.
    /// When `force` is true (e.g. explicitly opening a thread or clearing `AttentionState::Finished` / `ManualUnread`),
    /// always dispatch even if `nextSeq <= lastMarkedSeq` or `nextSeq == 0`.
    public static func shouldDispatchMarkAgentSeen(
        lastMarkedSeq: UInt64?,
        nextSeq: UInt64,
        force: Bool
    ) -> Bool {
        if force { return true }
        guard let prev = lastMarkedSeq else { return true }
        return nextSeq > prev
    }

    /// Provider switching is permitted only for brand-new drafts (no bound `selectedAgentId`)
    /// or when the active agent record is present and explicitly supports provider switching.
    public static func canSwitchProvider(
        selectedAgentId: String?,
        activeAgentExists: Bool,
        supportsProviderSwitch: Bool
    ) -> Bool {
        if let aid = selectedAgentId, !aid.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return activeAgentExists && supportsProviderSwitch
        }
        return true
    }

    /// Canonical deduplication key for model rows across provider aliases and punctuation variants.
    public static func modelDedupeKey(providerId: String, rawModelId: String) -> String {
        let p = normalizedProviderId(providerId)
        var m = rawModelId.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if m.hasPrefix("\(p)/") {
            m = String(m.dropFirst(p.count + 1))
        } else if m.hasPrefix("antigravity/") {
            m = String(m.dropFirst("antigravity/".count))
        } else if m.hasPrefix("agy/") {
            m = String(m.dropFirst("agy/".count))
        }
        // Normalize version separators (e.g. 4.6 -> 4-6) so alias entries collapse cleanly.
        m = m.replacingOccurrences(of: ".", with: "-")
        return "\(p)::\(m)"
    }

    public static func normalizedProviderId(_ providerId: String) -> String {
        let lower = providerId.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        switch lower {
        case "agy":
            return "antigravity"
        case "claude-code", "claude":
            return "claude_code"
        default:
            return lower
        }
    }

    public static func stageScopeKey(hostId: String?, agentId: String?) -> String? {
        guard let h = hostId?.trimmingCharacters(in: .whitespacesAndNewlines), !h.isEmpty,
              let a = agentId?.trimmingCharacters(in: .whitespacesAndNewlines), !a.isEmpty else {
            return nil
        }
        return "\(h)::\(a)"
    }

    public static func dismissedVersionKey(hostId: String, daemonVersion: String?) -> String {
        let v = (daemonVersion ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        return "\(hostId)::\(v)"
    }

    public static func overviewBadgeTone(
        hasPendingApproval: Bool,
        hasSubagentApproval: Bool,
        isError: Bool,
        hasUnread: Bool,
        hasBackgroundTasks: Bool,
        isRunning: Bool
    ) -> OrbitBadgeTone {
        if hasPendingApproval || hasSubagentApproval || isError {
            return .critical
        }
        if isRunning || hasUnread {
            return .accent
        }
        if hasBackgroundTasks {
            return .muted
        }
        return .none
    }

    /// Distinguishes intentional cleared fields vs unchanged `nil` fields when saving host overrides.
    public static func hostConfigFieldValue(edited: String, original: String?) -> String? {
        let trimmed = edited.trimmingCharacters(in: .whitespacesAndNewlines)
        let origTrimmed = (original ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            return origTrimmed.isEmpty ? nil : ""
        }
        return trimmed
    }

    /// Reorders project keys for `targetHostId` while preserving keys belonging to all other hosts.
    public static func reorderProjectKeys(
        existingOrder: [String],
        targetHostProjectKeys: [String],
        movedFromIndex: Int,
        movedToOffset: Int
    ) -> [String] {
        guard targetHostProjectKeys.indices.contains(movedFromIndex) else { return existingOrder }
        var reorderedTarget = targetHostProjectKeys
        let item = reorderedTarget.remove(at: movedFromIndex)
        let boundedDest = max(0, min(reorderedTarget.count, movedToOffset > movedFromIndex ? movedToOffset - 1 : movedToOffset))
        reorderedTarget.insert(item, at: boundedDest)

        let targetSet = Set(targetHostProjectKeys)
        var targetIter = reorderedTarget.makeIterator()
        var merged: [String] = []
        merged.reserveCapacity(max(existingOrder.count, reorderedTarget.count))

        for key in existingOrder {
            if targetSet.contains(key) {
                if let next = targetIter.next() {
                    merged.append(next)
                }
            } else {
                merged.append(key)
            }
        }
        while let remaining = targetIter.next() {
            if !merged.contains(remaining) {
                merged.append(remaining)
            }
        }
        return merged
    }
}

// MARK: - Approval & Decision Logic

public enum OrbitDecisionKind: String, Equatable, Sendable {
    case allowOnce
    case allowSession
    case allowAlways
    case decline
    case cancel
    case custom
}

public struct OrbitInspectableDecisionPayload: Equatable, Sendable {
    public let command: String?
    public let diff: String?
    public let path: String?
    public let formattedArgsJson: String?

    public var hasInspectableContent: Bool {
        command != nil || diff != nil || path != nil || formattedArgsJson != nil
    }
}

public enum OrbitDecisionLogic {
    /// Classifies approval decision identifiers safely so deny/decline/cancel never fall through to allow styling.
    public static func classify(_ raw: String) -> OrbitDecisionKind {
        let lower = raw.trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
            .replacingOccurrences(of: "-", with: "_")
            .replacingOccurrences(of: " ", with: "_")

        if lower == "cancel" || lower == "dismiss" || lower == "skip" || lower.contains("cancel") {
            return .cancel
        }
        if lower == "decline" || lower == "deny" || lower == "reject" || lower == "refuse"
            || lower == "block" || lower == "abort" || lower == "no"
            || lower.contains("decline") || lower.contains("deny") || lower.contains("reject") {
            return .decline
        }
        if lower == "allow_always" || lower == "always_allow" || lower == "always" || lower == "allow_all"
            || lower.contains("always") {
            return .allowAlways
        }
        if lower == "allow_for_session" || lower == "allow_session" || lower == "session" || lower.contains("session") {
            return .allowSession
        }
        if lower == "allow" || lower == "allow_once" || lower == "once" || lower == "approve"
            || lower == "accept" || lower == "confirm" || lower == "proceed" || lower == "continue" || lower == "yes"
            || lower.contains("allow") || lower.contains("approve") {
            return .allowOnce
        }
        return .custom
    }

    public static func decisionLabel(_ raw: String) -> String {
        switch classify(raw) {
        case .allowOnce:
            let lower = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            if lower == "approve" { return "Approve" }
            if lower == "accept" { return "Accept" }
            if lower == "allow" { return "Allow" }
            return "Allow Once"
        case .allowSession:
            return "Allow for Session"
        case .allowAlways:
            return "Always Allow"
        case .decline:
            let lower = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            if lower.contains("deny") { return "Deny" }
            if lower.contains("reject") { return "Reject" }
            return "Decline"
        case .cancel:
            return "Cancel"
        case .custom:
            let cleaned = raw
                .replacingOccurrences(of: "_", with: " ")
                .replacingOccurrences(of: "-", with: " ")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            return cleaned.isEmpty ? "Submit" : cleaned.capitalized
        }
    }

    public static func isDestructiveOrDenial(_ raw: String) -> Bool {
        let kind = classify(raw)
        return kind == .decline || kind == .cancel
    }

    public static func clampedDecisionIndex(current: Int, count: Int, delta: Int) -> Int {
        guard count > 0 else { return 0 }
        return max(0, min(count - 1, current + delta))
    }

    public static func inspectablePayload(
        rawArgsJson: String?,
        command: String?,
        diff: String?,
        path: String?
    ) -> OrbitInspectableDecisionPayload {
        let cleanCmd = nonEmptyTrimmed(command)
        let cleanDiff = nonEmptyTrimmed(diff)
        let cleanPath = nonEmptyTrimmed(path)
        var prettyJson: String?
        if let rawJson = nonEmptyTrimmed(rawArgsJson),
           let data = rawJson.data(using: .utf8),
           let obj = try? JSONSerialization.jsonObject(with: data, options: []),
           JSONSerialization.isValidJSONObject(obj),
           let prettyData = try? JSONSerialization.data(withJSONObject: obj, options: [.prettyPrinted, .sortedKeys]),
           let str = String(data: prettyData, encoding: .utf8) {
            if let dict = obj as? [String: Any], dict.isEmpty {
                prettyJson = nil
            } else if let arr = obj as? [Any], arr.isEmpty {
                prettyJson = nil
            } else {
                prettyJson = str
            }
        } else if let rawJson = nonEmptyTrimmed(rawArgsJson), rawJson != "{}" && rawJson != "[]" {
            prettyJson = rawJson
        }
        return OrbitInspectableDecisionPayload(
            command: cleanCmd,
            diff: cleanDiff,
            path: cleanPath,
            formattedArgsJson: prettyJson
        )
    }

    private static func nonEmptyTrimmed(_ value: String?) -> String? {
        guard let value else { return nil }
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

// MARK: - Composer, Mentions, Queue & Draft Scoping Logic

public enum OrbitComposerTriggerKind: String, Equatable, Sendable {
    case slash
    case mention
}

public struct OrbitComposerTrigger: Equatable, Sendable {
    public let kind: OrbitComposerTriggerKind
    public let query: String
    /// UTF-16 range covering the trigger character (`/` or `@`) plus the typed query up to the cursor.
    public let rangeLocationUtf16: Int
    public let rangeLengthUtf16: Int
}

public enum OrbitComposerLogic {
    public static let maxAttachmentBytes: Int = 25 * 1024 * 1024
    public static let largePasteCharThreshold: Int = 4_000
    public static let largePasteLineThreshold: Int = 100

    /// Maps Rust composer slash command IDs (`supervised`, `auto`, `full`, `plan`, etc.) to SF Symbols.
    public static func iconName(forSlashCommandId id: String) -> String {
        switch id.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
        case "supervised", "ask":
            return "shield"
        case "auto", "edits":
            return "checkmark.shield"
        case "full", "full_access", "bypass", "yolo":
            return "flame"
        case "plan":
            return "list.bullet.clipboard"
        case "compact":
            return "arrow.down.right.and.arrow.up.left"
        case "new":
            return "plus.bubble"
        case "clear":
            return "trash"
        case "review", "diff":
            return "doc.text.magnifyingglass"
        case "files", "file":
            return "folder"
        case "board":
            return "rectangle.3.group"
        case "pty", "terminal":
            return "terminal"
        case "subagents":
            return "person.2"
        case "model":
            return "cpu"
        case "effort":
            return "dial.medium"
        default:
            return "command"
        }
    }

    /// Matches Rust `composer.rs` workspace peer candidates (`workspace_peer`, plus legacy `peer`/`all`).
    public static func isPeerMentionCandidate(kind: String, key: String) -> Bool {
        let k = kind.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let keyLower = key.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return k == "workspace_peer" || k == "peer" || k == "all" || keyLower.hasPrefix("peer::")
    }

    public static func isAllPeerMention(key: String, handle: String) -> Bool {
        let keyLower = key.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let handleLower = handle.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return keyLower == "peer::all" || handleLower == "all" || handleLower == "@all"
    }

    /// True only when `text` starts (after optional leading whitespace) with the exact slash
    /// command token `command` (case-insensitive) followed by end-of-string or whitespace.
    /// Prevents prefix collisions like `/reviewers` or escaped `\/review` from matching `/review`.
    public static func matchesLeadingSlashCommand(_ text: String, command: String) -> Bool {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let lower = trimmed.lowercased()
        let cmdLower = command.lowercased()
        guard lower.hasPrefix(cmdLower) else { return false }
        let afterIdx = trimmed.index(trimmed.startIndex, offsetBy: cmdLower.count)
        return afterIdx == trimmed.endIndex || trimmed[afterIdx].isWhitespace
    }

    /// Finds an active `@mention` or `/slash` trigger immediately before `cursorUtf16Offset`,
    /// respecting word boundaries and ignoring emails (`user@domain`) or paths (`/Users/name`).
    public static func findActiveTrigger(in text: String, cursorUtf16Offset: Int) -> OrbitComposerTrigger? {
        let nsText = text as NSString
        let clampedCursor = max(0, min(cursorUtf16Offset, nsText.length))
        guard clampedCursor > 0 else { return nil }

        var idx = clampedCursor - 1
        while idx >= 0 {
            let code = nsText.character(at: idx)
            if isWhitespaceUtf16(code) {
                return nil
            }
            if code == 0x40 /* '@' */ || code == 0x2F /* '/' */ {
                let kind: OrbitComposerTriggerKind = (code == 0x40) ? .mention : .slash
                let validBoundary: Bool
                if idx == 0 {
                    validBoundary = true
                } else if kind == .slash {
                    // Slash commands only activate at start-of-input (ignoring leading whitespace)
                    // so mid-sentence references (`/goal`) and escaped slashes (`\/goal`) never hijack typing.
                    var allLeadingWhitespace = true
                    var j = 0
                    while j < idx {
                        if !isWhitespaceUtf16(nsText.character(at: j)) {
                            allLeadingWhitespace = false
                            break
                        }
                        j += 1
                    }
                    validBoundary = allLeadingWhitespace
                } else {
                    let prev = nsText.character(at: idx - 1)
                    validBoundary = isWhitespaceUtf16(prev)
                        || prev == 0x28 /* '(' */
                        || prev == 0x5B /* '[' */
                        || prev == 0x7B /* '{' */
                }
                if !validBoundary {
                    return nil
                }

                let queryRange = NSRange(location: idx + 1, length: clampedCursor - (idx + 1))
                let query = nsText.substring(with: queryRange)
                if kind == .slash && (query.contains("/") || query.contains(":")) {
                    return nil
                }
                return OrbitComposerTrigger(
                    kind: kind,
                    query: query,
                    rangeLocationUtf16: idx,
                    rangeLengthUtf16: clampedCursor - idx
                )
            }
            if idx == 0 { break }
            idx -= 1
        }
        return nil
    }

    /// Replaces the trigger UTF-16 range and returns the updated string plus the new UTF-16 caret position.
    public static func applyAutocompleteInsertion(
        text: String,
        trigger: OrbitComposerTrigger,
        replacement: String
    ) -> (newText: String, newCursorUtf16: Int) {
        let nsText = text as NSString
        let range = NSRange(
            location: max(0, min(trigger.rangeLocationUtf16, nsText.length)),
            length: max(0, min(trigger.rangeLengthUtf16, nsText.length - max(0, min(trigger.rangeLocationUtf16, nsText.length))))
        )
        let afterEnd = range.location + range.length
        let needsTrailingSpace = !replacement.hasSuffix(" ")
            && (afterEnd >= nsText.length || !isWhitespaceUtf16(nsText.character(at: afterEnd)))
        let inserted = needsTrailingSpace ? replacement + " " : replacement
        let updated = nsText.replacingCharacters(in: range, with: inserted)
        let newCursor = range.location + (inserted as NSString).length
        return (updated, newCursor)
    }

    /// Replaces any selected UTF-16 range (safe with emoji surrogate pairs and CRLF) and returns updated text + cursor.
    public static func insertText(
        into text: String,
        selectedRangeUtf16: NSRange,
        insertion: String
    ) -> (newText: String, newCursorUtf16: Int) {
        let nsText = text as NSString
        let loc = max(0, min(selectedRangeUtf16.location, nsText.length))
        let len = max(0, min(selectedRangeUtf16.length, nsText.length - loc))
        let safeRange = NSRange(location: loc, length: len)
        let updated = nsText.replacingCharacters(in: safeRange, with: insertion)
        let newCursor = loc + (insertion as NSString).length
        return (updated, newCursor)
    }

    public static func isLargeTextPaste(_ text: String) -> Bool {
        if text.count >= largePasteCharThreshold { return true }
        var lines = 1
        for ch in text where ch == "\n" {
            lines += 1
            if lines >= largePasteLineThreshold { return true }
        }
        return false
    }

    public static func attachmentExceedsLimit(_ byteCount: Int, maxBytes: Int = maxAttachmentBytes) -> Bool {
        byteCount > maxBytes
    }

    public static func scopedDraftKey(
        hostId: String?,
        agentId: String?,
        draftWorkspaceId: String?,
        draftCwd: String?
    ) -> String {
        let h = (hostId ?? "no-host").trimmingCharacters(in: .whitespacesAndNewlines)
        if let aid = agentId?.trimmingCharacters(in: .whitespacesAndNewlines), !aid.isEmpty {
            return "agent::\(h)::\(aid)"
        }
        if let wid = draftWorkspaceId?.trimmingCharacters(in: .whitespacesAndNewlines), !wid.isEmpty {
            return "workspace::\(h)::\(wid)"
        }
        let normCwd = OrbitPathLogic.normalizePath(draftCwd ?? "")
        return "draft::\(h)::\(normCwd)"
    }

    public static func canCreateWorktreeSession(isWorktreeMode: Bool, isGitRepo: Bool) -> Bool {
        !isWorktreeMode || isGitRepo
    }

    public static func nextUniqueQuoteId(existingIds: [String], baseTurnKey: String) -> String {
        let existing = Set(existingIds)
        if !existing.contains(baseTurnKey) {
            return baseTurnKey
        }
        var suffix = 2
        while existing.contains("\(baseTurnKey)#\(suffix)") {
            suffix += 1
        }
        return "\(baseTurnKey)#\(suffix)"
    }

    public static func mergeQueuedPrompts(_ first: String, _ second: String) -> String {
        let a = first.trimmingCharacters(in: .whitespacesAndNewlines)
        let b = second.trimmingCharacters(in: .whitespacesAndNewlines)
        if a.isEmpty { return b }
        if b.isEmpty { return a }
        return a + "\n\n" + b
    }

    public static func reorderQueueIds(_ ids: [String], fromIndex: Int, toOffset: Int) -> [String] {
        guard ids.indices.contains(fromIndex) else { return ids }
        var copy = ids
        let item = copy.remove(at: fromIndex)
        let dest = max(0, min(copy.count, toOffset > fromIndex ? toOffset - 1 : toOffset))
        copy.insert(item, at: dest)
        return copy
    }

    /// True if `token` appears in `text` delimited on both sides by start/end, whitespace, or punctuation
    /// (prevents `@app` from matching inside `@apple` or `foo@app.com`).
    public static func containsWholeToken(_ text: String, token: String) -> Bool {
        guard !token.isEmpty else { return false }
        var searchStart = text.startIndex
        while searchStart < text.endIndex,
              let range = text.range(of: token, range: searchStart..<text.endIndex) {
            let leadingOk: Bool
            if range.lowerBound == text.startIndex {
                leadingOk = true
            } else {
                let prev = text[text.index(before: range.lowerBound)]
                leadingOk = prev.isWhitespace || "([{\"'`,:;!?".contains(prev)
            }
            let trailingOk: Bool
            if range.upperBound == text.endIndex {
                trailingOk = true
            } else {
                let next = text[range.upperBound]
                trailingOk = next.isWhitespace || ")]}\"'`,.;:!?".contains(next)
            }
            if leadingOk && trailingOk {
                return true
            }
            searchStart = range.upperBound
        }
        return false
    }

    private static func isWhitespaceUtf16(_ code: unichar) -> Bool {
        code == 0x20 || code == 0x09 || code == 0x0A || code == 0x0D
    }
}

// MARK: - Sidebar, Navigation, & Board Logic

public enum OrbitSidebarLogic {
    /// Generates collision-free row IDs across Needs Attention, Pinned, Workspace Groups, Projects, and Standalone sections.
    public static func scopedRowId(
        section: String,
        hostId: String,
        agentId: String,
        containerId: String? = nil
    ) -> String {
        if let containerId, !containerId.isEmpty {
            return "\(section)::\(hostId)::\(containerId)::\(agentId)"
        }
        return "\(section)::\(hostId)::\(agentId)"
    }

    public static func matchesSearch(
        query: String,
        fields: [String?]
    ) -> Bool {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return true }
        return fields.compactMap { $0?.lowercased() }.contains { $0.contains(q) }
    }

    public static func shouldShowLegacyInbox(itemCount: Int) -> Bool {
        itemCount > 0
    }

    public static func shouldShowWorkspacePeerStrip(
        isExplicitCluster: Bool,
        peerCount: Int,
        groupTitle: String?
    ) -> Bool {
        if isExplicitCluster || peerCount > 1 { return true }
        let title = (groupTitle ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if title == "待归类" || title.lowercased() == "unassigned" || title.lowercased() == "ungrouped" {
            return peerCount > 1
        }
        return !title.isEmpty && peerCount > 0
    }
}

public enum OrbitBoardLogic {
    /// Converts any legacy localized Shared Blackboard headings into standard English headings matching Desktop.
    public static func englishSectionTitle(_ rawTitle: String) -> String {
        let trimmed = rawTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        switch trimmed {
        case "目标", "任务目标":
            return "Goal"
        case "决策与契约", "约定与契约":
            return "Decisions & Contracts"
        case "任务列表", "任务分工", "待办":
            return "Tasks"
        case "发现与问题", "审查发现":
            return "Findings"
        case "笔记", "备注":
            return "Notes"
        default:
            return trimmed
        }
    }
}

// MARK: - Time, Duration & Hex Color Logic

// MARK: - Store Shared Value Types

public enum OrbitGitReviewScope: String, CaseIterable, Identifiable, Equatable, Sendable {
    case workingTree = "working_tree"
    case staged = "staged"
    case all = "all"

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .workingTree: return "Unstaged"
        case .staged: return "Staged"
        case .all: return "All Changes"
        }
    }
}

public enum OrbitComposerUploadState: Equatable, Hashable, Sendable {
    case idle
    case uploading
    case uploaded
    case failed(String)
}

public struct OrbitPendingArchiveUndo: Equatable, Identifiable, Sendable {
    public let id: String
    public let hostId: String
    public let agentId: String
    public let title: String
    public let archivedAt: Date

    public init(hostId: String, agentId: String, title: String, archivedAt: Date = Date()) {
        self.id = "\(hostId)::\(agentId)::\(archivedAt.timeIntervalSince1970)"
        self.hostId = hostId
        self.agentId = agentId
        self.title = title
        self.archivedAt = archivedAt
    }
}

public struct OrbitSubagentInspectorTarget: Equatable, Identifiable, Sendable {
    public let id: String
    public let hostId: String
    public let parentAgentId: String
    public let childAgentId: String?
    public let providerSubagentId: String?
    public let title: String

    public init(
        hostId: String,
        parentAgentId: String,
        childAgentId: String? = nil,
        providerSubagentId: String? = nil,
        title: String
    ) {
        let subKey = childAgentId ?? providerSubagentId ?? UUID().uuidString
        self.id = "\(hostId)::\(parentAgentId)::\(subKey)"
        self.hostId = hostId
        self.parentAgentId = parentAgentId
        self.childAgentId = childAgentId
        self.providerSubagentId = providerSubagentId
        self.title = title
    }
}

public struct OrbitCloudSyncUiPreferences: Codable, Equatable, Sendable {
    public var sidebarProjectSort: String?
    public var sidebarThreadFilter: String?
    public var drawerFilter: String?
    public var coalesceExploration: Bool?
    public var defaultPermissionMode: String?
    public var appearanceMode: String?

    public init(
        sidebarProjectSort: String? = nil,
        sidebarThreadFilter: String? = nil,
        drawerFilter: String? = nil,
        coalesceExploration: Bool? = nil,
        defaultPermissionMode: String? = nil,
        appearanceMode: String? = nil
    ) {
        self.sidebarProjectSort = sidebarProjectSort
        self.sidebarThreadFilter = sidebarThreadFilter
        self.drawerFilter = drawerFilter
        self.coalesceExploration = coalesceExploration
        self.defaultPermissionMode = defaultPermissionMode
        self.appearanceMode = appearanceMode
    }
}

public enum OrbitTimeLogic {
    public static func formatDurationCompact(_ milliseconds: UInt64) -> String {
        if milliseconds == 0 {
            return "0s"
        }
        if milliseconds < 1_000 {
            return "\(milliseconds)ms"
        }
        let totalSeconds = milliseconds / 1_000
        if totalSeconds < 60 {
            return "\(totalSeconds)s"
        }
        let hours = totalSeconds / 3_600
        let minutes = (totalSeconds % 3_600) / 60
        let seconds = totalSeconds % 60
        if hours > 0 {
            return minutes > 0 ? "\(hours)h \(minutes)m" : "\(hours)h"
        }
        return seconds > 0 ? "\(minutes)m \(seconds)s" : "\(minutes)m"
    }

    public static func parseHexColor(_ hex: String) -> (red: Double, green: Double, blue: Double, alpha: Double)? {
        var cleaned = hex.trimmingCharacters(in: .whitespacesAndNewlines)
        if cleaned.hasPrefix("#") {
            cleaned.removeFirst()
        }
        guard let intVal = UInt64(cleaned, radix: 16) else { return nil }
        switch cleaned.count {
        case 3:
            let r = Double((intVal >> 8) & 0xF) / 15.0
            let g = Double((intVal >> 4) & 0xF) / 15.0
            let b = Double(intVal & 0xF) / 15.0
            return (r, g, b, 1.0)
        case 6:
            let r = Double((intVal >> 16) & 0xFF) / 255.0
            let g = Double((intVal >> 8) & 0xFF) / 255.0
            let b = Double(intVal & 0xFF) / 255.0
            return (r, g, b, 1.0)
        case 8:
            let a = Double((intVal >> 24) & 0xFF) / 255.0
            let r = Double((intVal >> 16) & 0xFF) / 255.0
            let g = Double((intVal >> 8) & 0xFF) / 255.0
            let b = Double(intVal & 0xFF) / 255.0
            return (r, g, b, a)
        default:
            return nil
        }
    }
}

// MARK: - Local Database Backup Exclusion & Data Protection

public enum OrbitStorageError: LocalizedError {
    case applicationSupportUnavailable

    public var errorDescription: String? {
        switch self {
        case .applicationSupportUnavailable:
            return "Application Support directory is unavailable."
        }
    }
}

public enum OrbitStorageProtection {
    /// Creates a dedicated directory under `baseDirectory` (e.g. `Application Support/Orbit`),
    /// excludes it from iCloud/iTunes backup, and sets hardware-backed file protection on iOS.
    /// SQLite database and `-wal`/`-shm` sidecars created inside inherit these protections automatically.
    public static func prepareProtectedDatabaseURL(
        in baseDirectory: URL,
        subdirectory: String = "Orbit",
        fileName: String = "orbit_ios_v3.sqlite"
    ) throws -> URL {
        let fm = FileManager.default
        var storageDir = baseDirectory.appendingPathComponent(subdirectory, isDirectory: true)
        try fm.createDirectory(at: storageDir, withIntermediateDirectories: true)
        #if os(iOS)
        try fm.setAttributes(
            [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication],
            ofItemAtPath: storageDir.path
        )
        #endif
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try storageDir.setResourceValues(values)
        return storageDir.appendingPathComponent(fileName, isDirectory: false)
    }
}

// MARK: - Plan Approval Extraction & Safety Logic (Desktop Parity)

public struct OrbitParsedPlanApproval: Equatable, Sendable {
    public let requestId: String
    public let title: String
    public let planMarkdown: String
    public let planFilePath: String?
    public let allowedPrompts: [String]

    public init(
        requestId: String,
        title: String,
        planMarkdown: String,
        planFilePath: String?,
        allowedPrompts: [String]
    ) {
        self.requestId = requestId
        self.title = title
        self.planMarkdown = planMarkdown
        self.planFilePath = planFilePath
        self.allowedPrompts = allowedPrompts
    }
}

public enum OrbitPlanApprovalLogic {
    private static let planApprovalToolLeaves: Set<String> = [
        "exitplanmode",
        "approveplan",
        "planapproval",
        "submitplan",
        "proposeplan",
        "reviewplan",
        "requestplanapproval"
    ]

    public static func normalizeToolLeaf(_ rawToolName: String) -> String {
        let leaf = rawToolName
            .split(whereSeparator: { $0 == ":" || $0 == "." || $0 == "/" })
            .last
            .map(String.init) ?? rawToolName
        return leaf
            .lowercased()
            .filter { $0.isLetter || $0.isNumber }
    }

    /// Mirrors Rust `orbit_protocol::is_plan_approval_tool_name`.
    public static func isPlanApprovalToolName(_ toolName: String?) -> Bool {
        guard let toolName, !toolName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return false
        }
        return planApprovalToolLeaves.contains(normalizeToolLeaf(toolName))
    }

    public static func isPlanApprovalPermission(
        title: String,
        commandOrPath: String,
        toolName: String?,
        argumentsJson: String?
    ) -> Bool {
        if isPlanApprovalToolName(toolName) {
            return true
        }
        let combinedTitle = "\(title) \(commandOrPath)".lowercased()
        if combinedTitle.contains("exitplanmode")
            || combinedTitle.contains("plan approval")
            || combinedTitle.contains("approve plan")
            || combinedTitle.contains("review plan") {
            return true
        }
        if let json = argumentsJson, extractPlanMarkdown(fromArgumentsJson: json) != nil {
            if combinedTitle.contains("plan") {
                return true
            }
        }
        return false
    }

    public static func extractPlanMarkdown(fromArgumentsJson rawJson: String?) -> String? {
        guard let rawJson,
              let data = rawJson.data(using: .utf8),
              let dict = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }
        let keys = ["plan", "plan_markdown", "planMarkdown", "markdown", "proposal", "content"]
        for key in keys {
            if let str = dict[key] as? String {
                let trimmed = str.trimmingCharacters(in: .whitespacesAndNewlines)
                if !trimmed.isEmpty {
                    return extractCodexProposedPlan(from: trimmed) ?? trimmed
                }
            }
        }
        return nil
    }

    public static func extractCodexProposedPlan(from text: String) -> String? {
        let lower = text.lowercased()
        guard let openRange = lower.range(of: "<proposed_plan"),
              let closeTagStart = lower.range(of: "</proposed_plan>", range: openRange.upperBound..<lower.endIndex),
              let openEnd = text[openRange.lowerBound..<text.endIndex].firstIndex(of: ">") else {
            return nil
        }
        let bodyStart = text.index(after: openEnd)
        guard bodyStart <= closeTagStart.lowerBound else { return nil }
        let body = String(text[bodyStart..<closeTagStart.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines)
        return body.isEmpty ? nil : body
    }

    public static func extractPlanTitle(fromMarkdown markdown: String) -> String? {
        for line in markdown.split(separator: "\n", omittingEmptySubsequences: false).prefix(24) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty else { continue }
            if trimmed.hasPrefix("#") {
                let heading = trimmed
                    .drop(while: { $0 == "#" })
                    .trimmingCharacters(in: .whitespaces)
                    .replacingOccurrences(of: #"\s*#+\s*$"#, with: "", options: .regularExpression)
                    .trimmingCharacters(in: .whitespaces)
                if !heading.isEmpty {
                    return heading.count > 96 ? String(heading.prefix(95)) + "…" : heading
                }
            }
        }
        return nil
    }

    public static func parsePlanApproval(
        requestId: String,
        title: String,
        commandOrPath: String,
        toolName: String?,
        argumentsJson: String?
    ) -> OrbitParsedPlanApproval? {
        guard isPlanApprovalPermission(
            title: title,
            commandOrPath: commandOrPath,
            toolName: toolName,
            argumentsJson: argumentsJson
        ) else {
            return nil
        }
        var planFilePath: String? = nil
        var allowedPrompts: [String] = []
        if let rawJson = argumentsJson,
           let data = rawJson.data(using: .utf8),
           let dict = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            for key in ["plan_file_path", "planFilePath", "file_path", "path"] {
                if let path = dict[key] as? String, !path.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    planFilePath = path.trimmingCharacters(in: .whitespacesAndNewlines)
                    break
                }
            }
            if let arr = (dict["allowedPrompts"] ?? dict["allowed_prompts"]) as? [Any] {
                for entry in arr {
                    if let s = entry as? String {
                        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
                        if !t.isEmpty { allowedPrompts.append(t) }
                    } else if let obj = entry as? [String: Any] {
                        let tool = (obj["tool"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                        let prompt = (obj["prompt"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                        if !tool.isEmpty && !prompt.isEmpty {
                            allowedPrompts.append("\(tool): \(prompt)")
                        } else if !prompt.isEmpty {
                            allowedPrompts.append(prompt)
                        } else if !tool.isEmpty {
                            allowedPrompts.append(tool)
                        }
                    }
                }
            }
        }

        let extractedMarkdown = extractPlanMarkdown(fromArgumentsJson: argumentsJson)
            ?? extractCodexProposedPlan(from: commandOrPath)
            ?? (commandOrPath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? title : commandOrPath)
        let resolvedTitle = extractPlanTitle(fromMarkdown: extractedMarkdown)
            ?? (title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Implementation Plan" : title)
        return OrbitParsedPlanApproval(
            requestId: requestId,
            title: resolvedTitle,
            planMarkdown: extractedMarkdown,
            planFilePath: planFilePath,
            allowedPrompts: allowedPrompts
        )
    }
}

// MARK: - Batch Approval Safety Filter (Desktop `selectSafeBatchPermissions` Parity)

public enum OrbitPermissionRiskLevel: String, Equatable, Sendable {
    case low
    case medium
    case high
    case critical
}

public struct OrbitBatchApprovalCandidate: Equatable, Sendable {
    public let hostId: String
    public let agentId: String
    public let requestId: String
    public let title: String
    public let commandOrPath: String
    public let toolName: String?
    public let argumentsJson: String?
    public let allowedDecisions: [String]
    public let isQuestion: Bool

    public init(
        hostId: String,
        agentId: String,
        requestId: String,
        title: String,
        commandOrPath: String,
        toolName: String? = nil,
        argumentsJson: String? = nil,
        allowedDecisions: [String],
        isQuestion: Bool = false
    ) {
        self.hostId = hostId
        self.agentId = agentId
        self.requestId = requestId
        self.title = title
        self.commandOrPath = commandOrPath
        self.toolName = toolName
        self.argumentsJson = argumentsJson
        self.allowedDecisions = allowedDecisions
        self.isQuestion = isQuestion
    }
}

public enum OrbitBatchApprovalLogic {
    private static let highRiskSubstrings: [String] = [
        "rm -rf", "rm -fr", "sudo ", "su ", "mkfs", "dd if=", "chmod 777", "chmod -r 777",
        "git push --force", "git push -f", "git reset --hard", "git clean -fd",
        "drop table", "drop database", "truncate table", "delete from",
        "curl | sh", "curl | bash", "wget | sh", "kubectl delete", "terraform destroy",
        "docker system prune", "shutdown", "reboot", "launchctl unload", "> /dev/"
    ]

    public static func permitsSingleAllowDecision(_ allowedDecisions: [String]) -> Bool {
        guard !allowedDecisions.isEmpty else { return false }
        return allowedDecisions.contains { raw in
            let norm = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            return norm == "allow" || norm == "allow_once" || norm == "accept"
        }
    }

    public static func assessRisk(title: String, commandOrPath: String, toolName: String?) -> OrbitPermissionRiskLevel {
        if OrbitPlanApprovalLogic.isPlanApprovalPermission(
            title: title,
            commandOrPath: commandOrPath,
            toolName: toolName,
            argumentsJson: nil
        ) {
            return .high
        }
        let combined = "\(title) \(commandOrPath) \(toolName ?? "")".lowercased()
        for danger in highRiskSubstrings {
            if combined.contains(danger) {
                return .critical
            }
        }
        if combined.contains("git push")
            || combined.contains("npm publish")
            || combined.contains("cargo publish")
            || combined.contains("ssh ")
            || combined.contains("scp ")
            || combined.contains("rm ") {
            return .high
        }
        if combined.contains("write") || combined.contains("edit") || combined.contains("bash") || combined.contains("shell") {
            return .medium
        }
        return .low
    }

    /// Returns strictly non-plan, non-question, non-High/Critical permissions that explicitly allow single-shot approval.
    public static func selectSafeBatchPermissions(_ queue: [OrbitBatchApprovalCandidate]) -> [OrbitBatchApprovalCandidate] {
        queue.filter { item in
            guard !item.isQuestion else { return false }
            guard !OrbitPlanApprovalLogic.isPlanApprovalPermission(
                title: item.title,
                commandOrPath: item.commandOrPath,
                toolName: item.toolName,
                argumentsJson: item.argumentsJson
            ) else {
                return false
            }
            let risk = assessRisk(title: item.title, commandOrPath: item.commandOrPath, toolName: item.toolName)
            guard risk == .low || risk == .medium else { return false }
            return permitsSingleAllowDecision(item.allowedDecisions)
        }
    }
}

// MARK: - Raw VT100 / PTY Key Encoder Logic

public enum OrbitPtySpecialKey: Equatable, Sendable {
    case escape
    case tab
    case backTab
    case enter
    case backspace
    case deleteForward
    case arrowUp
    case arrowDown
    case arrowLeft
    case arrowRight
    case home
    case end
    case pageUp
    case pageDown
}

public enum OrbitPtyInputLogic {
    /// Encodes a special terminal key into standard VT100 / xterm UTF-8 byte sequence.
    public static func encodeSpecialKey(_ key: OrbitPtySpecialKey) -> Data {
        switch key {
        case .escape:        return Data([0x1B])
        case .tab:           return Data([0x09])
        case .backTab:       return Data("\u{001B}[Z".utf8)
        case .enter:         return Data([0x0D])
        case .backspace:     return Data([0x7F])
        case .deleteForward: return Data("\u{001B}[3~".utf8)
        case .arrowUp:       return Data("\u{001B}[A".utf8)
        case .arrowDown:     return Data("\u{001B}[B".utf8)
        case .arrowRight:    return Data("\u{001B}[C".utf8)
        case .arrowLeft:     return Data("\u{001B}[D".utf8)
        case .home:          return Data("\u{001B}[H".utf8)
        case .end:           return Data("\u{001B}[F".utf8)
        case .pageUp:        return Data("\u{001B}[5~".utf8)
        case .pageDown:      return Data("\u{001B}[6~".utf8)
        }
    }

    /// Encodes printable text or a modified keystroke (Ctrl / Alt) directly into raw PTY bytes.
    public static func encodeText(_ text: String, ctrl: Bool = false, alt: Bool = false) -> Data {
        guard !text.isEmpty else { return Data() }
        if ctrl, let first = text.first {
            if let ctrlByte = controlByte(for: first) {
                var out = Data()
                if alt { out.append(0x1B) }
                out.append(ctrlByte)
                return out
            }
        }
        // Normalize soft-keyboard newline to carriage return for raw PTY lines.
        let normalized = text == "\n" ? "\r" : text
        if alt {
            var out = Data([0x1B])
            out.append(contentsOf: normalized.utf8)
            return out
        }
        return Data(normalized.utf8)
    }

    /// Converts ASCII characters (`a`-`z`, `A`-`Z`, `@`, `[`, `\`, `]`, `^`, `_`, `?`) into their Ctrl byte.
    public static func controlByte(for character: Character) -> UInt8? {
        guard let ascii = character.asciiValue else { return nil }
        switch ascii {
        case 0x40...0x5F: // '@' .. '_'
            return ascii & 0x1F
        case 0x61...0x7A: // 'a' .. 'z'
            return (ascii - 0x20) & 0x1F
        case 0x3F: // '?'
            return 0x7F
        case 0x20: // Ctrl+Space -> NUL
            return 0x00
        default:
            return nil
        }
    }
}

// MARK: - APNs Notification & Deep Link Routing Logic

public enum OrbitDeepLinkAction: Equatable, Sendable {
    case openSession(hostId: String, agentId: String?)
    case pairHost(uri: String)
    case syncLogin(uri: String)
}

public enum OrbitPushDeepLinkLogic {
    /// Parses `orbit://open?host=<id>&agent=<id>`, `orbit://pair/...`, or `orbit://sync-login?...`.
    public static func parseDeepLink(_ rawUrl: String) -> OrbitDeepLinkAction? {
        let trimmed = rawUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        let lower = trimmed.lowercased()
        if lower.hasPrefix("orbit://pair/") {
            return .pairHost(uri: trimmed)
        }
        if lower.hasPrefix("orbit://sync-login") {
            return .syncLogin(uri: trimmed)
        }
        guard let comps = URLComponents(string: trimmed),
              comps.scheme?.lowercased() == "orbit",
              comps.host?.lowercased() == "open" else {
            return nil
        }
        let items = comps.queryItems ?? []
        let hostId = items.first(where: { $0.name == "host" || $0.name == "host_id" })?.value?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let agentId = items.first(where: { $0.name == "agent" || $0.name == "agent_id" })?.value?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard let hostId, !hostId.isEmpty else { return nil }
        return .openSession(hostId: hostId, agentId: (agentId?.isEmpty == false) ? agentId : nil)
    }

    /// Parses an APNs `userInfo` dictionary (`deep_link`, `host_id`, `agent_id`, `event_kind`).
    public static func parseNotificationUserInfo(_ userInfo: [AnyHashable: Any]) -> OrbitDeepLinkAction? {
        if let deepLink = userInfo["deep_link"] as? String,
           let parsed = parseDeepLink(deepLink) {
            return parsed
        }
        if let hostId = (userInfo["host_id"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines),
           !hostId.isEmpty {
            let agentId = (userInfo["agent_id"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
            return .openSession(hostId: hostId, agentId: (agentId?.isEmpty == false) ? agentId : nil)
        }
        return nil
    }
}

// MARK: - Codex-Style Voice Dictation Intent Logic

public enum OrbitVoiceTranscriptAction: Equatable, Sendable {
    case appendText(String)
    case clearDraft
    case focusOverview
}

public enum OrbitVoiceCommandLogic {
    /// Merges newly dictated text into an existing draft without duplicating leading whitespace or trailing periods.
    public static func mergeDictatedText(existingDraft: String, spokenSegment: String) -> String {
        let spoken = spokenSegment.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !spoken.isEmpty else { return existingDraft }
        let base = existingDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        if base.isEmpty {
            return spoken
        }
        let separator = existingDraft.hasSuffix("\n") || existingDraft.hasSuffix(" ") ? "" : " "
        return existingDraft + separator + spoken
    }

    /// Classifies safe non-destructive voice transcript intents.
    /// Never auto-approves high-risk actions or auto-sends without explicit user action.
    public static func classifyTranscript(_ raw: String) -> OrbitVoiceTranscriptAction {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        let lower = trimmed.lowercased()
        if lower == "clear draft" || lower == "清空草稿" || lower == "下書きをクリア" {
            return .clearDraft
        }
        return .appendText(trimmed)
    }
}

