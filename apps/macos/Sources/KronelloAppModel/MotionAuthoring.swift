import Foundation
import CoreGraphics
import KronelloDesign

public enum TextSpanEditing {
    public static func byteRange(_ text: String, start: Int, end: Int) throws -> [String: Int] {
        let characters = Array(text)
        guard start >= 0, end > start, end <= characters.count else { throw ServiceFailure(code: "INVALID_TEXT_RANGE", message: "文字範囲は書記素の境界で指定してください") }
        return ["start": String(characters.prefix(start)).utf8.count, "end": String(characters.prefix(end)).utf8.count]
    }
    /// Rebuild complete style coverage using common grapheme prefix/suffix.
    /// Insertions inherit the adjacent style; unchanged suffix spans retain theirs.
    public static func replaced(_ text: [String: Any], value: String) throws -> [String: Any] {
        let old = Array(text.string("text")), new = Array(value), spans = text.objects("styles")
        if new.isEmpty { var result = text; result["text"] = value; result["styles"] = [[String: Any]](); return result }
        guard !spans.isEmpty else { throw ServiceFailure(code: "FONT_MISSING", message: "本文を作成する前に書体を指定してください") }
        var prefix = 0, suffix = 0
        while prefix < min(old.count, new.count), old[prefix] == new[prefix] { prefix += 1 }
        while suffix < min(old.count, new.count) - prefix, old[old.count - 1 - suffix] == new[new.count - 1 - suffix] { suffix += 1 }
        var oldOffsets = [0]
        for c in old { oldOffsets.append(oldOffsets.last! + String(c).utf8.count) }
        func style(at index: Int) -> [String: Any] {
            let byte = oldOffsets[min(max(0, index), max(0, old.count - 1))]
            return spans.first { byte >= Int($0.object("range").number("start")) && byte < Int($0.object("range").number("end")) } ?? spans[0]
        }
        var rebuilt: [[String: Any]] = [], cursor = 0
        for (index, character) in new.enumerated() {
            let oldIndex = index < prefix ? index : index >= new.count - suffix ? old.count - (new.count - index) : min(prefix, max(0, old.count - 1))
            var selected = style(at: oldIndex); selected.removeValue(forKey: "range")
            let end = cursor + String(character).utf8.count
            if let last = rebuilt.last {
                var identity = last; identity.removeValue(forKey: "range")
                if NSDictionary(dictionary: identity) == NSDictionary(dictionary: selected) {
                    rebuilt[rebuilt.count - 1]["range"] = ["start": Int(last.object("range").number("start")), "end": end]
                    cursor = end; continue
                }
            }
            selected["range"] = ["start": cursor, "end": end]; rebuilt.append(selected); cursor = end
        }
        var result = text; result["text"] = value; result["styles"] = rebuilt; return result
    }
    public static func applying(_ text: [String: Any], range: [String: Int], font: [String: Any]? = nil, fill: String? = nil, size: String? = nil) -> [String: Any] {
        let start = range["start"]!, end = range["end"]!
        var result = text, output: [[String: Any]] = []
        for span in text.objects("styles") {
            let a = Int(span.object("range").number("start")), b = Int(span.object("range").number("end"))
            if b <= start || a >= end { output.append(span); continue }
            if a < start { var left = span; left["range"] = ["start": a, "end": start]; output.append(left) }
            var middle = span; middle["range"] = ["start": max(a, start), "end": min(b, end)]
            if let font { middle["font"] = font }; if let fill { middle["fill"] = fill }; if let size { middle["size"] = size }
            output.append(middle)
            if b > end { var right = span; right["range"] = ["start": end, "end": b]; output.append(right) }
        }
        result["styles"] = output; return result
    }
}

