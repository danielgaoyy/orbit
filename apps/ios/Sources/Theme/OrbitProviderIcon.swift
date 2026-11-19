import CoreGraphics
import SwiftUI

/// Native vector Agent/Provider icons aligned 1:1 with Orbit Desktop (`provider-icons.tsx`).
public struct OrbitProviderIcon: View {
    public let provider: FfiProviderKind
    public var size: CGFloat

    public init(provider: FfiProviderKind, size: CGFloat = 16) {
        self.provider = provider
        self.size = size
    }

    public var body: some View {
        Canvas { context, canvasSize in
            let scale = min(canvasSize.width, canvasSize.height) / 24.0
            let dx = (canvasSize.width - 24.0 * scale) * 0.5
            let dy = (canvasSize.height - 24.0 * scale) * 0.5

            context.translateBy(x: dx, y: dy)
            context.scaleBy(x: scale, y: scale)

            switch provider {
            case .codex:
                let path = Path(Self.codexCGPath)
                context.fill(
                    path,
                    with: .linearGradient(
                        Gradient(stops: [
                            .init(color: Color(hex: "#9E94FF"), location: 0.0),
                            .init(color: Color(hex: "#6B8CFF"), location: 0.5),
                            .init(color: Color(hex: "#3B43FF"), location: 1.0)
                        ]),
                        startPoint: CGPoint(x: 12, y: 0),
                        endPoint: CGPoint(x: 12, y: 24)
                    ),
                    style: FillStyle(eoFill: true)
                )

            case .claudeCode:
                let path = Path(Self.claudeCGPath)
                context.fill(
                    path,
                    with: .color(Color(hex: "#D97757")),
                    style: FillStyle(eoFill: false)
                )

            case .antigravity:
                let clipPath = Path(Self.antigravityCGPath)
                context.drawLayer { layer in
                    layer.clip(to: clipPath)
                    let fullRect = Path(CGRect(x: 0, y: 0, width: 24, height: 24))
                    layer.fill(fullRect, with: .color(Color(hex: "#3186FF")))
                    layer.fill(
                        fullRect,
                        with: .radialGradient(
                            Gradient(stops: [
                                .init(color: Color(hex: "#00B95C"), location: 0.0),
                                .init(color: Color(hex: "#00B95C").opacity(0.82), location: 0.58),
                                .init(color: Color(hex: "#00B95C").opacity(0.0), location: 1.0)
                            ]),
                            center: CGPoint(x: 2, y: 11),
                            startRadius: 0,
                            endRadius: 14
                        )
                    )
                    layer.fill(
                        fullRect,
                        with: .radialGradient(
                            Gradient(stops: [
                                .init(color: Color(hex: "#FC413D"), location: 0.0),
                                .init(color: Color(hex: "#FC413D").opacity(0.78), location: 0.55),
                                .init(color: Color(hex: "#FC413D").opacity(0.0), location: 1.0)
                            ]),
                            center: CGPoint(x: 20, y: 6),
                            startRadius: 0,
                            endRadius: 13
                        )
                    )
                    layer.fill(
                        fullRect,
                        with: .radialGradient(
                            Gradient(stops: [
                                .init(color: Color(hex: "#FFE432"), location: 0.0),
                                .init(color: Color(hex: "#FBBC04").opacity(0.90), location: 0.55),
                                .init(color: Color(hex: "#FBBC04").opacity(0.0), location: 1.0)
                            ]),
                            center: CGPoint(x: 10.5, y: 2),
                            startRadius: 0,
                            endRadius: 10.5
                        )
                    )
                }
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }

    // MARK: - Cached Static Vector Paths (24×24 ViewBox, exact Desktop SVGs)

    private static let codexCGPath: CGPath = parseSVGPath(
        "M8.086.457a6.105 6.105 0 0 1 3.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 0 0 .107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 0 1-.18 1.631.167.167 0 0 0 .04.155 5.982 5.982 0 0 1 1.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 0 1-2.934 1.851.162.162 0 0 0-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 0 0-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 0 1-2.595-.622 6.058 6.058 0 0 1-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 0 1-.495-1.283 6.11 6.11 0 0 1-.017-3.064.166.166 0 0 0 .008-.074.115.115 0 0 0-.037-.064 5.958 5.958 0 0 1-1.38-2.202 5.196 5.196 0 0 1-.333-1.589 6.915 6.915 0 0 1 .188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 0 0 .087-.087A6.016 6.016 0 0 1 5.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 0 0-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 0 0 1.46.864l1.94-3.272a.849.849 0 0 0 .007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 0 0 0 1.695h4.848a.849.849 0 0 0 0-1.696h-4.848z"
    )

    private static let claudeCGPath: CGPath = parseSVGPath(
        "M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 0 1-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z"
    )

    private static let antigravityCGPath: CGPath = parseSVGPath(
        "M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z"
    )

    // MARK: - Minimal W3C SVG Path Parser (M/L/H/V/C/A/Z)

    private static func parseSVGPath(_ d: String) -> CGPath {
        let path = CGMutablePath()
        let chars = Array(d.utf8)
        let count = chars.count
        var idx = 0

        func skipSeparators() {
            while idx < count {
                let c = chars[idx]
                if c == 32 || c == 9 || c == 10 || c == 13 || c == 44 { // whitespace or ','
                    idx += 1
                } else {
                    break
                }
            }
        }

        func isCommandByte(_ c: UInt8) -> Bool {
            (c >= 65 && c <= 90) || (c >= 97 && c <= 122)
        }

        func hasMoreNumbers() -> Bool {
            skipSeparators()
            guard idx < count else { return false }
            let c = chars[idx]
            return (c >= 48 && c <= 57) || c == 43 || c == 45 || c == 46
        }

        func readNumber() -> CGFloat {
            skipSeparators()
            let start = idx
            if idx < count && (chars[idx] == 43 || chars[idx] == 45) {
                idx += 1
            }
            while idx < count && chars[idx] >= 48 && chars[idx] <= 57 {
                idx += 1
            }
            if idx < count && chars[idx] == 46 {
                idx += 1
                while idx < count && chars[idx] >= 48 && chars[idx] <= 57 {
                    idx += 1
                }
            }
            if idx < count && (chars[idx] == 101 || chars[idx] == 69) {
                idx += 1
                if idx < count && (chars[idx] == 43 || chars[idx] == 45) {
                    idx += 1
                }
                while idx < count && chars[idx] >= 48 && chars[idx] <= 57 {
                    idx += 1
                }
            }
            let slice = String(decoding: chars[start..<idx], as: UTF8.self)
            return CGFloat(Double(slice) ?? 0)
        }

        func readFlag() -> Bool {
            skipSeparators()
            guard idx < count else { return false }
            let c = chars[idx]
            idx += 1
            return c == 49 // '1'
        }

        var current = CGPoint.zero
        var subpathStart = CGPoint.zero
        var cmd: UInt8 = 0

        while idx < count {
            skipSeparators()
            guard idx < count else { break }
            if isCommandByte(chars[idx]) {
                cmd = chars[idx]
                idx += 1
            }

            switch cmd {
            case 77, 109: // M / m
                let rel = (cmd == 109)
                let x = readNumber()
                let y = readNumber()
                let pt = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                path.move(to: pt)
                current = pt
                subpathStart = pt
                // Subsequent coordinate pairs are implicit L / l
                cmd = rel ? 108 : 76

            case 76, 108: // L / l
                let rel = (cmd == 108)
                repeat {
                    let x = readNumber()
                    let y = readNumber()
                    let pt = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                    path.addLine(to: pt)
                    current = pt
                } while hasMoreNumbers()

            case 72, 104: // H / h
                let rel = (cmd == 104)
                repeat {
                    let x = readNumber()
                    let pt = CGPoint(x: rel ? current.x + x : x, y: current.y)
                    path.addLine(to: pt)
                    current = pt
                } while hasMoreNumbers()

            case 86, 118: // V / v
                let rel = (cmd == 118)
                repeat {
                    let y = readNumber()
                    let pt = CGPoint(x: current.x, y: rel ? current.y + y : y)
                    path.addLine(to: pt)
                    current = pt
                } while hasMoreNumbers()

            case 67, 99: // C / c
                let rel = (cmd == 99)
                repeat {
                    let x1 = readNumber(), y1 = readNumber()
                    let x2 = readNumber(), y2 = readNumber()
                    let x = readNumber(), y = readNumber()
                    let c1 = rel ? CGPoint(x: current.x + x1, y: current.y + y1) : CGPoint(x: x1, y: y1)
                    let c2 = rel ? CGPoint(x: current.x + x2, y: current.y + y2) : CGPoint(x: x2, y: y2)
                    let end = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                    path.addCurve(to: end, control1: c1, control2: c2)
                    current = end
                } while hasMoreNumbers()

            case 65, 97: // A / a
                let rel = (cmd == 97)
                repeat {
                    let rx = readNumber()
                    let ry = readNumber()
                    let xAxisRotation = readNumber()
                    let largeArc = readFlag()
                    let sweep = readFlag()
                    let x = readNumber()
                    let y = readNumber()
                    let end = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                    addSVGArc(
                        to: path,
                        from: current,
                        to: end,
                        rx: rx,
                        ry: ry,
                        angleDegrees: xAxisRotation,
                        largeArc: largeArc,
                        sweep: sweep
                    )
                    current = end
                } while hasMoreNumbers()

            case 90, 122: // Z / z
                path.closeSubpath()
                current = subpathStart

            default:
                idx += 1
            }
        }

        return path
    }

    /// Standard W3C SVG 1.1 Implementation Notes (F.6.5) elliptical arc to cubic Bézier segments.
    private static func addSVGArc(
        to path: CGMutablePath,
        from p0: CGPoint,
        to p1: CGPoint,
        rx rawRx: CGFloat,
        ry rawRy: CGFloat,
        angleDegrees: CGFloat,
        largeArc: Bool,
        sweep: Bool
    ) {
        if p0 == p1 { return }
        var rx = abs(Double(rawRx))
        var ry = abs(Double(rawRy))
        if rx < 1e-9 || ry < 1e-9 {
            path.addLine(to: p1)
            return
        }

        let phi = Double(angleDegrees) * .pi / 180.0
        let cosPhi = cos(phi)
        let sinPhi = sin(phi)

        let dx2 = Double(p0.x - p1.x) * 0.5
        let dy2 = Double(p0.y - p1.y) * 0.5
        let x1p = cosPhi * dx2 + sinPhi * dy2
        let y1p = -sinPhi * dx2 + cosPhi * dy2

        var rx2 = rx * rx
        var ry2 = ry * ry
        let x1p2 = x1p * x1p
        let y1p2 = y1p * y1p

        let lambda = x1p2 / rx2 + y1p2 / ry2
        if lambda > 1.0 {
            let root = sqrt(lambda)
            rx *= root
            ry *= root
            rx2 = rx * rx
            ry2 = ry * ry
        }

        let sign: Double = (largeArc == sweep) ? -1.0 : 1.0
        let num = max(0.0, rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2)
        let den = rx2 * y1p2 + ry2 * x1p2
        let coef = sign * sqrt(den > 1e-12 ? num / den : 0.0)
        let cxp = coef * ((rx * y1p) / ry)
        let cyp = coef * (-(ry * x1p) / rx)

        let cx = cosPhi * cxp - sinPhi * cyp + Double(p0.x + p1.x) * 0.5
        let cy = sinPhi * cxp + cosPhi * cyp + Double(p0.y + p1.y) * 0.5

        func vectorAngle(ux: Double, uy: Double, vx: Double, vy: Double) -> Double {
            let dot = ux * vx + uy * vy
            let mag = sqrt((ux * ux + uy * uy) * (vx * vx + vy * vy))
            let ratio = max(-1.0, min(1.0, mag > 1e-12 ? dot / mag : 1.0))
            let ang = acos(ratio)
            return (ux * vy - uy * vx) < 0 ? -ang : ang
        }

        let ux = (x1p - cxp) / rx
        let uy = (y1p - cyp) / ry
        let vx = (-x1p - cxp) / rx
        let vy = (-y1p - cyp) / ry

        let theta1 = vectorAngle(ux: 1, uy: 0, vx: ux, vy: uy)
        var dTheta = vectorAngle(ux: ux, uy: uy, vx: vx, vy: vy)
        if !sweep && dTheta > 0 {
            dTheta -= 2.0 * .pi
        } else if sweep && dTheta < 0 {
            dTheta += 2.0 * .pi
        }

        let segments = max(1, Int(ceil(abs(dTheta) / (.pi * 0.5))))
        let delta = dTheta / Double(segments)
        let t = (4.0 / 3.0) * tan(delta * 0.25)

        var angle = theta1
        for _ in 0..<segments {
            let nextAngle = angle + delta
            let cosA = cos(angle), sinA = sin(angle)
            let cosB = cos(nextAngle), sinB = sin(nextAngle)

            let e1x = cosA - t * sinA
            let e1y = sinA + t * cosA
            let e2x = cosB + t * sinB
            let e2y = sinB - t * cosB

            func mapPoint(_ ex: Double, _ ey: Double) -> CGPoint {
                let x = cosPhi * (rx * ex) - sinPhi * (ry * ey) + cx
                let y = sinPhi * (rx * ex) + cosPhi * (ry * ey) + cy
                return CGPoint(x: x, y: y)
            }

            path.addCurve(
                to: mapPoint(cosB, sinB),
                control1: mapPoint(e1x, e1y),
                control2: mapPoint(e2x, e2y)
            )
            angle = nextAngle
        }
    }
}
