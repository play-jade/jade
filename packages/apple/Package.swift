// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "JadeCoreBindings",
    platforms: [
        .macOS(.v14),
        .iOS(.v17),
    ],
    products: [
        .library(
            name: "JadeCoreBindings",
            targets: ["JadeCoreBindings"]
        ),
    ],
    targets: [
        .target(
            name: "core_ffiFFI",
            path: "Sources/core_ffiFFI",
            publicHeadersPath: "include"
        ),
        .target(
            name: "JadeCoreBindings",
            dependencies: ["core_ffiFFI"],
            path: "Sources/JadeCoreBindings",
            linkerSettings: [
                .linkedLibrary("sqlite3"),
                .unsafeFlags(["-L", "Artifacts", "-lcore_ffi"]),
            ]
        ),
        .executableTarget(
            name: "JadeCoreBindingsTests",
            dependencies: ["JadeCoreBindings"],
            path: "Tests/JadeCoreBindingsTests",
            linkerSettings: [
                .linkedLibrary("sqlite3"),
                .unsafeFlags(["-L", "Artifacts", "-lcore_ffi"]),
            ]
        ),
    ]
)
