import AppKit
import SwiftUI
import KronelloDesign
import KronelloAppModel

/// Shared bottom panel: time geometry and key selection survive editor switches.
struct DopeSheet: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    @State private var search = ""
    @State private var editor = "dope"
    @State private var graphMode = "value"
    @State private var snap = true
    @State private var zoom = "1"
    @State private var gesture: KeyGesture?
    @State private var delta: Int64 = 0
    @State private var gestureRows: [Row]?
    @State private var gestureCurves: [String: [[String: Any]]] = [:]
    @State private var marquee: CGRect?
    @State private var marqueeSelection: Set<KeyReference> = []
    @FocusState private var focused: Bool
    var column: CGFloat { model.ui.layout.valuesVisible ? KRWindowMetrics.valuesExpanded : KRWindowMetrics.valuesCollapsed }
    var frames: ClosedRange<Double> {
        let span = Double(max(1, model.durationFrames)) / (Double(zoom) ?? 1)
        let start = min(floor(model.currentFramePosition / span) * span, max(0, Double(model.durationFrames) - span))
        return start...(start + span)
    }
    var body: some View {
        KRPanel(header: {
            HStack(spacing: KRSpace.space3) {
                KRSegmentedControl([.init("dope", "Dope sheet"), .init("curve", "Curve editor")], selection: $editor)
                Text(model.timecode).krText(KRType.timecode).foregroundStyle(p.accentInk)
                interpolationControl
            }.padding(.leading, KRSpace.space3)
        }, actions: {
            KRPopupButton("時間軸の拡大", options: [.init("1", "100%"), .init("2", "200%"), .init("4", "400%")], selection: $zoom).frame(width: 96)
            KRButton(icon: .magnet, accessibilityLabel: "キーフレームにスナップ", pressed: snap) { snap.toggle() }
        }) {
            if editor == "curve" {
                CurvePanel(model: model, frames: frames, snap: snap, graphMode: $graphMode, fitTime: { zoom = "1" })
            } else { sheet }
        }.focusable().focused($focused).focusEffectDisabled().krFocusRing(focused, inset: true)
            .onKeyPress(.delete) { if editor == "dope" || graphMode == "value" { model.deleteSelectedKeys() }; return .handled }
            .onKeyPress(.leftArrow) { nudge(-1); return .handled }
            .onKeyPress(.rightArrow) { nudge(1); return .handled }
            .onKeyPress(.escape) { endGesture(); marquee = nil; return .handled }
    }
    var interpolationControl: some View {
        KRPopupButton("補間", options: [.init("linear", "Linear"), .init("cubic", "Cubic"), .init("hold", "Hold")], selection: Binding(get: {
            guard let ref = model.keySelection.first else { return "linear" }
            return model.curveKeys(ref.curve).first { model.keyTime($0) == ref.time }?.object("interpolation").string("kind") ?? "linear"
        }, set: { mode in if let candidate = model.interpolationCandidate(mode) { Task { _ = await model.apply(candidate) } } }))
            .frame(width: 96).disabled(model.keySelection.isEmpty || model.busy || model.pendingCandidate != nil || (editor == "curve" && graphMode == "velocity"))
    }
    var sheet: some View {
        GeometryReader { proxy in
            let width = max(1, proxy.size.width - column)
            VStack(spacing: 0) {
                HStack(spacing: 0) {
                    HStack(spacing: KRSpace.space2) {
                        KRSearchField("Property を検索", text: $search)
                        KRButton(icon: model.ui.layout.valuesVisible ? .chevronsLeft : .chevronsRight,
                            accessibilityLabel: model.ui.layout.valuesVisible ? "値の列を畳む" : "値の列を開く") { model.ui.layout.valuesVisible.toggle() }
                    }.padding(.horizontal, KRSpace.space2).frame(width: column, height: KRSize.rowHeight).overlay(alignment: .trailing) { p.line.frame(width: 1) }
                    KRRuler(ticks(width: width)).frame(width: width, height: KRSize.rowHeight)
                        .contentShape(Rectangle()).gesture(DragGesture(minimumDistance: 0).onChanged { model.seek(Int64(frameAt($0.location.x, width: width).rounded())) })
                }.overlay(alignment: .bottom) { p.line.frame(height: 1) }
                ScrollView {
                    HStack(alignment: .top, spacing: 0) {
                        VStack(spacing: 0) { ForEach(rows) { row in header(row).frame(width: column, height: KRSize.rowHeight).overlay(alignment: .bottom) { p.line.frame(height: 1) } } }
                            .overlay(alignment: .trailing) { p.line.frame(width: 1) }
                        VStack(spacing: 0) { ForEach(rows) { row in lane(row, width: width) } }
                            .overlay(alignment: .topLeading) {
                                if let marquee { Rectangle().fill(p.selectionBg.opacity(0.5)).overlay { Rectangle().stroke(p.selection, lineWidth: 1) }.frame(width: marquee.width, height: marquee.height).offset(x: marquee.minX, y: marquee.minY).allowsHitTesting(false) }
                            }.coordinateSpace(name: "dope-lanes")
                            .simultaneousGesture(DragGesture(minimumDistance: 0, coordinateSpace: .named("dope-lanes")).onChanged { v in
                                guard gesture == nil, !hitKey(v.startLocation, width: width) else { return }
                                focused = true
                                if marquee == nil { marqueeSelection = NSEvent.modifierFlags.contains(.shift) ? model.keySelection : [] }
                                let r = CGRect(x: min(v.startLocation.x, v.location.x), y: min(v.startLocation.y, v.location.y), width: abs(v.translation.width), height: abs(v.translation.height)); marquee = r
                                model.keySelection = marqueeSelection.union(keysIn(r, width: width))
                            }.onEnded { v in
                                guard !hitKey(v.startLocation, width: width) else { return }
                                if abs(v.translation.width) + abs(v.translation.height) < 3 { model.keySelection = [] }
                                marquee = nil
                            })
                    }
                }
            }.overlay(alignment: .topLeading) {
                KRPlayhead().frame(height: proxy.size.height).offset(x: column + x(model.currentFramePosition, width: width) - 6).allowsHitTesting(false)
            }.clipped()
        }
    }
    struct Row: Identifiable { let id: String; let layer: Layer; let property: [String: Any]? }
    var rows: [Row] {
        if let gestureRows { return gestureRows }
        return model.layers.flatMap { layer -> [Row] in
            let properties = model.ui.collapsed.contains(layer.id) ? [] : layer.properties.filter { search.isEmpty || PropertyPresentation.of($0).label.localizedCaseInsensitiveContains(search) }
            guard search.isEmpty || !properties.isEmpty || layer.name.localizedCaseInsensitiveContains(search) else { return [] }
            return [Row(id: layer.id, layer: layer, property: nil)] + properties.map { Row(id: layer.id + "/" + $0.string("id"), layer: layer, property: $0) }
        }
    }
    @ViewBuilder func header(_ row: Row) -> some View {
        if let property = row.property {
            let source = KRPropertySource(property)
            KRInspectorRow(PropertyPresentation.of(property).label, source: source, onKeyframe: model.onKeyframe(property), keyframeEditingEnabled: !model.ui.locked.contains(row.layer.id) && !model.busy,
                previous: source == .curve ? { model.seekAdjacent(property, forward: false) } : nil,
                toggleKeyframe: { model.toggleKeyframe(row.layer, property: property) }, next: source == .curve ? { model.seekAdjacent(property, forward: true) } : nil) {
                if model.ui.layout.valuesVisible { PropertyValue(model: model, layer: row.layer, property: property) }
            }.padding(.leading, CGFloat(row.layer.level) * KRSpace.space3)
        } else {
            KRLayerRow(row.layer.name, kind: row.layer.designKind, level: row.layer.level, hasChildren: !row.layer.properties.isEmpty,
                expanded: !model.ui.collapsed.contains(row.layer.id), hidden: !row.layer.enabled, locked: model.ui.locked.contains(row.layer.id), selected: model.ui.selection == row.layer.id,
                onSelect: { model.select(row.layer.id) }, onDisclosure: { toggle(row.layer.id) }, onVisibility: { model.toggleEnabled(row.layer) }, onLock: { model.toggleLock(row.layer.id) })
        }
    }
    func entries(_ row: Row) -> [(KeyReference, [String: Any])] {
        var seen: Set<KeyReference> = []
        return (row.property.map { [$0] } ?? row.layer.properties).flatMap { property in (gestureCurves[model.curveID(property) ?? ""] ?? model.keyframes(property)).compactMap { key in
            let ref = model.reference(property, key)
            return seen.insert(ref).inserted ? (ref, key) : nil
        } }
    }
    func lane(_ row: Row, width: CGFloat) -> some View {
        let summary = row.property == nil
        return ZStack(alignment: .topLeading) {
            (summary && model.ui.selection == row.layer.id ? p.selectionBg : p.surface100)
            ForEach(entries(row), id: \.0.id) { ref, key in
                let preview = gesture?.keys.contains { $0.reference == ref } == true ? Double(delta) : 0
                KRKeyframeGlyph(glyph(key), selected: model.keySelection.contains(ref) || gesture?.keys.contains { $0.reference == ref } == true, size: summary ? KRWindowMetrics.handle : KRSize.keyframeSize)
                    .contentShape(Rectangle()).gesture(DragGesture(minimumDistance: 0).onChanged { v in
                        focused = true
                        if abs(v.translation.width) > 2, gesture == nil {
                            if !model.keySelection.contains(ref) { model.selectKey(ref, extend: NSEvent.modifierFlags.contains(.shift)) }
                            gesture = model.beginKeyGesture()
                            if gesture != nil {
                                gestureRows = rows
                                gestureCurves = Dictionary(uniqueKeysWithValues: model.document.objects("curves").map { ($0.string("id"), $0.objects("keys")) })
                            }
                        }
                        if let gesture { delta = model.snappedDelta(gesture, frames: Double(v.translation.width / max(1, width - KRSpace.space2 * 2)) * (frames.upperBound - frames.lowerBound), snap: snap, tolerance: 6 / Double(max(1, width - 16)) * (frames.upperBound - frames.lowerBound)) }
                    }.onEnded { _ in
                        if let gesture { model.commitKeyMove(gesture, delta: delta) }
                        else { model.selectKey(ref, extend: NSEvent.modifierFlags.contains(.shift), toggle: NSEvent.modifierFlags.contains(.command)) }
                        endGesture()
                    }).position(x: x(model.keyFramePosition(key) + preview, width: width), y: KRSize.rowHeight / 2)
            }
        }.frame(width: width, height: KRSize.rowHeight).overlay(alignment: .bottom) { p.line.frame(height: 1) }.clipped()
    }
    func hitKey(_ point: CGPoint, width: CGFloat) -> Bool {
        let i = Int(point.y / KRSize.rowHeight)
        guard rows.indices.contains(i) else { return false }
        return entries(rows[i]).contains { abs(x(model.keyFramePosition($0.1), width: width) - point.x) <= KRSize.keyframeSize / 2 + KRSpace.space1 }
    }
    func keysIn(_ r: CGRect, width: CGFloat) -> Set<KeyReference> {
        Set(rows.enumerated().flatMap { i, row in entries(row).compactMap { ref, key in r.contains(CGPoint(x: x(model.keyFramePosition(key), width: width), y: (Double(i) + 0.5) * KRSize.rowHeight)) ? ref : nil } })
    }
    func glyph(_ key: [String: Any]) -> KRInterpolation { KRInterpolation(rawValue: key.object("interpolation").string("kind")) ?? .linear }
    func x(_ frame: Double, width: CGFloat) -> CGFloat { KRSpace.space2 + (frame - frames.lowerBound) / (frames.upperBound - frames.lowerBound) * max(1, width - KRSpace.space2 * 2) }
    func frameAt(_ x: CGFloat, width: CGFloat) -> Double { frames.lowerBound + Double((x - KRSpace.space2) / max(1, width - KRSpace.space2 * 2)) * (frames.upperBound - frames.lowerBound) }
    func ticks(width: CGFloat) -> [KRRulerTick] {
        (0...10).map { i in let frame = Int64(frames.lowerBound + Double(i) / 10 * (frames.upperBound - frames.lowerBound)); return .init("tick-\(i)", x: x(Double(frame), width: width), label: i == 10 ? nil : "\(frame / Int64(model.nominalFPS))s\(frame % Int64(model.nominalFPS))f") }
    }
    func endGesture() { gesture = nil; delta = 0; gestureRows = nil; gestureCurves = [:] }
    func nudge(_ delta: Int64) { guard editor == "dope" || graphMode == "value" else { return }; if let gesture = model.beginKeyGesture() { model.commitKeyMove(gesture, delta: delta) } }
    func toggle(_ id: String) { if model.ui.collapsed.contains(id) { model.ui.collapsed.remove(id) } else { model.ui.collapsed.insert(id) } }
}
