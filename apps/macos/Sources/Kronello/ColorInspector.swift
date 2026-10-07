import SwiftUI
import KronelloDesign
import KronelloAppModel

/// GUI-010 color-correction editor. Adds, edits and removes COLOR-002 effects
/// through `clip_set_effects`, preserving unrelated effects and animated
/// parameter sources (shown read-only rather than overwritten).
struct ClipColorInspector: View {
    @ObservedObject var model: EditorModel
    let clip: EditClip
    @State private var draftBases: [String: String] = [:]
    @Environment(\.krPalette) var p
    static let names: [String: String] = [
        "kronello.color.exposure": "露出 (Exposure)",
        "kronello.color.levels": "レベル (Levels)",
        "kronello.color.curves": "カーブ (Curves)",
        "kronello.color.hsl": "HSL",
    ]
    /// (parameter field, label, unit, range, step, precision)
    static func rows(_ spec: EditorModel.ColorEffectSpec) -> [(String, String, String, ClosedRange<Double>, Double, Int)] {
        switch spec.kind {
        case "color_exposure":
            return [("exposure", "露出", "EV", -24...24, 0.1, 2), ("offset", "オフセット", "", -4...4, 0.01, 3)]
        case "color_levels":
            return [("in_black", "入力黒", "", 0...1, 0.01, 3), ("in_white", "入力白", "", 0...1, 0.01, 3),
                    ("gamma", "ガンマ", "", 0.01...10, 0.01, 2), ("out_black", "出力黒", "", -1...4, 0.01, 3),
                    ("out_white", "出力白", "", -1...4, 0.01, 3)]
        case "color_hsl":
            return [("hue_shift", "色相", "°", -360...360, 1, 1), ("saturation", "彩度", "×", 0...4, 0.01, 2),
                    ("lightness", "明度", "", -1...1, 0.01, 3)]
        default: return []
        }
    }
    var disabled: Bool { model.trackLocked(clip.track) || model.busy || model.pendingCandidate != nil }
    var body: some View {
        let specs = EditorModel.colorEffectSpecs(on: clip)
        let missing = EditorModel.colorEffects.filter { spec in !specs.contains { $0.spec.effectID == spec.effectID } }
        Group {
            if !missing.isEmpty {
                KRInspectorSettingRow("追加") {
                    KRPopupButton("カラー補正を追加", options: missing.map { .init($0.kind, Self.names[$0.effectID] ?? $0.effectID) },
                        selection: .constant(""), onSelect: { model.addClipEffect($0) })
                        .frame(width: KRWindowMetrics.settingWidth)
                }
            }
            ForEach(Array(specs.enumerated()), id: \.offset) { _, entry in
                VStack(alignment: .leading, spacing: 0) {
                    KRInspectorSettingRow(Self.names[entry.spec.effectID] ?? entry.spec.effectID) {
                        KRButton(icon: .trash2, accessibilityLabel: "\(Self.names[entry.spec.effectID] ?? "エフェクト") を削除") {
                            model.removeClipEffect(clip, index: entry.index)
                        }
                    }
                    if entry.spec.kind == "color_curves" { CurveEditor(model: model, clip: clip, effect: entry.effect) }
                    else {
                        ForEach(Array(Self.rows(entry.spec).enumerated()), id: \.offset) { _, row in
                            let (field, label, unit, range, step, precision) = row
                            let constant = EditorModel.colorParameterIsConstant(clip, effect: entry.effect, parameter: field)
                            KRInspectorSettingRow(label) {
                                KRNumberField(value: .constant(EditorModel.colorParameterValue(clip, effect: entry.effect, parameter: field) ?? 0),
                                    unit: unit, step: step, range: range, precision: precision,
                                    accessibilityLabel: label, onEditingStart: { draftBases[field + entry.spec.effectID] = model.revision },
                                    onCommit: { _, value in
                                        model.setClipEffectParameter(clip, effect: entry.effect, parameter: field,
                                            kind: field == "hue_shift" ? "angle" : "scalar", value: value,
                                            base: draftBases.removeValue(forKey: field + entry.spec.effectID))
                                    }).frame(width: KRWindowMetrics.settingWidth)
                            }.disabled(!constant)
                        }
                        if Self.rows(entry.spec).contains(where: { !EditorModel.colorParameterIsConstant(clip, effect: entry.effect, parameter: $0.0) }) {
                            Text("アニメーション付きパラメータは読み取り専用です").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                        }
                    }
                }
            }
            if specs.isEmpty {
                Text("色補正エフェクトはまだありません").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
            }
        }.disabled(disabled)
    }
}

/// Row-based control-point editor for `color_curves`. Points stay sorted by x;
/// the model requires 2–64 rows with x strictly increasing inside 0...1.
private struct CurveEditor: View {
    @ObservedObject var model: EditorModel
    let clip: EditClip
    let effect: [String: Any]
    @State private var draftBase: String?
    @Environment(\.krPalette) var p
    var points: [(x: Double, y: Double)] { EditorModel.curveRows(clip, effect: effect) ?? [] }
    func commit(_ points: [(x: Double, y: Double)]) {
        guard let table = EditorModel.curveTableValue(points: points) else { return }
        model.setClipEffectParameter(clip, effect: effect, parameter: "curve", kind: "data_table", value: table, base: draftBase)
        draftBase = nil
    }
    var body: some View {
        let constant = EditorModel.colorParameterIsConstant(clip, effect: effect, parameter: "curve")
        if points.isEmpty {
            Text(constant ? "カーブを読み込めません" : "アニメーション付きパラメータは読み取り専用です")
                .krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
        } else {
            ForEach(Array(points.enumerated()), id: \.offset) { index, point in
                KRInspectorSettingRow("点 \(index + 1)") {
                    HStack(spacing: KRSpace.space1) {
                        KRInspectorAxis("X") {
                            KRNumberField(value: .constant(point.x), step: 0.01, range: 0...1, precision: 3,
                                accessibilityLabel: "点 \(index + 1) X", onEditingStart: { if draftBase == nil { draftBase = model.revision } },
                                onCommit: { _, value in
                                    var updated = points; updated[index].x = value
                                    commit(updated)
                                })
                        }
                        KRInspectorAxis("Y") {
                            KRNumberField(value: .constant(point.y), step: 0.01, range: 0...1, precision: 3,
                                accessibilityLabel: "点 \(index + 1) Y", onEditingStart: { if draftBase == nil { draftBase = model.revision } },
                                onCommit: { _, value in
                                    var updated = points; updated[index].y = value
                                    commit(updated)
                                })
                        }
                        KRButton(icon: .trash2, accessibilityLabel: "点 \(index + 1) を削除", size: 20, iconSize: 10) {
                            var updated = points; updated.remove(at: index)
                            draftBase = model.revision
                            commit(updated)
                        }.disabled(points.count <= 2)
                    }.frame(width: KRWindowMetrics.settingWidth)
                }.disabled(!constant)
            }
            KRInspectorSettingRow("") {
                KRButton("点を追加", variant: .secondary) {
                    // Insert at the widest x-gap midpoint.
                    var best = 0, gap = -1.0
                    for index in 0..<points.count - 1 where points[index + 1].x - points[index].x > gap {
                        gap = points[index + 1].x - points[index].x; best = index
                    }
                    var updated = points
                    let x = (points[best].x + points[best + 1].x) / 2, y = (points[best].y + points[best + 1].y) / 2
                    updated.insert((x, y), at: best + 1)
                    draftBase = model.revision
                    commit(updated)
                }.disabled(points.count >= 64 || !constant)
            }
        }
    }
}
