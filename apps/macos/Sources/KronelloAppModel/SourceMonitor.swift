import Foundation
import KronelloCore
import KronelloDesign

/// GUI-011 source-monitor session state (ADR-0128). Presentation only: the
/// loaded preview source, its source-local monitor time, the In/Out edit
/// range and an explicit destination-track override. Every project mutation
/// flows through the shared `edit.insert` / `edit.overwrite` operations.
public struct SourceMonitor: Equatable, Sendable {
    public var source: SourcePreview
    /// Display label (asset filename, group or composition name).
    public var name: String
    /// Source-local monitor time in rational seconds; feeds `render.frame`
    /// on the source surface. Never persisted as floating point.
    public var time: RationalTime
    /// Source In point in source-local time. nil means the source window
    /// start (the effective In defaults to the media edge).
    public var inPoint: RationalTime?
    /// Source Out point in source-local time. nil leaves the edit range open
    /// and disables insert/overwrite until the user sets it.
    public var outPoint: RationalTime?
    /// Explicit destination track id; nil resolves Sequence.targets by kind.
    public var track: String?
}

/// NLE-007: one multicam angle as read back from `document.multicams`.
public struct MulticamAngleInfo: Equatable, Sendable, Identifiable {
    public let id: String
    public let name: String
    /// Deterministic label for a blank `name` (source filename, else
    /// "アングル N"). Labels are presentation only; `id` stays the identity.
    public let fallbackName: String
    public let asset: String
    public let streamIndex: Int
    /// `media_time = multicam_time + sync_offset` (ADR-0127).
    public let syncOffset: RationalTime
    public var displayName: String { name.isEmpty ? fallbackName : name }
}

/// NLE-007: a multicam group with its ordered angles.
public struct MulticamGroupInfo: Equatable, Sendable, Identifiable {
    public let id: String
    public let name: String
    public let angles: [MulticamAngleInfo]
}

extension EditClip {
    /// NLE-007: `(group, angle)` for multicam clips; nil otherwise.
    public var multicam: (group: String, angle: String)? {
        let ref = authored.object("source_ref")
        guard ref.string("kind") == "multicam" else { return nil }
        return (ref.string("multicam"), ref.string("angle"))
    }
    /// GUI-011: previewable source for the source monitor; nil for generator,
    /// caption and adjustment clips (matching `SourcePreviewRef` narrowing).
    public var sourcePreview: SourcePreview? {
        SourcePreview.fromSourceRef(authored.object("source_ref"))
    }
    /// Clip-local source time at a given timeline position.
    public func sourceTime(at timeline: RationalTime) -> RationalTime {
        let sourceIn = RationalTime.wire(authored.object("source_in"))
        let offset = (try? timeline.checkedSubtracting(start)) ?? RationalTime(num: 0, den: 1)
        return (try? sourceIn.checkedAdding(offset)) ?? sourceIn
    }
}

extension EditorModel {
    // MARK: - Multicam document access (NLE-007)

    public var multicamGroups: [MulticamGroupInfo] {
        let assets = document.objects("assets")
        return document.objects("multicams").map { group in
            MulticamGroupInfo(id: group.string("id"), name: group.string("name"),
                angles: group.objects("angles").enumerated().map { index, angle in
                    let assetID = angle.string("asset")
                    let locator = assets.first { $0.string("id") == assetID }?.object("locator") ?? [:]
                    let file = URL(fileURLWithPath: locator.string("relative").isEmpty ? locator.string("absolute") : locator.string("relative")).lastPathComponent
                    return MulticamAngleInfo(id: angle.string("id"), name: angle.string("name"),
                        fallbackName: file.isEmpty ? "アングル \(index + 1)" : file,
                        asset: assetID, streamIndex: Int(angle.number("stream_index")),
                        syncOffset: .wire(angle.object("sync_offset")))
                })
        }
    }
    public func multicamGroup(_ id: String) -> MulticamGroupInfo? { multicamGroups.first { $0.id == id } }

    // MARK: - Source monitor state (GUI-011)

