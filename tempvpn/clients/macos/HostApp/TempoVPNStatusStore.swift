#if os(macOS)
import Foundation

public struct TempoVPNStatusStore {
    public var statusURL: URL

    public init(statusURL: URL = URL(fileURLWithPath: "/tmp/vpn-client-status.json")) {
        self.statusURL = statusURL
    }

    public func read() throws -> TempoVPNSessionStatus {
        let data = try Data(contentsOf: statusURL)
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode(TempoVPNSessionStatus.self, from: data)
    }

    public func remainingTimeLabel() throws -> String {
        let status = try read()
        let remaining = max(0, status.remainingSeconds)
        let minutes = remaining / 60
        let seconds = remaining % 60
        if minutes >= 60 {
            return String(format: "%dh %02dm", minutes / 60, minutes % 60)
        }
        return String(format: "%dm %02ds", minutes, seconds)
    }
}
#endif
