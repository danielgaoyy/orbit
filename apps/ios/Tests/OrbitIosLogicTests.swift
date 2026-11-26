import Foundation
import SQLite3

#if !ORBIT_CLI_TEST_RUNNER
import XCTest
#endif

// MARK: - Dual CLI + XCTest Harness

#if ORBIT_CLI_TEST_RUNNER
struct LogicTestFailure: Error, CustomStringConvertible {
    let message: String
    var description: String { message }
}

class OrbitLogicTestCase {
    required init() {}
}

func checkEqual<T: Equatable>(
    _ lhs: T,
    _ rhs: T,
    _ label: String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    if lhs != rhs {
        throw LogicTestFailure(message: "\(file):\(line) \(label) expected <\(rhs)>, got <\(lhs)>")
    }
}

func checkTrue(
    _ condition: Bool,
    _ label: String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    if !condition {
        throw LogicTestFailure(message: "\(file):\(line) \(label) expected true")
    }
}

func checkFalse(
    _ condition: Bool,
    _ label: String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    if condition {
        throw LogicTestFailure(message: "\(file):\(line) \(label) expected false")
    }
}

func checkNil<T>(
    _ value: T?,
    _ label: String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    if value != nil {
        throw LogicTestFailure(message: "\(file):\(line) \(label) expected nil, got <\(String(describing: value))>")
    }
}
#else
class OrbitLogicTestCase: XCTestCase {}

func checkEqual<T: Equatable>(
    _ lhs: T,
    _ rhs: T,
    _ label: String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    XCTAssertEqual(lhs, rhs, label, file: file, line: line)
}

func checkTrue(
    _ condition: Bool,
    _ label: String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    XCTAssertTrue(condition, label, file: file, line: line)
}

func checkFalse(
    _ condition: Bool,
    _ label: String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    XCTAssertFalse(condition, label, file: file, line: line)
}

func checkNil<T>(
    _ value: T?,
    _ label: String = "",
    file: StaticString = #file,
    line: UInt = #line
) throws {
    XCTAssertNil(value, label, file: file, line: line)
}
#endif

// MARK: - Parity & Bug Regression Tests (34 Cases)

final class OrbitIosLogicTests: OrbitLogicTestCase {
    // MARK: 1-7 Path Logic

    func testPathNormalizeAndCompactHomePrefixes() throws {
        try checkEqual(OrbitPathLogic.normalizePath("  /Users/murphy//go/src/orbit/  "), "/Users/murphy/go/src/orbit")
        try checkEqual(OrbitPathLogic.normalizePath("/"), "/")
        try checkEqual(OrbitPathLogic.normalizePath("C:\\repo\\src\\main.rs"), "C:/repo/src/main.rs")
        try checkEqual(OrbitPathLogic.compactPath("/Users/murphy/go/src/orbit"), "~/go/src/orbit")
        try checkEqual(OrbitPathLogic.compactPath("/home/ubuntu"), "~")
        try checkEqual(OrbitPathLogic.compactPath("/var/log/orbit.log"), "/var/log/orbit.log")
    }

    func testPathShortPathRetainsTrailingOrientation() throws {
        try checkEqual(OrbitPathLogic.shortPath("/Users/murphy/go/src/orbit/apps/ios/Sources"), ".../ios/Sources")
        try checkEqual(OrbitPathLogic.shortPath("/Users/murphy/project"), "~/project")
        try checkEqual(OrbitPathLogic.shortPath("/"), "/")
        try checkEqual(OrbitPathLogic.shortPath("~"), "~")
    }

    func testPathStripWorkspacePrefixInsideAndOutsideCwd() throws {
        try checkEqual(
            OrbitPathLogic.stripWorkspacePrefix("/Users/murphy/go/src/orbit/apps/ios/Sources/Theme/OrbitTheme.swift", cwd: "/Users/murphy/go/src/orbit"),
            "apps/ios/Sources/Theme/OrbitTheme.swift"
        )
        try checkEqual(
            OrbitPathLogic.stripWorkspacePrefix("~/go/src/orbit/README.md", cwd: "/Users/murphy/go/src/orbit"),
            "README.md"
        )
        try checkEqual(
            OrbitPathLogic.stripWorkspacePrefix("/Users/murphy/go/src/orbit", cwd: "/Users/murphy/go/src/orbit"),
            "."
        )
        try checkEqual(
            OrbitPathLogic.stripWorkspacePrefix("./crates/orbit-cli/src/main.rs", cwd: "/Users/murphy/go/src/orbit"),
            "crates/orbit-cli/src/main.rs"
        )
        try checkEqual(
            OrbitPathLogic.stripWorkspacePrefix("/opt/shared/tools/check.sh", cwd: "/Users/murphy/go/src/orbit"),
            ".../tools/check.sh"
        )
    }

    func testPathShortFileLabelAndResolveWorkspaceFilePath() throws {
        try checkEqual(
            OrbitPathLogic.shortFileLabel("/Users/murphy/go/src/orbit/apps/ios/Sources/App.swift", rootCwd: "/Users/murphy/go/src/orbit"),
            "apps/ios/Sources/App.swift"
        )
        try checkEqual(
            OrbitPathLogic.resolveWorkspaceFilePath("apps/ios/Sources/Stage/StreamStageView.swift#L42", rootCwd: "/Users/murphy/go/src/orbit"),
            "apps/ios/Sources/Stage/StreamStageView.swift"
        )
        try checkEqual(
            OrbitPathLogic.resolveWorkspaceFilePath("~/go/src/orbit/docs/GUIDE.md#L5-L12", rootCwd: "/Users/murphy/go/src/orbit"),
            "docs/GUIDE.md"
        )
        try checkEqual(
            OrbitPathLogic.resolveWorkspaceFilePath("./Sources/Main.swift", rootCwd: "/repo"),
            "Sources/Main.swift"
        )
    }

    func testPathBasenameAndExtensionsForImagesAndMarkdown() throws {
        try checkEqual(OrbitPathLogic.basename("/Users/murphy/go/src/orbit/README.md"), "README.md")
        try checkEqual(OrbitPathLogic.basename("~"), "~")
        try checkEqual(OrbitPathLogic.pathExtension("assets/logo.PNG#L1"), "png")
        try checkTrue(OrbitPathLogic.isPreviewableImagePath("screenshots/app.PNG"))
        try checkTrue(OrbitPathLogic.isPreviewableImagePath("icons/orbit.webp"))
        try checkFalse(OrbitPathLogic.isPreviewableImagePath("Sources/App.swift"))
        try checkTrue(OrbitPathLogic.isMarkdownPath("docs/README.mdx"))
        try checkTrue(OrbitPathLogic.isMarkdownPath("NOTES.markdown"))
        try checkFalse(OrbitPathLogic.isMarkdownPath("src/lib.rs"))
    }

