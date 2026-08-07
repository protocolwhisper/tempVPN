import Foundation

enum TempoPacketTunnelError: LocalizedError {
    case missingWireGuardConfig
    case wireGuardKitMissing
    case sessionUnavailable

    var errorDescription: String? {
        switch self {
        case .missingWireGuardConfig:
            return "The packet tunnel was started without a WireGuard configuration."
        case .wireGuardKitMissing:
            return "WireGuardKit is not linked into the Packet Tunnel Provider target."
        case .sessionUnavailable:
            return "The paid TempVPN session is no longer active."
        }
    }
}
