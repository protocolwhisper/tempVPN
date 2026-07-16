import Foundation

public enum TempoVPNProviderKeys {
    public static let tunnelName = "tunnelName"
    public static let wgQuickConfig = "wgQuickConfig"
    public static let notAfter = "notAfter"
    public static let remainingSeconds = "remainingSeconds"
    public static let sessionId = "sessionId"
    public static let nodeURL = "nodeURL"
}

public struct TempoVPNProfile: Sendable {
    public var tunnelName: String
    public var providerBundleIdentifier: String
    public var wgQuickConfig: String
    public var notAfter: Date?
    public var remainingSeconds: Int?
    public var sessionId: String?
    public var nodeURL: String?

    public init(
        tunnelName: String = "TempVPN",
        providerBundleIdentifier: String,
        wgQuickConfig: String,
        notAfter: Date? = nil,
        remainingSeconds: Int? = nil,
        sessionId: String? = nil,
        nodeURL: String? = nil
    ) {
        self.tunnelName = tunnelName
        self.providerBundleIdentifier = providerBundleIdentifier
        self.wgQuickConfig = wgQuickConfig
        self.notAfter = notAfter
        self.remainingSeconds = remainingSeconds
        self.sessionId = sessionId
        self.nodeURL = nodeURL
    }

    public var startOptions: [String: NSObject] {
        var options: [String: NSObject] = [
            TempoVPNProviderKeys.tunnelName: tunnelName as NSString,
            TempoVPNProviderKeys.wgQuickConfig: wgQuickConfig as NSString
        ]

        if let notAfter {
            options[TempoVPNProviderKeys.notAfter] = ISO8601DateFormatter()
                .string(from: notAfter) as NSString
        }

        if let remainingSeconds {
            options[TempoVPNProviderKeys.remainingSeconds] = NSNumber(value: remainingSeconds)
        }

        if let sessionId {
            options[TempoVPNProviderKeys.sessionId] = sessionId as NSString
        }
        if let nodeURL {
            options[TempoVPNProviderKeys.nodeURL] = nodeURL as NSString
        }

        return options
    }
}

public struct TempoVPNSessionStatus: Codable, Sendable {
    public var sessionId: String
    public var nodeURL: String
    public var tunnelIP: String
    public var exitIP: String?
    public var interfaceName: String
    public var notAfter: Date
    public var remainingSeconds: Int

    enum CodingKeys: String, CodingKey {
        case sessionId = "session_id"
        case nodeURL = "node_url"
        case tunnelIP = "tunnel_ip"
        case exitIP = "exit_ip"
        case interfaceName = "interface_name"
        case notAfter = "not_after"
        case remainingSeconds = "remaining_seconds"
    }

    public init(
        sessionId: String,
        nodeURL: String,
        tunnelIP: String,
        exitIP: String?,
        interfaceName: String,
        notAfter: Date,
        remainingSeconds: Int
    ) {
        self.sessionId = sessionId
        self.nodeURL = nodeURL
        self.tunnelIP = tunnelIP
        self.exitIP = exitIP
        self.interfaceName = interfaceName
        self.notAfter = notAfter
        self.remainingSeconds = remainingSeconds
    }
}
