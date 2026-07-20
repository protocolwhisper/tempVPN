import Foundation
import XCTest
@testable import tempvpnctl

final class TempVPNCLITests: XCTestCase {
    func testPaidSessionUsesServerWireNames() throws {
        let data = Data(#"{"session_id":"sess_123","node_url":"https://node.example/","remaining_seconds":1800,"state":"paused"}"#.utf8)
        let session = try JSONDecoder().decode(PaidSession.self, from: data)

        XCTAssertEqual(session.sessionId, "sess_123")
        XCTAssertEqual(session.nodeURL, "https://node.example/")
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

    func testInvalidNodeURLIsRejected() {
        XCTAssertThrowsError(try normalizedNodeURL("node.example"))
        XCTAssertThrowsError(try normalizedNodeURL("file:///tmp/node"))
    }

    func testCachedCatalogCanHealthCheckNodesAfterRegistryLeaseExpires() {
        let expired = CatalogNode(
            id: "eu-1",
            name: "EU One",
            region: "eu-west",
            apiURL: "https://node.example",
            wireguardEndpoint: "192.0.2.1:51820",
            expectedExitIP: "192.0.2.1",
            leaseExpiresAt: Date(timeIntervalSince1970: 1)
        )
        XCTAssertTrue(catalogCandidates(
            [expired], region: "eu-west", allowExpiredLeases: false, now: Date()
        ).isEmpty)
        XCTAssertEqual(catalogCandidates(
            [expired], region: "eu-west", allowExpiredLeases: true, now: Date()
        ).map(\.id), ["eu-1"])
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
