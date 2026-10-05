import SwiftUI
import KronelloDesign
import KronelloAppModel

@MainActor enum CurveEditorSheets {
    static var all: [(String, AnyView)] {
        [
            ("CurveEditor-value", AnyView(demo.frame(height: 320))),
            ("CurveEditor-velocity", AnyView(CurveEditorSheet(velocity: true).frame(height: 320))),
            ("CurveEditor-readout-edges", AnyView(VStack(spacing: KRSpace.space4) {
                graph(playhead: 0).frame(height: 220)
                graph(playhead: 120).frame(height: 220)
            })),
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
    static var demo: some View { CurveEditorSheet(velocity: false) }
    // Smoothstep Position over five seconds: at 2s12f, X/Y = 100/90 px
    // and dX/dt, dY/dt = 60/42 px/s. Y's constant offset has no velocity.
    static let sampleKeys: [[String: Any]] = [
        ["value": ["value": [0.0, 20.0]], "interpolation": ["kind": "cubic", "value": ["control1": [1.0 / 3, 0.0], "control2": [2.0 / 3, 1.0]]]],
        ["value": ["value": [200.0, 160.0]]]
    ]
    static func sample(_ frame: Double, velocity: Bool) -> [Double] {
        velocity ? CurveDisplay.velocity(sampleKeys, frame: frame, positions: [0, 120], framesPerSecond: 24)
            : CurveDisplay.sample(sampleKeys, frame: frame, positions: [0, 120])
    }
    static func graph(velocity: Bool = false, playhead: Double = 60) -> some View {
        GeometryReader { proxy in
            let width = proxy.size.width
            let samples = (0...120).map { sample(Double($0), velocity: velocity) }
            let now = sample(playhead, velocity: velocity)
            let presentation = PropertyPresentation.of("kronello.transform.position")
            KRCurveEditor(channels: (0..<2).map { axis in
                .init(axis == 0 ? "X" : "Y", points: samples.enumerated().map { CGPoint(x: Double($0.offset), y: $0.element[axis]) }, active: axis == 0, current: presentation.curveReadout(now[axis], velocity: velocity))
            },
                keys: [.init("a", point: .init(x: 0, y: 0), interpolation: .cubic, selected: false, label: "0s0f · 0 px"), .init("b", point: .init(x: 60, y: 100), interpolation: .cubic, selected: true, label: "2s12f · 100 px"), .init("c", point: .init(x: 120, y: 200), interpolation: .hold, selected: false, label: "5s0f · 200 px")],
                tangents: [.init("incoming", origin: .init(x: 60, y: 100), point: .init(x: 40, y: 50)), .init("outgoing", origin: .init(x: 60, y: 100), point: .init(x: 80, y: 150))],
                frames: 0...120, values: velocity ? -10...70 : -20...240,
                ticks: (0...10).map { .init("\($0)", x: KRSpace.space2 + Double($0) / 10 * max(1, width - KRSpace.space2 * 2), label: $0 == 10 ? nil : "\($0 / 2)s\($0 % 2 * 12)f") }, playhead: playhead, readOnly: velocity)
        }
    }
}

private struct CurveEditorSheet: View {
    @Environment(\.krPalette) var p
    let velocity: Bool
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: KRSpace.space2) {
                KRSegmentedControl([.init("value", "値"), .init("velocity", "速度")], selection: .constant(velocity ? "velocity" : "value"))
                Text("Position").krText(KRType.label)
                KRSegmentedControl([.init("aligned", "揃える"), .init("broken", "分ける")], selection: .constant("aligned")).disabled(velocity)
                KRButton(icon: .scan, accessibilityLabel: "全体を表示") {}
                Spacer()
                Text(velocity ? "速度は表示のみ" : "時間イージング").krText(KRType.caption).foregroundStyle(p.inkMuted)
            }.padding(.horizontal, KRSpace.space3).frame(height: KRSize.panelHeaderHeight)
            CurveEditorSheets.graph(velocity: velocity)
            VStack(alignment: .leading, spacing: KRSpace.space1) {
                Text("X / Y は同じ時間イージングを共有します。どちらの接線編集も両方に反映されます。")
                if velocity { Text("速度は表示のみです。キーと接線は値グラフで編集します。") }
            }.krText(KRType.caption).foregroundStyle(p.inkMuted)
                .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, KRSpace.space3)
                .frame(height: velocity ? KRSize.rowHeight * 2 : KRSize.rowHeight).background(p.surface100)
        }
    }
}
