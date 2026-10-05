import Foundation
import KronelloAppModel
import KronelloCore

@MainActor struct PlaybackChecks {
    func verifyBinaryProducerAndHostFallback() async throws {
        let folder = try GUIChecks().temporary()
        defer { try? FileManager.default.removeItem(at: folder) }
        let path = folder.appendingPathComponent("audio.kronello").path
        let native = try NativeProjectTransport(path: path, worker: nil)
        defer { native.close() }
        do { try await native.ready() } catch let error as ServiceFailure { try require(error.code == "PROJECT_NOT_FOUND", "fresh audio fixture") }
        let document = EditorModel.newDocument(name: "Playback checks")
        let created = try await native.call(["operation": "project.create", "project": path, "document": document])
        let revision = created.string("revision"), id = document.objects("compositions")[0].string("id")
        let target = PlaybackTarget.composition(id)
        let snapshot = try NativePreparedAudio(path: path, target: target, revision: revision)
        try require(!snapshot.hasAudio, "absence is explicit")
        let buffer = UnsafeMutablePointer<Float>.allocate(capacity: 8194)
        defer { buffer.deallocate() }
        buffer.initialize(repeating: 1, count: 8194)
        try snapshot.render(start: 137, frames: 4096, into: buffer)
        try require((0..<8192).allSatisfy { buffer[$0] == 0 }, "binary render buffer is filled by shared evaluator")
        do { try snapshot.render(start: 0, frames: 4097, into: buffer); throw GUICheckError(message: "unbounded block accepted") }
        catch NativeError.service(let code, _) { try require(code == "INVALID_AUDIO_INPUT", "bounded block typed error") }
        let playback = RealtimePlayback()
        try await playback.start(path: path, target: target, revision: revision, at: 219419)
        try require(playback.master == .hostClock && playback.status.contains("文書音声なし"), "host fallback visible without constructing an audio engine")
        try await Task.sleep(for: .milliseconds(20))
        let stopped = try await playback.stop()
        try require(stopped > 219419 && playback.position == stopped, "host clock derives samples without frame accumulation")
        try await playback.start(path: path, target: target, revision: revision, at: stopped, muted: true)
        try require(playback.position == stopped && playback.status.contains("ミュート"), "resume integer preserved, mute explicitly host clock")
        _ = try await playback.stop()
        do { try await playback.start(path: path, target: target, revision: "999", at: 0); throw GUICheckError(message: "wrong revision accepted") }
        catch NativeError.service(let code, _) { try require(code == "REVISION_CONFLICT", "producer revision error is typed") }
        try FileManager.default.removeItem(atPath: path)
        try snapshot.render(start: 999, frames: 4096, into: buffer)
        try require((0..<8192).allSatisfy { buffer[$0] == 0 }, "owned Rust snapshot never rereads project")
    }
    func verifyPresentationTickDoesNotQueryService() async throws {
        let folder = try GUIChecks().temporary()
        defer { try? FileManager.default.removeItem(at: folder) }
        let path = folder.appendingPathComponent("tick.kronello").path
        let native = try NativeProjectTransport(path: path, worker: nil)
        defer { native.close() }
        do { try await native.ready() } catch {}
        let fake = FakeTransport(), document = fake.document
        let created = try await native.call(["operation": "project.create", "project": path, "document": document])
        let model = EditorModel(path: path, transport: fake, stateStore: .init(root: folder.appendingPathComponent("state")))
        model.ui.composition = document.objects("compositions")[0].string("id")
        model.adopt(document: document, scene: [:], revision: created.string("revision"), actor: "test", external: false)
        model.playing = true
        for _ in 0..<100 {
            if model.playback.master == .hostClock { break }
            try await Task.sleep(for: .milliseconds(5))
        }
        try require(model.playback.master == .hostClock, "test requires explicitly absent audio fallback")
        try await Task.sleep(for: .milliseconds(80))
        model.tick()
        try require(model.frame > 0 && fake.requests.isEmpty, "presentation tick changes time without scene/project/history requests")
        model.playing = false
        try await Task.sleep(for: .milliseconds(20))
        await model.close()
    }
    func runAll() async throws {
        try await verifyBinaryProducerAndHostFallback(); print("PASS binary producer bounds, revision pinning and explicit host/mute clock (no device acceptance)")
        try await verifyPresentationTickDoesNotQueryService(); print("PASS clock-driven EditorModel presentation without per-frame queries")
        try verifySequenceConfiguration(); print("PASS typed Sequence target, NTSC rate, extent and duration integration")
        try verifyCompositionGeometrySurvivesPageTransitions(); print("PASS Composition viewer geometry survives page and playback target transitions")
    }
    func verifySequenceConfiguration() throws {
        let fake = FakeTransport(), model = EditorModel(path: "unused", transport: FakeTransport())
        var document = fake.document
        let id = UUID().uuidString
        document["sequences"] = [["id": id, "extent": ["width": 64, "height": 32],
            "tracks": [["clips": [["timeline_range": ["end": ["num": "180", "den": "1"]]]]]]]]
        model.ui.composition = document.objects("compositions")[0].string("id")
        model.adopt(document: document, scene: [:], revision: "1", actor: "test", external: false)
        model.configurePlayback(target: .sequence(id), rateNum: 24000, rateDen: 1001)
        try require(model.durationFrames == 4316 && model.extent.width == 64 && model.nominalFPS == 24, "Sequence playback must use its own duration, extent and NTSC rate")
        model.ui.time = RationalTime(num: 137 * 1001, den: 24000)
        try require(model.frame == 137, "Sequence frame grid uses configured rate")
    }
    func verifyCompositionGeometrySurvivesPageTransitions() throws {
        let fake = FakeTransport(), model = EditorModel(path: "unused", transport: FakeTransport())
        var document = fake.document
        let id = UUID().uuidString
        document["sequences"] = [["id": id, "extent": ["width": 64, "height": 32]]]
        let composition = document.objects("compositions")[0]
        model.ui.composition = composition.string("id")
        model.ui.page = "motion"
        model.adopt(document: document, scene: [:], revision: "1", actor: "test", external: false)
        let expected = model.compositionExtent
        try require(expected.width > 0 && expected.height > 0, "Composition fixture geometry is valid")
        // SwiftUI can reevaluate the departing Motion view before Edit loads its
        // Sequence query. Playback geometry is temporarily absent in that gap.
        model.ui.page = "edit"
        try require(model.extent == .zero, "reproduce the unloaded Sequence transition")
        try require(model.compositionExtent == expected, "departing Motion viewer retains Composition geometry")
        model.configurePlayback(target: .sequence(id), rateNum: 24, rateDen: 1)
        model.ui.page = "motion"
        try require(model.extent.width == 64 && model.compositionExtent == expected,
                    "entering Motion must not borrow the previous Sequence playback extent")
        model.configurePlayback(target: nil, rateNum: 24, rateDen: 1)
        try require(model.compositionExtent == expected, "Composition geometry is stable after route reset")
    }
}