    /// Mirror of `lower_source`'s active window in source-local monitor time:
    /// `[max(0, stream.start - offset), stream.start + stream.duration - offset)`
    /// for media (offset is the multicam angle sync offset, zero for assets)
    /// and `[0, composition.duration)` for compositions. `end` is nil when the
    /// stream declares no duration (unbounded preview window).
    public func sourceWindow(_ source: SourcePreview) -> (start: RationalTime, end: RationalTime?)? {
        switch source {
        case .composition(let id):
            guard let comp = document.objects("compositions").first(where: { $0.string("id") == id }) else { return nil }
            return (.init(num: 0, den: 1), .wire(comp.object("duration")))
        case .asset(let id, let index):
            guard let stream = assetStream(asset: id, index: index) else { return nil }
            return streamWindow(stream, offset: .init(num: 0, den: 1))
        case .multicam(let group, let angle):
            guard let angleInfo = multicamGroup(group)?.angles.first(where: { $0.id == angle }),
                  let stream = assetStream(asset: angleInfo.asset, index: angleInfo.streamIndex) else { return nil }
            return streamWindow(stream, offset: angleInfo.syncOffset)
        }
    }
    private func assetStream(asset: String, index: Int) -> [String: Any]? {
        document.objects("assets").first { $0.string("id") == asset }?
            .objects("streams").first { Int($0.string("index")) == index }
    }
    /// Design-space extent of a source for the preview `region.extent`,
    /// matching `lower_source` — stream pixels for media, design_extent for
    /// compositions, 1920×1080 when unknown (an unresolvable source still
    /// reaches the service so its typed error surfaces in the monitor).
    public func sourceExtent(for source: SourcePreview) -> CGSize {
        var width = 0.0, height = 0.0
        switch source {
        case .composition(let id):
            let dims = document.objects("compositions").first { $0.string("id") == id }?.object("design_extent") ?? [:]
            width = dims.number("width"); height = dims.number("height")
        case .asset(let id, let index):
            let stream = assetStream(asset: id, index: index) ?? [:]
            width = stream.number("width"); height = stream.number("height")
        case .multicam(let group, let angle):
            if let info = multicamGroup(group)?.angles.first(where: { $0.id == angle }),
               let stream = assetStream(asset: info.asset, index: info.streamIndex) {
                width = stream.number("width"); height = stream.number("height")
            }
        }
        return CGSize(width: width > 0 ? width : 1920, height: height > 0 ? height : 1080)
    }
    private func streamWindow(_ stream: [String: Any], offset: RationalTime) -> (RationalTime, RationalTime?) {
        let start = RationalTime.wire(stream.object("start_time"))
        let shiftedStart = subtractTime(start, offset).map { maxTime($0, .init(num: 0, den: 1)) } ?? .init(num: 0, den: 1)
        guard let durationValue = stream["duration"], !(durationValue is NSNull) else { return (shiftedStart, nil) }
        let duration = RationalTime.wire(stream.object("duration"))
        guard duration.doubleSeconds > 0 else { return (shiftedStart, nil) }
        let end = subtractTime(addTime(start, duration) ?? duration, offset)
        return (shiftedStart, end.map { maxTime($0, shiftedStart) })
    }
    private static func seconds(_ t: RationalTime) -> Double {
        (Double(t.num) ?? 0) / max(1e-9, Double(t.den) ?? 1)
    }
    private func maxTime(_ a: RationalTime, _ b: RationalTime) -> RationalTime {
        Self.seconds(a) >= Self.seconds(b) ? a : b
    }
    private func subtractTime(_ a: RationalTime, _ b: RationalTime) -> RationalTime? { try? a.checkedSubtracting(b) }
    private func addTime(_ a: RationalTime, _ b: RationalTime) -> RationalTime? { try? a.checkedAdding(b) }

    /// Source picker entries: every previewable stream, multicam group and
    /// composition in the loaded document, in document order.
    public func sourceChoices() -> [(preview: SourcePreview, name: String, kind: KRMediaKind)] {
        var choices: [(SourcePreview, String, KRMediaKind)] = []
        for asset in editAssets where asset.kind != .multicam {
            if let preview = SourcePreview.fromSourceRef(asset.source) { choices.append((preview, asset.name, asset.kind)) }
        }
        for group in multicamGroups {
            for angle in group.angles {
                choices.append((.multicam(group.id, angle.id), group.name + " · " + angle.displayName, .multicam))
            }
        }
        return choices
    }

