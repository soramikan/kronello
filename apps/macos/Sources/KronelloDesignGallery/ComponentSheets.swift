import KronelloDesign
import SwiftUI

@MainActor
enum ComponentSheets {
    static let diagnostic = KRDiagnostic("EVALUATION_ERROR", "式を評価できません。式を確認してください。")
    static let tabs: [KRTab] = [.init("layers", "Layers"), .init("project", "Project"), .init("title", "Title", closable: true)]
    static let options: [KRPopupOption] = [.init("full", "Full"), .init("half", "Half"), .init("quarter", "Quarter", disabled: true)]
    static let ticks: [KRRulerTick] = (0..<25).map { .init("tick-\($0)", x: CGFloat($0) * 24, label: $0 % 4 == 0 ? "\($0 / 4)s" : nil) }
    static func number(_ state: KRNumberFieldState = .resting, value: Double = 960, unit: String = "px") -> some View {
        KRNumberField(value: .constant(value), unit: unit, state: state, accessibilityLabel: "X 位置", onCommit: { _, _ in }).frame(width: 96)
    }
    static func inspector(_ label: String, source: KRPropertySource = .constant, on: Bool = false, selected: Bool = false, error: KRDiagnostic? = nil) -> some View {
        KRInspectorRow(label, source: source, onKeyframe: on, selected: selected, error: error, previous: {}, next: {}) {
            HStack(spacing: KRSpace.space1) {
                KRInspectorAxis("X") {
                    KRNumberField(value: .constant(960), error: error != nil, accessibilityLabel: "X " + label, onCommit: { _, _ in }).frame(width: 64)
                }
                KRInspectorAxis("Y") {
                    KRNumberField(value: .constant(720), error: error != nil, accessibilityLabel: "Y " + label, onCommit: { _, _ in }).frame(width: 64)
                }
            }.disabled(source == .expression)
        }
    }
    static var menuItems: [KRMenuItem] {
        [.init("heading", "レイヤー", kind: .heading), .init("copy", "コピー", icon: .copy, shortcut: "⌘C"),
         .init("paste", "ペースト", icon: .clipboardPaste, shortcut: "⌘V", disabled: true), .init("visible", "表示する", checked: true),
         .init("sub", "補間", children: [.init("linear", "Linear", checked: true), .init("cubic", "Cubic"), .init("hold", "Hold")]),
         .init("separator", kind: .separator), .init("delete", "削除", icon: .trash2, shortcut: "⌫", destructive: true)]
    }
    static var all: [(String, AnyView)] {
        controls + hierarchy + timeline + viewer + feedback + overlays + [
            ("Icons", AnyView(IconSheet())), ("Typography", AnyView(TypeSheet())), ("Palette", AnyView(PaletteSheet()))
        ]
    }
    static var controls: [(String, AnyView)] {
        [
            ("Button", AnyView(buttons)),
            ("SegmentedControl", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                SampleRow("selected / resting") { KRSegmentedControl([.init("value", "値"), .init("speed", "速度")], selection: .constant("value")) }
                SampleRow("hover") { KRSegmentedControl([.init("value", "値"), .init("speed", "速度")], selection: .constant("value"), appearance: .hover) }
                SampleRow("focused") { KRSegmentedControl([.init("value", "値"), .init("speed", "速度")], selection: .constant("value"), appearance: .focused) }
                SampleRow("disabled") { KRSegmentedControl([.init("value", "値"), .init("speed", "速度")], selection: .constant("speed")).disabled(true) }
            })),
            ("PopupButton", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                ForEach(KRControlAppearance.allCases, id: \.self) { state in SampleRow(state.rawValue) { KRPopupButton("解像度", options: options, selection: .constant("full"), appearance: state).frame(width: 160) } }
                SampleRow("disabled") { KRPopupButton("解像度", options: options, selection: .constant("full")).frame(width: 160).disabled(true) }
                SampleRow("open menu") { KRMenu(options.map { .init($0.id, $0.label, checked: $0.id == "full", disabled: $0.disabled) }, current: "full").frame(width: 240) }
            })),
            ("Menu", AnyView(HStack(alignment: .top, spacing: KRSpace.space4 * 2) {
                KRMenu(menuItems, current: "copy").frame(width: 300)
                KRMenu([.init("heading", "補間", kind: .heading), .init("linear", "Linear", checked: true), .init("cubic", "Cubic"), .init("hold", "Hold")], current: "linear").frame(width: 220)
            }.padding(KRSpace.space4))),
            ("NumberField", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                ForEach(KRNumberFieldState.allCases, id: \.self) { state in SampleRow(state.rawValue) { number(state) } }
                SampleRow("disabled") { number().disabled(true) }
                SampleRow("units") { HStack(spacing: KRSpace.space2) { number(value: 100, unit: "%"); number(value: 45, unit: "°"); number(value: 24, unit: "fps") } }
            })),
            ("TextField", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                SampleRow("value + help") { KRTextField("名前", value: .constant("見出し"), help: "レイヤーの表示名です。").frame(width: 360) }
                SampleRow("placeholder") { KRTextField("名前", value: .constant(""), placeholder: "名前を入力").frame(width: 360) }
                SampleRow("focused") { KRTextField("名前", value: .constant("見出し"), appearance: .focused).frame(width: 360) }
                SampleRow("error") { KRTextField("名前", value: .constant(""), error: .init("INVALID_NAME", "名前を入力してください。")).frame(width: 480) }
                SampleRow("disabled") { KRTextField("名前", value: .constant("見出し")).frame(width: 360).disabled(true) }
            })),
            ("SearchField", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                SampleRow("placeholder") { KRSearchField(text: .constant("")).frame(width: 240) }
                SampleRow("query") { KRSearchField(text: .constant("Title")).frame(width: 240) }
                SampleRow("focused") { KRSearchField(text: .constant("Title"), appearance: .focused).frame(width: 240) }
                SampleRow("disabled") { KRSearchField(text: .constant("")).frame(width: 240).disabled(true) }
            })),
            ("Checkbox", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                SampleRow("off") { KRCheckbox("起動時に表示", isOn: .constant(false)) }
                SampleRow("on") { KRCheckbox("起動時に表示", isOn: .constant(true)) }
                SampleRow("mixed") { KRCheckbox("表示する", isOn: .constant(false), mixed: true) }
                SampleRow("hover") { KRCheckbox("表示する", isOn: .constant(false), appearance: .hover) }
                SampleRow("focused") { KRCheckbox("表示する", isOn: .constant(true), appearance: .focused) }
                SampleRow("disabled off / on") { HStack(spacing: KRSpace.space4) { KRCheckbox("表示する", isOn: .constant(false)); KRCheckbox("表示する", isOn: .constant(true)) }.disabled(true) }
            })),
            ("Radio", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                Text("プレビュー解像度").krText(KRType.label)
                SampleRow("off / on") { HStack(spacing: KRSpace.space4) { KRRadio("Full", id: "full", selection: .constant("half")); KRRadio("Half", id: "half", selection: .constant("half")) } }
                SampleRow("hover") { KRRadio("Full", id: "full", selection: .constant("half"), appearance: .hover) }
                SampleRow("focused") { KRRadio("Full", id: "full", selection: .constant("full"), appearance: .focused) }
                SampleRow("disabled") { KRRadio("Full", id: "full", selection: .constant("full")).disabled(true) }
            })),
            ("Slider", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                ForEach([0, 50, 100], id: \.self) { value in SampleRow("\(value)%") { sliderPair(Double(value)) } }
                SampleRow("focused") { sliderPair(50, appearance: .focused) }
                SampleRow("disabled") { sliderPair(50).disabled(true) }
            })),
            ("FocusRing", AnyView(HStack(spacing: KRSpace.space4) {
                KRButton("書き出す", variant: .primary, appearance: .focused) {}
                number(.focused); KRCheckbox("表示する", isOn: .constant(true), appearance: .focused)
                KRPopupButton("解像度", options: options, selection: .constant("full"), appearance: .focused).frame(width: 120)
            }))
        ]
    }
    static var hierarchy: [(String, AnyView)] {
        [
            ("KeyframeNavigator", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                SampleRow("Constant") { KRKeyframeNavigator(source: .constant) }
                SampleRow("Curve on") { KRKeyframeNavigator(source: .curve, onKeyframe: true, previous: {}, next: {}) }
                SampleRow("Curve off") { KRKeyframeNavigator(source: .curve, previous: {}, next: {}) }
                SampleRow("no previous") { KRKeyframeNavigator(source: .curve, next: {}) }
                SampleRow("Expression") { KRKeyframeNavigator(source: .expression) }
            })),
            ("InspectorRow", AnyView(VStack(spacing: KRSpace.space1) {
                inspector("Position", source: .constant); inspector("Position", source: .curve, on: true)
                inspector("Position", source: .curve); inspector("Position", source: .expression)
                inspector("Position", source: .curve, on: true, selected: true)
                KRInspectorRow("Position", appearance: .hover) {
                    HStack(spacing: KRSpace.space1) {
                        KRInspectorAxis("X") { number(value: 960, unit: "") }
                        KRInspectorAxis("Y") { number(value: 720, unit: "") }
                    }
                }
                inspector("Position", source: .expression, error: diagnostic)
            }.frame(width: 480))),
            ("Panel", AnyView(HStack(spacing: KRSpace.space4) {
                KRPanel("Project", actions: { KRButton(icon: .plus, accessibilityLabel: "素材を追加") {} }) {
                    VStack(spacing: 0) { KRAssetRow("Interview_A.mov", kind: .video, meta: "1920×1080", duration: "00:00:12:00"); Spacer() }
                }.frame(width: 360, height: 160)
                KRPanel(header: { KRTabBar(tabs, selection: .constant("layers")) }, actions: { KRButton(icon: .ellipsis, accessibilityLabel: "パネルメニュー") {} }) {
                    VStack(spacing: 0) { KRLayerRow("Title", kind: .text, selected: true); Spacer() }
                }.frame(width: 380, height: 160)
            })),
            ("TabBar", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                SampleRow("active first") { KRTabBar(tabs, selection: .constant("layers")) }
                SampleRow("active closeable") { KRTabBar(tabs, selection: .constant("title")) }
                SampleRow("hover") { KRTabBar([.init("title", "Title", closable: true)], selection: .constant(""), appearance: .hover) }
                SampleRow("focused") { KRTabBar([.init("title", "Title")], selection: .constant("title"), appearance: .focused) }
            })),
            ("LayerRow", AnyView(VStack(spacing: 0) {
                KRLayerRow("Title group", kind: .group, hasChildren: true, expanded: true)
                KRLayerRow("見出し", kind: .text, level: 1, transformParent: "Anchor", selected: true)
                KRLayerRow("背景帯", kind: .shape, level: 1)
                KRLayerRow("Anchor", kind: .null, hidden: true)
                KRLayerRow("背景", kind: .media(.video), locked: true)
                KRLayerRow("Lower third", kind: .composition)
                KRLayerRow("音声", kind: .media(.audio), hidden: true, locked: true)
                KRLayerRow("Hover · controls", kind: .text, appearance: .hover)
                KRLayerRow("Focus", kind: .text, appearance: .focused)
            }.frame(width: 600))),
            ("AssetRow", AnyView(VStack(spacing: 0) {
                ForEach(KRMediaKind.allCases, id: \.self) { kind in KRAssetRow("Sample · \(kind.rawValue)", kind: kind, meta: "1920×1080 · 24 fps", duration: "00:00:12:00", selected: kind == .composition) }
                KRAssetRow("Interview_B.mov", kind: .video, meta: "", duration: "00:00:08:00", missing: "ASSET_MISSING")
                KRAssetRow("Interview_C.mov", kind: .video, meta: "", missing: "ASSET_HASH_MISMATCH")
                KRAssetRow("Hover.mov", kind: .video, meta: "1920×1080", appearance: .hover)
                KRAssetRow("Focus.mov", kind: .video, meta: "1920×1080", appearance: .focused)
            }))
        ]
    }
    static var timeline: [(String, AnyView)] {
        [
            ("KeyframeGlyph", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                ForEach(KRInterpolation.allCases, id: \.self) { mode in
                    SampleRow(mode.rawValue) { HStack(spacing: KRSpace.space4) {
                        labeledGlyph("rest", mode); labeledGlyph("hover", mode, appearance: .hover)
                        labeledGlyph("selected", mode, selected: true); labeledGlyph("hollow", mode, hollow: true)
                        labeledGlyph("on", mode, on: true); labeledGlyph("focus", mode, appearance: .focused)
                    } }
                }
                SampleRow("layer summary · 7px") {
                    HStack(spacing: KRSpace.space4) {
                        ForEach(KRInterpolation.allCases, id: \.self) { mode in KRKeyframeGlyph(mode, size: 7) }
                    }
                }
            })),
            ("Track", AnyView(VStack(spacing: 0) {
                KRRuler(ticks).padding(.leading, 168)
                KRTrack(header: .init("V2", "Title", kind: .composition, selected: true)) { KRClip("Lower third", kind: .composition, state: .selected).frame(width: 240).padding(.leading, KRSpace.space4) }
                KRTrack(header: .init("V1", "映像", kind: .video)) { KRClip("Interview_A.mov", kind: .video).frame(width: 360).padding(.leading, KRSpace.space2) }
                KRTrack(header: .init("A1", "音声", kind: .audio, hidden: true, locked: true), locked: true) { KRClip("Music.wav", kind: .audio).frame(width: 420).padding(.leading, KRSpace.space2) }
            }.overlay(alignment: .leading) { KRPlayhead().offset(x: 320) })),
            ("Clip", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                ForEach(KRMediaKind.allCases, id: \.self) { kind in SampleRow(kind.rawValue) { HStack(spacing: KRSpace.space2) {
                    KRClip("Sample", kind: kind).frame(width: 160); KRClip("選択中", kind: kind, state: .selected).frame(width: 160)
                } } }
                SampleRow("missing") { KRClip("Missing.mov", kind: .video, state: .missing("ASSET_MISSING")).frame(width: 240) }
                SampleRow("disabled") { KRClip("Disabled.mov", kind: .video, state: .disabled).frame(width: 240) }
                SampleRow("hover") { KRClip("Hover.mov", kind: .video, appearance: .hover).frame(width: 240) }
                SampleRow("focused") { KRClip("Focus.mov", kind: .video, appearance: .focused).frame(width: 240) }
            })),
            ("Ruler", AnyView(KRRuler(ticks))),
            ("Playhead", AnyView(HStack(spacing: KRSpace.space4 * 2) { KRPlayhead().frame(height: 140); KRPlayhead(showHandle: false).frame(height: 140) }))
        ]
    }
    static var viewer: [(String, AnyView)] {
        [
            ("ToolStrip", AnyView(HStack(alignment: .top, spacing: KRSpace.space4 * 2) {
                ForEach(0..<5) { index in VStack(spacing: KRSpace.space3) {
                    Text(["viewerLeft", "viewerRight", "floating", "collapsed", "floating collapsed"][index]).krText(KRType.caption)
                    KRToolStrip(KRTool.motion, selection: .constant("select"), placement: .constant(.init(index == 1 ? .viewerRight : index == 2 || index == 4 ? .floating(x: 40, y: 40) : .viewerLeft, collapsed: index >= 3)))
                } }
            }.padding(KRSpace.space4))),
            ("TransportBar", AnyView(VStack(spacing: KRSpace.space4) { transport(playing: false, looping: false); transport(playing: true, looping: true) })),
            ("TimecodeField", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
                SampleRow("current time") { KRTimecodeField(frames: .constant(2016), fps: 24) }
                SampleRow("editing") { KRTimecodeField(frames: .constant(2016), fps: 24, appearance: .focused) }
                SampleRow("invalid") { KRTimecodeField(frames: .constant(2016), fps: 24, invalid: true) }
                SampleRow("disabled") { KRTimecodeField(frames: .constant(2016), fps: 24).disabled(true) }
            })),
            ("ViewerFrame", AnyView(HStack(alignment: .top, spacing: KRSpace.space4) {
                VStack { Text("16:9 · selection").krText(KRType.caption); KRViewerFrame(selection: .init(CGRect(x: 0.15, y: 0.45, width: 0.7, height: 0.22), label: "layout 1200 × 162")).frame(width: 420, height: 280) }
                VStack { Text("narrow · 16:9").krText(KRType.caption); KRViewerFrame().frame(width: 140, height: 280) }
                VStack { Text("9:16").krText(KRType.caption); KRViewerFrame(aspectRatio: 9 / 16).frame(width: 140, height: 280) }
            }))
        ]
    }
    static var feedback: [(String, AnyView)] {
        [
            ("StatusBar", AnyView(VStack(spacing: KRSpace.space4) {
                KRStatusBar(revision: "rev 131")
                KRStatusBar(revision: "rev 131", externalChange: "MCP の変更を読み込みました（rev 130 → 131）")
                KRStatusBar(revision: "rev 131", error: .init("ASSET_MISSING", "1 件"))
                KRStatusBar(revision: "rev 131", job: .init("書き出し 42%", progress: 0.42, remainingCount: 2))
            })),
            ("ProgressBar", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) { ForEach([0.0, 0.42, 1.0], id: \.self) { value in SampleRow("\(Int(value * 100))%") { KRProgressBar(value) } } })),
            ("JobRow", AnyView(VStack(spacing: 0) {
                KRJobRow("書き出し: Teaser.mov", detail: "ProRes 422 HQ · PCM24 · rev 131", state: .running(progress: 0.42, remaining: "残り 12 秒")) { KRButton(icon: .x, accessibilityLabel: "中止", size: 20, iconSize: 12) {} }
                KRJobRow("書き出し: Portrait.mov", detail: "ProRes · rev 131", state: .queued) { KRButton(icon: .x, accessibilityLabel: "取り消す", size: 20, iconSize: 12) {} }
                KRJobRow("書き出し: Title.png", detail: "PNG · rev 130", state: .done(elapsed: "8 秒")) { KRButton(icon: .folderOpen, accessibilityLabel: "フォルダを開く", size: 20, iconSize: 12) {} }
                KRJobRow("書き出し: Teaser.mov", detail: "", state: .failed(.init("ENCODER_UNAVAILABLE", "設定を確認してください。"))) { KRButton("設定…") {} }
                KRJobRow("解析: Interview.mov", detail: "rev 129", state: .cancelled)
            }))
        ]
    }
    static var overlays: [(String, AnyView)] {
        [
            ("Dialog", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4 * 2) {
                KRDialog("素材が 1 件見つかりません", body: "書き出しを停止しました。元の素材を指定して再リンクしてください。", code: "ASSET_MISSING", detail: "media/Interview_B.mov\nasset: 01234567-89ab-cdef", actions: [.init("cancel", "キャンセル", variant: .plain), .init("file", "ファイルを選択…"), .init("folder", "フォルダを検索…", variant: .primary)])
                KRDialog("新しいプロジェクトを作成します", body: "現在の変更は保存されています。", actions: [.init("cancel", "キャンセル", variant: .plain), .init("create", "作成する", variant: .primary)])
            }.padding(KRSpace.space4))),
            ("Popover", AnyView(KRPopover("キーフレーム", arrowX: 120) {
                KRPopoverRow("補間") { KRPopupButton("補間", options: [.init("linear", "Linear"), .init("cubic", "Cubic"), .init("hold", "Hold")], selection: .constant("cubic")).frame(width: 120) }
                KRPopoverRow("値") { number(value: 1720) }
                KRPopoverRow("接線を揃える") { KRCheckbox("", isOn: .constant(true)).accessibilityLabel("接線を揃える") }
            }.padding(KRSpace.space4))),
            ("EmptyState", AnyView(HStack(spacing: KRSpace.space4) {
                KREmptyState(icon: .folderOpen, title: "素材がありません", message: "ファイルをドロップするか、読み込んでください。") { KRButton("素材を読み込む…") {} }
                KREmptyState(icon: .folderOpen, title: "ここにドロップ", message: "", dropSummary: "映像ファイル 3 件を読み込みます。")
            }))
        ]
    }
    static var buttons: some View {
        VStack(alignment: .leading, spacing: KRSpace.space4) {
            ForEach(KRButtonVariant.allCases, id: \.self) { variant in SampleRow(variant.rawValue) {
                HStack(spacing: KRSpace.space2) {
                    ForEach(KRControlAppearance.allCases, id: \.self) { state in KRButton(state.rawValue, variant: variant, appearance: state) {} }
                    KRButton("disabled", variant: variant) {}.disabled(true)
                }
            } }
            SampleRow("icon / toggle") { HStack(spacing: KRSpace.space2) {
                KRButton(icon: .magnet, accessibilityLabel: "スナップ") {}
                KRButton(icon: .magnet, accessibilityLabel: "スナップ", pressed: true) {}
                KRButton(icon: .magnet, accessibilityLabel: "スナップ", appearance: .focused) {}
                KRButton(icon: .magnet, accessibilityLabel: "スナップ") {}.disabled(true)
            } }
        }
    }
    static func sliderPair(_ value: Double, appearance: KRControlAppearance = .resting) -> some View {
        HStack(spacing: KRSpace.space2) {
            KRSlider(value: .constant(value), range: 0...100, accessibilityLabel: "不透明度", appearance: appearance, onCommit: { _, _ in }).frame(width: 240)
            number(value: value, unit: "%")
        }
    }
    static func labeledGlyph(_ label: String, _ mode: KRInterpolation, selected: Bool = false, hollow: Bool = false, on: Bool = false,
                             appearance: KRControlAppearance = .resting) -> some View {
        VStack(spacing: KRSpace.space1) {
            KRKeyframeGlyph(mode, selected: selected, hollow: hollow, on: on, appearance: appearance, action: {})
            Text(label).krText(KRType.caption)
        }.frame(width: 56)
    }
    static func transport(playing: Bool, looping: Bool) -> some View {
        KRTransportBar(frames: .constant(2016), fps: 24, duration: "00:00:14:00", playing: .constant(playing), looping: .constant(looping), zoom: .constant("fit"), resolution: .constant("full"))
    }
}
