import Foundation
import CoreGraphics
import KronelloDesign

public struct CanvasEdit {
    public let layer: Layer
    public let base: String
    public let time: RationalTime
    public let bounds: CGRect
    public let parent: CGAffineTransform
    public init(layer: Layer, base: String, time: RationalTime, bounds: CGRect, parent: CGAffineTransform) {
        self.layer = layer; self.base = base; self.time = time; self.bounds = bounds; self.parent = parent
    }
}

extension EditorModel {
    func affine(_ layer: Layer) -> CGAffineTransform {
        guard let values = layer.evaluated["world_transform"] as? [[Double]], values.count == 2,
              values.allSatisfy({ $0.count == 3 }) else { return .identity }
        return .init(a: values[0][0], b: values[1][0], c: values[0][1], d: values[1][1], tx: values[0][2], ty: values[1][2])
    }
    func parentTransform(_ layer: Layer) -> CGAffineTransform {
        layer.parent.flatMap { id in layers.first { $0.id == id } }.map(affine) ?? .identity
    }
    func worldOrigin(_ layer: Layer) -> CGPoint {
        let anchor = layer.property("kronello.transform.anchor").map { propertyNumbers(layer, $0) } ?? [0, 0]
        return CGPoint(x: anchor.count == 2 ? anchor[0] : 0, y: anchor.count == 2 ? anchor[1] : 0).applying(affine(layer))
    }
    func scaledBounds(_ bounds: CGRect, layer: Layer, x: Double, y: Double) -> CGRect {
        let world = affine(layer), anchor = worldOrigin(layer)
        let linear = CGAffineTransform(a: world.a, b: world.b, c: world.c, d: world.d, tx: 0, ty: 0)
        return bounds.applying(CGAffineTransform(translationX: -anchor.x, y: -anchor.y)
            .concatenating(linear.inverted()).concatenating(CGAffineTransform(scaleX: x, y: y))
            .concatenating(linear).concatenating(CGAffineTransform(translationX: anchor.x, y: anchor.y)))
    }
    func rotatedBounds(_ bounds: CGRect, layer: Layer, radians: Double) -> CGRect {
        let parent = parentTransform(layer), anchor = worldOrigin(layer)
        let linear = CGAffineTransform(a: parent.a, b: parent.b, c: parent.c, d: parent.d, tx: 0, ty: 0)
        return bounds.applying(CGAffineTransform(translationX: -anchor.x, y: -anchor.y)
            .concatenating(linear.inverted()).concatenating(CGAffineTransform(rotationAngle: radians))
            .concatenating(linear).concatenating(CGAffineTransform(translationX: anchor.x, y: anchor.y)))
    }
    public func textDocument(_ layer: Layer) -> [String: Any]? {
        let id = layer.authored.object("kind").object("value").string("content_ref")
        return document.objects("texts").first { $0.string("id") == id }
    }
    public func setText(_ layer: Layer, to value: String, base: String? = nil) {
        guard !ui.locked.contains(layer.id), var text = textDocument(layer) else { return }
        do {
            text = try TextSpanEditing.replaced(text, value: value)
            submit([["text_set": ["text": text]]], label: "Text の変更", base: base)
        } catch { mapFailure(error) }
    }
    public func setBool(_ layer: Layer, property: [String: Any], to value: Bool, base: String? = nil) {
        guard !ui.locked.contains(layer.id), property.object("source").string("kind") == "constant",
              layer.value(property).string("kind") == "bool" else { return }
        submit([["property_source_set": ["object": layer.id, "property": property.string("id"),
            "source": ["kind": "constant", "value": ["kind": "bool", "value": value]]]]], label: "Bool の変更", base: base)
    }
    public func setEnum(_ layer: Layer, property: [String: Any], to value: String, base: String? = nil) {
        guard !ui.locked.contains(layer.id), property.object("source").string("kind") == "constant" else { return }
        submit([["property_source_set": ["object": layer.id, "property": property.string("id"),
            "source": ["kind": "constant", "value": ["kind": "enum", "value": value]]]]], label: "Alignment の変更")
    }
    public func keyframes(_ property: [String: Any]) -> [[String: Any]] {
        guard let id = property.object("source")["value"] as? String else { return [] }
        return document.objects("curves").first { $0.string("id") == id }?.objects("keys") ?? []
    }
    public func keyFrame(_ key: [String: Any]) -> Int64 {
        let time = key.object("time")
        return RationalTime(num: Int64(time.string("num")) ?? 0, den: Int64(time.string("den")) ?? 1).frames(rateNum: rateNum, rateDen: rateDen)
    }
    public func keyTime(_ key: [String: Any]) -> RationalTime {
        let time = key.object("time")
        return .init(num: Int64(time.string("num")) ?? 0, den: Int64(time.string("den")) ?? 1)
    }
    public func seekKey(_ key: [String: Any]) {
        let time = keyTime(key), duration = current.object("duration")
        let end = RationalTime(num: Int64(duration.string("num")) ?? 0, den: Int64(duration.string("den")) ?? 1)
        guard !timeBefore(time, .init(num: 0, den: 1)), timeBefore(time, end) else { return }
        ui.time = time
        Task { do { try await reload() } catch { mapFailure(error) } }
    }
    public func onKeyframe(_ property: [String: Any]) -> Bool { keyframes(property).contains { keyTime($0) == ui.time } }
    public func seekAdjacent(_ property: [String: Any], forward: Bool) {
        let keys = keyframes(property).sorted { timeBefore(keyTime($0), keyTime($1)) }
        if let key = forward ? keys.first(where: { timeBefore(ui.time, keyTime($0)) }) : keys.last(where: { timeBefore(keyTime($0), ui.time) }) { seekKey(key) }
    }
    private func timeBefore(_ left: RationalTime, _ right: RationalTime) -> Bool {
        let lhs = (Int64(left.num) ?? 0).multipliedFullWidth(by: max(1, Int64(right.den) ?? 1))
        let rhs = (Int64(right.num) ?? 0).multipliedFullWidth(by: max(1, Int64(left.den) ?? 1))
        return lhs.high == rhs.high ? lhs.low < rhs.low : lhs.high < rhs.high
    }
    public func keyFramePosition(_ key: [String: Any]) -> Double {
        let time = keyTime(key)
        return (Double(time.num) ?? 0) / max(1, Double(time.den) ?? 1) * Double(rateNum) / Double(rateDen)
    }
    public var currentFramePosition: Double { keyFramePosition(["time": ui.time.wire]) }
    public func propertyError(_ layer: Layer, _ property: [String: Any]) -> KRDiagnostic? {
        guard layer.evaluated.isEmpty, let failure = previewFailure else { return nil }
        return .init(failure.code, failure.message)
    }
    public func beginCanvasEdit() -> CanvasEdit? {
        guard !busy, pendingCandidate == nil, let layer = selected, !ui.locked.contains(layer.id), let bounds = layer.bounds(ui.bounds) else { return nil }
        let parent = parentTransform(layer)
        guard abs(parent.a * parent.d - parent.b * parent.c) > 0.000001 else { return nil }
        return .init(layer: layer, base: revision, time: ui.time, bounds: bounds, parent: parent)
    }
    public func previewCanvas(_ edit: CanvasEdit, translation: CGSize, handle: Int?, rotate: Bool) {
        let dx = translation.width * extent.width, dy = translation.height * extent.height
        if let handle {
            let sx = [1, 6].contains(handle) ? 1 : max(0.01, 1 + ([0, 3, 5].contains(handle) ? -dx : dx) / max(1, edit.bounds.width))
            let sy = [3, 4].contains(handle) ? 1 : max(0.01, 1 + ([0, 1, 2].contains(handle) ? -dy : dy) / max(1, edit.bounds.height))
            candidateBounds = scaledBounds(edit.bounds, layer: edit.layer, x: sx, y: sy)
            if rotate {
                let angle = atan2(dy, max(1, edit.bounds.width) + dx)
                candidateBounds = rotatedBounds(edit.bounds, layer: edit.layer, radians: angle)
            }
        } else { candidateBounds = edit.bounds.offsetBy(dx: dx, dy: dy) }
    }
    public func commitCanvas(_ edit: CanvasEdit, translation: CGSize, handle: Int?, rotate: Bool) {
        guard abs(translation.width) + abs(translation.height) > 0, !ui.locked.contains(edit.layer.id) else { candidateBounds = nil; return }
        let key = handle == nil ? "kronello.transform.position" : rotate ? "kronello.transform.rotation" : "kronello.transform.scale"
        guard let property = transformProperty(edit.layer, key: key) else {
            candidateBounds = nil
            mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "このレイヤーには編集可能な \(key) がありません")); return
        }
        var values = propertyNumbers(edit.layer, property)
        let dx = translation.width * extent.width, dy = translation.height * extent.height
        if handle == nil && values.count == 2 {
            let inverse = edit.parent.inverted()
            values[0] += inverse.a * dx + inverse.c * dy; values[1] += inverse.b * dx + inverse.d * dy
        } else if rotate && values.count == 1 {
            values[0] += atan2(dy, max(1, edit.bounds.width) + dx) * 180 / .pi
        } else if let handle, values.count == 2 {
            if ![1, 6].contains(handle) { values[0] *= max(0.01, 1 + ([0, 3, 5].contains(handle) ? -dx : dx) / max(1, edit.bounds.width)) }
            if ![3, 4].contains(handle) { values[1] *= max(0.01, 1 + ([0, 1, 2].contains(handle) ? -dy : dy) / max(1, edit.bounds.height)) }
        } else { return }
        do { submit([try numericCommand(layer: edit.layer, property: property, values: values, time: edit.time)], label: key + " の変更", base: edit.base) }
        catch { mapFailure(error) }
    }
    public func hitTest(_ point: CGPoint) -> String? {
        layers.reversed().first { !ui.locked.contains($0.id) && $0.enabled && ($0.bounds("visual")?.contains(point) ?? false) }?.id
    }
    public func create(tool: String, from start: CGPoint, to end: CGPoint, points: [CGPoint] = []) {
        do { submit(try creationCommands(tool: tool, from: start, to: end, points: points), label: tool == "text" ? "Text の作成" : "Shape の作成") }
        catch { mapFailure(error) }
    }
    public func creationCommands(tool: String, from start: CGPoint, to end: CGPoint, points: [CGPoint] = []) throws -> [[String: Any]] {
        guard !current.isEmpty else { throw ServiceFailure(code: "INVALID_EDIT", message: "Composition がありません") }
        let nodeID = UUID().uuidString, contentID = UUID().uuidString
        var properties: [[String: Any]] = []
        func property(_ key: String, _ kind: String, _ value: Any) -> String {
            let id = UUID().uuidString
            properties.append(["id": id, "descriptor": ["key": key, "version": 1], "source": ["kind": "constant", "value": ["kind": kind, "value": value]], "modifiers": []])
            return id
        }
        let origin = CGPoint(x: min(start.x, end.x), y: min(start.y, end.y))
        _ = property("kronello.transform.position", "vec2", [origin.x, origin.y])
        _ = property("kronello.transform.scale", "vec2", [1.0, 1.0])
        _ = property("kronello.transform.rotation", "angle", 0.0)
        _ = property("kronello.opacity", "scalar", 1.0)
        let color = property("kronello.fill_color", "color", ["space": "srgb", "components": ["r": 1.0, "g": 1.0, "b": 1.0, "alpha": 1.0]])
        var commands: [[String: Any]] = []
        if tool == "text" {
            guard let font = textFont else { throw ServiceFailure(code: "FONT_MISSING", message: "明示的に解決した font lock が必要です") }
            let size = property("kronello.text.font_size", "scalar", 64.0)
            let wrap = property("kronello.text.wrap_width", "scalar", max(1, abs(end.x - start.x)))
            let height = property("kronello.text.line_height", "scalar", 80.0)
            let alignment = property("kronello.text.alignment", "enum", "start")
            commands.append(["text_set": ["text": ["id": contentID, "layout_version": 1, "text": "テキスト", "direction": "horizontal",
                "styles": [["range": ["start": 0, "end": "テキスト".utf8.count], "font": font, "size": size, "fill": color]],
                "wrap_width": wrap, "line_height": height, "alignment": alignment]]])
        } else {
            var geometry: [String: Any]
            if tool == "pen" {
                guard points.count >= 3 else { throw ServiceFailure(code: "INVALID_EDIT", message: "ペンで三点以上の輪郭を描いてください") }
                var segments: [[String: Any]] = points.enumerated().map { index, point in ["kind": index == 0 ? "move_to" : "line_to", "value": [point.x - origin.x, point.y - origin.y]] }
                segments.append(["kind": "close"])
                let path = property("kronello.shape.path", "path", ["segments": segments])
                geometry = ["kind": "bezier_path", "value": ["path": path]]
            } else {
                let size = property("kronello.shape.size", "vec2", [max(1, abs(end.x - start.x)), max(1, abs(end.y - start.y))])
                var value: [String: Any] = ["size": size]
                if tool == "rectangle" { value["corner_radius"] = property("kronello.shape.corner_radius", "scalar", 0.0) }
                geometry = ["kind": tool == "ellipse" ? "ellipse" : "rectangle", "value": value]
            }
            commands.append(["shape_set": ["shape": ["id": contentID, "geometry": geometry, "fill": ["color": color, "rule": "nonzero"], "stroke": NSNull()]]])
        }
        let node: [String: Any] = ["id": nodeID, "name": tool == "text" ? "Text" : "Shape", "enabled": true,
            "kind": ["kind": tool == "text" ? "text" : "shape", "value": ["content_ref": contentID]],
            "containment_parent": NSNull(), "transform_parent": NSNull(), "child_order": [String](),
            "active_range": ["start": ["num": "0", "den": "1"], "end": current.object("duration")], "properties": properties]
        commands.append(["node_add": ["composition": current.string("id"), "node": node, "index": (current["root_nodes"] as? [String] ?? []).count]])
        return commands
    }
    public static func newDocument(name: String) -> [String: Any] {
        ["id": UUID().uuidString, "schema_version": 1, "semantic_version": 1, "name": name,
         "compositions": [["id": UUID().uuidString, "duration": ["num": "10", "den": "1"], "design_extent": ["width": 1920, "height": 1080],
            "edit_rate": ["num": "24", "den": "1"], "root_nodes": [String](), "nodes": [[String: Any]](), "properties": [[String: Any]]()]], "curves": [[String: Any]]()]
    }
}
