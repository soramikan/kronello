import Foundation
import AppKit
import SwiftUI
import KronelloAppModel
import KronelloDesign

@MainActor final class UnusedTransport: ProjectTransport {
    var notificationHandler: ((String, [String: Any]) -> Void)?
    func ready() async throws {}
    func subscribe() async throws {}
    func poll() throws {}
    func close() {}
    func call(_ request: [String: Any]) async throws -> [String: Any] {
        fatalError("The presentation probe must not call the service")
    }
}

@MainActor func run() throws {
    if CommandLine.arguments.count == 3, CommandLine.arguments[1] == "--render-controls" {
        let directory = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        _ = KRFonts.registerBundled()
        for width in [420.0, 900.0] {
            let view = KRTransportBar(frames: .constant(0), fps: 30, duration: "00:00:04:00",
                playing: .constant(false), looping: .constant(false), zoom: .constant("fit"),
                resolution: .constant("full"), proxyMode: .constant("prefer"))
                .frame(width: width).krTheme(.dark)
            // Hosting captures AppKit text fields too; ImageRenderer omits
            // NSViewRepresentable content and cannot verify the full control bar.
            _ = NSApplication.shared
            let host = NSHostingView(rootView: view)
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: 64),
                                  styleMask: [], backing: .buffered, defer: false)
            window.contentView = host
            window.setContentSize(host.fittingSize)
            window.orderBack(nil)
            RunLoop.main.run(until: Date().addingTimeInterval(0.1))
            host.layoutSubtreeIfNeeded(); host.displayIfNeeded()
            guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { fatalError("render failed") }
            host.cacheDisplay(in: host.bounds, to: bitmap)
            guard let png = bitmap.representation(using: .png, properties: [:]) else { fatalError("PNG failed") }
            try png.write(to: directory.appendingPathComponent("transport-\(Int(width)).png"))
            window.orderOut(nil)
        }
        return
    }
    var cases: [[String: Any]] = []
    for count in [100, 1000, 5000] {
        let model = EditorModel(path: "/tmp/kronello-timeline-probe.kronello", transport: UnusedTransport())
        let assets: [[String: Any]] = (0..<16).map { index in
            ["id": UUID().uuidString.lowercased(), "kind": "video",
             "locator": ["absolute": "/tmp/source-\(index).mov"],
             "streams": [["index": 0, "width": 1920, "height": 1080, "codec": "mpeg4",
                          "duration": ["num": "600", "den": "1"], "start_time": ["num": "0", "den": "1"]]]]
        }
        let tracks: [[String: Any]] = (0..<8).map { _ in ["id": UUID().uuidString.lowercased(), "kind": "video"] }
        let clips: [[String: Any]] = (0..<count).map { index in
            ["track": tracks[index % tracks.count].string("id"), "kind": "video", "clip": [
                "id": UUID().uuidString.lowercased(),
                "source_ref": ["kind": "asset", "asset": assets[index % assets.count].string("id"), "stream_index": 0],
                "timeline_range": ["start": ["num": String(index), "den": "1"], "end": ["num": String(index + 1), "den": "1"]]]]
        }
        var document = EditorModel.newDocument(name: "Performance")
        document["compositions"] = []; document["assets"] = assets
        model.adopt(document: document, scene: [:], revision: "1", actor: "probe", external: false)
        model.ui.page = "edit"
        let query: [String: Any] = ["revision": "1", "sequence": ["id": UUID().uuidString.lowercased(),
            "tracks": tracks, "frame_rate": ["num": "30", "den": "1"]], "clips": clips,
            "asset_status": [["asset": assets[0].string("id"), "error": ["code": "ASSET_MISSING"]]]]
        let updateStart = ContinuousClock.now
        model.sequenceResult = query
        let update = updateStart.duration(to: .now).components
        let updateMS = Double(update.seconds) * 1000 + Double(update.attoseconds) / 1e15
        model.ui.clipSelection = clips.last!.object("clip").string("id")
        var samples: [Double] = [], checksums: [Int] = []
        for _ in 0..<12 {
            let start = ContinuousClock.now
            var checksum = model.editClips.count + Int(model.sequenceDurationFrames)
            checksum += model.selectedClip?.id.count ?? 0
            for track in tracks {
                checksum += model.clips(in: track.string("id")).count
            }
            for clip in model.editClips {
                checksum += model.clipName(clip).utf8.count
                checksum += model.clipMissing(clip)?.utf8.count ?? 0
                checksum += model.trackLocked(clip.track) ? 1 : 0
            }
            let elapsed = start.duration(to: .now).components
            samples.append(Double(elapsed.seconds) * 1000 + Double(elapsed.attoseconds) / 1e15)
            checksums.append(checksum)
        }
        let warm = samples.dropFirst().sorted()
        cases.append(["clips": count, "query_update_ms": updateMS, "cold_ms": samples[0], "warm_p50_ms": warm[5], "warm_p95_ms": warm[9],
                      "samples_ms": samples, "checksums": checksums])
    }
    let report: [String: Any] = ["path": "MainActor timeline presentation getters; no service, rendering or SwiftUI layout", "cases": cases]
    let data = try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
}

MainActor.assumeIsolated { try! run() }
