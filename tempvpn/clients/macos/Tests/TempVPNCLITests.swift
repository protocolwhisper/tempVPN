import Foundation
import XCTest
@testable import tempvpnctl

private final class StubURLProtocol: URLProtocol {
    static var handler: ((URLRequest) throws -> (Data, Int, TimeInterval))?

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        do {
            guard let handler = Self.handler else {
                throw TempVPNCLIError.invalidResponse("missing test URL handler")
            }
            let (data, status, delay) = try handler(request)
            let respond = {
                let response = HTTPURLResponse(
                    url: self.request.url!,
                    statusCode: status,
                    httpVersion: "HTTP/1.1",
                    headerFields: ["content-type": "application/json"]
                )!
                self.client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
                self.client?.urlProtocol(self, didLoad: data)
                self.client?.urlProtocolDidFinishLoading(self)
            }
            if delay == 0 { respond() }
            else { DispatchQueue.global().asyncAfter(deadline: .now() + delay, execute: respond) }
        } catch {
            client?.urlProtocol(self, didFailWithError: error)
        }
    }

    override func stopLoading() {}
}

final class TempVPNCLITests: XCTestCase {
    override func tearDown() {
        StubURLProtocol.handler = nil
        super.tearDown()
    }

    private func stubSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [StubURLProtocol.self]
        return URLSession(configuration: configuration)
    }

    private func node(
        id: String,
        country: String,
        city: String,
        apiURL: String = "https://node.example",
        expires: Date = Date().addingTimeInterval(90)
    ) -> CatalogNode {
        CatalogNode(
            id: id,
            name: id,
            region: "eu-central",
            countryCode: country,
            subdivisionCode: nil,
            city: city,
            acceptingSessions: true,
            availableSlots: 3,
            apiURL: apiURL,
            wireguardEndpoint: "192.0.2.1:51820",
            expectedExitIP: "192.0.2.1",
            leaseExpiresAt: expires
        )
    }

    func testPaidSessionUsesServerWireNames() throws {
        let data = Data(#"{"session_id":"sess_123","logical_node":"madrid","grace_deadline":"2030-01-01T00:00:00Z","remaining_seconds":1800,"state":"paused"}"#.utf8)
        let session = try JSONDecoder().decode(PaidSession.self, from: data)

        XCTAssertEqual(session.sessionId, "sess_123")
        XCTAssertNil(session.nodeURL)
        XCTAssertEqual(session.logicalNode, "madrid")
        XCTAssertEqual(session.notAfter, "2030-01-01T00:00:00Z")
        XCTAssertEqual(session.remainingSeconds, 1800)
    }

    func testNodeURLNormalizationPreservesSchemeAndDropsTrailingSlash() throws {
        XCTAssertEqual(
            try normalizedNodeURL(" https://node.example/api/ "),
            "https://node.example/api"
        )
    }

    func testEndpointURLKeepsRegistryBasePath() throws {
        let url = try endpointURL(
            baseURL: "https://registry.example/v1/",
            pathComponents: ["sessions", "sess_123", "status"]
        )
        XCTAssertEqual(url.absoluteString, "https://registry.example/v1/sessions/sess_123/status")
    }

    func testCatalogURLUsesStructuredEncodedFilters() throws {
        let filters = try DiscoveryFilters(country: " de ", city: "Frankfurt am Main", region: nil)
        let url = try catalogURL(baseURL: "https://registry.example/v1", filters: filters)
        let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        XCTAssertEqual(components?.path, "/v1/nodes")
        XCTAssertEqual(
            Dictionary(uniqueKeysWithValues: components?.queryItems?.map { ($0.name, $0.value!) } ?? []),
            ["country": "DE", "city": "Frankfurt am Main", "available": "true"]
        )
    }

    func testInvalidNodeURLIsRejected() {
        XCTAssertThrowsError(try normalizedNodeURL("node.example"))
        XCTAssertThrowsError(try normalizedNodeURL("file:///tmp/node"))
    }

    func testCachedCatalogCanHealthCheckNodesAfterRegistryLeaseExpires() {
        let expired = node(
            id: "eu-1",
            country: "DE",
            city: "Frankfurt",
            expires: Date(timeIntervalSince1970: 1)
        )
        let filters = try! DiscoveryFilters(country: "DE", city: nil, region: nil)
        XCTAssertTrue(catalogCandidates(
            [expired], filters: filters, allowExpiredLeases: false, now: Date()
        ).isEmpty)
        XCTAssertEqual(catalogCandidates(
            [expired], filters: filters, allowExpiredLeases: true, now: Date()
        ).map(\.id), ["eu-1"])
    }

    func testCountryCityAndGlobalEligibilityFilters() throws {
        let germany = node(id: "de", country: "DE", city: "Frankfurt")
        let france = node(id: "fr", country: "FR", city: "Paris")
        let germanFilters = try DiscoveryFilters(country: "de", city: "frankfurt", region: nil)
        XCTAssertEqual(catalogCandidates(
            [france, germany], filters: germanFilters, allowExpiredLeases: false, now: Date()
        ).map(\.id), ["de"])

        let global = try DiscoveryFilters(country: nil, city: nil, region: nil)
        XCTAssertEqual(Set(catalogCandidates(
            [france, germany], filters: global, allowExpiredLeases: false, now: Date()
        ).map(\.id)), Set(["de", "fr"]))
    }

    func testLegacyAndUnavailableNodesAreExcluded() throws {
        let legacy = CatalogNode(
            id: "legacy",
            name: "legacy",
            region: "eu",
            apiURL: "https://legacy.example",
            wireguardEndpoint: "192.0.2.1:51820",
            expectedExitIP: "192.0.2.1",
            leaseExpiresAt: Date().addingTimeInterval(90)
        )
        var full = node(id: "full", country: "DE", city: "Frankfurt")
        full = CatalogNode(
            id: full.id,
            name: full.name,
            region: full.region,
            countryCode: full.countryCode,
            city: full.city,
            acceptingSessions: true,
            availableSlots: 0,
            apiURL: full.apiURL,
            wireguardEndpoint: full.wireguardEndpoint,
            expectedExitIP: full.expectedExitIP,
            leaseExpiresAt: full.leaseExpiresAt
        )
        let filters = try DiscoveryFilters(country: nil, city: nil, region: nil)
        XCTAssertTrue(catalogCandidates(
            [legacy, full], filters: filters, allowExpiredLeases: false, now: Date()
        ).isEmpty)
    }

    func testLatencyRankingUsesClientMeasurementsAndStableIDs() async throws {
        StubURLProtocol.handler = { request in
            let delay = request.url?.host == "slow.example" ? 0.02 : 0.001
            return (Data("{}".utf8), 200, delay)
        }
        let ranked = await rankCatalogNodes([
            node(id: "slow", country: "DE", city: "Berlin", apiURL: "https://slow.example"),
            node(id: "fast", country: "DE", city: "Frankfurt", apiURL: "https://fast.example"),
        ], session: stubSession())
        XCTAssertEqual(ranked.first?.1.id, "fast")
    }

    func testFinalAvailabilityFailureStopsWorkflowBeforePayment() throws {
        XCTAssertThrowsError(try validateNodeHealthForPayment(NodeHealth(
            status: "ok",
            activeSessions: 1,
            acceptingSessions: false,
            availableSlots: 4
        ))) { error in
            guard case TempVPNCLIError.nodeUnavailable = error else {
                return XCTFail("expected nodeUnavailable, got \(error)")
            }
        }
    }

    func testPortableSessionCanReconnectThroughAnotherNodeId() throws {
        let data = try connectRequestBody(nodeId: "fr-paris", publicKey: "local-public-key")
        let body = try JSONSerialization.jsonObject(with: data) as? [String: String]
        XCTAssertEqual(body?["node_id"], "fr-paris")
        XCTAssertEqual(body?["client_public_key"], "local-public-key")
    }

    func testPaidCapabilityIsPersistedBeforeActivationWithPrivatePermissions() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        setenv("TEMPVPN_STATE_DIR", directory.path, 1)
        defer {
            unsetenv("TEMPVPN_STATE_DIR")
            try? FileManager.default.removeItem(at: directory)
        }
        let paid = PaidSession(
            sessionId: "sess_saved",
            nodeURL: "https://madrid.example",
            logicalNode: "madrid",
            notAfter: "2030-01-01T00:00:00Z",
            remainingSeconds: 600,
            state: "paused"
        )
        try saveCapability(paid, registryURL: "https://registry.example")
        XCTAssertEqual(try savedCapabilityCount(), 1)
        let attributes = try FileManager.default.attributesOfItem(
            atPath: try capabilityStoreURL().path
        )
        XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o600)
    }

    func testPausedResumeRefreshesGenerationMetadataAndKeepsKeychainPrivateKey() {
        let blue = wireGuardQuickConfiguration(
            sessionId: "sess_resume",
            assignedIP: "10.8.0.2/32",
            serverPublicKey: "blue-server-key",
            endpoint: "blue.node.example:51820"
        )
        let green = wireGuardQuickConfiguration(
            sessionId: "sess_resume",
            assignedIP: "10.8.0.2/32",
            serverPublicKey: "green-server-key",
            endpoint: "green.node.example:51820"
        )

        XCTAssertTrue(blue.contains("PrivateKey = keychain:sess_resume"))
        XCTAssertTrue(green.contains("PrivateKey = keychain:sess_resume"))
        XCTAssertTrue(green.contains("PublicKey = green-server-key"))
        XCTAssertTrue(green.contains("Endpoint = green.node.example:51820"))
        XCTAssertFalse(green.contains("blue-server-key"))
        XCTAssertFalse(green.contains("blue.node.example:51820"))
    }

    func testHeadlessAppDetectionRequiresEmbeddedPacketTunnel() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("TempVPN-\(UUID().uuidString).app", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        setenv("TEMPVPN_HOST_APP_PATH", directory.path, 1)
        defer { unsetenv("TEMPVPN_HOST_APP_PATH") }
        XCTAssertFalse(headlessAppIsInstalled())

        try FileManager.default.createDirectory(
            at: directory.appendingPathComponent("Contents/PlugIns/TempVPNPacketTunnel.appex"),
            withIntermediateDirectories: true
        )
        XCTAssertTrue(headlessAppIsInstalled())
    }

    func testNativeTunnelStatusNamesAreStableForAgents() {
        XCTAssertEqual(tunnelStatusName(.connected), "connected")
        XCTAssertEqual(tunnelStatusName(.disconnecting), "disconnecting")
    }
}
