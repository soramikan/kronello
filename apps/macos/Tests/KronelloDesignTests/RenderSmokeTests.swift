import AppKit
import KronelloDesign
import SwiftUI
import XCTest

final class RenderSmokeTests: XCTestCase {
    func testEveryComponentRendersInBothThemes() async throws {
        try await MainActor.run {
            KRFonts.registerBundled()
            for theme in KRTheme.allCases {
                for (name, view) in samples {
                    let renderer = ImageRenderer(content: view.frame(width: 700, height: 300).padding(KRSpace.space4)
                        .background(KRPalette.of(theme).surface100).environment(\.krFreezeActivity, true)
                        .environment(\.krStaticRendering, true).krTheme(theme))
                    renderer.scale = 2
                    let image = try XCTUnwrap(renderer.cgImage, "\(name)-\(theme.rawValue)")
                    XCTAssertEqual(image.width, 1464, name)
                    XCTAssertEqual(image.height, 664, name)
                }
            }
        }
    }

    func testViewerFitsBothContainerDimensions() {
        for ratio: CGFloat in [16 / 9, 9 / 16, 1, 4 / 3] {
            for container in [CGSize(width: 100, height: 600), CGSize(width: 800, height: 100), .zero] {
                let size = KRViewerFrame<EmptyView>.fittingSize(container: container, aspectRatio: ratio)
                XCTAssertLessThanOrEqual(size.width, container.width)
                XCTAssertLessThanOrEqual(size.height, container.height)
                XCTAssertEqual(size.width, size.height * ratio, accuracy: 0.0001)
            }
        }
    }

    @MainActor private var samples: [(String, AnyView)] {
        let number = KRNumberField(value: .constant(960), unit: "px", accessibilityLabel: "X 位置", onCommit: { _, _ in }).frame(width: 96)
        return [
            ("Button", AnyView(KRButton("書き出す", variant: .primary) {})),
            ("SegmentedControl", AnyView(KRSegmentedControl([.init("one", "値"), .init("two", "速度")], selection: .constant("one")))),
            ("PopupButton", AnyView(KRPopupButton("解像度", options: [.init("full", "Full")], selection: .constant("full")).frame(width: 120))),
            ("Menu", AnyView(KRMenu([.init("copy", "コピー", icon: .copy, shortcut: "⌘C")], current: "copy").frame(width: 240))),
            ("NumberField", AnyView(number)),
            ("TextField", AnyView(KRTextField("名前", value: .constant("見出し")).frame(width: 240))),
            ("SearchField", AnyView(KRSearchField(text: .constant("")).frame(width: 240))),
            ("Checkbox", AnyView(KRCheckbox("表示する", isOn: .constant(true)))),
            ("Radio", AnyView(KRRadio("Full", id: "full", selection: .constant("full")))),
            ("Slider", AnyView(KRSlider(value: .constant(50), range: 0...100, accessibilityLabel: "不透明度", onCommit: { _, _ in }).frame(width: 240))),
            ("KeyframeGlyph", AnyView(KRKeyframeGlyph(.cubic, selected: true, action: {}))),
            ("KeyframeNavigator", AnyView(KRKeyframeNavigator(source: .curve, onKeyframe: true, previous: {}, next: {}))),
            ("InspectorRow", AnyView(KRInspectorRow("Position", source: .curve) {
                HStack(spacing: KRSpace.space1) {
                    KRInspectorAxis("X") { number }
                    KRInspectorAxis("Y") { number }
                }
            })),
            ("Panel", AnyView(KRPanel("Project") { KREmptyState(icon: .folderOpen, title: "素材がありません", message: "素材を読み込んでください。") })),
            ("TabBar", AnyView(KRTabBar([.init("one", "Layers"), .init("two", "Project")], selection: .constant("one")))),
            ("LayerRow", AnyView(KRLayerRow("Title", kind: .text, selected: true))),
            ("AssetRow", AnyView(KRAssetRow("Interview.mov", kind: .video, meta: "1920×1080", duration: "00:00:12:00"))),
            ("Track", AnyView(KRTrack(header: .init("V1", "映像", kind: .video)) { KRClip("Title", kind: .composition).frame(width: 160) })),
            ("Clip", AnyView(KRClip("Title", kind: .composition, state: .selected).frame(width: 160))),
            ("Ruler", AnyView(KRRuler([.init("one", x: 0, label: "1s"), .init("two", x: 120, label: "2s")]))),
            ("Playhead", AnyView(KRPlayhead().frame(height: 120))),
            ("ToolStrip", AnyView(KRToolStrip(KRTool.motion, selection: .constant("select"), placement: .constant(.init())))),
            ("TransportBar", AnyView(KRTransportBar(frames: .constant(45), fps: 24, duration: "00:00:14:00", playing: .constant(false), looping: .constant(false), zoom: .constant("fit"), resolution: .constant("full")))),
            ("TimecodeField", AnyView(KRTimecodeField(frames: .constant(45), fps: 24))),
            ("ViewerFrame", AnyView(KRViewerFrame(selection: .init(CGRect(x: 0.2, y: 0.2, width: 0.6, height: 0.4), label: "layout 1200 × 162")))),
            ("StatusBar", AnyView(KRStatusBar(revision: "rev 131", job: .init("書き出し 42%", progress: 0.42)))),
            ("ProgressBar", AnyView(KRProgressBar(0.42))),
            ("JobRow", AnyView(KRJobRow("書き出し", detail: "rev 131", state: .running(progress: 0.42, remaining: "12 秒")))),
            ("Dialog", AnyView(KRDialog("素材が見つかりません", body: "再リンクしてください。", code: "ASSET_MISSING", actions: [.init("cancel", "キャンセル", variant: .plain), .init("relink", "再リンク", variant: .primary)]))),
            ("Popover", AnyView(KRPopover("キーフレーム") { KRPopoverRow("値") { number } })),
            ("EmptyState", AnyView(KREmptyState(icon: .folderOpen, title: "素材がありません", message: "素材を読み込んでください。"))),
            ("FocusRing", AnyView(KRButton("フォーカス", appearance: .focused) {}))
        ]
    }
}
