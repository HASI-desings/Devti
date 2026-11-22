import SwiftUI

struct RootView: View {
    @EnvironmentObject private var core: Core
    @State private var pairing = false

    var body: some View {
        ZStack {
            Theme.bg.ignoresSafeArea()
            if let error = core.error {
                VStack(spacing: 16) {
                    Text("Devti couldn't start").font(Theme.display(28))
                    Text(error).font(Theme.ui(16)).foregroundStyle(Theme.textDim).multilineTextAlignment(.center)
                }
                .padding(32)
            } else if core.peers.isEmpty && core.local != nil {
                Welcome(onPair: { pairing = true })
            } else {
                TabView {
                    HomeView(onPair: { pairing = true }).tabItem { Label("Home", systemImage: "triangle") }
                    RemoteView().tabItem { Label("Remote", systemImage: "hand.point.up.left") }
                    ScreenView().tabItem { Label("Screen", systemImage: "display") }
                    DeckView().tabItem { Label("Deck", systemImage: "square.grid.3x3") }
                }
                .tint(Theme.text)
            }
            if let from = core.ringingFrom { RingOverlay(from: from) }
        }
        .foregroundStyle(Theme.text)
        .sheet(isPresented: $pairing) { PairView(onClose: { pairing = false }).environmentObject(core) }
        .overlay(alignment: .top) { ToastView() }
        .preferredColorScheme(.dark)
    }
}

/// First run: what Devti is, honestly, and one button.
private struct Welcome: View {
    var onPair: () -> Void

    var body: some View {
        VStack(spacing: 28) {
            Spacer()
            Prism(voices: [], size: 170)
            VStack(spacing: 12) {
                Text("All your devices,\none living system.")
                    .font(Theme.display(34))
                    .multilineTextAlignment(.center)
                Text("Pair this iPhone with Devti on your computer. Everything stays on your own devices, encrypted.")
                    .font(Theme.ui(16))
                    .foregroundStyle(Theme.textDim)
                    .multilineTextAlignment(.center)
            }
            Spacer()
            Button("Scan the QR code", action: onPair).buttonStyle(PrimaryButtonStyle())
            Text("On your computer: open Devti and choose Pair.")
                .font(Theme.ui(13))
                .foregroundStyle(Theme.textFaint)
        }
        .padding(28)
    }
}

/// Find my iPhone: full screen while it rings.
private struct RingOverlay: View {
    @EnvironmentObject private var core: Core
    var from: String

    var body: some View {
        ZStack {
            Theme.bgDeep.opacity(0.96).ignoresSafeArea()
            VStack(spacing: 24) {
                Image(systemName: "dot.radiowaves.left.and.right")
                    .font(.system(size: 64, weight: .semibold))
                    .symbolEffectIfAvailable()
                Text("\(from) is looking for this iPhone").font(Theme.display(28)).multilineTextAlignment(.center)
                Button("I found it") { core.stopRinging() }.buttonStyle(PrimaryButtonStyle())
            }
            .padding(32)
        }
        .accessibilityAddTraits(.isModal)
    }
}

private extension View {
    @ViewBuilder func symbolEffectIfAvailable() -> some View {
        if #available(iOS 17.0, *) { symbolEffect(.pulse) } else { self }
    }
}

struct ToastView: View {
    @EnvironmentObject private var core: Core

    var body: some View {
        if let text = core.toast {
            Text(text)
                .font(Theme.ui(15, .medium))
                .padding(.horizontal, 18)
                .padding(.vertical, 12)
                .background(Theme.surface3, in: Capsule())
                .padding(.top, 8)
                .padding(.horizontal, 20)
                .transition(.move(edge: .top).combined(with: .opacity))
                .task(id: text) {
                    try? await Task.sleep(nanoseconds: 3_200_000_000)
                    if core.toast == text { withAnimation { core.toast = nil } }
                }
                .onTapGesture { withAnimation { core.toast = nil } }
                .accessibilityAddTraits(.isStaticText)
        }
    }
}
