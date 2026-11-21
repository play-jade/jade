// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "JadeMacOS",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(path: "../../packages/apple"),
    ],
    targets: [
        .executableTarget(
            name: "JadeMacOS",
            dependencies: [
                .product(name: "JadeCoreBindings", package: "apple"),
            ],
            path: "Sources/JadeMacOS",
            linkerSettings: [
                .unsafeFlags(["-L", "../../packages/apple/Artifacts", "-lcore_ffi", "-lsqlite3"]),
            ]
        ),
    ]
)
