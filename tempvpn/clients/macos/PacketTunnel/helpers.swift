import Foundation

enum SessionActionResult {
    case active
    case inactive
    case unavailable
}

func sessionActionURL(nodeURL: String, sessionId: String, action: String) -> URL? {
    guard var components = URLComponents(string: nodeURL) else { return nil }
    var path = components.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    for component in ["sessions", sessionId, action] {
        if !path.isEmpty { path.append("/") }
        path.append(component.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? component)
    }
    components.path = "/" + path
    return components.url
}

func sessionState(from data: Data) -> String? {
    guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
        return nil
    }
    return object["state"] as? String
}
