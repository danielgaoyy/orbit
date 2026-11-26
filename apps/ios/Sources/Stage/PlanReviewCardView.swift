import SwiftUI

/// Dedicated inline Plan Review Card matching Desktop's `PlanReviewCard` safety contract:
/// - Renders the proposed plan Markdown in Preview or inline Edit mode
/// - Surfaces plan file path and requested `allowedPrompts`
/// - Actions: Approve & Build (with execution mode switch), Request Revisions (feedback drawer), Dismiss
/// - Never included in blind batch approvals.
struct PlanReviewCardView: View {
    @ObservedObject var store: OrbitIosStore
    let plan: OrbitParsedPlanApproval
    let queuePosition: Int
    let totalQueued: Int
    let isSubmitting: Bool
    let onStepQueue: (Int) -> Void
    let onResolvePlan: (PlanApprovalAction, String?, String?, FfiSessionMode?) -> Void

    @Environment(\.orbitPalette) private var palette
    @State private var isEditMode: Bool = false
    @State private var editedMarkdown: String = ""
    @State private var isFeedbackOpen: Bool = false
    @State private var feedbackText: String = ""
    @State private var targetMode: FfiSessionMode = .acceptEdits
    @State private var isExpanded: Bool = false

    private var isModified: Bool {
        let orig = plan.planMarkdown.trimmingCharacters(in: .whitespacesAndNewlines)
        let cur = editedMarkdown.trimmingCharacters(in: .whitespacesAndNewlines)
        return !cur.isEmpty && cur != orig
    }

