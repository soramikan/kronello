import AppKit
import SwiftUI
import KronelloDesign
import KronelloAppModel

struct TextStyleInspector: View {
    @ObservedObject var model: EditorModel
    let layer: Layer
    @State private var draftBases: [String: String] = [:]
    @State private var first = 1.0
    @State private var last = 1.0
    @Environment(\.krPalette) var p
    var text: [String: Any] { model.textDocument(layer) ?? [:] }
    var count: Int { text.string("text").count }
    var valid: Bool { first >= 1 && last >= first && last <= Double(count) && first.rounded() == first && last.rounded() == last }
    var span: [String: Any] {
        guard let range = try? TextSpanEditing.byteRange(text.string("text"), start: Int(first) - 1, end: Int(last)) else { return [:] }
        return text.objects("styles").first { Int($0.object("range").number("start")) <= range["start"]! && Int($0.object("range").number("end")) > range["start"]! } ?? [:]
    }
    var font: [String: Any] { span.object("font") }
    var families: [String] { Array(Set(model.lockedFonts.map { $0.string("family") } + [font.string("family")])).filter { !$0.isEmpty }.sorted() }
    var weights: [[String: Any]] { model.lockedFonts.filter { $0.string("family") == font.string("family") } }
    var body: some View {
        Group {
            KRInspectorSettingRow("Text") {
                KRTextField("", value: .constant(text.string("text")), onEditingStart: { draftBases["text"] = model.revision }, onCommit: { model.setText(layer, to: $0, base: draftBases.removeValue(forKey: "text")) }).frame(width: KRWindowMetrics.settingWidth)
            }
            KRInspectorSettingRow("文字範囲") {
                HStack {
                    KRNumberField(value: $first, unit: "", accessibilityLabel: "範囲の開始文字", onCommit: { _, v in first = v })
                    Text("–").foregroundStyle(p.inkMuted)
                    KRNumberField(value: $last, unit: "", accessibilityLabel: "範囲の終了文字", onCommit: { _, v in last = v })
                }.frame(width: KRWindowMetrics.settingWidth)
            }.help("1 から始まる書記素単位の範囲です。結合濁点・IVS を分割しません")
            if !valid && count > 0 { KRErrorLine(.init("INVALID_TEXT_RANGE", "1…\(count) の文字範囲を指定してください")).padding(.horizontal, KRSpace.space3) }
            KRInspectorSettingRow("Font") {
                KRPopupButton("書体", options: families.map { .init($0, $0) }, selection: Binding(get: { font.string("family") }, set: { family in
                    if let selected = model.lockedFonts.first(where: { $0.string("family") == family }) { model.setSpanFont(layer, start: Int(first) - 1, end: Int(last), font: selected, base: draftBases.removeValue(forKey: "font")) }
                }), onEditingStart: { draftBases["font"] = model.revision }).frame(width: KRWindowMetrics.settingWidth).disabled(!valid || model.lockedFonts.isEmpty)
            }
            KRInspectorSettingRow("Weight") {
                KRPopupButton("ウェイト", options: weights.map { .init($0.string("sha256") + ":" + $0.string("face_index"), InspectorPanel.styleName($0.string("postscript_name"))) }, selection: Binding(get: { font.string("sha256") + ":" + font.string("face_index") }, set: { identity in
                    if let selected = weights.first(where: { $0.string("sha256") + ":" + $0.string("face_index") == identity }) { model.setSpanFont(layer, start: Int(first) - 1, end: Int(last), font: selected, base: draftBases.removeValue(forKey: "weight")) }
                }), onEditingStart: { draftBases["weight"] = model.revision }).frame(width: KRWindowMetrics.settingWidth).disabled(!valid || weights.isEmpty)
            }.help("読み込んだ書体のウェイトを選びます")
            KRInspectorSettingRow("書体ファイル") {
                KRButton("追加…", variant: .secondary) {
                    let panel = NSOpenPanel(); panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
                    panel.title = "固定して使うフォントファイルを選択"
                    if panel.runModal() == .OK, let url = panel.url { Task { await model.importFont(path: url.path) } }
                }
            }
            if let size = layer.properties.first(where: { $0.string("id") == span.string("size") }) {
                KRInspectorSettingRow("範囲の Size") {
                    KRNumberField(value: .constant(layer.value(size).number("value")), unit: "px", accessibilityLabel: "文字範囲のサイズ", onEditingStart: { draftBases["size"] = model.revision }, onCommit: { _, value in model.setSpanSize(layer, start: Int(first) - 1, end: Int(last), size: value, base: draftBases.removeValue(forKey: "size")) }).frame(width: KRWindowMetrics.settingWidth).disabled(!valid)
                }
            }
            if let fill = layer.properties.first(where: { $0.string("id") == span.string("fill") }) {
                KRInspectorSettingRow("範囲の Color") {
                    KRTextField("", value: .constant(PropertyColorEditor.hex(layer.value(fill))), onEditingStart: { draftBases["color"] = model.revision }, onCommit: { model.setSpanColor(layer, start: Int(first) - 1, end: Int(last), hex: $0, base: draftBases.removeValue(forKey: "color")) }).frame(width: KRWindowMetrics.settingWidth).disabled(!valid)
                }.help("#RRGGBB / #RRGGBBAA。選択した文字の色を変更します")
            }
            Text("選択した文字の書式を変更します").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
        }.disabled(model.ui.locked.contains(layer.id) || model.busy || model.pendingCandidate != nil)
            .onAppear { first = 1; last = Double(max(1, count)) }
            .onChange(of: layer.id) { _, _ in draftBases.removeAll(); first = 1; last = Double(max(1, count)) }
            .onChange(of: count) { _, value in first = min(first, Double(max(1, value))); last = min(last, Double(max(1, value))) }
    }
}

