import Foundation

enum TempVPNCLIError: LocalizedError {
    case usage(String)
    case invalidResponse
    case requestFailed(Int)
    case keychain(OSStatus)
    case tunnelManagerUnavailable

    var errorDescription: String? {
        switch self {
        case .usage(let message): return message
        case .invalidResponse: return "The paid session response is invalid."
        case .requestFailed(let status): return "The selected node returned HTTP \(status)."
        case .keychain(let status): return "Shared Keychain operation failed with status \(status)."
        case .tunnelManagerUnavailable: return "The TempVPN tunnel profile is not installed."
        }
    }
}
