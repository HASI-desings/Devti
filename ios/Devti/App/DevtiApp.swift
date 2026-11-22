import SwiftUI

@main
struct DevtiApp: App {
    @StateObject private var core = Core()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(core)
                .onAppear { core.start() }
                .onOpenURL { url in
                    // The system camera opens devti://pair links from the Hub's QR code.
                    if url.scheme == "devti" { core.pair(uri: url.absoluteString) }
                }
        }
        .onChange(of: phase) { p in
            if p == .active { core.becameActive() }
        }
    }
}
