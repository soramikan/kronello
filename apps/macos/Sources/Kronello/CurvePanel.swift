import AppKit
import SwiftUI
import KronelloAppModel
import KronelloDesign

struct CurvePanel: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    let frames: ClosedRange<Double>
    let snap: Bool
    @Binding var graphMode: String
    let fitTime: () -> Void
    @State private var propertyID: String?
    @State private var axis = 0
    @State private var hidden: Set<Int> = []
    @State private var alignment: Bool?
    @State private var keyGesture: KeyGesture?
    @State private var keyDelta: Int64 = 0
    @State private var tangentOrigin: (keys: [[String: Any]], base: String, aligned: Bool, axis: Int)?
    @State private var tangentPreview: EditCandidate?
    @State private var frozenValues: ClosedRange<Double>?
    @State private var frozenKeys: [[String: Any]]?
    @State private var frozenCurve: String?
    @State private var frozenSelection: Set<KeyReference>?
    @State private var frozenPresentation: PropertyPresentation?
    var targets: [(Layer, [String: Any])] { model.layers.flatMap { layer in layer.properties.filter { $0.object("source").string("kind") != "constant" }.map { (layer, $0) } } }
    var target: (Layer, [String: Any])? { targets.first { $0.1.string("id") == propertyID } ?? targets.first { $0.0.id == model.ui.selection } ?? targets.first }
    var curve: String { frozenCurve ?? target.flatMap { model.curveID($0.1) } ?? "" }
    var sourceKeys: [[String: Any]] { frozenKeys ?? model.curveKeys(curve) }
    var selection: Set<KeyReference> { frozenSelection ?? model.keySelection }
    var keys: [[String: Any]] {
        var keys = tangentOrigin?.keys ?? sourceKeys
        if let preview = tangentPreview {
            for command in preview.commands {
                let key = command.object("keyframe_replace").object("key")
                if let i = keys.firstIndex(where: { model.keyTime($0) == model.keyTime(key) }) { keys[i] = key }
            }
        }
        if let gesture = keyGesture {
            keys = keys.map { key in
                guard gesture.keys.contains(where: { $0.reference == KeyReference(curve: curve, time: model.keyTime(key)) }) else { return key }
                var moved = key; moved["time"] = RationalTime(num: (Int64(model.keyFramePosition(key).rounded()) + keyDelta) * model.rateDen, den: model.rateNum).wire; return moved
            }.sorted { model.keyFramePosition($0) < model.keyFramePosition($1) }
        }
        return keys
    }
    var presentation: PropertyPresentation { frozenPresentation ?? target.map { PropertyPresentation.of($0.1) } ?? .of("kronello.transform.position") }
    var count: Int { sourceKeys.first.map { CurveDisplay.numbers($0).count } ?? target.map { model.propertyNumbers($0.0, $0.1).count } ?? 0 }
    var selectedIndex: Int? { sourceKeys.firstIndex { selection.contains(.init(curve: curve, time: model.keyTime($0))) } }
    var aligned: Bool { alignment ?? selectedIndex.map { model.tangentAligned(sourceKeys, index: $0, axis: axis) } ?? false }
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: KRSpace.space2) {
                KRSegmentedControl([.init("value", "値"), .init("velocity", "速度")], selection: $graphMode)
                Text(presentation.label).krText(KRType.label).foregroundStyle(p.ink)
                KRSegmentedControl([.init("aligned", "揃える"), .init("broken", "分ける")], selection: Binding(get: { aligned ? "aligned" : "broken" }, set: { alignment = $0 == "aligned" })).disabled(selectedIndex == nil || graphMode == "velocity")
                KRButton(icon: .scan, accessibilityLabel: "全体を表示") { fitTime() }
                Spacer()
                Text(graphMode == "velocity" ? "速度は表示のみ" : "時間イージング").krText(KRType.caption).foregroundStyle(p.inkMuted)
            }.padding(.horizontal, KRSpace.space3).frame(height: KRSize.panelHeaderHeight).overlay(alignment: .bottom) { p.line.frame(height: 1) }
            HStack(spacing: 0) {
                channels.frame(width: KRWindowMetrics.valuesCollapsed).overlay(alignment: .trailing) { p.line.frame(width: 1) }
                GeometryReader { proxy in
                    if target?.1.object("source").string("kind") == "expression" {
                        KREmptyState(icon: .slidersHorizontal, title: "式のため編集不可", message: "式で決まる Property に編集用の曲線はありません。")
                    } else if keys.isEmpty || count == 0 {
                        KREmptyState(icon: .slidersHorizontal, title: "チャンネルを選択", message: "数値の AnimationCurve を選択してください。")
                    } else {
                        KRCurveEditor(channels: graphChannels, keys: graphKeys, tangents: graphTangents, frames: frames, values: valueRange,
                            ticks: ticks(proxy.size.width), playhead: model.currentFramePosition, readOnly: graphMode == "velocity",
                            select: select, move: move, tangent: tangent, clear: { model.keySelection = [] })
                    }
                }
            }
            HStack(spacing: KRSpace.space2) {
                Text("X / Y は同じ時間イージングを共有します。どちらの接線編集も両方に反映されます。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                Spacer()
            }.padding(.horizontal, KRSpace.space3).frame(height: KRSize.rowHeight).background(p.surface100)
        }.onChange(of: propertyID) { _, _ in axis = 0; alignment = nil; hidden = [] }
            .onChange(of: model.keySelection) { _, _ in alignment = nil }
            .onKeyPress(.escape) { keyGesture = nil; keyDelta = 0; tangentOrigin = nil; tangentPreview = nil; finishGesture(); return .handled }
    }
    var channels: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                Text("チャンネル").krText(KRType.heading).foregroundStyle(p.ink).padding(KRSpace.space3)
                ForEach(model.layers) { layer in
                    let properties = targets.filter { $0.0.id == layer.id }
                    if !properties.isEmpty {
                        Text(layer.name).krText(KRType.label).foregroundStyle(p.ink).padding(.leading, KRSpace.space2).frame(height: KRSize.rowHeight)
                        ForEach(properties, id: \.1.idString) { _, property in
                            KRButton(PropertyPresentation.of(property).label, icon: .chevronDown, variant: .plain) { propertyID = property.string("id") }.padding(.leading, KRSpace.space3)
                            if target?.1.string("id") == property.string("id") {
                                if property.object("source").string("kind") == "expression" { Text("式のため編集不可").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.leading, KRSpace.space4) }
                                else { ForEach(0..<count, id: \.self) { i in channelRow(i) } }
                            }
                        }
                    }
                }
            }
        }.background(p.surface100)
    }
    func channelRow(_ i: Int) -> some View {
        HStack(spacing: KRSpace.space1) {
            KRButton(channelName(i), variant: .plain) { axis = i; alignment = nil }
            Path { path in path.move(to: .init(x: 0, y: 5)); path.addLine(to: .init(x: 18, y: 5)) }.stroke(axis == i ? p.ink : p.inkMuted, style: StrokeStyle(lineWidth: axis == i ? 1.5 : 1, dash: axis == i ? [] : [3, 2])).frame(width: 18, height: 10)
            Text(currentValue(i)).krText(KRType.ruler).foregroundStyle(p.inkMuted).lineLimit(1)
            Spacer(minLength: 0)
            KRButton(icon: hidden.contains(i) ? .eyeOff : .eye, accessibilityLabel: channelName(i) + " の表示") { if hidden.contains(i) { hidden.remove(i) } else { hidden.insert(i) } }
        }.padding(.leading, KRSpace.space4).padding(.trailing, KRSpace.space2).frame(height: KRSize.rowHeight).background(axis == i ? p.selectionBg : .clear)
    }
    func channelName(_ i: Int) -> String { count == 1 ? presentation.label : ["X", "Y", "Z", "W"][min(i, 3)] }
    func currentValue(_ i: Int) -> String {
        let values = target.map { model.propertyNumbers($0.0, $0.1) } ?? []
        return values.indices.contains(i) ? String(format: "%.1f %@", values[i] * presentation.multiplier, presentation.unit) : "—"
    }
    func velocityValue(_ i: Int) -> String {
        let positions = keys.map(model.keyFramePosition), f = model.currentFramePosition
        let a = CurveDisplay.sample(keys, frame: f - 0.01, positions: positions), b = CurveDisplay.sample(keys, frame: f + 0.01, positions: positions)
        guard a.indices.contains(i), b.indices.contains(i) else { return "—" }
        let value = (b[i] - a[i]) / 0.02 * Double(model.rateNum) / Double(model.rateDen) * presentation.multiplier
        return String(format: "%.1f %@/s", value, presentation.unit)
    }
    func points(_ axis: Int) -> [CGPoint] {
        let positions = keys.map(model.keyFramePosition)
        return (0...400).compactMap { i in
            let frame = frames.lowerBound + Double(i) / 400 * (frames.upperBound - frames.lowerBound)
            var values = CurveDisplay.sample(keys, frame: frame, positions: positions)
            if graphMode == "velocity" {
                let a = CurveDisplay.sample(keys, frame: frame - 0.01, positions: positions), b = CurveDisplay.sample(keys, frame: frame + 0.01, positions: positions)
                values = zip(a, b).map { ($1 - $0) / 0.02 * Double(model.rateNum) / Double(model.rateDen) }
            }
            return values.indices.contains(axis) ? CGPoint(x: frame, y: values[axis] * presentation.multiplier) : nil
        }
    }
    var graphChannels: [KRCurveChannel] { (0..<count).filter { !hidden.contains($0) }.map { .init(channelName($0), points: points($0), active: axis == $0, current: graphMode == "value" ? currentValue($0) : velocityValue($0)) } }
    var valueRange: ClosedRange<Double> {
        if let frozenValues { return frozenValues }
        let values = graphChannels.flatMap { $0.points.map(\.y) } + graphTangents.map { $0.point.y }
        let lo = values.min() ?? 0, hi = values.max() ?? 1, margin = max(1, (hi - lo) * 0.15)
        return (lo - margin)...(hi + margin)
    }
    var graphKeys: [KRCurveKey] {
        sourceKeys.enumerated().compactMap { i, key in
            let ref = KeyReference(curve: curve, time: model.keyTime(key)), values = CurveDisplay.numbers(key)
            guard values.indices.contains(axis), !hidden.contains(axis) else { return nil }
            let f = model.keyFramePosition(key) + (keyGesture?.keys.contains { $0.reference == ref } == true ? Double(keyDelta) : 0)
            let frame = Int64(f.rounded()), fps = Int64(model.nominalFPS)
            return .init(ref.id, point: CGPoint(x: f, y: values[axis] * presentation.multiplier), interpolation: KRInterpolation(rawValue: key.object("interpolation").string("kind")) ?? .linear, selected: selection.contains(ref), label: "\(frame / fps)s\(frame % fps)f · " + String(format: "%.1f %@", values[axis] * presentation.multiplier, presentation.unit))
        }
    }
    var graphTangents: [KRCurveTangent] {
        guard graphMode == "value", !hidden.contains(axis) else { return [] }
        return sourceKeys.enumerated().flatMap { i, key -> [KRCurveTangent] in
            guard selection.contains(.init(curve: curve, time: model.keyTime(key))) else { return [] }
            return [true, false].compactMap { incoming in
                let segment = incoming ? i - 1 : i
                guard keys.indices.contains(segment), keys.indices.contains(segment + 1), keys[segment].object("interpolation").string("kind") == "cubic" else { return nil }
                let a = CurveDisplay.numbers(keys[segment]), b = CurveDisplay.numbers(keys[segment + 1])
                guard a.indices.contains(axis), b.indices.contains(axis) else { return nil }
                let (c1, c2) = CurveDisplay.controls(keys[segment]), control = incoming ? c2 : c1
                let f = model.keyFramePosition(keys[segment]), span = model.keyFramePosition(keys[segment + 1]) - f
                return .init(KeyReference(curve: curve, time: model.keyTime(key)).id + "/" + (incoming ? "in" : "out"), origin: CGPoint(x: model.keyFramePosition(keys[i]), y: CurveDisplay.numbers(keys[i])[axis] * presentation.multiplier), point: CGPoint(x: f + span * control.x, y: (a[axis] + (b[axis] - a[axis]) * control.y) * presentation.multiplier))
            }
        }
    }
    func select(_ id: String) {
        guard let i = sourceKeys.firstIndex(where: { KeyReference(curve: curve, time: model.keyTime($0)).id == id }) else { return }
        model.selectKey(.init(curve: curve, time: model.keyTime(sourceKeys[i]), object: target?.0.id, property: target?.1.string("id")), extend: NSEvent.modifierFlags.contains(.shift), toggle: NSEvent.modifierFlags.contains(.command))
    }
    func move(_ id: String, _ frames: Double, _ finished: Bool) {
        guard let i = sourceKeys.firstIndex(where: { KeyReference(curve: curve, time: model.keyTime($0)).id == id }), graphMode == "value" else { return }
        if finished && keyGesture == nil { return }
        if keyGesture == nil {
            let ref = KeyReference(curve: curve, time: model.keyTime(sourceKeys[i]))
            if !model.keySelection.contains(ref) { select(id) }
            keyGesture = model.beginKeyGesture()
            if keyGesture != nil { freezeGesture() }
        }
        if let gesture = keyGesture {
            keyDelta = model.snappedDelta(gesture, frames: frames, snap: snap, tolerance: 1)
            if finished { model.commitKeyMove(gesture, delta: keyDelta); keyGesture = nil; keyDelta = 0; finishGesture() }
        }
    }
    func tangent(_ id: String, _ point: CGPoint, _ finished: Bool) {
        let parts = id.split(separator: "/")
        guard let side = parts.last, let i = sourceKeys.firstIndex(where: { KeyReference(curve: curve, time: model.keyTime($0)).id == parts.dropLast().joined(separator: "/") }), graphMode == "value", !model.busy, model.pendingCandidate == nil else { return }
        if finished && tangentOrigin == nil { return }
        if tangentOrigin == nil { frozenValues = valueRange; freezeGesture(); tangentOrigin = (sourceKeys, model.revision, aligned, axis) }
        guard let origin = tangentOrigin else { return }
        let incoming = side == "in", segment = incoming ? i - 1 : i
        let a = CurveDisplay.numbers(origin.keys[segment]), b = CurveDisplay.numbers(origin.keys[segment + 1])
        let f = model.keyFramePosition(origin.keys[segment]), span = model.keyFramePosition(origin.keys[segment + 1]) - f, dv = b[axis] - a[axis]
        let control = CGPoint(x: (point.x - f) / span, y: abs(dv) > 1e-12 ? (point.y / presentation.multiplier - a[axis]) / dv : (incoming ? CurveDisplay.controls(origin.keys[segment]).1.y : CurveDisplay.controls(origin.keys[segment]).0.y))
        do {
            tangentPreview = try model.tangentCandidate(curve: curve, index: i, incoming: incoming, control: control, aligned: origin.aligned, axis: origin.axis, keys: origin.keys, base: origin.base)
            if finished, let candidate = tangentPreview { Task { _ = await model.apply(candidate) }; tangentPreview = nil; tangentOrigin = nil; finishGesture() }
        } catch { if finished { model.mapFailure(error); tangentPreview = nil; tangentOrigin = nil; finishGesture() } }
    }
    func freezeGesture() {
        frozenKeys = sourceKeys; frozenCurve = curve; frozenSelection = model.keySelection; frozenPresentation = presentation
    }
    func finishGesture() {
        frozenKeys = nil; frozenCurve = nil; frozenSelection = nil; frozenPresentation = nil; frozenValues = nil
    }
    func ticks(_ width: CGFloat) -> [KRRulerTick] {
        (0...10).map { i in let f = Int64(frames.lowerBound + Double(i) / 10 * (frames.upperBound - frames.lowerBound)); return .init("\(i)", x: KRSpace.space2 + Double(i) / 10 * max(1, width - KRSpace.space2 * 2), label: i == 10 ? nil : "\(f / Int64(model.nominalFPS))s\(f % Int64(model.nominalFPS))f") }
    }
}
private extension Dictionary where Key == String, Value == Any { var idString: String { string("id") } }
