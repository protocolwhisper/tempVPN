import Foundation

enum TempVPNCLIError: LocalizedError {
    case usage(String)
    case invalidResponse(String)
    case invalidURL(String)
    case requestFailed(status: Int, message: String?)
    case keychain(OSStatus)
    case keychainItemNotFound
    case sharedKeychainUnavailable
    case noHealthyNodes
    case alreadyConnected
    case hostAppNotInstalled
    case tunnelManagerUnavailable
    case invalidTunnelConfiguration
    case tunnelStartFailed
    case tunnelOperationTimedOut

    var errorDescription: String? {
        switch self {
        case .usage(let message): return message
        case .invalidResponse(let message): return "The paid session response is invalid: \(message)"
        case .invalidURL(let value): return "Invalid URL: \(value)"
        case .requestFailed(let status, let message):
            if let message, !message.isEmpty {
                return "The selected node returned HTTP \(status): \(message)"
            }
            return "The selected node returned HTTP \(status)."
        case .keychain(let status): return "Keychain operation failed with status \(status)."
        case .keychainItemNotFound: return "The WireGuard private key was not found in Keychain."
        case .sharedKeychainUnavailable:
            return "tempvpnctl is not signed for the TempVPN shared Keychain group. Sign the CLI and headless app with the same Apple team."
        case .noHealthyNodes: return "No healthy VPN nodes were found."
        case .alreadyConnected: return "TempVPN is already connected. Disconnect it before connecting again."
        case .hostAppNotInstalled:
            return "The headless TempVPN.app and Packet Tunnel extension are not installed."
        case .tunnelManagerUnavailable: return "The TempVPN VPN profile is not installed."
        case .invalidTunnelConfiguration: return "The TempVPN VPN profile is missing its node or session configuration."
        case .tunnelStartFailed: return "The TempVPN Packet Tunnel extension failed to start."
        case .tunnelOperationTimedOut: return "Timed out waiting for the TempVPN connection state to change."
        }
    }
}
