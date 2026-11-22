import AVFoundation
import SwiftUI

/// Pairing: scan the Hub's QR code, then check the six digits match on both screens.
struct PairView: View {
    @EnvironmentObject private var core: Core
    var onClose: () -> Void
    @State private var cameraDenied = false

    var body: some View {
        ZStack {
            Theme.bg.ignoresSafeArea()
            VStack(spacing: 22) {
                HStack {
                    Text(title).font(Theme.display(28))
                    Spacer()
                    Button("Close") {
                        if case .verify = core.pairing { core.confirm(false) }
                        core.pairing = nil
                        onClose()
                    }
                    .font(Theme.ui(16, .semibold))
                    .foregroundStyle(Theme.textDim)
                }
                content
                Spacer(minLength: 0)
            }
            .padding(24)
        }
        .foregroundStyle(Theme.text)
    }

    private var title: String {
        switch core.pairing {
        case .verify: return "Check the code"
        case .done: return "Paired"
        default: return "Pair with a computer"
        }
    }

    @ViewBuilder private var content: some View {
        switch core.pairing {
        case .none:
            if cameraDenied {
                Panel {
                    Text("Devti needs the camera to scan the code.").font(Theme.ui(17, .semibold))
                    Text("Allow it in Settings → Devti → Camera. Or scan the code with the Camera app — it opens Devti.")
                        .font(Theme.ui(15)).foregroundStyle(Theme.textDim)
                }
            } else {
                QRScanner { code in
                    if code.hasPrefix("devti://") { core.pair(uri: code) }
                } onDenied: { cameraDenied = true }
                    .aspectRatio(1, contentMode: .fit)
                    .clipShape(RoundedRectangle(cornerRadius: 28, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 28, style: .continuous).strokeBorder(Theme.line, lineWidth: 1))
                Text("Point the camera at the QR code in Devti on your computer.")
                    .font(Theme.ui(15)).foregroundStyle(Theme.textDim).multilineTextAlignment(.center)
            }
        case .connecting:
            ProgressView().tint(Theme.text).padding(40)
            Text("Connecting…").font(Theme.ui(16)).foregroundStyle(Theme.textDim)
        case let .verify(_, peer, sas, confirmed):
            Text("Does \(peer.name) show this code?").font(Theme.ui(18, .semibold))
            Text(sas.replacingOccurrences(of: " ", with: "  "))
                .font(Theme.display(48, .heavy))
                .monospacedDigit()
                .kerning(4)
                .padding(.vertical, 8)
                .accessibilityLabel("Code \(sas.map(String.init).joined(separator: " "))")
            if confirmed {
                Text("Waiting for \(peer.name)…").font(Theme.ui(15)).foregroundStyle(Theme.textDim)
            } else {
                Button("The codes match") { core.confirm(true) }.buttonStyle(PrimaryButtonStyle())
                Button("They're different") { core.confirm(false) }
                    .font(Theme.ui(16, .semibold)).foregroundStyle(Theme.danger)
                Text("Different codes mean the connection didn't go straight between your devices.")
                    .font(Theme.ui(13)).foregroundStyle(Theme.textFaint).multilineTextAlignment(.center)
            }
        case let .done(peer):
            Prism(voices: [Theme.voice(peer.info.id)], size: 140)
            Text("\(peer.info.name) and this iPhone are linked.").font(Theme.ui(18, .semibold)).multilineTextAlignment(.center)
            Button("Done") {
                core.pairing = nil
                onClose()
            }
            .buttonStyle(PrimaryButtonStyle())
        case let .failed(why):
            Panel {
                Text("Pairing didn't finish").font(Theme.ui(17, .semibold))
                Text(why).font(Theme.ui(15)).foregroundStyle(Theme.textDim)
            }
            Button("Try again") { core.pairing = nil }.buttonStyle(PrimaryButtonStyle())
        }
    }
}

/// A camera preview that reports the first QR code it sees.
struct QRScanner: UIViewRepresentable {
    var onCode: (String) -> Void
    var onDenied: () -> Void

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        view.backgroundColor = UIColor(Theme.surface)
        context.coordinator.attach(to: view)
        return view
    }

    func updateUIView(_ uiView: PreviewView, context: Context) {}

    static func dismantleUIView(_ uiView: PreviewView, coordinator: Coordinator) {
        coordinator.stop()
    }

    func makeCoordinator() -> Coordinator { Coordinator(onCode: onCode, onDenied: onDenied) }

    final class PreviewView: UIView {
        override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }
        var preview: AVCaptureVideoPreviewLayer { layer as! AVCaptureVideoPreviewLayer }
    }

    final class Coordinator: NSObject, AVCaptureMetadataOutputObjectsDelegate {
        private let session = AVCaptureSession()
        private let onCode: (String) -> Void
        private let onDenied: () -> Void
        private var reported = false

        init(onCode: @escaping (String) -> Void, onDenied: @escaping () -> Void) {
            self.onCode = onCode
            self.onDenied = onDenied
        }

        func attach(to view: PreviewView) {
            AVCaptureDevice.requestAccess(for: .video) { granted in
                DispatchQueue.main.async {
                    guard granted else { return self.onDenied() }
                    self.configure(view)
                }
            }
        }

        private func configure(_ view: PreviewView) {
            guard let camera = AVCaptureDevice.default(for: .video),
                  let input = try? AVCaptureDeviceInput(device: camera),
                  session.canAddInput(input) else { return }
            session.addInput(input)
            let output = AVCaptureMetadataOutput()
            guard session.canAddOutput(output) else { return }
            session.addOutput(output)
            output.setMetadataObjectsDelegate(self, queue: .main)
            output.metadataObjectTypes = [.qr]
            view.preview.session = session
            view.preview.videoGravity = .resizeAspectFill
            DispatchQueue.global(qos: .userInitiated).async { self.session.startRunning() }
        }

        func stop() {
            DispatchQueue.global(qos: .userInitiated).async { self.session.stopRunning() }
        }

        func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput objects: [AVMetadataObject], from connection: AVCaptureConnection) {
            guard !reported, let code = (objects.first as? AVMetadataMachineReadableCodeObject)?.stringValue else { return }
            reported = true
            stop()
            DispatchQueue.main.async {
                UINotificationFeedbackGenerator().notificationOccurred(.success)
                self.onCode(code)
            }
        }
    }
}
