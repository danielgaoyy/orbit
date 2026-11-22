import SwiftUI
import UIKit
import WebKit

private struct OrbitDiagramRendererKey: EnvironmentKey {
    static let defaultValue: (@Sendable (FfiDiagramKind, String, Bool) async -> FfiRenderDiagramView?)? = nil
}

extension EnvironmentValues {
    var orbitDiagramRenderer: (@Sendable (FfiDiagramKind, String, Bool) async -> FfiRenderDiagramView?)? {
        get { self[OrbitDiagramRendererKey.self] }
        set { self[OrbitDiagramRendererKey.self] = newValue }
    }
}

private struct OrbitSvgWebCanvas: UIViewRepresentable {
    let svg: String
    let isDark: Bool
    let allowZoomScroll: Bool

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        let web = WKWebView(frame: .zero, configuration: config)
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.backgroundColor = .clear
        web.scrollView.isScrollEnabled = allowZoomScroll
        web.scrollView.bouncesZoom = allowZoomScroll
        web.scrollView.minimumZoomScale = 1.0
        web.scrollView.maximumZoomScale = allowZoomScroll ? 4.5 : 1.0
        web.scrollView.showsVerticalScrollIndicator = false
        web.scrollView.showsHorizontalScrollIndicator = allowZoomScroll
        syncHtmlIfNeeded(web: web, coordinator: context.coordinator)
        return web
    }

    func updateUIView(_ uiView: WKWebView, context: Context) {
        uiView.scrollView.isScrollEnabled = allowZoomScroll
        uiView.scrollView.bouncesZoom = allowZoomScroll
        uiView.scrollView.maximumZoomScale = allowZoomScroll ? 4.5 : 1.0
        syncHtmlIfNeeded(web: uiView, coordinator: context.coordinator)
    }

    static func dismantleUIView(_ uiView: WKWebView, coordinator: Coordinator) {
        uiView.stopLoading()
        uiView.loadHTMLString("", baseURL: nil)
    }

    private func syncHtmlIfNeeded(web: WKWebView, coordinator: Coordinator) {
        guard coordinator.lastSvg != svg || coordinator.lastIsDark != isDark || coordinator.lastAllowZoom != allowZoomScroll else {
            return
        }
        coordinator.lastSvg = svg
        coordinator.lastIsDark = isDark
        coordinator.lastAllowZoom = allowZoomScroll

        let fgHex = isDark ? "#F4F4F5" : "#18181B"
        let viewportMeta = allowZoomScroll
            ? "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0, maximum-scale=5.0, user-scalable=yes\">"
            : "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no\">"
        let html = """
        <!doctype html>
        <html>
        <head>
        <meta charset="utf-8">
        \(viewportMeta)
        <style>
          html, body {
            margin: 0;
            padding: 0;
            width: 100%;
            height: 100%;
            background: transparent;
            color: \(fgHex);
            overflow: \(allowZoomScroll ? "auto" : "hidden");
            -webkit-text-size-adjust: 100%;
            font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif;
          }
          .wrap {
            width: 100%;
            min-height: 100%;
            display: flex;
            align-items: center;
            justify-content: center;
            box-sizing: border-box;
            padding: \(allowZoomScroll ? "16px" : "8px");
          }
          svg {
            display: block;
            max-width: 100%;
            max-height: 100%;
            width: auto;
            height: auto;
          }
        </style>
        </head>
        <body><div class="wrap">\(svg)</div></body>
        </html>
        """
        web.loadHTMLString(html, baseURL: nil)
    }

    final class Coordinator {
        var lastSvg: String?
        var lastIsDark: Bool?
        var lastAllowZoom: Bool?
    }
}

struct DiagramBlockView: View {
    let kind: FfiDiagramKind
    let language: String
    let rawCode: String
    let lines: [FfiHighlightedLine]
    var isStreaming: Bool = false

    @Environment(\.orbitPalette) private var palette
    @Environment(\.orbitDiagramRenderer) private var diagramRenderer

    @State private var rendered: FfiRenderDiagramView?
    @State private var isLoading: Bool = true
    @State private var showCode: Bool = false
    @State private var showFullscreen: Bool = false
    @State private var copied: Bool = false

    private var kindTitle: String {
        switch kind {
        case .mermaid: return "Mermaid"
        case .graphviz: return "Graphviz"
        case .svg: return "SVG"
        }
    }

    private var headerTitle: String {
        if let label = rendered?.diagramTypeLabel, !label.isEmpty, label != kindTitle {
            return "\(kindTitle) · \(label)"
        }
        return kindTitle
    }

    private var hasValidSvg: Bool {
        guard let rendered else { return false }
        return rendered.error == nil && rendered.svg != nil
    }

