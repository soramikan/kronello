import AppKit
import SwiftUI
import UniformTypeIdentifiers
import KronelloAppModel
import KronelloDesign

private let projectAssetType = UTType(exportedAs: "com.kronello.project-asset", conformingTo: .data)

struct EditPage: View {
    @ObservedObject var model: EditorModel
    var body: some View {
        KREditLayout(project: { EditProjectPanel(model: model) }, viewer: { SequenceViewer(model: model) },
                     inspector: { ClipInspector(model: model) }, tracks: { SequenceTracks(model: model) })
            .onAppear { model.activatePlayback(for: model.sequence) }
            .onChange(of: model.sequence.string("id") + model.activeRate.string("num") + "/" + model.activeRate.string("den")) { _, _ in model.activatePlayback(for: model.sequence) }
    }
}

struct EditProjectPanel: View {
    @ObservedObject var model: EditorModel
    @State private var tab = "project"
    @State private var search = ""
    var body: some View {
        KRPanel(header: { KRTabBar([.init("project", "Project"), .init("effects", "Effects")], selection: $tab) }, actions: {}) {
            VStack(spacing: 0) {
                KRSearchField("素材を検索", text: $search).padding(KRSpace.space2)
                if tab == "effects" {
                    VStack(alignment: .leading, spacing: KRSpace.space3) {
                        KRButton("Gaussian Blur", icon: .sparkles) { model.addClipEffect("blur") }
                        KRButton("Drop Shadow", icon: .layers) { model.addClipEffect("shadow") }
                        KRButton("Chroma Key", icon: .sparkles) { model.addClipEffect("chroma_key") }
                        KRButton("Luma Key", icon: .sparkles) { model.addClipEffect("luma_key") }
                        KRButton("Glow", icon: .sparkles) { model.addClipEffect("glow") }
                        KRButton("Sharpen", icon: .sparkles) { model.addClipEffect("sharpen") }
                        KRButton("Vignette", icon: .sparkles) { model.addClipEffect("vignette") }
                        KRButton("Corner Pin", icon: .sparkles) { model.addClipEffect("corner_pin") }
                        Text("選択中の映像クリップに追加します。").krText(KRType.caption)
                    }.padding().disabled(model.selectedClip == nil || model.selectedClip?.kind == .audio || model.busy || model.pendingCandidate != nil)
                }
                else {
                    ScrollView(.vertical) {
                            VStack(spacing: 0) {
                                ForEach(model.editAssets.filter { search.isEmpty || $0.name.localizedCaseInsensitiveContains(search) }) { asset in
                                    KRAssetRow(asset.name, kind: asset.kind, meta: asset.meta,
                                        duration: KRTimecode.format(frames: max(0, asset.duration.frames(rateNum: model.rateNum, rateDen: model.rateDen)), fps: model.nominalFPS),
                                        selected: model.assetSelection == asset.id, missing: asset.missing,
                                        onSelect: { model.assetSelection = asset.id }, onOpen: {
                                            if let composition = asset.source["composition"] as? String { model.ui.page = "motion"; model.setComposition(composition) }
                                        })
                                        .onDrag {
                                            if ProcessInfo.processInfo.environment["KRONELLO_TRACE_ASSET_DRAG"] == "1" { NSLog("asset drag SwiftUI provider") }
                                            model.assetSelection = asset.id; model.cancelClipGesture()
                                            let provider = NSItemProvider()
                                            provider.registerDataRepresentation(forTypeIdentifier: projectAssetType.identifier, visibility: .ownProcess) { completion in
                                                completion(Data(asset.id.utf8), nil); return nil
                                            }
                                            return provider
                                        }
                                        .disabled(model.busy || model.pendingCandidate != nil)
                                }
                            }
                    }
                }
                Spacer(minLength: 0)
            }
        }
    }
}

