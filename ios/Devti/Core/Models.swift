import Foundation

// The shapes Devti Core sends as JSON (see core/src/node/events.rs). Field names are the
// core's snake_case, decoded with `.convertFromSnakeCase`.

struct DeviceInfo: Codable, Hashable {
    var id: String
    var name: String
    var kind: String
    var platform: String
    var osVersion: String?
    var model: String?
    var appVersion: String?
}

struct BatteryState: Codable, Hashable {
    var percent: Int
    var charging: Bool
}

struct DeviceStatus: Codable, Hashable {
    var battery: BatteryState?
}

struct Permissions: Codable, Hashable {
    var screenView: Bool
    var control: Bool
    var files: Bool
    var clipboard: Bool
    var notifications: Bool
    var commands: Bool
    var bluetooth: Bool
    /// Phase 7; records saved before it existed have no value.
    var browse: Bool?
}

struct ConnectionSnapshot: Codable, Hashable {
    /// "lan", "vpn" or "internet".
    var kind: String
    var address: String
    var rttMs: Int
    var since: Int64
}

struct PeerSnapshot: Codable, Hashable, Identifiable {
    var info: DeviceInfo
    var fingerprint: String
    var role: String
    var permissions: Permissions
    var pairedAt: Int64
    var lastSeen: Int64?
    var online: Bool
    var connection: ConnectionSnapshot?
    var status: DeviceStatus

    var id: String { info.id }
    var isComputer: Bool { info.kind == "desktop" || info.kind == "laptop" }
}

struct LocalSnapshot: Codable, Hashable {
    var info: DeviceInfo
    var fingerprint: String
    var port: Int
    var addresses: [String]
    var status: DeviceStatus
    var paused: Bool
}

struct TransferRecord: Codable, Hashable, Identifiable {
    var id: UInt64
    var deviceId: String
    var deviceName: String
    var incoming: Bool
    var name: String
    var path: String
    var size: UInt64
    var done: UInt64
    /// queued, active, paused, done, failed, cancelled.
    var state: String
    var ts: Int64

    var finished: Bool { state == "done" || state == "failed" || state == "cancelled" }
    var fraction: Double { size == 0 ? 1 : min(1, Double(done) / Double(size)) }
}

struct CatalogApp: Codable, Hashable, Identifiable {
    var id: String
    var name: String
}

struct CatalogCommand: Codable, Hashable, Identifiable {
    var id: String
    var name: String
    var confirm: Bool?
}

struct DeckButton: Codable, Hashable, Identifiable {
    var id: String
    var label: String
    var icon: String?
    var voice: String?
}

struct DeckPage: Codable, Hashable {
    var name: String
    var buttons: [DeckButton]
}

struct Catalog: Codable, Hashable {
    var apps: [CatalogApp]
    var commands: [CatalogCommand]
    var deck: [DeckPage]
    var system: [String]

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        apps = try c.decodeIfPresent([CatalogApp].self, forKey: .apps) ?? []
        commands = try c.decodeIfPresent([CatalogCommand].self, forKey: .commands) ?? []
        deck = try c.decodeIfPresent([DeckPage].self, forKey: .deck) ?? []
        system = try c.decodeIfPresent([String].self, forKey: .system) ?? []
    }
}

struct FsEntry: Codable, Hashable, Identifiable {
    var name: String
    var dir: Bool
    var size: UInt64
    var modified: Int64
    var id: String { name }
}

struct FsListing: Hashable {
    var entries: [FsEntry]
    var truncated: Bool
    var error: String?
    var at: Date
}

struct HandoffItem: Codable, Hashable {
    var kind: String
    var url: String?
    var title: String?
    var text: String?
    var positionMs: UInt64?
}

struct ReachInfo: Codable, Hashable {
    var enabled: Bool
    var port: Int
    var extra: [String]
}

/// Where a device is (Find my device).
struct DeviceLocation: Codable, Hashable {
    var lat: Double?
    var lon: Double?
    var accuracyM: Double?
    var ts: Int64
    var source: String?
    var publicIp: String?
    var localIps: [String]?
    var network: String?
    var error: String?
}

/// Connecting from anywhere (built in since 1.1).
struct AnywhereInfo: Codable, Hashable {
    var enabled: Bool
    var running: Bool
    var online: Bool
    var relay: String?
    var customRelay: String
    var publicIp: String?
}

enum PairingStage: Equatable {
    case connecting
    case verify(id: UInt64, peer: DeviceInfo, sas: String, confirmed: Bool)
    case done(PeerSnapshot)
    case failed(String)
}

// MARK: - Events

/// Every event carries its kind in `type`; the rest depends on it.
struct EventHead: Decodable { var type: String }

enum Events {
    struct Local: Decodable { var local: LocalSnapshot }
    struct Peer: Decodable { var peer: PeerSnapshot }
    struct PeerRemoved: Decodable { var id: String; var reason: String? }
    struct PairingVerify: Decodable { var pairingId: UInt64; var peer: DeviceInfo; var sas: String }
    struct PairingCompleted: Decodable { var pairingId: UInt64; var peer: PeerSnapshot }
    struct PairingFailed: Decodable { var pairingId: UInt64; var reason: String?; var message: String? }
    struct Paused: Decodable { var paused: Bool }
    struct Transfer: Decodable { var transfer: TransferRecord }
    struct Clipboard: Decodable { var fromName: String; var text: String }
    struct Ring: Decodable { var fromName: String; var on: Bool }
    struct CatalogReceived: Decodable { var from: String; var catalog: Catalog }
    struct RunFinished: Decodable { var command: String; var ok: Bool; var output: String }
    struct FsListed: Decodable { var from: String; var path: String; var entries: [FsEntry]; var truncated: Bool; var error: String? }
    struct FsFailed: Decodable { var path: String; var error: String }
    struct Handoff: Decodable { var fromName: String; var item: HandoffItem }
    struct ScreenRequested: Decodable { var from: String; var on: Bool }
    struct LocateRequested: Decodable { var from: String }
    struct LocationReceived: Decodable { var from: String; var location: DeviceLocation }
    struct LowBattery: Decodable { var name: String; var percent: Int }
    struct Dnd: Decodable { var on: Bool }
}

enum JSON {
    static let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }()

    static func decode<T: Decodable>(_ type: T.Type, _ text: String) -> T? {
        try? decoder.decode(type, from: Data(text.utf8))
    }

    static func string<T: Encodable>(_ value: T) -> String {
        let e = JSONEncoder()
        e.keyEncodingStrategy = .convertToSnakeCase
        return (try? e.encode(value)).flatMap { String(data: $0, encoding: .utf8) } ?? "null"
    }
}

/// The sentence inside a core error, for showing to people.
func readable(_ error: Error) -> String {
    if let e = error as? DevtiError {
        switch e {
        case let .InvalidInput(reason), let .Failed(reason): return reason
        case .Paused: return "Devti is paused on this iPhone. Resume it in Settings."
        }
    }
    return error.localizedDescription
}
