import Foundation
import CoreGraphics

/// FX-004 (ADR-0114) clip mask authoring. Every edit is one `clip_masks_set`
/// timeline command: the mask stack and its backing properties replace
/// atomically, sharing plan/apply/undo with `clip_set_effects`. Mask paths are
/// clip-local design_px Beziers; the GUI edits anchor/control handles of the
/// constant `kronello.mask.path` property and keeps animated sources read-only.
extension EditorModel {
    /// Mask fields and their backing descriptor keys, in wire order.
    public static let maskFields: [(field: String, key: String)] = [
        ("path", "kronello.mask.path"),
        ("feather", "kronello.mask.feather"),
        ("expansion", "kronello.mask.expansion"),
        ("opacity", "kronello.mask.opacity"),
    ]
    /// Mask rows authored on the clip, in authored order.
    public static func clipMasks(_ clip: EditClip) -> [[String: Any]] {
        clip.authored.objects("masks")
    }
    /// The backing property behind one mask field reference, if present.
    public static func maskProperty(_ clip: EditClip, mask: [String: Any], field: String) -> [String: Any]? {
        guard let id = mask[field] as? String else { return nil }
        return clip.authored.objects("properties").first { $0.string("id") == id }
    }
    /// Constant scalar behind a mask parameter; nil for animated sources.
    public static func maskScalar(_ clip: EditClip, mask: [String: Any], field: String) -> Double? {
        guard let property = maskProperty(clip, mask: mask, field: field),
              property.object("source").string("kind") == "constant" else { return nil }
        return (property.object("source").object("value")["value"] as? NSNumber)?.doubleValue
    }
    /// Whether the mask parameter is a constant scalar the GUI may rewrite.
    public static func maskParameterIsConstant(_ clip: EditClip, mask: [String: Any], field: String) -> Bool {
        maskScalar(clip, mask: mask, field: field) != nil
    }

    /// Editable path state: anchor points plus per-vertex incoming/outgoing
    /// Bezier handles. A quadratic control lands on both adjacent vertices so
    /// untouched quads serialize back as `quad_to`; asymmetric drags upgrade
    /// the segment to `cubic_to` (identical math, exact shape).
    public struct MaskVertex: Equatable {
        public var anchor: CGPoint
        public var controlIn: CGPoint?
        public var controlOut: CGPoint?
        public init(anchor: CGPoint, controlIn: CGPoint? = nil, controlOut: CGPoint? = nil) {
            self.anchor = anchor
            self.controlIn = controlIn
            self.controlOut = controlOut
        }
    }
    static func point(_ raw: Any?) -> CGPoint? {
        guard let pair = raw as? [Any], pair.count == 2,
              let x = (pair[0] as? NSNumber)?.doubleValue, let y = (pair[1] as? NSNumber)?.doubleValue,
              x.isFinite, y.isFinite else { return nil }
        return CGPoint(x: x, y: y)
    }
    /// Anchor/handle decomposition of the constant mask path; nil when the
    /// path is animated, missing, or malformed.
    public static func maskVertices(_ clip: EditClip, mask: [String: Any]) -> [MaskVertex]? {
        guard let property = maskProperty(clip, mask: mask, field: "path"),
              property.object("source").string("kind") == "constant",
              property.object("source").object("value").string("kind") == "path" else { return nil }
        let segments = property.object("source").object("value").object("value").objects("segments")
        var vertices: [MaskVertex] = []
        for segment in segments {
            switch segment.string("kind") {
            case "move_to", "line_to":
                guard let anchor = point(segment["value"]) else { return nil }
                vertices.append(MaskVertex(anchor: anchor))
            case "quad_to":
                let value = segment.object("value")
                guard let control = point(value["control"]), let end = point(value["end"]) else { return nil }
                if !vertices.isEmpty { vertices[vertices.count - 1].controlOut = control }
                vertices.append(MaskVertex(anchor: end, controlIn: control))
            case "cubic_to":
                let value = segment.object("value")
                guard let c1 = point(value["control1"]), let c2 = point(value["control2"]),
                      let end = point(value["end"]) else { return nil }
                if !vertices.isEmpty { vertices[vertices.count - 1].controlOut = c1 }
                vertices.append(MaskVertex(anchor: end, controlIn: c2))
            case "close": break
            default: return nil
            }
        }
        guard !vertices.isEmpty else { return nil }
        return vertices
    }
    static func segmentWire(_ kind: String, _ value: Any) -> [String: Any] {
        ["kind": kind, "value": value]
    }
    /// Rebuild wire segments from vertices plus authored closure.
    public static func maskSegments(_ vertices: [MaskVertex], closed: Bool) -> [[String: Any]] {
        func pair(_ p: CGPoint) -> [Double] { [p.x, p.y] }
        var segments: [[String: Any]] = [segmentWire("move_to", pair(vertices[0].anchor))]
        for index in 1..<vertices.count {
            let out = vertices[index - 1].controlOut, into = vertices[index].controlIn
            let end = vertices[index].anchor
            switch (out, into) {
            case (nil, nil):
                segments.append(segmentWire("line_to", pair(end)))
            case let (a?, nil):
                segments.append(segmentWire("quad_to", ["control": pair(a), "end": pair(end)]))
            case let (nil, b?):
                segments.append(segmentWire("quad_to", ["control": pair(b), "end": pair(end)]))
            case let (a?, b?) where a == b:
                segments.append(segmentWire("quad_to", ["control": pair(a), "end": pair(end)]))
            case let (a, b):
                segments.append(segmentWire("cubic_to", [
                    "control1": pair(a ?? vertices[index - 1].anchor),
                    "control2": pair(b ?? end),
                    "end": pair(end)]))
            }
        }
        if closed { segments.append(["kind": "close"]) }
        return segments
    }

