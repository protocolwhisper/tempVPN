import CryptoKit
import Foundation
import NetworkExtension

private let providerBundleIdentifier = "com.tempo.tempvpn.PacketTunnel"

private struct PaidSession: Decodable {
    let sessionId: String
    let nodeURL: String
    enum CodingKeys: String, CodingKey { case sessionId = "session_id"; case nodeURL = "node_url" }
}

private struct ActiveSession: Decodable {
    let sessionId: String
    let nodeURL: String
    let assignedIP: String
    let serverPublicKey: String
    let endpoint: String
    let expectedExitIP: String
    let remainingSeconds: Int
    let notAfter: String
    enum CodingKeys: String, CodingKey {
        case sessionId = "session_id"; case nodeURL = "node_url"; case assignedIP = "assigned_ip"
        case serverPublicKey = "server_public_key"; case endpoint; case expectedExitIP = "expected_exit_ip"
        case remainingSeconds = "remaining_seconds"; case notAfter = "not_after"
    }
}

@main
struct TempVPNCLI {
    static func main() async {
        do { try await run(Array(CommandLine.arguments.dropFirst())) }
        catch {
            fputs("tempvpnctl: \(error.localizedDescription)\n", stderr)
            Foundation.exit(1)
        }
    }

    static func run(_ arguments: [String]) async throws {
        guard let command = arguments.first else { throw TempVPNCLIError.usage(usage) }
        let json = arguments.contains("--json")
        switch command {
        case "select":
            if let nodeURL = option("--node-url", in: arguments) {
                _ = try await medianHealthLatency(apiURL: nodeURL)
                try emit(["node_url": nodeURL], json: json)
                return
            }
            let registry = option("--registry-url", in: arguments)
                ?? ProcessInfo.processInfo.environment["VPN_CLIENT_REGISTRY_URL"]
            guard let registry else {
                throw TempVPNCLIError.usage(
                    "Set --registry-url or VPN_CLIENT_REGISTRY_URL before selecting a node."
                )
            }
            let node = try await selectNode(registryURL: registry, region: option("--region", in: arguments))
            try emit(["node_url": node.apiURL, "node_id": node.id, "region": node.region], json: json)
        case "connect":
            guard let index = arguments.firstIndex(of: "--session-response"),
                  arguments.indices.contains(index + 1) else { throw TempVPNCLIError.usage(usage) }
            try await connect(responsePath: arguments[index + 1], json: json)
        case "status": try await status(json: json)
        case "disconnect": try await disconnect(json: json)
        default: throw TempVPNCLIError.usage(usage)
        }
    }

    static func option(_ name: String, in arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: name), arguments.indices.contains(index + 1) else { return nil }
        return arguments[index + 1]
    }

    static func connect(responsePath: String, json: Bool) async throws {
        let decoder = JSONDecoder()
        let paid = try decoder.decode(PaidSession.self, from: readInput(responsePath))
        let privateKey = Curve25519.KeyAgreement.PrivateKey()
        try storePrivateKey(privateKey.rawRepresentation, account: paid.sessionId)

        let publicKey = privateKey.publicKey.rawRepresentation.base64EncodedString()
        let url = URL(string: "\(paid.nodeURL)/sessions/\(paid.sessionId)/connect")!
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["client_public_key": publicKey])
        let (data, response) = try await URLSession.shared.data(for: request)
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else { throw TempVPNCLIError.requestFailed(code) }
        let active = try decoder.decode(ActiveSession.self, from: data)
        guard active.nodeURL.trimmingCharacters(in: CharacterSet(charactersIn: "/")) ==
                paid.nodeURL.trimmingCharacters(in: CharacterSet(charactersIn: "/")) else {
            throw TempVPNCLIError.invalidResponse
        }

        let wgQuick = """
        [Interface]
        PrivateKey = keychain:\(paid.sessionId)
        Address = \(active.assignedIP)
        DNS = 1.1.1.1

        [Peer]
        PublicKey = \(active.serverPublicKey)
        Endpoint = \(active.endpoint)
        AllowedIPs = 0.0.0.0/0, ::/0
        PersistentKeepalive = 25
        """
        let managers = try await loadManagers()
        let manager = managers.first { $0.localizedDescription == "TempVPN" } ?? NETunnelProviderManager()
        let tunnel = NETunnelProviderProtocol()
        tunnel.providerBundleIdentifier = providerBundleIdentifier
        tunnel.serverAddress = active.endpoint
        tunnel.providerConfiguration = [
            "tunnelName": "TempVPN", "wgQuickConfig": wgQuick,
            "sessionId": paid.sessionId, "nodeURL": paid.nodeURL,
            "remainingSeconds": active.remainingSeconds, "notAfter": active.notAfter,
        ]
        manager.localizedDescription = "TempVPN"
        manager.protocolConfiguration = tunnel
        manager.isEnabled = true
        try await saveManager(manager)
        try manager.connection.startVPNTunnel()
        try emit(["state": "connecting", "session_id": paid.sessionId, "node_url": paid.nodeURL], json: json)
    }

    static func status(json: Bool) async throws {
        guard let manager = try await loadManagers().first(where: { $0.localizedDescription == "TempVPN" })
        else { throw TempVPNCLIError.tunnelManagerUnavailable }
        try emit(["status": String(describing: manager.connection.status)], json: json)
    }

    static func disconnect(json: Bool) async throws {
        guard let manager = try await loadManagers().first(where: { $0.localizedDescription == "TempVPN" })
        else { throw TempVPNCLIError.tunnelManagerUnavailable }
        manager.connection.stopVPNTunnel()
        try emit(["status": "disconnecting"], json: json)
    }

    static let usage = "tempvpnctl select [--registry-url <url>] [--region <region>] [--node-url <url>] [--json]\ntempvpnctl connect --session-response <path|-> [--region <region>] [--json]\ntempvpnctl status [--json]\ntempvpnctl disconnect [--json]"
}
