import KronelloDesign
import SwiftUI

/// Static, explicitly illustrative data assembled with the public component library.
struct MotionScreen: View {
    var curveEditor = false
    @Environment(\.krPalette) var p
    var body: some View {
        VStack(spacing: 0) {
            toolbar.frame(height: 44)
            HStack(spacing: 0) {
                layers.frame(width: 248)
                viewer
                inspector.frame(width: 304)
            }.frame(height: 488)
            Group { if curveEditor { curveSheet } else { dopeSheet } }.frame(height: 344)
            KRStatusBar(revision: "rev 131", externalChange: "MCP の変更を読み込みました（rev 130 → 131）", job: .init("書き出し 42%", progress: 0.42))
        }.frame(width: 1440, height: 900).background(p.surface100).foregroundStyle(p.ink)
    }
    var toolbar: some View {
        HStack(spacing: KRSpace.space4) {
            VStack(alignment: .leading, spacing: 0) {
                Text("Lower third · Kronello").krText(KRType.heading)
                Text("Design gallery / illustrative project").krText(KRType.caption).foregroundStyle(p.inkMuted)
            }.frame(width: 340, alignment: .leading)
            Spacer()
            KRSegmentedControl([.init("edit", "編集"), .init("motion", "モーション"), .init("template", "テンプレート"), .init("export", "書き出し")], selection: .constant("motion"))
            Spacer()
            KRButton(icon: .undo2, accessibilityLabel: "取り消す") {}
            KRButton(icon: .redo2, accessibilityLabel: "やり直す") {}.disabled(true)
            KRPopupButton("ワークスペース", options: [.init("standard", "標準", icon: .layoutPanelLeft)], selection: .constant("standard")).frame(width: 144)
        }.padding(.horizontal, KRSpace.space3).background(p.surface100)
            .overlay(alignment: .bottom) { p.line.frame(height: 1) }
    }
    var layers: some View {
        KRPanel(header: { KRTabBar([.init("layers", "Layers"), .init("project", "Project")], selection: .constant("layers")) }, actions: {
            KRButton(icon: .ellipsis, accessibilityLabel: "Layers メニュー") {}
        }) {
            VStack(spacing: 0) {
                KRSearchField("レイヤーを検索", text: .constant("")).padding(KRSpace.space2)
                KRLayerRow("Title group", kind: .group, hasChildren: true, expanded: true)
                KRLayerRow("見出し", kind: .text, level: 1, selected: true)
                KRLayerRow("背景帯", kind: .shape, level: 1)
                KRLayerRow("Anchor", kind: .null, hidden: true)
                KRLayerRow("Background", kind: .media(.image), locked: true)
                Spacer(minLength: 0)
            }
        }
    }
    var viewer: some View {
        KRPanel(header: { KRTabBar([.init("lower", "Lower third", closable: true)], selection: .constant("lower")) }, actions: {
            KRButton(icon: .magnet, accessibilityLabel: "スナップ", pressed: true) {}
            KRButton(icon: .grid3x3, accessibilityLabel: "ガイド") {}
        }) {
            VStack(spacing: 0) {
                HStack(alignment: .top, spacing: 0) {
                    KRToolStrip(KRTool.motion, selection: .constant("select"), placement: .constant(.init()))
                    KRViewerFrame(selection: .init(CGRect(x: 0.16, y: 0.62, width: 0.68, height: 0.2), label: "layout 1200 × 162")) {
                        VStack(alignment: .leading, spacing: KRSpace.space2) {
                            Spacer()
                            Text("いま、時間を編む。").krText(KRType.display).foregroundStyle(p.ink)
                                .padding(KRSpace.space4).frame(maxWidth: .infinity, alignment: .leading).background(p.surface100)
                            Spacer().frame(height: KRSpace.space4 * 3)
                        }.padding(.horizontal, KRSpace.space4 * 4)
                    }.padding(KRSpace.space4)
                }.frame(maxHeight: .infinity).background(p.surface0)
                KRTransportBar(frames: .constant(45), fps: 24, duration: "00:00:14:00", playing: .constant(false), looping: .constant(true), zoom: .constant("fit"), resolution: .constant("full"))
            }
        }
    }
    var inspector: some View {
        KRPanel("Inspector", actions: { KRButton(icon: .ellipsis, accessibilityLabel: "Inspector メニュー") {} }) {
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: KRSpace.space2) {
                    KRIconView(.type).foregroundStyle(p.inkMuted)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("見出し").krText(KRType.body)
                        Text("Text · Title group").krText(KRType.caption).foregroundStyle(p.inkMuted)
                    }
                }.padding(KRSpace.space3)
                heading("Transform")
                KRInspectorRow("Position", source: .curve, onKeyframe: true, previous: {}, next: {}) {
                    HStack(spacing: KRSpace.space1) {
                        KRInspectorAxis("X") { field(960, unit: "", width: 64) }
                        KRInspectorAxis("Y") { field(720, unit: "", width: 64) }
                    }
                }
                KRInspectorRow("Scale", source: .constant) { field(100, unit: "%") }
                KRInspectorRow("Rotation", source: .constant) { field(0, unit: "°") }
                KRInspectorRow("Opacity", source: .curve, previous: {}, next: {}) { field(100, unit: "%") }
                heading("Text")
                KRInspectorSettingRow("Font") { KRPopupButton("書体", options: [.init("noto", "Noto Sans JP")], selection: .constant("noto")).frame(width: 144) }
                KRInspectorSettingRow("Weight") { KRPopupButton("太さ", options: [.init("600", "Semibold")], selection: .constant("600")).frame(width: 144) }
                KRInspectorRow("Size") { field(64, unit: "px") }
                heading("Layout")
                KRInspectorRow("Wrap width") { field(1200, unit: "px") }
                KRInspectorSettingRow("Bounds") { KRSegmentedControl([.init("layout", "layout"), .init("ink", "ink"), .init("visual", "visual")], selection: .constant("layout")).fixedSize() }
                Spacer(minLength: 0)
            }
        }
    }
    func heading(_ title: String) -> some View {
        Text(title).krText(KRType.heading).padding(.horizontal, KRSpace.space3).padding(.top, KRSpace.space3).padding(.bottom, KRSpace.space1)
    }
    func field(_ value: Double, unit: String, width: CGFloat = 88) -> some View {
        KRNumberField(value: .constant(value), unit: unit, accessibilityLabel: "値", onCommit: { _, _ in }).frame(width: width)
    }
    var curveSheet: some View {
        KRPanel(header: {
            HStack(spacing: KRSpace.space3) {
                KRSegmentedControl([.init("dope", "Dope sheet"), .init("curve", "Curve editor")], selection: .constant("curve"))
                Text("00:00:02:12").krText(KRType.timecode).foregroundStyle(p.accentInk)
                KRPopupButton("補間", options: [.init("cubic", "Cubic")], selection: .constant("cubic")).frame(width: 96)
            }.padding(.leading, KRSpace.space3)
        }, actions: {
            KRPopupButton("時間軸の拡大", options: [.init("1", "100%")], selection: .constant("1")).frame(width: 96)
            KRButton(icon: .magnet, accessibilityLabel: "キーフレームにスナップ", pressed: true) {}
        }) {
            HStack(spacing: 0) {
                VStack(alignment: .leading, spacing: 0) {
                    Text("チャンネル").krText(KRType.heading).padding(KRSpace.space3)
                    Text("見出し").krText(KRType.label).padding(.leading, KRSpace.space2).frame(height: KRSize.rowHeight)
                    KRButton("Position", icon: .chevronDown, variant: .plain) {}.padding(.leading, KRSpace.space3)
                    ForEach(["X", "Y"], id: \.self) { channel in
                        HStack { Text(channel).krText(KRType.label); Text(channel == "X" ? "100.0 px" : "90.0 px").krText(KRType.ruler); Spacer(); KRButton(icon: .eye, accessibilityLabel: channel + " の表示") {} }
                            .padding(.horizontal, KRSpace.space4).frame(height: KRSize.rowHeight).background(channel == "X" ? p.selectionBg : .clear)
                    }
                    Spacer()
                }.frame(width: 216).overlay(alignment: .trailing) { p.line.frame(width: 1) }
                CurveEditorSheets.demo
            }
        }
    }
    var dopeSheet: some View {
        KRPanel(header: {
            HStack(spacing: KRSpace.space3) {
                KRSegmentedControl([.init("dope", "Dope sheet"), .init("curve", "Curve editor")], selection: .constant("dope"))
                Text("00:00:01:21").krText(KRType.timecode).foregroundStyle(p.accentInk)
                KRPopupButton("補間", options: [.init("cubic", "Cubic")], selection: .constant("cubic")).frame(width: 96)
            }.padding(.leading, KRSpace.space3)
        }, actions: {
            KRButton(icon: .magnet, accessibilityLabel: "キーフレームにスナップ", pressed: true) {}
            KRPopupButton("時間軸の拡大", options: [.init("1", "100%")], selection: .constant("1")).frame(width: 96)
        }) {
            HStack(alignment: .top, spacing: 0) {
                VStack(spacing: 0) {
                    HStack(spacing: KRSpace.space2) {
                        KRSearchField("Property を検索", text: .constant(""))
                        KRButton(icon: .chevronsLeft, accessibilityLabel: "値の列を畳む") {}
                    }.padding(.horizontal, KRSpace.space2).frame(height: KRSize.rowHeight)
                    KRLayerRow("見出し", kind: .text, hasChildren: true, expanded: true, selected: true)
                    KRInspectorRow("Position", source: .curve, onKeyframe: true, previous: {}, next: {}) {
                        HStack(spacing: KRSpace.space1) {
                            KRInspectorAxis("X") { field(960, unit: "", width: 64) }
                            KRInspectorAxis("Y") { field(720, unit: "", width: 64) }
                        }
                    }
                    dopeProperty("Scale", 100, unit: "%")
                    dopeProperty("Opacity", 100, unit: "%")
                    KRLayerRow("背景帯", kind: .shape, hasChildren: true, expanded: true)
                    dopeProperty("Scale", 100, unit: "%")
                    dopeProperty("Opacity", 100, unit: "%")
                    Spacer(minLength: 0)
                }.frame(width: 376).overlay(alignment: .trailing) { p.line.frame(width: 1) }
                VStack(spacing: 0) {
                    GeometryReader { proxy in KRRuler((0...10).map { .init("t\($0)", x: KRSpace.space2 + Double($0) / 10 * max(1, proxy.size.width - KRSpace.space2 * 2), label: $0 == 10 ? nil : "\($0)s") }) }.frame(height: KRSize.rowHeight)
                    ForEach(0..<7) { row in
                        GeometryReader { geometry in
                            HStack(spacing: 0) {
                                ForEach(0..<10) { _ in Spacer(minLength: 0); p.line.frame(width: 1) }
                            }
                            ForEach(0..<4) { index in
                                KRKeyframeGlyph(row == 2 ? .hold : row == 3 ? .linear : .cubic, selected: row == 1 && index == 1,
                                                size: row == 0 || row == 4 ? 7 : KRSize.keyframeSize, action: {})
                                    .position(x: CGFloat([24, 180, 460, 700][index]) + CGFloat(row % 2) * 12, y: geometry.size.height / 2)
                            }
                        }.frame(height: KRSize.rowHeight).background(row == 0 ? p.selectionBg : p.surface100)
                            .overlay(alignment: .bottom) { p.line.frame(height: 1) }
                    }
                    Spacer(minLength: 0)
                }.overlay(alignment: .leading) { KRPlayhead().offset(x: 180) }
            }
        }
    }
    func dopeProperty(_ title: String, _ value: Double, unit: String, on: Bool = false) -> some View {
        KRInspectorRow(title, source: .curve, onKeyframe: on, previous: {}, next: {}) { field(value, unit: unit) }
    }
}
