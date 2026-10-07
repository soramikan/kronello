import AppKit
import SwiftUI
import KronelloAppModel
import KronelloDesign

struct ExportPage: View {
    @Environment(\.krPalette) var p
    @ObservedObject var editor: EditorModel
    @State private var previewFrame: Int64 = 0
    @State private var playing = false
    @State private var looping = false
    @StateObject private var model: ExportPageModel
    init(model: EditorModel) { editor = model; _model = StateObject(wrappedValue: ExportPageModel(editor: model)) }
    var body: some View {
        KRExportPageLayout(settings: { settings }, preview: { preview }, check: { check }, jobs: { jobs })
            .task { await model.load(); await model.check() }
            .onDisappear { playing = false; model.stopPolling() }
            .onChange(of: model.target) { _, _ in playing = false; model.applyWorkAreaDefault(); previewFrame = model.firstFrame; model.previewFailure = nil }
            .onChange(of: model.firstFrame) { _, value in previewFrame = value }
            .onChange(of: model.exclusiveFrame) { _, _ in previewFrame = min(max(model.firstFrame,previewFrame),max(model.firstFrame,model.exclusiveFrame-1)) }
            .task(id: playing) {
                while playing && !Task.isCancelled {
                    do { try await Task.sleep(for: .milliseconds(max(1,1000 / model.nominalFPS))) } catch { return }
                    if previewFrame + 1 >= model.exclusiveFrame { if looping { previewFrame = model.firstFrame } else { playing = false } }
                    else { previewFrame += 1 }
                }
            }
    }
    var settings: some View {
        KRPanel("書き出し設定") {
            VStack(alignment: .leading, spacing: KRSpace.space3) {
                ScrollView {
                    VStack(alignment: .leading, spacing: KRSpace.space3) {
                        KRPopupButton("対象", options: model.targets.map { .init($0.string("id"), $0.string("kind") + " · " + ($0.object("value").string("name").isEmpty ? String($0.object("value").string("id").prefix(8)) : $0.object("value").string("name"))) }, selection: $model.target)
                        KRInspectorSettingRow("範囲") { KRPopupButton("範囲", options: [.init("all", "全体"), .init("inout", "イン〜アウト")], selection: $model.rangeMode).frame(width: 144) }
                        if model.rangeMode == "inout" {
                            KRInspectorSettingRow("In") { WorkflowFrameField(Double(model.startFrame), step: 1, range: 0...Double(max(0,model.totalFrames-1)), unit: "f", width: 120, accessibilityLabel: "書き出し開始フレーム", onCommit: { _, v in model.startFrame = Int64(v) }) }
                            KRInspectorSettingRow("Out (exclusive)") { WorkflowFrameField(Double(model.endFrame), step: 1, range: 1...Double(max(1,model.totalFrames)), unit: "f", width: 120, accessibilityLabel: "書き出し終了フレーム（排他的）", onCommit: { _, v in model.endFrame = Int64(v) }) }
                        }
                        KRPopupButton("プリセット", options: model.profiles.map { .init($0.string("format"), profileLabel($0), disabled: $0.string("device_availability") == "unavailable") }, selection: $model.format, onSelect: { _ in model.selectProfile() })
                        ForEach(model.profiles.filter { $0.string("device_availability") == "unavailable" }, id: \.formatID) { profile in
                            KRErrorLine(.init(profile.object("reason").string("code"), profileLabel(profile) + " は利用できません"))
                        }
                        if model.format == "caption_sidecar" {
                            KRInspectorSettingRow("字幕形式") { KRPopupButton("字幕形式", options: [.init("srt", "SRT"), .init("vtt", "VTT"), .init("itt", "ITT")], selection: $model.captionFormat).frame(width: 144) }
                            Text("対象 sequence のキューをサイドカー化します。フレームは描画しません。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                        } else {
                            KRInspectorSettingRow("Profile version") { KRPopupButton("Profile version", options: (model.selectedProfile["profile_versions"] as? [Int] ?? []).map { .init(String($0), String($0)) }, selection: $model.version).frame(width: 96) }
                            Text("\(String(format:"%.0f",model.extent[0]))×\(String(format:"%.0f",model.extent[1])) · \(model.fpsNum)/\(model.fpsDen) fps · 対象に従う").krText(KRType.ruler).foregroundStyle(p.inkMuted)
                            Text(model.format == "pro_res_hdr_mov" ? "HDR · Rec.2100" : "SDR · Rec.709").krText(KRType.body)
                        }
                        if model.format == "pro_res_hdr_mov" {
                            KRInspectorSettingRow("Transfer") { KRPopupButton("Transfer", options: [.init("pq", "PQ (SMPTE 2084)"), .init("hlg", "HLG")], selection: $model.transfer).frame(width: 160) }
                        }
                        if model.format != "image_sequence" && model.format != "caption_sidecar" {
                            KRInspectorSettingRow("Background") { KRPopupButton("Background", options: [.init("black", "黒（明示）"), .init("white", "白（明示）")], selection: $model.background).frame(width: 144) }
                            KRInspectorSettingRow("Audio") { KRPopupButton("Audio", options: (model.selectedProfile["audio_modes"] as? [String] ?? []).map { .init($0, $0 == "document" ? "作品のミックス" : $0 == "silence" ? "無音" : "明示 clips（空）", disabled: model.format == "pro_res_mov" && model.version == "1" && $0 != "explicit") }, selection: $model.audio).frame(width: 160) }
                            Text((model.selectedProfile["audio_codecs"] as? [String] ?? []).joined(separator: " · ") + " · 48 kHz · stereo\nクリッピングは worker が型付きエラーで停止します。")
                                .krText(KRType.caption).foregroundStyle(p.inkMuted)
                        } else if model.format == "image_sequence" { Text("PNG + RGBA16F + JSON · 音声なし").krText(KRType.caption).foregroundStyle(p.inkMuted) }
                        if model.selectedProfile.string("execution") == "hardware" { Text("hardware 必須 · device の利用可否は投入時に確認します。software へ自動代替しません。").krText(KRType.caption).foregroundStyle(p.inkMuted) }
                        KRTextField("出力先", value: $model.destination)
                        KRButton("選択…", variant: .secondary) { chooseDestination() }
                    }.padding(KRSpace.space3)
                }
                Spacer(minLength: 0)
                KRExportStartButton() { Task { await model.submit() } }.disabled(!model.canSubmit).frame(maxWidth: .infinity).padding(.horizontal, KRSpace.space3)
                Text("rev \(model.checkedRevision ?? editor.revision) の内容を固定し、別プロセスで書き出します。アプリを閉じても続きます。")
                    .krText(KRType.caption).foregroundStyle(p.inkMuted).padding([.horizontal,.bottom], KRSpace.space3)
            }
        }
    }
    var preview: some View {
        KRPanel("プレビュー") {
            VStack(spacing: KRSpace.space2) {
                if let error = model.previewFailure {
                    KRViewerError(.init(error.code,error.message), copy: { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(error.copyText,forType:.string) }, retry: { model.previewFailure = nil }).padding(KRSpace.space3)
                } else { KRViewerFrame(aspectRatio: max(1,model.extent[0]) / max(1,model.extent[1])) {
                    ExportMetalPreview(editor: editor, input: model.input, time: model.time(previewFrame), onFailure: { model.previewFailure = $0 })
                }.padding(KRSpace.space3) }
                HStack {
                    Text(KRTimecode.format(frames: model.firstFrame, fps: model.nominalFPS)).krText(KRType.ruler)
                    Rectangle().fill(p.selectionBg).overlay { Rectangle().strokeBorder(p.selection, lineWidth: 1) }.frame(height: 16)
                    Text(KRTimecode.format(frames: model.exclusiveFrame, fps: model.nominalFPS)).krText(KRType.ruler)
                }.padding(KRSpace.space3)
                HStack(spacing: KRSpace.space2) {
                    KRTimecodeField(frames: $previewFrame, fps: model.nominalFPS, onSeek: { previewFrame = min(max(model.firstFrame,$0),max(model.firstFrame,model.exclusiveFrame-1)) })
                    Spacer(minLength: 0)
                    KRButton(icon: .skipBack, accessibilityLabel: "範囲の先頭へ") { previewFrame = model.firstFrame }
                    KRButton(icon: .stepBack, accessibilityLabel: "1 フレーム戻る") { previewFrame = max(model.firstFrame,previewFrame-1) }
                    KRButton(icon: playing ? .pause : .play, accessibilityLabel: playing ? "一時停止" : "映像を再生") { playing.toggle() }
                    KRButton(icon: .stepForward, accessibilityLabel: "1 フレーム進む") { previewFrame = min(max(model.firstFrame,model.exclusiveFrame-1),previewFrame+1) }
                    KRButton(icon: .skipForward, accessibilityLabel: "範囲の末尾へ") { previewFrame = max(model.firstFrame,model.exclusiveFrame-1) }
                    KRButton(icon: .repeat, accessibilityLabel: "ループ再生", pressed: looping) { looping.toggle() }
                }.padding(.horizontal, KRSpace.space3)
                Text("映像プレビュー · Out は区間の終端（含まない）").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.bottom, KRSpace.space3)
            }
        }
    }
    var check: some View {
        KRPanel("書き出し前の確認", actions: { KRButton(icon: .refreshCw, accessibilityLabel: "もう一度確認") { Task { await model.check() } }.disabled(model.checking) }) {
            ScrollView {
                VStack(alignment: .leading, spacing: KRSpace.space3) {
                    if let conflict = model.diagnostics.first(where: { $0.code == "REVISION_CONFLICT" }) {
                        KRConflictBanner(.init(conflict.code,conflict.message), reapplyLabel: "再確認", discard: { model.clearInspection() }, reapply: { Task { try? await editor.reload(); await model.check() } })
                    }
                    if model.checking { HStack { KRActivityIndicator(); Text("意味的検査中").krText(KRType.body) } }
                    if !model.errors.isEmpty {
                        HStack(alignment: .top) { KRIconView(.triangleAlert).foregroundStyle(p.danger); Text("エラーが \(model.errors.count) 件あります。解消するまで書き出せません。").krText(KRType.body) }.padding(KRSpace.space2).background(p.surface200)
                        ForEach(Array(model.errors.enumerated()), id: \.offset) { _, error in KRErrorLine(.init(error.code,error.message)) }
                    } else if model.canSubmit { HStack { KRIconView(.circleCheck); Text("確認した時刻に意味的エラーはありません").krText(KRType.body) }.foregroundStyle(p.inkMuted) }
                    Text("範囲の先頭時刻で render.explain を実行します。全フレーム・音声のクリッピング・encoder の起動を保証する検査ではありません。")
                        .krText(KRType.caption).foregroundStyle(p.inkMuted)
                    Text("フォントは作品の版固定と明示したローカル入力を使用します。代替フォントは使いません。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                    Text("\(max(0,model.exclusiveFrame-model.firstFrame)) frames · ファイルサイズは未測定").krText(KRType.ruler).foregroundStyle(p.inkMuted)
                    if !model.canSubmit && !model.checking { KRButton("再確認", variant: .secondary) { Task { await model.check() } } }
                }.padding(KRSpace.space3)
            }
        }
    }
    var jobs: some View {
        KRPanel(header: {
            KRPanelTitle("ジョブ")
            KRSegmentedControl([.init("all", "すべて"), .init("active", "実行中"), .init("done", "完了"), .init("failed", "失敗")], selection: $model.filter)
        }, actions: {
            KRCheckbox("完了した記録を隠す", isOn: $model.hideCompleted)
            KRButton(icon: .refreshCw, accessibilityLabel: "ジョブを更新") { Task { await model.refreshJobs(); model.beginPollingIfActive() } }
        }) {
            VStack(spacing: 0) {
                if let error = model.jobFailure { KRErrorLine(.init(error.code,error.message)).padding(KRSpace.space2) }
                ScrollView {
                    VStack(spacing: 0) {
                        if model.visibleJobs.isEmpty { Text("表示するジョブはありません").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(KRSpace.space3) }
                        ForEach(model.visibleJobs, id: \.selfID) { job in
                            KRJobRow(URL(fileURLWithPath: job.string("destination")).lastPathComponent,
                                detail: job.object("output_profile").string("format") + " · rev " + job.string("revision") + " · " + String(job.string("snapshot_hash").prefix(8)), state: ExportPageModel.state(job)) {
                                if ["queued","running"].contains(job.string("status")) {
                                    KRButton("中止", variant: .plain) { Task { await model.cancel(job.string("id")) } }.disabled(job["cancel_requested"] as? Bool == true)
                                } else if job.string("status") == "succeeded" {
                                    KRButton("表示", variant: .plain) { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: job.string("destination"))]) }
                                } else if ["failed","interrupted"].contains(job.string("status")) {
                                    KRButton("設定を確認", variant: .plain) { model.destination = job.string("destination"); Task { await model.check() } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    func profileLabel(_ profile: [String: Any]) -> String {
        switch profile.string("format") { case "pro_res_mov": return "ProRes 422 HQ · MOV · PCM24"; case "pro_res_sdr_from_hdr_mov": return "ProRes 422 HQ · MOV · HDR→SDR"; case "pro_res_hdr_mov": return "ProRes 422 HQ · MOV · HDR"; case "av1_mp4": return "AV1 · MP4 · ALAC/AAC"; case "av1_webm": return "AV1 · WebM · Opus"; case "h264_mov": return "H.264 · MOV · ALAC/AAC · hardware"; case "hevc_mov": return "HEVC · MOV · ALAC/AAC · hardware"; case "image_sequence": return "画像連番 · PNG + JSON"; case "caption_sidecar": return "字幕サイドカー · SRT/VTT/ITT"; default: return profile.string("format") }
    }
    func chooseDestination() {
        let panel = NSSavePanel(); panel.title = "書き出し先を選択"
        let ext = model.format == "caption_sidecar" ? model.captionFormat : model.selectedProfile.string("container_extension")
        panel.nameFieldStringValue = "export" + (ext.isEmpty ? "" : "." + ext)
        if panel.runModal() == .OK, let url = panel.url { model.destination = url.path }
    }
}
private extension Dictionary where Key == String, Value == Any { var formatID: String { string("format") } }
