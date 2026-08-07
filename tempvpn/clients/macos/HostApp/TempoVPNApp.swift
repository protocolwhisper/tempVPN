#if os(macOS)
import AppKit

/// An intentionally invisible host required by macOS to install the embedded
/// Packet Tunnel Provider. Agents interact only with `tempvpnctl`.
@main
final class TempoVPNApp: NSObject, NSApplicationDelegate {
    private static let appDelegate = TempoVPNApp()

    static func main() {
        let application = NSApplication.shared
        application.setActivationPolicy(.prohibited)
        application.delegate = appDelegate
        application.run()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        // LaunchServices has now seen the containing app and its extension.
        // There is no user interface or resident menu-bar process to keep alive.
        DispatchQueue.main.async {
            NSApplication.shared.terminate(nil)
        }
    }
}
#endif
