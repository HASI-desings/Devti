import Foundation
import Security

/// Keeps the device's private key (and other secrets the core asks for) in the iOS
/// Keychain, readable only on this device after its first unlock and never backed up to
/// iCloud or moved to a new phone.
final class KeychainStore: PlatformSecretStore, @unchecked Sendable {
    private let service = "app.devti.ios"

    private func query(_ key: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: key]
    }

    func get(key: String) throws -> Data? {
        var q = query(key)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess else { throw DevtiError.Failed(reason: "Keychain read failed (\(status))") }
        return out as? Data
    }

    func set(key: String, value: Data) throws {
        let attrs: [String: Any] = [
            kSecValueData as String: value,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        var status = SecItemUpdate(query(key) as CFDictionary, attrs as CFDictionary)
        if status == errSecItemNotFound {
            var add = query(key)
            add.merge(attrs) { $1 }
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw DevtiError.Failed(reason: "Keychain write failed (\(status))") }
    }

    func delete(key: String) throws {
        let status = SecItemDelete(query(key) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw DevtiError.Failed(reason: "Keychain delete failed (\(status))")
        }
    }

    func backendName() -> String { "iOS Keychain" }
}
