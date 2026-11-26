import Foundation
import Security

// MARK: - Hardware Keychain Master Key & Stable Device Identity

public enum OrbitKeychainError: LocalizedError, Equatable {
    case csprngFailed(OSStatus)
    case keychainStatus(OSStatus)
    case invalidKeyLength(Int)

    public var errorDescription: String? {
        switch self {
        case let .csprngFailed(status):
            return "CSPRNG key generation failed (status \(status))."
        case let .keychainStatus(status):
            return "Keychain operation failed (status \(status))."
        case let .invalidKeyLength(count):
            return "Expected a 32-byte master key, got \(count) bytes."
        }
    }
}

/// Manages the 32-byte AES-256 master key protecting `orbit-store` SQLite secrets
/// inside iOS hardware-backed Keychain (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`),
/// with a protected file fallback exclusively when Keychain entitlements are unavailable
/// (for example in unsigned unit-test sandboxes).
public final class OrbitKeychain: @unchecked Sendable {
    public static let shared = OrbitKeychain()

    public static let masterKeyByteCount = 32
    public static let defaultService = "dev.orbit.app.security"
    public static let masterKeyAccount = "orbit.local-store.master-key.v1"
    public static let pushDeviceIdAccount = "orbit.push.device-id.v1"

    private let service: String

    public init(service: String = OrbitKeychain.defaultService) {
        self.service = service
    }

    public enum MasterKeySource: String, Equatable, Sendable {
        case hardwareKeychain
        case protectedFileFallback
    }

    public struct MasterKeyResult: Equatable, Sendable {
        public let key: Data
        public let source: MasterKeySource
        public let newlyGenerated: Bool
    }

    /// Loads or generates the 32-byte master key for `OrbitMobileCore.newWithMasterKey`.
    /// Never overwrites an existing key; if the Keychain is unavailable in an unsigned sandbox
    /// or transiently unreadable, falls back cleanly to a 0600 backup-excluded file inside `fallbackDirectory`.
    public func loadOrCreateMasterKey(fallbackDirectory: URL? = nil) throws -> MasterKeyResult {
        var keychainUnavailable = false
        do {
            if let existing = try readKeychainData(account: Self.masterKeyAccount) {
                guard existing.count == Self.masterKeyByteCount else {
                    throw OrbitKeychainError.invalidKeyLength(existing.count)
                }
                return MasterKeyResult(key: existing, source: .hardwareKeychain, newlyGenerated: false)
            }
        } catch {
            if fallbackDirectory == nil {
                throw error
            }
            keychainUnavailable = true
        }

        // If a protected fallback key already exists (e.g. from an unsigned local build),
        // prefer reusing it directly rather than generating a conflicting new key.
        if let dir = fallbackDirectory, let migrated = readProtectedFallbackKey(in: dir) {
            if !keychainUnavailable, (try? writeKeychainData(migrated, account: Self.masterKeyAccount)) != nil {
                try? FileManager.default.removeItem(at: fallbackKeyURL(in: dir))
                return MasterKeyResult(key: migrated, source: .hardwareKeychain, newlyGenerated: false)
            }
            return MasterKeyResult(key: migrated, source: .protectedFileFallback, newlyGenerated: false)
        }

        let fresh = try Self.generateRandomBytes(count: Self.masterKeyByteCount)
        if !keychainUnavailable {
            do {
                try writeKeychainData(fresh, account: Self.masterKeyAccount)
                return MasterKeyResult(key: fresh, source: .hardwareKeychain, newlyGenerated: true)
            } catch {
                if let dir = fallbackDirectory {
                    try writeProtectedFallbackKey(fresh, in: dir)
                    return MasterKeyResult(key: fresh, source: .protectedFileFallback, newlyGenerated: true)
                }
                throw error
            }
        } else if let dir = fallbackDirectory {
            try writeProtectedFallbackKey(fresh, in: dir)
            return MasterKeyResult(key: fresh, source: .protectedFileFallback, newlyGenerated: true)
        }
        throw OrbitKeychainError.keychainStatus(errSecMissingEntitlement)
    }

