import SwiftUI
import KronelloDesign

@MainActor enum GUIStateSheets {
    static var all: [(String, AnyView)] {
        [
            ("Welcome", AnyView(KRWelcome(recent: [
                .init(id: "present", name: "Lower third", location: "/Projects/Lower third", date: "2 時間前", missing: false),
                .init(id: "missing", name: "Teaser", location: "/Volumes/Media", date: "昨日", missing: true)
            ], showAtLaunch: .constant(true), onNew: {}, onOpen: {}, onRecent: { _ in }))),
            ("StateBand", AnyView(KRStateBand("安全モードで開いています（PROJECT_LOCKED）"))),
            ("ConflictBanner", AnyView(KRConflictBanner(.init("REVISION_CONFLICT", "移動を適用できませんでした。最新の状態を読み込みました。"), discard: {}, reapply: {}))),
            ("ViewerError", AnyView(KRViewerError(.init("ADAPTER_UNAVAILABLE", "GPU を利用できません。"), copy: {}, retry: {}).frame(height: 240))),
            ("ManipulationOverlay", AnyView(KRManipulationOverlay(.init(CGRect(x: 0.2, y: 0.2, width: 0.5, height: 0.5), label: "layout 960 × 540"), onPreview: { _, _, _ in }, onCommit: { _, _, _ in }).frame(height: 280))),
            ("UnavailableControls", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                KRSegmentedControl([.init("dope", "Dope sheet"), .init("curve", "Curve editor")], selection: .constant("dope"))
                KRToolStrip([.init("text", icon: .type, name: "テキスト", shortcut: "T", unavailableReason: "FONT_MISSING")], selection: .constant("select"), placement: .constant(.init())).frame(height: 120)
                KRInspectorRow("Position", keyframeEditingEnabled: false) { Text("960").krText(KRType.timecode) }
                KRLayerRow("Title", kind: .text, diagnostic: .init("FONT_MISSING", "指定された font lock を解決できません"))
            }))
        ]
    }
}
