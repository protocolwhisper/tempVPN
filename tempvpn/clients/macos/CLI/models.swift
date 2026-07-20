import Foundation

struct CatalogNode: Codable, Sendable {
    let id: String
    let name: String
    let region: String
    let apiURL: String
    let wireguardEndpoint: String
    let expectedExitIP: String
    let leaseExpiresAt: Date

    enum CodingKeys: String, CodingKey {
        case id, name, region
        case apiURL = "api_url"
        case wireguardEndpoint = "wireguard_endpoint"
        case expectedExitIP = "expected_exit_ip"
        case leaseExpiresAt = "lease_expires_at"
    }
}

struct CatalogCache: Codable {
    let fetchedAt: Date
    let nodes: [CatalogNode]

    enum CodingKeys: String, CodingKey {
        case fetchedAt = "fetched_at"
        case nodes
    }
}

struct PaidSession: Codable {
    let sessionId: String
    let nodeURL: String
    let notAfter: String?
    let remainingSeconds: Int?
    let state: String?

    enum CodingKeys: String, CodingKey {
        case sessionId = "session_id"
        case nodeURL = "node_url"
        case notAfter = "not_after"
        case remainingSeconds = "remaining_seconds"
        case state
    }
}

struct NodeSession: Codable {
    let sessionId: String
    let nodeURL: String
    let assignedIP: String?
    let serverPublicKey: String
    let endpoint: String
    let expectedExitIP: String
    let remainingSeconds: Int
    let notAfter: String
    let state: String

    enum CodingKeys: String, CodingKey {
        case sessionId = "session_id"
        case nodeURL = "node_url"
        case assignedIP = "assigned_ip"
        case serverPublicKey = "server_public_key"
        case endpoint
        case expectedExitIP = "expected_exit_ip"
        case remainingSeconds = "remaining_seconds"
        case notAfter = "not_after"
        case state
    }
}

struct ErrorResponse: Decodable {
    let error: String?
}
