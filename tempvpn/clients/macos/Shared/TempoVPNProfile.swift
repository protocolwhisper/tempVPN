import Foundation

public enum TempoVPNProviderKeys {
    public static let tunnelName = "tunnelName"
    public static let wgQuickConfig = "wgQuickConfig"
    public static let notAfter = "notAfter"
    public static let remainingSeconds = "remainingSeconds"
    public static let sessionId = "sessionId"
    public static let nodeURL = "nodeURL"
    public static let nodeName = "nodeName"
    public static let countryCode = "countryCode"
    public static let subdivisionCode = "subdivisionCode"
    public static let city = "city"
    public static let region = "region"
}

public struct TempoVPNProfile: Sendable {
    public var tunnelName: String
    public var providerBundleIdentifier: String
    public var wgQuickConfig: String
    public var notAfter: Date?
    public var remainingSeconds: Int?
    public var sessionId: String?
    public var nodeURL: String?
    public var nodeName: String?
    public var countryCode: String?
    public var subdivisionCode: String?
    public var city: String?
    public var region: String?

    public init(
        tunnelName: String = "TempVPN",
        providerBundleIdentifier: String,
        wgQuickConfig: String,
        notAfter: Date? = nil,
        remainingSeconds: Int? = nil,
        sessionId: String? = nil,
        nodeURL: String? = nil,
        nodeName: String? = nil,
        countryCode: String? = nil,
        subdivisionCode: String? = nil,
        city: String? = nil,
        region: String? = nil
    ) {
        self.tunnelName = tunnelName
        self.providerBundleIdentifier = providerBundleIdentifier
        self.wgQuickConfig = wgQuickConfig
        self.notAfter = notAfter
        self.remainingSeconds = remainingSeconds
        self.sessionId = sessionId
        self.nodeURL = nodeURL
        self.nodeName = nodeName
        self.countryCode = countryCode
        self.subdivisionCode = subdivisionCode
        self.city = city
        self.region = region
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
        if let nodeName { options[TempoVPNProviderKeys.nodeName] = nodeName as NSString }
        if let countryCode { options[TempoVPNProviderKeys.countryCode] = countryCode as NSString }
        if let subdivisionCode {
            options[TempoVPNProviderKeys.subdivisionCode] = subdivisionCode as NSString
        }
        if let city { options[TempoVPNProviderKeys.city] = city as NSString }
        if let region { options[TempoVPNProviderKeys.region] = region as NSString }

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
