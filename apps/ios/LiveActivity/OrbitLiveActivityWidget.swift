import SwiftUI
import WidgetKit
#if canImport(ActivityKit)
import ActivityKit
#endif

#if canImport(ActivityKit)
@available(iOS 16.2, *)
struct OrbitLiveActivityWidget: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: OrbitAgentActivityAttributes.self) { context in
            OrbitLockScreenActivityView(context: context)
                .activityBackgroundTint(Color(red: 0.090, green: 0.094, blue: 0.118))
                .activitySystemActionForegroundColor(Color(red: 0.945, green: 0.949, blue: 0.969))
                .widgetURL(URL(string: context.state.deepLink))
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    HStack(spacing: 6) {
                        statusIndicatorDot(for: context.state, size: 8)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(context.state.agentTitle)
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(.white)
                                .lineLimit(1)
                            Text(context.state.hostName)
                                .font(.system(size: 11, weight: .regular))
                                .foregroundStyle(Color.white.opacity(0.62))
                                .lineLimit(1)
                        }
                    }
                    .padding(.leading, 2)
                }

                DynamicIslandExpandedRegion(.trailing) {
                    statusPill(for: context.state)
                        .padding(.trailing, 2)
                }

                DynamicIslandExpandedRegion(.bottom) {
                    HStack(spacing: 8) {
                        Text(context.state.body.isEmpty ? context.state.statusLabel : context.state.body)
                            .font(.system(size: 12, weight: .regular))
                            .foregroundStyle(Color.white.opacity(0.82))
                            .lineLimit(2)
                            .frame(maxWidth: .infinity, alignment: .leading)

                        if context.state.isRunning {
                            Text(context.attributes.startedAt, style: .timer)
                                .font(.system(size: 11, weight: .medium, design: .monospaced))
                                .monospacedDigit()
                                .foregroundStyle(Color.white.opacity(0.60))
                        }
                    }
                    .padding(.top, 4)
                    .padding(.horizontal, 2)
                }
            } compactLeading: {
                HStack(spacing: 4) {
                    statusIndicatorDot(for: context.state, size: 7)
                    Text(context.state.hostName)
                        .font(.system(size: 11.5, weight: .medium))
                        .foregroundStyle(.white)
                        .lineLimit(1)
                        .frame(maxWidth: 62, alignment: .leading)
                }
            } compactTrailing: {
                if context.state.needsAttention {
                    let count = max(1, context.state.pendingDecisionCount)
                    HStack(spacing: 3) {
                        Image(systemName: "exclamationmark.shield.fill")
                            .font(.system(size: 10, weight: .semibold))
                        Text("\(count)")
                            .font(.system(size: 11, weight: .semibold))
                            .monospacedDigit()
                    }
                    .foregroundStyle(Color(red: 0.961, green: 0.718, blue: 0.286))
                } else if context.state.isError {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Color(red: 0.973, green: 0.443, blue: 0.443))
                } else if context.state.isRunning {
                    Image(systemName: "sparkle")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Color(red: 0.243, green: 0.835, blue: 0.596))
                } else {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Color(red: 0.243, green: 0.835, blue: 0.596))
                }
            } minimal: {
                if context.state.needsAttention {
                    Image(systemName: "exclamationmark.shield.fill")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Color(red: 0.961, green: 0.718, blue: 0.286))
                } else if context.state.isError {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Color(red: 0.973, green: 0.443, blue: 0.443))
                } else {
                    statusIndicatorDot(for: context.state, size: 8)
                }
            }
            .widgetURL(URL(string: context.state.deepLink))
            .keylineTint(accentColor(for: context.state))
        }
    }

    private func accentColor(for state: OrbitAgentActivityAttributes.ContentState) -> Color {
        if state.needsAttention {
            return Color(red: 0.961, green: 0.718, blue: 0.286) // #F5B749
        }
        if state.isError {
            return Color(red: 0.973, green: 0.443, blue: 0.443) // #F87171
        }
        if state.isRunning {
            return Color(red: 0.243, green: 0.835, blue: 0.596) // #3ED598
        }
        return Color(red: 0.514, green: 0.533, blue: 0.612)     // #83889C
    }

    @ViewBuilder
    private func statusIndicatorDot(for state: OrbitAgentActivityAttributes.ContentState, size: CGFloat) -> some View {
        Circle()
            .fill(accentColor(for: state))
            .frame(width: size, height: size)
    }

    @ViewBuilder
    private func statusPill(for state: OrbitAgentActivityAttributes.ContentState) -> some View {
        let tint = accentColor(for: state)
        HStack(spacing: 4) {
            if state.needsAttention {
                Image(systemName: "exclamationmark.shield.fill")
                    .font(.system(size: 10, weight: .semibold))
                Text("\(max(1, state.pendingDecisionCount))")
                    .font(.system(size: 11, weight: .semibold))
                    .monospacedDigit()
            } else {
                Text(state.statusLabel)
                    .font(.system(size: 11, weight: .semibold))
                    .lineLimit(1)
            }
        }
        .foregroundStyle(tint)
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(tint.opacity(0.16), in: Capsule())
    }
}

@available(iOS 16.2, *)
private struct OrbitLockScreenActivityView: View {
    let context: ActivityViewContext<OrbitAgentActivityAttributes>

    private var accentColor: Color {
        if context.state.needsAttention {
            return Color(red: 0.961, green: 0.718, blue: 0.286)
        }
        if context.state.isError {
            return Color(red: 0.973, green: 0.443, blue: 0.443)
        }
        if context.state.isRunning {
            return Color(red: 0.243, green: 0.835, blue: 0.596)
        }
        return Color(red: 0.514, green: 0.533, blue: 0.612)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Circle()
                    .fill(accentColor)
                    .frame(width: 8, height: 8)

                Text(context.state.agentTitle)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Color(red: 0.945, green: 0.949, blue: 0.969))
                    .lineLimit(1)

                Spacer(minLength: 4)

                if context.state.needsAttention {
                    HStack(spacing: 4) {
                        Image(systemName: "exclamationmark.shield.fill")
                            .font(.system(size: 10, weight: .semibold))
                        Text("\(max(1, context.state.pendingDecisionCount)) Decision")
                            .font(.system(size: 11, weight: .semibold))
                            .monospacedDigit()
                    }
                    .foregroundStyle(accentColor)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(accentColor.opacity(0.16), in: Capsule())
                } else {
                    Text(context.state.statusLabel)
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(accentColor)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(accentColor.opacity(0.14), in: Capsule())
                }
            }

            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(context.state.hostName)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(Color(red: 0.698, green: 0.714, blue: 0.780))
                    .lineLimit(1)

                Text("·")
                    .foregroundStyle(Color(red: 0.514, green: 0.533, blue: 0.612))

                Text(context.state.body.isEmpty ? context.attributes.providerLabel : context.state.body)
                    .font(.system(size: 12, weight: .regular))
                    .foregroundStyle(Color(red: 0.698, green: 0.714, blue: 0.780))
                    .lineLimit(1)

                Spacer(minLength: 4)

                if context.state.isRunning {
                    Text(context.attributes.startedAt, style: .timer)
                        .font(.system(size: 11, weight: .medium, design: .monospaced))
                        .monospacedDigit()
                        .foregroundStyle(Color(red: 0.514, green: 0.533, blue: 0.612))
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
    }
}
#endif
