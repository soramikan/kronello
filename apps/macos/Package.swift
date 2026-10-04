// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "Kronello",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "KronelloDesign", targets: ["KronelloDesign"]),
    ],
    targets: [
        // Design system: generated tokens and icons, fonts, and SwiftUI components.
        .target(name: "KronelloDesign", resources: [.copy("Resources/Fonts")]),
        // Renders every component in both themes to PNG for design review:
        // `swift run KronelloDesignGallery <output-directory>`.
        .executableTarget(name: "KronelloDesignGallery", dependencies: ["KronelloDesign"]),
        .testTarget(name: "KronelloDesignTests", dependencies: ["KronelloDesign"]),
    ]
)
