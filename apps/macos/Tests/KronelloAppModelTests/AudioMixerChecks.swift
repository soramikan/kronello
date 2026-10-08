import Foundation
import KronelloAppModel
import KronelloCore
import KronelloDesign

/// Model-level checks for AUDIO-009: the metered FFI render decodes the
/// evaluator's per-block peak/RMS, scrubbing is a bounded run of the shared
/// playback pipeline, meters decay to silence on stop, and mixer faders
/// commit through the shared edit API as a single undoable event.
@MainActor struct AudioMixerChecks {
    /// adopt() fires audio.analyze, which writes its receipt into the store
    /// and bumps the revision. Wait for that background write, then reload so
    /// e.revision is the value audio preparation pins.
    private func settleRevision(_ checks: WaveformChecks, _ f: WaveformChecks.Fixture) async throws {
        _ = try await checks.waitForWaveform(f)
        try await f.editor.reload()
    }
    /// kronello_audio_render_metered fills the same PCM as the plain render
    /// and reports master + per-track levels keyed by the sequence TrackId.
    func verifyMeteredRender() async throws {
        let checks = WaveformChecks(), f = try await checks.fixture(), e = f.editor
        try await settleRevision(checks, f)
        let snapshot = try NativePreparedAudio(path: e.path, target: .sequence(f.sequence), revision: e.revision)
        try require(snapshot.hasAudio, "fixture has decodable audio")
        let plain = UnsafeMutablePointer<Float>.allocate(capacity: 8192)
        let metered = UnsafeMutablePointer<Float>.allocate(capacity: 8192)
        defer { plain.deallocate(); metered.deallocate() }
        try snapshot.render(start: 0, frames: 4096, into: plain)
        let meters = try snapshot.renderMetered(start: 0, frames: 4096, into: metered)
        try require((0..<8192).allSatisfy { plain[$0] == metered[$0] }, "metered render shares the evaluation path")
        try require(meters.master_peak.count == 2 && meters.masterPeak > 0.3, "master peak measured by the evaluator")
        try require(meters.masterRms > 0.2 && meters.masterRms <= meters.masterPeak, "master RMS stays bounded by peak")
        try require(meters.tracks.count == 1 && meters.track(f.track)?.stereoPeak == meters.masterPeak,
                    "track meter is keyed by the sequence's TrackId")
        let silence = try snapshot.renderMetered(start: 2 * 48000, frames: 4096, into: metered)
        try require(silence.masterPeak == 0 && silence.tracks.isEmpty, "outside the clip reads as silence")
        await checks.finish(f)
    }
    /// A seek with scrub enabled starts a bounded run of the realtime
    /// pipeline: meters reach the model during the run, playback stops
    /// itself, and the transport flag / document stay untouched. Disabled or
    /// muted monitoring keeps seeks silent.
    func verifyScrubPublishesMetersAndStops() async throws {
        let checks = WaveformChecks(), f = try await checks.fixture(), e = f.editor
        try await settleRevision(checks, f)
        e.ui.page = "edit"
        e.configurePlayback(target: .sequence(f.sequence), rateNum: e.rateNum, rateDen: e.rateDen)
        let base = e.revision, applies = f.transport.applyCount
        e.audioScrubEnabled = false
        let disabledEpoch = e.playback.clockEpoch
        e.seek(5)
        try await Task.sleep(for: .milliseconds(80))
        try require(e.playback.clockEpoch == disabledEpoch && e.playback.master == .stopped,
                    "disabled scrub keeps seeks silent")
        e.audioScrubEnabled = true; e.playbackMuted = true
        e.seek(6)
        try await Task.sleep(for: .milliseconds(80))
        try require(e.playback.clockEpoch == disabledEpoch, "muted monitoring suppresses scrub")
        e.playbackMuted = false
        e.seek(6)
        var sawRun = false, sawLevels = false
        for _ in 0..<1400 {
            if e.playback.master != .stopped { sawRun = true }
            if (e.playbackMeters?.masterPeak ?? 0) > 0 { sawLevels = true }
            if sawRun && e.playback.master == .stopped { break }
            if let failure = e.failure ?? e.revisionConflict { throw failure }
            try await Task.sleep(for: .milliseconds(5))
        }
        try require(sawRun, "scrub starts through the shared playback pipeline")
        try require(e.playback.master == .stopped, "scrub run stops itself after the bounded window")
        try require(!e.playing && e.frame == 6, "scrub never flips the transport or moves the playhead")
        try require(sawLevels, "evaluator meters reached the model during the run")
        try await Task.sleep(for: .milliseconds(50))
        try require(e.playbackMeters?.masterPeak == 0, "meters decay to silence after the run")
        try require(e.revision == base && f.transport.applyCount == applies, "scrub authors no edits")
        await checks.finish(f)
    }
    /// The mixer fader writes every clip's shared kronello.audio.volume
    /// Property as one edit.apply event; session undo restores it.
    func verifyTrackVolumeSharedEdit() async throws {
        let checks = WaveformChecks(), f = try await checks.fixture(), e = f.editor
        try await settleRevision(checks, f)
        guard let track = e.sequence.objects("tracks").first(where: { $0.string("id") == f.track }) else {
            throw GUICheckError(message: "audio track missing from the sequence query")
        }
        try require(e.trackVolume(track) == 1.0, "authored clips default to unity gain")
        let base = e.revision, applies = f.transport.applyCount
        e.setTrackVolume(track, gain: 0.5)
        try await MotionChecks().waitForEdit(e, after: base)
        try require(f.transport.applyCount == applies + 1, "one fader commit is one undoable edit")
        let commands = f.transport.lastApply.objects("commands")
        try require(commands.count == 1, "one clip on the track yields one command")
        let volume = commands[0].object("timeline").object("clip_set_volume")
        try require(volume.string("clip") == f.clip
                    && volume.object("volume").object("descriptor").string("key") == "kronello.audio.volume"
                    && volume.object("volume").object("source").object("value").number("value") == 0.5,
                    "fader writes the shared linear gain Property")
        guard let updated = e.sequence.objects("tracks").first(where: { $0.string("id") == f.track }) else {
            throw GUICheckError(message: "audio track lost after the volume edit")
        }
        try require(e.trackVolume(updated) == 0.5, "reloaded track reflects the committed gain")
        await e.undo()
        guard let restored = e.sequence.objects("tracks").first(where: { $0.string("id") == f.track }) else {
            throw GUICheckError(message: "audio track lost after undo")
        }
        try require(e.trackVolume(restored) == 1.0, "session undo restores unity in one event")
        await checks.finish(f)
    }
    func runAll() async throws {
        try await verifyMeteredRender(); print("PASS metered FFI render decodes evaluator peak/RMS with identical PCM")
        try await verifyScrubPublishesMetersAndStops(); print("PASS audio scrub runs the shared pipeline, publishes meters, stops itself")
        try await verifyTrackVolumeSharedEdit(); print("PASS mixer fader commits one undoable clip_set_volume through the shared edit API")
    }
}
