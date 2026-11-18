import SwiftUI

@main
struct OrbitMobileApp: App {
    @StateObject private var store = OrbitIosStore()

    var body: some Scene {
        WindowGroup {
            OrbitRootView(store: store)
                .preferredColorScheme(.dark)
        }
    }
}

struct OrbitRootView: View {
    @ObservedObject var store: OrbitIosStore
    @State private var selectedTab: Int = 0

    private var activeCwd: String {
        store.currentHostDetail?.workspaces.first?.cwd
            ?? store.currentHostDetail?.agents.first?.cwd
            ?? "~"
    }

    var body: some View {
        TabView(selection: $selectedTab) {
            NavigationStack {
                FleetHomeView(store: store, onOpenStream: { selectedTab = 1 })
            }
            .tabItem {
                Label("Fleet", systemImage: "server.rack")
            }
            .tag(0)

            NavigationStack {
                AgentTimelineScreen(store: store)
            }
            .tabItem {
                Label("Stream", systemImage: "sparkles")
            }
            .tag(1)

            NavigationStack {
                GitDiffScreen(store: store, cwd: activeCwd)
            }
            .tabItem {
                Label("Diff", systemImage: "arrow.triangle.branch")
            }
            .tag(2)

            NavigationStack {
                NativeTerminalScreen(store: store, cwd: activeCwd)
            }
            .tabItem {
                Label("PTY", systemImage: "terminal")
            }
            .tag(3)
        }
    }
}

struct FleetHomeView: View {
    @ObservedObject var store: OrbitIosStore
    let onOpenStream: () -> Void

    @State private var pairUri = ""
    @State private var newAgentPrompt = ""
    @State private var newAgentCwd = "~"
    @State private var selectedProvider: FfiProviderKind = .antigravity

    var body: some View {
        List {
            Section("Pair Host (`orbit pair`)") {
                HStack {
                    TextField("orbit://pair/...", text: $pairUri)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .font(.system(.footnote, design: .monospaced))
                    Button("Pair") {
                        store.pairHost(uri: pairUri)
                        pairUri = ""
                    }
                    .buttonStyle(.borderedProminent)
                }
                if let err = store.lastError {
                    Text(err)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }

            Section("Connected Fleet (\(store.hosts.count))") {
                ForEach(store.hosts, id: \.hostId) { host in
                    Button {
                        store.selectHost(host.hostId)
                    } label: {
                        HStack {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(host.displayName)
                                    .font(.headline)
                                    .foregroundStyle(.primary)
                                Text(connectionBadgeText(host.connectionState))
                                    .font(.caption2)
                                    .foregroundStyle(connectionBadgeColor(host.connectionState))
                            }
                            Spacer()
                            if host.pendingApprovalCount > 0 {
                                Text("\(host.pendingApprovalCount) pending")
                                    .font(.caption2.bold())
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 3)
                                    .background(Color.orange.opacity(0.2))
                                    .foregroundStyle(.orange)
                                    .clipShape(Capsule())
                            }
                        }
                    }
                    .swipeActions {
                        Button(role: .destructive) {
                            store.removeHost(host.hostId)
                        } label: {
                            Label("Remove", systemImage: "trash")
                        }
                    }
                }
            }

            if let detail = store.currentHostDetail {
                Section("Sessions on \(detail.host.displayName)") {
                    ForEach(detail.agents, id: \.agentId) { agent in
                        Button {
                            store.selectAgent(agent.agentId)
                            onOpenStream()
                        } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 3) {
                                    Text(agent.title)
                                        .font(.subheadline.weight(.semibold))
                                        .foregroundStyle(.primary)
                                    Text("\(agent.providerLabel) · \(agent.cwd)")
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                Text(agent.statusLabel)
                                    .font(.caption2.weight(.medium))
                                    .foregroundStyle(agent.isAwaitingPermission ? .orange : .blue)
                            }
                        }
                    }
                }

                Section("Launch New Agent") {
                    Picker("Provider", selection: $selectedProvider) {
                        Text("Antigravity").tag(FfiProviderKind.antigravity)
                        Text("Codex").tag(FfiProviderKind.codex)
                        Text("Claude").tag(FfiProviderKind.claudeCode)
                    }
                    .pickerStyle(.segmented)

                    TextField("Workspace CWD", text: $newAgentCwd)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .font(.system(.caption, design: .monospaced))

                    HStack {
                        TextField("Initial task prompt...", text: $newAgentPrompt)
                        Button("Start") {
                            store.createAgent(
                                cwd: newAgentCwd,
                                provider: selectedProvider,
                                mode: .acceptEdits,
                                prompt: newAgentPrompt
                            )
                            newAgentPrompt = ""
                            onOpenStream()
                        }
                        .buttonStyle(.borderedProminent)
                    }
                }
            }
        }
        .navigationTitle("Orbit Fleet")
    }

    private func connectionBadgeText(_ state: FfiConnectionState) -> String {
        switch state {
        case .onlineDirect: return "Online · LAN Direct"
        case .onlineRelay: return "Online · Noise E2EE Relay"
        case .connectingDirect, .connectingRelay: return "Connecting..."
        case .disconnected: return "Offline"
        }
    }

    private func connectionBadgeColor(_ state: FfiConnectionState) -> Color {
        switch state {
        case .onlineDirect: return .green
        case .onlineRelay: return .purple
        case .connectingDirect, .connectingRelay: return .orange
        case .disconnected: return .secondary
        }
    }
}

