import Foundation

/// GUI-012 (ADR-0138): mixer loudness readouts. BS.1770 measurement runs
/// through the shared `audio.loudness` query — the mixer never analyzes PCM
/// itself, so GUI, CLI and MCP report identical values. Measurement is
/// on-demand (per strip and master), keyed by (target, revision) so an edit
/// invalidates stale readings automatically.
public struct LoudnessReading: Equatable, Sendable {
    public let integratedLUFS: Double?
    public let truePeakDBTP: Double?
    public init(integratedLUFS: Double?, truePeakDBTP: Double?) {
        self.integratedLUFS = integratedLUFS
        self.truePeakDBTP = truePeakDBTP
    }
    /// Display string for a strip readout; silence/inaudible is explicit.
    public var label: String {
        guard let lufs = integratedLUFS else { return "-∞ LUFS" }
        return String(format: "%.1f LUFS", lufs)
    }
}

extension EditorModel {
    /// `audio.loudness` input for one audio track, measured in place with
    /// every other track muted in the service-side snapshot.
    private func loudnessInput(track: String) -> [String: Any] {
        ["kind": "track", "sequence": sequence.string("id"), "track": track]
    }
    /// Whole-sequence input for the master strip (full content extent).
    private var loudnessInputSequence: [String: Any] {
        ["kind": "sequence", "sequence": sequence.string("id")]
    }
    private func measure(_ input: [String: Any], key: String) async {
        guard !busy else { return }
        loudnessBusy.insert(key)
        defer { loudnessBusy.remove(key) }
        do {
            let result = try await request("audio.loudness", ["base_revision": revision, "input": input])
            loudnessReadings[key] = LoudnessReading(
                integratedLUFS: result["integrated_lufs"] as? Double,
                truePeakDBTP: result["true_peak_dbtp"] as? Double)
            // The reported revision pins the reading; a later edit hides it.
            loudnessRevisions[key] = result.string("revision")
        } catch { mapFailure(error) }
    }
    /// Kick a per-track measurement; the readout appears when the service
    /// responds. Re-measure after edits is the caller's choice.
    public func measureTrackLoudness(_ track: [String: Any]) {
        let id = track.string("id")
        guard track.string("kind") == "audio", !id.isEmpty else { return }
        Task { await measure(loudnessInput(track: id), key: "track:\(id)") }
    }
    public func measureSequenceLoudness() {
        guard !sequence.string("id").isEmpty else { return }
        Task { await measure(loudnessInputSequence, key: "sequence") }
    }
    /// Cached readout for a strip; nil until measured, and hidden once stale
    /// (an edit since the measurement means the value no longer applies).
    public func trackLoudness(_ track: [String: Any]) -> LoudnessReading? {
        reading(for: "track:\(track.string("id"))")
    }
    public var sequenceLoudness: LoudnessReading? { reading(for: "sequence") }
    private func reading(for key: String) -> LoudnessReading? {
        guard loudnessRevisions[key] == revision else { return nil }
        return loudnessReadings[key]
    }
    public func loudnessMeasuring(_ track: [String: Any]) -> Bool {
        loudnessBusy.contains("track:\(track.string("id"))")
    }
    public var sequenceLoudnessMeasuring: Bool { loudnessBusy.contains("sequence") }
    /// AUDIO-008 normalize through the shared edit path: the service measures
    /// the clip's integrated loudness and appends one `kronello.audio.gain`
    /// effect. Target is clamped to the service's accepted broadcast range.
    public func normalizeClip(_ clip: EditClip, targetLUFS: Double = -23.0) {
        guard !trackLocked(clip.track), clip.kind == .audio,
              targetLUFS.isFinite, (-70...(-5)).contains(targetLUFS) else { return }
        submitDirect("audio.normalize", [
            "sequence": sequence.string("id"), "clip": clip.id, "target_lufs": targetLUFS,
        ], label: "ラウドネス正規化")
    }
}
