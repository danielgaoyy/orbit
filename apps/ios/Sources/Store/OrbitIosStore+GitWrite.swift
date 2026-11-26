import Foundation

// MARK: - Git Write, Stash, Rebase, Conflicts & Pull Request Store Extensions

extension OrbitIosStore {
    public func refreshGitWriteMetadata() {
        guard let core, let hid = selectedHostId else { return }
        let cwd = activeCwd
        Task { @MainActor [weak self] in
            let fetched: (GitPrStatus?, [GitStashEntry], [GitConflictFile], GitRebaseStatus?) =
                await Task.detached(priority: .userInitiated) {
                    let pr = try? core.gitPrStatus(hostId: hid, cwd: cwd)
                    let stashes = (try? core.gitStashList(hostId: hid, cwd: cwd))?.entries ?? []
                    let conflicts = (try? core.gitConflicts(hostId: hid, cwd: cwd))?.files ?? []
                    let rebase = try? core.gitRebaseStatus(hostId: hid, cwd: cwd)
                    return (pr, stashes, conflicts, rebase)
                }.value
            guard let self, self.selectedHostId == hid else { return }
            self.gitPrStatus = fetched.0
            self.gitStashes = fetched.1
            self.gitConflicts = fetched.2
            self.gitRebaseStatus = fetched.3
        }
    }

    public func gitStage(paths: [String], all: Bool = false) {
        guard let core, let hid = selectedHostId else { return }
        let cwd = activeCwd
        gitWriteBusy = true
        gitWriteError = nil
        Task { @MainActor [weak self] in
            do {
                _ = try await Task.detached(priority: .userInitiated) {
                    try core.gitStagePaths(hostId: hid, cwd: cwd, paths: paths, all: all)
                }.value
                guard let self else { return }
                self.gitWriteBusy = false
                self.hapticLight()
                self.refreshGitReview()
            } catch {
                self?.gitWriteBusy = false
                self?.gitWriteError = OrbitErrorFormatter.humanize(error)
            }
        }
    }

    public func gitUnstage(paths: [String], all: Bool = false) {
        guard let core, let hid = selectedHostId else { return }
        let cwd = activeCwd
        gitWriteBusy = true
        gitWriteError = nil
        Task { @MainActor [weak self] in
            do {
                _ = try await Task.detached(priority: .userInitiated) {
                    try core.gitUnstagePaths(hostId: hid, cwd: cwd, paths: paths, all: all)
                }.value
                guard let self else { return }
                self.gitWriteBusy = false
                self.hapticLight()
                self.refreshGitReview()
            } catch {
                self?.gitWriteBusy = false
                self?.gitWriteError = OrbitErrorFormatter.humanize(error)
            }
        }
    }

    public func generateGitCommitMessage() async -> String? {
        guard let core, let hid = selectedHostId else { return nil }
        let cwd = activeCwd
        gitWriteBusy = true
        gitWriteError = nil
        do {
            let generated: GitGeneratedCommitMessage = try await Task.detached(priority: .userInitiated) {
                try core.gitGenerateCommitMessage(hostId: hid, cwd: cwd)
            }.value
            gitWriteBusy = false
            hapticLight()
            return generated.message
        } catch {
            gitWriteBusy = false
            gitWriteError = OrbitErrorFormatter.humanize(error)
            return nil
        }
    }

    public func commitGitChanges(message: String, amend: Bool = false, onSuccess: (() -> Void)? = nil) {
        guard let core, let hid = selectedHostId else { return }
        let trimmed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        let cwd = activeCwd
        gitWriteBusy = true
        gitWriteError = nil
        Task { @MainActor [weak self] in
            do {
                let res: GitCommitResult = try await Task.detached(priority: .userInitiated) {
                    try core.gitCommit(hostId: hid, cwd: cwd, message: trimmed, amend: amend)
                }.value
                guard let self else { return }
                self.gitWriteBusy = false
                self.hapticSuccess()
                self.showToast("Committed \(res.shortSha)")
                self.refreshGitReview()
                self.refreshGitWriteMetadata()
                onSuccess?()
            } catch {
                self?.gitWriteBusy = false
                self?.gitWriteError = OrbitErrorFormatter.humanize(error)
            }
        }
    }

    public func saveGitStash(message: String?, includeUntracked: Bool) {
        guard let core, let hid = selectedHostId else { return }
        let cwd = activeCwd
        let cleanMsg = message?.trimmingCharacters(in: .whitespacesAndNewlines)
        gitWriteBusy = true
        gitWriteError = nil
        Task { @MainActor [weak self] in
            do {
                _ = try await Task.detached(priority: .userInitiated) {
                    try core.gitStashSave(
                        hostId: hid,
                        cwd: cwd,
                        message: (cleanMsg?.isEmpty == false) ? cleanMsg : nil,
                        includeUntracked: includeUntracked
                    )
                }.value
                guard let self else { return }
                self.gitWriteBusy = false
                self.hapticSuccess()
                self.showToast("Saved stash")
                self.refreshGitReview()
                self.refreshGitWriteMetadata()
            } catch {
                self?.gitWriteBusy = false
                self?.gitWriteError = OrbitErrorFormatter.humanize(error)
            }
        }
    }

