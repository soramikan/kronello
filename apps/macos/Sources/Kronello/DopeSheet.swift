import SwiftUI
import KronelloDesign
import KronelloAppModel

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
            VStack(spacing: 0) {
                HStack(spacing: 0) {
                    HStack(spacing: KRSpace.space2) {
                        KRSearchField("Property を検索", text: $search)
                        KRButton(icon: model.ui.layout.valuesVisible ? .chevronsLeft : .chevronsRight, accessibilityLabel: model.ui.layout.valuesVisible ? "値の列を畳む" : "値の列を開く") { model.ui.layout.valuesVisible.toggle() }
                    }.padding(.horizontal, KRSpace.space2).frame(width: column, height: KRSize.rowHeight)
                    GeometryReader { proxy in
                        KRRuler((0...10).map { index in
                            let frame = Int64(index) * max(1, model.durationFrames) / 10
                            return .init("tick-\(index)", x: CGFloat(index) / 10 * proxy.size.width, label: "\(frame / Int64(model.nominalFPS))s\(frame % Int64(model.nominalFPS))f")
                        })
                    }.frame(height: KRSize.rowHeight)
                }
                ScrollView {
                    VStack(spacing: 0) {
                        ForEach(model.layers) { layer in
                            if search.isEmpty || layer.name.localizedCaseInsensitiveContains(search) {
                                HStack(spacing: 0) {
                                    KRLayerRow(layer.name, kind: layer.designKind, level: layer.level, hasChildren: true,
                                        expanded: !model.ui.collapsed.contains(layer.id), locked: model.ui.locked.contains(layer.id), selected: model.ui.selection == layer.id,
                                        onSelect: { model.select(layer.id) }, onDisclosure: { toggle(layer.id) },
                                        onVisibility: { model.toggleEnabled(layer) }, onLock: { model.toggleLock(layer.id) })
                                        .frame(width: column)
                                    lane(layer.properties.flatMap(model.keyframes), summary: true)
                                }
                                if !model.ui.collapsed.contains(layer.id) {
                                    ForEach(layer.properties, id: \.selfID) { property in
                                        HStack(spacing: 0) {
                                            KRInspectorRow(property.object("descriptor").string("key").split(separator: ".").last.map(String.init) ?? "Property",
                                                source: property.object("source").string("kind") == "expression" ? .expression : property.object("source").string("kind") == "curve" ? .curve : .constant,
                                                onKeyframe: model.onKeyframe(property), keyframeEditingEnabled: false) {
                                                if model.ui.layout.valuesVisible {
                                                    Text(model.propertyNumbers(layer, property).map { String(format: "%.1f", $0) }.joined(separator: ", "))
                                                        .krText(KRType.timecode).foregroundStyle(p.inkMuted)
                                                }
                                            }.frame(width: column).help("GUI-001 の Dope sheet は読み取り専用です")
                                            lane(model.keyframes(property))
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    func lane(_ keys: [[String: Any]], summary: Bool = false) -> some View {
        GeometryReader { geometry in
            ZStack(alignment: .topLeading) {
                p.surface100
                ForEach(Array(keys.enumerated()), id: \.offset) { index, key in
                    KRKeyframeGlyph(key.object("interpolation").string("kind") == "hold" ? .hold : key.object("interpolation").string("kind") == "cubic" ? .cubic : .linear,
                        selected: model.keyTime(key) == model.ui.time, size: summary ? KRWindowMetrics.handle : KRSize.keyframeSize,
                        action: { model.seekKey(key) })
                        .position(x: model.keyFramePosition(key) / Double(max(1, model.durationFrames)) * geometry.size.width, y: KRSize.rowHeight / 2)
                }
                KRPlayhead().offset(x: model.currentFramePosition / Double(max(1, model.durationFrames)) * geometry.size.width).allowsHitTesting(false)
            }.overlay(alignment: .bottom) { p.line.frame(height: 1) }
                .overlay(alignment: .leading) { p.line.frame(width: 1) }
        }.frame(height: KRSize.rowHeight)
    }
    func toggle(_ id: String) {
        if model.ui.collapsed.contains(id) { model.ui.collapsed.remove(id) } else { model.ui.collapsed.insert(id) }
    }
}
