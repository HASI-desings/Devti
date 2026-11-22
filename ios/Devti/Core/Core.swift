import AudioToolbox
import Foundation
import UIKit
import UserNotifications

/// One frame of a computer's screen this iPhone is showing.
struct ScreenFrame {
    var image: UIImage
    var width: Int
    var height: Int
    var latencyMs: Int
    var fps: Int
}

/// Receives the core's callbacks on its threads and hands them to the main actor.
final class CoreListener: EventListener, FrameListener, @unchecked Sendable {
    weak var core: Core?

    func onEvent(eventJson: String) {
        Task { @MainActor [weak core = self.core] in core?.apply(eventJson) }
    }

    func onFrame(deviceId: String, seq: UInt64, width: UInt32, height: UInt32, latencyMs: UInt32, fps: UInt8, jpeg: Data) {
        guard let image = UIImage(data: jpeg) else { return }
        let frame = ScreenFrame(image: image, width: Int(width), height: Int(height), latencyMs: Int(latencyMs), fps: Int(fps))
        Task { @MainActor [weak core = self.core] in core?.frame = frame }
    }
}

/// The app's Devti Core node and everything the UI shows, kept on the main actor.
///
/// iOS suspends apps shortly after they leave the screen, so the connection lives while
/// Devti is open; it reconnects by itself when you come back (`becameActive`).
@MainActor
final class Core: ObservableObject {
    @Published private(set) var error: String?
    @Published private(set) var local: LocalSnapshot?
    @Published private(set) var peers: [PeerSnapshot] = []
    @Published var pairing: PairingStage?
    @Published private(set) var transfers: [TransferRecord] = []
    @Published private(set) var catalogs: [String: Catalog] = [:]
    @Published private(set) var listings: [String: FsListing] = [:]
    @Published var frame: ScreenFrame?
    @Published var ringingFrom: String?
    @Published var toast: String?
    @Published private(set) var dnd = false

    private var node: DevtiMobile?
    private let listener = CoreListener()
    private var ringTimer: Timer?

    /// The computer this iPhone belongs to: the first paired computer.
    var hub: PeerSnapshot? { peers.first(where: \.isComputer) ?? peers.first }
    var computers: [PeerSnapshot] { peers.filter(\.isComputer) }