struct AgentTimelineScreen: View {
    @ObservedObject var store: OrbitIosStore
    @State private var composer = ""

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    ForEach(Array(store.timelineItems.enumerated()), id: \.offset) { _, item in
                        TimelineCardView(store: store, item: item)
                    }
                }
                .padding()
            }

            Divider()

            HStack(spacing: 8) {
                TextField("Send follow-up or steer...", text: $composer)
                    .textFieldStyle(.roundedBorder)
                Button {
                    store.interruptAgent()
                } label: {
                    Image(systemName: "stop.circle.fill")
                        .foregroundStyle(.red)
                }
                Button {
                    let msg = composer.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !msg.isEmpty else { return }
                    composer = ""
                    store.sendPrompt(msg)
                } label: {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.title2)
                }
            }
            .padding(12)
        }
        .navigationTitle("Agent Stream")
        .navigationBarTitleDisplayMode(.inline)
    }
}

struct TimelineCardView: View {
    @ObservedObject var store: OrbitIosStore
    let item: FfiTimelineItem

    var body: some View {
        switch item {
        case let .userPrompt(_, _, _, blocks):
            VStack(alignment: .leading, spacing: 6) {
                RichBlocksView(blocks: blocks)
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.blue.opacity(0.18))
            .clipShape(RoundedRectangle(cornerRadius: 12))

        case let .assistantThinking(_, _, text):
            DisclosureGroup("Reasoning trace") {
                Text(text)
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .font(.caption)
            .padding(10)
            .background(Color.white.opacity(0.04))
            .clipShape(RoundedRectangle(cornerRadius: 10))

        case let .assistantMessage(_, _, _, _, blocks):
            VStack(alignment: .leading, spacing: 6) {
                RichBlocksView(blocks: blocks)
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.white.opacity(0.06))
            .clipShape(RoundedRectangle(cornerRadius: 12))

        case let .toolCall(_, _, status, summaryTitle, previewBody, _):
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(summaryTitle)
                        .font(.system(.caption, design: .monospaced))
                        .lineLimit(1)
                    Spacer()
                    Text(String(describing: status))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                if let body = previewBody, !body.isEmpty {
                    Text(body)
                        .font(.system(.caption2, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .lineLimit(6)
                }
            }
            .padding(10)
            .background(Color.black.opacity(0.35))
            .clipShape(RoundedRectangle(cornerRadius: 8))

        case let .permissionRequest(_, requestId, title, commandOrPath, allowedDecisions, resolvedDecision):
            VStack(alignment: .leading, spacing: 8) {
                Label(title, systemImage: "exclamationmark.shield.fill")
                    .font(.subheadline.bold())
                    .foregroundStyle(.orange)
                Text(commandOrPath)
                    .font(.system(.caption, design: .monospaced))
                if let resolved = resolvedDecision {
                    Text("Decision: \(resolved)")
                        .font(.caption.bold())
                        .foregroundStyle(.green)
                } else {
                    HStack {
                        ForEach(allowedDecisions.isEmpty ? ["allow", "deny"] : allowedDecisions, id: \.self) { dec in
                            Button(dec) {
                                store.resolvePermission(requestId: requestId, decision: dec)
                            }
                            .buttonStyle(.borderedProminent)
                            .tint(dec.lowercased().contains("deny") ? .red : .blue)
                        }
                    }
                }
            }
            .padding(12)
            .background(Color.orange.opacity(0.12))
            .clipShape(RoundedRectangle(cornerRadius: 12))

        case let .questionRequest(_, requestId, questions, resolvedAnswers):
            VStack(alignment: .leading, spacing: 8) {
                ForEach(questions, id: \.id) { q in
                    Text(q.question)
                        .font(.subheadline.weight(.medium))
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack {
                            ForEach(q.options, id: \.self) { opt in
                                Button(opt) {
                                    store.answerQuestion(requestId: requestId, option: opt)
                                }
                                .buttonStyle(.bordered)
                                .disabled(resolvedAnswers != nil)
                            }
                        }
                    }
                }
            }
            .padding(12)
            .background(Color.cyan.opacity(0.1))
            .clipShape(RoundedRectangle(cornerRadius: 12))

        case let .notice(_, _, message):
            Text(message)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }
}

struct RichBlocksView: View {
    let blocks: [FfiRichBlock]

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case let .heading(level, spans):
                    Text(buildInlineAttributed(spans))
                        .font(level <= 2 ? .headline : .subheadline.bold())
                case let .paragraph(spans), let .blockQuote(spans):
                    Text(buildInlineAttributed(spans))
                        .font(.subheadline)
                case let .listItem(ordered, index, _, spans):
                    HStack(alignment: .top, spacing: 6) {
                        Text(ordered ? "\(index)." : "•")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(buildInlineAttributed(spans))
                            .font(.subheadline)
                    }
                case let .codeBlock(language, _, lines):
                    VStack(alignment: .leading, spacing: 4) {
                        if !language.isEmpty {
                            Text(language)
                                .font(.system(.caption2, design: .monospaced))
                                .foregroundStyle(.secondary)
                        }
                        ScrollView(.horizontal, showsIndicators: false) {
                            VStack(alignment: .leading, spacing: 2) {
                                ForEach(lines, id: \.lineNumber) { ln in
                                    Text(buildHighlightedLine(ln))
                                        .font(.system(size: 12, design: .monospaced))
                                }
                            }
                        }
                    }
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color(hex: "#080C14"))
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                case .horizontalRule:
                    Divider()
                }
            }
        }
    }

    private func buildInlineAttributed(_ spans: [FfiInlineSpan]) -> AttributedString {
        var result = AttributedString()
        for sp in spans {
            var part = AttributedString(sp.text)
            if sp.inlineCode {
                part.font = .system(.footnote, design: .monospaced)
                part.foregroundColor = .cyan
            } else if sp.bold && sp.italic {
                part.font = .subheadline.bold().italic()
            } else if sp.bold {
                part.font = .subheadline.bold()
            } else if sp.italic {
                part.font = .subheadline.italic()
            }
            result.append(part)
        }
        return result
    }

    private func buildHighlightedLine(_ line: FfiHighlightedLine) -> AttributedString {
        var result = AttributedString()
        for tok in line.tokens {
            var part = AttributedString(tok.text)
            part.foregroundColor = Color(hex: tok.fgHex)
            result.append(part)
        }
        return result
    }
}