    public func readExistingProtectedFallbackKey(in directory: URL) -> Data? {
        readProtectedFallbackKey(in: directory)
    }

    /// Returns a stable per-install push device identifier persisted in Keychain (or UserDefaults fallback).
    public func stablePushDeviceId() -> String {
        if let data = try? readKeychainData(account: Self.pushDeviceIdAccount),
           let id = String(data: data, encoding: .utf8),
           !id.isEmpty {
            return id
        }
        let defaultsKey = "orbit.ios.push.deviceId.v1"
        if let existing = UserDefaults.standard.string(forKey: defaultsKey), !existing.isEmpty {
            if let bytes = existing.data(using: .utf8) {
                try? writeKeychainData(bytes, account: Self.pushDeviceIdAccount)
            }
            return existing
        }
        let created = "ios-\(UUID().uuidString.lowercased())"
        if let bytes = created.data(using: .utf8) {
            try? writeKeychainData(bytes, account: Self.pushDeviceIdAccount)
        }
        UserDefaults.standard.set(created, forKey: defaultsKey)
        return created
    }

    public static func generateRandomBytes(count: Int) throws -> Data {
        var bytes = Data(count: count)
        let status: OSStatus = bytes.withUnsafeMutableBytes { rawBuf in
            guard let ptr = rawBuf.baseAddress else { return errSecParam }
            return SecRandomCopyBytes(kSecRandomDefault, count, ptr)
        }
        guard status == errSecSuccess else {
            throw OrbitKeychainError.csprngFailed(status)
        }
        return bytes
    }

    // MARK: - Keychain Primitives

    private func readKeychainData(account: String) throws -> Data? {
        let query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
            kSecMatchLimit: kSecMatchLimitOne,
            kSecReturnData: true
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound {
            return nil
        }
        if status == errSecMissingEntitlement {
            return nil
        }
        guard status == errSecSuccess else {
            throw OrbitKeychainError.keychainStatus(status)
        }
        return result as? Data
    }

    private func writeKeychainData(_ data: Data, account: String) throws {
        let baseQuery: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account
        ]
        let attributes: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
            kSecValueData: data,
            kSecAttrAccessible: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        ]
        let status = SecItemAdd(attributes as CFDictionary, nil)
        if status == errSecDuplicateItem {
            let updateStatus = SecItemUpdate(
                baseQuery as CFDictionary,
                [
                    kSecValueData: data,
                    kSecAttrAccessible: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
                ] as CFDictionary
            )
            guard updateStatus == errSecSuccess else {
                throw OrbitKeychainError.keychainStatus(updateStatus)
            }
            return
        }
        guard status == errSecSuccess else {
            throw OrbitKeychainError.keychainStatus(status)
        }
    }

    // MARK: - Protected File Fallback (Unsigned Sandbox / Unit Test Only)

    private func fallbackKeyURL(in directory: URL) -> URL {
        directory.appendingPathComponent(".orbit_master_key.bin", isDirectory: false)
    }

    private func readProtectedFallbackKey(in directory: URL) -> Data? {
        let url = fallbackKeyURL(in: directory)
        guard let data = try? Data(contentsOf: url), data.count == Self.masterKeyByteCount else {
            return nil
        }
        return data
    }

    private func writeProtectedFallbackKey(_ data: Data, in directory: URL) throws {
        let fm = FileManager.default
        try fm.createDirectory(at: directory, withIntermediateDirectories: true)
        var url = fallbackKeyURL(in: directory)
        try data.write(to: url, options: [.atomic])
        try fm.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        #if os(iOS)
        try? fm.setAttributes(
            [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication],
            ofItemAtPath: url.path
        )
        #endif
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? url.setResourceValues(values)
    }
}