struct SequenceViewer: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    @State private var safeArea = false
    @State private var panOrigin: CGPoint?
    @State private var toolPlacement = KRToolStripPlacement()
    static let tools: [KRTool] = [
        .init("select", icon: .mousePointer2, name: "選択", shortcut: "V"),
        .init("blade", icon: .scissors, name: "ブレード", shortcut: "B"),
        .init("slip", icon: .slidersHorizontal, name: "スリップ", shortcut: "Y", separatorBefore: true),
        .init("slide", icon: .chevronsUpDown, name: "スライド", shortcut: "U"),
        .init("roll", icon: .repeat, name: "ロール", shortcut: "N"),
        .init("hand", icon: .hand, name: "手のひら", shortcut: "H", separatorBefore: true),
        .init("zoom", icon: .zoomIn, name: "ズーム", shortcut: "Z", unavailableReason: "表示倍率は下の欄で変更します")]
    var body: some View {
        KRPanel(header: {
            KRTabBar(model.document.objects("sequences").enumerated().map { .init($0.element.string("id"), "Sequence \($0.offset + 1)") },
                     selection: Binding(get: { model.ui.sequence ?? "" }, set: { model.setSequence($0) }))
        }, actions: { HStack {
            if model.usesCPUReference { Text("CPU 参照").krText(KRType.caption).foregroundStyle(p.inkMuted) }
            KRButton(icon: .scan, accessibilityLabel: "セーフエリア", pressed: safeArea) { safeArea.toggle() }
        } }) {
            VStack(spacing: 0) {
                if let conflict = model.revisionConflict { KRConflictBanner(.init(conflict.code, conflict.message), discard: model.discardCandidate, reapply: model.reapply) }
                if let text = model.deletedSelection { KRStateBand(text) }
                HStack(spacing: 0) {
                    KRToolStrip(Self.tools, selection: $model.editTool, placement: $toolPlacement)
                    ZStack {
                        if model.ui.sequence == nil { VStack(spacing: KRSpace.space3) {
                            KREmptyState(icon: .clapperboard, title: "Sequence がありません", message: "空のシーケンス（映像・音声トラック各 1 本）を作成できます。")
                            KRButton("シーケンスを作成", variant: .secondary) { model.createSequence() }.disabled(model.busy || model.pendingCandidate != nil)
                        } }
                        else {
                            KRViewerFrame(aspectRatio: max(1, model.extent.width) / max(1, model.extent.height)) {
                                MetalPreview(model: model)
                                if safeArea { Rectangle().strokeBorder(p.inkMuted, style: .init(lineWidth: 1, dash: [4, 4])).padding(24).allowsHitTesting(false) }
                            }.offset(x: model.editViewSettings.panX, y: model.editViewSettings.panY)
                                .contentShape(Rectangle())
                                .gesture(DragGesture().onChanged { value in
                                    guard model.editTool == "hand" else { return }
                                    if panOrigin == nil { panOrigin = .init(x: model.editViewSettings.panX, y: model.editViewSettings.panY) }
                                    var settings = model.editViewSettings
                                    settings.panX = (panOrigin?.x ?? 0) + value.translation.width
                                    settings.panY = (panOrigin?.y ?? 0) + value.translation.height
                                    model.editViewSettings = settings
                                }.onEnded { _ in panOrigin = nil })
                                .padding(KRSpace.space3).clipped()
                        }
                        if let failure = model.sequenceFailure ?? model.previewFailure {
                            VStack(spacing: KRSpace.space3) {
                                KRViewerError(.init(failure.code, failure.message), copy: { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(failure.copyText, forType: .string) },
                                              retry: { model.previewFailure = nil; Task { do { try await model.reload() } catch { model.mapFailure(error) } } })
                                if model.offersCPUReference { KRButton("CPU 参照で表示", variant: .secondary) { model.chooseCPUReference() } }
                            }
                        }
                        if model.sequenceLoading || model.busy || model.previewRendering || model.previewStale { VStack { HStack {
                            if model.sequenceLoading || model.busy || model.previewRendering { KRActivityIndicator() }
                            Text(model.usesCPUReference && model.playing ? "CPU 参照 · 再生中は前回のフレームを表示" : "更新中 · 表示は前回の結果です").krText(KRType.caption).foregroundStyle(p.inkMuted); Spacer()
                        }; Spacer() }.padding(KRSpace.space3).allowsHitTesting(false) }
                    }.background(p.surface0)
                }
                KRTransportBar(frames: Binding(get: { model.frame }, set: { model.seek($0) }), fps: model.nominalFPS, duration: model.durationCode,
                    playing: $model.playing, looping: $model.ui.looping, zoom: $model.ui.zoom, resolution: $model.ui.resolution,
                    onStep: { model.seek(model.frame + Int64($0)) }, onBoundary: { model.seek($0 ? model.durationFrames - 1 : 0) })
            }
        }
    }
}