    var body: some View {
        VStack(alignment: .leading, spacing: OrbitSpacing.sm) {
            // Header
            HStack(spacing: OrbitSpacing.xs + 2) {
                Image(systemName: "list.bullet.clipboard")
                    .font(OrbitTypography.iconSmall)
                    .foregroundStyle(palette.accentSky)

                VStack(alignment: .leading, spacing: 1) {
                    Text(plan.title)
                        .font(OrbitTypography.subhead(.semibold))
                        .foregroundStyle(palette.fgPrimary)
                        .lineLimit(1)

                    if let path = plan.planFilePath, !path.isEmpty {
                        Text(OrbitPathLogic.stripWorkspacePrefix(path, cwd: store.activeCwd))
                            .font(OrbitTypography.code(.regular))
                            .foregroundStyle(palette.fgMuted)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                }

                Spacer(minLength: 4)

                // Preview / Edit pill toggle
                HStack(spacing: 2) {
                    segmentChip(title: OrbitL10n.tr("Preview"), selected: !isEditMode) {
                        isEditMode = false
                    }
                    segmentChip(title: OrbitL10n.tr("Edit"), selected: isEditMode) {
                        isEditMode = true
                    }
                }
                .padding(2)
                .background(palette.subtleFill, in: Capsule())

                Button {
                    withAnimation(OrbitMotion.ui) {
                        isExpanded.toggle()
                    }
                } label: {
                    Image(systemName: isExpanded ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(palette.fgSecondary)
                        .frame(width: 28, height: 28)
                        .background(palette.subtleFill, in: Circle())
                }
                .buttonStyle(.plain)

                if totalQueued > 1 {
                    HStack(spacing: 2) {
                        Button { onStepQueue(-1) } label: {
                            Image(systemName: "chevron.left")
                                .font(.system(size: 10, weight: .semibold))
                                .frame(width: 22, height: 24)
                        }
                        .buttonStyle(.plain)
                        Text("\(queuePosition)/\(totalQueued)")
                            .font(OrbitTypography.meta(.semibold))
                        Button { onStepQueue(1) } label: {
                            Image(systemName: "chevron.right")
                                .font(.system(size: 10, weight: .semibold))
                                .frame(width: 22, height: 24)
                        }
                        .buttonStyle(.plain)
                    }
                    .foregroundStyle(palette.accentSky)
                    .padding(.horizontal, 6)
                    .background(palette.accentSky.opacity(0.12), in: Capsule())
                }
            }

            // Plan Markdown Body (Preview or Edit)
            if isEditMode {
                TextEditor(text: $editedMarkdown)
                    .font(OrbitTypography.code(.regular))
                    .foregroundStyle(palette.fgPrimary)
                    .scrollContentBackground(.hidden)
                    .padding(8)
                    .frame(height: isExpanded ? 260 : 160)
                    .background(palette.recessed)
                    .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .strokeBorder(palette.hairline, lineWidth: 0.5)
                    )
            } else {
                ScrollView(.vertical, showsIndicators: true) {
                    let bodyText = editedMarkdown.isEmpty ? plan.planMarkdown : editedMarkdown
                    RichMarkdownView(
                        blocks: parseMarkdownPreview(markdown: bodyText, streaming: false),
                        fallbackText: bodyText
                    )
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(OrbitSpacing.sm + 2)
                }
                .frame(maxHeight: isExpanded ? 280 : 170)
                .background(palette.recessed)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .strokeBorder(palette.hairline, lineWidth: 0.5)
                )
            }

            // Requested Allowed Prompts
            if !plan.allowedPrompts.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text(OrbitL10n.tr("Requested Permissions"))
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(palette.fgMuted)
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 6) {
                            ForEach(plan.allowedPrompts, id: \.self) { perm in
                                Text(perm)
                                    .font(OrbitTypography.code(.regular))
                                    .foregroundStyle(palette.fgSecondary)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 3)
                                    .background(palette.subtleFill, in: Capsule())
                            }
                        }
                    }
                }
            }

            // Execution Mode Switcher
            HStack(spacing: OrbitSpacing.xs) {
                Text(OrbitL10n.tr("Execution Mode"))
                    .font(OrbitTypography.meta(.medium))
                    .foregroundStyle(palette.fgMuted)
                Spacer(minLength: 4)
                modePill(.acceptEdits, label: OrbitL10n.tr("Accept Edits"))
                modePill(.default, label: OrbitL10n.tr("Always Ask"))
                modePill(.auto, label: OrbitL10n.tr("Auto (Guardian)"))
            }

            // Optional Feedback Drawer for Request Revisions
            if isFeedbackOpen {
                VStack(alignment: .leading, spacing: 6) {
                    TextField(
                        OrbitL10n.tr("Describe requested changes to this plan…"),
                        text: $feedbackText,
                        axis: .vertical
                    )
                    .font(OrbitTypography.subhead(.regular))
                    .foregroundStyle(palette.fgPrimary)
                    .lineLimit(2...4)
                    .padding(8)
                    .background(palette.recessed)
                    .clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 9, style: .continuous)
                            .strokeBorder(palette.borderDefault, lineWidth: 0.5)
                    )
                }
                .transition(.opacity.combined(with: .move(edge: .top)))
            }

            // Action Buttons
            HStack(spacing: OrbitSpacing.sm) {
                Button {
                    onResolvePlan(.dismiss, nil, nil, nil)
                } label: {
                    Text(OrbitL10n.tr("Dismiss"))
                        .font(OrbitTypography.subhead(.medium))
                        .foregroundStyle(palette.fgSecondary)
                        .padding(.horizontal, 12)
                        .frame(minHeight: OrbitSpacing.hitTarget)
                        .background(palette.subtleFill, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                }
                .buttonStyle(.plain)

                if isFeedbackOpen {
                    HStack(spacing: OrbitSpacing.xs) {
                        Button {
                            withAnimation(OrbitMotion.ui) {
                                isFeedbackOpen = false
                                feedbackText = ""
                            }
                        } label: {
                            Text(OrbitL10n.tr("Cancel"))
                                .font(OrbitTypography.subhead(.medium))
                                .foregroundStyle(palette.fgSecondary)
                                .padding(.horizontal, 12)
                                .frame(minHeight: OrbitSpacing.hitTarget)
                                .background(palette.subtleFill, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)

                        let canSend = !feedbackText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isModified
                        Button {
                            let cleanFb = feedbackText.trimmingCharacters(in: .whitespacesAndNewlines)
                            onResolvePlan(
                                .requestChanges,
                                isModified ? editedMarkdown : nil,
                                cleanFb.isEmpty ? nil : cleanFb,
                                nil
                            )
                        } label: {
                            Text(OrbitL10n.tr("Send Revisions"))
                                .font(OrbitTypography.subhead(.semibold))
                                .foregroundStyle(canSend ? palette.onPrimary : palette.fgMuted)
                                .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                                .background(canSend ? palette.primary : palette.subtleFill, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .disabled(!canSend)
                    }
                } else {
                    Button {
                        withAnimation(OrbitMotion.ui) {
                            isFeedbackOpen = true
                        }
                    } label: {
                        Text(OrbitL10n.tr("Request Revisions"))
                            .font(OrbitTypography.subhead(.medium))
                            .foregroundStyle(palette.fgPrimary)
                            .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                            .background(palette.subtleFill, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .strokeBorder(palette.hairline, lineWidth: 0.5)
                            )
                    }
                    .buttonStyle(.plain)

                    Button {
                        onResolvePlan(
                            .approveAndBuild,
                            isModified ? editedMarkdown : nil,
                            nil,
                            targetMode
                        )
                    } label: {
                        Text(OrbitL10n.tr("Approve & Build"))
                            .font(OrbitTypography.subhead(.semibold))
                            .foregroundStyle(palette.onPrimary)
                            .frame(maxWidth: .infinity, minHeight: OrbitSpacing.hitTarget)
                            .background(palette.primary, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                    }
                    .buttonStyle(.plain)
                }
            }
            .disabled(isSubmitting)
            .opacity(isSubmitting ? 0.6 : 1.0)
        }
        .padding(OrbitSpacing.md)
        .orbitGlassSurface(
            in: RoundedRectangle(cornerRadius: 18, style: .continuous),
            tint: palette.accentSky.opacity(0.06),
            fallbackFill: palette.elevated,
            fallbackBorder: palette.accentSky.opacity(0.35),
            fallbackShadowOpacity: palette.isDark ? 0.16 : 0.08
        )
        .onAppear {
            editedMarkdown = plan.planMarkdown
            targetMode = store.selectedMode == .plan ? .acceptEdits : OrbitFormatters.normalizePermissionMode(store.selectedMode)
        }
        .onChange(of: plan.requestId) { _ in
            editedMarkdown = plan.planMarkdown
            isEditMode = false
            isFeedbackOpen = false
            feedbackText = ""
        }
    }

    private func segmentChip(title: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 11, weight: selected ? .semibold : .medium))
                .foregroundStyle(selected ? palette.fgPrimary : palette.fgMuted)
                .padding(.horizontal, 8)
                .frame(height: 24)
                .background(selected ? palette.elevated : Color.clear, in: Capsule())
        }
        .buttonStyle(.plain)
    }

    private func modePill(_ mode: FfiSessionMode, label: String) -> some View {
        let selected = OrbitFormatters.normalizePermissionMode(targetMode) == mode
        return Button {
            store.hapticLight()
            targetMode = mode
        } label: {
            Text(label)
                .font(.system(size: 11, weight: selected ? .semibold : .medium))
                .foregroundStyle(selected ? palette.fgPrimary : palette.fgMuted)
                .padding(.horizontal, 8)
                .frame(height: 24)
                .background(selected ? palette.activeRowFill : palette.subtleFill, in: Capsule())
        }
        .buttonStyle(.plain)
    }
}

