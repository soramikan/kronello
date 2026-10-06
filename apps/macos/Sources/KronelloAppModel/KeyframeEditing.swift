import Foundation
import CoreGraphics

/// Keys have no authored ID: use the stable curve ID and exact rational time.
public struct KeyReference: Hashable, Identifiable {
    public let curve: String
    public let time: RationalTime
    public let object: String?
    public let property: String?
    public var id: String { curve + "/" + time.num + "/" + time.den }
    public init(curve: String, time: RationalTime, object: String? = nil, property: String? = nil) { self.curve = curve; self.time = time; self.object = object; self.property = property }
    public static func == (a: Self, b: Self) -> Bool { a.curve == b.curve && a.time == b.time }
    public func hash(into h: inout Hasher) { h.combine(curve); h.combine(time.num); h.combine(time.den) }
}

/// Frozen source and revision, retained throughout a preview gesture.
public struct KeyGesture {
    public let base: String
    public let keys: [(reference: KeyReference, key: [String: Any])]
}

public enum CurveDisplay {
    /// Read-only derivative of the segment containing frame under [start, end).
    /// At a key use its outgoing segment; outside the segments the value holds.
    public static func velocity(_ keys: [[String: Any]], frame: Double, positions: [Double], framesPerSecond: Double) -> [Double] {
        guard let first = keys.first, keys.count == positions.count else { return [] }
        let zero = numbers(first).map { _ in 0.0 }
        guard let i = positions.lastIndex(where: { $0 <= frame }), i + 1 < keys.count,
              positions[i + 1] > positions[i], keys[i].object("interpolation").string("kind") != "hold" else { return zero }
        // Keep the finite-difference window inside this segment. In particular,
        // do not average an outgoing derivative with the pre-key constant region.
        let step = min(0.01, (positions[i + 1] - positions[i]) / 2)
        let lo = max(positions[i], frame - step), hi = min(positions[i + 1], frame + step)
        guard hi > lo else { return zero }
        let a = sample(keys, frame: lo, positions: positions)
        let b = sample(keys, frame: hi, positions: positions)
        return zip(a, b).map { ($1 - $0) / (hi - lo) * framesPerSecond }
    }
    public static func numbers(_ key: [String: Any]) -> [Double] {
        let v = key.object("value")["value"]
        if let a = v as? [Double] { return a }
        if let n = v as? NSNumber { return [n.doubleValue] }
        return []
    }
    public static func controls(_ key: [String: Any]) -> (CGPoint, CGPoint) {
        let v = key.object("interpolation").object("value")
        let a = v["control1"] as? [Double] ?? [1.0 / 3, 1.0 / 3]
        let b = v["control2"] as? [Double] ?? [2.0 / 3, 2.0 / 3]
        return (CGPoint(x: a[0], y: a[1]), CGPoint(x: b[0], y: b[1]))
    }
    public static func bezier(_ a: Double, _ b: Double, _ t: Double) -> Double {
        let u = 1 - t
        return 3 * u * u * t * a + 3 * u * t * t * b + t * t * t
    }
    /// Numeric primary-curve display only; document evaluation stays in the service.
    public static func sample(_ keys: [[String: Any]], frame: Double, positions: [Double]) -> [Double] {
        guard let first = keys.first, let last = keys.last else { return [] }
        guard let i = positions.lastIndex(where: { $0 <= frame }) else { return numbers(first) }
        if i == keys.count - 1 { return numbers(last) }
        let a = numbers(keys[i]), b = numbers(keys[i + 1])
        guard a.count == b.count else { return [] }
        var t = (frame - positions[i]) / max(1e-12, positions[i + 1] - positions[i])
        let kind = keys[i].object("interpolation").string("kind")
        if kind == "hold" { t = 0 }
        if kind == "cubic" {
            let (c1, c2) = controls(keys[i]); var low = 0.0, high = 1.0
            for _ in 0..<64 {
                let m = (low + high) / 2
                if bezier(c1.x, c2.x, m) < t { low = m } else { high = m }
            }
            t = bezier(c1.y, c2.y, (low + high) / 2)
        }
        return zip(a, b).map { (1 - t) * $0 + t * $1 }
    }
}

