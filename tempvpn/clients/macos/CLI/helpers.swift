import Foundation

let tempVPNVersion = "0.1.0"
let tempVPNInterfaceName = "tempvpn"

func option(_ name: String, in arguments: [String]) throws -> String? {
    guard let index = arguments.firstIndex(of: name) else { return nil }
    guard arguments.indices.contains(index + 1), !arguments[index + 1].hasPrefix("--") else {
        throw TempVPNCLIError.usage("Missing value for \(name).\n\n\(TempVPNCLI.usage)")
    }
    return arguments[index + 1]
}

func hasFlag(_ name: String, in arguments: [String]) -> Bool {
    arguments.contains(name)
}

func normalizedNodeURL(_ raw: String) throws -> String {
    let value = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    guard let url = URL(string: value),
          let scheme = url.scheme?.lowercased(),
          ["http", "https"].contains(scheme),
          url.host != nil else {
        throw TempVPNCLIError.invalidURL(raw)
    }
    return value
}

func endpointURL(baseURL: String, pathComponents: [String]) throws -> URL {
    guard var components = URLComponents(string: try normalizedNodeURL(baseURL)) else {
        throw TempVPNCLIError.invalidURL(baseURL)
    }
    var path = components.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    for component in pathComponents {
        if !path.isEmpty { path.append("/") }
        path.append(component.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? component)
    }
    components.path = "/" + path
    guard let url = components.url else { throw TempVPNCLIError.invalidURL(baseURL) }
    return url
}

func catalogURL(baseURL: String, filters: DiscoveryFilters) throws -> URL {
    let base = try endpointURL(baseURL: baseURL, pathComponents: ["nodes"])
    guard var components = URLComponents(url: base, resolvingAgainstBaseURL: false) else {
        throw TempVPNCLIError.invalidURL(baseURL)
    }
    var items: [URLQueryItem] = []
    if let country = filters.country { items.append(URLQueryItem(name: "country", value: country)) }
    if let city = filters.city { items.append(URLQueryItem(name: "city", value: city)) }
    if let region = filters.region { items.append(URLQueryItem(name: "region", value: region)) }
    items.append(URLQueryItem(name: "available", value: String(filters.available)))
    components.queryItems = items
    guard let url = components.url else { throw TempVPNCLIError.invalidURL(baseURL) }
    return url
}

func readInput(_ path: String) throws -> Data {
    if path == "-" { return FileHandle.standardInput.readDataToEndOfFile() }
    return try Data(contentsOf: URL(fileURLWithPath: path))
}

func emit(_ value: Any, json: Bool) throws {
    if json {
        let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
        print(String(decoding: data, as: UTF8.self))
    } else if let dictionary = value as? [String: Any] {
        for (key, value) in dictionary.sorted(by: { $0.key < $1.key }) {
            print("\(key): \(value)")
        }
    } else {
        print(value)
    }
}

func applicationSupportDirectory() throws -> URL {
    if let override = ProcessInfo.processInfo.environment["TEMPVPN_STATE_DIR"], !override.isEmpty {
        let directory = URL(fileURLWithPath: override, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700]
        )
        return directory
    }
    let base = try FileManager.default.url(
        for: .applicationSupportDirectory,
        in: .userDomainMask,
        appropriateFor: nil,
        create: true
    )
    let directory = base.appendingPathComponent("TempVPN", isDirectory: true)
    try FileManager.default.createDirectory(
        at: directory,
        withIntermediateDirectories: true,
        attributes: [.posixPermissions: 0o700]
    )
    return directory
}

func decodeResponseError(_ data: Data) -> String? {
    if let response = try? JSONDecoder().decode(ErrorResponse.self, from: data),
       let error = response.error {
        return error
    }
    let text = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    return text.isEmpty ? nil : text
}

func headlessAppIsInstalled() -> Bool {
    let environment = ProcessInfo.processInfo.environment
    let home = FileManager.default.homeDirectoryForCurrentUser.path
    let candidates = [
        environment["TEMPVPN_HOST_APP_PATH"],
        "/Applications/TempVPN.app",
        "\(home)/Applications/TempVPN.app",
    ].compactMap { $0 }
    return candidates.contains { path in
        FileManager.default.fileExists(
            atPath: "\(path)/Contents/PlugIns/TempVPNPacketTunnel.appex"
        )
    }
}