    // MARK: - command construction (pure, testable without a transport)

    /// A fresh full-frame Add mask plus its four backing properties.
    public static func newMaskParts(extent: CGSize) -> (mask: [String: Any], properties: [[String: Any]]) {
        var properties: [[String: Any]] = []
        func authored(_ key: String, _ type: String, _ value: Any) -> String {
            let id = UUID().uuidString
            properties.append(["id": id, "descriptor": ["key": key, "version": 1],
                "source": ["kind": "constant", "value": ["kind": type, "value": value]], "modifiers": []])
            return id
        }
        let vertices = [
            MaskVertex(anchor: .init(x: 0, y: 0)),
            MaskVertex(anchor: .init(x: extent.width, y: 0)),
            MaskVertex(anchor: .init(x: extent.width, y: extent.height)),
            MaskVertex(anchor: .init(x: 0, y: extent.height)),
        ]
        let path = authored("kronello.mask.path", "path",
            ["segments": maskSegments(vertices, closed: true)])
        let mask: [String: Any] = [
            "id": UUID().uuidString,
            "path": path,
            "mode": "add",
            "feather": authored("kronello.mask.feather", "scalar", 0.0),
            "expansion": authored("kronello.mask.expansion", "scalar", 0.0),
            "opacity": authored("kronello.mask.opacity", "scalar", 1.0),
            "invert": false,
            "closed": true,
        ]
        return (mask, properties)
    }

    // MARK: - mutations

