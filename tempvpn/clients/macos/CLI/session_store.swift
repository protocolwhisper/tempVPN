import Foundation

private struct SavedCapability: Codable {
    let sessionId: String
    let registryURL: String
    var nodeURL: String?
    var logicalNode: String?
    var notAfter: String?
    var remainingSeconds: Int?
    var state: String?

    enum CodingKeys: String, CodingKey {
        case sessionId = "session_id"
        case registryURL = "registry_url"
        case nodeURL = "node_url"
        case logicalNode = "logical_node"
        case notAfter = "not_after"
        case remainingSeconds = "remaining_seconds"
        case state
    }

    var paidSession: PaidSession {
        PaidSession(
            sessionId: sessionId,
            nodeURL: nodeURL,
            logicalNode: logicalNode,
            notAfter: notAfter,
            remainingSeconds: remainingSeconds,
            state: state
        )
    }
}

private struct SavedCapabilityFile: Codable {
    var sessions: [SavedCapability]
}

func capabilityStoreURL() throws -> URL {
    try applicationSupportDirectory().appendingPathComponent("sessions.json")
}

private func loadCapabilities() throws -> [SavedCapability] {
    let url = try capabilityStoreURL()
    guard FileManager.default.fileExists(atPath: url.path) else { return [] }
    return try JSONDecoder().decode(SavedCapabilityFile.self, from: Data(contentsOf: url)).sessions
}

private func writeCapabilities(_ sessions: [SavedCapability]) throws {
    let url = try capabilityStoreURL()
    let data = try JSONEncoder().encode(SavedCapabilityFile(sessions: sessions))
    try data.write(to: url, options: [.atomic, .completeFileProtection])
    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
}

func saveCapability(_ paid: PaidSession, registryURL: String) throws {
    var sessions = try loadCapabilities()
    sessions.removeAll { $0.sessionId == paid.sessionId }
    sessions.append(SavedCapability(
        sessionId: paid.sessionId,
        registryURL: registryURL,
        nodeURL: paid.nodeURL,
        logicalNode: paid.logicalNode,
        notAfter: paid.notAfter,
        remainingSeconds: paid.remainingSeconds,
        state: paid.state
    ))
    try writeCapabilities(sessions)
}

func savedCapabilityCount() throws -> Int {
    try loadCapabilities().count
}

func updateCapability(_ status: PortableSession, registryURL: String) throws {
    var sessions = try loadCapabilities()
    guard let index = sessions.firstIndex(where: { $0.sessionId == status.sessionId }) else {
        return
    }
    sessions[index].notAfter = status.notAfter
    sessions[index].remainingSeconds = status.remainingSeconds
    sessions[index].state = status.state
    try writeCapabilities(sessions)
}

func reusableCapability(
    registryURL: String,
    requiredSeconds: Int
) async throws -> PaidSession? {
    for saved in try loadCapabilities() where saved.registryURL == registryURL {
        guard (saved.remainingSeconds ?? 0) >= requiredSeconds else { continue }
        do {
            let status = try await fetchSession(
                registryURL: registryURL,
                sessionId: saved.sessionId
            )
            try updateCapability(status, registryURL: registryURL)
            if status.state == "paused", status.remainingSeconds >= requiredSeconds {
                return SavedCapability(
                    sessionId: saved.sessionId,
                    registryURL: registryURL,
                    nodeURL: saved.nodeURL,
                    logicalNode: saved.logicalNode,
                    notAfter: status.notAfter,
                    remainingSeconds: status.remainingSeconds,
                    state: status.state
                ).paidSession
            }
        } catch TempVPNCLIError.requestFailed(let status, _) where status == 404 {
            continue
        }
    }
    return nil
}