extension EditorModel {
    public var canvasSettings: CanvasSettings {
        get { ui.canvas ?? .init() }
        set { ui.canvas = newValue }
    }
    public var editViewSettings: EditViewSettings {
        get { ui.editView ?? .init() }
        set { ui.editView = newValue }
    }
    public func addGuide(axis: String, position: Double) {
        guard ["x", "y"].contains(axis), position.isFinite else { return }
        var settings = canvasSettings; settings.guides.append(.init(axis: axis, position: position)); canvasSettings = settings
    }
    public func removeGuide(_ id: String) { var settings = canvasSettings; settings.guides.removeAll { $0.id == id }; canvasSettings = settings }
    public func snappedCanvasTranslation(_ edit: CanvasEdit, translation: CGSize, snap: Bool = true) -> CGSize {
        guard snap, canvasSettings.snap else { return translation }
        let dx = translation.width * compositionExtent.width, dy = translation.height * compositionExtent.height
        let bounds = edit.bounds.offsetBy(dx: dx, dy: dy)
        func adjustment(axis: String, anchors: [Double], extent: Double) -> Double {
            let targets = [0.0, extent / 2, extent] + canvasSettings.guides.filter { $0.axis == axis }.map(\.position)
            let deltas = anchors.flatMap { anchor in targets.map { $0 - anchor } }
            guard let delta = deltas.min(by: { abs($0) < abs($1) }), abs(delta) <= 8 else { return 0 }
            return delta
        }
        return CGSize(width: (dx + adjustment(axis: "x", anchors: [bounds.minX, bounds.midX, bounds.maxX], extent: compositionExtent.width)) / max(1, compositionExtent.width), height: (dy + adjustment(axis: "y", anchors: [bounds.minY, bounds.midY, bounds.maxY], extent: compositionExtent.height)) / max(1, compositionExtent.height))
    }
    public var lockedFonts: [[String: Any]] { fonts.map { $0.object("identity") }.filter { !$0.string("sha256").isEmpty } }
    public func importFont(path: String, faceIndex: Int = 0) async {
        do {
            let identity = try await transport.call(["operation": "font.pin", "path": path, "face_index": faceIndex])
            let source = LockedFontSource(identity: identity, path: path)
            fonts.removeAll { NSDictionary(dictionary: $0.object("identity")) == NSDictionary(dictionary: identity) }
            fonts.append(["identity": identity, "path": path])
            var saved = ui.fontSources ?? []; saved.removeAll { $0.sha256 == source.sha256 && $0.faceIndex == source.faceIndex }; saved.append(source); ui.fontSources = saved
            try await reload()
        } catch { mapFailure(error) }
    }
    public func setSpanFont(_ layer: Layer, start: Int, end: Int, font: [String: Any], base: String? = nil) {
        guard !ui.locked.contains(layer.id), let text = textDocument(layer), lockedFonts.contains(where: { NSDictionary(dictionary: $0) == NSDictionary(dictionary: font) }) else { return }
        do {
            let range = try TextSpanEditing.byteRange(text.string("text"), start: start, end: end)
            submit([["text_set": ["text": TextSpanEditing.applying(text, range: range, font: font)]]], label: "文字範囲の書体・ウェイトの変更", base: base)
        } catch { mapFailure(error) }
    }
    public func setSpanSize(_ layer: Layer, start: Int, end: Int, size: Double, base: String? = nil) {
        guard !ui.locked.contains(layer.id), let text = textDocument(layer), size > 0, size.isFinite else { return }
        do {
            let range = try TextSpanEditing.byteRange(text.string("text"), start: start, end: end), id = UUID().uuidString
            let property: [String: Any] = ["id": id, "descriptor": ["key": "kronello.text.style_size", "version": 1], "source": ["kind": "constant", "value": ["kind": "scalar", "value": size]], "modifiers": []]
            submit([["node_property_insert": ["composition": current.string("id"), "node": layer.id, "property": property]], ["text_set": ["text": TextSpanEditing.applying(text, range: range, size: id)]]], label: "文字範囲のサイズの変更", base: base)
        } catch { mapFailure(error) }
    }
    public static func colorValue(hex: String) throws -> [String: Any] {
        let input = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard [6, 8].contains(input.count), input.allSatisfy({ $0.isHexDigit }) else { throw ServiceFailure(code: "INVALID_COLOR", message: "#RRGGBB または #RRGGBBAA を指定してください") }
        let chars = Array(input)
        var components = stride(from: 0, to: chars.count, by: 2).map { Double(UInt8(String(chars[$0...$0 + 1]), radix: 16)!) / 255 }
        if components.count == 3 { components.append(1) }
        return ["kind": "color", "value": ["space": "srgb", "components": ["r": components[0], "g": components[1], "b": components[2], "alpha": components[3]]]]
    }
    public func colorCommand(layer: Layer, property: [String: Any], color: [String: Any], time: RationalTime) throws -> [String: Any] {
        let source = property.object("source")
        if source.string("kind") == "expression" { throw ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "Expression の色は直接編集できません") }
        if source.string("kind") == "curve", let id = source["value"] as? String {
            let interpolation = Self.colorInterpolation(keys: keyframes(property), time: time)
            return ["keyframe_upsert": ["curve": id, "key": ["time": time.wire, "value": color, "interpolation": interpolation]]]
        }
        return ["property_source_set": ["object": layer.id, "property": property.string("id"), "source": ["kind": "constant", "value": color]]]
    }
    public static func colorInterpolation(keys: [[String: Any]], time: RationalTime) -> [String: Any] {
        guard let n = Int64(time.num), let d = Int64(time.den), d > 0 else { return ["kind": "linear"] }
        var selected = keys.first
        for key in keys {
            let value = key.object("time")
            guard let kn = Int64(value.string("num")), let kd = Int64(value.string("den")), kd > 0 else { continue }
            let lhs = kn.multipliedFullWidth(by: d), rhs = n.multipliedFullWidth(by: kd)
            if lhs.high < rhs.high || (lhs.high == rhs.high && lhs.low <= rhs.low) { selected = key }
            else { break }
        }
        return selected?.object("interpolation") ?? ["kind": "linear"]
    }
    public func setColor(_ layer: Layer, property: [String: Any], hex: String, base: String? = nil, time: RationalTime? = nil) {
        guard !ui.locked.contains(layer.id) else { return }
        do { submit([try colorCommand(layer: layer, property: property, color: Self.colorValue(hex: hex), time: time ?? ui.time)], label: "色の変更", base: base) }
        catch { mapFailure(error) }
    }
    public func setSpanColor(_ layer: Layer, start: Int, end: Int, hex: String, base: String? = nil) {
        guard !ui.locked.contains(layer.id), let text = textDocument(layer) else { return }
        do {
            let range = try TextSpanEditing.byteRange(text.string("text"), start: start, end: end), id = UUID().uuidString
            let property: [String: Any] = ["id": id, "descriptor": ["key": "kronello.text.style_color", "version": 1], "source": ["kind": "constant", "value": try Self.colorValue(hex: hex)], "modifiers": []]
            submit([["node_property_insert": ["composition": current.string("id"), "node": layer.id, "property": property]], ["text_set": ["text": TextSpanEditing.applying(text, range: range, fill: id)]]], label: "文字範囲の色の変更", base: base)
        } catch { mapFailure(error) }
    }
}

extension EditorModel {
    public static func srgbComponents(_ value: [String: Any]) -> [Double] {
        let color = value.object("value"), components = color.object("components")
        var rgb = ["r", "g", "b"].map { components.number($0) }
        if color.string("space") == "linear_rec2020" {
            let matrix = [[1.660491,-0.5876411,-0.0728499],[-0.1245505,1.1328999,-0.0083494],[-0.0181508,-0.1005789,1.1187297]]
            rgb = matrix.map { zip($0,rgb).reduce(0) { $0 + $1.0 * $1.1 } }
        }
        if color.string("space") != "srgb" { rgb = rgb.map { $0 <= 0.0031308 ? $0 * 12.92 : 1.055 * pow($0,1 / 2.4) - 0.055 } }
        return rgb + [components["alpha"] == nil ? 1 : components.number("alpha")]
    }
}
