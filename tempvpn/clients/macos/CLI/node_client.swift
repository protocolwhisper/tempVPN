import Foundation

private let catalogCacheTTL: TimeInterval = 24 * 60 * 60

func selectNode(registryURL: String, region: String?) async throws -> CatalogNode {
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .iso8601
    let cacheURL = try applicationSupportDirectory().appendingPathComponent("nodes.json")
    let nodes: [CatalogNode]
    let usingCachedCatalog: Bool

    do {
        let url = try endpointURL(baseURL: registryURL, pathComponents: ["nodes"])
        var request = URLRequest(url: url)
        request.timeoutInterval = 10
        let (data, response) = try await URLSession.shared.data(for: request)
        try validateHTTPResponse(response, data: data)
        nodes = try decoder.decode([CatalogNode].self, from: data)
        usingCachedCatalog = false

        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        let cache = CatalogCache(fetchedAt: Date(), nodes: nodes)
        try? encoder.encode(cache).write(to: cacheURL, options: [.atomic])
    } catch {
        guard let data = try? Data(contentsOf: cacheURL),
              let cache = try? decoder.decode(CatalogCache.self, from: data),
              Date().timeIntervalSince(cache.fetchedAt) <= catalogCacheTTL else {
            throw error
        }
        nodes = cache.nodes
        usingCachedCatalog = true
    }

    let candidates = catalogCandidates(
        nodes,
        region: region,
        allowExpiredLeases: usingCachedCatalog,
        now: Date()
    )
    let ranked = await withTaskGroup(of: (TimeInterval, CatalogNode)?.self) { group in
        for node in candidates {
            group.addTask {
                guard let latency = try? await medianHealthLatency(apiURL: node.apiURL) else { return nil }
                return (latency, node)
            }
        }
        var results: [(TimeInterval, CatalogNode)] = []
        for await result in group {
            if let result { results.append(result) }
        }
        return results.sorted { lhs, rhs in
            if lhs.0 == rhs.0 { return lhs.1.id < rhs.1.id }
            return lhs.0 < rhs.0
        }
    }
    guard let selected = ranked.first?.1 else { throw TempVPNCLIError.noHealthyNodes }
    return selected
}

func catalogCandidates(
    _ nodes: [CatalogNode],
    region: String?,
    allowExpiredLeases: Bool,
    now: Date
) -> [CatalogNode] {
    let requestedRegion = region?.lowercased()
    return nodes.filter { node in
        (allowExpiredLeases || node.leaseExpiresAt > now)
            && (requestedRegion == nil || node.region.lowercased() == requestedRegion)
    }
}

func medianHealthLatency(apiURL: String) async throws -> TimeInterval {
    var samples: [TimeInterval] = []
    for _ in 0..<3 {
        let url = try endpointURL(baseURL: apiURL, pathComponents: ["health"])
        var request = URLRequest(url: url)
        request.timeoutInterval = 2
        let start = ContinuousClock.now
        let (data, response) = try await URLSession.shared.data(for: request)
        try validateHTTPResponse(response, data: data)
        let elapsed = start.duration(to: .now).components
        samples.append(
            Double(elapsed.seconds)
                + Double(elapsed.attoseconds) / 1_000_000_000_000_000_000
        )
    }
    return samples.sorted()[1]
}

func connectSession(_ paid: PaidSession, publicKey: String) async throws -> NodeSession {
    let url = try endpointURL(
        baseURL: paid.nodeURL,
        pathComponents: ["sessions", paid.sessionId, "connect"]
    )
    let body = try JSONSerialization.data(withJSONObject: ["client_public_key": publicKey])
    return try await sendJSON(url: url, method: "POST", body: body)
}

func heartbeatSession(nodeURL: String, sessionId: String) async throws -> NodeSession {
    let url = try endpointURL(
        baseURL: nodeURL,
        pathComponents: ["sessions", sessionId, "heartbeat"]
    )
    return try await sendJSON(url: url, method: "POST", body: Data("{}".utf8))
}

func pauseSession(nodeURL: String, sessionId: String) async throws -> NodeSession {
    let url = try endpointURL(
        baseURL: nodeURL,
        pathComponents: ["sessions", sessionId, "pause"]
    )
    return try await sendJSON(url: url, method: "POST", body: Data("{}".utf8))
}

func fetchSession(nodeURL: String, sessionId: String) async throws -> NodeSession {
    let url = try endpointURL(
        baseURL: nodeURL,
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