    func testPathParseFileTargetFileUrlAndLineRange() throws {
        let parsed = OrbitPathLogic.parseFileTargetAndLine("file:///Users/murphy/go/src/orbit/Cargo.toml#L12-L20", cwd: "/Users/murphy/go/src/orbit")
        try checkEqual(parsed.path, "Cargo.toml")
        try checkEqual(parsed.line, 12)
        try checkEqual(parsed.endLine, 20)

        let decoded = OrbitPathLogic.parseFileTargetAndLine("file:///Users/murphy/My%20Docs/notes.md#L7", cwd: nil)
        try checkEqual(decoded.path, "/Users/murphy/My Docs/notes.md")
        try checkEqual(decoded.line, 7)
        try checkNil(decoded.endLine)
    }

    func testPathParseFileTargetColonLineAndColumn() throws {
        let lineOnly = OrbitPathLogic.parseFileTargetAndLine("Sources/Store/OrbitIosStore.swift:42", cwd: nil)
        try checkEqual(lineOnly.path, "Sources/Store/OrbitIosStore.swift")
        try checkEqual(lineOnly.line, 42)
        try checkNil(lineOnly.endLine)

        let lineCol = OrbitPathLogic.parseFileTargetAndLine("Sources/Store/OrbitIosStore.swift:748:12", cwd: nil)
        try checkEqual(lineCol.path, "Sources/Store/OrbitIosStore.swift")
        try checkEqual(lineCol.line, 748)
    }

    // MARK: 8-9 Markdown Links & Tables

    func testMarkdownLinkClassificationWebMailtoAnchorAndFile() throws {
        let web = OrbitMarkdownLinkLogic.parseLink("https://example.com/docs", workspaceCwd: "/repo")
        try checkEqual(web, .externalWeb(URL(string: "https://example.com/docs")!))

        let mail = OrbitMarkdownLinkLogic.parseLink("mailto:dev@orbit.dev", workspaceCwd: "/repo")
        try checkEqual(mail, .mailto(URL(string: "mailto:dev@orbit.dev")!))

        let anchor = OrbitMarkdownLinkLogic.parseLink("#architecture", workspaceCwd: "/repo")
        try checkEqual(anchor, .anchor("architecture"))

        let fileLink = OrbitMarkdownLinkLogic.parseLink("Sources/Stage/RichMarkdownView.swift#L88", workspaceCwd: "/repo")
        try checkEqual(fileLink, .filePath(.init(path: "Sources/Stage/RichMarkdownView.swift", line: 88, endLine: nil)))

        let absLink = OrbitMarkdownLinkLogic.parseLink("/repo/README.md#L3", workspaceCwd: "/repo")
        try checkEqual(absLink, .filePath(.init(path: "README.md", line: 3, endLine: nil)))
    }

    func testMarkdownASCIIBoxTableFalsePositiveGuard() throws {
        let asciiArt = [
            "┌─────────┬──────────┐",
            "│ Host    │ Status   │",
            "├─────────┼──────────┤",
            "│ devbox  │ online   │",
            "└─────────┴──────────┘"
        ]
        try checkTrue(OrbitMarkdownLinkLogic.isASCIIBoxTableFalsePositive(asciiArt))

        let realMdTable = [
            "| Host | Status |",
            "| --- | --- |",
            "| devbox | online |"
        ]
        try checkFalse(OrbitMarkdownLinkLogic.isASCIIBoxTableFalsePositive(realMdTable))
    }

    // MARK: 10-17 Agent Status, Models & Multi-Host State

    func testAgentGenericHarnessHandleDetection() throws {
        try checkTrue(OrbitAgentLogic.isGenericHarnessHandle(nil))
        try checkTrue(OrbitAgentLogic.isGenericHarnessHandle("  "))
        try checkTrue(OrbitAgentLogic.isGenericHarnessHandle("codex"))
        try checkTrue(OrbitAgentLogic.isGenericHarnessHandle("@agy"))
        try checkTrue(OrbitAgentLogic.isGenericHarnessHandle("claude-code-2"))
        try checkTrue(OrbitAgentLogic.isGenericHarnessHandle("peer_3"))
        try checkFalse(OrbitAgentLogic.isGenericHarnessHandle("ios-verifier"))
        try checkFalse(OrbitAgentLogic.isGenericHarnessHandle("reviewer"))
    }

    func testAgentForegroundRunningVsBackgroundWaiting() throws {
        try checkFalse(
            OrbitAgentLogic.isAgentForegroundRunning(statusRaw: "waiting_background", hasActiveTurn: false)
        )
        try checkFalse(
            OrbitAgentLogic.isAgentForegroundRunning(statusRaw: "Waiting for 2 background tasks", hasActiveTurn: false)
        )
        try checkFalse(
            OrbitAgentLogic.isAgentForegroundRunning(statusRaw: "Running (2 background tasks)", hasActiveTurn: true)
        )
        try checkTrue(
            OrbitAgentLogic.isAgentBackgroundWaiting(statusRaw: "waiting_background", hasActiveTurn: false, hasBackgroundTasks: true)
        )
        try checkTrue(
            OrbitAgentLogic.isAgentBackgroundWaiting(statusRaw: "Waiting for 2 background tasks", hasActiveTurn: false, hasBackgroundTasks: true)
        )
        try checkTrue(
            OrbitAgentLogic.isAgentForegroundRunning(statusRaw: "working", hasActiveTurn: false)
        )
        try checkFalse(
            OrbitAgentLogic.isAgentBackgroundWaiting(statusRaw: "working", hasActiveTurn: true, hasBackgroundTasks: true)
        )
        try checkTrue(
            OrbitAgentLogic.isAgentForegroundRunning(statusRaw: "idle", hasActiveTurn: true)
        )
    }

    func testWakeupFilteringAndSettledStepGroupDisclosure() throws {
        try checkTrue(OrbitAgentLogic.isHiddenWakeupLabel("Resumed from background task"))
        try checkTrue(OrbitAgentLogic.isHiddenWakeupLabel("Waiting for 2 background tasks"))
        try checkTrue(OrbitAgentLogic.isHiddenWakeupLabel("   "))
        try checkFalse(OrbitAgentLogic.isHiddenWakeupLabel("Waking from cron schedule"))

        // Pre-reply step group containing a running background `Schedule` must settle once prose follows.
        try checkFalse(
            OrbitAgentLogic.isStepGroupLiveActive(
                isTurnActive: true,
                hasFollowingContent: true,
                hasRunningStep: true,
                isLastVisibleSegment: false,
                unrecoveredFailureCount: 0
            )
        )
        // Finished turn (e.g. background-waiting) must NEVER keep a trailing step group live.
        try checkFalse(
            OrbitAgentLogic.isStepGroupLiveActive(
                isTurnActive: false,
                hasFollowingContent: false,
                hasRunningStep: true,
                isLastVisibleSegment: true,
                unrecoveredFailureCount: 0
            )
        )
        // Genuinely active foreground turn with trailing step group stays live.
        try checkTrue(
            OrbitAgentLogic.isStepGroupLiveActive(
                isTurnActive: true,
                hasFollowingContent: false,
                hasRunningStep: false,
                isLastVisibleSegment: true,
                unrecoveredFailureCount: 0
            )
        )
    }

