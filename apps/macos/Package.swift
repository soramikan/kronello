// swift-tools-version: 5.10
import PackageDescription
import Foundation

let libraryPath = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().appendingPathComponent("Libraries").path

let package = Package(
    name: "Kronello",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "KronelloCore", targets: ["KronelloCore"]),
        .executable(name: "KronelloPreviewHarness", targets: ["KronelloPreviewHarness"]),
        .executable(name: "KronelloJSONBenchmark", targets: ["KronelloJSONBenchmark"])
    ],
    targets: [
        .target(name: "CKronelloFFI", publicHeadersPath: "include"),
        .target(name: "KronelloCore", dependencies: ["CKronelloFFI"],
                linkerSettings: [.unsafeFlags(["-L", libraryPath, "-lkronello_ffi", "-Xlinker", "-rpath", "-Xlinker", libraryPath])]),
        .executableTarget(name: "KronelloPreviewHarness", dependencies: ["KronelloCore"]),
        .executableTarget(name: "KronelloJSONBenchmark", dependencies: ["KronelloCore"]),
        .testTarget(name: "KronelloCoreTests", dependencies: ["KronelloCore", "CKronelloFFI"])
    ]
)

