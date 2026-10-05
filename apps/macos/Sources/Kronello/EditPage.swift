import AppKit
import SwiftUI
import UniformTypeIdentifiers
import KronelloAppModel
import KronelloDesign

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
                if tab == "effects" { KREmptyState(icon: .sparkles, title: "Effects", message: "クリップの効果追加は未対応です。") }
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
                                            model.assetSelection = asset.id; model.cancelClipGesture()
                                            let provider = NSItemProvider()
                                            provider.registerDataRepresentation(forTypeIdentifier: "com.kronello.project-asset", visibility: .ownProcess) { completion in
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
    @State private var toolPlacement = KRToolStripPlacement()
    static let tools: [KRTool] = [
        .init("select", icon: .mousePointer2, name: "選択", shortcut: "V"),
        .init("blade", icon: .scissors, name: "ブレード", shortcut: "B"),
        .init("hand", icon: .hand, name: "手のひら", shortcut: "H", unavailableReason: "手のひら操作は未対応です"),
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
                        if model.ui.sequence == nil { KREmptyState(icon: .clapperboard, title: "Sequence がありません", message: "共有 API で Sequence を作成してください。") }
                        else {
                            KRViewerFrame(aspectRatio: max(1, model.extent.width) / max(1, model.extent.height)) {
                                MetalPreview(model: model)
                                if safeArea { Rectangle().strokeBorder(p.inkMuted, style: .init(lineWidth: 1, dash: [4, 4])).padding(24).allowsHitTesting(false) }
                            }.padding(KRSpace.space3)
                        }
                        if let failure = model.sequenceFailure ?? model.previewFailure {
                            VStack(spacing: KRSpace.space3) {
                                KRViewerError(.init(failure.code, failure.message), copy: { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(failure.detailText, forType: .string) },
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
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    var body: some View {
        KRPanel("Inspector") {
            if let clip = model.selectedClip {
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
                            timeRow("ソース開始", frames: RationalTime.wire(clip.authored.object("source_in")).frames(rateNum: model.rateNum, rateDen: model.rateDen), action: nil)
                        }
                        section("時間") {
                            KRInspectorSettingRow("速度") { Text(clip.speedLabel).krText(KRType.timecode).foregroundStyle(p.inkMuted) }
                            KRInspectorSettingRow("逆再生") { KRCheckbox("", isOn: .constant(clip.reversed)).accessibilityLabel("逆再生").disabled(true) }
                            Text("速度・ソース開始の変更は未対応です。").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                        }
                        section("合成") {
                            KRInspectorSettingRow("不透明度") { Text(opacity(clip)).krText(KRType.timecode).foregroundStyle(p.inkMuted) }
                            KRInspectorSettingRow("描画モード") { Text("Normal").krText(KRType.label).foregroundStyle(p.inkMuted) }
                            Text("合成設定は表示のみです。").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                        }
                        if clip.composition != nil { KRButton("モーションで開く", icon: .layers, variant: .secondary) { model.openClipInMotion(clip) }.padding(.horizontal, KRSpace.space3) }
                    }.padding(.vertical, KRSpace.space3)
                }
            } else { KREmptyState(icon: .mousePointer2, title: "クリップを選択", message: "トラックでクリップを選択してください。") }
        }
    }
    func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: KRSpace.space1) { Text(title).krText(KRType.heading).padding(.horizontal, KRSpace.space3); content() }
    }
    func timeRow(_ title: String, frames: Int64, action: ((Int64) -> Void)?) -> some View {
        KRInspectorSettingRow(title) {
            KRTimecodeField(frames: .constant(max(0, frames)), fps: model.nominalFPS, currentTime: false, label: title, onSeek: { action?($0) })
                .disabled(action == nil || model.busy || model.pendingCandidate != nil)
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
            KRButton(icon: .magnet, accessibilityLabel: "スナップ", pressed: model.editSnap) { model.editSnap.toggle() }
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
                                KRRuler(rulerTicks(width: laneWidth))
                                    .contentShape(Rectangle()).gesture(DragGesture(minimumDistance: 0).onChanged { model.seek(Int64(max(0, ($0.location.x - KRSpace.space2) / frameWidth).rounded())) })
                            }
                            ForEach(model.orderedTracks, id: \.selfID) { track in
                                lane(track, width: laneWidth, frameWidth: frameWidth)
                            }
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
            case .escape: model.cancelClipGesture()
            case .space: model.playing.toggle()
            case .leftArrow: model.seek(model.frame - 1)
            case .rightArrow: model.seek(model.frame + 1)
            case "v": model.editTool = "select"
            case "b": model.editTool = "blade"
            default: return .ignored
            }
            return .handled
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
        let id = track.string("id"), locked = model.ui.locked.contains(id)
        return KRTrack(header: .init(model.trackNumber(id), track.string("kind") == "audio" ? "Audio" : "Video", kind: track.string("kind") == "audio" ? .audio : .video,
            selected: model.selectedClip?.track == id, locked: locked, visibilityEnabled: false, onLock: { model.toggleLock(id) }), headerWidth: 200, locked: locked) {
            ZStack(alignment: .leading) {
                Color.clear.contentShape(Rectangle()).onTapGesture { tracksFocused = true; model.selectClip(nil) }
                ForEach(model.editClips.filter { $0.track == id }) { clip in
                    clipView(clip, frameWidth: frameWidth, locked: locked)
                }
                if let candidate = model.timelineCandidate, candidate.track == id, candidate.mode == .place {
                    KRClip(candidate.name, kind: candidate.kind, state: candidate.missing.map { .missing($0) } ?? .selected)
                        .frame(width: max(1, Double(candidate.end - candidate.start) * frameWidth)).offset(x: KRSpace.space2 + Double(candidate.start) * frameWidth).allowsHitTesting(false)
                }
            }.frame(width: width, height: KRSize.trackHeight)
                .onDrop(of: ["com.kronello.project-asset"], delegate: AssetPlacementDrop(model: model, track: id, frameWidth: frameWidth))
        }
    }
    func clipView(_ clip: EditClip, frameWidth: Double, locked: Bool) -> some View {
        let c = model.timelineCandidate.flatMap { $0.clip.string("id") == clip.id ? $0 : nil }
        let start = c?.start ?? clip.start.frames(rateNum: model.rateNum, rateDen: model.rateDen)
        let end = c?.end ?? clip.end.frames(rateNum: model.rateNum, rateDen: model.rateDen)
        let missing = model.clipMissing(clip)
        return KRClip(model.clipName(clip), kind: clip.kind, state: missing.map { .missing($0) } ?? (model.ui.clipSelection == clip.id ? .selected : .resting), onSelect: { model.selectClip(clip.id) })
            .allowsHitTesting(model.editTool != "blade")
            .accessibilityHidden(model.editTool == "blade")
            .overlay { if model.editTool == "blade" {
                KRBladeHitArea("\(model.clipName(clip)) を分割", begin: { fraction in
                    model.beginClipGesture(clip, mode: .blade); model.updateClipGesture(at: model.bladeFrame(clip, fraction: fraction))
                }, update: { model.updateClipGesture(at: model.bladeFrame(clip, fraction: $0)) }, release: { Task { await model.commitClipGesture() } })
            } }
            .overlay(alignment: .leading) { if model.editTool != "blade" { trimHandle(clip, mode: .trimStart, frameWidth: frameWidth).frame(width: 6) } }
            .overlay(alignment: .trailing) { if model.editTool != "blade" { trimHandle(clip, mode: .trimEnd, frameWidth: frameWidth).frame(width: 6) } }
            .overlay { if let c, c.mode == .blade { p.selection.frame(width: 1).offset(x: Double(c.cut - start) * frameWidth - Double(end - start) * frameWidth / 2).allowsHitTesting(false) } }
            .frame(width: max(1, Double(end - start) * frameWidth))
            .offset(x: KRSpace.space2 + Double(start) * frameWidth)
            .simultaneousGesture(TapGesture(count: 2).onEnded { if model.editTool != "blade", clip.composition != nil { model.openClipInMotion(clip) } })
            .gesture(DragGesture(minimumDistance: 3, coordinateSpace: .named("sequenceTracks")).onChanged { value in
                guard model.editTool != "blade" else { return }
                model.beginClipGesture(clip, mode: .move)
                model.updateClipGesture(delta: Int64((value.translation.width / frameWidth).rounded()))
            }.onEnded { _ in if model.editTool != "blade" { Task { await model.commitClipGesture() } } }, including: model.editTool == "blade" ? .none : .all)
            .disabled(locked || model.busy || model.pendingCandidate != nil)
    }
    func trimHandle(_ clip: EditClip, mode: TimelineCandidate.Mode, frameWidth: Double) -> some View {
        Rectangle().fill(Color.clear).contentShape(Rectangle()).highPriorityGesture(DragGesture(minimumDistance: 3, coordinateSpace: .named("sequenceTracks")).onChanged {
            model.beginClipGesture(clip, mode: mode); model.updateClipGesture(delta: Int64(($0.translation.width / frameWidth).rounded()))
        }.onEnded { _ in Task { await model.commitClipGesture() } }).help(mode == .trimStart ? "開始をトリム" : "末尾をトリム")
    }
}