    func testAgentProviderSwitchGuard() throws {
        try checkFalse(
            OrbitAgentLogic.canSwitchProvider(selectedAgentId: "agent-1", activeAgentExists: false, supportsProviderSwitch: true)
        )
        try checkFalse(
            OrbitAgentLogic.canSwitchProvider(selectedAgentId: "agent-1", activeAgentExists: true, supportsProviderSwitch: false)
        )
        try checkTrue(
            OrbitAgentLogic.canSwitchProvider(selectedAgentId: "agent-1", activeAgentExists: true, supportsProviderSwitch: true)
        )
        try checkTrue(
            OrbitAgentLogic.canSwitchProvider(selectedAgentId: nil, activeAgentExists: false, supportsProviderSwitch: false)
        )
    }

    func testAgentModelDeduplicationAcrossAliasesAndDots() throws {
        try checkEqual(OrbitAgentLogic.normalizedProviderId("agy"), "antigravity")
        try checkEqual(OrbitAgentLogic.normalizedProviderId("claude-code"), "claude_code")
        let k1 = OrbitAgentLogic.modelDedupeKey(providerId: "agy", rawModelId: "antigravity/claude-sonnet-4.6")
        let k2 = OrbitAgentLogic.modelDedupeKey(providerId: "antigravity", rawModelId: "claude-sonnet-4-6")
        try checkEqual(k1, k2)
    }

    func testAgentStageScopeAndDismissedVersionKeys() throws {
        try checkEqual(OrbitAgentLogic.stageScopeKey(hostId: "h1", agentId: "a1"), "h1::a1")
        try checkNil(OrbitAgentLogic.stageScopeKey(hostId: "h1", agentId: nil))
        try checkNil(OrbitAgentLogic.stageScopeKey(hostId: "  ", agentId: "a1"))
        try checkEqual(OrbitAgentLogic.dismissedVersionKey(hostId: "h1", daemonVersion: "0.8.2"), "h1::0.8.2")
        try checkTrue(
            OrbitAgentLogic.dismissedVersionKey(hostId: "h1", daemonVersion: "0.8.2")
                != OrbitAgentLogic.dismissedVersionKey(hostId: "h1", daemonVersion: "0.8.3")
        )
    }

    func testAgentHostConfigClearableFields() throws {
        try checkEqual(OrbitAgentLogic.hostConfigFieldValue(edited: "", original: "https://relay.example"), "")
        try checkNil(OrbitAgentLogic.hostConfigFieldValue(edited: "   ", original: nil))
        try checkNil(OrbitAgentLogic.hostConfigFieldValue(edited: "", original: ""))
        try checkEqual(OrbitAgentLogic.hostConfigFieldValue(edited: " devbox ", original: "old"), "devbox")
    }

    func testAgentOverviewBadgeTonePriority() throws {
        try checkEqual(
            OrbitAgentLogic.overviewBadgeTone(hasPendingApproval: false, hasSubagentApproval: true, isError: false, hasUnread: true, hasBackgroundTasks: true, isRunning: true),
            .critical
        )
        try checkEqual(
            OrbitAgentLogic.overviewBadgeTone(hasPendingApproval: false, hasSubagentApproval: false, isError: true, hasUnread: false, hasBackgroundTasks: false, isRunning: false),
            .critical
        )
        try checkEqual(
            OrbitAgentLogic.overviewBadgeTone(hasPendingApproval: false, hasSubagentApproval: false, isError: false, hasUnread: true, hasBackgroundTasks: true, isRunning: false),
            .accent
        )
        try checkEqual(
            OrbitAgentLogic.overviewBadgeTone(hasPendingApproval: false, hasSubagentApproval: false, isError: false, hasUnread: false, hasBackgroundTasks: true, isRunning: false),
            .muted
        )
        try checkEqual(
            OrbitAgentLogic.overviewBadgeTone(hasPendingApproval: false, hasSubagentApproval: false, isError: false, hasUnread: false, hasBackgroundTasks: false, isRunning: false),
            .none
        )
    }

    func testAgentMultiHostProjectReorderPreservesOtherHosts() throws {
        let existing = ["h1::p1", "h2::pX", "h1::p2", "h1::p3", "h2::pY"]
        let updated = OrbitAgentLogic.reorderProjectKeys(
            existingOrder: existing,
            targetHostProjectKeys: ["h1::p1", "h1::p2", "h1::p3"],
            movedFromIndex: 0,
            movedToOffset: 3
        )
        try checkEqual(updated, ["h1::p2", "h2::pX", "h1::p3", "h1::p1", "h2::pY"])
    }

    // MARK: 18-20 Decisions & Permissions

    func testDecisionClassificationAndCancelDeclineSafety() throws {
        try checkEqual(OrbitDecisionLogic.classify("decline"), .decline)
        try checkEqual(OrbitDecisionLogic.classify("decline_allow_list"), .decline)
        try checkEqual(OrbitDecisionLogic.classify("deny"), .decline)
        try checkEqual(OrbitDecisionLogic.classify("reject"), .decline)
        try checkEqual(OrbitDecisionLogic.classify("cancel"), .cancel)
        try checkEqual(OrbitDecisionLogic.classify("allow_for_session"), .allowSession)
        try checkEqual(OrbitDecisionLogic.classify("always_allow"), .allowAlways)
        try checkEqual(OrbitDecisionLogic.classify("allow_once"), .allowOnce)
        try checkEqual(OrbitDecisionLogic.classify("run_sandboxed"), .custom)
        try checkTrue(OrbitDecisionLogic.isDestructiveOrDenial("decline"))
        try checkTrue(OrbitDecisionLogic.isDestructiveOrDenial("cancel"))
        try checkFalse(OrbitDecisionLogic.isDestructiveOrDenial("allow_always"))
    }

    func testDecisionLabelAndClampedNavigationIndex() throws {
        try checkEqual(OrbitDecisionLogic.decisionLabel("decline"), "Decline")
        try checkEqual(OrbitDecisionLogic.decisionLabel("deny_once"), "Deny")
        try checkEqual(OrbitDecisionLogic.decisionLabel("cancel"), "Cancel")
        try checkEqual(OrbitDecisionLogic.decisionLabel("allow_for_session"), "Allow for Session")
        try checkEqual(OrbitDecisionLogic.decisionLabel("allow_always"), "Always Allow")
        try checkEqual(OrbitDecisionLogic.decisionLabel("approve"), "Approve")
        try checkEqual(OrbitDecisionLogic.decisionLabel("custom_option"), "Custom Option")
        try checkEqual(OrbitDecisionLogic.clampedDecisionIndex(current: 1, count: 2, delta: 1), 1)
        try checkEqual(OrbitDecisionLogic.clampedDecisionIndex(current: 0, count: 2, delta: -1), 0)
        try checkEqual(OrbitDecisionLogic.clampedDecisionIndex(current: 0, count: 0, delta: 1), 0)
    }

