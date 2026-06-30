import Foundation
import NetworkExtension

#if canImport(WireGuardKit)
import WireGuardKit
#endif

final class PacketTunnelProvider: NEPacketTunnelProvider {
#if canImport(WireGuardKit)
    private var adapter: WireGuardAdapter?
#endif

    override func startTunnel(
        options: [String: NSObject]?,
        completionHandler: @escaping (Error?) -> Void
    ) {
        guard let wgQuickConfig = options?[TempoVPNProviderKeys.wgQuickConfig] as? String else {
            completionHandler(TempoPacketTunnelError.missingWireGuardConfig)
            return
        }

        let tunnelName = options?[TempoVPNProviderKeys.tunnelName] as? String ?? "Tempo VPN"

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
#if canImport(WireGuardKit)
        adapter?.stop { _ in
            self.adapter = nil
            completionHandler()
        }
#else
        completionHandler()
#endif
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