    public func applyGitStash(_ entry: GitStashEntry, pop: Bool) {
        guard let core, let hid = selectedHostId else { return }
        let cwd = activeCwd
        gitWriteBusy = true
        gitWriteError = nil
        Task { @MainActor [weak self] in
            do {
                _ = try await Task.detached(priority: .userInitiated) {
                    if pop {
                        return try core.gitStashPop(hostId: hid, cwd: cwd, stashRef: entry.stashRef, expectedOid: entry.oid)
                    } else {
                        return try core.gitStashApply(hostId: hid, cwd: cwd, stashRef: entry.stashRef, expectedOid: entry.oid)
                    }
                }.value
                guard let self else { return }
                self.gitWriteBusy = false
                self.hapticSuccess()
                self.showToast(pop ? "Popped stash" : "Applied stash")
                self.refreshGitReview()
                self.refreshGitWriteMetadata()
            } catch {
                self?.gitWriteBusy = false
                self?.gitWriteError = OrbitErrorFormatter.humanize(error)
            }
        }
    }

    public func dropGitStash(_ entry: GitStashEntry) {
        guard let core, let hid = selectedHostId else { return }
        let cwd = activeCwd
        gitWriteBusy = true
        gitWriteError = nil
        Task { @MainActor [weak self] in
            do {
                _ = try await Task.detached(priority: .userInitiated) {
                    try core.gitStashDrop(hostId: hid, cwd: cwd, stashRef: entry.stashRef, expectedOid: entry.oid)
                }.value
                guard let self else { return }
                self.gitWriteBusy = false
                self.hapticMedium()
                self.refreshGitWriteMetadata()
            } catch {
                self?.gitWriteBusy = false
                self?.gitWriteError = OrbitErrorFormatter.humanize(error)
            }
        }
    }

    public func createGitPullRequest(
        title: String,
        body: String,
        base: String?,
        draft: Bool,
        onCreated: ((GitCreatePrResult) -> Void)? = nil
    ) {
        guard let core, let hid = selectedHostId else { return }
        let cwd = activeCwd
        let cleanTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleanTitle.isEmpty else { return }
        let cleanBase = base?.trimmingCharacters(in: .whitespacesAndNewlines)
        gitWriteBusy = true
        gitWriteError = nil
        Task { @MainActor [weak self] in
            do {
                let res: GitCreatePrResult = try await Task.detached(priority: .userInitiated) {
                    try core.gitCreatePr(
                        hostId: hid,
                        cwd: cwd,
                        title: cleanTitle,
                        body: body,
                        base: (cleanBase?.isEmpty == false) ? cleanBase : nil,
                        draft: draft
                    )
                }.value
                guard let self else { return }
                self.gitWriteBusy = false
                self.hapticSuccess()
                self.showToast("PR created")
                self.refreshGitWriteMetadata()
                onCreated?(res)
            } catch {
                self?.gitWriteBusy = false
                self?.gitWriteError = OrbitErrorFormatter.humanize(error)
            }
        }
    }

    public func resolveGitConflict(
        file: GitConflictFile,
        blockIndex: UInt32? = nil,
        resolution: GitConflictResolution
    ) {
        guard let core, let hid = selectedHostId else { return }
        let cwd = activeCwd
        gitWriteBusy = true
        gitWriteError = nil
        Task { @MainActor [weak self] in
            do {
                _ = try await Task.detached(priority: .userInitiated) {
                    try core.gitResolveConflict(
                        hostId: hid,
                        cwd: cwd,
                        path: file.path,
                        blockIndex: blockIndex,
                        resolution: resolution,
                        autoStage: true,
                        expectedSha256: file.contentSha256
                    )
                }.value
                guard let self else { return }
                self.gitWriteBusy = false
                self.hapticSuccess()
                self.refreshGitReview()
                self.refreshGitWriteMetadata()
            } catch {
                self?.gitWriteBusy = false
                self?.gitWriteError = OrbitErrorFormatter.humanize(error)
            }
        }
    }

    public func performGitRebaseAction(_ action: String, ontoRef: String? = nil) {
        guard let core, let hid = selectedHostId else { return }
        let cwd = activeCwd
        let expectedStep = gitRebaseStatus?.currentStep
        gitWriteBusy = true
        gitWriteError = nil
        Task { @MainActor [weak self] in
            do {
                try await Task.detached(priority: .userInitiated) {
                    switch action {
                    case "start":
                        if let ref = ontoRef?.trimmingCharacters(in: .whitespacesAndNewlines), !ref.isEmpty {
                            _ = try core.gitRebaseStart(hostId: hid, cwd: cwd, ontoRef: ref)
                        }
                    case "continue":
                        _ = try core.gitRebaseContinue(hostId: hid, cwd: cwd)
                    case "skip":
                        _ = try core.gitRebaseSkip(hostId: hid, cwd: cwd, expectedStep: expectedStep)
                    case "abort":
                        _ = try core.gitRebaseAbort(hostId: hid, cwd: cwd)
                    default:
                        break
                    }
                }.value
                guard let self else { return }
                self.gitWriteBusy = false
                self.hapticMedium()
                self.refreshGitReview()
                self.refreshGitWriteMetadata()
            } catch {
                self?.gitWriteBusy = false
                self?.gitWriteError = OrbitErrorFormatter.humanize(error)
            }
        }
    }

    /// Runs a quick git command (like `git push` or `git checkout -b <branch>`) in the active host PTY terminal.
    public func runGitCommandInTerminal(_ command: String) {
        let trimmed = command.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        openTerminal()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
            self?.sendTerminalKeys(trimmed + "\r")
        }
    }
}
