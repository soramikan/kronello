import SwiftUI
import KronelloDesign
import KronelloAppModel

/// motion.md Dope sheet: left column (rows with navigator, name and the same value
/// fields as the Inspector) and right lanes under one ruler and one playhead.
struct DopeSheet: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    @State private var search = ""
    var column: CGFloat { model.ui.layout.valuesVisible ? KRWindowMetrics.valuesExpanded : KRWindowMetrics.valuesCollapsed }
    var body: some View {
        KRPanel(header: {
            HStack(spacing: KRSpace.space3) {
                KRSegmentedControl([.init("dope", "Dope sheet"), .init("curve", "Curve editor", unavailableReason: "Curve editor は GUI-002 で追加します")], selection: .constant("dope"))
                Text(model.timecode).krText(KRType.timecode).foregroundStyle(p.accentInk)
            }.padding(.leading, KRSpace.space3)
        }, actions: {
            KRButton(icon: .magnet, accessibilityLabel: "キーフレームにスナップ") {}.disabled(true).help("キーフレーム編集は GUI-002 で追加します")
        }) {
            GeometryReader { proxy in
                let laneWidth = max(1, proxy.size.width - column)
                VStack(spacing: 0) {
                    HStack(spacing: 0) {
                        HStack(spacing: KRSpace.space2) {
                            KRSearchField("Property を検索", text: $search)
                            KRButton(icon: model.ui.layout.valuesVisible ? .chevronsLeft : .chevronsRight,
                                accessibilityLabel: model.ui.layout.valuesVisible ? "値の列を畳む" : "値の列を開く") { model.ui.layout.valuesVisible.toggle() }
                        }.padding(.horizontal, KRSpace.space2).frame(width: column, height: KRSize.rowHeight)
                            .overlay(alignment: .trailing) { p.line.frame(width: 1) }
                        KRRuler(ticks(width: laneWidth)).frame(width: laneWidth, height: KRSize.rowHeight)
                    }.overlay(alignment: .bottom) { p.line.frame(height: 1) }
                    ScrollView {
                        VStack(spacing: 0) {
                            ForEach(rows, id: \.id) { row in
                                HStack(spacing: 0) {
                                    header(row).frame(width: column).overlay(alignment: .trailing) { p.line.frame(width: 1) }
                                    lane(row, width: laneWidth)
                                }.frame(height: KRSize.rowHeight).overlay(alignment: .bottom) { p.line.frame(height: 1) }
                            }
                        }
                    }
                }.overlay(alignment: .topLeading) {
                    // One playhead over ruler and lanes; its handle sits in the ruler.
                    KRPlayhead().frame(height: proxy.size.height)
                        .offset(x: column + x(model.currentFramePosition, width: laneWidth) - 6)
                }.clipped()
            }
        }
    }
    struct Row: Identifiable {
        let id: String
        let layer: Layer
        let property: [String: Any]?
    }
    var rows: [Row] {
        model.layers.flatMap { layer -> [Row] in
            let properties = model.ui.collapsed.contains(layer.id) ? [] : layer.properties.filter {
                search.isEmpty || PropertyPresentation.of($0).label.localizedCaseInsensitiveContains(search)
            }
            guard search.isEmpty || !properties.isEmpty || layer.name.localizedCaseInsensitiveContains(search) else { return [] }
            return [Row(id: layer.id, layer: layer, property: nil)] + properties.map { Row(id: layer.id + "/" + $0.string("id"), layer: layer, property: $0) }
        }
    }
    @ViewBuilder func header(_ row: Row) -> some View {
        if let property = row.property {
            let source = KRPropertySource(property)
            KRInspectorRow(PropertyPresentation.of(property).label, source: source, onKeyframe: model.onKeyframe(property), keyframeEditingEnabled: false,
                previous: source == .curve ? { model.seekAdjacent(property, forward: false) } : nil,
                next: source == .curve ? { model.seekAdjacent(property, forward: true) } : nil) {
                if model.ui.layout.valuesVisible { PropertyValue(model: model, layer: row.layer, property: property) }
            }.padding(.leading, CGFloat(row.layer.level) * KRSpace.space3)
        } else {
            KRLayerRow(row.layer.name, kind: row.layer.designKind, level: row.layer.level, hasChildren: !row.layer.properties.isEmpty,
                expanded: !model.ui.collapsed.contains(row.layer.id), hidden: !row.layer.enabled, locked: model.ui.locked.contains(row.layer.id),
                selected: model.ui.selection == row.layer.id,
                onSelect: { model.select(row.layer.id) }, onDisclosure: { toggle(row.layer.id) },
                onVisibility: { model.toggleEnabled(row.layer) }, onLock: { model.toggleLock(row.layer.id) })
        }
    }
    func lane(_ row: Row, width: CGFloat) -> some View {
        let keys = row.property.map(model.keyframes) ?? row.layer.properties.flatMap(model.keyframes)
        let summary = row.property == nil
        return ZStack(alignment: .topLeading) {
            (summary && model.ui.selection == row.layer.id ? p.selectionBg : p.surface100)
            ForEach(Array(keys.enumerated()), id: \.offset) { _, key in
                KRKeyframeGlyph(glyph(key), selected: model.keyTime(key) == model.ui.time,
                    size: summary ? KRWindowMetrics.handle : KRSize.keyframeSize, action: { model.seekKey(key) })
                    .position(x: x(model.keyFramePosition(key), width: width), y: KRSize.rowHeight / 2)
            }
        }.frame(width: width, height: KRSize.rowHeight).clipped()
    }
    func glyph(_ key: [String: Any]) -> KRInterpolation {
        switch key.object("interpolation").string("kind") { case "hold": return .hold; case "cubic": return .cubic; default: return .linear }
    }
    /// Lanes keep a small inset so keys at the first and last frame stay whole.
    func x(_ frame: Double, width: CGFloat) -> CGFloat {
        let inset = KRSpace.space2
        return inset + CGFloat(frame / Double(max(1, model.durationFrames))) * max(1, width - inset * 2)
    }
    func ticks(width: CGFloat) -> [KRRulerTick] {
        let fps = Int64(max(1, model.nominalFPS))
        return (0...10).map { index in
            let frame = Int64(index) * max(1, model.durationFrames) / 10
            // The end tick is unlabeled so its label never runs past the lane.
            return .init("tick-\(index)", x: x(Double(frame), width: width), label: index == 10 ? nil : "\(frame / fps)s\(frame % fps)f")
        }
    }
    func toggle(_ id: String) {
        if model.ui.collapsed.contains(id) { model.ui.collapsed.remove(id) } else { model.ui.collapsed.insert(id) }
    }
}
