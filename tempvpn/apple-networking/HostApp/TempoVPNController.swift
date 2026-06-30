#if os(macOS)
import Foundation
import NetworkExtension

public final class TempoVPNController {
    public init() {}

    public func connect(profile: TempoVPNProfile) async throws {
        let manager = try await installOrUpdate(profile: profile)
        guard let session = manager.connection as? NETunnelProviderSession else {
            throw TempoVPNControllerError.invalidTunnelSession
        }

        try session.startTunnel(options: profile.startOptions)
    }

    public func disconnect(tunnelName: String = "Tempo VPN") async throws {
        let manager = try await loadManager(named: tunnelName)
        manager?.connection.stopVPNTunnel()
    }

    @discardableResult
    public func installOrUpdate(profile: TempoVPNProfile) async throws -> NETunnelProviderManager {
        let manager = try await loadManager(named: profile.tunnelName) ?? NETunnelProviderManager()

        let tunnelProtocol = NETunnelProviderProtocol()
        tunnelProtocol.providerBundleIdentifier = profile.providerBundleIdentifier
        tunnelProtocol.serverAddress = profile.tunnelName
        tunnelProtocol.disconnectOnSleep = false
        tunnelProtocol.providerConfiguration = [
            TempoVPNProviderKeys.tunnelName: profile.tunnelName
        ]

        manager.localizedDescription = profile.tunnelName
        manager.protocolConfiguration = tunnelProtocol
        manager.isEnabled = true

        try await save(manager)
        try await load(manager)
        return manager
    }

    private func loadManager(named tunnelName: String) async throws -> NETunnelProviderManager? {
        let managers = try await loadAllManagers()
        return managers.first { manager in
            manager.localizedDescription == tunnelName
        }
    }

    private func loadAllManagers() async throws -> [NETunnelProviderManager] {
        try await withCheckedThrowingContinuation { continuation in
            NETunnelProviderManager.loadAllFromPreferences { managers, error in
                if let error {
                    continuation.resume(throwing: error)
                    return
                }

                continuation.resume(returning: managers ?? [])
            }
        }
    }

    private func save(_ manager: NETunnelProviderManager) async throws {
        try await withCheckedThrowingContinuation { continuation in
            manager.saveToPreferences { error in
                if let error {
                    continuation.resume(throwing: error)
                    return
                }

                continuation.resume()
            }
        }
    }

    private func load(_ manager: NETunnelProviderManager) async throws {
        try await withCheckedThrowingContinuation { continuation in
            manager.loadFromPreferences { error in
                if let error {
                    continuation.resume(throwing: error)
                    return
                }

                continuation.resume()
            }
        }
    }
}

public enum TempoVPNControllerError: LocalizedError {
    case invalidTunnelSession

    public var errorDescription: String? {
        switch self {
        case .invalidTunnelSession:
            return "The saved VPN manager did not expose a tunnel provider session."
        }
    }
}
#endif
