import SwiftUI
import KronelloDesign

struct WorkflowComponentSheets {
    static var all: [(String,AnyView)] {
        [("ExportStartButton",AnyView(VStack(alignment:.leading,spacing:KRSpace.space2) { KRExportStartButton(){}.disabled(true); KRErrorLine(.init("FONT_MISSING","型付きエラーがある間は書き出せません")) })), ("TemplatePublicInputs",AnyView(TemplatePublicInputsSheet())), ("ExportRecheckBanner",AnyView(KRConflictBanner(.init("REVISION_CONFLICT","確認後に作品が変更されました"),reapplyLabel:"再確認",discard:{},reapply:{}))), ("TemplateVariant", AnyView(TemplateVariantSheet())), ("DurationPolicyBands",AnyView(KRDurationPolicyBands(authoring:"8s",placement:"00:00:10:00",intro:"2/5s",outro:"3/10s",mode:"loop",authoringSeconds:8,placementSeconds:10,introSeconds:0.4,outroSeconds:0.3))),
         ("JobRow-all-states", AnyView(WorkflowJobSheet())), ("MigrationPlan", AnyView(KRDialog("移行計画",body:"差分を確認して適用してください。既存の配置は明示した適用まで変わりません。",detail:"/instance/version\n  1.0.0 → 1.1.0\n/definition/duration_policy/middle_mode\n  loop → hold",actions:[.init("cancel","キャンセル",variant:.plain){},.init("apply","移行を適用",variant:.secondary){}])))]
    }
}
struct TemplateVariantSheet: View {
    var body: some View {
        HStack(alignment:.top,spacing:KRSpace.space3) {
            KRTemplateVariantView("landscape",extent:.init(width:1920,height:1080),bounds:[.init(x:180,y:700,width:1400,height:180)],stage:"layout").frame(width:340,height:320)
            KRTemplateVariantView("portrait",extent:.init(width:1080,height:1920),bounds:[],stage:"layout",diagnostic:.init("TEMPLATE_OVERFLOW","行数 4 > max_lines 2")).frame(width:340,height:320)
        }
    }
}
struct WorkflowJobSheet: View {
    var body: some View {
        VStack(spacing:0) {
            KRJobRow("Queued.mov",detail:"ProRes/PCM24 · rev 131",state:.queued) { KRButton("中止",variant:.plain){} }
            KRJobRow("Running.mp4",detail:"AV1/ALAC · rev 130",state:.running(progress:0.42,remaining:"残り時間は未測定")) { KRButton("中止",variant:.plain){} }
            KRJobRow("Complete.mov",detail:"HEVC/ALAC · rev 128",state:.done(elapsed:"18s")) { KRButton("表示",variant:.plain){} }
            KRJobRow("Failed.mov",detail:"H.264/ALAC · rev 129",state:.failed(.init("ENCODER_UNAVAILABLE","device を利用できません"))) { KRButton("設定を確認",variant:.plain){} }
            KRJobRow("Cancelled.mov",detail:"ProRes/PCM24 · rev 127",state:.cancelled)
            KRJobRow("Interrupted.mov",detail:"ProRes/PCM24 · rev 126",state:.interrupted) { KRButton("設定を確認",variant:.plain){} }
        }
    }
}
struct WorkflowScreen: View {
    @Environment(\.krPalette) var p
    let template: Bool
    var body: some View {
        VStack(spacing:0) {
            HStack {
                VStack(alignment:.leading) { Text("GUI-004 · Kronello").krText(KRType.heading); Text("Gallery · illustrative data").krText(KRType.caption).foregroundStyle(p.inkMuted) }
                Spacer()
                KRSegmentedControl([.init("edit","編集"),.init("motion","モーション"),.init("template","テンプレート"),.init("export","書き出し")],selection:.constant(template ? "template":"export"))
                Spacer(); KRPopupButton("ワークスペース",options:[.init("standard","標準")],selection:.constant("standard")).frame(width:144)
            }.padding(.horizontal,KRSpace.space3).frame(height:44).krBottomLine()
            if template { templateScreen } else { exportScreen }
            KRStatusBar(revision:"rev 131",error:template ? .init("TEMPLATE_OVERFLOW","1 件") : .init("FONT_MISSING","1 件"),job:.init("書き出し 42%",progress:0.42))
        }.frame(width:1440,height:900).foregroundStyle(p.ink).background(p.surface100)
    }
    var templateScreen: some View {
        KRTemplatePageLayout(templates:{
            KRPanel("Templates") {
                VStack(alignment:.leading,spacing:KRSpace.space3) {
                    Text("lower_third_ja").krText(KRType.timecode)
                    Text("1.0.0 · 公開入力 2 · 配置 4").krText(KRType.caption).foregroundStyle(p.inkMuted)
                    Text("1.1.0 · 公開入力 2 · 配置 0").krText(KRType.caption).foregroundStyle(p.inkMuted)
                }.padding(KRSpace.space3)
            }
        },comparison:{
            KRPanel(header:{ KRPanelTitle("Variant の比較"); KRSegmentedControl([.init("layout","layout"),.init("ink","ink"),.init("visual","visual")],selection:.constant("layout")) },actions:{ Text("00:00:02:00").krText(KRType.ruler).foregroundStyle(p.accentInk) }) { TemplateVariantSheet().padding(KRSpace.space3) }
        },policy:{
            KRPanel("尺のポリシー（新版の下書き）") {
                VStack(alignment:.leading,spacing:KRSpace.space3) {
                    HStack { KRPopupButton("中間区間",options:[.init("loop","loop")],selection:.constant("loop")).frame(width:110); Text("minimum_middle · 12f").krText(KRType.ruler); Spacer(); Text("配置の尺 · 00:00:10:00").krText(KRType.ruler) }.padding(.horizontal,KRSpace.space3)
                    KRDurationPolicyBands(authoring:"8s",placement:"00:00:10:00",intro:"2/5s",outro:"3/10s",mode:"loop",authoringSeconds:8,placementSeconds:10,introSeconds:0.4,outroSeconds:0.3)
                    Text("intro + outro + minimum_middle 未満は DURATION_TOO_SHORT。新版の公開では配置を更新しません。").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal,KRSpace.space3)
                }.padding(.vertical,KRSpace.space2)
            }
        },inputs:{
            KRPanel(header:{KRTabBar([.init("inputs","公開入力"),.init("versions","版")],selection:.constant("inputs"))},actions:{}) {
                VStack(alignment:.leading,spacing:KRSpace.space3) {
                    Text("headline · Text · default あり").krText(KRType.timecode)
                    KRTextField("",value:.constant("長い日本語の見出しで比較します"))
                    Text("見出し › Text").krText(KRType.caption).foregroundStyle(p.inkMuted)
                    Text("accent · Color · sRGB").krText(KRType.timecode)
                    KRTextField("",value:.constant("#C8280A"))
                    Spacer()
                    Text("既存の配置は元の版に固定され、自動では更新しません。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                    KRButton("移行計画…",variant:.secondary){}
                    KRButton("新版として公開…",variant:.secondary){}
                }.padding(KRSpace.space3)
            }
        })
    }
    var exportScreen: some View {
        KRExportPageLayout(settings:{
            KRPanel("書き出し設定") {
                VStack(alignment:.leading,spacing:KRSpace.space3) {
                    KRPopupButton("対象",options:[.init("comp","Composition · Teaser")],selection:.constant("comp"))
                    KRInspectorSettingRow("範囲") { KRPopupButton("範囲",options:[.init("all","全体")],selection:.constant("all")).frame(width:140) }
                    KRPopupButton("プリセット",options:[.init("prores","ProRes 422 HQ · MOV · PCM24")],selection:.constant("prores"))
                    Text("1920×1080 · 24/1 fps · 対象に従う").krText(KRType.ruler).foregroundStyle(p.inkMuted)
                    Text("SDR · Rec.709 · 背景 黒（明示）").krText(KRType.body)
                    Text("PCM24 · 48 kHz · stereo\nクリッピングは worker が停止します。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                    KRTextField("出力先",value:.constant("/Exports/Teaser.mov")); KRButton("選択…",variant:.secondary){}
                    Spacer(); KRExportStartButton(){}.disabled(true)
                    Text("rev 131 の内容を固定し、別プロセスで書き出します。アプリを閉じても続きます。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                }.padding(KRSpace.space3)
            }
        },preview:{
            KRPanel("プレビュー · Teaser") {
                VStack(spacing:KRSpace.space3) { KRViewerFrame().padding(KRSpace.space3); HStack { Text("00:00:00:00"); Rectangle().fill(p.selectionBg).overlay { Rectangle().strokeBorder(p.selection,lineWidth:1) }.frame(height:16); Text("00:00:08:00") }.krText(KRType.ruler).padding(KRSpace.space3) }
            }
        },check:{
            KRPanel("書き出し前の確認",actions:{KRButton(icon:.refreshCw,accessibilityLabel:"もう一度確認"){} }) {
                VStack(alignment:.leading,spacing:KRSpace.space3) {
                    KRErrorLine(.init("FONT_MISSING","作品の固定フォントを指定してください"))
                    Text("エラーが 1 件あります。解消するまで書き出せません。").krText(KRType.body)
                    Text("範囲の先頭時刻で意味的検査。全フレーム・音声・encoder の受理を保証しません。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                    Text("192 frames · サイズは未測定").krText(KRType.ruler).foregroundStyle(p.inkMuted)
                    KRButton("再確認",variant:.secondary){}
                }.padding(KRSpace.space3)
            }
        },jobs:{ KRPanel("ジョブ") { ScrollView { WorkflowJobSheet() } } })
    }
}

struct TemplatePublicInputsSheet: View {
    var body: some View {
        VStack(alignment:.leading,spacing:KRSpace.space3) {
            SampleRow("Text") { KRTemplateValueEditor(value:["kind":"string","value":"日本語の見出し"],schema:[:],assets:[]){_ in}.frame(width:260) }
            SampleRow("Scalar") { KRTemplateValueEditor(value:["kind":"scalar","value":0.8],schema:["minimum":0.0,"maximum":1.0],assets:[]){_ in}.frame(width:88) }
            SampleRow("Color · sRGB") { KRTemplateValueEditor(value:color,schema:[:],assets:[]){_ in}.frame(width:260) }
            SampleRow("MediaSlot") { KRTemplateValueEditor(value:["kind":"asset_ref","value":"media"],schema:[:],assets:[["id":"media","path":"Interview.mov"]]){_ in}.frame(width:260) }
            SampleRow("DataTable") { KRTemplateValueEditor(value:["kind":"data_table","value":["columns":["headline":"string"],"rows":[["headline":["kind":"string","value":"長い公開入力で比較"]]]]],schema:[:],assets:[]){_ in}.frame(width:400) }
        }
    }
    var color: [String:Any] { ["kind":"color","value":["space":"srgb","components":["r":0.78,"g":0.16,"b":0.04,"alpha":1.0]]] }
}