@MainActor private final class DraftColorPanel: NSObject, ObservableObject {
    private var onChange: ((NSColor) -> Void)?
    private var active = false
    func show(_ color: NSColor, onChange: @escaping (NSColor) -> Void) {
        self.onChange = onChange; active = true
        let panel = NSColorPanel.shared
        panel.worksWhenModal = true; panel.showsAlpha = true; panel.isContinuous = true
        panel.color = color
        panel.setTarget(self); panel.setAction(#selector(changed(_:)))
        panel.makeKeyAndOrderFront(nil)
    }
    @objc private func changed(_ panel: NSColorPanel) { onChange?(panel.color) }
    func close() {
        guard active else { return }
        active = false; onChange = nil
        NSColorPanel.shared.orderOut(nil)
        NSColorPanel.shared.setTarget(nil); NSColorPanel.shared.setAction(nil)
    }
}

struct PropertyColorEditor: View {
    @ObservedObject var model: EditorModel
    let layer: Layer
    let property: [String: Any]
    @State private var open = false
    @State private var draft = Color.white
    @StateObject private var panel = DraftColorPanel()
    @State private var base = ""
    @State private var time = RationalTime(num: 0, den: 1)
    @Environment(\.krPalette) var p
    static func hex(_ value: [String: Any]) -> String {
        let bytes = EditorModel.srgbComponents(value).map { Int((min(1, max(0, $0)) * 255).rounded()) }
        return String(format: "#%02X%02X%02X%02X", bytes[0], bytes[1], bytes[2], bytes[3])
    }
    var value: [String: Any] { layer.value(property) }
    var body: some View {
        HStack(spacing: KRSpace.space2) {
            KRButton(icon: .circle, accessibilityLabel: "色を選択") {
                let c = EditorModel.srgbComponents(value)
                draft = Color(.sRGB, red: min(1, max(0, c[0])), green: min(1, max(0, c[1])), blue: min(1, max(0, c[2])), opacity: c[3])
                base = model.revision; time = model.ui.time; open = true
            }.sheet(isPresented: $open) {
                VStack(spacing: KRSpace.space3) {
                    HStack {
                        Rectangle().fill(draft).frame(width: 48, height: 24).border(p.line)
                        KRButton("色を選択…", variant: .secondary) {
                            panel.show(NSColor(draft)) { draft = Color(nsColor: $0) }
                        }
                    }
                    Text("sRGB Color").krText(KRType.caption)
                    HStack {
                        KRButton("キャンセル", variant: .secondary) { panel.close(); open = false }
                        KRButton("適用", variant: .secondary) {
                            guard let color = NSColor(draft).usingColorSpace(.sRGB) else { return }
                            let hex = String(format: "#%02X%02X%02X%02X", Int((color.redComponent * 255).rounded()), Int((color.greenComponent * 255).rounded()), Int((color.blueComponent * 255).rounded()), Int((color.alphaComponent * 255).rounded()))
                            panel.close(); model.setColor(layer, property: property, hex: hex, base: base, time: time); open = false
                        }
                    }
                }.padding(KRSpace.space4).frame(width: 240).onDisappear { panel.close() }
            }
            KRTextField("", value: .constant(Self.hex(value)), onEditingStart: { base = model.revision; time = model.ui.time }, onCommit: { model.setColor(layer, property: property, hex: $0, base: base.isEmpty ? nil : base, time: time) }).frame(width: 108)
        }.disabled(property.object("source").string("kind") == "expression")
            .help("sRGB 色を設定。Curve は現在時刻に一つの key を更新し、Expression は直接編集しません")
    }
}

/// Color editor for a constant clip-effect parameter (FX-005 `key_color`).
/// Non-constant sources stay read-only; edits commit through
/// `setClipEffectParameter` so the undo label and revision base stay uniform.
struct ClipEffectColorEditor: View {
    @ObservedObject var model: EditorModel
    let clip: EditClip
    let effect: [String: Any]
    let parameter: String
    @State private var open = false
    @State private var draft = Color.white
    @StateObject private var panel = DraftColorPanel()
    @State private var base: String?
    @Environment(\.krPalette) var p
    var components: [Double] { EditorModel.colorParameterColor(clip, effect: effect, parameter: parameter) ?? [0, 0, 0, 1] }
    var hex: String {
        let bytes = components.map { Int((min(1, max(0, $0)) * 255).rounded()) }
        return String(format: "#%02X%02X%02X%02X", bytes[0], bytes[1], bytes[2], bytes[3])
    }
    var body: some View {
        HStack(spacing: KRSpace.space2) {
            KRButton(icon: .circle, accessibilityLabel: "色を選択") {
                let c = components
                draft = Color(.sRGB, red: min(1, max(0, c[0])), green: min(1, max(0, c[1])), blue: min(1, max(0, c[2])), opacity: c[3])
                base = model.revision; open = true
            }.sheet(isPresented: $open) {
                VStack(spacing: KRSpace.space3) {
                    HStack {
                        Rectangle().fill(draft).frame(width: 48, height: 24).border(p.line)
                        KRButton("色を選択…", variant: .secondary) {
                            panel.show(NSColor(draft)) { draft = Color(nsColor: $0) }
                        }
                    }
                    Text("sRGB Color").krText(KRType.caption)
                    HStack {
                        KRButton("キャンセル", variant: .secondary) { panel.close(); open = false }
                        KRButton("適用", variant: .secondary) {
                            guard let color = NSColor(draft).usingColorSpace(.sRGB) else { return }
                            let hex = String(format: "#%02X%02X%02X%02X", Int((color.redComponent * 255).rounded()), Int((color.greenComponent * 255).rounded()), Int((color.blueComponent * 255).rounded()), Int((color.alphaComponent * 255).rounded()))
                            panel.close(); commit(hex); open = false
                        }
                    }
                }.padding(KRSpace.space4).frame(width: 240).onDisappear { panel.close() }
            }
            KRTextField("", value: .constant(hex), onEditingStart: { base = model.revision }, onCommit: { commit($0) }).frame(width: 108)
        }.disabled(!EditorModel.colorParameterIsConstant(clip, effect: effect, parameter: parameter))
            .help("sRGB 色を設定。アニメーション付きパラメータは読み取り専用です")
    }
    func commit(_ hex: String) {
        guard let wrapped = try? EditorModel.colorValue(hex: hex), let value = wrapped["value"] else { return }
        model.setClipEffectParameter(clip, effect: effect, parameter: parameter, kind: "color", value: value, base: base)
        base = nil
    }
}