    func testDecisionInspectablePayloadFormatsJsonAndSkipsEmpty() throws {
        let inspect = OrbitDecisionLogic.inspectablePayload(
            rawArgsJson: "{\"command\":\"cargo test\",\"timeout\":30}",
            command: "cargo test",
            diff: nil,
            path: nil
        )
        try checkTrue(inspect.hasInspectableContent)
        try checkEqual(inspect.command, "cargo test")
        try checkTrue(inspect.formattedArgsJson?.contains("\"command\"") == true)

        let emptyInspect = OrbitDecisionLogic.inspectablePayload(
            rawArgsJson: "{}",
            command: "  ",
            diff: "",
            path: nil
        )
        try checkFalse(emptyInspect.hasInspectableContent)
        try checkNil(emptyInspect.formattedArgsJson)
    }

    // MARK: 21-28 Composer, Mentions, Drafts & Queues

    func testComposerSlashCommandIcons() throws {
        try checkEqual(OrbitComposerLogic.iconName(forSlashCommandId: "supervised"), "shield")
        try checkEqual(OrbitComposerLogic.iconName(forSlashCommandId: "auto"), "checkmark.shield")
        try checkEqual(OrbitComposerLogic.iconName(forSlashCommandId: "full"), "flame")
        try checkEqual(OrbitComposerLogic.iconName(forSlashCommandId: "plan"), "list.bullet.clipboard")
        try checkEqual(OrbitComposerLogic.iconName(forSlashCommandId: "review"), "doc.text.magnifyingglass")
        try checkEqual(OrbitComposerLogic.iconName(forSlashCommandId: "board"), "rectangle.3.group")
        try checkEqual(OrbitComposerLogic.iconName(forSlashCommandId: "subagents"), "person.2")
        try checkEqual(OrbitComposerLogic.iconName(forSlashCommandId: "unknown_custom"), "command")
    }

    func testComposerPeerMentionDetectionAndAllTarget() throws {
        try checkTrue(OrbitComposerLogic.isPeerMentionCandidate(kind: "workspace_peer", key: "peer::worker"))
        try checkTrue(OrbitComposerLogic.isPeerMentionCandidate(kind: "peer", key: "worker"))
        try checkFalse(OrbitComposerLogic.isPeerMentionCandidate(kind: "file", key: "Sources/App.swift"))
        try checkTrue(OrbitComposerLogic.isAllPeerMention(key: "peer::all", handle: "all"))
        try checkTrue(OrbitComposerLogic.isAllPeerMention(key: "custom", handle: "@all"))
        try checkFalse(OrbitComposerLogic.isAllPeerMention(key: "peer::reviewer", handle: "reviewer"))
    }

    func testComposerTriggerBoundariesIgnoreEmailsAndPaths() throws {
        let atStart = OrbitComposerLogic.findActiveTrigger(in: "/super", cursorUtf16Offset: 6)
        try checkEqual(atStart?.kind, .slash)
        try checkEqual(atStart?.query, "super")

        let parensMention = OrbitComposerLogic.findActiveTrigger(in: "ask (@rev", cursorUtf16Offset: 9)
        try checkEqual(parensMention?.kind, .mention)
        try checkEqual(parensMention?.query, "rev")

        let email = OrbitComposerLogic.findActiveTrigger(in: "send to user@example", cursorUtf16Offset: 20)
        try checkNil(email)

        let path = OrbitComposerLogic.findActiveTrigger(in: "/Users/murphy", cursorUtf16Offset: 13)
        try checkNil(path)

        let leadingWhitespaceSlash = OrbitComposerLogic.findActiveTrigger(in: "  /goal", cursorUtf16Offset: 7)
        try checkEqual(leadingWhitespaceSlash?.kind, .slash)
        try checkEqual(leadingWhitespaceSlash?.query, "goal")

        let midSentenceSlash = OrbitComposerLogic.findActiveTrigger(in: "compare /goal", cursorUtf16Offset: 13)
        try checkNil(midSentenceSlash)

        let escapedSlash = OrbitComposerLogic.findActiveTrigger(in: "\\/goal", cursorUtf16Offset: 6)
        try checkNil(escapedSlash)

        try checkTrue(OrbitComposerLogic.matchesLeadingSlashCommand("/review", command: "/review"))
        try checkTrue(OrbitComposerLogic.matchesLeadingSlashCommand("  /REVIEW check diff", command: "/review"))
        try checkFalse(OrbitComposerLogic.matchesLeadingSlashCommand("/reviewers list", command: "/review"))
        try checkFalse(OrbitComposerLogic.matchesLeadingSlashCommand("\\/review check diff", command: "/review"))
        try checkFalse(OrbitComposerLogic.matchesLeadingSlashCommand("run /review now", command: "/review"))
    }

    func testComposerAutocompleteInsertionEmojiAndCrlfOffsets() throws {
        let emojiText = "Hi 🚀\r\n@rev"
        let utf16Len = (emojiText as NSString).length
        let trig = OrbitComposerLogic.findActiveTrigger(in: emojiText, cursorUtf16Offset: utf16Len)
        try checkEqual(trig?.kind, .mention)
        try checkEqual(trig?.query, "rev")

        if let trig {
            let applied = OrbitComposerLogic.applyAutocompleteInsertion(text: emojiText, trigger: trig, replacement: "@reviewer")
            try checkEqual(applied.newText, "Hi 🚀\r\n@reviewer ")
            try checkEqual(applied.newCursorUtf16, (applied.newText as NSString).length)
        }
    }

    func testComposerInsertTextReplacesUtf16SelectionSafely() throws {
        let initial = "Hi 🚀 world"
        let ns = initial as NSString
        let range = NSRange(location: 5, length: 0) // right after "Hi 🚀"
        let res = OrbitComposerLogic.insertText(into: initial, selectedRangeUtf16: range, insertion: "!")
        try checkEqual(res.newText, "Hi 🚀! world")
        try checkEqual(res.newCursorUtf16, 6)
        try checkTrue(ns.length > 0)
    }

    func testComposerLargeTextPasteAndAttachmentSizeLimit() throws {
        try checkTrue(OrbitComposerLogic.isLargeTextPaste(String(repeating: "a", count: 4000)))
        try checkTrue(OrbitComposerLogic.isLargeTextPaste(Array(repeating: "line", count: 100).joined(separator: "\n")))
        try checkFalse(OrbitComposerLogic.isLargeTextPaste("short text\nsecond line"))
        try checkFalse(OrbitComposerLogic.attachmentExceedsLimit(25 * 1024 * 1024))
        try checkTrue(OrbitComposerLogic.attachmentExceedsLimit(25 * 1024 * 1024 + 1))
    }

