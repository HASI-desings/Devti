import SwiftUI
import UniformTypeIdentifiers

/// Home: the computer this iPhone belongs to, quick actions, transfers and settings.
struct HomeView: View {
    @EnvironmentObject private var core: Core
    var onPair: () -> Void
    @State private var importing = false
    @State private var browsing = false
    @State private var forget: PeerSnapshot?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    hubCard
                    if let hub = core.hub { actions(hub) }
                    transfers
                    devices
                    settings
                    honesty
                }
                .padding(20)
            }
            .background(Theme.bg)
            .navigationTitle("Devti")
            .toolbarBackground(Theme.bg, for: .navigationBar)
            .fileImporter(isPresented: $importing, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
                if case let .success(urls) = result, let hub = core.hub { core.send(urls, to: hub.id) }
            }
            .sheet(isPresented: $browsing) { BrowseView().environmentObject(core) }
            .confirmationDialog(
                "Remove \(forget?.info.name ?? "")?",
                isPresented: Binding(get: { forget != nil }, set: { if !$0 { forget = nil } }),
                titleVisibility: .visible
            ) {
                Button("Remove", role: .destructive) { if let f = forget { core.forget(f.id) } }
            } message: {
                Text("It will be disconnected and forgotten on both devices. You can pair again later.")
            }
        }
    }

    // MARK: Sections

    private var hubCard: some View {
        let hub = core.hub
        let myVoice = Theme.voice(core.local?.info.id ?? "")
        return VStack(spacing: 14) {
            Prism(voices: hub?.online == true ? [myVoice] : [], size: 150)
            Text(hub?.info.name ?? "No computer yet").font(Theme.display(30)).multilineTextAlignment(.center)
            Text(status(hub)).font(Theme.ui(15)).foregroundStyle(Theme.textDim).multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
    }

    private func status(_ hub: PeerSnapshot?) -> String {
        guard let hub else { return "Pair with your computer to join its ecosystem." }
        if core.local?.paused == true { return "Devti is paused on this iPhone." }
        guard hub.online, let c = hub.connection else {
            return "Waiting for \(hub.info.name). It connects while Devti is open here — at home or anywhere else."
        }
        let path: String
        switch c.kind {
        case "vpn": path = "through your private network"
        case "internet": path = "directly over the internet"
        case "relay": path = "through an encrypted relay"
        default: path = "on your network"
        }
        return "Connected \(path), \(c.rttMs) ms away"
    }

    private func actions(_ hub: PeerSnapshot) -> some View {
        VStack(spacing: 10) {
            HStack(spacing: 10) {
                Button { core.ring(hub.id, true) } label: { Label("Find it", systemImage: "dot.radiowaves.left.and.right") }
                    .buttonStyle(QuietButtonStyle())
                    .disabled(!hub.online)
                Button { importing = true } label: { Label("Send files", systemImage: "arrow.up.doc") }
                    .buttonStyle(QuietButtonStyle())
            }
            HStack(spacing: 10) {
                // The system paste button reads the clipboard without iOS's "Allow Paste" prompt.
                PasteButton(payloadType: String.self) { strings in
                    if let text = strings.first { Task { @MainActor in core.sendClipboard(text) } }
                }
                .labelStyle(.titleAndIcon)
                .buttonBorderShape(.roundedRectangle(radius: 16))
                .tint(Theme.surface2)
                .frame(maxWidth: .infinity)
                Button { browsing = true } label: { Label("Browse", systemImage: "folder") }
                    .buttonStyle(QuietButtonStyle())
                    .disabled(!hub.online)
            }
        }
    }

    @ViewBuilder private var transfers: some View {
        if !core.transfers.isEmpty {
            Text("Transfers").font(Theme.display(22))
            Panel {
                ForEach(core.transfers.prefix(6)) { t in
                    VStack(alignment: .leading, spacing: 6) {
                        HStack {
                            Image(systemName: t.incoming ? "arrow.down" : "arrow.up").foregroundStyle(Theme.textDim)
                            Text(t.name).font(Theme.ui(15, .semibold)).lineLimit(1)
                            Spacer()
                            Text(label(t)).font(Theme.ui(13)).foregroundStyle(Theme.textDim)
                        }
                        if !t.finished {
                            ProgressView(value: t.fraction).tint(Theme.voice(t.deviceId))
                        }
                    }
                    .contextMenu {
                        if !t.finished { Button("Cancel", role: .destructive) { core.cancelTransfer(t.id) } }
                    }
                }
                if core.transfers.contains(where: { $0.incoming && $0.state == "done" }) {
                    Text("Received files are in Files → On My iPhone → Devti.").font(Theme.ui(13)).foregroundStyle(Theme.textFaint)
                }
            }
        }
    }

    private func label(_ t: TransferRecord) -> String {
        switch t.state {
        case "done": return t.incoming ? "Received" : "Sent"
        case "failed": return "Failed"
        case "cancelled": return "Cancelled"
        case "queued": return "Waiting"
        default: return "\(Int(t.fraction * 100)) %"
        }
    }

    private var devices: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Paired").font(Theme.display(22))
            ForEach(core.peers) { p in
                Panel {
                    HStack(spacing: 12) {
                        VoiceDot(color: Theme.voice(p.id), online: p.online)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(p.info.name).font(Theme.ui(17, .semibold))
                            Text(p.online ? "Connected" : "Offline").font(Theme.ui(13)).foregroundStyle(Theme.textDim)
                        }
                        Spacer()
                        if let b = p.status.battery {
                            Label("\(b.percent)%", systemImage: b.charging ? "battery.100.bolt" : "battery.75")
                                .font(Theme.ui(14))
                                .foregroundStyle(Theme.textDim)
                        }
                    }
                    Button("Remove", role: .destructive) { forget = p }
                        .font(Theme.ui(14, .semibold))
                        .foregroundStyle(Theme.textDim)
                }
            }
            Button("Pair another computer", action: onPair).buttonStyle(QuietButtonStyle())
        }
    }

    private var settings: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Everywhere").font(Theme.display(22))
            Panel {
                Toggle(isOn: Binding(get: { core.dnd }, set: { core.setDnd($0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Do Not Disturb, everywhere").font(Theme.ui(16, .semibold))
                        Text("Silences your other devices together.").font(Theme.ui(13)).foregroundStyle(Theme.textDim)
                    }
                }
                Divider().overlay(Theme.line)
                Toggle(isOn: Binding(get: { core.anywhere?.enabled != false }, set: { core.setAnywhere($0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Connect from anywhere").font(Theme.ui(16, .semibold))
                        Text("On mobile data or any Wi‑Fi, this iPhone and your computer find each other — directly when they can, through an encrypted relay when they can't.")
                            .font(Theme.ui(13)).foregroundStyle(Theme.textDim)
                    }
                }
                Divider().overlay(Theme.line)
                Toggle(isOn: Binding(get: { core.reach?.enabled == true }, set: { core.setReach($0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Also share my VPN addresses").font(Theme.ui(16, .semibold))
                        Text("Optional: if you use Tailscale or WireGuard, your computer can use it too.")
                            .font(Theme.ui(13)).foregroundStyle(Theme.textDim)
                    }
                }
                Divider().overlay(Theme.line)
                Toggle(isOn: Binding(get: { core.local?.paused == true }, set: { core.setPaused($0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Pause Devti").font(Theme.ui(16, .semibold))
                        Text("Disconnects everything until you turn it back on.").font(Theme.ui(13)).foregroundStyle(Theme.textDim)
                    }
                }
            }
            .tint(Theme.ok)
        }
    }

    private var honesty: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("What works on iPhone").font(Theme.display(22))
            Panel {
                ForEach(Self.works, id: \.self) { line in
                    Label(line, systemImage: "checkmark").font(Theme.ui(14)).foregroundStyle(Theme.textDim)
                }
                Text("Apple doesn't let any app read texts or calls, mirror other apps' notifications, share the screen without a recording extension, or stay connected in the background. Devti connects while it's open and catches up when you come back.")
                    .font(Theme.ui(13))
                    .foregroundStyle(Theme.textFaint)
            }
        }
    }

    static let works = [
        "Pairing by QR code, verified on both screens",
        "Battery level on your computer",
        "Files both ways (received files are in the Files app)",
        "Paste to your computer; its copies land on this iPhone",
        "Touchpad, keyboard, media and power for your computer",
        "Your computer's shortcuts deck and commands",
        "Your computer's screen on this iPhone or iPad",
        "Browse and download your computer's shared folders",
        "Find this iPhone (while Devti is open) and find your computer",
        "Do Not Disturb everywhere, and connecting from anywhere",
    ]
}
