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
                                TextStyleInspector(model: model, layer: layer)
                                property(layer, key: "kronello.text.line_height", label: "Line height", unit: "px")
                                if let alignment = layer.property("kronello.text.alignment") {
                                    KRInspectorSettingRow("Alignment") {
                                        KRPopupButton("文字揃え", options: [.init("start", "Start"), .init("center", "Center"), .init("end", "End")],
                                            selection: Binding(get: { layer.value(alignment).string("value") }, set: { model.setEnum(layer, property: alignment, to: $0) }))
                                            .frame(width: KRWindowMetrics.settingWidth)
                                            .disabled(model.ui.locked.contains(layer.id) || alignment.object("source").string("kind") != "constant" || model.busy)
                                    }
                                }
                            }
                        }
                        section("Matte") {
                            let relation = model.matteRelation(layer)
                            KRInspectorSettingRow("Matte layer") {
                                KRPopupButton("Matte layer", options: [.init("none", "なし")] + model.matteCandidates.filter { $0.string("id") != layer.id }.map { .init($0.string("id"), $0["name"] as? String ?? $0.string("id")) },
                                    selection: Binding(get: { relation?.string("matte") ?? "none" }, set: { model.setMatte(layer, target: $0) })).frame(width: KRWindowMetrics.settingWidth)
                            }
                            if let relation {
                                KRInspectorSettingRow("Mask") {
                                    KRPopupButton("Mask", options: [.init("alpha", "Alpha"), .init("luminance", "Luminance")], selection: Binding(get: { relation.string("kind") }, set: { model.setMatte(layer, kind: $0) })).frame(width: KRWindowMetrics.settingWidth)
                                }
                                KRInspectorSettingRow("Invert") {
                                    KRCheckbox("反転", isOn: Binding(get: { relation["invert"] as? Bool ?? false }, set: { model.setMatte(layer, invert: $0) }))
                                }
                                KRInspectorSettingRow("Show matte") {
                                    KRCheckbox("Matte layer を表示", isOn: Binding(get: { relation["visible"] as? Bool ?? false }, set: { model.setMatte(layer, visible: $0) }))
                                }
                            }
                        }.disabled(model.ui.locked.contains(layer.id) || model.busy)
                        section("Layout") {
                            if layer.kind == "text" { property(layer, key: "kronello.text.wrap_width", label: "Wrap width", unit: "px") }
                            KRInspectorSettingRow("Bounds") {
                                KRSegmentedControl([.init("layout", "layout"), .init("ink", "ink"), .init("visual", "visual")], selection: $model.ui.bounds).fixedSize()
                            }
                        }
                        if TemplateInstanceInspection.instance(layer, document: model.document) != nil {
                            TemplateInstanceInspectionPanel(model: model)
                        }
                    } else {
                        KREmptyState(icon: .mousePointer2, title: "レイヤーを選択", message: "Layers または Viewer でレイヤーを選択してください。")
                    }
                }
            }
        }
    }
    /// Style suffix of a PostScript name ("NotoSansCJKjp-Bold" -> "Bold"); "Regular" when absent.
    static func styleName(_ postscript: String) -> String {
        guard let dash = postscript.lastIndex(of: "-") else { return "Regular" }
        let style = postscript[postscript.index(after: dash)...]
        return style.isEmpty ? "Regular" : String(style)
    }
    func section<Content: View>(_ name: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            KRButton(name, icon: collapsed.contains(name) ? .chevronRight : .chevronDown, variant: .plain) {
                if collapsed.contains(name) { collapsed.remove(name) } else { collapsed.insert(name) }
            }.padding(.horizontal, KRSpace.space2).padding(.top, KRSpace.space3)
            if !collapsed.contains(name) { content() }
        }
    }
    @ViewBuilder func property(_ layer: Layer, key: String, label: String, unit: String = "", multiplier: Double = 1) -> some View {
        if let property = model.transformProperty(layer, key: key) {
            let source = KRPropertySource(property)
            KRInspectorRow(label, source: source,
                onKeyframe: model.onKeyframe(property), error: model.propertyError(layer, property) ?? (source == .expression ? nil : model.expressionError(layer, property)), keyframeEditingEnabled: !model.ui.locked.contains(layer.id) && !model.busy,
                previous: source == .curve ? { model.seekAdjacent(property, forward: false) } : nil,
                toggleKeyframe: { model.toggleKeyframe(layer, property: property) },
                next: source == .curve ? { model.seekAdjacent(property, forward: true) } : nil) {
                PropertyValue(model: model, layer: layer, property: property)
            }
            if source == .expression {
                ExpressionField(model: model, layer: layer, property: property)
                    .padding(.leading, 48 + KRSpace.space2 * 2)
                    .padding(.trailing, KRSpace.space3)
                    .padding(.bottom, KRSpace.space1)
                    .disabled(model.ui.locked.contains(layer.id) || model.busy || model.pendingCandidate != nil)
            }
        } else {
            KRInspectorRow(label, keyframeEditingEnabled: false) { Text("—").krText(KRType.timecode).foregroundStyle(p.inkMuted) }
                .help("このレイヤーに編集可能な Property がありません")
        }
    }
}
