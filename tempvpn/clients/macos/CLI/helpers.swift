import Foundation
import NetworkExtension
import Security

struct CatalogNode: Codable, Sendable {
    let id: String
    let name: String
    let region: String
    let apiURL: String
    let wireguardEndpoint: String
    let expectedExitIP: String
    let leaseExpiresAt: Date
    enum CodingKeys: String, CodingKey {
        case id, name, region
        case apiURL = "api_url"; case wireguardEndpoint = "wireguard_endpoint"
        case expectedExitIP = "expected_exit_ip"; case leaseExpiresAt = "lease_expires_at"
    }
}

private struct CatalogCache: Codable {
    let fetchedAt: Date
    let nodes: [CatalogNode]
}

let tempVPNKeychainService = "com.protocolwhisper.tempvpn.wireguard"

func tempVPNKeychainGroup() -> String? {
    guard let task = SecTaskCreateFromSelf(nil),
          let groups = SecTaskCopyValueForEntitlement(
            task,
            "keychain-access-groups" as CFString,
            nil
          ) as? [String] else { return nil }
    return groups.first { $0.hasSuffix("com.protocolwhisper.tempvpn.shared") }
}

func readInput(_ path: String) throws -> Data {
    if path == "-" { return FileHandle.standardInput.readDataToEndOfFile() }
    return try Data(contentsOf: URL(fileURLWithPath: path))
}

func storePrivateKey(_ data: Data, account: String) throws {
    var base: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: tempVPNKeychainService,
        kSecAttrAccount as String: account,
    ]
    if let group = tempVPNKeychainGroup() { base[kSecAttrAccessGroup as String] = group }
    SecItemDelete(base as CFDictionary)
    var item = base
    item[kSecValueData as String] = data
    item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
    let status = SecItemAdd(item as CFDictionary, nil)
    guard status == errSecSuccess else { throw TempVPNCLIError.keychain(status) }
}

func loadManagers() async throws -> [NETunnelProviderManager] {
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<[NETunnelProviderManager], Error>) in
        NETunnelProviderManager.loadAllFromPreferences { managers, error in
            if let error { continuation.resume(throwing: error) }
            else { continuation.resume(returning: managers ?? []) }
        }
    }
}

func saveManager(_ manager: NETunnelProviderManager) async throws {
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
        manager.saveToPreferences { error in
            if let error { continuation.resume(throwing: error) }
            else { continuation.resume() }
        }
    }
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
        manager.loadFromPreferences { error in
            if let error { continuation.resume(throwing: error) }
            else { continuation.resume() }
        }
    }
}

func emit(_ value: Any, json: Bool) throws {
    if json {
        let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
        print(String(decoding: data, as: UTF8.self))
    } else if let dictionary = value as? [String: Any] {
        dictionary.sorted { $0.key < $1.key }.forEach { print("\($0.key): \($0.value)") }
    } else {
        print(value)
    }
}

func selectNode(registryURL: String, region: String?) async throws -> CatalogNode {
    let cacheURL = FileManager.default.temporaryDirectory.appendingPathComponent("tempvpn-macos-nodes.json")
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .iso8601
    let nodes: [CatalogNode]
    do {
        let url = URL(string: "\(registryURL.trimmingCharacters(in: CharacterSet(charactersIn: "/")))/nodes")!
        let (data, response) = try await URLSession.shared.data(from: url)
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else { throw TempVPNCLIError.requestFailed(code) }
        nodes = try decoder.decode([CatalogNode].self, from: data)
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        try? encoder.encode(CatalogCache(fetchedAt: Date(), nodes: nodes)).write(to: cacheURL, options: .atomic)
    } catch {
        let cache = try decoder.decode(CatalogCache.self, from: Data(contentsOf: cacheURL))
        guard Date().timeIntervalSince(cache.fetchedAt) <= 86_400 else { throw error }
        nodes = cache.nodes
    }
    let candidates = nodes.filter { node in
        node.leaseExpiresAt > Date() && (region == nil || node.region == region)
    }
    let ranked = await withTaskGroup(of: (TimeInterval, CatalogNode)?.self) { group in
        for node in candidates {
            group.addTask { (try? await medianHealthLatency(node)) }
        }
        var results: [(TimeInterval, CatalogNode)] = []
        for await result in group { if let result { results.append(result) } }
        return results.sorted { $0.0 < $1.0 }
    }
    guard let selected = ranked.first?.1 else { throw TempVPNCLIError.usage("No healthy VPN nodes were found.") }
    return selected
}

private func medianHealthLatency(_ node: CatalogNode) async throws -> (TimeInterval, CatalogNode) {
    return (try await medianHealthLatency(apiURL: node.apiURL), node)
}

func medianHealthLatency(apiURL: String) async throws -> TimeInterval {
    var samples: [TimeInterval] = []
    for _ in 0..<3 {
        let base = apiURL.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        var request = URLRequest(url: URL(string: "\(base)/health")!)
        request.timeoutInterval = 2
        let start = Date()
        let (_, response) = try await URLSession.shared.data(for: request)
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else { throw TempVPNCLIError.requestFailed(code) }
        samples.append(Date().timeIntervalSince(start))
    }
    samples.sort()
    return samples[1]
}
