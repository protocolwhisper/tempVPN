// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "TempVPNCLI",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "tempvpnctl", targets: ["tempvpnctl"]),
    ],
    targets: [
        .executableTarget(
            name: "tempvpnctl",
            path: "CLI"
        ),
        .testTarget(
            name: "TempVPNCLITests",
            dependencies: ["tempvpnctl"],
            path: "Tests"
        ),
    ]
)