struct AssetPlacementDrop: DropDelegate {
    let model: EditorModel
    let track: String
    let frameWidth: Double
    func validateDrop(info: DropInfo) -> Bool { info.hasItemsConforming(to: ["com.kronello.project-asset"]) && model.assetSelection != nil && !model.busy && model.pendingCandidate == nil }
    func dropUpdated(info: DropInfo) -> DropProposal? {
        let frame = Int64(max(0, (info.location.x - KRSpace.space2) / frameWidth).rounded())
        if model.timelineCandidate == nil, let asset = model.editAssets.first(where: { $0.id == model.assetSelection }) { model.beginAssetGesture(asset, track: track, at: frame) }
        model.updateClipGesture(at: frame)
        return DropProposal(operation: model.timelineCandidate == nil ? .forbidden : .copy)
    }
    func dropExited(info: DropInfo) { model.cancelClipGesture() }
    func performDrop(info: DropInfo) -> Bool {
        guard model.timelineCandidate != nil else { return false }
        Task { await model.commitClipGesture() }; return true
    }
}

extension EditorModel {
    var orderedTracks: [[String: Any]] {
        let tracks = sequence.objects("tracks")
        return tracks.filter { $0.string("kind") == "video" }.reversed() + tracks.filter { $0.string("kind") == "audio" }
    }
    func trackNumber(_ id: String) -> String {
        let tracks = sequence.objects("tracks"), kind = tracks.first { $0.string("id") == id }?.string("kind") ?? "video"
        return (kind == "audio" ? "A" : "V") + String((tracks.filter { $0.string("kind") == kind }.firstIndex { $0.string("id") == id } ?? 0) + 1)
    }
}