struct GitDiffScreen: View {
    @ObservedObject var store: OrbitIosStore
    let cwd: String

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 14) {
                if let review = store.gitReview, !review.diffFiles.isEmpty {
                    ForEach(review.diffFiles, id: \.path) { file in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text(file.path)
                                    .font(.system(.caption, design: .monospaced).bold())
                                Spacer()
                                Text("+\(file.additions) -\(file.deletions)")
                                    .font(.system(.caption2, design: .monospaced))
                                    .foregroundStyle(.green)
                            }
                            ForEach(Array(file.lines.enumerated()), id: \.offset) { _, ln in
                                Text(ln.content)
                                    .font(.system(size: 11.5, design: .monospaced))
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 1)
                                    .background(diffRowBg(ln.kind))
                            }
                        }
                        .padding(10)
                        .background(Color.white.opacity(0.04))
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                    }
                } else {
                    Text("Working tree clean or tap Refresh.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .padding()
                }
            }
            .padding()
        }
        .navigationTitle("Git Diff")
        .toolbar {
            Button("Refresh") {
                store.loadGitReview(cwd: cwd)
            }
        }
        .onAppear {
            store.loadGitReview(cwd: cwd)
        }
    }

    private func diffRowBg(_ kind: FfiDiffLineKind) -> Color {
        switch kind {
        case .added: return Color.green.opacity(0.16)
        case .removed: return Color.red.opacity(0.16)
        case .hunkHeader: return Color.blue.opacity(0.14)
        case .meta, .context: return Color.clear
        }
    }
}

struct NativeTerminalScreen: View {
    @ObservedObject var store: OrbitIosStore
    let cwd: String
    @State private var inputLine = ""

    var body: some View {
        VStack(spacing: 8) {
            ScrollView([.vertical, .horizontal]) {
                LazyVStack(alignment: .leading, spacing: 1) {
                    if let screen = store.terminalScreen {
                        ForEach(screen.rowsData, id: \.rowIndex) { row in
                            Text(renderTerminalRow(row))
                                .font(.system(size: 12, design: .monospaced))
                        }
                    }
                }
                .padding(10)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color(hex: "#060911"))
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .padding(.horizontal, 12)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(["ESC", "TAB", "CTRL+C", "UP", "DOWN"], id: \.self) { label in
                        Button(label) {
                            let seq: String
                            switch label {
                            case "ESC": seq = "\u{001B}"
                            case "TAB": seq = "\t"
                            case "CTRL+C": seq = "\u{0003}"
                            case "UP": seq = "\u{001B}[A"
                            default: seq = "\u{001B}[B"
                            }
                            store.sendTerminalKeys(seq)
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    }
                }
                .padding(.horizontal, 12)
            }

            HStack {
                TextField("Command...", text: $inputLine)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .font(.system(.footnote, design: .monospaced))
                    .textFieldStyle(.roundedBorder)
                Button("Run") {
                    store.sendTerminalKeys(inputLine + "\r")
                    inputLine = ""
                }
                .buttonStyle(.borderedProminent)
            }
            .padding(.horizontal, 12)
            .padding(.bottom, 8)
        }
        .navigationTitle("Native PTY")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            store.startTerminal(cwd: cwd)
        }
    }

    private func renderTerminalRow(_ row: FfiTerminalRow) -> AttributedString {
        var out = AttributedString()
        for sp in row.spans {
            var part = AttributedString(sp.text)
            part.foregroundColor = Color(hex: sp.fgHex)
            out.append(part)
        }
        return out
    }
}
