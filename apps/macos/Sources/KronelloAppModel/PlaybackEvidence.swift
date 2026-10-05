import Foundation
import Darwin
import KronelloCore

/// Debug evidence is collected on MainActor, never the render callback. The
/// timestamp is native Metal submission; physical output needs loopback capture.
@MainActor public final class PlaybackEvidence {
    public private(set) var records: [[String: Any]] = []
    public init() {}
    public func event(_ name: String, sample: Int64, details: [String: Any] = [:]) {
        var record = details
        record["record"] = "event"; record["name"] = name; record["sample"] = String(sample)
        record["host_time"] = String(mach_absolute_time()); records.append(record)
    }
    public func presented(frame: Int64, rateNum: Int64, rateDen: Int64, playback: RealtimePlayback, response: JSONValue) throws {
        let data = try JSONEncoder().encode(response)
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw NativeError.rejected }
        let preview = object.object("preview")
        guard preview["presented"] as? Bool == true else {
            event("video_skipped", sample: playback.position, details: ["reason": preview.string("skipped")]); return
        }
        guard let hostTime = UInt64(preview.string("presentation_host_time")), let clock = playback.clock, clock.valid else {
            throw NativeError.service("AUDIO_EVIDENCE_UNAVAILABLE", "Presentation/device timestamp missing")
        }
        let sample = try playback.samplePosition(hostTime: hostTime)
        let expected = try PlaybackMath.videoFrame(sample: sample, rateNum: rateNum, rateDen: rateDen)
        records.append(["record": "presentation", "master": playback.master.rawValue,
            "epoch": String(playback.clockEpoch),
            "host_time": String(hostTime), "callback_host_time": String(clock.hostTime),
            "audio_sample": String(sample), "callback_sample": String(clock.sample), "callback_frames": clock.callbackFrames,
            "latency_samples": String(playback.latencySamples), "frame": String(frame), "expected_frame": String(expected),
            "offset_frames": String(frame - expected), "fps_num": String(rateNum), "fps_den": String(rateDen),
            "underruns": String(clock.underruns), "missing_frames": String(clock.missingFrames),
            "audio_revision": String(clock.revision), "video_revision": preview.string("revision")])
    }
    public func write(to url: URL) throws {
        let lines = try records.map { try JSONSerialization.data(withJSONObject: $0, options: [.sortedKeys]) }
        var data = Data()
        for line in lines { data.append(line); data.append(10) }
        try data.write(to: url, options: .atomic)
    }
}