    func testComposerContainsWholeTokenPreventsSubstringBleed() throws {
        try checkTrue(OrbitComposerLogic.containsWholeToken("Check @app please", token: "@app"))
        try checkTrue(OrbitComposerLogic.containsWholeToken("(@app).", token: "@app"))
        try checkFalse(OrbitComposerLogic.containsWholeToken("Check @apple please", token: "@app"))
        try checkFalse(OrbitComposerLogic.containsWholeToken("foo@app.com", token: "@app"))
        try checkFalse(OrbitComposerLogic.containsWholeToken("Check @app", token: ""))
    }

    func testComposerScopedDraftWorktreeGuardQuoteIdAndQueueOps() throws {
        let scopeA = OrbitComposerLogic.scopedDraftKey(hostId: "h1", agentId: "a1", draftWorkspaceId: nil, draftCwd: nil)
        let scopeB = OrbitComposerLogic.scopedDraftKey(hostId: "h1", agentId: "a2", draftWorkspaceId: nil, draftCwd: nil)
        let draftWs = OrbitComposerLogic.scopedDraftKey(hostId: "h1", agentId: nil, draftWorkspaceId: "ws-1", draftCwd: "/repo")
        try checkTrue(scopeA != scopeB)
        try checkTrue(scopeA != draftWs)

        try checkFalse(OrbitComposerLogic.canCreateWorktreeSession(isWorktreeMode: true, isGitRepo: false))
        try checkTrue(OrbitComposerLogic.canCreateWorktreeSession(isWorktreeMode: true, isGitRepo: true))
        try checkTrue(OrbitComposerLogic.canCreateWorktreeSession(isWorktreeMode: false, isGitRepo: false))

        try checkEqual(OrbitComposerLogic.nextUniqueQuoteId(existingIds: ["turn-1"], baseTurnKey: "turn-1"), "turn-1#2")
        try checkEqual(OrbitComposerLogic.nextUniqueQuoteId(existingIds: ["turn-1", "turn-1#2"], baseTurnKey: "turn-1"), "turn-1#3")

        try checkEqual(OrbitComposerLogic.mergeQueuedPrompts("First", "Second"), "First\n\nSecond")
        try checkEqual(OrbitComposerLogic.reorderQueueIds(["q1", "q2", "q3"], fromIndex: 2, toOffset: 0), ["q3", "q1", "q2"])
    }

    // MARK: 29-31 Sidebar & Navigation

    func testSidebarScopedRowIdPreventsForEachCollisions() throws {
        let attentionId = OrbitSidebarLogic.scopedRowId(section: "attention", hostId: "h1", agentId: "a1")
        let pinnedId = OrbitSidebarLogic.scopedRowId(section: "pinned", hostId: "h1", agentId: "a1")
        let projectId = OrbitSidebarLogic.scopedRowId(section: "project", hostId: "h1", agentId: "a1", containerId: "proj-1")
        try checkTrue(attentionId != pinnedId)
        try checkTrue(pinnedId != projectId)
        try checkEqual(projectId, "project::h1::proj-1::a1")
    }

    func testSidebarSearchFilteringAndLegacyInboxVisibility() throws {
        try checkTrue(OrbitSidebarLogic.matchesSearch(query: "  ", fields: ["Anything"]))
        try checkFalse(OrbitSidebarLogic.matchesSearch(query: "ios", fields: ["Backend API", "/srv/api"]))
        try checkTrue(OrbitSidebarLogic.matchesSearch(query: "ios", fields: ["iOS Parity", "/Users/murphy/orbit"]))
        try checkFalse(OrbitSidebarLogic.shouldShowLegacyInbox(itemCount: 0))
        try checkTrue(OrbitSidebarLogic.shouldShowLegacyInbox(itemCount: 1))
    }

    func testSidebarWorkspacePeerStripVisibility() throws {
        try checkFalse(OrbitSidebarLogic.shouldShowWorkspacePeerStrip(isExplicitCluster: false, peerCount: 1, groupTitle: "待归类"))
        try checkFalse(OrbitSidebarLogic.shouldShowWorkspacePeerStrip(isExplicitCluster: false, peerCount: 1, groupTitle: "Unassigned"))
        try checkTrue(OrbitSidebarLogic.shouldShowWorkspacePeerStrip(isExplicitCluster: false, peerCount: 2, groupTitle: "Unassigned"))
        try checkTrue(OrbitSidebarLogic.shouldShowWorkspacePeerStrip(isExplicitCluster: true, peerCount: 1, groupTitle: "UI Parity"))
    }

    // MARK: 32-34 Board, Time, Colors & Store Value Types

    func testBoardEnglishSectionTitlesNormalization() throws {
        try checkEqual(OrbitBoardLogic.englishSectionTitle("目标"), "Goal")
        try checkEqual(OrbitBoardLogic.englishSectionTitle("决策与契约"), "Decisions & Contracts")
        try checkEqual(OrbitBoardLogic.englishSectionTitle("任务列表"), "Tasks")
        try checkEqual(OrbitBoardLogic.englishSectionTitle("发现与问题"), "Findings")
        try checkEqual(OrbitBoardLogic.englishSectionTitle("备注"), "Notes")
        try checkEqual(OrbitBoardLogic.englishSectionTitle("Custom Section"), "Custom Section")
    }

    func testTimeDurationCompactAndHexColorParsing() throws {
        try checkEqual(OrbitTimeLogic.formatDurationCompact(0), "0s")
        try checkEqual(OrbitTimeLogic.formatDurationCompact(450), "450ms")
        try checkEqual(OrbitTimeLogic.formatDurationCompact(72_000), "1m 12s")
        try checkEqual(OrbitTimeLogic.formatDurationCompact(3_900_000), "1h 5m")

        let rgb = OrbitTimeLogic.parseHexColor("#FF8000")
        try checkEqual(rgb?.red, 1.0)
        try checkEqual(rgb?.blue, 0.0)
        try checkEqual(rgb?.alpha, 1.0)

        let argb = OrbitTimeLogic.parseHexColor("#80FF0000")
        try checkEqual(argb?.red, 1.0)
        try checkEqual(argb?.green, 0.0)
        try checkTrue(abs((argb?.alpha ?? 0.0) - (128.0 / 255.0)) < 0.001)
    }