struct ClipInspector: View {
    @State private var draftBases: [String: String] = [:]
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    var body: some View {
        KRPanel("Inspector") {
            if let marker = model.selectedMarker {
                ScrollView {
                    VStack(alignment: .leading, spacing: KRSpace.space4) {
                        HStack(spacing: KRSpace.space2) {
                            MarkerDiamond(color: SequenceTracks.markerColor(marker.color)).frame(width: 10, height: 10)
                            Text(marker.comment.isEmpty ? "マーカー" : marker.comment).krText(KRType.heading).lineLimit(1)
                        }.padding(.horizontal, KRSpace.space3)
                        Text(marker.clip == nil ? "シーケンスマーカー" : "クリップマーカー").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                        section("位置") {
                            timeRow("時間", frames: marker.time.frames(rateNum: model.rateNum, rateDen: model.rateDen), action: nil)
                        }
                        section("色") {
                            KRInspectorSettingRow("色") {
                                KRPopupButton("色", options: EditMarker.colors.map { KRPopupOption($0, $0.capitalized) },
                                    selection: Binding(get: { marker.color }, set: { model.recolorMarker(marker, color: $0) }))
                            }
                        }
                        KRButton("マーカーを削除", icon: .trash2, variant: .destructive) { model.removeMarker(marker) }.padding(.horizontal, KRSpace.space3)
                        KRButton("再生ヘッドを移動", icon: .mousePointer2, variant: .secondary) { model.seek(marker.time.frames(rateNum: model.rateNum, rateDen: model.rateDen)) }.padding(.horizontal, KRSpace.space3)
                    }.padding(.vertical, KRSpace.space3)
                }
            } else if let clip = model.selectedClip {
                ScrollView {
                    VStack(alignment: .leading, spacing: KRSpace.space4) {
                        HStack(spacing: KRSpace.space2) { KRIconView(clip.kind.icon).foregroundStyle(clip.kind.color(in: p)); Text(model.clipName(clip)).krText(KRType.heading).lineLimit(1) }.padding(.horizontal, KRSpace.space3)
                        Text("\(clip.kind.rawValue.capitalized) クリップ · \(model.trackNumber(clip.track))").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                        if let missing = model.clipMissing(clip) { KRErrorLine(.init(missing, "参照素材を確認してください。")).padding(.horizontal, KRSpace.space3) }
                        if let reason = clip.query["unsupported_reason"] as? String { KRErrorLine(.init("UNSUPPORTED_FEATURE", reason)).padding(.horizontal, KRSpace.space3) }
                        section("配置") {
                            timeRow("開始", frames: clip.start.frames(rateNum: model.rateNum, rateDen: model.rateDen)) { value in
                                model.beginClipGesture(clip, mode: .move); model.updateClipGesture(at: value); Task { await model.commitClipGesture() }
                            }
                            timeRow("尺", frames: clip.end.frames(rateNum: model.rateNum, rateDen: model.rateDen) - clip.start.frames(rateNum: model.rateNum, rateDen: model.rateDen)) { value in
                                model.beginClipGesture(clip, mode: .trimEnd); model.updateClipGesture(delta: value - (clip.end.frames(rateNum: model.rateNum, rateDen: model.rateDen) - clip.start.frames(rateNum: model.rateNum, rateDen: model.rateDen))); Task { await model.commitClipGesture() }
                            }
                            if clip.kind != .subtitle {
                                timeRow("ソース開始", frames: RationalTime.wire(clip.authored.object("source_in")).frames(rateNum: model.rateNum, rateDen: model.rateDen), onEditingStart: { draftBases["source"] = model.revision }) { model.setClipTime(clip, sourceIn: model.frameTime($0), base: draftBases.removeValue(forKey: "source")) }
                            }
                        }
                        if clip.kind == .subtitle { section("字幕") { CaptionInspector(model: model, clip: clip) } }
                        if clip.kind != .subtitle { section("時間") {
                            KRInspectorSettingRow("速度") {
                                KRNumberField(value: .constant(clip.linearRate.map { Double($0.num)! / Double($0.den)! * 100 } ?? 100), unit: "%", step: 0.1, range: 0.1...10000, accessibilityLabel: "速度", onEditingStart: { draftBases["speed"] = model.revision }, onCommit: { _, value in model.setClipTime(clip, speedPercent: value, base: draftBases.removeValue(forKey: "speed")) }).disabled(clip.linearRate == nil)
                            }
                            KRInspectorSettingRow("逆再生") { KRCheckbox("", isOn: Binding(get: { clip.reversed }, set: { model.setClipReverse(clip, enabled: $0) })).accessibilityLabel("逆再生").disabled(clip.linearRate == nil) }
                            if clip.linearRate == nil {
                                // NLE-006: piecewise maps (speed ramps and freeze holds) are
                                // shown read-only; percent/reverse edits need a Linear map.
                                Text("\(clip.speedLabel)：非線形タイムマップです。").krText(KRType.caption).foregroundStyle(p.accentInk).padding(.horizontal, KRSpace.space3)
                            }
                            Text("速度は0.1%単位。配置の尺を維持し、逆再生は選択区間の末尾から始めます。").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                        } }
                        if clip.kind == .audio { section("音量") {
                            KRInspectorSettingRow("音量") {
                                KRNumberField(value: .constant(EditorModel.clipVolume(clip) * 100), unit: "%", step: 1, range: 0...800, accessibilityLabel: "音量", onEditingStart: { draftBases["volume"] = model.revision }, onCommit: { _, value in model.setClipVolume(clip, value: value / 100, base: draftBases.removeValue(forKey: "volume")) })
                            }
                            Text("kronello.audio.volume の線形ゲイン。100% が等倍です。").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                        } }
                        section("合成") {
                            KRInspectorSettingRow("不透明度") {
                                KRNumberField(value: .constant(clip.authored.objects("properties").first { $0.object("descriptor").string("key") == "kronello.opacity" }?.object("source").object("value").number("value") ?? 1), unit: "", step: 0.01, range: 0...1, precision: 2, accessibilityLabel: "不透明度", onEditingStart: { draftBases["opacity"] = model.revision }, onCommit: { _, value in model.setClipProperty(clip, key: "kronello.opacity", kind: "scalar", value: value, base: draftBases.removeValue(forKey: "opacity")) })
                            }
                            KRInspectorSettingRow("描画モード") {
                                KRPopupButton("描画モード", options: EditPresentation.blendModes.map { .init($0.wire, $0.label) }, selection: Binding(get: { clip.authored.objects("properties").first { $0.object("descriptor").string("key") == "kronello.blend_mode" }?.object("source").object("value").string("value") ?? "normal" }, set: { model.setClipProperty(clip, key: "kronello.blend_mode", kind: "enum", value: $0, base: draftBases.removeValue(forKey: "blend")) }), onEditingStart: { draftBases["blend"] = model.revision })
                            }
                        }.disabled(clip.kind == .audio)
                        section("Effects") {
                            ForEach(Array(clip.authored.objects("effects").enumerated()), id: \.offset) { index, effect in
                                VStack(alignment: .leading, spacing: 0) {
                                    HStack { Text(ClipColorInspector.names[effect.string("effect_id")] ?? Self.effectName(effect.string("effect_id"))).krText(KRType.label); Spacer(); KRButton(icon: .trash2, accessibilityLabel: "効果を削除") { model.removeClipEffect(clip, index: index) } }.padding(.horizontal, KRSpace.space3)
                                    if effect.string("effect_id") == "kronello.keying.chroma" {
                                        KRInspectorSettingRow("キー色") {
                                            ClipEffectColorEditor(model: model, clip: clip, effect: effect, parameter: "key_color")
                                        }
                                    }
                                    ForEach(Array(Self.effectRows(effect.string("effect_id")).enumerated()), id: \.offset) { _, row in
                                        let (field, label, unit, range, step) = row
                                        if Self.vec2Fields.contains(field) {
                                            KRInspectorSettingRow(label) {
                                                HStack(spacing: KRSpace.space1) {
                                                    ForEach(0..<2, id: \.self) { axis in
                                                        KRInspectorAxis(axis == 0 ? "X" : "Y") {
                                                            KRNumberField(value: .constant((EditorModel.vec2ParameterValue(clip, effect: effect, parameter: field) ?? [0, 0])[axis]), step: step,
                                                                accessibilityLabel: label + (axis == 0 ? " X" : " Y"), onEditingStart: { draftBases[field] = model.revision },
                                                                onCommit: { _, value in
                                                                    var pair = EditorModel.vec2ParameterValue(clip, effect: effect, parameter: field) ?? [0, 0]
                                                                    pair[axis] = value
                                                                    model.setClipEffectParameter(clip, effect: effect, parameter: field, kind: "vec2", value: pair, base: draftBases.removeValue(forKey: field))
                                                                })
                                                        }
                                                    }
                                                }
                                            }
                                        } else {
                                            KRInspectorSettingRow(label) {
                                                KRNumberField(value: .constant(EditorModel.colorParameterValue(clip, effect: effect, parameter: field) ?? 0), unit: unit, step: step, range: range, precision: 2,
                                                    accessibilityLabel: label, onEditingStart: { draftBases[field] = model.revision },
                                                    onCommit: { _, value in model.setClipEffectParameter(clip, effect: effect, parameter: field, kind: "scalar", value: value, base: draftBases.removeValue(forKey: field)) })
                                            }
                                        }
                                    }
                                }
                            }
                        }.disabled(clip.kind == .audio)
                        if clip.kind != .audio { section("カラー") { ClipColorInspector(model: model, clip: clip) } }
                        if clip.composition != nil { KRButton("モーションで開く", icon: .layers, variant: .secondary) { model.openClipInMotion(clip) }.padding(.horizontal, KRSpace.space3) }
                        section("編集") {
                            KRInspectorSettingRow("有効") {
                                KRCheckbox("", isOn: Binding(get: { clip.enabled }, set: { model.setClipEnabled(clip, enabled: $0) }))
                                    .accessibilityLabel("クリップの有効")
                                    .disabled(model.trackLocked(clip.track) || model.busy || model.pendingCandidate != nil)
                            }
                            Text("無効なクリップは区間を保ったまま映像・音声・字幕・トランジションに寄与しません。").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                            if clip.kind != .subtitle {
                                KRButton("再生ヘッドでフリーズ", icon: .pause, variant: .secondary) { model.freezeClipAtPlayhead(clip) }
                                    .disabled(model.busy || model.pendingCandidate != nil || model.trackLocked(clip.track)
                                        || !(model.frame > clip.start.frames(rateNum: model.rateNum, rateDen: model.rateDen)
                                            && model.frame < clip.end.frames(rateNum: model.rateNum, rateDen: model.rateDen)))
                                    .padding(.horizontal, KRSpace.space3)
                            }
                            KRButton("再生ヘッドにクリップマーカー", icon: .circle, variant: .secondary) { model.addClipMarker(clip) }
                                .disabled(model.busy || model.pendingCandidate != nil)
                                .padding(.horizontal, KRSpace.space3)
                            KRButton("クリップを削除", icon: .trash2, variant: .destructive) { model.deleteSelectedClip() }
                                .disabled(model.busy || model.pendingCandidate != nil)
                                .padding(.horizontal, KRSpace.space3)
                            KRButton("リップル削除（隙間を詰める）", icon: .trash2, variant: .destructive) { model.deleteSelectedClip(ripple: true) }
                                .disabled(model.busy || model.pendingCandidate != nil)
                                .padding(.horizontal, KRSpace.space3)
                            Text("リップル削除は対象クリップの区間を全トラックから詰めます。⌥⌫ でも実行できます。").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                        }
                    }.padding(.vertical, KRSpace.space3)
                }
            } else { KREmptyState(icon: .mousePointer2, title: "クリップを選択", message: "トラックでクリップを選択してください。") }
        }.onChange(of: model.selectedClip?.id) { _, _ in draftBases.removeAll() }
    }
    func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: KRSpace.space1) { Text(title).krText(KRType.heading).padding(.horizontal, KRSpace.space3); content() }
    }
    func timeRow(_ title: String, frames: Int64, onEditingStart: @escaping () -> Void = {}, action: ((Int64) -> Void)?) -> some View {
        KRInspectorSettingRow(title) {
            KRTimecodeField(frames: .constant(max(0, frames)), fps: model.nominalFPS, currentTime: false, label: title, onEditingStart: onEditingStart, onSeek: { action?($0) })
                .disabled(action == nil || model.busy || model.pendingCandidate != nil)
        }
    }
    /// Display names for non-color clip effects.
    static func effectName(_ id: String) -> String {
        switch id {
        case "kronello.gaussian_blur": return "Gaussian Blur"
        case "kronello.drop_shadow": return "Drop Shadow"
        case "kronello.audio.gain": return "Audio Gain"
        case "kronello.keying.chroma": return "Chroma Key"
        case "kronello.keying.luma": return "Luma Key"
        case "kronello.glow": return "Glow"
        case "kronello.sharpen": return "Sharpen"
        case "kronello.vignette": return "Vignette"
        case "kronello.corner_pin": return "Corner Pin"
        default: return id
        }
    }
    /// Parameters that render as an X/Y vec2 pair instead of a scalar field.
    static let vec2Fields: Set<String> = ["offset", "top_left", "top_right", "bottom_right", "bottom_left"]
    /// Editable constant parameters of a clip effect: (parameter field, row
    /// label, unit, range, step). Fields in `vec2Fields` render as a pair.
    static func effectRows(_ id: String) -> [(String, String, String, ClosedRange<Double>, Double)] {
        switch id {
        case "kronello.gaussian_blur":
            return [("sigma", "ぼかし", "px", 0...256, 0.1)]
        case "kronello.drop_shadow":
            return [("sigma", "ぼかし", "px", 0...256, 0.1), ("offset", "オフセット", "px", -512...512, 0.5),
                    ("opacity", "不透明度", "", 0...1, 0.01)]
        case "kronello.keying.chroma":
            return [("similarity", "類似度", "", 0...1, 0.01), ("edge_shrink", "縮小", "px", 0...64, 0.1),
                    ("edge_feather", "ぼかし", "px", 0...64, 0.1), ("spill", "スピル除去", "", 0...1, 0.01)]
        case "kronello.keying.luma":
            return [("key_luma", "キー輝度", "", 0...1, 0.01), ("tolerance", "許容度", "", 0...1, 0.01),
                    ("edge_shrink", "縮小", "px", 0...64, 0.1), ("edge_feather", "ぼかし", "px", 0...64, 0.1)]
        case "kronello.glow":
            return [("threshold", "しきい値", "", 0...4, 0.01), ("radius", "半径", "px", 0...256, 0.1),
                    ("intensity", "強度", "×", 0...8, 0.01)]
        case "kronello.sharpen":
            return [("amount", "量", "×", 0...8, 0.01), ("radius", "半径", "px", 0...64, 0.1)]
        case "kronello.vignette":
            return [("amount", "量", "", 0...1, 0.01), ("midpoint", "中間点", "", 0...1, 0.01),
                    ("feather", "ぼかし", "", 0...4, 0.01), ("roundness", "丸み", "", 0...1, 0.01)]
        case "kronello.corner_pin":
            return [("top_left", "左上", "px", -8192...8192, 1), ("top_right", "右上", "px", -8192...8192, 1),
                    ("bottom_right", "右下", "px", -8192...8192, 1), ("bottom_left", "左下", "px", -8192...8192, 1)]
        default: return []
        }
    }
    func opacity(_ clip: EditClip) -> String {
        guard let property = clip.authored.objects("properties").first(where: { $0.object("descriptor").string("key") == "kronello.opacity" }) else { return "100.0%" }
        let source = property.object("source")
        guard source.string("kind") == "constant" else { return source.string("kind") == "curve" ? "アニメーション" : "Expression" }
        return String(format: "%.1f%%", source.object("value").number("value") * 100)
    }
}

