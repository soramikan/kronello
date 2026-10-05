import Foundation
import KronelloDesign

public struct PreviewIdentity: Equatable {
    public let target: String
    public let revision: String
    public let time: RationalTime
    public init(target: String, revision: String, time: RationalTime) { self.target = target; self.revision = revision; self.time = time }
}
struct PreviewIssue { let identity: PreviewIdentity; let failure: ServiceFailure }

public struct EditAsset: Identifiable {
    public let id: String
    public let name: String
    public let kind: KRMediaKind
    public let source: [String: Any]
    public let duration: RationalTime
    public let sourceIn: RationalTime
    public let meta: String
    public var missing: String?
}

public struct EditClip: Identifiable {
    public let query: [String: Any]
    public init(query: [String: Any]) { self.query = query }
    public var authored: [String: Any] { query.object("clip") }
    public var id: String { authored.string("id") }
    public var track: String { query.string("track") }
    public var kind: KRMediaKind { KRMediaKind(rawValue: query.string("kind")) ?? .generator }
    public var start: RationalTime { .wire(authored.object("timeline_range").object("start")) }
    public var end: RationalTime { .wire(authored.object("timeline_range").object("end")) }
    public var composition: String? { authored.object("source_ref")["composition"] as? String }
    public var linearRate: RationalTime? {
        let map = authored.object("time_map")
        guard map.string("kind") == "linear" else { return nil }
        return .wire(map.object("speed"))
    }
    public var speedLabel: String {
        guard let rate = linearRate else { return "非線形" }
        return String(format: "%.1f%%", Double(rate.num)! / Double(rate.den)! * 100)
    }
    public var reversed: Bool { linearRate.map { Int64($0.num)! < 0 } ?? false }
}

public enum EditPresentation {
    public static func rateLabel(num: Int64, den: Int64) -> String {
        num % den == 0 ? "\(num / den) fps" : String(format: "%.3f fps", Double(num) / Double(den))
    }
    public static func rulerLabel(frame: Int64, fps: Int) -> String { "\(frame / Int64(fps))s\(frame % Int64(fps))f" }
}

public struct TimelineCandidate {
    public enum Mode { case place, move, trimStart, trimEnd, blade }
    public let base: String
    public let sequence: String
    public let track: String
    public let clip: [String: Any]
    public let name: String
    public let kind: KRMediaKind
    public let missing: String?
    public let mode: Mode
    public let originalStart: Int64
    public let originalEnd: Int64
    public var start: Int64
    public var end: Int64
    public var cut: Int64
    public let rightID: String
}

extension RationalTime {
    public static func wire(_ value: [String: Any]) -> Self {
        .init(num: Int64(value.string("num")) ?? 0, den: Int64(value.string("den")) ?? 1)
    }
}