    func testStoreSharedValueTypesScopeUploadUndoAndCloudPrefs() throws {
        try checkEqual(OrbitGitReviewScope.workingTree.label, "Unstaged")
        try checkEqual(OrbitGitReviewScope.staged.label, "Staged")
        try checkEqual(OrbitGitReviewScope.all.label, "All Changes")

        let failedUpload = OrbitComposerUploadState.failed("Network timeout")
        try checkEqual(failedUpload, .failed("Network timeout"))

        let undo = OrbitPendingArchiveUndo(hostId: "h1", agentId: "a1", title: "Session A")
        try checkEqual(undo.hostId, "h1")
        try checkEqual(undo.agentId, "a1")

        let target = OrbitSubagentInspectorTarget(hostId: "h1", parentAgentId: "p1", childAgentId: "c1", title: "Worker")
        try checkEqual(target.id, "h1::p1::c1")

        let prefs = OrbitCloudSyncUiPreferences(
            sidebarProjectSort: "recent",
            sidebarThreadFilter: "running",
            drawerFilter: "all",
            coalesceExploration: true,
            defaultPermissionMode: "accept_edits",
            appearanceMode: "dark"
        )
        let data = try JSONEncoder().encode(prefs)
        let decoded = try JSONDecoder().decode(OrbitCloudSyncUiPreferences.self, from: data)
        try checkEqual(decoded, prefs)
    }

    func testStorageProtectionExcludesDatabaseAndSidecarsFromBackup() throws {
        let fm = FileManager.default
        let tempBase = fm.temporaryDirectory.appendingPathComponent("orbit-storage-test-\(UUID().uuidString)", isDirectory: true)
        try fm.createDirectory(at: tempBase, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: tempBase) }

        let dbURL = try OrbitStorageProtection.prepareProtectedDatabaseURL(in: tempBase)
        let storageDir = dbURL.deletingLastPathComponent()
        try checkEqual(storageDir.lastPathComponent, "Orbit")
        try checkEqual(dbURL.lastPathComponent, "orbit_ios_v3.sqlite")

        let dirValues = try storageDir.resourceValues(forKeys: [.isExcludedFromBackupKey])
        try checkEqual(dirValues.isExcludedFromBackup, true, "storage directory excluded from backup")
        #if os(iOS) && !targetEnvironment(simulator)
        let dirAttrs = try fm.attributesOfItem(atPath: storageDir.path)
        try checkEqual(
            dirAttrs[.protectionKey] as? FileProtectionType,
            .completeUntilFirstUserAuthentication,
            "storage directory file protection"
        )
        #endif

        // Open a real SQLite database in WAL mode inside the protected directory and verify
        // the DB and its -wal/-shm sidecars live under the excluded/protected directory.
        var db: OpaquePointer?
        try checkEqual(sqlite3_open(dbURL.path, &db), SQLITE_OK, "sqlite3_open")
        defer { sqlite3_close(db) }
        try checkEqual(sqlite3_exec(db, "PRAGMA journal_mode=WAL;", nil, nil, nil), SQLITE_OK, "enable WAL")
        try checkEqual(sqlite3_exec(db, "CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT);", nil, nil, nil), SQLITE_OK, "create table")
        try checkEqual(sqlite3_exec(db, "INSERT INTO kv VALUES ('host', 'devbox');", nil, nil, nil), SQLITE_OK, "insert row")

        let walURL = URL(fileURLWithPath: dbURL.path + "-wal")
        let shmURL = URL(fileURLWithPath: dbURL.path + "-shm")
        try checkTrue(fm.fileExists(atPath: dbURL.path), "sqlite db file created")
        try checkTrue(fm.fileExists(atPath: walURL.path), "sqlite -wal sidecar created")
        try checkTrue(fm.fileExists(atPath: shmURL.path), "sqlite -shm sidecar created")

