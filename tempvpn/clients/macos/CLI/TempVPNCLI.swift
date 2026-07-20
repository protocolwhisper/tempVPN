import Foundation
import NetworkExtension

@main
struct TempVPNCLI {
    static func main() async {
        do {
            try await run(Array(CommandLine.arguments.dropFirst()))
        } catch {
            fputs("tempvpnctl: \(error.localizedDescription)\n", stderr)
            Foundation.exit(1)
        }
    }

    static func run(_ arguments: [String]) async throws {
        guard let command = arguments.first else { throw TempVPNCLIError.usage(usage) }
        let json = hasFlag("--json", in: arguments)

        switch command {
        case "select": try await select(arguments: arguments, json: json)
        case "connect": try await connect(arguments: arguments, json: json)
        case "status": try await status(json: json)
        case "disconnect": try await disconnect(json: json)
        case "--version", "version": print("tempvpnctl \(tempVPNVersion)")
        case "--help", "-h", "help": print(usage)
        default: throw TempVPNCLIError.usage(usage)
        }
    }

    private static func select(arguments: [String], json: Bool) async throws {
        if let nodeURL = try option("--node-url", in: arguments) {
            let normalized = try normalizedNodeURL(nodeURL)
            let latency = try await medianHealthLatency(apiURL: normalized)
            try emit([
                "node_url": normalized,
                "latency_ms": Int(latency * 1_000),
            ], json: json)
            return
        }

        let registry = try option("--registry-url", in: arguments)
            ?? ProcessInfo.processInfo.environment["VPN_CLIENT_REGISTRY_URL"]
        guard let registry else {
            throw TempVPNCLIError.usage(
                "Set --registry-url or VPN_CLIENT_REGISTRY_URL before selecting a node."
            )
        }
        let node = try await selectNode(
            registryURL: registry,
            region: try option("--region", in: arguments)
        )
        try emit([
            "node_url": node.apiURL,
            "node_id": node.id,
            "name": node.name,
            "region": node.region,
            "expected_exit_ip": node.expectedExitIP,
        ], json: json)
    }

    private static func connect(arguments: [String], json: Bool) async throws {
        guard headlessAppIsInstalled() else { throw TempVPNCLIError.hostAppNotInstalled }
        guard let responsePath = try option("--session-response", in: arguments) else {
            throw TempVPNCLIError.usage("--session-response <path|-> is required.\n\n\(usage)")
        }

        let paid = try JSONDecoder().decode(PaidSession.self, from: readInput(responsePath))
        let paidNodeURL = try normalizedNodeURL(paid.nodeURL)
        if let selectedNodeURL = try option("--node-url", in: arguments),
           try normalizedNodeURL(selectedNodeURL) != paidNodeURL {
            throw TempVPNCLIError.invalidResponse(
                "the paid session belongs to \(paidNodeURL), not \(selectedNodeURL)"
            )
        }

        let publicKey = try loadOrCreateWireGuardPublicKey(sessionId: paid.sessionId)
        let active = try await connectSession(paid, publicKey: publicKey)
        guard try normalizedNodeURL(active.nodeURL) == paidNodeURL else {
            _ = try? await pauseSession(nodeURL: paidNodeURL, sessionId: paid.sessionId)
            throw TempVPNCLIError.invalidResponse("the node returned a session owned by another node")
        }
        guard let assignedIP = active.assignedIP, !assignedIP.isEmpty else {
            _ = try? await pauseSession(nodeURL: paidNodeURL, sessionId: paid.sessionId)
            throw TempVPNCLIError.invalidResponse("the node did not assign a tunnel IP")
        }

        let wgQuick = """
        [Interface]
        PrivateKey = keychain:\(paid.sessionId)
        Address = \(assignedIP)
        DNS = 1.1.1.1

        [Peer]
        PublicKey = \(active.serverPublicKey)
        Endpoint = \(active.endpoint)
        AllowedIPs = 0.0.0.0/0, ::/0
        PersistentKeepalive = 25
        """
        let configuration: [String: Any] = [
            TempVPNProviderKey.tunnelName: tempVPNTunnelName,
            TempVPNProviderKey.wgQuickConfig: wgQuick,
            TempVPNProviderKey.sessionId: active.sessionId,
            TempVPNProviderKey.nodeURL: paidNodeURL,
            TempVPNProviderKey.remainingSeconds: active.remainingSeconds,
            TempVPNProviderKey.notAfter: active.notAfter,
            TempVPNProviderKey.assignedIP: assignedIP,
            TempVPNProviderKey.expectedExitIP: active.expectedExitIP,
        ]

        do {
            try await installAndStartTunnel(configuration: configuration)
        } catch {
            _ = try? await pauseSession(nodeURL: paidNodeURL, sessionId: paid.sessionId)
            throw error
        }

        try emit([
            "status": "connected",
            "session_id": active.sessionId,
            "node_url": paidNodeURL,
            "profile": tempVPNTunnelName,
            "assigned_ip": assignedIP,
            "endpoint": active.endpoint,
            "expected_exit_ip": active.expectedExitIP,
            "remaining_seconds": active.remainingSeconds,
            "not_after": active.notAfter,
        ], json: json)
    }

