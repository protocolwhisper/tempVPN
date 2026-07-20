import Foundation
import NetworkExtension

let tempVPNProviderBundleIdentifier = "com.tempo.tempvpn.PacketTunnel"
let tempVPNTunnelName = "TempVPN"

enum TempVPNProviderKey {
    static let tunnelName = "tunnelName"
    static let wgQuickConfig = "wgQuickConfig"
    static let notAfter = "notAfter"
    static let remainingSeconds = "remainingSeconds"
    static let sessionId = "sessionId"
    static let nodeURL = "nodeURL"
    static let assignedIP = "assignedIP"
    static let expectedExitIP = "expectedExitIP"
}

func installAndStartTunnel(configuration: [String: Any]) async throws {
    let managers = try await loadTunnelManagers()
    let manager = managers.first { $0.localizedDescription == tempVPNTunnelName }
        ?? NETunnelProviderManager()
    if [.connected, .connecting, .reasserting].contains(manager.connection.status) {
        throw TempVPNCLIError.alreadyConnected
    }

    let tunnel = NETunnelProviderProtocol()
    tunnel.providerBundleIdentifier = tempVPNProviderBundleIdentifier
    tunnel.serverAddress = tempVPNTunnelName
    tunnel.disconnectOnSleep = false
    tunnel.providerConfiguration = configuration

    manager.localizedDescription = tempVPNTunnelName
    manager.protocolConfiguration = tunnel
    manager.isEnabled = true
    try await saveTunnelManager(manager)
    try await reloadTunnelManager(manager)
    try manager.connection.startVPNTunnel()
    try await waitForTunnel(manager, target: .connected)
}

func currentTunnelManager() async throws -> NETunnelProviderManager {
    guard let manager = try await loadTunnelManagers().first(where: {
        $0.localizedDescription == tempVPNTunnelName
    }) else {
        throw TempVPNCLIError.tunnelManagerUnavailable
    }
    return manager
}

func stopTunnel(_ manager: NETunnelProviderManager) async throws {
    manager.connection.stopVPNTunnel()
    try await waitForTunnel(manager, target: .disconnected)
}

func tunnelConfiguration(_ manager: NETunnelProviderManager) throws -> [String: Any] {
    guard let tunnel = manager.protocolConfiguration as? NETunnelProviderProtocol,
          let configuration = tunnel.providerConfiguration else {
        throw TempVPNCLIError.invalidTunnelConfiguration
    }
    return configuration
}

func tunnelStatusName(_ status: NEVPNStatus) -> String {
    switch status {
    case .invalid: return "invalid"
    case .disconnected: return "disconnected"
    case .connecting: return "connecting"
    case .connected: return "connected"
    case .reasserting: return "reasserting"
    case .disconnecting: return "disconnecting"
    @unknown default: return "unknown"
    }
}

private func loadTunnelManagers() async throws -> [NETunnelProviderManager] {
    try await withCheckedThrowingContinuation { continuation in
        NETunnelProviderManager.loadAllFromPreferences { managers, error in
            if let error { continuation.resume(throwing: error) }
            else { continuation.resume(returning: managers ?? []) }
        }
    }
}

private func saveTunnelManager(_ manager: NETunnelProviderManager) async throws {
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
        manager.saveToPreferences { error in
            if let error { continuation.resume(throwing: error) }
            else { continuation.resume() }
        }
    }
}

private func reloadTunnelManager(_ manager: NETunnelProviderManager) async throws {
    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
        manager.loadFromPreferences { error in
            if let error { continuation.resume(throwing: error) }
            else { continuation.resume() }
        }
    }
}

private func waitForTunnel(
    _ manager: NETunnelProviderManager,
    target: NEVPNStatus,
    timeoutSeconds: Int = 30
) async throws {
    for _ in 0..<(timeoutSeconds * 4) {
        let status = manager.connection.status
        if status == target { return }
        if target == .connected && [.invalid, .disconnected].contains(status) {
            throw TempVPNCLIError.tunnelStartFailed
        }
        try await Task.sleep(for: .milliseconds(250))
    }
    throw TempVPNCLIError.tunnelOperationTimedOut
}
