import AppKit
import QuartzCore
import Darwin
import KronelloCore
import KronelloAppModel

@MainActor final class AudioHarness: NSObject, NSApplicationDelegate {
    let playback = RealtimePlayback()
    let evidence = PlaybackEvidence()
    var window: NSWindow!
    var layer: CAMetalLayer!
    var session: ProjectSession?
    var pendingRender: Task<Void, Error>?
    var rendering = false
    var lastFrame: Int64 = -1
    var producerError: Error?
    func applicationDidFinishLaunching(_ notification: Foundation.Notification) {
        NSApp.setActivationPolicy(.regular)
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 320), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "AUDIO-002 · real device + Metal submission measurement"
        let view = NSView(frame: window.contentView!.bounds)
        layer = CAMetalLayer(); layer.isOpaque = true; layer.drawableSize = CGSize(width: 640, height: 320)
        view.wantsLayer = true; view.layer = layer; window.contentView = view
        window.center(); window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        playback.onFailure = { [weak self] error in self?.producerError = error }
        Task {
            do { try await run(); NSApp.terminate(nil) }
            catch {
                _ = try? await playback.stop()
                fputs("AUDIO-002 host measurement failed: \(error)\n", stderr); exit(1)
            }
        }
    }
    func run() async throws {
        let args = CommandLine.arguments
        guard args.count == 7, let num = Int64(args[3]), let den = Int64(args[4]), num > 0, den > 0 else {
            throw NativeError.service("USAGE", "KronelloAudioHarness PROJECT SEQUENCE FPS_NUM FPS_DEN REVISION OUTPUT.jsonl")
        }
        let path = args[1], target = PlaybackTarget.sequence(args[2]), revision = args[5]
        let output = URL(fileURLWithPath: args[6])
        guard !FileManager.default.fileExists(atPath: output.path) else { throw NativeError.service("OUTPUT_EXISTS", output.path) }
        let session = try ProjectSession(path: path); self.session = session
        _ = try NativeProjectTransport.result(JSONSerialization.jsonObject(with: JSONEncoder().encode(try await session.ready())) as! [String: Any])
        try await session.attach(metalLayer: Unmanaged.passUnretained(layer).toOpaque(), width: 640, height: 320)
        // Warm GPU before the audio engine starts; no CPU reference fallback.
        _ = try await session.redraw(request(path, target, 0, num, den))
        try await playback.start(path: path, target: target, revision: revision, at: 0)
        try requireAudioDevice()
        evidence.event("start", sample: 0, details: ["fps_num": String(num), "fps_den": String(den), "revision": revision])
        let wall = ContinuousClock.now
        var phase = 0
        while wall.duration(to: .now) < .seconds(40) {
            if let producerError { throw producerError }
            try requireAudioDevice()
            let elapsed = wall.duration(to: .now)
            if phase == 0 && elapsed >= .seconds(7) {
                try await drain()
                let sample = try PlaybackMath.seekSample(frame: 137, rateNum: num, rateDen: den)
                try await playback.start(path: path, target: target, revision: revision, at: sample)
                evidence.event("seek", sample: sample, details: ["frame": "137"]); lastFrame = -1; phase = 1
            } else if phase == 1 && elapsed >= .seconds(14) {
                try await drain()
                let sample = try await playback.stop(); evidence.event("stop", sample: sample)
                try await Task.sleep(for: .seconds(1))
                try await playback.start(path: path, target: target, revision: revision, at: sample)
                guard playback.position == sample else { throw NativeError.service("AUDIO_RESUME_MISMATCH", "Resume sample changed") }
                evidence.event("resume", sample: sample); lastFrame = -1; phase = 2
            } else if phase == 2 && elapsed >= .seconds(23) {
                try await drain()
                let sample = try PlaybackMath.seekSample(frame: 777, rateNum: num, rateDen: den)
                try await playback.start(path: path, target: target, revision: revision, at: sample)
                evidence.event("seek", sample: sample, details: ["frame": "777"]); lastFrame = -1; phase = 3
            }
            let sample = try playback.samplePosition()
            let frame = try PlaybackMath.videoFrame(sample: sample, rateNum: num, rateDen: den)
            if !rendering { try await drain() }
            if !rendering && frame != lastFrame && playback.clock?.valid == true {
                rendering = true; lastFrame = frame
                let epoch = playback.clockEpoch
                pendingRender = Task {
                    defer { rendering = false }
                    let response = try await session.redraw(request(path, target, frame, num, den))
                    guard epoch == playback.clockEpoch else { return }
                    try evidence.presented(frame: frame, rateNum: num, rateDen: den, playback: playback, response: response)
                }
            }
            try await Task.sleep(for: .milliseconds(8))
        }
        try await drain()
        let sample = try await playback.stop()
        evidence.event("finish", sample: sample, details: ["underruns": String(playback.clock?.underruns ?? 0), "missing_frames": String(playback.clock?.missingFrames ?? 0)])
        try evidence.write(to: output); session.close()
        print("HOST MEASUREMENT WRITTEN \(output.path) — analyse with scripts/analyze_audio_002.py")
    }
    func requireAudioDevice() throws {
        guard playback.master == .audioDevice else { throw NativeError.service("AUDIO_DEVICE_REQUIRED", playback.status) }
    }
    func drain() async throws { if let pendingRender { try await pendingRender.value }; pendingRender = nil }
    func request(_ path: String, _ target: PlaybackTarget, _ frame: Int64, _ num: Int64, _ den: Int64) throws -> API.Request {
        let product = frame.multipliedReportingOverflow(by: den)
        guard !product.overflow else { throw NativeError.service("TIME_ERROR", "Harness frame overflow") }
        return try NativeProjectTransport.request(["operation": "render.frame", "input": ["project": path, "target": target.wire,
            "region": ["origin": [0, 0], "extent": [64, 32], "pixels": [640, 320]]],
            "time": ["num": String(product.partialValue), "den": String(num)]])
    }
}
MainActor.assumeIsolated {
    let app = NSApplication.shared
    let harness = AudioHarness()
    app.delegate = harness
    withExtendedLifetime(harness) { app.run() }
}
