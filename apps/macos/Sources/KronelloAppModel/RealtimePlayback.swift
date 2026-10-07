import Foundation
import AVFoundation
import CoreAudio
import Darwin
import KronelloCore

/// A serial producer owns Rust resources, allocation and evaluation. The audio
/// device thread only invokes the preallocated native ring's copy/silence path.
private final class AudioProducer: @unchecked Sendable {
    let queue = DispatchQueue(label: "kronello.audio.producer", qos: .userInitiated)
    let preparation = DispatchQueue(label: "kronello.audio.prepare", qos: .userInitiated)
    let ring: NativeAudioRing
    private let buffer = UnsafeMutablePointer<Float>.allocate(capacity: 8192)
    private var snapshot: NativePreparedAudio?
    private var nextSample: Int64 = 0
    private var running = false
    private var epoch: UInt64 = 0
    private var timer: DispatchSourceTimer?
    var failure: (@Sendable (Error, UInt64) -> Void)?
    /// AUDIO-009: one publication per fill pass carries the last rendered
    /// block's evaluator meters; the callback is tagged with its epoch.
    var meters: (@Sendable (PlaybackMeters, UInt64) -> Void)?
    init(ring: NativeAudioRing) { self.ring = ring }
    deinit { timer?.cancel(); buffer.deallocate() }
    func prepare(path: String, target: PlaybackTarget, revision: String) async throws -> NativePreparedAudio {
        try await withCheckedThrowingContinuation { continuation in
            preparation.async {
                do { continuation.resume(returning: try NativePreparedAudio(path: path, target: target, revision: revision)) }
                catch { continuation.resume(throwing: error) }
            }
        }
    }
    func reset(_ snapshot: NativePreparedAudio?, at sample: Int64, epoch: UInt64) async throws {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                self.running = false; self.timer?.cancel(); self.timer = nil
                self.epoch = epoch
                self.ring.reset(origin: sample); self.snapshot = snapshot; self.nextSample = sample
                do { if snapshot != nil { try self.fill() }; continuation.resume() }
                catch { continuation.resume(throwing: error) }
            }
        }
    }
    func resume() {
        queue.async { [self] in
            self.running = true
            let timer = DispatchSource.makeTimerSource(queue: self.queue)
            timer.schedule(deadline: .now(), repeating: .milliseconds(5), leeway: .milliseconds(1))
            timer.setEventHandler { [weak self] in
                guard let self, self.running else { return }
                do { try self.fill() }
                catch { self.running = false; self.failure?(error, self.epoch) }
            }
            self.timer = timer; timer.resume()
        }
    }
    func switchSnapshot(_ snapshot: NativePreparedAudio) async {
        await withCheckedContinuation { continuation in
            queue.async { self.snapshot = snapshot; continuation.resume() }
        } // The last block is already atomically published. Bound in-flight resources.
    }
    private func fill() throws {
        guard let snapshot else { return }
        let clock = ring.clock
        if clock.valid { nextSample = max(nextSample, clock.sample + Int64(clock.callbackFrames)) }
        // Bounded work per timer firing. Publication is once per complete block.
        var last: PlaybackMeters?
        for _ in 0..<8 {
            guard ring.available >= 4096 else { break }
            last = try snapshot.renderMetered(start: nextSample, frames: 4096, into: buffer)
            guard ring.push(buffer, frames: 4096, start: nextSample, revision: snapshot.revision) else {
                throw NativeError.service("AUDIO_RING_INVARIANT", "Single producer block publication failed")
            }
            nextSample += 4096
        }
        if let last { meters?(last, epoch) }
    }
}