    private static func status(json: Bool) async throws {
        let manager = try await currentTunnelManager()
        let configuration = try tunnelConfiguration(manager)
        guard let sessionId = configuration[TempVPNProviderKey.sessionId] as? String,
              let nodeURL = configuration[TempVPNProviderKey.nodeURL] as? String else {
            throw TempVPNCLIError.invalidTunnelConfiguration
        }
        let server = try? await fetchSession(nodeURL: nodeURL, sessionId: sessionId)
        try emit([
            "status": tunnelStatusName(manager.connection.status),
            "session_id": sessionId,
            "node_url": nodeURL,
            "profile": manager.localizedDescription ?? tempVPNTunnelName,
            "assigned_ip": configuration[TempVPNProviderKey.assignedIP] as? String ?? "unknown",
            "expected_exit_ip": configuration[TempVPNProviderKey.expectedExitIP] as? String ?? "unknown",
            "remaining_seconds": server?.remainingSeconds
                ?? configuration[TempVPNProviderKey.remainingSeconds] as? Int
                ?? 0,
            "not_after": server?.notAfter
                ?? configuration[TempVPNProviderKey.notAfter] as? String
                ?? "unknown",
            "server_state": server?.state ?? "unavailable",
        ], json: json)
    }

    private static func disconnect(json: Bool) async throws {
        let manager = try await currentTunnelManager()
        let configuration = try tunnelConfiguration(manager)
        guard let sessionId = configuration[TempVPNProviderKey.sessionId] as? String,
              let nodeURL = configuration[TempVPNProviderKey.nodeURL] as? String else {
            throw TempVPNCLIError.invalidTunnelConfiguration
        }

        if manager.connection.status != .disconnected {
            try await stopTunnel(manager)
        }
        // The Packet Tunnel pauses during stop. Repeat idempotently so a crash
        // in the extension cannot continue consuming connected-time balance.
        let paused = try await pauseSession(nodeURL: nodeURL, sessionId: sessionId)
        try emit([
            "status": paused.state == "expired" ? "expired" : "paused",
            "session_id": paused.sessionId,
            "remaining_seconds": paused.remainingSeconds,
            "not_after": paused.notAfter,
        ], json: json)
    }

    static let usage = """
    Usage:
      tempvpnctl select [--registry-url <url>] [--region <region>] [--node-url <url>] [--json]
      tempvpnctl connect --session-response <path|-> [--node-url <selected-url>] [--json]
      tempvpnctl status [--json]
      tempvpnctl disconnect [--json]
      tempvpnctl --version

    Payment is separate: select a node, pay that exact node's POST /sessions with
    mppx, then pass the paid JSON response to `tempvpnctl connect`.
    """
}
