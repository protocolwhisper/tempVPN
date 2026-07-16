#if os(macOS)
import SwiftUI

@main
struct TempoVPNApp: App {
    @State private var statusText = "Disconnected"
    @State private var remainingText = "--"
    @State private var errorText: String?

    private let statusStore = TempoVPNStatusStore()
    private let controller = TempoVPNController()

    var body: some Scene {
        MenuBarExtra("TempVPN", systemImage: "lock.shield") {
            VStack(alignment: .leading, spacing: 8) {
                Text(statusText)
                Text(remainingText)
                    .monospacedDigit()

                if let errorText {
                    Text(errorText)
                        .foregroundStyle(.secondary)
                }

                Divider()

                Button("Refresh") {
                    refreshStatus()
                }

                Button("Disconnect") {
                    disconnect()
                }

                Button("Quit") {
                    NSApplication.shared.terminate(nil)
                }
            }
            .padding(.vertical, 4)
            .task {
                refreshStatus()
            }
        }
        .menuBarExtraStyle(.menu)
    }

    private func refreshStatus() {
        do {
            let status = try statusStore.read()
            statusText = "Connected: \(status.nodeURL)"
            remainingText = try statusStore.remainingTimeLabel()
            errorText = nil
        } catch {
            statusText = "Disconnected"
            remainingText = "--"
            errorText = nil
        }
    }

    private func disconnect() {
        Task {
            do {
                try await controller.disconnect()
                refreshStatus()
            } catch {
                errorText = error.localizedDescription
            }
        }
    }
}
#endif
