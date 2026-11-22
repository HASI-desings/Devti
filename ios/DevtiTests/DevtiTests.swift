import XCTest
@testable import Devti

/// Secrets in memory, for nodes that live only during a test.
final class MemoryStore: PlatformSecretStore, @unchecked Sendable {
    private var items: [String: Data] = [:]
    private let lock = NSLock()
    func get(key: String) throws -> Data? { lock.lock(); defer { lock.unlock() }; return items[key] }
    func set(key: String, value: Data) throws { lock.lock(); items[key] = value; lock.unlock() }
    func delete(key: String) throws { lock.lock(); items[key] = nil; lock.unlock() }
    func backendName() -> String { "test memory" }
}

/// Collects a node's events.
final class Collector: EventListener, @unchecked Sendable {
    private var events: [String] = []
    private let lock = NSLock()
    func onEvent(eventJson: String) { lock.lock(); events.append(eventJson); lock.unlock() }
    var all: [String] { lock.lock(); defer { lock.unlock() }; return events }
}

final class DevtiTests: XCTestCase {
    private func node(_ name: String, kind: String, platform: String, _ listener: Collector) throws -> DevtiMobile {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let config = MobileConfig(dataDir: dir.path, deviceName: name, kind: kind, platform: platform, osVersion: "test",
                                  model: nil, appVersion: "1.0.0", discovery: false)
        return try DevtiMobile(config: config, secrets: MemoryStore(), listener: listener)
    }

    private func wait(_ what: String, timeout: TimeInterval = 20, _ check: () -> Bool) {
        let end = Date().addingTimeInterval(timeout)
        while !check() {
            if Date() > end { return XCTFail("timed out waiting for \(what)") }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
    }

    /// The Swift bindings drive the real Rust core: two nodes pair over loopback QUIC exactly
    /// as the app does after scanning a QR code, then a clipboard and a listing go across.
    func testTwoCoresPairThroughTheSwiftBindings() throws {
        let hubEvents = Collector(), phoneEvents = Collector()
        let hub = try node("Studio Laptop", kind: "laptop", platform: "linux", hubEvents)
        let phone = try node("iPhone", kind: "phone", platform: "ios", phoneEvents)
        defer { hub.shutdown(); phone.shutdown() }

        // Read the link without assuming the rest of the window's shape.
        let raw = try hub.openPairingWindowJson()
        let uri = try XCTUnwrap((try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])?["uri"] as? String)
        let id = try phone.pairWithUri(uri: uri)

        wait("verify on both") {
            hubEvents.all.contains { $0.contains("\"pairing_verify\"") } && phoneEvents.all.contains { $0.contains("\"pairing_verify\"") }
        }
        let hubVerify = try XCTUnwrap(hubEvents.all.compactMap { JSON.decode(Events.PairingVerify.self, $0) }.first)
        let phoneVerify = try XCTUnwrap(phoneEvents.all.compactMap { JSON.decode(Events.PairingVerify.self, $0) }.first)
        XCTAssertEqual(hubVerify.sas, phoneVerify.sas, "both screens show the same code")
        XCTAssertEqual(phoneVerify.pairingId, id)
        try hub.confirmPairing(pairingId: hubVerify.pairingId, accept: true)
        try phone.confirmPairing(pairingId: id, accept: true)

        wait("online") { (JSON.decode([PeerSnapshot].self, phone.peersJson()) ?? []).contains { $0.online } }
        let peers = try XCTUnwrap(JSON.decode([PeerSnapshot].self, phone.peersJson()))
        XCTAssertEqual(peers.first?.info.name, "Studio Laptop")
        XCTAssertEqual(peers.first?.isComputer, true)
        XCTAssertEqual(peers.first?.connection?.kind, "lan")

        // Clipboard from the "computer" arrives as an event the app's model decodes.
        _ = try hub.sendClipboard(text: "from the laptop")
        wait("clipboard") { phoneEvents.all.contains { $0.contains("from the laptop") } }
        let clip = try XCTUnwrap(phoneEvents.all.compactMap { JSON.decode(Events.Clipboard.self, $0) }.first)
        XCTAssertEqual(clip.fromName, "Studio Laptop")

        // A shared folder listed through the same calls BrowseView makes.
        let shared = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: shared, withIntermediateDirectories: true)
        try Data("hello".utf8).write(to: shared.appendingPathComponent("notes.txt"))
        try hub.setSharedFolders(foldersJson: "[{\"name\":\"Notes\",\"path\":\"\(shared.path)\"}]")
        let hubId = try XCTUnwrap(peers.first?.info.id)
        try phone.fsList(deviceId: hubId, path: "/Notes")
        wait("listing") { phoneEvents.all.contains { $0.contains("\"fs_listing\"") } }
        let listing = try XCTUnwrap(phoneEvents.all.compactMap { JSON.decode(Events.FsListed.self, $0) }.first)
        XCTAssertEqual(listing.entries.map(\.name), ["notes.txt"])
        XCTAssertEqual(listing.entries.first?.size, 5)
    }

    func testInputMatchesTheProtocol() throws {
        let json = Input.encode([Input.move(3, -2), Input.click("right"), Input.key("tab", ["alt"]), Input.text("hé")])
        let events = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [[String: Any]])
        XCTAssertEqual(events.map { $0["k"] as? String }, ["move", "click", "key", "text"])
        XCTAssertEqual(events[1]["button"] as? String, "right")
        XCTAssertEqual(events[2]["mods"] as? [String], ["alt"])
        XCTAssertTrue(Input.system("set_volume", percent: 180).contains("\"percent\":100"))
        XCTAssertEqual(Input.moveTo(1.4, -0.2)["x"] as? Double, 1)
    }

    func testVoicesMatchAndroid() {
        // Same FNV-1a as voiceFor() in the Android app, so a device has one color everywhere.
        XCTAssertEqual(Theme.voice("ABC"), Theme.voices["mint"])
        XCTAssertEqual(Theme.voice("N2B7Q4XZ5K3M8P6R"), Theme.voices["rose"])
    }

    func testModelsDecodeCoreJSON() throws {
        let peer = """
        {"info":{"id":"ABC","name":"Studio Laptop","kind":"laptop","platform":"linux","os_version":"Ubuntu","model":null,
         "app_version":"1.0.0","capabilities":[]},"fingerprint":"aa bb","role":"trusted",
         "permissions":{"screen_view":true,"control":true,"files":true,"clipboard":true,"notifications":true,
         "commands":true,"bluetooth":true},"paired_at":1,"last_seen":null,"online":true,
         "connection":{"kind":"vpn","address":"100.101.1.2:47810","rtt_ms":18,"since":5},"status":{"battery":{"percent":76,"charging":true}}}
        """
        let p = try XCTUnwrap(JSON.decode(PeerSnapshot.self, peer))
        XCTAssertTrue(p.isComputer)
        XCTAssertEqual(p.connection?.kind, "vpn")
        XCTAssertNil(p.permissions.browse, "records from before Phase 7 have no browse value")
        let big = "{\"type\":\"pairing_verify\",\"pairing_id\":18446744073709551000,\"peer\":{\"id\":\"X\",\"name\":\"N\",\"kind\":\"desktop\",\"platform\":\"windows\"},\"sas\":\"123 456\"}"
        XCTAssertEqual(JSON.decode(Events.PairingVerify.self, big)?.pairingId, 18_446_744_073_709_551_000, "64-bit ids stay exact")
        XCTAssertEqual(JSON.decode(Catalog.self, "{}")?.commands.count, 0)
    }
}
