import SwiftUI

/// The Hub, drawn as Devti's prism: three facets around a bright core. Connected devices
/// light the facets in their voices; it breathes slowly while waiting.
struct Prism: View {
    var voices: [Color]
    var size: CGFloat = 160
    @State private var breathe = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Canvas { ctx, sz in
            let s = min(sz.width, sz.height) / 120
            func p(_ x: Double, _ y: Double) -> CGPoint { CGPoint(x: x * s, y: y * s + 5 * s) }
            let top = p(60, 10), left = p(10, 98), right = p(110, 98), core = p(62, 66)
            let colors = voices.isEmpty ? [Theme.surface3, Theme.surface3, Theme.surface3] :
                (0..<3).map { voices[$0 % voices.count] }
            let facets = [[top, left, core], [left, right, core], [right, top, core]]
            for (i, f) in facets.enumerated() {
                var path = Path()
                path.addLines(f)
                path.closeSubpath()
                ctx.fill(path, with: .linearGradient(
                    Gradient(colors: [colors[i].opacity(0.95), colors[i].opacity(0.3)]),
                    startPoint: f[0], endPoint: f[2]
                ))
            }
            var outline = Path()
            outline.addLines([top, left, right])
            outline.closeSubpath()
            ctx.stroke(outline, with: .color(Theme.text), style: StrokeStyle(lineWidth: 2.2 * s / 1.6, lineJoin: .round))
            for v in [top, left, right] {
                var spoke = Path()
                spoke.move(to: v)
                spoke.addLine(to: core)
                ctx.stroke(spoke, with: .color(Theme.text.opacity(0.45)), lineWidth: 1.4 * s / 1.6)
            }
            let glow = 22 * s
            ctx.fill(Path(ellipseIn: CGRect(x: core.x - glow, y: core.y - glow, width: glow * 2, height: glow * 2)),
                     with: .radialGradient(Gradient(colors: [.white.opacity(0.7), .white.opacity(0)]),
                                           center: core, startRadius: 0, endRadius: glow))
            ctx.fill(Path(ellipseIn: CGRect(x: core.x - 4 * s, y: core.y - 4 * s, width: 8 * s, height: 8 * s)), with: .color(.white))
        }
        .frame(width: size, height: size)
        .scaleEffect(breathe ? 1.03 : 0.98)
        .animation(reduceMotion ? nil : .easeInOut(duration: 3.2).repeatForever(autoreverses: true), value: breathe)
        .onAppear { breathe = true }
        .accessibilityHidden(true)
    }
}
