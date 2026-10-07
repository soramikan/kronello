import Foundation
import KronelloDesign

/// A resolved sequence or clip marker from `sequence.query` presentation data.
/// Marker times are always sequence times (ADR-0110); `clip` scopes clip markers.
public struct EditMarker: Identifiable, Equatable {
    public let id: String
    public let clip: String?
    public let track: String?
    public let time: RationalTime
    public let color: String
    public let comment: String
    public static let colors = ["red", "green", "blue", "yellow", "purple", "cyan", "orange", "white"]
}

extension EditorModel {
    public var sequenceMarkers: [EditMarker] {
        sequence.objects("markers").map { Self.marker($0, clip: nil, track: nil) }
    }
    public var clipMarkers: [EditMarker] {
        editClips.flatMap { clip in clip.authored.objects("markers").map { Self.marker($0, clip: clip.id, track: clip.track) } }
    }
    public var allMarkers: [EditMarker] { sequenceMarkers + clipMarkers }
    public var selectedMarker: EditMarker? { allMarkers.first { $0.id == markerSelection } }
    nonisolated static func marker(_ raw: [String: Any], clip: String?, track: String?) -> EditMarker {
        EditMarker(id: raw.string("id"), clip: clip, track: track, time: .wire(raw.object("time")),
                   color: raw.string("color"), comment: raw.string("comment"))
    }
    /// Marker selection is independent of clip selection; picking one clears the other.
    public func selectMarker(_ marker: EditMarker?) {
        markerSelection = marker?.id
        if marker != nil { ui.clipSelection = nil; setDeletedSelection(nil) }
        if marker == nil { markerDrag = nil }
    }
    /// The inclusive content extent end in frames; sequence markers may sit on it.
    public var contentEndFrame: Int64 { sequenceDurationFrames }
    /// Add a sequence marker at the playhead (default) or an exact frame,
    /// clamped into the content extent the shared model validates.
    public func addSequenceMarker(at frame: Int64? = nil, color: String = "red") {
        guard !sequence.string("id").isEmpty else { return }
        let target = min(max(0, frame ?? self.frame), contentEndFrame)
        let marker: [String: Any] = ["id": UUID().uuidString.lowercased(), "time": frameTime(target).wire, "color": color]
        submit([timelineCommand("marker_set", ["sequence": sequence.string("id"), "marker": marker])], label: "マーカーの追加")
    }
    /// Clip markers stay inside the placement's timeline range; the frame is clamped into it.
    public func addClipMarker(_ clip: EditClip, at frame: Int64? = nil, color: String = "blue") {
        guard !ui.locked.contains(clip.track) else { return }
        let start = clip.start.frames(rateNum: rateNum, rateDen: rateDen)
        let end = clip.end.frames(rateNum: rateNum, rateDen: rateDen)
        guard end > start else { return }
        let target = min(max(frame ?? self.frame, start), end - 1)
        let marker: [String: Any] = ["id": UUID().uuidString.lowercased(), "time": frameTime(target).wire, "color": color]
        submit([timelineCommand("marker_set", ["sequence": sequence.string("id"), "clip": clip.id, "marker": marker])], label: "クリップマーカーの追加")
    }
    /// `marker_set` upserts by id, so recoloring reuses it with the same marker.
    public func recolorMarker(_ marker: EditMarker, color: String) {
        guard EditMarker.colors.contains(color) else { return }
        var fields: [String: Any] = ["sequence": sequence.string("id"),
            "marker": ["id": marker.id, "time": marker.time.wire, "color": color,
                       "comment": marker.comment.isEmpty ? NSNull() : marker.comment as Any]]
        if let clip = marker.clip { fields["clip"] = clip }
        submit([timelineCommand("marker_set", fields)], label: "マーカー色の変更")
    }
    public func removeMarker(_ marker: EditMarker) {
        var fields: [String: Any] = ["sequence": sequence.string("id"), "marker": marker.id]
        if let clip = marker.clip { fields["clip"] = clip }
        if markerSelection == marker.id { markerSelection = nil }
        markerDrag = nil
        submit([timelineCommand("marker_remove", fields)], label: "マーカーの削除")
    }
    public func moveMarker(_ marker: EditMarker, to frame: Int64) {
        guard marker.time.frames(rateNum: rateNum, rateDen: rateDen) != frame else { return }
        var fields: [String: Any] = ["sequence": sequence.string("id"), "marker": marker.id, "time": frameTime(frame).wire]
        if let clip = marker.clip { fields["clip"] = clip }
        submit([timelineCommand("marker_move", fields)], label: "マーカーの移動")
    }
    /// Ruler marker drag: a live frame preview committed once on release.
    public func beginMarkerDrag(_ marker: EditMarker) {
        selectMarker(marker)
        markerDrag = (marker.id, marker.time.frames(rateNum: rateNum, rateDen: rateDen))
    }
    public func updateMarkerDrag(to frame: Int64) {
        guard let drag = markerDrag else { return }
        var excluded: Set<Int64> = []
        var lower: Int64 = 0, upper = contentEndFrame
        if let marker = allMarkers.first(where: { $0.id == drag.id }) {
            excluded.insert(marker.time.frames(rateNum: rateNum, rateDen: rateDen))
            if let clipID = marker.clip, let clip = editClips.first(where: { $0.id == clipID }) {
                lower = clip.start.frames(rateNum: rateNum, rateDen: rateDen)
                upper = clip.end.frames(rateNum: rateNum, rateDen: rateDen) - 1
            }
        }
        markerDrag = (drag.id, min(max(lower, snappedFrame(frame, excludingFrames: excluded)), upper))
    }
    public func commitMarkerDrag() {
        guard let drag = markerDrag else { return }
        markerDrag = nil
        guard let marker = allMarkers.first(where: { $0.id == drag.id }),
              drag.frame != marker.time.frames(rateNum: rateNum, rateDen: rateDen) else { return }
        moveMarker(marker, to: drag.frame)
    }
    /// The In/Out work area in sequence time; absent means the whole sequence.
    public var workArea: (start: RationalTime, end: RationalTime)? {
        let area = sequence.object("work_area")
        guard !area.isEmpty else { return nil }
        return (.wire(area.object("start")), .wire(area.object("end")))
    }
    public func setInPoint() {
        let end = min(workArea?.end.frames(rateNum: rateNum, rateDen: rateDen) ?? contentEndFrame, contentEndFrame)
        guard frame < end else {
            mapFailure(ServiceFailure(code: "INVALID_CLIP", message: "In は Out より前に設定してください")); return
        }
        submit([timelineCommand("work_area_set", ["sequence": sequence.string("id"),
            "work_area": ["start": frameTime(frame).wire, "end": frameTime(end).wire]])], label: "In 点の設定")
    }
    /// The exclusive Out bound: the frame under the playhead stays inside the range.
    public func setOutPoint() {
        let start = workArea?.start.frames(rateNum: rateNum, rateDen: rateDen) ?? 0
        let out = min(frame + 1, contentEndFrame)
        guard out > start else {
            mapFailure(ServiceFailure(code: "INVALID_CLIP", message: "Out は In より後に設定してください")); return
        }
        submit([timelineCommand("work_area_set", ["sequence": sequence.string("id"),
            "work_area": ["start": frameTime(start).wire, "end": frameTime(out).wire]])], label: "Out 点の設定")
    }
    public func clearWorkArea() {
        guard workArea != nil else { return }
        submit([timelineCommand("work_area_set", ["sequence": sequence.string("id"), "work_area": NSNull()])], label: "In/Out の解除")
    }
}
