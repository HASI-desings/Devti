import SwiftUI

/// The computer's shortcuts deck and saved commands, designed on the Hub.
struct DeckView: View {
    @EnvironmentObject private var core: Core
    @State private var page = 0

    private var hub: PeerSnapshot? { core.computers.first(where: \.online) ?? core.computers.first }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    if let hub, let catalog = core.catalogs[hub.id] {
                        if !catalog.deck.isEmpty { deck(hub, catalog) }
                        if !catalog.commands.isEmpty { commands(hub, catalog) }
                        if catalog.deck.isEmpty && catalog.commands.isEmpty {
                            Text("Nothing here yet. Design the deck and add commands in Devti on \(hub.info.name) → Control.")
                                .font(Theme.ui(15)).foregroundStyle(Theme.textDim)
                        }
                    } else if let hub {
                        Text(hub.online ? "Loading \(hub.info.name)'s deck…" : "\(hub.info.name) is offline.")
                            .font(Theme.ui(15)).foregroundStyle(Theme.textDim)
                    } else {
                        Text("Pair a computer to use its deck.").font(Theme.ui(15)).foregroundStyle(Theme.textDim)
                    }
                }
                .padding(20)
            }
            .background(Theme.bg)
            .navigationTitle("Deck")
            .task(id: hub?.online) { if let hub, hub.online { core.requestCatalog(hub.id) } }
            .refreshable { if let hub { core.requestCatalog(hub.id) } }
        }
    }

    private func deck(_ hub: PeerSnapshot, _ catalog: Catalog) -> some View {
        let pages = catalog.deck
        let current = pages[min(page, pages.count - 1)]
        return VStack(alignment: .leading, spacing: 14) {
            if pages.count > 1 {
                Picker("Page", selection: $page) {
                    ForEach(pages.indices, id: \.self) { Text(pages[$0].name).tag($0) }
                }
                .pickerStyle(.segmented)
            }
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 100), spacing: 12)], spacing: 12) {
                ForEach(current.buttons) { b in
                    let voice = Theme.voices[b.voice ?? ""] ?? Theme.text
                    Button {
                        UIImpactFeedbackGenerator(style: .light).impactOccurred()
                        core.deckPress(hub.id, b.id)
                    } label: {
                        VStack(spacing: 10) {
                            Circle().fill(voice).frame(width: 12, height: 12).shadow(color: voice.opacity(0.7), radius: 6)
                            Text(b.label).font(Theme.ui(14, .semibold)).multilineTextAlignment(.center).lineLimit(2)
                        }
                        .frame(maxWidth: .infinity, minHeight: 96)
                        .background(Theme.surface, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
                        .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(voice.opacity(0.35)))
                    }
                    .buttonStyle(.plain)
                    .disabled(!hub.online)
                }
            }
        }
    }

    private func commands(_ hub: PeerSnapshot, _ catalog: Catalog) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Commands").font(Theme.display(22))
            ForEach(catalog.commands) { c in
                Button { core.run(hub.id, c.id) } label: {
                    HStack {
                        Text(c.name).font(Theme.ui(16, .semibold))
                        Spacer()
                        Image(systemName: "play.fill").foregroundStyle(Theme.textDim)
                    }
                    .padding(16)
                    .background(Theme.surface, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                }
                .buttonStyle(.plain)
                .disabled(!hub.online)
            }
        }
    }
}