    static var receivedDir: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("Received", isDirectory: true)
    }

    func start() {
        guard node == nil else { return }
        listener.core = self
        do {
            let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("devti", isDirectory: true)
            try FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
            try FileManager.default.createDirectory(at: Self.receivedDir, withIntermediateDirectories: true)
            let pad = UIDevice.current.userInterfaceIdiom == .pad
            let config = MobileConfig(
                dataDir: support.path,
                deviceName: UIDevice.current.name,
                kind: pad ? "tablet" : "phone",
                platform: pad ? "ipados" : "ios",
                osVersion: "\(UIDevice.current.systemName) \(UIDevice.current.systemVersion)",
                model: UIDevice.current.model,
                appVersion: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "1.0.0",
                // iOS only lets apps with Apple's multicast entitlement browse the network,
                // so the iPhone pairs by QR code and reconnects to known addresses.
                discovery: false
            )
            let n = try DevtiMobile(config: config, secrets: KeychainStore(), listener: listener)
            node = n
            try? n.setDownloadsDir(path: Self.receivedDir.path)
            refresh()
            watchBattery()
        } catch {
            self.error = "Devti couldn't start: \(readable(error))"
        }
    }

    func refresh() {
        guard let n = node else { return }
        local = JSON.decode(LocalSnapshot.self, n.localJson())
        peers = sort(JSON.decode([PeerSnapshot].self, n.peersJson()) ?? [])
        transfers = Array((JSON.decode([TransferRecord].self, n.transfersJson()) ?? []).sorted { $0.ts > $1.ts }.prefix(50))
        dnd = n.dnd()
    }

    /// The app came to the screen: reconnect now instead of waiting for the next retry.
    func becameActive() {
        node?.reconnectNow()
        refresh()
    }

    private func sort(_ list: [PeerSnapshot]) -> [PeerSnapshot] {
        list.sorted { ($0.online ? 0 : 1, $0.info.name) < ($1.online ? 0 : 1, $1.info.name) }
    }

    private func upsert(_ peer: PeerSnapshot) {
        peers = sort(peers.filter { $0.id != peer.id } + [peer])
    }

    // MARK: Events

    func apply(_ json: String) {
        guard let head = JSON.decode(EventHead.self, json) else { return }
        switch head.type {
        case "resync":
            refresh()
        case "local_changed":
            if let e = JSON.decode(Events.Local.self, json) { local = e.local }
        case "peer_changed":
            if let e = JSON.decode(Events.Peer.self, json) { upsert(e.peer) }
        case "peer_removed":
            guard let e = JSON.decode(Events.PeerRemoved.self, json) else { return }
            let gone = peers.first { $0.id == e.id }
            peers.removeAll { $0.id == e.id }
            if e.reason == "revoked_by_peer", let gone { toast = "\(gone.info.name) removed this iPhone from its devices." }
        case "pairing_started":
            pairing = .connecting
        case "pairing_verify":
            if let e = JSON.decode(Events.PairingVerify.self, json) {
                pairing = .verify(id: e.pairingId, peer: e.peer, sas: e.sas, confirmed: false)
            }
        case "pairing_completed":
            if let e = JSON.decode(Events.PairingCompleted.self, json) {
                upsert(e.peer)
                pairing = .done(e.peer)
                askForNotifications()
            }
        case "pairing_failed":
            if let e = JSON.decode(Events.PairingFailed.self, json) {
                pairing = .failed(e.message?.isEmpty == false ? e.message! : "Pairing didn't finish. Try again.")
            }
        case "paused":
            if let e = JSON.decode(Events.Paused.self, json) { local?.paused = e.paused }
        case "transfer":
            guard let e = JSON.decode(Events.Transfer.self, json) else { return }
            transfers = Array(([e.transfer] + transfers.filter { $0.id != e.transfer.id }).sorted { $0.ts > $1.ts }.prefix(50))
            if e.transfer.incoming && e.transfer.state == "done" {
                notify("Received \(e.transfer.name)", "From \(e.transfer.deviceName). It's in Files → On My iPhone → Devti.")
            }
        case "dnd_changed":
            if let e = JSON.decode(Events.Dnd.self, json) { dnd = e.on }
        case "clipboard_received":
            // Writing the pasteboard is allowed any time; reading it is not (see sendClipboard).
            if let e = JSON.decode(Events.Clipboard.self, json) {
                UIPasteboard.general.string = e.text
                toast = "Copied from \(e.fromName)"
            }
        case "ring_requested":
            if let e = JSON.decode(Events.Ring.self, json) { e.on ? startRinging(e.fromName) : stopRinging() }
        case "catalog_received":
            if let e = JSON.decode(Events.CatalogReceived.self, json) { catalogs[e.from] = e.catalog }
        case "run_finished":
            if let e = JSON.decode(Events.RunFinished.self, json) {
                toast = e.ok ? (e.output.isEmpty ? "Done." : e.output) : "That didn't work: \(e.output)"
            }
        case "fs_listing":
            if let e = JSON.decode(Events.FsListed.self, json) {
                listings["\(e.from)|\(e.path)"] = FsListing(entries: e.entries, truncated: e.truncated, error: e.error, at: Date())
            }
        case "fs_error":
            if let e = JSON.decode(Events.FsFailed.self, json) { toast = e.error }
        case "handoff":
            if let e = JSON.decode(Events.Handoff.self, json) { continueHere(e.item, from: e.fromName) }
        case "screen_requested":
            // iOS only lets a broadcast extension share the screen; Devti doesn't ship one.
            if let e = JSON.decode(Events.ScreenRequested.self, json), e.on {
                try? node?.screenStopSharing(deviceId: e.from)
                toast = "iPhone can't share its screen with Devti — Apple only allows that for screen-recording extensions."
            }
        case "low_battery":
            if let e = JSON.decode(Events.LowBattery.self, json) { notify("\(e.name) battery low", "\(e.percent)% left.") }
        default:
            break
        }
    }

    // MARK: Pairing

    func pair(uri: String) {
        pairing = .connecting
        do { _ = try start(orFail: { try $0.pairWithUri(uri: uri) }) } catch { pairing = .failed(readable(error)) }
    }

    func confirm(_ accept: Bool) {
        guard case let .verify(id, peer, sas, _) = pairing else { return }
        if accept { pairing = .verify(id: id, peer: peer, sas: sas, confirmed: true) }
        do { try node?.confirmPairing(pairingId: id, accept: accept) } catch { pairing = .failed(readable(error)) }
        if !accept { pairing = nil }
    }

    private func start(orFail f: (DevtiMobile) throws -> UInt64) throws -> UInt64 {
        start()
        guard let n = node else { throw DevtiError.Failed(reason: error ?? "Devti isn't running.") }
        return try f(n)
    }

    // MARK: Everyday actions

    /// Runs a core call and turns its error into a toast.
    @discardableResult
    private func act(_ f: (DevtiMobile) throws -> Void) -> Bool {
        guard let n = node else { return false }
        do {
            try f(n)
            return true
        } catch {
            toast = readable(error)
            return false
        }
    }

    func forget(_ id: String) { act { try $0.revoke(deviceId: id) } }
    func setPaused(_ paused: Bool) { act { try $0.setPaused(paused: paused) } }
    func rename(_ name: String) { act { try $0.rename(name: name) } }
    func setDnd(_ on: Bool) { node?.setDnd(on: on) }

    /// Sends text from the system paste control (reading the pasteboard yourself shows
    /// iOS's "Allow Paste" prompt every time; the paste button doesn't).
    func sendClipboard(_ text: String) {
        if act({ _ = try $0.sendClipboard(text: text) }) { toast = "Sent to your devices' clipboards" }
    }

    func ring(_ id: String, _ on: Bool) { act { try $0.ring(deviceId: id, on: on) } }

    /// Copies picked documents into the app (they must stay readable for resumes) and sends them.
    func send(_ urls: [URL], to id: String) {
        let staging = FileManager.default.temporaryDirectory.appendingPathComponent("outgoing", isDirectory: true)
        try? FileManager.default.createDirectory(at: staging, withIntermediateDirectories: true)
        var sent = 0
        for url in urls {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            let copy = staging.appendingPathComponent(UUID().uuidString, isDirectory: true).appendingPathComponent(url.lastPathComponent)
            do {
                try FileManager.default.createDirectory(at: copy.deletingLastPathComponent(), withIntermediateDirectories: true)
                try FileManager.default.copyItem(at: url, to: copy)
                if act({ _ = try $0.sendFile(deviceId: id, path: copy.path) }) { sent += 1 }
            } catch {
                toast = "\(url.lastPathComponent) couldn't be read."
            }
        }
        if sent > 0 { toast = sent == 1 ? "Sending…" : "Sending \(sent) files…" }
    }

    func cancelTransfer(_ id: UInt64) { act { try $0.cancelTransfer(id: id) } }

    // Remote control of a computer.
    func input(_ id: String, _ events: [Input.Event]) { _ = try? node?.sendInput(deviceId: id, eventsJson: Input.encode(events)) }
    func system(_ id: String, _ action: String, percent: Int? = nil) {
        act { try $0.systemAction(deviceId: id, actionJson: Input.system(action, percent: percent)) }
    }
    func requestCatalog(_ id: String) { _ = try? node?.requestCatalog(deviceId: id) }
    func deckPress(_ id: String, _ button: String) { act { try $0.deckPress(deviceId: id, button: button) } }
    func run(_ id: String, _ command: String) { act { try $0.runCommand(deviceId: id, command: command) } }
    func launch(_ id: String, _ app: String) { act { try $0.launchApp(deviceId: id, app: app) } }

    // A computer's screen here.
    func watch(_ id: String, maxWidth: Int) {
        frame = nil
        act { try $0.screenStart(deviceId: id, maxWidth: UInt32(maxWidth), fps: 30, listener: listener) }
    }

    func stopWatching(_ id: String) {
        _ = try? node?.screenStop(deviceId: id)
        frame = nil
    }

    // Shared folders and reach (Phase 7).
    func fsList(_ id: String, _ path: String) { act { try $0.fsList(deviceId: id, path: path) } }
    func fsGet(_ id: String, _ path: String) {
        if act({ try $0.fsGet(deviceId: id, path: path) }) { toast = "Downloading — it goes to Files → On My iPhone → Devti." }
    }

    var reach: ReachInfo? { node.flatMap { JSON.decode(ReachInfo.self, $0.reachInfoJson()) } }
    func setReach(_ on: Bool) {
        let extra = JSON.string(reach?.extra ?? [])
        act { try $0.setReach(enabled: on, extraJson: extra) }
        objectWillChange.send()
    }

    // MARK: Phone things

    private func continueHere(_ item: HandoffItem, from: String) {
        if let s = item.url, var url = URL(string: s), s.hasPrefix("http") {
            if let ms = item.positionMs, ms >= 1000, s.contains("youtu") {
                if var c = URLComponents(url: url, resolvingAgainstBaseURL: false) {
                    let kept = (c.queryItems ?? []).filter { $0.name != "t" }
                    c.queryItems = kept + [URLQueryItem(name: "t", value: "\(ms / 1000)s")]
                    url = c.url ?? url
                }
            }
            UIApplication.shared.open(url)
            toast = "Continued from \(from)"
        } else if let t = item.text, !t.isEmpty {
            UIPasteboard.general.string = t
            toast = "Copied from \(from)"
        }
    }

    private func startRinging(_ from: String) {
        ringingFrom = from
        ringTimer?.invalidate()
        ringTimer = Timer.scheduledTimer(withTimeInterval: 1.6, repeats: true) { _ in
            AudioServicesPlayAlertSound(SystemSoundID(1005))
            AudioServicesPlaySystemSound(kSystemSoundID_Vibrate)
        }
        ringTimer?.fire()
        notify("\(from) is looking for this iPhone", "Open Devti and tap “I found it”.")
    }

    func stopRinging() {
        ringTimer?.invalidate()
        ringTimer = nil
        ringingFrom = nil
    }

    private func askForNotifications() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    private func notify(_ title: String, _ body: String) {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
    }

    private func watchBattery() {
        UIDevice.current.isBatteryMonitoringEnabled = true
        reportBattery()
        for name in [UIDevice.batteryLevelDidChangeNotification, UIDevice.batteryStateDidChangeNotification] {
            NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.reportBattery() }
            }
        }
    }

    private func reportBattery() {
        let level = UIDevice.current.batteryLevel
        guard level >= 0 else { return } // the simulator has no battery
        let charging = UIDevice.current.batteryState == .charging || UIDevice.current.batteryState == .full
        node?.setBattery(percent: UInt8(max(0, min(100, Int((level * 100).rounded())))), charging: charging)
    }

    // MARK: Tests

    /// For unit tests: the node, once started.
    var mobile: DevtiMobile? { node }
}
