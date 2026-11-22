import SwiftUI

/// A computer's screen on this iPhone or iPad. Tap clicks where you tapped; hold
/// right-clicks. Frames adapt to the network, so a slow link lowers the frame rate rather
/// than adding delay.
struct ScreenView: View {
    @EnvironmentObject private var core: Core
    @State private var watching: String?

    private var pc: PeerSnapshot? { core.computers.first(where: \.online) }

    var body: some View {
        NavigationStack {
            ZStack {
                Theme.bgDeep.ignoresSafeArea()
                if let id = watching, let frame = core.frame {
                    GeometryReader { geo in
                        let fit = fitted(frame, in: geo.size)
                        Image(uiImage: frame.image)
                            .resizable()
                            .interpolation(.medium)
                            .frame(width: fit.width, height: fit.height)
                            .position(x: geo.size.width / 2, y: geo.size.height / 2)
                            .gesture(SpatialTapGesture().onEnded { t in click(id, t.location, fit, geo.size, "left") })
                            .accessibilityLabel("Screen of the computer")
                    }
                    VStack {
                        Spacer()
                        Text("\(frame.fps) fps · \(frame.latencyMs) ms")
                            .font(Theme.ui(12)).foregroundStyle(Theme.textDim)
                            .padding(.horizontal, 10).padding(.vertical, 6)
                            .background(.black.opacity(0.4), in: Capsule())
                            .padding(10)
                    }
                } else if watching != nil {
                    ProgressView("Connecting…").tint(Theme.text)
                } else {
                    VStack(spacing: 18) {
                        Image(systemName: "display").font(.system(size: 52)).foregroundStyle(Theme.textFaint)
                        Text(pc.map { "See \($0.info.name)'s screen here" } ?? "No computer is connected")
                            .font(Theme.display(24)).multilineTextAlignment(.center)
                        if let pc {
                            Button("Show the screen") { start(pc.id) }.buttonStyle(PrimaryButtonStyle())
                        }
                    }
                    .padding(28)
                }
            }
            .navigationTitle("Screen")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if let id = watching {
                    Button("Stop") {
                        core.stopWatching(id)
                        watching = nil
                    }
                }
            }
            .onDisappear {
                if let id = watching { core.stopWatching(id) }
                watching = nil
            }
        }
    }

    private func start(_ id: String) {
        let scale = UIScreen.main.scale
        let bounds = UIScreen.main.bounds
        watching = id
        core.watch(id, maxWidth: min(1920, max(640, Int(max(bounds.width, bounds.height) * scale))))
    }

    private func fitted(_ f: ScreenFrame, in size: CGSize) -> CGSize {
        let ratio = CGFloat(f.width) / CGFloat(max(1, f.height))
        return size.width / size.height > ratio
            ? CGSize(width: size.height * ratio, height: size.height)
            : CGSize(width: size.width, height: size.width / ratio)
    }

    private func click(_ id: String, _ p: CGPoint, _ fit: CGSize, _ area: CGSize, _ button: String) {
        // The tap is in the image's own coordinates; send it as fractions of the screen.
        let x = Double(p.x / fit.width), y = Double(p.y / fit.height)
        core.input(id, [Input.moveTo(x, y), Input.click(button)])
    }
}
