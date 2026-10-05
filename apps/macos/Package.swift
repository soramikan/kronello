// swift-tools-version: 5.10
import PackageDescription
import Foundation

let libraryPath = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().appendingPathComponent("Libraries").path

let package = Package(
    name: "Kronello",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "Kronello", targets: ["Kronello"]),
        .library(name: "KronelloCore", targets: ["KronelloCore"]),
        .library(name: "KronelloDesign", targets: ["KronelloDesign"]),
        .executable(name: "KronelloPreviewHarness", targets: ["KronelloPreviewHarness"]),
        .executable(name: "KronelloJSONBenchmark", targets: ["KronelloJSONBenchmark"])
    ],
    targets: [
        .target(name: "KronelloAppModel", dependencies: ["KronelloCore", "KronelloDesign"]),
        .executableTarget(name: "Kronello", dependencies: ["KronelloCore", "KronelloDesign", "KronelloAppModel"],
            linkerSettings: [.unsafeFlags(["-Xlinker", "-rpath", "-Xlinker", "@executable_path/../Frameworks"])]),
        .testTarget(name: "KronelloAppModelTests", dependencies: ["KronelloAppModel", "KronelloCore", "KronelloDesign"]),
        // Native FFI boundary (FFI-001).
        .target(name: "CKronelloFFI", publicHeadersPath: "include"),
        .target(name: "KronelloCore", dependencies: ["CKronelloFFI"],
                linkerSettings: [.unsafeFlags(["-L", libraryPath, "-lkronello_ffi", "-Xlinker", "-rpath", "-Xlinker", libraryPath])]),
        .executableTarget(name: "KronelloPreviewHarness", dependencies: ["KronelloCore"]),
        .executableTarget(name: "KronelloJSONBenchmark", dependencies: ["KronelloCore"]),
        .testTarget(name: "KronelloCoreTests", dependencies: ["KronelloCore", "CKronelloFFI"]),
        // Design system: generated tokens and icons, fonts, and SwiftUI components.
        .target(name: "KronelloDesign", resources: [.copy("Resources/Fonts")]),
        // Renders every component in both themes to PNG for design review:
        // `swift run --package-path apps/macos KronelloDesignGallery <output-directory>`.
        .executableTarget(name: "KronelloDesignGallery", dependencies: ["KronelloDesign", "KronelloAppModel"]),
        .testTarget(name: "KronelloDesignTests", dependencies: ["KronelloDesign"])
    ]
)