    var body: some View {
        let taskKey = "\(kind)::\(palette.isDark ? "dark" : "light")::\(rawCode)"
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: OrbitSpacing.xs) {
                HStack(spacing: 6) {
                    Image(systemName: iconName)
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(palette.fgMuted)

                    Text(headerTitle)
                        .font(OrbitTypography.code(.medium))
                        .foregroundStyle(palette.fgMuted)
                        .lineLimit(1)

                    if let err = rendered?.error, !err.isEmpty {
                        Text("Code view")
                            .font(OrbitTypography.meta(.medium))
                            .foregroundStyle(palette.warning)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(Capsule().fill(palette.warning.opacity(0.14)))
                    }
                }

                Spacer()

                if hasValidSvg {
                    HStack(spacing: 2) {
                        modeChip(title: "Diagram", active: !showCode) {
                            showCode = false
                        }
                        modeChip(title: "Code", active: showCode) {
                            showCode = true
                        }
                    }
                    .padding(2)
                    .background(
                        RoundedRectangle(cornerRadius: 7, style: .continuous)
                            .fill(palette.subtleFill)
                    )

                    Button {
                        UIImpactFeedbackGenerator(style: .light).impactOccurred()
                        showFullscreen = true
                    } label: {
                        Image(systemName: "arrow.up.left.and.arrow.down.right")
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(palette.fgMuted)
                            .frame(width: 30, height: 30)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Expand Diagram")
                }

                Button {
                    copySource()
                } label: {
                    Image(systemName: copied ? "checkmark" : "square.on.square")
                        .font(.system(size: 12.5, weight: .medium))
                        .foregroundStyle(copied ? palette.direct : palette.fgMuted)
                        .orbitSymbolReplaceTransition()
                        .frame(width: 32, height: 32)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(copied ? "Copied Diagram Source" : "Copy Diagram Source")
            }
            .padding(.leading, OrbitSpacing.md)
            .padding(.trailing, OrbitSpacing.xs)
            .frame(height: 36)
            .background(palette.isDark ? Color.white.opacity(0.025) : Color.black.opacity(0.022))

            OrbitHairline()

            if isStreaming {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 6) {
                        ProgressView()
                            .controlSize(.mini)
                        Text("Streaming \(kindTitle) source…")
                            .font(OrbitTypography.meta(.regular))
                            .foregroundStyle(palette.fgMuted)
                    }
                    rawDiagramSourceBody
                }
                .padding(OrbitSpacing.sm)
            } else if !showCode, let rendered, rendered.error == nil, let svg = rendered.svg {
                let ratio = max(0.35, min(3.0, CGFloat(rendered.width) / max(1.0, CGFloat(rendered.height))))
                let maxInlineHeight = min(max(CGFloat(rendered.height) * 1.15, 140), 420)
                OrbitSvgWebCanvas(svg: svg, isDark: palette.isDark, allowZoomScroll: false)
                    .aspectRatio(ratio, contentMode: .fit)
                    .frame(maxWidth: .infinity, maxHeight: maxInlineHeight)
                    .padding(OrbitSpacing.xs)
                    .contentShape(Rectangle())
                    .onTapGesture {
                        showFullscreen = true
                    }
            } else if isLoading && !showCode {
                HStack(spacing: OrbitSpacing.sm) {
                    ProgressView()
                        .controlSize(.small)
                    Text("Rendering \(kindTitle)…")
                        .font(OrbitTypography.meta(.regular))
                        .foregroundStyle(palette.fgMuted)
                }
                .frame(maxWidth: .infinity, minHeight: 96)
            } else {
                rawDiagramSourceBody
                    .padding(OrbitSpacing.sm)
            }
        }
        .background(palette.code)
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(palette.hairline, lineWidth: 0.5)
        )
        .task(id: "\(taskKey)::\(isStreaming)") {
            guard !isStreaming else {
                isLoading = false
                return
            }
            isLoading = true
            let result = await diagramRenderer?(kind, rawCode, palette.isDark)
            guard !Task.isCancelled else { return }
            rendered = result
            isLoading = false
        }
        .fullScreenCover(isPresented: $showFullscreen) {
            if let svg = rendered?.svg {
                NavigationStack {
                    OrbitSvgWebCanvas(svg: svg, isDark: palette.isDark, allowZoomScroll: true)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(palette.canvas.ignoresSafeArea())
                        .navigationTitle(headerTitle)
                        .navigationBarTitleDisplayMode(.inline)
                        .toolbar {
                            ToolbarItem(placement: .topBarLeading) {
                                Button("Done") {
                                    showFullscreen = false
                                }
                            }
                            ToolbarItem(placement: .topBarTrailing) {
                                Button {
                                    copySource()
                                } label: {
                                    Image(systemName: copied ? "checkmark" : "square.on.square")
                                }
                            }
                        }
                }
            }
        }
    }

    private var rawDiagramSourceBody: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Text(rawCode)
                .font(OrbitTypography.code(.regular))
                .foregroundStyle(palette.fgSecondary)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var iconName: String {
        switch kind {
        case .mermaid: return "point.3.connected.trianglepath.dotted"
        case .graphviz: return "arrow.triangle.branch"
        case .svg: return "photo"
        }
    }

    @ViewBuilder
    private func modeChip(title: String, active: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(title)
                .font(OrbitTypography.meta(active ? .semibold : .medium))
                .foregroundStyle(active ? palette.fgPrimary : palette.fgMuted)
                .padding(.horizontal, 7)
                .padding(.vertical, 3)
                .background(
                    RoundedRectangle(cornerRadius: 5, style: .continuous)
                        .fill(active ? palette.elevated : Color.clear)
                )
        }
        .buttonStyle(.plain)
    }

    private func copySource() {
        UIPasteboard.general.string = rawCode
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        withAnimation(.easeOut(duration: 0.15)) {
            copied = true
        }
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 1_500_000_000)
            withAnimation(.easeIn(duration: 0.2)) {
                copied = false
            }
        }
    }
}
