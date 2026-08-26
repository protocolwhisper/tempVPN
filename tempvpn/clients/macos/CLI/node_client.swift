import Foundation

private let catalogCacheTTL: TimeInterval = 24 * 60 * 60
private let maximumConcurrentHealthProbes = 8

func selectNode(
    registryURL: String,
    filters: DiscoveryFilters,
    session: URLSession = .shared
) async throws -> CatalogNode {
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .iso8601
    let cacheURL = try applicationSupportDirectory().appendingPathComponent("nodes.json")
    let nodes: [CatalogNode]
    let usingCachedCatalog: Bool

    do {
        let url = try catalogURL(baseURL: registryURL, filters: filters)
        var request = URLRequest(url: url)
        request.timeoutInterval = 10
        let (data, response) = try await session.data(for: request)
        try validateHTTPResponse(response, data: data)
        nodes = try decoder.decode([CatalogNode].self, from: data)
        usingCachedCatalog = false

        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        let cache = CatalogCache(fetchedAt: Date(), filters: filters, nodes: nodes)
        try? encoder.encode(cache).write(to: cacheURL, options: [.atomic])
    } catch {
        guard let data = try? Data(contentsOf: cacheURL),
              let cache = try? decoder.decode(CatalogCache.self, from: data),
              Date().timeIntervalSince(cache.fetchedAt) <= catalogCacheTTL,
              cache.filters == filters else {
            throw error
        }
        nodes = cache.nodes
        usingCachedCatalog = true
    }

    let candidates = catalogCandidates(
        nodes,
        filters: filters,
        allowExpiredLeases: usingCachedCatalog,
        now: Date()
    )
    let ranked = await rankCatalogNodes(candidates, session: session)
    guard let selected = ranked.first?.1 else { throw TempVPNCLIError.noHealthyNodes }
    try await checkNodeAvailability(apiURL: selected.apiURL, session: session)
    return selected
}

func catalogCandidates(
    _ nodes: [CatalogNode],
    filters: DiscoveryFilters,
    allowExpiredLeases: Bool,
    now: Date
) -> [CatalogNode] {
    return nodes.filter { node in
        (allowExpiredLeases || node.leaseExpiresAt > now)
            && matchesDiscoveryValue(node.countryCode, filters.country)
            && matchesDiscoveryValue(node.city, filters.city)
            && matchesDiscoveryValue(node.region, filters.region)
            && (!filters.available || (
                node.acceptingSessions == true && (node.availableSlots ?? 0) > 0
            ))
    }
}

private func matchesDiscoveryValue(_ actual: String?, _ requested: String?) -> Bool {
    guard let requested else { return true }
    return actual?.trimmingCharacters(in: .whitespacesAndNewlines)
        .caseInsensitiveCompare(requested) == .orderedSame
}

func rankCatalogNodes(
    _ candidates: [CatalogNode],
    session: URLSession = .shared
) async -> [(TimeInterval, CatalogNode)] {
    var results: [(TimeInterval, CatalogNode)] = []
    for start in stride(from: 0, to: candidates.count, by: maximumConcurrentHealthProbes) {
        let end = min(start + maximumConcurrentHealthProbes, candidates.count)
        let batch = candidates[start..<end]
        let batchResults = await withTaskGroup(of: (TimeInterval, CatalogNode)?.self) { group in
            for node in batch {
                group.addTask {
                    guard let latency = try? await medianHealthLatency(
                        apiURL: node.apiURL,
                        session: session
                    ) else { return nil }
                    return (latency, node)
                }
            }
            var ranked: [(TimeInterval, CatalogNode)] = []
            for await result in group {
                if let result { ranked.append(result) }
            }
            return ranked
        }
        results.append(contentsOf: batchResults)
    }
    return results.sorted { lhs, rhs in
        if lhs.0 == rhs.0 { return lhs.1.id < rhs.1.id }
        return lhs.0 < rhs.0
    }
}

