import CryptoKit
import Foundation
import Security

private let keychainService = "com.protocolwhisper.tempvpn.wireguard"
private let sharedGroupSuffix = "com.protocolwhisper.tempvpn.shared"

func loadOrCreateWireGuardPublicKey(sessionId: String) throws -> String {
    let privateKey: Curve25519.KeyAgreement.PrivateKey
    if let raw = try loadPrivateKey(account: sessionId) {
        privateKey = try Curve25519.KeyAgreement.PrivateKey(rawRepresentation: raw)
    } else {
        privateKey = Curve25519.KeyAgreement.PrivateKey()
        try storePrivateKey(privateKey.rawRepresentation, account: sessionId)
    }
    return privateKey.publicKey.rawRepresentation.base64EncodedString()
}

private func sharedKeychainGroup() throws -> String {
    guard let task = SecTaskCreateFromSelf(nil),
          let groups = SecTaskCopyValueForEntitlement(
            task,
            "keychain-access-groups" as CFString,
            nil
          ) as? [String],
          let group = groups.first(where: { $0.hasSuffix(sharedGroupSuffix) }) else {
        throw TempVPNCLIError.sharedKeychainUnavailable
    }
    return group
}

private func keychainQuery(account: String) throws -> [String: Any] {
    [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: keychainService,
        kSecAttrAccount as String: account,
        kSecAttrAccessGroup as String: try sharedKeychainGroup(),
    ]
}

private func loadPrivateKey(account: String) throws -> Data? {
    var query = try keychainQuery(account: account)
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    var value: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &value)
    if status == errSecItemNotFound { return nil }
    guard status == errSecSuccess else { throw TempVPNCLIError.keychain(status) }
    guard let data = value as? Data else { throw TempVPNCLIError.keychainItemNotFound }
    return data
}

private func storePrivateKey(_ key: Data, account: String) throws {
    let query = try keychainQuery(account: account)
    SecItemDelete(query as CFDictionary)
    var item = query
    item[kSecValueData as String] = key
    item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
    let status = SecItemAdd(item as CFDictionary, nil)
    guard status == errSecSuccess else { throw TempVPNCLIError.keychain(status) }
}
