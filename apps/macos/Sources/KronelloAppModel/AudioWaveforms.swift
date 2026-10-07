import Foundation

/// Decoded RMS rows of one `AudioAnalysisDataAsset`, cached per asset stream.
/// The asset is immutable (hash-verified at decode), so the cache never expires.
public struct ClipWaveform: Equatable, Sendable {
    public let key: String
    public let analysisID: String
    public let startSample: Int64
    public let hop: Int64
    public let sampleRate: Int64
    public let rms: [Float]
    public init?(analysis: [String: Any], key: String) {
        let config = analysis.object("config"), frames = analysis.objects("frames")
        guard !analysis.string("id").isEmpty, !frames.isEmpty else { return nil }
        self.key = key
        analysisID = analysis.string("id")
        startSample = Int64(analysis.string("start_sample")) ?? 0
        hop = max(1, Int64(config.string("hop")) ?? 1)
        sampleRate = max(1, Int64(config.string("sample_rate")) ?? 48000)
        rms = frames.map { Float($0.number("rms")) }
    }
    /// Per-column peak RMS over a source-seconds window. Columns outside the
    /// analysed domain read as silence rather than inventing data.
    public func peaks(from startSeconds: Double, to endSeconds: Double, columns: Int) -> [Float] {
        guard columns > 0, !rms.isEmpty else { return [] }
        let span = endSeconds - startSeconds
        guard span > 0, span.isFinite else { return [Float](repeating: 0, count: columns) }
        var result = [Float](repeating: 0, count: columns)
        for column in 0..<columns {
            let lo = startSeconds + span * Double(column) / Double(columns)
            let hi = startSeconds + span * Double(column + 1) / Double(columns)
            var first = Int((lo * Double(sampleRate) - Double(startSample)) / Double(hop))
            var last = Int((hi * Double(sampleRate) - Double(startSample)) / Double(hop))
            first = max(0, first)
            last = min(rms.count - 1, max(first, last))
            guard first < rms.count else { continue }
            result[column] = rms[first...last].max() ?? 0
        }
        return result
    }
    public var peak: Float { rms.max() ?? 0 }
}

extension RationalTime {
    /// Presentation-only seconds; authored time stays rational elsewhere.
    public var seconds: Double { (Double(num) ?? 0) / max(1, Double(den) ?? 1) }
}

extension EditorModel {
    /// Fixed analysis configuration for clip waveforms: ~21 ms hop, no bands.
    /// The work budget covers roughly 95 s of 48 kHz audio per asset stream;
    /// longer sources record a typed failure instead of retrying forever.
    public static let waveformConfig: [String: Any] = [
        "version": 1, "sample_rate": 48000, "window": 1024, "hop": 1024,
        "bands": [Any](),
        "time_map": ["kind": "linear", "offset": ["num": "0", "den": "1"], "speed": ["num": "1", "den": "1"]]]
    /// Cache key shared between clip lookup, pending dedupe and failure state.
    public func waveformKey(for clip: EditClip) -> String? {
        let source = clip.authored.object("source_ref")
        guard source.string("kind") == "asset", !source.string("asset").isEmpty else { return nil }
        return source.string("asset") + ":" + source.string("stream_index")
    }
    public func waveform(for clip: EditClip) -> ClipWaveform? {
        waveformKey(for: clip).flatMap { waveforms[$0] }
    }
    /// Source-seconds window the clip samples under its linear time map;
    /// nil for nonlinear maps, which draw no waveform rather than a wrong one.
    public func waveformRange(for clip: EditClip) -> ClosedRange<Double>? {
        guard let rate = clip.linearRate, let num = Int64(rate.num), let den = Int64(rate.den), den > 0 else { return nil }
        let speed = Double(num) / Double(den)
        let offset = RationalTime.wire(clip.authored.object("time_map").object("offset")).seconds
        let duration = clip.end.seconds - clip.start.seconds
        let first = RationalTime.wire(clip.authored.object("source_in")).seconds + offset
        return clip.reversed ? (first - duration * speed)...first : first...(first + duration * speed)
    }
    /// Pull newly stored analyses out of the shared document into the cache.
    func refreshWaveformCache() {
        for analysis in document.objects("audio_analyses") {
            let source = analysis.object("source")
            guard source.string("kind") == "asset", !source.string("asset").isEmpty else { continue }
            let key = source.string("asset") + ":" + source.string("stream_index")
            guard waveforms[key] == nil else { continue }
            if let wave = ClipWaveform(analysis: analysis, key: key) { waveforms[key] = wave }
        }
    }
    /// Analyze every audio clip source on the current sequence, once per asset stream.
    public func ensureAudioWaveforms() {
        guard ui.page == "edit", sequenceFailure == nil else { return }
        for clip in editClips where clip.kind == .audio { ensureWaveform(for: clip) }
    }
    /// Kick a bounded `audio.analyze` for the clip's asset stream when no cached
    /// or stored analysis exists. analyze is a mutating call, so it only runs
    /// while no edit transaction is open; a stale base retries on the next reload.
    @discardableResult
    public func ensureWaveform(for clip: EditClip) -> Bool {
        guard let key = waveformKey(for: clip), waveforms[key] == nil,
              !waveformPending.contains(key), waveformFailures[key] == nil,
              !busy, pendingCandidate == nil, timelineCandidate == nil else { return false }
        let source = clip.authored.object("source_ref")
        waveformPending.insert(key)
        let base = revision, analysisID = UUID().uuidString.lowercased()
        let streamIndex = Int(source.string("stream_index")) ?? 0
        Task {
            defer { waveformPending.remove(key) }
            do {
                _ = try await request("audio.analyze", [
                    "base_revision": base, "id": analysisID,
                    "input": ["kind": "asset", "asset": source.string("asset"), "stream_index": streamIndex],
                    "config": Self.waveformConfig,
                    "idempotency_key": UUID().uuidString.lowercased()])
                try await reload()
            } catch {
                let failure = serviceFailure(error)
                if failure.code != "REVISION_CONFLICT" { waveformFailures[key] = failure.code }
            }
        }
        return true
    }
}
