import SwiftUI

/// What a computer shares: tap a folder to open it, a file to download it into Files →
/// On My iPhone → Devti.
struct BrowseView: View {
    @EnvironmentObject private var core: Core
    @Environment(\.dismiss) private var dismiss
    @State private var device: String?
    @State private var path = "/"
    @State private var asked = Date.distantPast

    private var online: [PeerSnapshot] { core.computers.filter(\.online) }
    private var current: String? { device ?? online.first?.id }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 14) {
                if online.count > 1 {
                    Picker("Computer", selection: Binding(get: { current ?? "" }, set: { open($0, "/") })) {
                        ForEach(online) { Text($0.info.name).tag($0.id) }
                    }
                    .pickerStyle(.segmented)
                }
                crumbs
                listing
            }
            .padding(20)
            .background(Theme.bg)
            .navigationTitle("Browse")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { Button("Done") { dismiss() } }
            .task(id: current) { if let id = current { open(id, path) } }
        }
        .foregroundStyle(Theme.text)
    }

    private func open(_ id: String, _ to: String) {
        device = id
        path = to
        asked = Date()
        core.fsList(id, to)
    }

    private var crumbs: some View {
        let parts = path.split(separator: "/").map(String.init)
        return ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                Button("Shared folders") { if let id = current { open(id, "/") } }
                    .foregroundStyle(parts.isEmpty ? Theme.text : Theme.textDim)
                ForEach(parts.indices, id: \.self) { i in
                    Image(systemName: "chevron.right").font(.system(size: 11)).foregroundStyle(Theme.textFaint)
                    Button(parts[i]) {
                        if let id = current { open(id, "/" + parts[0...i].joined(separator: "/")) }
                    }
                    .foregroundStyle(i == parts.count - 1 ? Theme.text : Theme.textDim)
                }
            }
            .font(Theme.ui(15, .semibold))
        }
    }

    @ViewBuilder private var listing: some View {
        let entry = current.flatMap { core.listings["\($0)|\(path)"] }
        if current == nil {
            Text("No computer is connected right now.").font(Theme.ui(15)).foregroundStyle(Theme.textDim)
            Spacer()
        } else if entry == nil || entry!.at < asked {
            ProgressView().tint(Theme.text).frame(maxWidth: .infinity)
            Spacer()
        } else if let error = entry?.error {
            Text(error).font(Theme.ui(15)).foregroundStyle(Theme.danger)
            Spacer()
        } else if let entry, entry.entries.isEmpty {
            Text(path == "/" ? "This computer doesn't share any folders yet. On the computer: Files → Browse → Share a folder." : "This folder is empty.")
                .font(Theme.ui(15)).foregroundStyle(Theme.textDim)
            Spacer()
        } else if let entry, let id = current {
            List(entry.entries) { e in
                Button {
                    let child = (path == "/" ? "" : path) + "/" + e.name
                    if e.dir { open(id, child) } else { core.fsGet(id, child) }
                } label: {
                    HStack(spacing: 12) {
                        Image(systemName: e.dir ? "folder.fill" : "doc").foregroundStyle(e.dir ? Theme.text : Theme.textDim)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(e.name).font(Theme.ui(16, .semibold)).lineLimit(1)
                            if !e.dir {
                                Text(ByteCountFormatter.string(fromByteCount: Int64(e.size), countStyle: .file))
                                    .font(Theme.ui(13)).foregroundStyle(Theme.textDim)
                            }
                        }
                        Spacer()
                        Image(systemName: e.dir ? "chevron.right" : "arrow.down.circle").foregroundStyle(Theme.textFaint)
                    }
                }
                .listRowBackground(Theme.surface)
                .accessibilityHint(e.dir ? "Opens the folder" : "Downloads the file")
            }
            .scrollContentBackground(.hidden)
            .listStyle(.insetGrouped)
            if entry.truncated {
                Text("Showing the first 2,000 items.").font(Theme.ui(13)).foregroundStyle(Theme.textFaint)
            }
        }
    }
}
