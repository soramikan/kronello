import AppKit
import KronelloDesign
import SwiftUI

enum GalleryError: Error { case renderFailed(String), missingFonts }

@MainActor
func save(_ view: AnyView, name: String, directory: URL) throws {
    let renderer = ImageRenderer(content: view.environment(\.krFreezeActivity, true).environment(\.krStaticRendering, true))
    renderer.scale = 2
    guard let image = renderer.nsImage, let tiff = image.tiffRepresentation,
          let bitmap = NSBitmapImageRep(data: tiff), let png = bitmap.representation(using: .png, properties: [:]) else {
        throw GalleryError.renderFailed(name)
    }
    let url = directory.appendingPathComponent(name + ".png")
    try png.write(to: url)
    print("wrote \(url.path) (\(bitmap.pixelsWide)×\(bitmap.pixelsHigh), 2x)")
}

let directory = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? "/tmp/kronello-gallery", isDirectory: true)
do {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    try MainActor.assumeIsolated {
        let fonts = KRFonts.registerBundled()
        guard !fonts.isEmpty, KRFonts.isAvailable(KRFontFamily.sans[0]), KRFonts.isAvailable(KRFontFamily.mono[0]) else { throw GalleryError.missingFonts }
        print("Gallery output: \(directory.path)")
        print("Bundled fonts: \(fonts.map(\.lastPathComponent).joined(separator: ", "))")
        for theme in KRTheme.allCases {
            for (name, content) in ComponentSheets.all + EditSheets.all + WorkflowComponentSheets.all + TemplateInspectionSheets.all {
                try save(AnyView(GallerySheet(name, theme: theme, content: content).krTheme(theme)), name: "\(name)-\(theme.rawValue)", directory: directory)
            }
            try save(AnyView(WorkflowScreen(template: true).krTheme(theme)), name: "Screen-template-\(theme.rawValue)", directory: directory)
            try save(AnyView(WorkflowScreen(template: false).krTheme(theme)), name: "Screen-export-\(theme.rawValue)", directory: directory)
            try save(AnyView(MotionScreen().krTheme(theme)), name: "Screen-motion-\(theme.rawValue)", directory: directory)
            try save(AnyView(MotionScreen(curveEditor: true).krTheme(theme)), name: "Screen-motion-curve-\(theme.rawValue)", directory: directory)
            try save(AnyView(EditScreen().krTheme(theme)), name: "Screen-edit-\(theme.rawValue)", directory: directory)
        }
    }
} catch {
    fputs("Gallery failed: \(error)\n", stderr)
    exit(1)
}
