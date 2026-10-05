import Foundation
import AudioToolbox
import CKronelloFFI

public enum PlaybackTarget: Equatable, Sendable {
    case composition(String), sequence(String)
    public var wire: [String: String] {
        switch self {
        case .composition(let id): return ["kind": "composition", "composition": id]
        case .sequence(let id): return ["kind": "sequence", "sequence": id]
        }
    }
}

/// One serial producer owns preparation, block rendering and destruction.
public final class NativePreparedAudio: @unchecked Sendable {
    private let resource: UnsafeMutableRawPointer
    public let revision: UInt64
    public let hasAudio: Bool
    // PreparedAudio is immutable Send + Sync Rust data; ARC keeps its allocation
    // live while transferred from the preparation queue to the serial producer.
    public init(path: String, target: PlaybackTarget, revision: String) throws {
        let data = try JSONSerialization.data(withJSONObject: ["project": path, "target": target.wire, "expected_revision": revision], options: [.sortedKeys])
        var error: UnsafeMutablePointer<CChar>?
        var audible = false
        let resource = data.withUnsafeBytes { kronello_audio_prepare($0.bindMemory(to: UInt8.self).baseAddress, $0.count, &audible, &error) }
        try Self.check(error)
        guard let resource, let parsed = UInt64(revision) else { throw NativeError.rejected }
        self.resource = resource; self.revision = parsed; hasAudio = audible
    }
    deinit { kronello_audio_free(resource) }
    public func render(start: Int64, frames: Int, into buffer: UnsafeMutablePointer<Float>) throws {
        var error: UnsafeMutablePointer<CChar>?
        let ok = kronello_audio_render(resource, start, frames, buffer, &error)
        try Self.check(error)
        guard ok else { throw NativeError.rejected }
    }
    private static func check(_ error: UnsafeMutablePointer<CChar>?) throws {
        guard let error else { return }
        defer { kronello_free(error) }
        let data = Data(String(cString: error).utf8)
        let value = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        throw NativeError.service(value?["code"] as? String ?? "INVALID_RESPONSE", value?["message"] as? String ?? "Audio producer failed")
    }
}

public struct PlaybackClock: Sendable {
    public let sample: Int64
    public let hostTime: UInt64
    public let underruns: UInt64
    public let missingFrames: UInt64
    public let revision: UInt64
    public let callbackFrames: UInt32
    public let valid: Bool
    public let timestampError: Bool
}

/// The native C consumer is lock-free and has no Swift/Rust/JSON work.
/// Reset requires engine.stop() plus a producer barrier. The callback captures
/// this already allocated object; consume creates no collection or closure.
public final class NativeAudioRing: @unchecked Sendable {
    public static let capacity = 32768
    private let ring: OpaquePointer
    public init() throws {
        guard let ring = kr_audio_create() else { throw NativeError.service("AUDIO_RING_UNAVAILABLE", "Lock-free audio atomics unavailable") }
        self.ring = ring
    }
    deinit { kr_audio_free(ring) }
    public var available: Int { Int(kr_audio_available(ring)) }
    public func reset(origin: Int64) { kr_audio_reset(ring, origin) }
    public func push(_ buffer: UnsafePointer<Float>, frames: Int, start: Int64, revision: UInt64) -> Bool {
        guard frames > 0 && frames <= 4096 else { return false }
        return kr_audio_push(ring, buffer, UInt32(frames), start, revision)
    }
    public func consume(time: UnsafePointer<AudioTimeStamp>, frames: UInt32, buffers: UnsafeMutablePointer<AudioBufferList>) {
        kr_audio_consume(ring, time, frames, buffers)
    }
    public var clock: PlaybackClock {
        let c = kr_audio_clock(ring)
        return .init(sample: c.sample, hostTime: c.host_time, underruns: c.underruns, missingFrames: c.missing_frames,
                     revision: c.revision, callbackFrames: c.callback_frames, valid: c.valid, timestampError: c.timestamp_error)
    }
}

public enum PlaybackMath {
    public static func seekSample(frame: Int64, rateNum: Int64, rateDen: Int64) throws -> Int64 {
        try checked(kr_audio_seek_sample(frame, rateNum, rateDen))
    }
    public static func videoFrame(sample: Int64, latency: Int64 = 0, rateNum: Int64, rateDen: Int64) throws -> Int64 {
        try checked(kr_audio_video_frame(sample, latency, rateNum, rateDen))
    }
    public static func hostSamples(ticks: UInt64, numerator: UInt32, denominator: UInt32) throws -> Int64 {
        try checked(kr_audio_host_samples(ticks, numerator, denominator))
    }
    private static func checked(_ value: Int64) throws -> Int64 {
        guard value >= 0 else { throw NativeError.service("TIME_ERROR", "Playback clock overflow or invalid input") }
        return value
    }
}