    /// Load a source into the monitor. Monitor time starts at the source
    /// window's first sample; In/Out default to the whole window so a plain
    /// Insert drops the entire clip.
    public func openSource(_ source: SourcePreview, name: String) {
        guard let window = sourceWindow(source) else {
            mapFailure(ServiceFailure(code: "SOURCE_MISSING", message: "ソースを解決できません"))
            return
        }
        sourceMonitor = SourceMonitor(source: source, name: name, time: window.start,
                                      inPoint: window.start, outPoint: window.end, track: nil)
        sourcePreviewFailure = nil
    }
    public func openAssetInSource(_ asset: EditAsset) {
        guard let preview = SourcePreview.fromSourceRef(asset.source) else {
            mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "このソースはプレビューできません"))
            return
        }
        openSource(preview, name: asset.name)
    }
    /// Timeline clips open their *source* at the position under the playhead
    /// when the playhead is inside the clip (match-frame behavior).
    public func openClipInSource(_ clip: EditClip) {
        guard let preview = clip.sourcePreview else {
            mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "このクリップのソースはプレビューできません"))
            return
        }
        openSource(preview, name: clipName(clip))
        guard var monitor = sourceMonitor, let window = sourceWindow(preview) else { return }
        let inside = ui.time.doubleSeconds >= clip.start.doubleSeconds && ui.time.doubleSeconds < clip.end.doubleSeconds
        let time = inside ? clip.sourceTime(at: ui.time) : window.start
        monitor.time = clampSourceTime(time, window: window)
        sourceMonitor = monitor
    }
    public func closeSourceMonitor() { sourceMonitor = nil; sourcePreviewFailure = nil; sourcePreviewRendering = false }

    // MARK: - Source transport (scrub only; media PTS mapping is service-side)

    /// Sequence rate drives the monitor's frame stepping so In/Out land on
    /// the same grid as the destination timeline.
    public var sourceFrame: Int64 { (sourceMonitor?.time ?? .init(num: 0, den: 1)).frames(rateNum: rateNum, rateDen: rateDen) }
    public var sourceTimecode: String { KRTimecode.format(frames: max(0, sourceFrame), fps: nominalFPS) }
    public var sourceDurationFrames: Int64? {
        guard let monitor = sourceMonitor, let window = sourceWindow(monitor.source), let end = window.end else { return nil }
        return end.frames(rateNum: rateNum, rateDen: rateDen)
    }
    private func clampSourceTime(_ time: RationalTime, window: (start: RationalTime, end: RationalTime?)) -> RationalTime {
        var result = maxTime(time, window.start)
        if let end = window.end, Self.seconds(result) > Self.seconds(end) { result = end }
        return result
    }
    public func seekSourceFrame(_ frame: Int64) {
        guard var monitor = sourceMonitor, let window = sourceWindow(monitor.source) else { return }
        monitor.time = clampSourceTime(RationalTime(num: frame * rateDen, den: rateNum), window: window)
        sourceMonitor = monitor
    }
    /// GUI-011: source In/Out controls mark the three-point source range.
    public func setSourceInPoint() {
        guard var monitor = sourceMonitor else { return }
        monitor.inPoint = monitor.time
        if let out = monitor.outPoint, Self.seconds(out) <= Self.seconds(monitor.time) { monitor.outPoint = nil }
        sourceMonitor = monitor
    }
    public func setSourceOutPoint() {
        guard var monitor = sourceMonitor, let window = sourceWindow(monitor.source) else { return }
        monitor.outPoint = monitor.time
        if let existing = monitor.inPoint, Self.seconds(existing) >= Self.seconds(monitor.time) {
            monitor.inPoint = window.start
        }
        sourceMonitor = monitor
    }
    public func clearSourcePoints() {
        guard var monitor = sourceMonitor, let window = sourceWindow(monitor.source) else { return }
        monitor.inPoint = window.start
        monitor.outPoint = window.end
        sourceMonitor = monitor
    }
    /// Switch the angle the source monitor previews for a multicam source.
    /// Monitor-local preview switch — no project mutation.
    public func previewSourceAngle(_ angle: String) {
        guard var monitor = sourceMonitor, case .multicam(let group, _) = monitor.source,
              let window = sourceWindow(.multicam(group, angle)) else { return }
        monitor.source = .multicam(group, angle)
        monitor.time = clampSourceTime(monitor.time, window: window)
        sourceMonitor = monitor
    }

    // MARK: - Destination track (GUI-011)

    /// Destination kind implied by the loaded source. Mirrors the service's
    /// `source_track_kind`: `AssetKind::Audio` goes to audio tracks,
    /// everything else (video, image, multicam, composition) targets video.
    public var sourceIsAudio: Bool {
        guard let monitor = sourceMonitor, case .asset(let id, _) = monitor.source,
              let asset = document.objects("assets").first(where: { $0.string("id") == id }) else { return false }
        return asset.string("kind") == "audio"
    }
    /// Tracks eligible as the destination: same kind, unlocked first
    /// (a locked target surfaces the typed service error on submit instead).
    public func sourceDestinationTracks() -> [[String: Any]] {
        sequence.objects("tracks").filter { $0.string("kind") == (sourceIsAudio ? "audio" : "video") }
    }
    /// The effective destination: explicit override or the sequence target
    /// for the source kind. Mirrors `destination_track` in the service.
    public var sourceDestinationTrack: String? {
        if let override = sourceMonitor?.track { return override }
        return sequence.object("targets").string(sourceIsAudio ? "audio" : "video").nilIfEmpty
    }
    public var sourceDestinationLabel: String {
        sourceDestinationTrack.map { trackNumber($0) } ?? "ターゲットなし"
    }
    public func setSourceDestination(_ track: String?) {
        sourceMonitor?.track = track
    }

    // MARK: - Three-point editing (GUI-011)

    /// Effective `[in, out)` source range; nil until both edges resolve to a
    /// nonempty window (unbounded sources need an explicit Out).
    public var sourceEditRange: (start: RationalTime, end: RationalTime)? {
        guard let monitor = sourceMonitor, let window = sourceWindow(monitor.source) else { return nil }
        let start = monitor.inPoint ?? window.start
        guard let end = monitor.outPoint, Self.seconds(end) > Self.seconds(start) else { return nil }
        return (start, end)
    }
    /// Destination point: work-area start when set, else the playhead —
    /// the third point of the three-point edit (ADR-0128).
    public var sourceDestinationTime: RationalTime { workArea?.start ?? ui.time }
    private func sourceEditFields() -> [String: Any]? {
        guard let monitor = sourceMonitor, let sequenceID = ui.sequence, !sequenceID.isEmpty,
              let range = sourceEditRange else { return nil }
        var fields: [String: Any] = [
            "sequence": sequenceID,
            "clip": UUID().uuidString.lowercased(),
            "source": monitor.source.wire,
            "source_range": ["start": range.start.wire, "end": range.end.wire],
            "at": sourceDestinationTime.wire,
        ]
        if let track = monitor.track { fields["track"] = track }
        return fields
    }
    public func insertSource() {
        guard !busy, pendingCandidate == nil else { return }
        guard var fields = sourceEditFields() else {
            mapFailure(ServiceFailure(code: "INVALID_CLIP", message: "インサートにはソースの In/Out 範囲が必要です"))
            return
        }
        // `linked` is insert-only (ripple expands across reciprocal links);
        // EditOverwriteRequest rejects it via additionalProperties.
        fields["linked"] = true
        submitDirect("edit.insert", fields, label: "インサート編集")
    }
    public func overwriteSource() {
        guard !busy, pendingCandidate == nil else { return }
        guard var fields = sourceEditFields() else {
            mapFailure(ServiceFailure(code: "INVALID_CLIP", message: "オーバーライトにはソースの In/Out 範囲が必要です"))
            return
        }
        // Stable tail id chosen up front; the service uses it only when the
        // overwrite splits a covered clip (EditOverwriteRequest.split_tail).
        fields["split_tail"] = UUID().uuidString.lowercased()
        submitDirect("edit.overwrite", fields, label: "オーバーライト編集")
    }

    // MARK: - Multicam editing (NLE-007)

    /// Switch the active angle of one timeline clip. Only the addressed
    /// clip's `source_ref.angle` changes (service-validated).
    public func switchClipAngle(_ clip: EditClip, to angle: String) {
        guard clip.multicam != nil, !trackLocked(clip.track), !busy, pendingCandidate == nil,
              clip.multicam?.angle != angle else { return }
        submitDirect("clip.angle_switch", ["sequence": sequence.string("id"), "clip": clip.id, "angle": angle],
                     label: "アングルの切替")
    }
    /// `multicam.create` (ADR-0127): caller-chosen stable ids; sync is
    /// "timecode" | "audio" | "manual"; `offsets` supplies per-angle sync
    /// offsets for manual mode. `MULTICAM_SYNC_FAILED` arrives typed.
    public func createMulticam(name: String, sync: String, angles: [[String: Any]], reference: String?, offsets: [String: RationalTime]) {
        guard !busy, pendingCandidate == nil, angles.count >= 2 else { return }
        var fields: [String: Any] = [
            "multicam": UUID().uuidString.lowercased(),
            "name": name,
            "sync": sync,
            "angles": angles,
        ]
        if let reference { fields["reference"] = reference }
        if !offsets.isEmpty { fields["offsets"] = offsets.mapValues { $0.wire } }
        submitDirect("multicam.create", fields, label: "マルチカムの作成")
    }
}

private extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}

private extension RationalTime {
    /// Display/clamp comparison only; persisted time stays rational.
    var doubleSeconds: Double { (Double(num) ?? 0) / max(1e-9, Double(den) ?? 1) }
}
