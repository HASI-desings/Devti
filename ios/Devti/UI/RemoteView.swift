import SwiftUI

/// The iPhone as a computer's touchpad, keyboard and media remote.
struct RemoteView: View {
    @EnvironmentObject private var core: Core
    @State private var selected: String?
    @State private var typed = ""
    @State private var confirmPower: String?
    @FocusState private var keyboard: Bool

    private var target: PeerSnapshot? {
        core.computers.first { $0.id == selected } ?? core.computers.first(where: \.online) ?? core.computers.first
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 14) {
                if let pc = target {
                    if core.computers.count > 1 { picker }
                    if pc.online {
                        Touchpad { core.input(pc.id, $0) }
                        keys(pc.id)
                        media(pc.id)
                    } else {
                        Spacer()
                        Text("\(pc.info.name) is offline.").font(Theme.ui(16)).foregroundStyle(Theme.textDim)
                        Spacer()
                    }
                } else {
                    Spacer()
                    Text("Pair a computer to use this iPhone as its remote.").font(Theme.ui(16)).foregroundStyle(Theme.textDim)
                    Spacer()
                }
            }
            .padding(16)
            .background(Theme.bg)
            .navigationTitle(target?.info.name ?? "Remote")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if let pc = target, pc.online {
                    Menu {
                        Button("Lock") { core.system(pc.id, "lock") }
                        Button("Sleep") { confirmPower = "sleep" }
                        Button("Restart") { confirmPower = "restart" }
                        Button("Shut down", role: .destructive) { confirmPower = "shutdown" }
                    } label: { Image(systemName: "power") }
                        .accessibilityLabel("Power")
                }
            }
            .confirmationDialog("Are you sure?", isPresented: Binding(get: { confirmPower != nil }, set: { if !$0 { confirmPower = nil } })) {
                if let pc = target, let action = confirmPower {
                    Button(action.capitalized, role: .destructive) { core.system(pc.id, action) }
                }
            }
        }
    }

    private var picker: some View {
        Picker("Computer", selection: Binding(get: { target?.id ?? "" }, set: { selected = $0 })) {
            ForEach(core.computers) { Text($0.info.name).tag($0.id) }
        }
        .pickerStyle(.segmented)
    }

    /// An invisible text field takes whatever the keyboard types; the diff becomes input.
    private func keys(_ id: String) -> some View {
        HStack(spacing: 8) {
            Button { keyboard.toggle() } label: { Label("Keyboard", systemImage: "keyboard") }
                .buttonStyle(QuietButtonStyle(active: keyboard))
            Button { core.input(id, [Input.key("enter")]) } label: { Image(systemName: "return") }
                .buttonStyle(QuietButtonStyle()).frame(width: 64).accessibilityLabel("Enter")
            Button { core.input(id, [Input.key("backspace")]) } label: { Image(systemName: "delete.left") }
                .buttonStyle(QuietButtonStyle()).frame(width: 64).accessibilityLabel("Backspace")
            Button { core.input(id, [Input.key("escape")]) } label: { Text("esc") }
                .buttonStyle(QuietButtonStyle()).frame(width: 64)
        }
        .background(
            TextField("", text: $typed)
                .focused($keyboard)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .opacity(0.01)
                .onChange(of: typed) { new in
                    guard !new.isEmpty else { return }
                    core.input(id, [Input.text(new)])
                    typed = ""
                }
                .onSubmit { core.input(id, [Input.key("enter")]) }
        )
    }

    private func media(_ id: String) -> some View {
        HStack(spacing: 8) {
            ForEach([
                ("previous", "backward.fill", "Previous"), ("play_pause", "playpause.fill", "Play or pause"),
                ("next", "forward.fill", "Next"), ("volume_down", "speaker.wave.1.fill", "Volume down"),
                ("volume_up", "speaker.wave.3.fill", "Volume up"),
            ], id: \.0) { item in
                Button { core.input(id, [Input.media(item.0)]) } label: { Image(systemName: item.1) }
                    .buttonStyle(QuietButtonStyle())
                    .accessibilityLabel(item.2)
            }
        }
    }
}

/// One finger moves (accelerated), a tap clicks, two fingers scroll, a long press right-clicks.
struct Touchpad: View {
    var send: ([Input.Event]) -> Void
    @State private var last: CGPoint?
    @State private var lastTime = Date()
    @State private var pending = CGSize.zero
    @State private var scrollLast: CGFloat?

    var body: some View {
        GeometryReader { geo in
            ZStack {
                RoundedRectangle(cornerRadius: 28, style: .continuous)
                    .fill(Theme.surface)
                    .overlay(RoundedRectangle(cornerRadius: 28, style: .continuous).strokeBorder(Theme.line))
                VStack(spacing: 6) {
                    Image(systemName: "hand.point.up.left").font(.system(size: 28)).foregroundStyle(Theme.textFaint)
                    Text("Move · tap to click · hold to right-click").font(Theme.ui(13)).foregroundStyle(Theme.textFaint)
                }
                // Scroll strip on the right edge.
                HStack {
                    Spacer()
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .fill(Theme.surface2)
                        .frame(width: 44)
                        .overlay(Image(systemName: "arrow.up.and.down").foregroundStyle(Theme.textFaint))
                        .padding(8)
                        .gesture(DragGesture(minimumDistance: 0).onChanged { v in
                            if let l = scrollLast { send([Input.scroll(0, Double(l - v.location.y) / 12)]) }
                            scrollLast = v.location.y
                        }.onEnded { _ in scrollLast = nil })
                        .accessibilityLabel("Scroll")
                }
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { v in
                        let now = Date()
                        if let l = last {
                            let dx = v.location.x - l.x, dy = v.location.y - l.y
                            let ms = max(1, now.timeIntervalSince(lastTime) * 1000)
                            let speed = sqrt(dx * dx + dy * dy) / ms
                            pending.width += Input.accelerate(dx, speedPointsPerMs: speed)
                            pending.height += Input.accelerate(dy, speedPointsPerMs: speed)
                            // Batch tiny moves: one message every ~12 ms is plenty.
                            if ms >= 12 || abs(pending.width) + abs(pending.height) > 8 {
                                send([Input.move(pending.width, pending.height)])
                                pending = .zero
                                lastTime = now
                            }
                        } else {
                            lastTime = now
                        }
                        last = v.location
                    }
                    .onEnded { _ in
                        if pending != .zero { send([Input.move(pending.width, pending.height)]) }
                        pending = .zero
                        last = nil
                    }
            )
            .simultaneousGesture(TapGesture(count: 2).onEnded { send([Input.click(count: 2)]) })
            .simultaneousGesture(TapGesture().onEnded { send([Input.click()]) })
            .simultaneousGesture(LongPressGesture(minimumDuration: 0.5).onEnded { _ in
                UIImpactFeedbackGenerator(style: .medium).impactOccurred()
                send([Input.click("right")])
            })
            .frame(width: geo.size.width, height: geo.size.height)
            .accessibilityElement()
            .accessibilityLabel("Touchpad")
            .accessibilityAddTraits(.allowsDirectInteraction)
        }
    }
}
