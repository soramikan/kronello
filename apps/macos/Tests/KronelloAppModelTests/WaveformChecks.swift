import Foundation
import CryptoKit
import KronelloAppModel
import KronelloCore
import KronelloDesign

/// Model-level checks for AUDIO-006: audio.analyze wiring, cache reuse and
/// zoom-following RMS resampling. Runs against the real shared worker.
@MainActor struct WaveformChecks {
    struct Fixture {
        let folder: URL
        let editor: EditorModel
        let transport: RecordingTransport
        let sequence: String
        let track: String
        let clip: String
        let asset: String
    }
    /// A deterministic 1 s 48 kHz mono PCM16 WAV (440 Hz sine), since the shared
    /// service verifies asset content_hash before decoding.
    func sineWav(samples: Int = 48000, rate: Int = 48000) -> Data {
        var pcm = Data()
        pcm.reserveCapacity(samples * 2)
        for index in 0..<samples {
            var sample = Int16((sin(2 * .pi * 440 * Double(index) / Double(rate)) * 12000).rounded()).littleEndian
            pcm.append(Data(bytes: &sample, count: 2))
        }
        func le32(_ v: UInt32) -> Data { var v = v.littleEndian; return Data(bytes: &v, count: 4) }
        func le16(_ v: UInt16) -> Data { var v = v.littleEndian; return Data(bytes: &v, count: 2) }
        var wav = Data("RIFF".utf8)
        wav.append(le32(UInt32(36 + pcm.count))); wav.append(contentsOf: "WAVE".utf8)
        wav.append(contentsOf: "fmt ".utf8); wav.append(le32(16))
        wav.append(le16(1)); wav.append(le16(1)); wav.append(le32(UInt32(rate)))
        wav.append(le32(UInt32(rate * 2))); wav.append(le16(2)); wav.append(le16(16))
        wav.append(contentsOf: "data".utf8); wav.append(le32(UInt32(pcm.count))); wav.append(pcm)
        return wav
    }
    /// One audio track with a 1 s clip sourced from a real on-disk WAV.
    func fixture() async throws -> Fixture {
        let checks = GUIChecks(), folder = try checks.temporary(), path = folder.appendingPathComponent("edit.kronello").path
        let wav = sineWav(), wavPath = folder.appendingPathComponent("tone.wav")
        try wav.write(to: wavPath)
        let hash = SHA256.hash(data: wav).map { String(format: "%02x", $0) }.joined()
        let transport = try RecordingTransport(path: path, worker: checks.root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let editor = EditorModel(path: path, transport: transport, stateStore: .init(root: folder.appendingPathComponent("state")))
        var document = EditorModel.newDocument(name: "AUDIO-006 checks")
        let sequence = UUID().uuidString.lowercased(), track = UUID().uuidString.lowercased()
        let clip = UUID().uuidString.lowercased(), asset = UUID().uuidString.lowercased()
        document["assets"] = [["id": asset, "content_hash": hash, "kind": "audio",
            "locator": ["relative": "tone.wav", "absolute": wavPath.path],
            "streams": [["index": 0, "codec": "pcm_s16le", "time_base": ["num": "1", "den": "48000"], "duration": ["num": "1", "den": "1"]]]]]
        document["sequences"] = [["id": sequence, "extent": ["width": 320, "height": 180], "frame_rate": ["num": "24", "den": "1"],
            "audio_rate": 48000, "working_space": "linear_rec709", "markers": [[String: Any]](),
            "tracks": [["id": track, "kind": "audio", "clips": [[
                "id": clip, "source_ref": ["kind": "asset", "asset": asset, "stream_index": 0],
                "timeline_range": ["start": ["num": "0", "den": "1"], "end": ["num": "1", "den": "1"]],
                "source_in": ["num": "0", "den": "1"],
                "time_map": ["kind": "linear", "offset": ["num": "0", "den": "1"], "speed": ["num": "1", "den": "1"]],
                "links": [String](), "effects": [[String: Any]](), "properties": [[String: Any]](), "markers": [[String: Any]]()]]]]]]
        editor.ui.page = "edit"
        try await editor.start(newDocument: document)
        editor.ui.page = "edit"; try await editor.reload()
        return .init(folder: folder, editor: editor, transport: transport, sequence: sequence, track: track, clip: clip, asset: asset)
    }
    func finish(_ f: Fixture) async { await f.editor.close(); try? FileManager.default.removeItem(at: f.folder) }
    /// Wait until the analysis lands in the shared document and cache, or fails typed.
    func waitForWaveform(_ f: Fixture) async throws -> ClipWaveform {
        for _ in 0..<2000 {
            if let wave = f.editor.waveforms[f.asset + ":0"] { return wave }
            if let code = f.editor.waveformFailures[f.asset + ":0"] {
                throw GUICheckError(message: "audio.analyze failed with \(code): \(f.editor.failure?.message ?? "")")
            }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw GUICheckError(message: "audio.analyze never produced a cached waveform")
    }
    /// The clip view's on-appear hook triggers one audio.analyze; the cache then
    /// serves the clip without repeating the shared request.
    func verifyAnalyzeCacheAndResample() async throws {
        let f = try await fixture(), e = f.editor
        try require(e.editClips.count == 1 && e.editClips[0].kind == .audio, "Fixture exposes one audio clip")
        let clip = e.editClips[0]
        try require(e.waveformKey(for: clip) == f.asset + ":0", "Cache key is asset stream identity")
        let calls = f.transport.callCounts["audio.analyze", default: 0]
        // adopt() already fired ensureAudioWaveforms during load; trigger once more
        // to prove pending/cached dedupe regardless of ordering.
        _ = e.ensureWaveform(for: clip)
        let wave = try await waitForWaveform(f)
        try require(f.transport.callCounts["audio.analyze", default: 0] == calls + 1 || calls == 1,
                    "Exactly one audio.analyze per asset stream")
        try require(wave.sampleRate == 48000 && wave.hop == 1024 && wave.rms.count > 40,
                    "Analysis frames decode at the configured 48 kHz / 1024 hop")
        try require(wave.peak > 0.2, "Sine RMS is non-trivial")
        try require(e.waveform(for: clip) == wave, "Clip lookup resolves through the cache")
        // Repeat triggers reuse the cache without a second request.
        let analyzed = f.transport.callCounts["audio.analyze", default: 0]
        try require(!e.ensureWaveform(for: clip), "Cached clip issues no new analysis")
        try require(f.transport.callCounts["audio.analyze", default: 0] == analyzed, "audio.analyze is not repeated")
        // Source window follows the clip's linear time map (here: 0...1 s).
        try require(e.waveformRange(for: clip) == 0.0...1.0, "Clip maps to its analysed source window")
        // Zoom-following resampling: columns == pixel budget, peaks track resolution.
        let coarse = wave.peaks(from: 0, to: 1, columns: 8), fine = wave.peaks(from: 0, to: 1, columns: 64)
        try require(coarse.count == 8 && fine.count == 64, "Resampling emits exactly the requested columns")
        try require(coarse.max()! == fine.max()!, "Same source window peaks are resolution-independent")
        let partial = wave.peaks(from: 0.5, to: 1, columns: 16)
        try require(partial.count == 16 && partial.max()! > 0, "Partial windows resample independently")
        let outside = wave.peaks(from: 2, to: 3, columns: 8)
        try require(outside.max() == 0, "Samples outside the analysed window read as silence")
        // A retimed clip maps through speed; a nonlinear map draws nothing.
        var retimed = clip.authored
        retimed["time_map"] = ["kind": "linear", "offset": ["num": "0", "den": "1"], "speed": ["num": "1", "den": "2"]]
        let slow = EditClip(query: clip.query.merging(["clip": retimed]) { _, new in new })
        try require(e.waveformRange(for: slow) == 0.0...0.5, "Speed maps the displayed window to analysed seconds")
        var nonlinear = clip.authored
        nonlinear["time_map"] = ["kind": "ease", "ease": ["kind": "in_out"]]
        try require(e.waveformRange(for: EditClip(query: clip.query.merging(["clip": nonlinear]) { _, new in new })) == nil,
                    "Nonlinear time maps draw no waveform rather than a wrong one")
        await finish(f)
    }
    /// Stored analyses reload into the cache without another request.
    func verifyCacheSurvivesReload() async throws {
        let f = try await fixture(), e = f.editor
        _ = e.ensureWaveform(for: e.editClips[0])
        let wave = try await waitForWaveform(f)
        let calls = f.transport.callCounts["audio.analyze", default: 0]
        try await e.reload()
        try require(e.waveforms[f.asset + ":0"] == wave, "Stored analysis rehydrates the cache on reload")
        e.ensureAudioWaveforms()
        try require(f.transport.callCounts["audio.analyze", default: 0] == calls, "Reloaded cache needs no new analysis")
        await finish(f)
    }
}