extension EditorModel {
    public var sequence: [String: Any] { sequenceResult.object("sequence") }
    public var activeRate: [String: Any] { ui.page == "edit" ? sequence.object("frame_rate") : current.object("edit_rate") }
    public var activeExtent: [String: Any] { ui.page == "edit" ? sequence.object("extent") : current.object("design_extent") }
    public var editClips: [EditClip] { sequenceResult.objects("clips").map { EditClip(query: $0) } }
    public var selectedClip: EditClip? { editClips.first { $0.id == ui.clipSelection } }
    public var sequenceDurationFrames: Int64 { max(1, editClips.map { $0.end.frames(rateNum: rateNum, rateDen: rateDen) }.max() ?? Int64(nominalFPS * 5)) }
    public func frameTime(_ frame: Int64) -> RationalTime { .init(num: frame * rateDen, den: rateNum) }
    public var editAssets: [EditAsset] {
        let media = document.objects("assets").flatMap { asset -> [EditAsset] in
            let kind = KRMediaKind(rawValue: asset.string("kind")) ?? .video
            let locator = asset.object("locator")
            let name = URL(fileURLWithPath: locator.string("relative").isEmpty ? locator.string("absolute") : locator.string("relative")).lastPathComponent
            return asset.objects("streams").map { stream in
                let index = stream.string("index")
                let video = stream.number("width") > 0
                return EditAsset(id: asset.string("id") + ":" + index, name: name + " · stream " + index,
                    kind: video ? (kind == .image ? .image : .video) : .audio,
                    source: ["kind": "asset", "asset": asset.string("id"), "stream_index": Int(index) ?? 0],
                    duration: .wire(stream.object("duration")), sourceIn: video ? .wire(stream.object("start_time")) : .init(num: 0, den: 1),
                    meta: video ? "\(Int(stream.number("width")))×\(Int(stream.number("height")))" : stream.string("codec"),
                    missing: sequenceResult.objects("asset_status").first { $0.string("asset") == asset.string("id") }?.object("error")["code"] as? String)
            }
        }
        return media + compositions.enumerated().map { index, comp in
            EditAsset(id: comp.string("id"), name: "Composition \(index + 1)", kind: .composition,
                source: ["kind": "composition", "composition": comp.string("id")], duration: .wire(comp.object("duration")),
                sourceIn: .init(num: 0, den: 1), meta: "\(comp.objects("nodes").count) 件", missing: nil)
        }
    }
    public func clipName(_ clip: EditClip) -> String {
        let source = clip.authored.object("source_ref")
        return editAssets.first { NSDictionary(dictionary: $0.source) == NSDictionary(dictionary: source) }?.name ?? source.string("generator")
    }
    public func clipMissing(_ clip: EditClip) -> String? {
        let source = clip.authored.object("source_ref")
        return editAssets.first { NSDictionary(dictionary: $0.source) == NSDictionary(dictionary: source) }?.missing
    }
    public func validateClipSelection(actor: String) {
        guard ui.page == "edit", sequenceFailure == nil, let id = ui.clipSelection else { return }
        if !editClips.contains(where: { $0.id == id }) {
            ui.clipSelection = nil
            setDeletedSelection("選択していたクリップは削除されました。\(actor) · rev \(revision)")
        }
    }
    public func selectClip(_ id: String?) { ui.clipSelection = id; setDeletedSelection(nil) }
    public func openClipInMotion(_ clip: EditClip) {
        guard let composition = clip.composition else { return }
        playing = false; timelineCandidate = nil; ui.page = "motion"; ui.time = .init(num: 0, den: 1)
        setComposition(composition)
    }
    public func setSequence(_ id: String) {
        playing = false; timelineCandidate = nil; ui.sequence = id; ui.clipSelection = nil; ui.time = .init(num: 0, den: 1)
        Task { do { try await reload() } catch { mapFailure(error) } }
    }
    public func activatePlayback(for sequence: [String: Any]) {
        guard !sequence.string("id").isEmpty else { return }
        // AUDIO-002 replaces this existing transport activation with configurePlayback(target: .sequence(id), rateNum: rateNum, rateDen: rateDen).
        playing = false
    }
    public func beginClipGesture(_ clip: EditClip, mode: TimelineCandidate.Mode) {
        guard !busy, pendingCandidate == nil, !ui.locked.contains(clip.track), timelineCandidate == nil else { return }
        selectClip(clip.id)
        let start = clip.start.frames(rateNum: rateNum, rateDen: rateDen), end = clip.end.frames(rateNum: rateNum, rateDen: rateDen)
        timelineCandidate = .init(base: revision, sequence: sequence.string("id"), track: clip.track, clip: clip.authored, name: clipName(clip), kind: clip.kind, missing: clipMissing(clip),
            mode: mode, originalStart: start, originalEnd: end, start: start, end: end, cut: start, rightID: UUID().uuidString.lowercased())
    }
    public func beginAssetGesture(_ asset: EditAsset, track: String, at frame: Int64) {
        guard !busy, pendingCandidate == nil, timelineCandidate == nil, !ui.locked.contains(track) else { return }
        let length = asset.duration.frames(rateNum: rateNum, rateDen: rateDen)
        guard length > 0, let target = sequence.objects("tracks").first(where: { $0.string("id") == track }),
              (target.string("kind") == "audio") == (asset.kind == .audio) else { return }
        let start = max(0, frame)
        let clip: [String: Any] = ["id": UUID().uuidString.lowercased(), "source_ref": asset.source,
            "source_in": asset.sourceIn.wire, "timeline_range": ["start": frameTime(start).wire, "end": frameTime(start + length).wire],
            "time_map": ["kind": "linear", "offset": frameTime(0).wire, "speed": ["num": "1", "den": "1"]],
            "links": [], "effects": [], "properties": []]
        timelineCandidate = .init(base: revision, sequence: sequence.string("id"), track: track, clip: clip, name: asset.name, kind: asset.kind, missing: asset.missing, mode: .place,
            originalStart: start, originalEnd: start + length, start: start, end: start + length, cut: start, rightID: UUID().uuidString.lowercased())
    }
    public func updateClipGesture(delta: Int64 = 0, at: Int64? = nil) {
        guard var c = timelineCandidate else { return }
        switch c.mode {
        case .place, .move:
            c.start = max(0, at ?? (c.originalStart + delta)); c.end = c.start + c.originalEnd - c.originalStart
        case .trimStart: c.start = min(c.originalEnd - 1, max(c.originalStart, c.originalStart + delta))
        case .trimEnd: c.end = max(c.originalStart + 1, min(c.originalEnd, c.originalEnd + delta))
        case .blade: c.cut = at ?? c.originalStart + delta
        }
        if editSnap, c.mode == .place || c.mode == .move {
            let targets = [frame] + editClips.filter { $0.id != c.clip.string("id") }.flatMap { [$0.start.frames(rateNum: rateNum, rateDen: rateDen), $0.end.frames(rateNum: rateNum, rateDen: rateDen)] }
            if let snap = targets.min(by: { abs($0 - c.start) < abs($1 - c.start) }), abs(snap - c.start) <= 2 {
                c.end += snap - c.start; c.start = snap
            }
        }
        timelineCandidate = c
    }
    public func cancelClipGesture() { timelineCandidate = nil }
    public func bladeFrame(_ clip: EditClip, fraction: Double) -> Int64 {
        let start = clip.start.frames(rateNum: rateNum, rateDen: rateDen), end = clip.end.frames(rateNum: rateNum, rateDen: rateDen)
        return start + Int64((fraction * Double(end - start)).rounded())
    }
    public func timelineCommand(_ name: String, _ fields: [String: Any]) -> [String: Any] { ["timeline": [name: fields]] }
    @discardableResult public func commitClipGesture() async -> [String: Any]? {
        guard let c = timelineCandidate else { return nil }
        timelineCandidate = nil
        var fields: [String: Any] = ["sequence": c.sequence, "clip": c.clip.string("id")]
        let command: [String: Any]
        switch c.mode {
        case .place:
            var clip = c.clip; clip["timeline_range"] = ["start": frameTime(c.start).wire, "end": frameTime(c.end).wire]
            command = timelineCommand("clip_place", ["sequence": c.sequence, "track": c.track, "clip": clip])
        case .move:
            guard c.start != c.originalStart else { return nil }
            fields["delta"] = frameTime(c.start - c.originalStart).wire; fields["linked"] = true
            command = timelineCommand("clip_move", fields)
        case .trimStart, .trimEnd:
            guard c.start != c.originalStart || c.end != c.originalEnd else { return nil }
            fields["range"] = ["start": frameTime(c.start).wire, "end": frameTime(c.end).wire]
            command = timelineCommand("clip_trim", fields)
        case .blade:
            fields["time"] = frameTime(c.cut).wire; fields["right_clip"] = c.rightID
            command = timelineCommand("clip_split", fields)
        }
        return await apply(.init(base: c.base, commands: [command], label: c.mode == .blade ? "クリップの分割" : c.mode == .place ? "クリップの配置" : "クリップの配置・尺の変更"))
    }
}