@MainActor public final class RealtimePlayback {
    public enum Master: String { case stopped, preparing, audioDevice = "audio_device", hostClock = "host_clock" }
    public private(set) var master: Master = .stopped
    public private(set) var fallbackReason = ""
    public private(set) var latencySamples: Int64 = 0
    public private(set) var position: Int64 = 0
    public private(set) var pinnedRevision: UInt64 = 0
    public var onFailure: ((Error) -> Void)?
    /// AUDIO-009: latest rendered-block peak/RMS on the main actor, or
    /// `.silent` whenever playback stops.
    public var onMeters: ((PlaybackMeters) -> Void)?
    private var ring: NativeAudioRing?
    private var producer: AudioProducer?
    private var snapshot: NativePreparedAudio?
    private var engine: AVAudioEngine?
    private var source: AVAudioSourceNode?
    private var configurationObserver: NSObjectProtocol?
    private var origin: Int64 = 0
    private var hostOrigin: UInt64 = 0
    private var generation: UInt64 = 0
    private var scrubTask: Task<Void, Never>?
    private var scrubSerial: UInt64 = 0
    private var updateInFlight = false
    private var requestedRevision: String?
    private var muted = false
    public var clockEpoch: UInt64 { generation }
    private var timebase = mach_timebase_info_data_t()
    public init() { mach_timebase_info(&timebase) }
    public var clock: PlaybackClock? { ring?.clock }
    public var status: String {
        let count = clock?.underruns ?? 0
        switch master {
        case .stopped: return "停止 · underrun \(count)"
        case .preparing: return "音声を準備中"
        case .audioDevice: return "音声クロック · underrun \(count)"
        case .hostClock: return "ホストクロック（\(fallbackReason)） · underrun \(count)"
        }
    }
    private func ensureProducer() throws -> AudioProducer {
        if let producer { return producer }
        let ring = try NativeAudioRing(), producer = AudioProducer(ring: ring)
        self.ring = ring; self.producer = producer
        producer.failure = { [weak self] error, epoch in
            Task { @MainActor in
                guard let self, self.clockEpoch == epoch else { return }
                self.onFailure?(error)
            }
        }
        producer.meters = { [weak self] meters, epoch in
            Task { @MainActor in
                guard let self, self.clockEpoch == epoch else { return }
                self.onMeters?(meters)
            }
        }
        return producer
    }
    public func start(path: String, target: PlaybackTarget, revision: String, at sample: Int64, muted: Bool = false) async throws {
        try await start(path: path, target: target, revision: revision, at: sample, muted: muted, scrub: false)
    }
    private func start(path: String, target: PlaybackTarget, revision: String, at sample: Int64, muted: Bool, scrub: Bool) async throws {
        if !scrub { scrubSerial &+= 1; scrubTask?.cancel(); scrubTask = nil }
        guard sample >= 0 else { throw NativeError.service("INVALID_AUDIO_INPUT", "Negative playback sample") }
        let token = generation + 1 // stop() invalidates older requests before its first suspension.
        _ = try await stop()
        guard token == generation else { throw CancellationError() }
        master = .preparing; origin = sample; position = sample
        self.muted = muted
        let producer = try ensureProducer()
        do {
            let snapshot = try await producer.prepare(path: path, target: target, revision: revision)
            guard token == generation else { throw CancellationError() }
            self.snapshot = snapshot; pinnedRevision = snapshot.revision
            try await producer.reset(snapshot.hasAudio && !muted ? snapshot : nil, at: sample, epoch: token)
            guard token == generation else { throw CancellationError() }
            if muted || !snapshot.hasAudio {
                beginHostClock(muted ? "ミュート" : "文書音声なし"); return
            }
            var address = AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDefaultOutputDevice,
                mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
            var device = AudioDeviceID(kAudioObjectUnknown), size = UInt32(MemoryLayout<AudioDeviceID>.size)
            let deviceStatus = AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &device)
            guard deviceStatus == noErr else { throw NativeError.service("AUDIO_DEVICE_QUERY_FAILED", "Default output query failed: \(deviceStatus)") }
            guard device != kAudioObjectUnknown else { beginHostClock("音声デバイスなし"); return }
            let engine = AVAudioEngine()
            guard engine.outputNode.outputFormat(forBus: 0).sampleRate > 0,
                  engine.outputNode.outputFormat(forBus: 0).channelCount > 0 else {
                beginHostClock("音声デバイスなし"); return
            }
            guard let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 48000, channels: 2, interleaved: false), let ring else {
                throw NativeError.service("AUDIO_FORMAT_UNAVAILABLE", "48 kHz stereo f32 unavailable")
            }
            // This callback only copies/silences using C11 lock-free atomics.
            let source = AVAudioSourceNode(format: format) { isSilence, timestamp, frames, buffers in
                ring.consume(time: timestamp, frames: frames, buffers: buffers)
                isSilence.pointee = false
                return noErr
            }
            engine.attach(source); engine.connect(source, to: engine.mainMixerNode, format: format)
            engine.prepare()
            self.engine = engine; self.source = source
            try engine.start(); producer.resume(); master = .audioDevice
            let latency = (engine.outputNode.outputPresentationLatency * 48000).rounded(.up)
            guard latency.isFinite, latency >= 0, latency < Double(Int64.max) else {
                throw NativeError.service("AUDIO_LATENCY_INVALID", "Output device latency is invalid")
            }
            latencySamples = Int64(latency)
            configurationObserver = NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: .main) { [weak self] _ in
                Task { @MainActor in
                    guard let self, self.master == .audioDevice else { return }
                    self.onFailure?(NativeError.service("AUDIO_DEVICE_CHANGED", "音声デバイスが変わりました。再生を停止して再準備してください"))
                }
            }
        } catch {
            guard token == generation else { throw CancellationError() }
            engine?.stop(); master = .stopped
            throw error
        }
    }
    private func beginHostClock(_ reason: String) {
        latencySamples = 0; fallbackReason = reason; hostOrigin = mach_absolute_time(); master = .hostClock
    }
    /// AUDIO-009: audio scrub is a bounded run of the same prepare / render /
    /// ring / device pipeline — never a separate evaluator. A short delay
    /// coalesces rapid seeks into one start; every run stops itself after a
    /// fixed window, and ordinary playback cancels any pending run.
    public func scrub(path: String, target: PlaybackTarget, revision: String, at sample: Int64) {
        guard sample >= 0 else { return }
        scrubSerial &+= 1
        let serial = scrubSerial
        scrubTask?.cancel()
        scrubTask = Task { [weak self] in
            guard let self else { return }
            do {
                try await Task.sleep(for: .milliseconds(30))
                guard !Task.isCancelled, self.scrubSerial == serial, self.master != .preparing else { return }
                try await self.start(path: path, target: target, revision: revision, at: sample, muted: false, scrub: true)
                try await Task.sleep(for: .milliseconds(160))
                guard !Task.isCancelled, self.scrubSerial == serial else { return }
                _ = try await self.stop()
            } catch is CancellationError { } catch { self.onFailure?(error) }
        }
    }
    public func samplePosition(hostTime: UInt64 = mach_absolute_time()) throws -> Int64 {
        switch master {
        case .audioDevice:
            guard let clock = ring?.clock else { return position }
            if clock.timestampError { throw NativeError.service("AUDIO_TIMESTAMP_INVALID", "Device render timestamp or audio format changed") }
            // A bounded atomic read may miss a concurrent clock publication.
            // Retain the last position rather than jump back to the seek origin.
            guard clock.valid else { return position }
            let ticks = hostTime >= clock.hostTime ? hostTime - clock.hostTime : clock.hostTime - hostTime
            let elapsed = try PlaybackMath.hostSamples(ticks: ticks, numerator: timebase.numer, denominator: timebase.denom)
            if hostTime >= clock.hostTime && elapsed > max(48000, Int64(clock.callbackFrames) * 4) {
                throw NativeError.service("AUDIO_CLOCK_STALLED", "Audio device callbacks stopped advancing")
            }
            let sample = hostTime >= clock.hostTime ? clock.sample.addingReportingOverflow(elapsed) : clock.sample.subtractingReportingOverflow(elapsed)
            guard !sample.overflow else { throw NativeError.service("TIME_ERROR", "Device clock overflow") }
            let compensated = sample.partialValue.subtractingReportingOverflow(latencySamples)
            guard !compensated.overflow else { throw NativeError.service("TIME_ERROR", "Latency compensation overflow") }
            position = max(origin, compensated.partialValue)
        case .hostClock:
            let elapsed = try PlaybackMath.hostSamples(ticks: hostTime >= hostOrigin ? hostTime - hostOrigin : 0, numerator: timebase.numer, denominator: timebase.denom)
            let sample = origin.addingReportingOverflow(elapsed)
            guard !sample.overflow else { throw NativeError.service("TIME_ERROR", "Host clock overflow") }
            position = sample.partialValue
        case .stopped, .preparing: break
        }
        return position
    }
    /// Stop retains the exact audible sample, then flushes only after both
    /// endpoints are quiescent. Resume uses this integer, not displayed frame.
    @discardableResult public func stop() async throws -> Int64 {
        generation += 1
        let token = generation
        var clockError: Error?
        let sample: Int64
        do { sample = try samplePosition() }
        catch { clockError = error; sample = position }
        master = .stopped; engine?.stop()
        if let observer = configurationObserver { NotificationCenter.default.removeObserver(observer) }
        configurationObserver = nil; engine = nil; source = nil
        if let producer { try await producer.reset(nil, at: sample, epoch: token) }
        guard token == generation else { return sample }
        origin = sample; position = sample
        onMeters?(.silent)
        if let clockError { throw clockError }
        return sample
    }
    public func updateSnapshot(path: String, target: PlaybackTarget, revision: String) async throws {
        requestedRevision = revision
        guard !updateInFlight, master == .audioDevice || master == .hostClock, let producer else { return }
        updateInFlight = true
        defer { updateInFlight = false }
        let token = generation
        while let revision = requestedRevision {
            requestedRevision = nil
            let next: NativePreparedAudio
            do { next = try await producer.prepare(path: path, target: target, revision: revision) }
            catch {
                guard generation == token else { return }
                if requestedRevision != nil { continue }
                throw error
            }
            guard generation == token else { return }
            if requestedRevision != nil { continue } // Coalesce before allocating another candidate.
            if !muted && next.hasAudio != (snapshot?.hasAudio ?? false) {
                // Adding/removing the final audio source changes the master/graph.
                let sample = try samplePosition()
                snapshot = next
                try await start(path: path, target: target, revision: revision, at: sample, muted: muted)
                return
            }
            if master == .audioDevice { await producer.switchSnapshot(next) }
            snapshot = next; pinnedRevision = next.revision
        }
    }
}