        for fileURL in [dbURL, walURL, shmURL] {
            try checkEqual(
                fileURL.deletingLastPathComponent().standardizedFileURL,
                storageDir.standardizedFileURL,
                "\(fileURL.lastPathComponent) resides inside backup-excluded storage directory"
            )
            #if os(iOS) && !targetEnvironment(simulator)
            let attrs = try fm.attributesOfItem(atPath: fileURL.path)
            try checkEqual(
                attrs[.protectionKey] as? FileProtectionType,
                .completeUntilFirstUserAuthentication,
                "\(fileURL.lastPathComponent) inherited file protection"
            )
            #endif
        }
    }

    func testMarkAgentSeenDeduplicationAndForceClearOnUnread() throws {
        // First time viewing a thread always dispatches
        try checkTrue(OrbitAgentLogic.shouldDispatchMarkAgentSeen(lastMarkedSeq: nil, nextSeq: 0, force: false))
        try checkTrue(OrbitAgentLogic.shouldDispatchMarkAgentSeen(lastMarkedSeq: nil, nextSeq: 42, force: false))
        // Quiet streaming cursor advances dispatch only when seq strictly increases
        try checkFalse(OrbitAgentLogic.shouldDispatchMarkAgentSeen(lastMarkedSeq: 42, nextSeq: 42, force: false))
        try checkFalse(OrbitAgentLogic.shouldDispatchMarkAgentSeen(lastMarkedSeq: 42, nextSeq: 40, force: false))
        try checkTrue(OrbitAgentLogic.shouldDispatchMarkAgentSeen(lastMarkedSeq: 42, nextSeq: 43, force: false))
        // Explicit open or unread/attention clear MUST dispatch even when seq did not increase or is 0
        try checkTrue(OrbitAgentLogic.shouldDispatchMarkAgentSeen(lastMarkedSeq: 42, nextSeq: 42, force: true))
        try checkTrue(OrbitAgentLogic.shouldDispatchMarkAgentSeen(lastMarkedSeq: 42, nextSeq: 0, force: true))
    }

    // MARK: 35-40 iOS 5.0 Parity Logic (Plan Review, Batch Safety, Raw PTY, Push Deep Links, Voice, Keychain)

    func testPlanApprovalDetectionAndMarkdownExtraction() throws {
        try checkTrue(OrbitPlanApprovalLogic.isPlanApprovalToolName("ExitPlanMode"))
        try checkTrue(OrbitPlanApprovalLogic.isPlanApprovalToolName("claude:exit_plan_mode"))
        try checkTrue(OrbitPlanApprovalLogic.isPlanApprovalToolName("request_plan_approval"))
        try checkFalse(OrbitPlanApprovalLogic.isPlanApprovalToolName("Bash"))

        let argsJson = """
        {
          "plan": "# Auth Migration Plan\\n\\n1. Add Keychain key\\n2. Migrate DB",
          "plan_file_path": "/repo/.claude/plans/auth.md",
          "allowedPrompts": [{"tool": "Bash", "prompt": "cargo test"}]
        }
        """
        let parsed = OrbitPlanApprovalLogic.parsePlanApproval(
            requestId: "req-plan-1",
            title: "Claude wants to use ExitPlanMode",
            commandOrPath: "",
            toolName: "ExitPlanMode",
            argumentsJson: argsJson
        )
        try checkEqual(parsed?.title, "Auth Migration Plan")
        try checkEqual(parsed?.planFilePath, "/repo/.claude/plans/auth.md")
        try checkEqual(parsed?.allowedPrompts, ["Bash: cargo test"])

        let codexPlan = OrbitPlanApprovalLogic.extractCodexProposedPlan(
            from: "Intro <proposed_plan>\n# Codex Plan\n- Step A\n</proposed_plan> outro"
        )
        try checkEqual(codexPlan, "# Codex Plan\n- Step A")
    }

    func testBatchApprovalSafetyExcludesPlansQuestionsAndHighRisk() throws {
        let queue: [OrbitBatchApprovalCandidate] = [
            OrbitBatchApprovalCandidate(
                hostId: "h1", agentId: "a1", requestId: "r-low",
                title: "Read Cargo.toml", commandOrPath: "Cargo.toml",
                toolName: "Read", allowedDecisions: ["deny", "allow"]
            ),
            OrbitBatchApprovalCandidate(
                hostId: "h1", agentId: "a1", requestId: "r-plan",
                title: "ExitPlanMode", commandOrPath: "",
                toolName: "ExitPlanMode", allowedDecisions: ["deny", "allow"]
            ),
            OrbitBatchApprovalCandidate(
                hostId: "h1", agentId: "a1", requestId: "r-rmrf",
                title: "Run shell", commandOrPath: "rm -rf /tmp/build",
                toolName: "Bash", allowedDecisions: ["deny", "allow"]
            ),
            OrbitBatchApprovalCandidate(
                hostId: "h1", agentId: "a1", requestId: "r-push",
                title: "Run shell", commandOrPath: "git push origin main",
                toolName: "Bash", allowedDecisions: ["deny", "allow"]
            ),
            OrbitBatchApprovalCandidate(
                hostId: "h1", agentId: "a1", requestId: "r-question",
                title: "Pick target", commandOrPath: "Which env?",
                allowedDecisions: ["allow"], isQuestion: true
            ),
            OrbitBatchApprovalCandidate(
                hostId: "h1", agentId: "a1", requestId: "r-deny-only",
                title: "Read file", commandOrPath: "README.md",
                toolName: "Read", allowedDecisions: ["deny"]
            )
        ]

        let safe = OrbitBatchApprovalLogic.selectSafeBatchPermissions(queue)
        try checkEqual(safe.map(\.requestId), ["r-low"], "Only low/medium non-plan non-question allow-permitted items qualify")
    }

    func testRawPtyEncoderProducesAccurateVt100AndCtrlBytes() throws {
        try checkEqual(OrbitPtyInputLogic.encodeSpecialKey(.escape), Data([0x1B]))
        try checkEqual(OrbitPtyInputLogic.encodeSpecialKey(.tab), Data([0x09]))
        try checkEqual(OrbitPtyInputLogic.encodeSpecialKey(.backspace), Data([0x7F]))
        try checkEqual(OrbitPtyInputLogic.encodeSpecialKey(.enter), Data([0x0D]))
        try checkEqual(OrbitPtyInputLogic.encodeSpecialKey(.arrowUp), Data("\u{001B}[A".utf8))
        try checkEqual(OrbitPtyInputLogic.encodeSpecialKey(.arrowDown), Data("\u{001B}[B".utf8))
        try checkEqual(OrbitPtyInputLogic.encodeSpecialKey(.arrowRight), Data("\u{001B}[C".utf8))
        try checkEqual(OrbitPtyInputLogic.encodeSpecialKey(.arrowLeft), Data("\u{001B}[D".utf8))
        try checkEqual(OrbitPtyInputLogic.encodeText("c", ctrl: true), Data([0x03]), "Ctrl+C -> ETX")
        try checkEqual(OrbitPtyInputLogic.encodeText("D", ctrl: true), Data([0x04]), "Ctrl+D -> EOT")
        try checkEqual(OrbitPtyInputLogic.encodeText("b", alt: true), Data("\u{001B}b".utf8), "Alt+b -> ESC b")
        try checkEqual(OrbitPtyInputLogic.encodeText("\n"), Data([0x0D]), "Soft Return -> CR")
        try checkEqual(OrbitPtyInputLogic.encodeText("hello"), Data("hello".utf8))
    }

    func testPushNotificationDeepLinkAndUserInfoRouting() throws {
        let openAction = OrbitPushDeepLinkLogic.parseDeepLink("orbit://open?host=host_devbox&agent=ag_7")
        try checkEqual(openAction, .openSession(hostId: "host_devbox", agentId: "ag_7"))

        let pairAction = OrbitPushDeepLinkLogic.parseDeepLink("orbit://pair/abc123")
        try checkEqual(pairAction, .pairHost(uri: "orbit://pair/abc123"))

        let syncAction = OrbitPushDeepLinkLogic.parseDeepLink("orbit://sync-login?token=xyz")
        try checkEqual(syncAction, .syncLogin(uri: "orbit://sync-login?token=xyz"))

        let fromPayload = OrbitPushDeepLinkLogic.parseNotificationUserInfo([
            "event_kind": "permission_required",
            "host_id": "host_1",
            "agent_id": "ag_99",
            "deep_link": "orbit://open?host=host_1&agent=ag_99"
        ])
        try checkEqual(fromPayload, .openSession(hostId: "host_1", agentId: "ag_99"))

        let fallbackFields = OrbitPushDeepLinkLogic.parseNotificationUserInfo([
            "host_id": "host_2",
            "agent_id": "ag_42"
        ])
        try checkEqual(fallbackFields, .openSession(hostId: "host_2", agentId: "ag_42"))
    }

    func testVoiceDictationTranscriptMergeAndIntentClassification() throws {
        try checkEqual(
            OrbitVoiceCommandLogic.mergeDictatedText(existingDraft: "", spokenSegment: "Run all tests"),
            "Run all tests"
        )
        try checkEqual(
            OrbitVoiceCommandLogic.mergeDictatedText(existingDraft: "Fix auth", spokenSegment: "and update docs"),
            "Fix auth and update docs"
        )
        try checkEqual(OrbitVoiceCommandLogic.classifyTranscript("clear draft"), .clearDraft)
        try checkEqual(OrbitVoiceCommandLogic.classifyTranscript("清空草稿"), .clearDraft)
        try checkEqual(OrbitVoiceCommandLogic.classifyTranscript("下書きをクリア"), .clearDraft)
        try checkEqual(
            OrbitVoiceCommandLogic.classifyTranscript("Refactor the network layer"),
            .appendText("Refactor the network layer")
        )
    }

    func testKeychainCsprngAndProtectedFallbackPersistence() throws {
        let randomKey = try OrbitKeychain.generateRandomBytes(count: OrbitKeychain.masterKeyByteCount)
        try checkEqual(randomKey.count, 32)

        let tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("orbit-keychain-test-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let serviceName = "dev.orbit.app.unit-test.\(UUID().uuidString)"
        let kc = OrbitKeychain(service: serviceName)
        let first = try kc.loadOrCreateMasterKey(fallbackDirectory: tempDir)
        let second = try kc.loadOrCreateMasterKey(fallbackDirectory: tempDir)
        try checkEqual(first.key.count, 32)
        try checkEqual(first.key, second.key, "Repeated opens must return the identical persisted 32-byte master key")
        try checkFalse(second.newlyGenerated, "Second load must reuse existing key")
    }
}

