import SwiftUI
import KronelloDesign
import KronelloAppModel

struct InspectorPanel: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    @Binding var historyOpen: Bool
    @State private var collapsed: Set<String> = []
    var body: some View {
        KRPanel("Inspector") {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if let notice = model.deletedSelection {
                        Text(notice).krText(KRType.body).padding(KRSpace.space3)
                        KRButton("履歴で確認…", variant: .secondary) { historyOpen = true }.padding(KRSpace.space3)
                    } else if let layer = model.selected {
                        HStack(spacing: KRSpace.space2) {
                            KRIconView(layer.designKind.icon).foregroundStyle(p.inkMuted)
                            VStack(alignment: .leading, spacing: KRSpace.space1) {
                                KRTextField("", value: .constant(layer.name), onCommit: { model.rename(layer, name: $0) })
                                Text(layer.kind.capitalized + " · " + (layer.parent.flatMap { id in model.layers.first { $0.id == id }?.name } ?? "Composition"))
                                    .krText(KRType.caption).foregroundStyle(p.inkMuted)
                            }
                        }.padding(KRSpace.space3).disabled(model.ui.locked.contains(layer.id) || model.busy)
                        section("Transform") {
                            property(layer, key: "kronello.transform.position", label: "Position", unit: "px")
                            property(layer, key: "kronello.transform.scale", label: "Scale", unit: "%", multiplier: 100)
                            property(layer, key: "kronello.transform.rotation", label: "Rotation", unit: "°")
                            property(layer, key: "kronello.opacity", label: "Opacity", unit: "%", multiplier: 100)
                        }
                        if layer.kind == "text" {
                            section("Text") {
                                if let text = model.textDocument(layer) {
                                    KRTextField("Text", value: .constant(text.string("text")), onCommit: { model.setText(layer, to: $0) })
                                        .padding(.horizontal, KRSpace.space3).disabled(model.ui.locked.contains(layer.id) || model.busy)
                                    KRPopoverRow("Font") { Text(text.objects("styles").first?.object("font").string("family") ?? "Font").krText(KRType.label).foregroundStyle(p.inkMuted) }
                                        .padding(.horizontal, KRSpace.space3)
                                    KRPopoverRow("Weight") { Text(text.objects("styles").first?.object("font").string("postscript_name") ?? "Font lock").krText(KRType.caption).foregroundStyle(p.inkMuted).lineLimit(1) }
                                        .padding(.horizontal, KRSpace.space3).help("固定 font face。weight の変更は後続タスクです")
                                }
                                property(layer, key: "kronello.text.font_size", label: "Size", unit: "px")
                                property(layer, key: "kronello.text.line_height", label: "Line height", unit: "px")
                                if let alignment = layer.property("kronello.text.alignment") {
                                    KRPopoverRow("Alignment") {
                                        KRPopupButton("文字揃え", options: [.init("start", "Start"), .init("center", "Center"), .init("end", "End")],
                                            selection: Binding(get: { layer.value(alignment).string("value") }, set: { model.setEnum(layer, property: alignment, to: $0) }))
                                            .disabled(model.ui.locked.contains(layer.id) || alignment.object("source").string("kind") != "constant" || model.busy)
                                    }.padding(.horizontal, KRSpace.space3)
                                }
                            }
                        }
                        section("Layout") {
                            if layer.kind == "text" { property(layer, key: "kronello.text.wrap_width", label: "Wrap width", unit: "px") }
                            KRPopoverRow("Bounds") {
                                KRSegmentedControl([.init("layout", "layout"), .init("ink", "ink"), .init("visual", "visual")], selection: $model.ui.bounds)
                            }.padding(.horizontal, KRSpace.space3)
                        }
                    } else {
                        KREmptyState(icon: .mousePointer2, title: "レイヤーを選択", message: "Layers または Viewer でレイヤーを選択してください。")
                    }
                }
            }
        }
    }
    func section<Content: View>(_ name: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            KRButton(name, icon: collapsed.contains(name) ? .chevronRight : .chevronDown, variant: .plain) {
                if collapsed.contains(name) { collapsed.remove(name) } else { collapsed.insert(name) }
            }.padding(.horizontal, KRSpace.space2).padding(.top, KRSpace.space3)
            if !collapsed.contains(name) { content() }
        }
    }
    @ViewBuilder func property(_ layer: Layer, key: String, label: String, unit: String, multiplier: Double = 1) -> some View {
        if let property = model.transformProperty(layer, key: key) {
            let source = property.object("source").string("kind")
            let numbers = model.propertyNumbers(layer, property)
            KRInspectorRow(label, source: source == "curve" ? .curve : source == "expression" ? .expression : .constant,
                onKeyframe: model.onKeyframe(property), error: model.propertyError(layer, property), keyframeEditingEnabled: false,
                previous: source == "curve" ? { model.seekAdjacent(property, forward: false) } : nil,
                next: source == "curve" ? { model.seekAdjacent(property, forward: true) } : nil) {
                HStack(spacing: KRSpace.space1) {
                    if numbers.isEmpty { Text("—").krText(KRType.timecode).foregroundStyle(p.inkMuted) }
                    ForEach(Array(numbers.enumerated()), id: \.offset) { axis, number in
                        if numbers.count > 1 {
                            KRInspectorAxis(axis == 0 ? "X" : "Y") { field(layer, property: property, axis: axis, number: number, unit: unit, multiplier: multiplier) }
                        } else { field(layer, property: property, axis: axis, number: number, unit: unit, multiplier: multiplier) }
                    }
                }.disabled(model.ui.locked.contains(layer.id) || model.busy || model.pendingCandidate != nil)
            }
        } else {
            KRInspectorRow(label, keyframeEditingEnabled: false) { Text("—").krText(KRType.timecode).foregroundStyle(p.inkMuted) }
                .help("このレイヤーに編集可能な Property がありません")
        }
    }
    func field(_ layer: Layer, property: [String: Any], axis: Int, number: Double, unit: String, multiplier: Double) -> some View {
        KRNumberField(value: .constant(number * multiplier), unit: unit, step: multiplier == 100 ? 1 : 1,
            error: model.propertyError(layer, property) != nil, accessibilityLabel: property.object("descriptor").string("key") + " \(axis)",
            onPreview: { model.previewNumber(layer: layer, property: property, axis: axis, to: $0 / multiplier) },
            onCommit: { model.commitNumber(layer: layer, property: property, axis: axis, from: $0 / multiplier, to: $1 / multiplier) })
            .frame(width: KRWindowMetrics.numberWidth)
    }
}