extension EditorModel {
    public func curveID(_ property: [String: Any]) -> String? {
        property.object("source").string("kind") == "curve" ? property.object("source")["value"] as? String : nil
    }
    public func reference(_ property: [String: Any], _ key: [String: Any]) -> KeyReference {
        let owner = layers.first { $0.properties.contains { $0.string("id") == property.string("id") } }?.id
        return .init(curve: curveID(property) ?? "", time: keyTime(key), object: owner, property: property.string("id"))
    }
    public func curveKeys(_ id: String) -> [[String: Any]] { document.objects("curves").first { $0.string("id") == id }?.objects("keys") ?? [] }
    public func selectKey(_ ref: KeyReference, extend: Bool = false, toggle: Bool = false) {
        if toggle && keySelection.contains(ref) { keySelection.remove(ref) }
        else if extend || toggle { keySelection.insert(ref) }
        else { keySelection = [ref] }
    }
    public func beginKeyGesture() -> KeyGesture? {
        guard !busy, pendingCandidate == nil, !keySelection.isEmpty else { return nil }
        let keys = keySelection.sorted { $0.id < $1.id }.compactMap { ref -> (KeyReference, [String: Any])? in
            guard let key = curveKeys(ref.curve).first(where: { keyTime($0) == ref.time }), curveEditable(ref.curve) else { return nil }
            return (ref, key)
        }
        return keys.isEmpty ? nil : .init(base: revision, keys: keys)
    }
    public func curveEditable(_ id: String) -> Bool {
        !layers.contains { ui.locked.contains($0.id) && $0.properties.contains { curveID($0) == id } }
    }
    public func snappedDelta(_ gesture: KeyGesture, frames: Double, snap: Bool, tolerance: Double) -> Int64 {
        var delta = frames.rounded()
        let origins = gesture.keys.map { keyFramePosition($0.key) }
        if snap {
            let targets = [currentFramePosition] + document.objects("curves").flatMap { curve in
                curve.objects("keys").filter { key in !gesture.keys.contains { $0.reference == KeyReference(curve: curve.string("id"), time: keyTime(key)) } }.map(keyFramePosition)
            }
            let offsets = origins.flatMap { origin in targets.map { $0 - origin } }
            if let best = offsets.min(by: { abs($0 - frames) < abs($1 - frames) }), abs(best - frames) <= tolerance { delta = best.rounded() }
        }
        let minimum = origins.min() ?? 0, maximum = origins.max() ?? 0
        delta = min(max(delta, -minimum), Double(max(0, durationFrames - 1)) - maximum)
        return Int64(delta.rounded())
    }
    public func moveCandidate(_ gesture: KeyGesture, delta: Int64) throws -> EditCandidate? {
        guard delta != 0 else { return nil }
        guard gesture.keys.allSatisfy({ curveEditable($0.reference.curve) }) else { throw ServiceFailure(code: "INVALID_EDIT", message: "ロックしたレイヤーのキーは移動できません") }
        var commands: [[String: Any]] = gesture.keys.map { ["keyframe_remove": ["curve": $0.reference.curve, "time": $0.reference.time.wire]] }
        for item in gesture.keys {
            let sum = Int64(keyFramePosition(item.key).rounded()).addingReportingOverflow(delta)
            guard !sum.overflow else { throw ServiceFailure(code: "INVALID_EDIT", message: "キー時刻を表現できません") }
            let frame = sum.partialValue
            guard frame >= 0, frame < durationFrames else { throw ServiceFailure(code: "INVALID_EDIT", message: "キーは Composition の範囲内に移動してください") }
            let product = frame.multipliedReportingOverflow(by: rateDen)
            guard !product.overflow else { throw ServiceFailure(code: "INVALID_EDIT", message: "キー時刻を表現できません") }
            var key = item.key; key["time"] = RationalTime(num: product.partialValue, den: rateNum).wire
            commands.append(["keyframe_insert": ["curve": item.reference.curve, "key": key]])
        }
        return .init(base: gesture.base, commands: commands, label: "キーフレームの移動")
    }
    public func commitKeyMove(_ gesture: KeyGesture, delta: Int64) {
        do {
            guard let candidate = try moveCandidate(gesture, delta: delta) else { return }
            Task {
                guard await apply(candidate) != nil else { return }
                let insertions = candidate.commands.suffix(gesture.keys.count)
                keySelection = Set(zip(gesture.keys, insertions).map { item, command in
                    let key = command.object("keyframe_insert").object("key")
                    return KeyReference(curve: item.reference.curve, time: keyTime(key), object: item.reference.object, property: item.reference.property)
                })
            }
        } catch { mapFailure(error) }
    }
    public func interpolationCandidate(_ mode: String) -> EditCandidate? {
        guard let gesture = beginKeyGesture() else { return nil }
        let commands: [[String: Any]] = gesture.keys.compactMap { item -> [String: Any]? in
            if item.key.object("interpolation").string("kind") == mode { return nil }
            var key = item.key
            key["interpolation"] = mode == "cubic" ? ["kind": "cubic", "value": ["control1": [1.0 / 3, 1.0 / 3], "control2": [2.0 / 3, 2.0 / 3]]] : ["kind": mode]
            return ["keyframe_replace": ["curve": item.reference.curve, "key": key]]
        }
        return commands.isEmpty ? nil : .init(base: gesture.base, commands: commands, label: "補間の変更")
    }
    public func toggleKeyframe(_ layer: Layer, property: [String: Any]) {
        guard !ui.locked.contains(layer.id), !busy, pendingCandidate == nil else { return }
        let base = revision, time = ui.time
        Task {
            do {
                let commands: [[String: Any]]
                if curveID(property) != nil, let key = keyframes(property).first(where: { keyTime($0) == time }) {
                    commands = try await deletionCommands([reference(property, key)], time: time, base: base)
                } else {
                    let value = layer.value(property)
                    guard !value.isEmpty, property.object("source").string("kind") != "expression" else { throw ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "式のキーは編集できません") }
                    let key: [String: Any] = ["time": time.wire, "value": value, "interpolation": ["kind": ["scalar", "angle", "vec2", "vec3", "color"].contains(value.string("kind")) ? "linear" : "hold"]]
                    if let curve = curveID(property) { commands = [["keyframe_insert": ["curve": curve, "key": key]]] }
                    else {
                        let curve = UUID().uuidString
                        var property = property, prefix: [[String: Any]] = []
                        if property.string("id").isEmpty { property["id"] = UUID().uuidString; prefix.append(["node_property_insert": ["composition": current.string("id"), "node": layer.id, "property": property]]) }
                        commands = prefix + [["property_source_set": ["object": layer.id, "property": property.string("id"), "source": ["kind": "curve", "value": curve], "curve": ["id": curve, "value_type": value.string("kind"), "interpolation_version": 1, "keys": [key]]]]]
                    }
                }
                _ = await apply(.init(base: base, commands: commands, label: "キーフレームの追加 / 削除"))
            } catch { mapFailure(error) }
        }
    }
    /// Detach the edited Property on its last key; preserve shared curve resources.
    public func deletionCommands(_ refs: Set<KeyReference>, time: RationalTime, base: String) async throws -> [[String: Any]] {
        var commands: [[String: Any]] = [], retained: Set<String> = []
        let emptied = Set(refs.map(\.curve)).filter { id in curveKeys(id).allSatisfy { refs.contains(.init(curve: id, time: keyTime($0))) } }
        for id in emptied.sorted() {
            let owners = compositions.flatMap { comp -> [(composition: String, object: String, property: [String: Any], kind: String)] in
                let nodes = comp.objects("nodes").flatMap { node in node.objects("properties").filter { curveID($0) == id }.map { (comp.string("id"), node.string("id"), $0, "node") } }
                return nodes + comp.objects("properties").filter { curveID($0) == id }.map { (comp.string("id"), comp.string("id"), $0, "composition") }
            }
            let selected = refs.filter { $0.curve == id }
            let addressed = selected.compactMap { $0.property }
            let edited = owners.filter { owner in addressed.contains(owner.property.string("id")) || (addressed.isEmpty && owners.count == 1) }
            guard !edited.isEmpty else { throw ServiceFailure(code: "INVALID_EDIT", message: "最後のキーを削除する Property を選択してください") }
            for owner in edited {
                let key: [String: Any] = ["kind": owner.kind, "instance_path": [String](), owner.kind: owner.object, "property": owner.property.string("id")]
                let sample = try await request("property.sample", ["composition": owner.composition, "keys": [key], "times": [time.wire]])
                let value = sample.objects("samples").first?.objects("values").first ?? [:]
                guard !value.isEmpty else { throw ServiceFailure(code: "EVALUATION_ERROR", message: "最後のキーを削除する値を評価できません") }
                commands.append(["property_source_set": ["object": owner.object, "property": owner.property.string("id"), "source": ["kind": "constant", "value": value]]])
            }
            let expressionConsumer = document.objects("expressions").contains { expression in
                expression.objects("nodes").contains { $0.object("curve_sample").string("curve") == id }
            }
            if owners.count > edited.count || expressionConsumer { retained.insert(id) }
        }
        commands += refs.sorted { $0.id < $1.id }.filter { !retained.contains($0.curve) }.map { ["keyframe_remove": ["curve": $0.curve, "time": $0.time.wire]] }
        return commands
    }
    public func deleteSelectedKeys() {
        guard let gesture = beginKeyGesture() else { return }
        let refs = Set(gesture.keys.map(\.reference)), time = ui.time
        Task {
            do { let commands = try await deletionCommands(refs, time: time, base: gesture.base)
                if await apply(.init(base: gesture.base, commands: commands, label: "キーフレームの削除")) != nil { keySelection.subtract(refs) }
            } catch { mapFailure(error) }
        }
    }
    public func tangentAligned(_ keys: [[String: Any]], index: Int, axis: Int) -> Bool {
        guard index > 0, index + 1 < keys.count else { return false }
        func slope(_ i: Int, incoming: Bool) -> Double? {
            guard keys[i].object("interpolation").string("kind") == "cubic" else { return nil }
            let a = CurveDisplay.numbers(keys[i]), b = CurveDisplay.numbers(keys[i + 1])
            guard a.indices.contains(axis), b.indices.contains(axis) else { return nil }
            let (c1, c2) = CurveDisplay.controls(keys[i]), x = incoming ? 1 - c2.x : c1.x, y = incoming ? 1 - c2.y : c1.y
            guard x > 1e-9 else { return nil }
            return y / x * (b[axis] - a[axis]) / (keyFramePosition(keys[i + 1]) - keyFramePosition(keys[i]))
        }
        guard let a = slope(index - 1, incoming: true), let b = slope(index, incoming: false) else { return false }
        return abs(a - b) <= 1e-6 * max(1, abs(a), abs(b))
    }
    public func tangentCandidate(curve: String, index: Int, incoming: Bool, control: CGPoint, aligned: Bool, axis: Int, keys: [[String: Any]], base: String) throws -> EditCandidate {
        let segment = incoming ? index - 1 : index
        guard curveEditable(curve), keys.indices.contains(segment), keys.indices.contains(segment + 1), control.x.isFinite, control.y.isFinite else { throw ServiceFailure(code: "INVALID_EDIT", message: "接線を編集できません") }
        var replacements: [Int: [String: Any]] = [:]
        func replace(_ i: Int, incoming: Bool, control: CGPoint) {
            var key = keys[i]; var (a, b) = CurveDisplay.controls(key)
            if incoming { b = CGPoint(x: min(1, max(a.x, control.x)), y: control.y) }
            else { a = CGPoint(x: max(0, min(b.x, control.x)), y: control.y) }
            key["interpolation"] = ["kind": "cubic", "value": ["control1": [Double(a.x), Double(a.y)], "control2": [Double(b.x), Double(b.y)]]]; replacements[i] = key
        }
        replace(segment, incoming: incoming, control: control)
        if aligned, index > 0, index + 1 < keys.count {
            let edited = replacements[segment]!, (a, b) = CurveDisplay.controls(edited)
            let x = incoming ? 1 - b.x : a.x, y = incoming ? 1 - b.y : a.y
            let values = keys.map(CurveDisplay.numbers)
            guard values.allSatisfy({ $0.indices.contains(axis) }), x > 1e-9 else { throw ServiceFailure(code: "INVALID_EDIT", message: "垂直接線は揃えられません") }
            let dv = values[segment + 1][axis] - values[segment][axis]
            let slope = y / x * dv / (keyFramePosition(keys[segment + 1]) - keyFramePosition(keys[segment]))
            let other = incoming ? index : index - 1
            let otherDV = values[other + 1][axis] - values[other][axis]
            guard abs(otherDV) > 1e-12 || abs(slope) < 1e-12 else { throw ServiceFailure(code: "INVALID_EDIT", message: "値が一定の隣接区間には接線を揃えられません") }
            let (oa, ob) = CurveDisplay.controls(keys[other]); let ox = incoming ? max(1e-6, oa.x) : max(1e-6, 1 - ob.x)
            let oy = abs(otherDV) > 1e-12 ? slope * (keyFramePosition(keys[other + 1]) - keyFramePosition(keys[other])) / otherDV * ox : 0
            replace(other, incoming: !incoming, control: CGPoint(x: incoming ? ox : 1 - ox, y: incoming ? oy : 1 - oy))
        }
        return .init(base: base, commands: replacements.keys.sorted().map { ["keyframe_replace": ["curve": curve, "key": replacements[$0]!]] }, label: "時間イージングの接線変更")
    }
}