#if ORBIT_CLI_TEST_RUNNER
@main
enum OrbitLogicCliMain {
    static func main() {
        let suite = OrbitIosLogicTests()
        let cases: [(String, () throws -> Void)] = [
            ("testPathNormalizeAndCompactHomePrefixes", suite.testPathNormalizeAndCompactHomePrefixes),
            ("testPathShortPathRetainsTrailingOrientation", suite.testPathShortPathRetainsTrailingOrientation),
            ("testPathStripWorkspacePrefixInsideAndOutsideCwd", suite.testPathStripWorkspacePrefixInsideAndOutsideCwd),
            ("testPathShortFileLabelAndResolveWorkspaceFilePath", suite.testPathShortFileLabelAndResolveWorkspaceFilePath),
            ("testPathBasenameAndExtensionsForImagesAndMarkdown", suite.testPathBasenameAndExtensionsForImagesAndMarkdown),
            ("testPathParseFileTargetFileUrlAndLineRange", suite.testPathParseFileTargetFileUrlAndLineRange),
            ("testPathParseFileTargetColonLineAndColumn", suite.testPathParseFileTargetColonLineAndColumn),
            ("testMarkdownLinkClassificationWebMailtoAnchorAndFile", suite.testMarkdownLinkClassificationWebMailtoAnchorAndFile),
            ("testMarkdownASCIIBoxTableFalsePositiveGuard", suite.testMarkdownASCIIBoxTableFalsePositiveGuard),
            ("testAgentGenericHarnessHandleDetection", suite.testAgentGenericHarnessHandleDetection),
            ("testAgentForegroundRunningVsBackgroundWaiting", suite.testAgentForegroundRunningVsBackgroundWaiting),
            ("testWakeupFilteringAndSettledStepGroupDisclosure", suite.testWakeupFilteringAndSettledStepGroupDisclosure),
            ("testMarkAgentSeenDeduplicationAndForceClearOnUnread", suite.testMarkAgentSeenDeduplicationAndForceClearOnUnread),
            ("testAgentProviderSwitchGuard", suite.testAgentProviderSwitchGuard),
            ("testAgentModelDeduplicationAcrossAliasesAndDots", suite.testAgentModelDeduplicationAcrossAliasesAndDots),
            ("testAgentStageScopeAndDismissedVersionKeys", suite.testAgentStageScopeAndDismissedVersionKeys),
            ("testAgentHostConfigClearableFields", suite.testAgentHostConfigClearableFields),
            ("testAgentOverviewBadgeTonePriority", suite.testAgentOverviewBadgeTonePriority),
            ("testAgentMultiHostProjectReorderPreservesOtherHosts", suite.testAgentMultiHostProjectReorderPreservesOtherHosts),
            ("testDecisionClassificationAndCancelDeclineSafety", suite.testDecisionClassificationAndCancelDeclineSafety),
            ("testDecisionLabelAndClampedNavigationIndex", suite.testDecisionLabelAndClampedNavigationIndex),
            ("testDecisionInspectablePayloadFormatsJsonAndSkipsEmpty", suite.testDecisionInspectablePayloadFormatsJsonAndSkipsEmpty),
            ("testComposerSlashCommandIcons", suite.testComposerSlashCommandIcons),
            ("testComposerPeerMentionDetectionAndAllTarget", suite.testComposerPeerMentionDetectionAndAllTarget),
            ("testComposerTriggerBoundariesIgnoreEmailsAndPaths", suite.testComposerTriggerBoundariesIgnoreEmailsAndPaths),
            ("testComposerAutocompleteInsertionEmojiAndCrlfOffsets", suite.testComposerAutocompleteInsertionEmojiAndCrlfOffsets),
            ("testComposerInsertTextReplacesUtf16SelectionSafely", suite.testComposerInsertTextReplacesUtf16SelectionSafely),
            ("testComposerLargeTextPasteAndAttachmentSizeLimit", suite.testComposerLargeTextPasteAndAttachmentSizeLimit),
            ("testComposerContainsWholeTokenPreventsSubstringBleed", suite.testComposerContainsWholeTokenPreventsSubstringBleed),
            ("testComposerScopedDraftWorktreeGuardQuoteIdAndQueueOps", suite.testComposerScopedDraftWorktreeGuardQuoteIdAndQueueOps),
            ("testSidebarScopedRowIdPreventsForEachCollisions", suite.testSidebarScopedRowIdPreventsForEachCollisions),
            ("testSidebarSearchFilteringAndLegacyInboxVisibility", suite.testSidebarSearchFilteringAndLegacyInboxVisibility),
            ("testSidebarWorkspacePeerStripVisibility", suite.testSidebarWorkspacePeerStripVisibility),
            ("testBoardEnglishSectionTitlesNormalization", suite.testBoardEnglishSectionTitlesNormalization),
            ("testTimeDurationCompactAndHexColorParsing", suite.testTimeDurationCompactAndHexColorParsing),
            ("testStoreSharedValueTypesScopeUploadUndoAndCloudPrefs", suite.testStoreSharedValueTypesScopeUploadUndoAndCloudPrefs),
            ("testStorageProtectionExcludesDatabaseAndSidecarsFromBackup", suite.testStorageProtectionExcludesDatabaseAndSidecarsFromBackup),
            ("testPlanApprovalDetectionAndMarkdownExtraction", suite.testPlanApprovalDetectionAndMarkdownExtraction),
            ("testBatchApprovalSafetyExcludesPlansQuestionsAndHighRisk", suite.testBatchApprovalSafetyExcludesPlansQuestionsAndHighRisk),
            ("testRawPtyEncoderProducesAccurateVt100AndCtrlBytes", suite.testRawPtyEncoderProducesAccurateVt100AndCtrlBytes),
            ("testPushNotificationDeepLinkAndUserInfoRouting", suite.testPushNotificationDeepLinkAndUserInfoRouting),
            ("testVoiceDictationTranscriptMergeAndIntentClassification", suite.testVoiceDictationTranscriptMergeAndIntentClassification),
            ("testKeychainCsprngAndProtectedFallbackPersistence", suite.testKeychainCsprngAndProtectedFallbackPersistence),
        ]

        var passed = 0
        var failed = 0
        for (name, fn) in cases {
            do {
                try fn()
                passed += 1
                print("[PASS] \(name)")
            } catch {
                failed += 1
                fputs("[FAIL] \(name): \(error)\n", stderr)
            }
        }
        print("OrbitIosLogicTests: \(passed) passed, \(failed) failed")
        if failed > 0 {
            exit(1)
        }
    }
}
#endif