    /// Atomic stack replacement; the shared edit transaction validates the
    /// result (`MASK_*` codes surface to the user as typed failures).
    public func replaceClipMasks(_ clip: EditClip, masks: [[String: Any]], properties: [[String: Any]], label: String, base: String? = nil) {
        guard !ui.locked.contains(clip.track) else { return }
        submit([timelineCommand("clip_masks_set", ["sequence": sequence.string("id"), "clip": clip.id,
            "masks": masks, "properties": properties])], label: label, base: base)
    }
    public func addClipMask(_ clip: EditClip) {
        guard clip.kind != .audio, clip.kind != .subtitle else { return }
        let (mask, extra) = Self.newMaskParts(extent: extent)
        replaceClipMasks(clip,
            masks: Self.clipMasks(clip) + [mask],
            properties: clip.authored.objects("properties") + extra,
            label: "マスクの追加")
    }
    /// Removes the mask and any properties no other mask/effect references.
    public func removeClipMask(_ clip: EditClip, index: Int) {
        var masks = Self.clipMasks(clip)
        guard masks.indices.contains(index) else { return }
        let removed = masks.remove(at: index)
        let kept = Set(masks.flatMap { m in Self.maskFields.compactMap { m[$0.field] as? String } })
        let owned = Set(Self.maskFields.compactMap { removed[$0.field] as? String }).subtracting(kept)
        let properties = clip.authored.objects("properties").filter { !owned.contains($0.string("id")) }
        replaceClipMasks(clip, masks: masks, properties: properties, label: "マスクの削除")
    }
    /// Mode/invert/closed live on the mask row itself; constants are
    /// translated to the wire names.
    public func setClipMaskField(_ clip: EditClip, index: Int, field: String, value: Any, base: String? = nil) {
        var masks = Self.clipMasks(clip)
        guard masks.indices.contains(index) else { return }
        masks[index][field] = value
        var properties = clip.authored.objects("properties")
        if field == "closed", let closed = value as? Bool,
           let vertices = Self.maskVertices(clip, mask: masks[index]) {
            // Keep the wire path topology in sync with authored closure.
            let segments = Self.maskSegments(vertices, closed: closed)
            if let path = Self.maskProperty(clip, mask: masks[index], field: "path"),
               let p = properties.firstIndex(where: { $0.string("id") == path.string("id") }),
               properties[p].object("source").string("kind") == "constant" {
                properties[p]["source"] = ["kind": "constant", "value": ["kind": "path", "value": ["segments": segments]]]
            }
        }
        let labels = ["mode": "マスクモード", "invert": "マスク反転", "closed": "マスク閉合"]
        replaceClipMasks(clip, masks: masks, properties: properties, label: labels[field] ?? "マスクの変更", base: base)
    }
    /// Constant scalar parameter (feather/expansion/opacity). Animated
    /// sources stay read-only instead of being silently flattened.
    public func setClipMaskScalar(_ clip: EditClip, index: Int, field: String, value: Double, base: String? = nil) {
        let masks = Self.clipMasks(clip)
        guard masks.indices.contains(index),
              let property = Self.maskProperty(clip, mask: masks[index], field: field),
              let p = clip.authored.objects("properties").firstIndex(where: { $0.string("id") == property.string("id") }) else { return }
        var properties = clip.authored.objects("properties")
        guard properties[p].object("source").string("kind") == "constant" else {
            mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "アニメーション付きマスクパラメータは値を固定せず保持します")); return
        }
        properties[p]["source"] = ["kind": "constant", "value": ["kind": "scalar", "value": value]]
        replaceClipMasks(clip, masks: masks, properties: properties, label: "マスクパラメータの変更", base: base)
    }
    /// Moves vertex `vertexIndex` to `point`, dragging its adjacent Bezier
    /// handles along so the curve shape is preserved (FX-004).
    public func setClipMaskAnchor(_ clip: EditClip, index: Int, vertexIndex: Int, point: CGPoint, base: String? = nil) {
        let masks = Self.clipMasks(clip)
        guard masks.indices.contains(index),
              var vertices = Self.maskVertices(clip, mask: masks[index]),
              vertices.indices.contains(vertexIndex) else { return }
        let delta = CGVector(dx: point.x - vertices[vertexIndex].anchor.x,
                             dy: point.y - vertices[vertexIndex].anchor.y)
        vertices[vertexIndex].anchor = point
        vertices[vertexIndex].controlIn = vertices[vertexIndex].controlIn.map {
            CGPoint(x: $0.x + delta.dx, y: $0.y + delta.dy) }
        vertices[vertexIndex].controlOut = vertices[vertexIndex].controlOut.map {
            CGPoint(x: $0.x + delta.dx, y: $0.y + delta.dy) }
        commitMaskPath(clip, index: index, vertices: vertices, label: "マスク頂点の移動", base: base)
    }
    /// Moves one Bezier handle of vertex `vertexIndex` (`in`coming or
    /// `out`going) without touching the anchor.
    public func setClipMaskHandle(_ clip: EditClip, index: Int, vertexIndex: Int, incoming: Bool, point: CGPoint, base: String? = nil) {
        let masks = Self.clipMasks(clip)
        guard masks.indices.contains(index),
              var vertices = Self.maskVertices(clip, mask: masks[index]),
              vertices.indices.contains(vertexIndex) else { return }
        if incoming { vertices[vertexIndex].controlIn = point } else { vertices[vertexIndex].controlOut = point }
        commitMaskPath(clip, index: index, vertices: vertices, label: "マスクハンドルの移動", base: base)
    }
    /// Splits the edge after vertex `afterVertex` at its midpoint; line edges
    /// insert a straight vertex, quad/cubic edges split by de Casteljau.
    public func insertClipMaskVertex(_ clip: EditClip, index: Int, afterVertex: Int) {
        let masks = Self.clipMasks(clip)
        guard masks.indices.contains(index),
              var vertices = Self.maskVertices(clip, mask: masks[index]),
              vertices.indices.contains(afterVertex) else { return }
        let closed = masks[index]["closed"] as? Bool ?? true
        let next = (afterVertex + 1) % vertices.count
        guard next != afterVertex, closed || afterVertex + 1 < vertices.count else { return }
        let a = vertices[afterVertex], b = vertices[next]
        func mid(_ p: CGPoint, _ q: CGPoint) -> CGPoint { CGPoint(x: (p.x + q.x) / 2, y: (p.y + q.y) / 2) }
        let inserted: MaskVertex
        switch (a.controlOut, b.controlIn) {
        case (nil, nil):
            inserted = MaskVertex(anchor: mid(a.anchor, b.anchor))
        case let (c?, nil): // quad split
            let q0 = mid(a.anchor, c), q1 = mid(c, b.anchor), m = mid(q0, q1)
            inserted = MaskVertex(anchor: m, controlIn: q0, controlOut: q1)
            vertices[afterVertex].controlOut = q0
            vertices[next].controlIn = q1
        case let (nil, c?): // quad split
            let q0 = mid(a.anchor, c), q1 = mid(c, b.anchor), m = mid(q0, q1)
            inserted = MaskVertex(anchor: m, controlIn: q0, controlOut: q1)
            vertices[afterVertex].controlOut = q0
            vertices[next].controlIn = q1
        case let (c1?, c2?): // cubic split at t = 1/2
            let m01 = mid(a.anchor, c1), m12 = mid(c1, c2), m23 = mid(c2, b.anchor)
            let m012 = mid(m01, m12), m123 = mid(m12, m23), m = mid(m012, m123)
            inserted = MaskVertex(anchor: m, controlIn: m012, controlOut: m123)
            vertices[afterVertex].controlOut = m01
            vertices[next].controlIn = m23
        }
        vertices.insert(inserted, at: next == 0 ? vertices.count : next)
        commitMaskPath(clip, index: index, vertices: vertices, label: "マスク頂点の追加")
    }
    /// Drops vertex `vertexIndex`; the surrounding segment endpoints merge
    /// while their Bezier handles are kept (FX-004 point editing).
    public func removeClipMaskVertex(_ clip: EditClip, index: Int, vertexIndex: Int) {
        let masks = Self.clipMasks(clip)
        guard masks.indices.contains(index),
              var vertices = Self.maskVertices(clip, mask: masks[index]),
              vertices.indices.contains(vertexIndex), vertices.count > 2 else { return }
        // The surviving edge from the previous vertex keeps its own handles.
        let next = vertices[(vertexIndex + 1) % vertices.count]
        let previous = vertices[(vertexIndex + vertices.count - 1) % vertices.count]
        vertices.remove(at: vertexIndex)
        if let i = vertices.firstIndex(where: { $0.anchor == next.anchor }) {
            vertices[i].controlIn = next.controlIn ?? vertices[i].controlIn
        }
        if let i = vertices.firstIndex(where: { $0.anchor == previous.anchor }) {
            vertices[i].controlOut = previous.controlOut ?? vertices[i].controlOut
        }
        commitMaskPath(clip, index: index, vertices: vertices, label: "マスク頂点の削除")
    }
    /// Rewrites the constant `kronello.mask.path` property behind mask `index`.
    private func commitMaskPath(_ clip: EditClip, index: Int, vertices: [MaskVertex], label: String, base: String? = nil) {
        let masks = Self.clipMasks(clip)
        guard masks.indices.contains(index),
              let path = Self.maskProperty(clip, mask: masks[index], field: "path") else { return }
        var properties = clip.authored.objects("properties")
        guard let p = properties.firstIndex(where: { $0.string("id") == path.string("id") }) else { return }
        guard properties[p].object("source").string("kind") == "constant" else {
            mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "アニメーション付きマスクパスは値を固定せず保持します")); return
        }
        let closed = masks[index]["closed"] as? Bool ?? true
        properties[p]["source"] = ["kind": "constant", "value": [
            "kind": "path", "value": ["segments": Self.maskSegments(vertices, closed: closed)]]]
        replaceClipMasks(clip, masks: masks, properties: properties, label: label, base: base)
    }

    // MARK: - FX-007 adjustment clips (ADR-0116)

    /// Command list that appends a fresh video track above every existing
    /// track and places a two-second adjustment clip at the playhead. Effects
    /// are added afterwards through the normal clip inspector.
    public func adjustmentAddCommands(at frame: Int64, durationFrames: Int64) -> (track: String, clip: String, commands: [[String: Any]]) {
        let track = UUID().uuidString.lowercased(), clip = UUID().uuidString.lowercased()
        let commands: [[String: Any]] = [
            timelineCommand("track_append", ["sequence": sequence.string("id"),
                "track": ["id": track, "kind": "video", "clips": [[String: Any]](),
                          "state": ["visible": true, "muted": false]]]),
            timelineCommand("clip_place", ["sequence": sequence.string("id"), "track": track,
                "clip": ["id": clip, "source_ref": ["kind": "adjustment"],
                         "timeline_range": ["start": frameTime(max(0, frame)).wire,
                                            "end": frameTime(max(0, frame) + durationFrames).wire],
                         "source_in": ["num": "0", "den": "1"], "audio_retime": "reject",
                         "time_map": ["kind": "linear", "offset": ["num": "0", "den": "1"], "speed": ["num": "1", "den": "1"]],
                         "links": [[String: Any]](), "effects": [[String: Any]](),
                         "properties": [[String: Any]](), "masks": [[String: Any]](),
                         "markers": [[String: Any]]()]]),
        ]
        return (track, clip, commands)
    }
    /// Adds an adjustment clip at the playhead and selects it after the Event
    /// lands (a rejected placement never leaves a dangling selection).
    public func addAdjustmentClip() {
        guard !busy, pendingCandidate == nil, !sequence.isEmpty else { return }
        let result = adjustmentAddCommands(at: frame, durationFrames: Int64(nominalFPS) * 2)
        Task {
            guard await apply(.init(base: revision, commands: result.commands, label: "アジャストメントクリップの追加")) != nil else { return }
            selectClip(result.clip)
        }
    }
}