struct SequenceTracks: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    @FocusState private var tracksFocused: Bool
    var body: some View {
        KRPanel(header: { HStack(spacing: KRSpace.space2) {
            KRPanelTitle("Sequence")
            Text("\(Int(model.extent.width))×\(Int(model.extent.height)) · \(EditPresentation.rateLabel(num: model.rateNum, den: model.rateDen)) · \(Int(model.sequence.number("audio_rate")) / 1000) kHz").krText(KRType.caption).foregroundStyle(p.inkMuted)
            Text(model.timecode).krText(KRType.timecode).foregroundStyle(p.accentInk)
        } }, actions: {
            KRButton(icon: .captions, accessibilityLabel: "字幕を追加") { model.addCaption() }
                .disabled(model.busy || model.pendingCandidate != nil || model.ui.sequence == nil)
            KRButton(icon: .magnet, accessibilityLabel: "スナップ", pressed: model.editSnap) { model.editSnap.toggle() }
            KRButton(icon: .circle, accessibilityLabel: "再生ヘッドにマーカーを追加") { model.addSequenceMarker() }
            KRButton("In", variant: .plain) { model.setInPoint() }
                .disabled(model.busy || model.pendingCandidate != nil)
            KRButton("Out", variant: .plain) { model.setOutPoint() }
                .disabled(model.busy || model.pendingCandidate != nil)
            KRButton(icon: .x, accessibilityLabel: "In/Out を解除") { model.clearWorkArea() }
                .disabled(model.workArea == nil)
            KRButton(icon: .chevronsLeft, accessibilityLabel: "時間軸を縮小") { model.editScale = max(1, model.editScale / 2) }
            KRButton(icon: .plus, accessibilityLabel: "時間軸を拡大") { model.editScale = min(16, model.editScale * 2) }
        }) {
            GeometryReader { proxy in
                let laneWidth = max(320, proxy.size.width - 200) * model.editScale
                let frameWidth = (laneWidth - KRSpace.space2 * 2) / Double(max(model.durationFrames, Int64(model.nominalFPS * 5)))
                ScrollView([.horizontal, .vertical]) {
                    ZStack(alignment: .topLeading) {
                        VStack(spacing: 0) {
                            HStack(spacing: 0) {
                                Color.clear.frame(width: 200, height: KRSize.rulerHeight)
                                rulerRow(width: laneWidth, frameWidth: frameWidth)
                            }
                            ForEach(model.orderedTracks, id: \.selfID) { track in
                                lane(track, width: laneWidth, frameWidth: frameWidth)
                            }
                        }
                        if let area = workAreaFrames() {
                            p.selection.frame(width: 1).frame(height: KRSize.rulerHeight + CGFloat(model.orderedTracks.count) * KRSize.trackHeight)
                                .offset(x: 200 + KRSpace.space2 + Double(area.start) * frameWidth).allowsHitTesting(false)
                            p.selection.frame(width: 1).frame(height: KRSize.rulerHeight + CGFloat(model.orderedTracks.count) * KRSize.trackHeight)
                                .offset(x: 200 + KRSpace.space2 + Double(area.end) * frameWidth).allowsHitTesting(false)
                        }
                        KRPlayhead().frame(height: KRSize.rulerHeight + CGFloat(model.orderedTracks.count) * KRSize.trackHeight)
                            .offset(x: 200 + KRSpace.space2 + Double(model.frame) * frameWidth - 6)
                    }.frame(width: 200 + laneWidth, alignment: .topLeading).coordinateSpace(name: "sequenceTracks")
                }
            }
        }
        .focusable().focused($tracksFocused)
        .overlay { if tracksFocused { Rectangle().strokeBorder(p.selection, lineWidth: 2).allowsHitTesting(false) } }
        .onKeyPress { key in
            guard tracksFocused else { return .ignored }
            switch key.key {
            case .escape: model.cancelClipGesture(); model.selectMarker(nil)
            case .space: model.playing.toggle()
            case .leftArrow: model.seek(model.frame - 1)
            case .rightArrow: model.seek(model.frame + 1)
            case .upArrow: model.jumpToTimelineBoundary(forward: false)
            case .downArrow: model.jumpToTimelineBoundary(forward: true)
            case .delete, .deleteForward:
                if let marker = model.selectedMarker { model.removeMarker(marker) }
                else if key.modifiers.contains(.option) { model.deleteSelectedClip(ripple: true) }
                else { model.deleteSelectedClip() }
            case "v": model.editTool = "select"
            case "b": model.editTool = "blade"
            case "y": model.editTool = "slip"
            case "u": model.editTool = "slide"
            case "n": model.editTool = "roll"
            case "h": model.editTool = "hand"
            case "m":
                if key.modifiers.contains(.shift), let clip = model.selectedClip { model.addClipMarker(clip) }
                else { model.addSequenceMarker() }
            case "i": model.setInPoint()
            case "o": model.setOutPoint()
            case "x": if key.modifiers.contains(.option) { model.clearWorkArea() } else { return .ignored }
            default: return .ignored
            }
            return .handled
        }
    }
    func workAreaFrames() -> (start: Int64, end: Int64)? {
        guard let area = model.workArea else { return nil }
        return (area.start.frames(rateNum: model.rateNum, rateDen: model.rateDen),
                area.end.frames(rateNum: model.rateNum, rateDen: model.rateDen))
    }
    /// Ruler ticks, the work-area band, and draggable sequence markers.
    /// A double-click on an empty ruler area adds a marker at that frame.
    func rulerRow(width: Double, frameWidth: Double) -> some View {
        ZStack(alignment: .topLeading) {
            if let area = workAreaFrames() {
                p.selection.opacity(0.18)
                    .frame(width: max(0, Double(area.end - area.start) * frameWidth), height: KRSize.rulerHeight)
                    .offset(x: KRSpace.space2 + Double(area.start) * frameWidth)
                    .allowsHitTesting(false)
            }
            KRRuler(rulerTicks(width: width)).allowsHitTesting(false)
            Color.clear.contentShape(Rectangle())
                .simultaneousGesture(SpatialTapGesture(count: 2).onEnded { value in
                    let frame = Int64(max(0, (value.location.x - KRSpace.space2) / frameWidth).rounded())
                    model.addSequenceMarker(at: model.snappedFrame(frame))
                })
                .gesture(DragGesture(minimumDistance: 0).onChanged { value in
                    let frame = Int64(max(0, (value.location.x - KRSpace.space2) / frameWidth).rounded())
                    model.seek(model.snappedFrame(frame))
                })
            ForEach(model.sequenceMarkers) { marker in
                markerGlyph(marker, frameWidth: frameWidth)
            }
        }.frame(width: width, height: KRSize.rulerHeight)
    }
    /// Marker diamond: click selects and seeks, drag issues marker_move on release.
    func markerGlyph(_ marker: EditMarker, frameWidth: Double) -> some View {
        let markerFrame = marker.time.frames(rateNum: model.rateNum, rateDen: model.rateDen)
        let shown = model.markerDrag?.id == marker.id ? model.markerDrag?.frame ?? markerFrame : markerFrame
        return MarkerDiamond(color: Self.markerColor(marker.color), selected: model.markerSelection == marker.id)
            .frame(width: 9, height: 9)
            .offset(x: KRSpace.space2 + Double(shown) * frameWidth - 4.5, y: 2)
            .padding(4)
            .contentShape(Rectangle())
            .padding(-4)
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { value in
                    if model.markerDrag == nil { model.beginMarkerDrag(marker) }
                    model.updateMarkerDrag(to: markerFrame + Int64((value.translation.width / frameWidth).rounded()))
                }
                .onEnded { value in
                    if abs(value.translation.width) < 2 {
                        model.markerDrag = nil
                        model.selectMarker(marker)
                        model.seek(markerFrame)
                    } else {
                        model.commitMarkerDrag()
                    }
                })
            .accessibilityElement()
            .accessibilityLabel(marker.comment.isEmpty ? "マーカー" : "マーカー \(marker.comment)")
            .accessibilityValue(KRTimecode.format(frames: markerFrame, fps: model.nominalFPS))
            .help(marker.comment.isEmpty ? "マーカー" : marker.comment)
    }
    static func markerColor(_ name: String) -> Color {
        switch name {
        case "green": return .green
        case "blue": return .blue
        case "yellow": return .yellow
        case "purple": return .purple
        case "cyan": return .cyan
        case "orange": return .orange
        case "white": return .white
        default: return .red
        }
    }
    func rulerTicks(width: Double) -> [KRRulerTick] {
        let total = Double(max(model.durationFrames, Int64(model.nominalFPS * 5)))
        return (0...8).map { index in
            let fraction = Double(index) / 8
            let x = KRSpace.space2 + (width - KRSpace.space2 * 2) * fraction
            let label = index == 8 ? nil : EditPresentation.rulerLabel(frame: Int64(total * fraction), fps: model.nominalFPS)
            return KRRulerTick("\(index)", x: x, label: label)
        }
    }
    func lane(_ track: [String: Any], width: Double, frameWidth: Double) -> some View {
        let id = track.string("id"), kind = track.string("kind"), locked = model.trackLocked(id)
        return KRTrack(header: .init(model.trackNumber(id), kind == "audio" ? "Audio" : kind == "caption" ? "Caption" : "Video", kind: kind == "audio" ? .audio : kind == "caption" ? .subtitle : .video,
            selected: model.selectedClip?.track == id, hidden: kind == "audio" ? (track.object("state")["muted"] as? Bool ?? false) : !(track.object("state")["visible"] as? Bool ?? true), locked: locked, targeted: model.trackTargeted(track), visibilityEnabled: !locked && !model.busy && model.pendingCandidate == nil, onVisibility: { model.setTrackOutput(track) }, onLock: { model.setTrackLocked(track) }, onTarget: { model.setTrackTarget(track) }), headerWidth: 200, locked: locked) {
            ZStack(alignment: .leading) {
                Color.clear.contentShape(Rectangle()).onTapGesture { tracksFocused = true; model.selectClip(nil) }
                ForEach(model.editClips.filter { $0.track == id }) { clip in
                    clipView(clip, frameWidth: frameWidth, locked: locked)
                }
                if let candidate = model.timelineCandidate, candidate.track == id {
                    if candidate.mode == .place {
                        KRClip(candidate.name, kind: candidate.kind, state: candidate.missing.map { .missing($0) } ?? .selected)
                            .frame(width: max(1, Double(candidate.end - candidate.start) * frameWidth)).offset(x: KRSpace.space2 + Double(candidate.start) * frameWidth).allowsHitTesting(false)
                    } else if candidate.mode == .roll {
                        p.accentInk.frame(width: 1, height: KRSize.trackHeight)
                            .offset(x: KRSpace.space2 + Double(candidate.cut) * frameWidth).allowsHitTesting(false)
                    }
                }
            }.frame(width: width, height: KRSize.trackHeight)
                .onDrop(of: [projectAssetType], delegate: AssetPlacementDrop(model: model, track: id, frameWidth: frameWidth))
        }
    }
    /// Waveform rendered from the shared audio.analyze frames: per-pixel peak
    /// RMS, resampled whenever frameWidth changes. Linear time maps only.
    @ViewBuilder func clipWaveform(_ clip: EditClip, frameWidth: Double) -> some View {
        if clip.kind == .audio {
            Group {
                if let wave = model.waveform(for: clip), let range = model.waveformRange(for: clip) {
                    Canvas { context, size in
                        var peaks = wave.peaks(from: range.lowerBound, to: range.upperBound, columns: max(1, Int(size.width)))
                        if clip.reversed { peaks.reverse() }
                        let gain = max(wave.peak, 0.001)
                        for (column, peak) in peaks.enumerated() where peak > 0 {
                            let height = max(1, size.height * CGFloat(min(1, peak / gain)))
                            context.fill(Path(CGRect(x: CGFloat(column), y: size.height - height, width: 1, height: height)), with: .color(.white.opacity(0.55)))
                        }
                    }.frame(height: 14).padding(.bottom, 4)
                }
            }.allowsHitTesting(false)
            .task(id: model.revision) { model.ensureWaveform(for: clip) }
        }
    }
    /// Clip-local marker glyphs along the clip top edge (display only).
    @ViewBuilder func clipMarkers(_ clip: EditClip, start: Int64, frameWidth: Double) -> some View {
        ForEach(clip.markers) { marker in
            let offset = marker.time.frames(rateNum: model.rateNum, rateDen: model.rateDen)
            MarkerDiamond(color: Self.markerColor(marker.color), selected: false)
                .frame(width: 6, height: 6)
                .offset(x: Double(offset - start) * frameWidth - 3, y: 1)
                .allowsHitTesting(false)
                .help(marker.comment.isEmpty ? "マーカー" : marker.comment)
        }
    }
    func clipView(_ clip: EditClip, frameWidth: Double, locked: Bool) -> some View {
        let c = model.timelineCandidate.flatMap { $0.clip.string("id") == clip.id ? $0 : nil }
        let start = c?.start ?? clip.start.frames(rateNum: model.rateNum, rateDen: model.rateDen)
        let end = c?.end ?? clip.end.frames(rateNum: model.rateNum, rateDen: model.rateDen)
        let missing = model.clipMissing(clip)
        return KRClip(model.clipName(clip), kind: clip.kind, state: missing.map { .missing($0) } ?? (model.ui.clipSelection == clip.id ? .selected : .resting), onSelect: { model.selectClip(clip.id) })
            .allowsHitTesting(false)
            .accessibilityHidden(true)
            .opacity(clip.enabled ? 1 : 0.4)
            .overlay(alignment: .bottom) { clipWaveform(clip, frameWidth: frameWidth) }
            .overlay(alignment: .topLeading) { clipMarkers(clip, start: start, frameWidth: frameWidth) }
            // NLE-006: a piecewise map carries a speed ramp or freeze hold; mark it on the lane.
            .overlay(alignment: .topTrailing) { if clip.timeMapKind == "piecewise_linear" {
                KRIconView(.slidersHorizontal).foregroundStyle(p.accentInk).padding(2)
            } }
            // A cue has no source window: splitting is not a caption operation.
            .overlay { if model.editTool == "blade" && clip.kind != .subtitle {
                KRBladeHitArea("\(model.clipName(clip)) を分割", begin: { fraction in
                    model.beginClipGesture(clip, mode: .blade); model.updateClipGesture(at: model.bladeFrame(clip, fraction: fraction))
                }, update: { model.updateClipGesture(at: model.bladeFrame(clip, fraction: $0)) }, release: { Task { await model.commitClipGesture() } })
            } else {
                KRClipHitArea(missing.map { model.clipName(clip) + " " + $0 } ?? model.clipName(clip),
                    select: { tracksFocused = true; model.selectClip(clip.id) },
                    open: { if clip.composition != nil { model.openClipInMotion(clip) } },
                    begin: { mode in
                        if model.editTool == "roll" {
                            model.beginRollGesture(clip, atStartEdge: mode == .trimStart); return
                        }
                        let gesture: TimelineCandidate.Mode
                        switch model.editTool {
                        case "slip": gesture = .slip
                        case "slide": gesture = .slide
                        default: gesture = mode == .move ? .move : mode == .trimStart ? .trimStart : .trimEnd
                        }
                        model.beginClipGesture(clip, mode: gesture)
                    }, update: { model.updateClipGesture(delta: Int64(($0 / frameWidth).rounded())) },
                    release: { Task { await model.commitClipGesture() } }, cancel: model.cancelClipGesture)
            } }
            .overlay { if let c, c.mode == .blade { p.selection.frame(width: 1).offset(x: Double(c.cut - start) * frameWidth - Double(end - start) * frameWidth / 2).allowsHitTesting(false) } }
            .overlay { if let c, c.delta != 0, c.mode == .slip || c.mode == .slide || c.mode == .roll {
                Text("\(c.delta > 0 ? "+" : "")\(c.delta) f").krText(KRType.caption).foregroundStyle(p.ink)
                    .padding(.horizontal, 3).background(p.surface200).clipShape(RoundedRectangle(cornerRadius: 3)).allowsHitTesting(false)
            } }
            .frame(width: max(1, Double(end - start) * frameWidth))
            .offset(x: KRSpace.space2 + Double(start) * frameWidth)
            .disabled(locked || model.busy || model.pendingCandidate != nil)
    }
}

