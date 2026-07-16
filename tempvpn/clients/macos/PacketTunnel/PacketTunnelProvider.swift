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

    override func startTunnel(
        options: [String: NSObject]?,
        completionHandler: @escaping (Error?) -> Void
    ) {
        let saved = (protocolConfiguration as? NETunnelProviderProtocol)?.providerConfiguration
        let configuration = options ?? saved
        guard let storedConfig = configuration?[TempoVPNProviderKeys.wgQuickConfig] as? String,
              let wgQuickConfig = resolvePrivateKey(in: storedConfig) else {
            completionHandler(TempoPacketTunnelError.missingWireGuardConfig)
            return
        }

        let tunnelName = configuration?[TempoVPNProviderKeys.tunnelName] as? String ?? "TempVPN"
        sessionId = configuration?[TempoVPNProviderKeys.sessionId] as? String
        nodeURL = configuration?[TempoVPNProviderKeys.nodeURL] as? String

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
                await postSessionAction("heartbeat", sessionId: sessionId, nodeURL: nodeURL)
            }
        }
    }

    private func pauseSession(completionHandler: @escaping () -> Void) {
        guard let sessionId, let nodeURL else {
            completionHandler()
            return
        }
        Task {
            await postSessionAction("pause", sessionId: sessionId, nodeURL: nodeURL)
            completionHandler()
        }
    }

    private func postSessionAction(_ action: String, sessionId: String, nodeURL: String) async {
        guard let url = URL(
            string: "\(nodeURL.trimmingCharacters(in: CharacterSet(charactersIn: "/")))/sessions/\(sessionId)/\(action)"
        ) else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        _ = try? await URLSession.shared.data(for: request)
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

enum TempoPacketTunnelError: LocalizedError {
    case missingWireGuardConfig
    case wireGuardKitMissing

    var errorDescription: String? {
        switch self {
        case .missingWireGuardConfig:
            return "The packet tunnel was started without a WireGuard configuration."
        case .wireGuardKitMissing:
            return "WireGuardKit is not linked into the Packet Tunnel Provider target."
        }
    }
}
