import AppKit
import SwiftUI
import KronelloDesign
import KronelloAppModel

/// GUI-009 cue editor. Every control commits a `caption_set` Event through the
/// shared edit pipeline, so Undo, conflict handling and preview refresh are
/// identical to every other edit surface.
struct CaptionInspector: View {
    @ObservedObject var model: EditorModel
    let clip: EditClip
    @State private var draftBases: [String: String] = [:]
    @Environment(\.krPalette) var p
    var caption: [String: Any] { model.captionDocument(for: clip) ?? [:] }
    var style: [String: Any] { caption.object("style") }
    var placement: [String: Any] { caption.object("placement") }
    var font: [String: Any] { style.object("font") }
    var families: [String] { Array(Set(model.lockedFonts.map { $0.string("family") } + [font.string("family")])).filter { !$0.isEmpty }.sorted() }
    var weights: [[String: Any]] { model.lockedFonts.filter { $0.string("family") == font.string("family") } }
    var disabled: Bool { model.trackLocked(clip.track) || model.busy || model.pendingCandidate != nil }
    static let anchors: [(id: String, label: String)] = [
        ("top_left", "左上"), ("top_center", "上"), ("top_right", "右上"),
        ("center_left", "左"), ("center", "中央"), ("center_right", "右"),
        ("bottom_left", "左下"), ("bottom_center", "下"), ("bottom_right", "右下"),
    ]
    static func hex(_ color: [String: Any]) -> String { PropertyColorEditor.hex(["value": color]) }
    static func swatch(_ color: [String: Any]) -> Color {
        let c = EditorModel.srgbComponents(["value": color])
        return Color(.sRGB, red: min(1, max(0, c[0])), green: min(1, max(0, c[1])), blue: min(1, max(0, c[2])), opacity: c[3])
    }
    var body: some View {
        Group {
            if caption.isEmpty {
                KRErrorLine(.init("SOURCE_MISSING", "字幕ドキュメントが見つかりません")).padding(.horizontal, KRSpace.space3)
            } else {
                KRInspectorSettingRow("本文") {
                    KRTextArea("", value: .constant(caption.string("text")), lines: 2,
                        onEditingStart: { draftBases["text"] = model.revision },
                        onCommit: { model.setCaptionText(clip, to: $0, base: draftBases.removeValue(forKey: "text")) })
                        .frame(width: KRWindowMetrics.settingWidth)
                }
                KRInspectorSettingRow("書体") {
                    KRPopupButton("書体", options: families.map { .init($0, $0) },
                        selection: Binding(get: { font.string("family") }, set: { family in
                            if let selected = model.lockedFonts.first(where: { $0.string("family") == family }) {
                                model.setCaptionFont(clip, font: selected, base: draftBases.removeValue(forKey: "font"))
                            }
                        }), onEditingStart: { draftBases["font"] = model.revision })
                        .frame(width: KRWindowMetrics.settingWidth).disabled(model.lockedFonts.isEmpty)
                }
                KRInspectorSettingRow("ウェイト") {
                    KRPopupButton("ウェイト", options: weights.map { .init($0.string("sha256") + ":" + $0.string("face_index"), InspectorPanel.styleName($0.string("postscript_name"))) },
                        selection: Binding(get: { font.string("sha256") + ":" + font.string("face_index") }, set: { identity in
                            if let selected = weights.first(where: { $0.string("sha256") + ":" + $0.string("face_index") == identity }) {
                                model.setCaptionFont(clip, font: selected, base: draftBases.removeValue(forKey: "weight"))
                            }
                        }), onEditingStart: { draftBases["weight"] = model.revision })
                        .frame(width: KRWindowMetrics.settingWidth).disabled(weights.isEmpty)
                }.help("登録済みフォントのみ選べます")
                KRInspectorSettingRow("サイズ") {
                    KRNumberField(value: .constant(style.number("size")), unit: "px", step: 1, range: 1...1000, precision: 1,
                        accessibilityLabel: "字幕サイズ", onEditingStart: { draftBases["size"] = model.revision },
                        onCommit: { _, value in model.setCaptionSize(clip, size: value, base: draftBases.removeValue(forKey: "size")) })
                        .frame(width: KRWindowMetrics.settingWidth)
                }
                KRInspectorSettingRow("文字色") {
                    HStack(spacing: KRSpace.space1) {
                        Rectangle().fill(Self.swatch(style.object("fill"))).frame(width: 16, height: 16).border(p.lineStrong)
                        KRTextField("", value: .constant(Self.hex(style.object("fill"))),
                            onEditingStart: { draftBases["fill"] = model.revision },
                            onCommit: { model.setCaptionFill(clip, hex: $0, base: draftBases.removeValue(forKey: "fill")) })
                            .frame(width: 100)
                    }
                }.help("#RRGGBB / #RRGGBBAA")
                let outline = style["outline"] as? [String: Any]
                KRInspectorSettingRow("縁取り") {
                    KRCheckbox("", isOn: Binding(get: { outline != nil }, set: { enabled in
                        if enabled { model.setCaptionOutline(clip, width: max(0.5, outline?.number("width") ?? 2)) }
                        else { model.removeCaptionOutline(clip) }
                    })).accessibilityLabel("縁取り")
                }
                if let outline {
                    KRInspectorSettingRow("縁の幅") {
                        KRNumberField(value: .constant(outline.number("width")), unit: "px", step: 0.5, range: 0...100, precision: 1,
                            accessibilityLabel: "縁取りの幅", onEditingStart: { draftBases["outline.width"] = model.revision },
                            onCommit: { _, value in model.setCaptionOutline(clip, width: value, base: draftBases.removeValue(forKey: "outline.width")) })
                            .frame(width: KRWindowMetrics.settingWidth)
                    }
                    KRInspectorSettingRow("縁の色") {
                        HStack(spacing: KRSpace.space1) {
                            Rectangle().fill(Self.swatch(outline.object("color"))).frame(width: 16, height: 16).border(p.lineStrong)
                            KRTextField("", value: .constant(Self.hex(outline.object("color"))),
                                onEditingStart: { draftBases["outline.color"] = model.revision },
                                onCommit: { model.setCaptionOutline(clip, width: outline.number("width"), hex: $0, base: draftBases.removeValue(forKey: "outline.color")) })
                                .frame(width: 100)
                        }
                    }
                }
                let background = style["background"] as? [String: Any]
                KRInspectorSettingRow("背景") {
                    KRCheckbox("", isOn: Binding(get: { background != nil }, set: { enabled in
                        model.setCaptionBackground(clip, hex: enabled ? "#00000080" : nil)
                    })).accessibilityLabel("背景")
                }
                if let background {
                    KRInspectorSettingRow("背景色") {
                        HStack(spacing: KRSpace.space1) {
                            Rectangle().fill(Self.swatch(background)).frame(width: 16, height: 16).border(p.lineStrong)
                            KRTextField("", value: .constant(Self.hex(background)),
                                onEditingStart: { draftBases["background"] = model.revision },
                                onCommit: { model.setCaptionBackground(clip, hex: $0, base: draftBases.removeValue(forKey: "background")) })
                                .frame(width: 100)
                        }
                    }
                }
                KRInspectorSettingRow("配置") {
                    KRPopupButton("配置", options: Self.anchors.map { .init($0.id, $0.label) },
                        selection: Binding(get: { placement.string("anchor") }, set: { model.setCaptionAnchor(clip, anchor: $0) }),
                        onEditingStart: {}).frame(width: KRWindowMetrics.settingWidth)
                }
                let offset = placement["offset"] as? [[String: Any]] ?? []
                KRInspectorSettingRow("オフセット") {
                    HStack(spacing: KRSpace.space1) {
                        KRInspectorAxis("X") {
                            KRNumberField(value: .constant(offset.indices.contains(0) ? EditorModel.captionPercent(offset[0]) : 0), unit: "%", step: 1, range: -100...100, precision: 1,
                                accessibilityLabel: "オフセット X", onEditingStart: { draftBases["offset.x"] = model.revision },
                                onCommit: { _, value in model.setCaptionOffset(clip, axis: 0, percent: value, base: draftBases.removeValue(forKey: "offset.x")) })
                        }
                        KRInspectorAxis("Y") {
                            KRNumberField(value: .constant(offset.indices.contains(1) ? EditorModel.captionPercent(offset[1]) : 0), unit: "%", step: 1, range: -100...100, precision: 1,
                                accessibilityLabel: "オフセット Y", onEditingStart: { draftBases["offset.y"] = model.revision },
                                onCommit: { _, value in model.setCaptionOffset(clip, axis: 1, percent: value, base: draftBases.removeValue(forKey: "offset.y")) })
                        }
                    }.frame(width: KRWindowMetrics.settingWidth)
                }.help("シーケンス幅・高さに対する比率。+X は右、+Y は下")
                let inset = placement["safe_area_inset"] as? [[String: Any]] ?? []
                KRInspectorSettingRow("セーフエリア") {
                    HStack(spacing: KRSpace.space1) {
                        KRInspectorAxis("X") {
                            KRNumberField(value: .constant(inset.indices.contains(0) ? EditorModel.captionPercent(inset[0]) : 0), unit: "%", step: 1, range: 0...50, precision: 1,
                                accessibilityLabel: "セーフエリア X", onEditingStart: { draftBases["inset.x"] = model.revision },
                                onCommit: { _, value in model.setCaptionInset(clip, axis: 0, percent: value, base: draftBases.removeValue(forKey: "inset.x")) })
                        }
                        KRInspectorAxis("Y") {
                            KRNumberField(value: .constant(inset.indices.contains(1) ? EditorModel.captionPercent(inset[1]) : 0), unit: "%", step: 1, range: 0...50, precision: 1,
                                accessibilityLabel: "セーフエリア Y", onEditingStart: { draftBases["inset.y"] = model.revision },
                                onCommit: { _, value in model.setCaptionInset(clip, axis: 1, percent: value, base: draftBases.removeValue(forKey: "inset.y")) })
                        }
                    }.frame(width: KRWindowMetrics.settingWidth)
                }.help("両端からの余白比率 (0〜50%)")
            }
        }.disabled(disabled)
    }
}