/// Small diamond used for sequence markers on the ruler and clip markers.
struct MarkerDiamond: View {
    let color: Color
    var selected = false
    var body: some View {
        DiamondShape().fill(color)
            .overlay(DiamondShape().stroke(selected ? Color.white : Color.black.opacity(0.35), lineWidth: 1))
    }
}
struct DiamondShape: Shape {
    func path(in rect: CGRect) -> Path {
        Path { path in
            path.move(to: CGPoint(x: rect.midX, y: rect.minY))
            path.addLine(to: CGPoint(x: rect.maxX, y: rect.midY))
            path.addLine(to: CGPoint(x: rect.midX, y: rect.maxY))
            path.addLine(to: CGPoint(x: rect.minX, y: rect.midY))
            path.closeSubpath()
        }
    }
}

struct AssetPlacementDrop: DropDelegate {
    let model: EditorModel
    let track: String
    let frameWidth: Double
    var receiver: AssetPlacementReceiver { .init(model: model, track: track) }
    func trace(_ event: String) {
        if ProcessInfo.processInfo.environment["KRONELLO_TRACE_ASSET_DRAG"] == "1" { NSLog("asset drag destination: %@", event) }
    }
    func validateDrop(info: DropInfo) -> Bool {
        let accepted = info.hasItemsConforming(to: [projectAssetType]) && receiver.canAccept
        trace("validate \(accepted)"); return accepted
    }
    func dropEntered(info: DropInfo) { trace("entered"); _ = updateCandidate(info: info) }
    @discardableResult func updateCandidate(info: DropInfo) -> Bool {
        let frame = Int64(max(0, (info.location.x - KRSpace.space2) / frameWidth).rounded())
        return receiver.update(at: frame)
    }
    func dropUpdated(info: DropInfo) -> DropProposal? {
        trace("updated"); return DropProposal(operation: updateCandidate(info: info) ? .copy : .forbidden)
    }
    func dropExited(info: DropInfo) { trace("exited"); receiver.exit() }
    func performDrop(info: DropInfo) -> Bool {
        trace("perform"); guard validateDrop(info: info) else { return false }
        guard updateCandidate(info: info) else { return false }
        Task { await receiver.commit() }; return true
    }
}
