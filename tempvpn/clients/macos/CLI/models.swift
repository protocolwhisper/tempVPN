import Foundation

struct CatalogNode: Codable, Sendable {
    let id: String
    let name: String
    let region: String
    let countryCode: String?
    let subdivisionCode: String?
    let city: String?
    let acceptingSessions: Bool?
    let availableSlots: Int?
    let apiURL: String
    let wireguardEndpoint: String
    let expectedExitIP: String
    let leaseExpiresAt: Date

    enum CodingKeys: String, CodingKey {
        case id, name, region, city
        case countryCode = "country_code"
        case subdivisionCode = "subdivision_code"
        case acceptingSessions = "accepting_sessions"
        case availableSlots = "available_slots"
        case apiURL = "api_url"
        case wireguardEndpoint = "wireguard_endpoint"
        case expectedExitIP = "expected_exit_ip"
        case leaseExpiresAt = "lease_expires_at"
    }

    init(
        id: String,
        name: String,
        region: String,
        countryCode: String? = nil,
        subdivisionCode: String? = nil,
        city: String? = nil,
        acceptingSessions: Bool? = nil,
        availableSlots: Int? = nil,
        apiURL: String,
        wireguardEndpoint: String,
        expectedExitIP: String,
        leaseExpiresAt: Date
    ) {
        self.id = id
        self.name = name
        self.region = region
        self.countryCode = countryCode
        self.subdivisionCode = subdivisionCode
        self.city = city
        self.acceptingSessions = acceptingSessions
        self.availableSlots = availableSlots
        self.apiURL = apiURL
        self.wireguardEndpoint = wireguardEndpoint
        self.expectedExitIP = expectedExitIP
        self.leaseExpiresAt = leaseExpiresAt
    }
}

struct DiscoveryFilters: Codable, Equatable, Sendable {
    let country: String?
    let city: String?
    let region: String?
    let available: Bool

    init(country: String?, city: String?, region: String?) throws {
        let normalizedCountry = country?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .uppercased()
        if let normalizedCountry,
           normalizedCountry.count != 2
            || !normalizedCountry.unicodeScalars.allSatisfy(CharacterSet.letters.contains) {
            throw TempVPNCLIError.usage(
                "--country must be an ISO 3166-1 alpha-2 code such as DE."
            )
        }
        self.country = normalizedCountry
        self.city = try normalizeDiscoveryText(city, option: "--city")
        self.region = try normalizeDiscoveryText(region, option: "--region")
        self.available = true
    }
}

private func normalizeDiscoveryText(_ value: String?, option: String) throws -> String? {
    guard let value else { return nil }
    let normalized = value.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !normalized.isEmpty else {
        throw TempVPNCLIError.usage("(option) cannot be empty.")
    }
    return normalized
}

struct CatalogCache: Codable {
    let fetchedAt: Date
    let filters: DiscoveryFilters?
    let nodes: [CatalogNode]

    enum CodingKeys: String, CodingKey {
        case fetchedAt = "fetched_at"
        case filters, nodes
    }
}

struct NodeHealth: Codable, Equatable {
    let status: String
    let activeSessions: Int?
    let acceptingSessions: Bool?
    let availableSlots: Int?

    enum CodingKeys: String, CodingKey {
        case status
        case activeSessions = "active_sessions"
        case acceptingSessions = "accepting_sessions"
        case availableSlots = "available_slots"
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
