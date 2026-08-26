import Foundation
import NetworkExtension
import Security

#if canImport(WireGuardKit)
import WireGuardKit
#endif

final class PacketTunnelProvider: NEPacketTunnelProvider {
#if canImport(WireGuardKit)
    private var adapter: WireGuardAdapter?
#endif
    private var heartbeatTask: Task<Void, Never>?
    private var sessionId: String?
    private var nodeURL: String?
    private var consecutiveHeartbeatFailures = 0

    override func startTunnel(
        options: [String: NSObject]?,
        completionHandler: @escaping (Error?) -> Void
    ) {
        let saved = (protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration
        var configuration = saved ?? [:]
        options?.forEach { configuration[$0.key] = $0.value }
        guard let storedConfig = configuration[TempoVPNProviderKeys.wgQuickConfig] as? String,
              let wgQuickConfig = resolvePrivateKey(in: storedConfig) else {
            completionHandler(TempoPacketTunnelError.missingWireGuardConfig)
            return
        }

        let tunnelName = configuration[TempoVPNProviderKeys.tunnelName] as? String ?? "TempVPN"
        sessionId = configuration[TempoVPNProviderKeys.sessionId] as? String
        nodeURL = configuration[TempoVPNProviderKeys.nodeURL] as? String

#if canImport(WireGuardKit)
        do {
            let tunnelConfiguration = try TunnelConfiguration(
                fromWgQuickConfig: wgQuickConfig,
                called: tunnelName
            )
            let adapter = WireGuardAdapter(with: self) { logLevel, message in
                NSLog("tempVPN WireGuardKit [%@]: %@", "\(logLevel)", message)
            }
            self.adapter = adapter

            adapter.start(tunnelConfiguration: tunnelConfiguration) { error in
                if error == nil {
                    self.startHeartbeatLoop()
                }
                completionHandler(error)
            }
        } catch {
            completionHandler(error)
        }
#else
        completionHandler(TempoPacketTunnelError.wireGuardKitMissing)
#endif
    }

    override func stopTunnel(
        with reason: NEProviderStopReason,
        completionHandler: @escaping () -> Void
    ) {
        heartbeatTask?.cancel()
        heartbeatTask = nil
#if canImport(WireGuardKit)
        guard let adapter else {
            pauseSession(completionHandler: completionHandler)
            return
        }
        adapter.stop { _ in
            self.adapter = nil
            self.pauseSession(completionHandler: completionHandler)
        }
#else
        pauseSession(completionHandler: completionHandler)
#endif
    }

    private func startHeartbeatLoop() {
        guard let sessionId, let nodeURL else { return }
        heartbeatTask?.cancel()
        heartbeatTask = Task {
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(30))
                guard !Task.isCancelled else { return }
                let heartbeat = await postSessionAction(
                    "heartbeat",
                    sessionId: sessionId,
                    nodeURL: nodeURL
                )
                switch heartbeat {
                case .active:
                    self.consecutiveHeartbeatFailures = 0
                case .inactive:
                    self.cancelTunnelWithError(TempoPacketTunnelError.sessionUnavailable)
                    return
                case .unavailable:
                    self.consecutiveHeartbeatFailures += 1
                    if self.consecutiveHeartbeatFailures >= 3 {
                        self.cancelTunnelWithError(TempoPacketTunnelError.sessionUnavailable)
                        return
                    }
                }
            }
        }
    }

    private func pauseSession(completionHandler: @escaping () -> Void) {
        guard let sessionId, let nodeURL else {
            completionHandler()
            return
        }
        Task {
            _ = await postSessionAction("pause", sessionId: sessionId, nodeURL: nodeURL)
            completionHandler()
        }
    }

    private func postSessionAction(
        _ action: String,
        sessionId: String,
        nodeURL: String
    ) async -> SessionActionResult {
        guard let url = sessionActionURL(
            nodeURL: nodeURL,
            sessionId: sessionId,
            action: action
        ) else { return .unavailable }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 5
        guard let (data, response) = try? await URLSession.shared.data(for: request),
              let http = response as? HTTPURLResponse else { return .unavailable }
        guard (200..<300).contains(http.statusCode) else {
            return (400..<500).contains(http.statusCode) ? .inactive : .unavailable
        }
        if action == "heartbeat",
           let state = sessionState(from: data),
           state != "active" {
            return .inactive
        }
        return .active
    }

    private func resolvePrivateKey(in configuration: String) -> String? {
        guard let marker = configuration
            .split(separator: "\n")
            .map(String.init)
            .first(where: { $0.trimmingCharacters(in: .whitespaces).hasPrefix("PrivateKey = keychain:") }),
              let account = marker.split(separator: ":", maxSplits: 1).last.map(String.init),
              let task = SecTaskCreateFromSelf(nil),
              let groups = SecTaskCopyValueForEntitlement(
                task,
                "keychain-access-groups" as CFString,
                nil
              ) as? [String],
              let group = groups.first(where: { $0.hasSuffix("com.protocolwhisper.tempvpn.shared") })
        else { return nil }
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "com.protocolwhisper.tempvpn.wireguard",
            kSecAttrAccount as String: account,
            kSecAttrAccessGroup as String: group,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data else { return nil }
        let key = data.base64EncodedString()
        return configuration.replacingOccurrences(of: "keychain:\(account)", with: key)
    }
}
