import SwiftUI
import KronelloDesign

@MainActor enum CurveEditorSheets {
    static var all: [(String, AnyView)] {
        [
            ("CurveEditor-value", AnyView(demo.frame(height: 320))),
            ("CurveEditor-velocity", AnyView(graph(velocity: true).frame(height: 280))),
            ("CurveEditor-expression", AnyView(KREmptyState(icon: .slidersHorizontal, title: "式のため編集不可", message: "式で決まる Property に編集用の曲線はありません。").frame(height: 200))),
            ("Keyframe-selection", AnyView(HStack(spacing: KRSpace.space4) {
                ForEach(KRInterpolation.allCases, id: \.self) { mode in
                    KRKeyframeGlyph(mode); KRKeyframeGlyph(mode, selected: true); KRKeyframeGlyph(mode, appearance: .focused)
                }
            })),
            ("KeyframeNavigator-editing", AnyView(VStack(alignment: .leading, spacing: KRSpace.space2) {
                KRInspectorRow("Position", source: .constant) { Text("960 · 720").krText(KRType.timecode) }
                KRInspectorRow("Position", source: .curve, onKeyframe: true, previous: {}, next: {}) { Text("960 · 720").krText(KRType.timecode) }
                KRInspectorRow("Position", source: .curve, previous: {}, next: {}) { Text("960 · 720").krText(KRType.timecode) }
                KRInspectorRow("Position", source: .expression) { Text("960 · 720").krText(KRType.timecode) }
            }))
        ]
    }
    static var demo: some View {
        VStack(spacing: 0) {
            HStack(spacing: KRSpace.space2) {
                KRSegmentedControl([.init("value", "値"), .init("velocity", "速度")], selection: .constant("value"))
                Text("Position").krText(KRType.label)
                KRSegmentedControl([.init("aligned", "揃える"), .init("broken", "分ける")], selection: .constant("aligned"))
                KRButton(icon: .scan, accessibilityLabel: "全体を表示") {}
                Spacer()
                Text("時間イージング").krText(KRType.caption)
            }.padding(.horizontal, KRSpace.space3).frame(height: KRSize.panelHeaderHeight)
            graph()
            Text("X / Y は同じ時間イージングを共有します。どちらの接線編集も両方に反映されます。").krText(KRType.caption)
                .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, KRSpace.space3).frame(height: KRSize.rowHeight)
        }
    }
    static func graph(velocity: Bool = false) -> some View {
        GeometryReader { proxy in
            let width = proxy.size.width
            let points = (0...120).map { i -> CGPoint in let t = Double(i) / 120; return CGPoint(x: t * 120, y: velocity ? 200 * t * (1 - t) : 200 * (3 * t * t - 2 * t * t * t)) }
            KRCurveEditor(channels: [.init("X", points: points, active: true, current: velocity ? "表示のみ" : "100.0 px"), .init("Y", points: points.map { CGPoint(x: $0.x, y: $0.y * 0.7 + 20) }, active: false, current: velocity ? "表示のみ" : "90.0 px")],
                keys: [.init("a", point: .init(x: 0, y: 0), interpolation: .cubic, selected: false, label: "0s0f · 0 px"), .init("b", point: .init(x: 60, y: 100), interpolation: .cubic, selected: true, label: "2s12f · 100 px"), .init("c", point: .init(x: 120, y: 200), interpolation: .hold, selected: false, label: "5s0f · 200 px")],
                tangents: [.init("incoming", origin: .init(x: 60, y: 100), point: .init(x: 40, y: 50)), .init("outgoing", origin: .init(x: 60, y: 100), point: .init(x: 80, y: 150))],
                frames: 0...120, values: velocity ? -10...70 : -20...240,
                ticks: (0...10).map { .init("\($0)", x: KRSpace.space2 + Double($0) / 10 * max(1, width - KRSpace.space2 * 2), label: $0 == 10 ? nil : "\($0 / 2)s\($0 % 2 * 12)f") }, playhead: 60, readOnly: velocity)
        }
    }
}
