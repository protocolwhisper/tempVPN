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
        case "check": try await check(arguments: arguments, json: json)
        case "connect": try await connect(arguments: arguments, json: json)
        case "status": try await status(json: json)
        case "disconnect": try await disconnect(json: json)
        case "--version", "version": print("tempvpnctl \(tempVPNVersion)")
        case "--help", "-h", "help": print(usage)
        default: throw TempVPNCLIError.usage(usage)
        }
    }

    private static func select(arguments: [String], json: Bool) async throws {
        let policy = try option("--selection-policy", in: arguments) ?? "lowest-latency"
        guard ["lowest-latency", "lowest_latency"].contains(policy) else {
            throw TempVPNCLIError.usage(
                "Only --selection-policy lowest-latency is currently supported."
            )
        }
        if let nodeURL = try option("--node-url", in: arguments) {
            let normalized = try normalizedNodeURL(nodeURL)
            let latency = try await medianHealthLatency(apiURL: normalized)
            try await checkNodeAvailability(apiURL: normalized)
            try emit([
                "node_url": normalized,
                "latency_ms": Int(latency * 1_000),
                "selection_policy": "lowest_latency",
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
        let filters = try DiscoveryFilters(
            country: try option("--country", in: arguments),
            city: try option("--city", in: arguments),
            region: try option("--region", in: arguments)
        )
        let node = try await selectNode(registryURL: registry, filters: filters)
        var result: [String: Any] = [
            "node_url": node.apiURL,
            "node_id": node.id,
            "node_name": node.name,
            "region": node.region,
            "expected_exit_ip": node.expectedExitIP,
            "selection_policy": "lowest_latency",
        ]
        if let countryCode = node.countryCode { result["country_code"] = countryCode }
        if let subdivisionCode = node.subdivisionCode {
            result["subdivision_code"] = subdivisionCode
        }
        if let city = node.city { result["city"] = city }
        try emit(result, json: json)
    }

    private static func check(arguments: [String], json: Bool) async throws {
        guard let rawNodeURL = try option("--node-url", in: arguments) else {
            throw TempVPNCLIError.usage("check requires --node-url <selected-url>.")
        }
        let nodeURL = try normalizedNodeURL(rawNodeURL)
        let health = try await fetchNodeHealth(apiURL: nodeURL)
        try validateNodeHealthForPayment(health)
        try emit([
            "status": "available",
            "node_url": nodeURL,
            "accepting_sessions": health.acceptingSessions ?? false,
            "available_slots": health.availableSlots ?? 0,
        ], json: json)
    }

    private static func connect(arguments: [String], json: Bool) async throws {
        guard headlessAppIsInstalled() else { throw TempVPNCLIError.hostAppNotInstalled }
        guard let responsePath = try option("--session-response", in: arguments) else {
            throw TempVPNCLIError.usage("--session-response <path|-> is required.\n\n\(usage)")
        }

        let paid = try JSONDecoder().decode(PaidSession.self, from: readInput(responsePath))
        let paidNodeURL = try enforceSelectedNode(
            selectedNodeURL: try option("--node-url", in: arguments),
            paidNodeURL: paid.nodeURL
        )

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

        let wgQuick = wireGuardQuickConfiguration(
            sessionId: paid.sessionId,
            assignedIP: assignedIP,
            serverPublicKey: active.serverPublicKey,
            endpoint: active.endpoint
        )
        var configuration: [String: Any] = [
            TempVPNProviderKey.tunnelName: tempVPNTunnelName,
            TempVPNProviderKey.wgQuickConfig: wgQuick,
            TempVPNProviderKey.sessionId: active.sessionId,
            TempVPNProviderKey.nodeURL: paidNodeURL,
            TempVPNProviderKey.remainingSeconds: active.remainingSeconds,
            TempVPNProviderKey.notAfter: active.notAfter,
            TempVPNProviderKey.assignedIP: assignedIP,
            TempVPNProviderKey.expectedExitIP: active.expectedExitIP,
        ]
        let selectedNodeName = try option("--node-name", in: arguments)
        let selectedCountryCode = try option("--country-code", in: arguments)
        let selectedSubdivisionCode = try option("--subdivision-code", in: arguments)
        let selectedCity = try option("--city", in: arguments)
        let selectedRegion = try option("--region", in: arguments)
        if let selectedNodeName { configuration[TempVPNProviderKey.nodeName] = selectedNodeName }
        if let selectedCountryCode {
            configuration[TempVPNProviderKey.countryCode] = selectedCountryCode
        }
        if let selectedSubdivisionCode {
            configuration[TempVPNProviderKey.subdivisionCode] = selectedSubdivisionCode
        }
        if let selectedCity { configuration[TempVPNProviderKey.city] = selectedCity }
        if let selectedRegion { configuration[TempVPNProviderKey.region] = selectedRegion }

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
            "node_name": selectedNodeName ?? "unknown",
            "country_code": selectedCountryCode ?? "unknown",
            "city": selectedCity ?? "unknown",
            "region": selectedRegion ?? "unknown",
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
        let remainingSeconds = server?.remainingSeconds
            ?? configuration[TempVPNProviderKey.remainingSeconds] as? Int
            ?? 0
        let notAfter = server?.notAfter
            ?? configuration[TempVPNProviderKey.notAfter] as? String
            ?? "unknown"
        let result: [String: Any] = [
            "status": tunnelStatusName(manager.connection.status),
            "session_id": sessionId,
            "node_url": nodeURL,
            "profile": manager.localizedDescription ?? tempVPNTunnelName,
            "assigned_ip": configuration[TempVPNProviderKey.assignedIP] as? String ?? "unknown",
            "expected_exit_ip": configuration[TempVPNProviderKey.expectedExitIP] as? String ?? "unknown",
            "remaining_seconds": remainingSeconds,
            "not_after": notAfter,
            "server_state": server?.state ?? "unavailable",
            "node_name": configuration[TempVPNProviderKey.nodeName] as? String ?? "unknown",
            "country_code": configuration[TempVPNProviderKey.countryCode] as? String ?? "unknown",
            "city": configuration[TempVPNProviderKey.city] as? String ?? "unknown",
            "region": configuration[TempVPNProviderKey.region] as? String ?? "unknown",
        ]
        try emit(result, json: json)
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
      tempvpnctl select [--registry-url <url>] [--country <ISO-2>] [--city <city>]
                        [--region <region>] [--selection-policy lowest-latency]
                        [--node-url <url>] [--json]
      tempvpnctl check --node-url <selected-url> [--json]
      tempvpnctl connect --session-response <path|-> --node-url <selected-url>
                         [--node-name <name>] [--country-code <ISO-2>]
                         [--subdivision-code <code>] [--city <city>]
                         [--region <region>] [--json]
      tempvpnctl status [--json]
      tempvpnctl disconnect [--json]
      tempvpnctl --version

    Payment is separate: select a node, pay that exact node's POST /sessions with
    mppx, then pass the paid JSON response to `tempvpnctl connect`. Run `check`
    immediately before mppx so a draining or full node fails before payment.
    """
}

func wireGuardQuickConfiguration(
    sessionId: String,
    assignedIP: String,
    serverPublicKey: String,
    endpoint: String
) -> String {
    """
    [Interface]
    PrivateKey = keychain:\(sessionId)
    Address = \(assignedIP)
    DNS = 1.1.1.1

    [Peer]
    PublicKey = \(serverPublicKey)
    Endpoint = \(endpoint)
    AllowedIPs = 0.0.0.0/0, ::/0
    PersistentKeepalive = 25
    """
}
