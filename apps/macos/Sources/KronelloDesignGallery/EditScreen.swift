import SwiftUI
import KronelloDesign

/// Illustrative Edit-page data using the same layout and components as the app.
struct EditScreen: View {
    @Environment(\.krPalette) var p
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 0) {
                    Text("GUI-003 日本語カット編集").krText(KRType.heading)
                    Text("Design gallery / illustrative project").krText(KRType.caption).foregroundStyle(p.inkMuted)
                }.frame(width: 380, alignment: .leading)
                Spacer()
                KRSegmentedControl([.init("edit", "編集"), .init("motion", "モーション"), .init("template", "テンプレート"), .init("export", "書き出し")], selection: .constant("edit"))
                Spacer()
                KRButton(icon: .undo2, accessibilityLabel: "取り消す") {}
                KRButton(icon: .redo2, accessibilityLabel: "やり直す") {}.disabled(true)
                KRPopupButton("ワークスペース", options: [.init("standard", "標準", icon: .layoutPanelLeft)], selection: .constant("standard")).fixedSize()
            }.padding(.horizontal, KRSpace.space3).frame(height: 44)
            KREditLayout(project: { project }, viewer: { viewer }, inspector: { inspector }, tracks: { tracks })
            KRStatusBar(revision: "rev 4", externalChange: "別のセッションの変更を読み込みました（rev 3 → 4）", error: .init("ASSET_MISSING", "1 件"))
        }.frame(width: 1440, height: 900).background(p.surface100).foregroundStyle(p.ink)
    }
    var project: some View {
        KRPanel(header: { KRTabBar([.init("project", "Project"), .init("effects", "Effects")], selection: .constant("project")) }, actions: {}) {
            VStack(spacing: 0) {
                KRSearchField("素材を検索", text: .constant("")).padding(KRSpace.space2)
                KRAssetRow("video.mov", kind: .video, meta: "320×180", duration: "00:00:05:00")
                KRAssetRow("audio.wav", kind: .audio, meta: "PCM", duration: "00:00:05:00")
                KRAssetRow("Composition 1", kind: .composition, meta: "3 件", duration: "00:00:05:00", selected: true)
                KRAssetRow("missing.mov", kind: .video, meta: "", duration: "00:00:05:00", missing: "ASSET_MISSING")
                Spacer()
            }
        }
    }
    var viewer: some View {
        KRPanel(header: { KRTabBar([.init("sequence", "Sequence 1")], selection: .constant("sequence")) }, actions: {
            Text("CPU 参照").krText(KRType.caption).foregroundStyle(p.inkMuted)
            KRButton(icon: .scan, accessibilityLabel: "セーフエリア") {}
        }) {
            VStack(spacing: 0) {
                HStack(spacing: 0) {
                    KRToolStrip([.init("select", icon: .mousePointer2, name: "選択", shortcut: "V"), .init("blade", icon: .scissors, name: "ブレード", shortcut: "B"),
                        .init("hand", icon: .hand, name: "手のひら", shortcut: "H", unavailableReason: "未対応"), .init("zoom", icon: .zoomIn, name: "ズーム", shortcut: "Z", unavailableReason: "未対応")],
                        selection: .constant("select"), placement: .constant(.init()))
                    KRViewerFrame { p.surface0; Text("Sequence Viewer").krText(KRType.heading).foregroundStyle(p.inkMuted) }.padding(KRSpace.space3)
                }.background(p.surface0)
                KRTransportBar(frames: .constant(24), fps: 24, duration: "00:00:05:00", playing: .constant(false), looping: .constant(false), zoom: .constant("fit"), resolution: .constant("full"))
            }
        }
    }
    var inspector: some View {
        KRPanel("Inspector") {
            VStack(alignment: .leading, spacing: KRSpace.space4) {
                HStack { KRIconView(.layers).foregroundStyle(p.kindComposition); Text("Composition 1").krText(KRType.heading) }.padding(.horizontal, KRSpace.space3)
                Text("Composition クリップ · V2").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                VStack(spacing: KRSpace.space1) {
                    heading("配置")
                    setting("開始", 24); setting("尺", 72); setting("ソース開始", 0, disabled: true)
                }
                VStack(spacing: KRSpace.space1) {
                    heading("時間")
                    KRInspectorSettingRow("速度") { Text("100.0%").krText(KRType.timecode).foregroundStyle(p.inkMuted) }
                    KRInspectorSettingRow("逆再生") { KRCheckbox("", isOn: .constant(false)).accessibilityLabel("逆再生").disabled(true) }
                    Text("速度・ソース開始の変更は未対応です。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                }
                VStack(spacing: KRSpace.space1) {
                    heading("合成")
                    KRInspectorSettingRow("不透明度") { Text("100.0%").krText(KRType.timecode).foregroundStyle(p.inkMuted) }
                    KRInspectorSettingRow("描画モード") { Text("Normal").krText(KRType.label).foregroundStyle(p.inkMuted) }
                    Text("合成設定は表示のみです。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                }
                KRButton("モーションで開く", icon: .layers, variant: .secondary) {}.padding(.horizontal, KRSpace.space3)
                Spacer()
            }.padding(.vertical, KRSpace.space3)
        }
    }
    func heading(_ name: String) -> some View { Text(name).krText(KRType.heading).frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, KRSpace.space3) }
    func setting(_ name: String, _ frames: Int64, disabled: Bool = false) -> some View {
        KRInspectorSettingRow(name) { KRTimecodeField(frames: .constant(frames), fps: 24, currentTime: false, label: name).disabled(disabled) }
    }
    var tracks: some View {
        KRPanel(header: { HStack { KRPanelTitle("Sequence"); Text("320×180 · 24/1 fps · 48 kHz").krText(KRType.caption).foregroundStyle(p.inkMuted); Text("00:00:01:00").krText(KRType.timecode).foregroundStyle(p.accentInk) } }, actions: {
            KRButton(icon: .magnet, accessibilityLabel: "スナップ", pressed: true) {}
            KRButton(icon: .chevronsLeft, accessibilityLabel: "時間軸を縮小") {}
            KRButton(icon: .plus, accessibilityLabel: "時間軸を拡大") {}
        }) {
            ZStack(alignment: .topLeading) {
                VStack(spacing: 0) {
                    HStack(spacing: 0) { Color.clear.frame(width: 200, height: 20); KRRuler((0...5).map { .init("\($0)", x: 8 + Double($0) * 240, label: $0 == 5 ? nil : "\($0)s0f") }) }
                    KRTrack(header: .init("V2", "Video", kind: .video, selected: true, visibilityEnabled: false), headerWidth: 200) {
                        KRClip("Composition 1", kind: .composition, state: .selected).frame(width: 720).offset(x: 248)
                    }
                    KRTrack(header: .init("V1", "Video", kind: .video, visibilityEnabled: false), headerWidth: 200) {
                        ZStack(alignment: .leading) {
                            KRClip("video.mov · stream 0", kind: .video).frame(width: 720).offset(x: 8)
                            KRClip("missing.mov", kind: .video, state: .missing("ASSET_MISSING")).frame(width: 240).offset(x: 968)
                        }
                    }
                    KRTrack(header: .init("A1", "Audio", kind: .audio, visibilityEnabled: false), headerWidth: 200) {
                        KRClip("audio.wav · stream 0", kind: .audio).frame(width: 1200).offset(x: 8)
                    }
                    Spacer()
                }
                KRPlayhead().frame(height: 128).offset(x: 442)
            }
        }
    }
}

enum EditSheets {
    static var all: [(String, AnyView)] { [
        ("Blade-hit-and-CPU-preview", AnyView(EditReviewSheet())),
        ("Timecode-role", AnyView(VStack(alignment: .leading, spacing: KRSpace.space4) {
            SampleRow("current / amber") { KRTimecodeField(frames: .constant(24), fps: 24) }
            SampleRow("placement / neutral") { KRTimecodeField(frames: .constant(24), fps: 24, currentTime: false, label: "開始") }
            SampleRow("focused placement") { KRTimecodeField(frames: .constant(24), fps: 24, appearance: .focused, currentTime: false, label: "開始") }
            SampleRow("disabled source") { KRTimecodeField(frames: .constant(0), fps: 24, currentTime: false, label: "ソース開始").disabled(true) }
        })),
        ("EditClip-AssetRow-focus", AnyView(VStack(spacing: KRSpace.space4) {
            KRClip("Composition", kind: .composition, state: .selected, appearance: .focused).frame(width: 320)
            KRClip("missing.mov", kind: .video, state: .missing("ASSET_MISSING")).frame(width: 320)
            KRAssetRow("video.mov", kind: .video, meta: "320×180", duration: "00:00:05:00", appearance: .focused)
            KRAssetRow("audio.wav", kind: .audio, meta: "PCM", duration: "00:00:05:00", selected: true)
        })),
        ("Track-unavailable-visibility", AnyView(VStack(spacing: KRSpace.space4) {
            KRTrack(header: .init("V2", "Video", kind: .video, selected: true, visibilityEnabled: false), headerWidth: 200) { KRClip("Composition", kind: .composition, state: .selected).frame(width: 320) }
            KRTrack(header: .init("A1", "Audio", kind: .audio, locked: true, visibilityEnabled: false), headerWidth: 200, locked: true) { KRClip("Audio", kind: .audio).frame(width: 400) }
        }))
    ] }
}

private struct EditReviewSheet: View {
    @Environment(\.krPalette) var p
    var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space4) {
            Text("video requires explicit media backend").krText(KRType.label)
            KRErrorLine(.init("UNSUPPORTED_FEATURE", "video requires explicit media backend"))
            KRButton("CPU 参照で表示", variant: .secondary) {}
            Text("CPU 参照").krText(KRType.caption).foregroundStyle(p.inkMuted)
            Text("CPU 参照 · 再生中は前回のフレームを表示").krText(KRType.caption).foregroundStyle(p.inkMuted)
            KRClip("Composition 1", kind: .composition, state: .selected)
                .allowsHitTesting(false)
                .overlay { KRBladeHitArea("Composition 1 を分割", begin: { _ in }, update: { _ in }, release: {}) }
                .frame(width: 320)
            KRInspectorSettingRow("逆再生") { KRCheckbox("", isOn: .constant(true)).accessibilityLabel("逆再生").disabled(true) }
            Text("1920×1080 · 23.976 fps · 48 kHz").krText(KRType.caption).foregroundStyle(p.inkMuted)
            KRRuler([.init("0", x: 8, label: "0s0f"), .init("12", x: 100, label: "0s12f"), .init("24", x: 200, label: "1s0f")]).frame(height: 20)
        }
    }
}
