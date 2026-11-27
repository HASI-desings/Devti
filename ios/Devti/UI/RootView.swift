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
            if let g = core.stolen { StolenOverlay(lock: g) }
        }
        .foregroundStyle(Theme.text)
        .sheet(isPresented: $pairing) { PairView(onClose: { pairing = false }).environmentObject(core) }
        .alert(
            "Let \(core.guardClaim?.name ?? "your computer") protect this iPhone?",
            isPresented: Binding(get: { core.guardClaim != nil }, set: { if !$0 { core.guardClaim = nil } })
        ) {
            Button("Allow") { core.answerGuardClaim(true) }
            Button("Don't allow", role: .cancel) { core.answerGuardClaim(false) }
        } message: {
            Text("It can show your message on this iPhone and see where it is while Devti is open. Apple doesn't let apps lock an iPhone, so also turn on Find My iPhone.")
        }
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

/// The stolen screen on iPhone: covers Devti while it's open. iOS doesn't let it do more.
private struct StolenOverlay: View {
    @EnvironmentObject private var core: Core
    let lock: LocalGuard
    @State private var answer = ""
    @State private var note: String?

    var body: some View {
        VStack(spacing: 16) {
            Spacer()
            Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 54)).foregroundStyle(.red)
            Text(lock.state.message).font(.system(size: 28, weight: .bold)).multilineTextAlignment(.center)
            if !lock.state.contact.isEmpty {
                Text("Owner: \(lock.state.contact)").font(.system(size: 18)).foregroundStyle(.white.opacity(0.85))
            }
            Text("This iPhone is locked by its owner and is sharing its location.")
                .font(.system(size: 14)).foregroundStyle(.white.opacity(0.6)).multilineTextAlignment(.center)
            Spacer().frame(height: 24)
            Text("Owner's unlock · question \(lock.challenge)").font(.system(size: 15, weight: .semibold))
            TextField("Answer", text: $answer)
                .keyboardType(.numberPad)
                .multilineTextAlignment(.center)
                .padding(12)
                .background(RoundedRectangle(cornerRadius: 12).stroke(.white.opacity(0.3)))
                .frame(maxWidth: 280)
            Button("Unlock") {
                switch core.tryStolenCode(answer) {
                case .some(true): note = nil
                case .some(false): note = "That's not the answer. A new question is shown."
                case .none: note = "Please wait before trying again."
                }
                answer = ""
            }
            .buttonStyle(QuietButtonStyle())
            .frame(maxWidth: 280)
            if let note { Text(note).font(.system(size: 13)).foregroundStyle(.red.opacity(0.9)) }
            Spacer()
        }
        .padding(28)
        .foregroundStyle(.white)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.black.ignoresSafeArea())
    }
}