/// Calm collapsible Session Plan Shelf docked above the composer (matching Desktop `ComposerPlanShelf`).
struct SessionPlanShelfView: View {
    @ObservedObject var store: OrbitIosStore
    @ObservedObject private var stageState: OrbitStageStreamState
    @Environment(\.orbitPalette) private var palette
    @State private var isExpanded: Bool = false

    init(store: OrbitIosStore) {
        self.store = store
        self._stageState = ObservedObject(wrappedValue: store.stageState)
    }

    private var planSteps: [SessionPlanStepView] {
        stageState.agentStage?.insights.planSteps ?? []
    }

    private var completedCount: Int {
        planSteps.filter { $0.status == .completed }.count
    }

    private var activeStep: SessionPlanStepView? {
        planSteps.first(where: { $0.status == .inProgress })
    }

    private var shouldShowShelf: Bool {
        guard !planSteps.isEmpty else { return false }
        let hasPendingDecisions = !(stageState.agentStage?.pendingPermissions.isEmpty ?? true)
            || !(stageState.agentStage?.pendingQuestions.isEmpty ?? true)
        if hasPendingDecisions { return false }
        return completedCount < planSteps.count || store.isActiveAgentTurnBusy
    }

    var body: some View {
        if shouldShowShelf {
            VStack(alignment: .leading, spacing: 0) {
                Button {
                    store.hapticLight()
                    withAnimation(OrbitMotion.ui) {
                        isExpanded.toggle()
                    }
                } label: {
                    HStack(spacing: OrbitSpacing.sm) {
                        if activeStep != nil {
                            OrbitSpinnerRingView(color: palette.fgPrimary, size: 12)
                        } else {
                            Image(systemName: "checklist")
                                .font(OrbitTypography.iconMicro)
                                .foregroundStyle(palette.fgSecondary)
                        }

                        Text(activeStep?.activeForm ?? activeStep?.title ?? OrbitL10n.tr("Session Plan"))
                            .font(OrbitTypography.subhead(.medium))
                            .foregroundStyle(palette.fgPrimary)
                            .lineLimit(1)

                        Spacer(minLength: 4)

                        Text("\(completedCount)/\(planSteps.count)")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.fgMuted)

                        Image(systemName: isExpanded ? "chevron.down" : "chevron.up")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(palette.fgMuted)
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .frame(minHeight: 38)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)

                if isExpanded {
                    OrbitHairline()
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(planSteps, id: \.id) { step in
                            HStack(alignment: .top, spacing: 8) {
                                stepStatusIcon(step.status)
                                    .padding(.top, 2)
                                Text(step.title)
                                    .font(OrbitTypography.meta(step.status == .inProgress ? .semibold : .regular))
                                    .foregroundStyle(step.status == .completed ? palette.fgMuted : palette.fgPrimary)
                                    .strikethrough(step.status == .completed || step.status == .skipped)
                                    .lineLimit(2)
                                Spacer(minLength: 0)
                            }
                        }
                    }
                    .padding(.horizontal, OrbitSpacing.md)
                    .padding(.vertical, OrbitSpacing.sm)
                }
            }
            .orbitGlassSurface(
                in: RoundedRectangle(cornerRadius: 16, style: .continuous),
                fallbackFill: palette.elevated,
                fallbackBorder: palette.hairline
            )
            .padding(.horizontal, OrbitSpacing.lg)
            .padding(.top, OrbitSpacing.xs)
        }
    }

    @ViewBuilder
    private func stepStatusIcon(_ status: SessionPlanStepStatus) -> some View {
        switch status {
        case .completed:
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 12))
                .foregroundStyle(palette.direct)
        case .inProgress:
            OrbitSpinnerRingView(color: palette.fgPrimary, size: 11)
        case .failed:
            Image(systemName: "xmark.circle.fill")
                .font(.system(size: 12))
                .foregroundStyle(palette.danger)
        case .skipped:
            Image(systemName: "minus.circle")
                .font(.system(size: 12))
                .foregroundStyle(palette.fgMuted)
        case .pending:
            Image(systemName: "circle")
                .font(.system(size: 12))
                .foregroundStyle(palette.fgFaint)
        }
    }
}
