import Foundation

public enum TempoVPNProviderKeys {
    public static let tunnelName = "tunnelName"
    public static let wgQuickConfig = "wgQuickConfig"
    public static let expiresAt = "expiresAt"
}

public struct TempoVPNProfile: Sendable {
    public var tunnelName: String
    public var providerBundleIdentifier: String
    public var wgQuickConfig: String
    public var expiresAt: Date?

    public init(
        tunnelName: String = "Tempo VPN",
        providerBundleIdentifier: String,
        wgQuickConfig: String,
        expiresAt: Date? = nil
    ) {
        self.tunnelName = tunnelName
        self.providerBundleIdentifier = providerBundleIdentifier
        self.wgQuickConfig = wgQuickConfig
        self.expiresAt = expiresAt
    }

    public var startOptions: [String: NSObject] {
        var options: [String: NSObject] = [
            TempoVPNProviderKeys.tunnelName: tunnelName as NSString,
            TempoVPNProviderKeys.wgQuickConfig: wgQuickConfig as NSString
        ]

        if let expiresAt {
            options[TempoVPNProviderKeys.expiresAt] = ISO8601DateFormatter()
                .string(from: expiresAt) as NSString
        }

        return options
    }
}