func medianHealthLatency(
    apiURL: String,
    session: URLSession = .shared
) async throws -> TimeInterval {
    var samples: [TimeInterval] = []
    for _ in 0..<3 {
        let url = try endpointURL(baseURL: apiURL, pathComponents: ["health"])
        var request = URLRequest(url: url)
        request.timeoutInterval = 2
        let start = ContinuousClock.now
        let (data, response) = try await session.data(for: request)
        try validateHTTPResponse(response, data: data)
        let elapsed = start.duration(to: .now).components
        samples.append(
            Double(elapsed.seconds)
                + Double(elapsed.attoseconds) / 1_000_000_000_000_000_000
        )
    }
    return samples.sorted()[1]
}

func fetchNodeHealth(apiURL: String, session: URLSession = .shared) async throws -> NodeHealth {
    let url = try endpointURL(baseURL: apiURL, pathComponents: ["health"])
    var request = URLRequest(url: url)
    request.timeoutInterval = 2
    let (data, response) = try await session.data(for: request)
    try validateHTTPResponse(response, data: data)
    return try JSONDecoder().decode(NodeHealth.self, from: data)
}

func validateNodeHealthForPayment(_ health: NodeHealth) throws {
    guard health.status == "ok" else {
        throw TempVPNCLIError.noHealthyNodes
    }
    guard health.acceptingSessions == true, (health.availableSlots ?? 0) > 0 else {
        throw TempVPNCLIError.nodeUnavailable
    }
}

func checkNodeAvailability(
    apiURL: String,
    session: URLSession = .shared
) async throws {
    try validateNodeHealthForPayment(try await fetchNodeHealth(apiURL: apiURL, session: session))
}

func connectSession(
    _ paid: PaidSession,
    registryURL: String,
    nodeId: String,
    publicKey: String
) async throws -> NodeSession {
    let url = try endpointURL(
        baseURL: registryURL,
        pathComponents: ["sessions", paid.sessionId, "connect"]
    )
    let body = try connectRequestBody(nodeId: nodeId, publicKey: publicKey)
    return try await sendJSON(url: url, method: "POST", body: body)
}

func connectRequestBody(nodeId: String, publicKey: String) throws -> Data {
    try JSONSerialization.data(withJSONObject: [
        "node_id": nodeId,
        "client_public_key": publicKey,
    ])
}

func heartbeatSession(registryURL: String, sessionId: String) async throws -> PortableSession {
    let url = try endpointURL(
        baseURL: registryURL,
        pathComponents: ["sessions", sessionId, "heartbeat"]
    )
    return try await sendJSON(url: url, method: "POST", body: Data("{}".utf8))
}

func pauseSession(registryURL: String, sessionId: String) async throws -> PortableSession {
    let url = try endpointURL(
        baseURL: registryURL,
        pathComponents: ["sessions", sessionId, "pause"]
    )
    return try await sendJSON(url: url, method: "POST", body: Data("{}".utf8))
}

func fetchSession(registryURL: String, sessionId: String) async throws -> PortableSession {
    let url = try endpointURL(
        baseURL: registryURL,
        pathComponents: ["sessions", sessionId, "status"]
    )
    return try await sendJSON(url: url, method: "GET", body: nil)
}

private func sendJSON<T: Decodable>(url: URL, method: String, body: Data?) async throws -> T {
    var request = URLRequest(url: url)
    request.httpMethod = method
    request.timeoutInterval = 10
    request.setValue("application/json", forHTTPHeaderField: "content-type")
    request.httpBody = body
    let (data, response) = try await URLSession.shared.data(for: request)
    try validateHTTPResponse(response, data: data)
    return try JSONDecoder().decode(T.self, from: data)
}

private func validateHTTPResponse(_ response: URLResponse, data: Data) throws {
    guard let response = response as? HTTPURLResponse else {
        throw TempVPNCLIError.invalidResponse("the node did not return an HTTP response")
    }
    guard (200..<300).contains(response.statusCode) else {
        throw TempVPNCLIError.requestFailed(
            status: response.statusCode,
            message: decodeResponseError(data)
        )
    }
}
