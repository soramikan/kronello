import AppKit
import SwiftUI
import UniformTypeIdentifiers
import KronelloCore
import KronelloAppModel
import KronelloDesign

private let projectAssetType = UTType(exportedAs: "com.kronello.project-asset", conformingTo: .data)

struct EditPage: View {
    @ObservedObject var model: EditorModel
    @ObservedObject var workflow: WorkflowSettings
    var body: some View {
        let layout = workflow.layout(for: "edit")
        KREditLayout(projectPanel: layout.leadingPanel, inspectorPanel: layout.trailingPanel, tracksPanel: layout.bottomPanel,
                     projectWidth: layout.leadingWidth, inspectorWidth: layout.trailingWidth, tracksHeight: layout.bottomHeight,
                     // GUI-011 (ADR-0128): Source monitor sits left of the
                     // Program monitor; both render through one session.
                     project: { EditProjectPanel(model: model) },
                     viewer: { HStack(spacing: 0) { SourceViewer(model: model, workflow: workflow); SequenceViewer(model: model) } },
                     inspector: { ClipInspector(model: model) }, tracks: { SequenceTracks(model: model, workflow: workflow) })
            .onAppear { model.activatePlayback(for: model.sequence) }
            .onChange(of: model.sequence.string("id") + model.activeRate.string("num") + "/" + model.activeRate.string("den")) { _, _ in model.activatePlayback(for: model.sequence) }
    }
}

struct EditProjectPanel: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    @State private var tab = "project"
    @State private var search = ""
    var body: some View {
        KRPanel(header: { KRTabBar([.init("project", "Project"), .init("markers", "マーカー"), .init("effects", "Effects")], selection: $tab) }, actions: {}) {
            VStack(spacing: 0) {
                if tab != "markers" { KRSearchField("素材を検索", text: $search).padding(KRSpace.space2) }
                if tab == "effects" {
                    VStack(alignment: .leading, spacing: KRSpace.space3) {
                        VStack(alignment: .leading, spacing: KRSpace.space3) {
                            KRButton("Gaussian Blur", icon: .sparkles) { model.addClipEffect("blur") }
                            KRButton("Drop Shadow", icon: .layers) { model.addClipEffect("shadow") }
                            KRButton("Chroma Key", icon: .sparkles) { model.addClipEffect("chroma_key") }
                            KRButton("Luma Key", icon: .sparkles) { model.addClipEffect("luma_key") }
                            KRButton("Glow", icon: .sparkles) { model.addClipEffect("glow") }
                            KRButton("Sharpen", icon: .sparkles) { model.addClipEffect("sharpen") }
                            KRButton("Vignette", icon: .sparkles) { model.addClipEffect("vignette") }
                            KRButton("Corner Pin", icon: .sparkles) { model.addClipEffect("corner_pin") }
                            // TRACK-002: analyze the clip's media, then bind
                            // kronello.stabilize to the committed tracking asset.
                            KRButton("スタビライズ", icon: .crosshair) { if let clip = model.selectedClip { model.stabilizeClip(clip) } }
                                .disabled(model.selectedClip.map { clip in
                                    model.stabilizePending.contains(clip.id) || clip.timeMapKind != "linear"
                                        || (clip.kind != .video && clip.kind != .multicam)
                                        || model.trackableSource(clip) == nil || model.clipHasStabilize(clip)
                                } ?? true)
                            if let clip = model.selectedClip, model.stabilizePending.contains(clip.id) {
                                Text("解析中…").krText(KRType.caption)
                            }
                            Text("選択中の映像クリップに追加します。").krText(KRType.caption)
                        }.disabled(model.selectedClip == nil || model.selectedClip?.kind == .audio || model.busy || model.pendingCandidate != nil)
                        Divider()
                        VStack(alignment: .leading, spacing: KRSpace.space3) {
                            KRButton("アジャストメントクリップを追加", icon: .sparkles) { model.addAdjustmentClip() }
                            Text("再生ヘッドに新規映像トラックへ追加し、下の映像全体へ効果をかけます。").krText(KRType.caption)
                        }.disabled(model.sequence.isEmpty || model.busy || model.pendingCandidate != nil)
                    }.padding()
                }
                else if tab == "markers" {
                    // GUI-012: sequence + clip markers in one list; selecting a
                    // row seeks the playhead and loads the marker into the
                    // inspector (comment edit lives there).
                    ScrollView(.vertical) {
                        VStack(spacing: 0) {
                            if model.allMarkers.isEmpty {
                                KREmptyState(icon: .circle, title: "マーカーなし",
                                    message: "ルーラーのダブルクリックか「再生ヘッドにマーカー」で追加できます。")
                                    .padding(KRSpace.space4)
                            }
                            ForEach(model.allMarkers.filter { search.isEmpty || $0.comment.localizedCaseInsensitiveContains(search) }) { marker in
                                markerRow(marker)
                            }
                        }
                    }
                }
                else {
                    ScrollView(.vertical) {
                            VStack(spacing: 0) {
                                ForEach(model.editAssets.filter { search.isEmpty || $0.name.localizedCaseInsensitiveContains(search) }) { asset in
                                    KRAssetRow(asset.name, kind: asset.kind, meta: asset.meta,
                                        duration: KRTimecode.format(frames: max(0, asset.duration.frames(rateNum: model.rateNum, rateDen: model.rateDen)), fps: model.nominalFPS),
                                        selected: model.assetSelection == asset.id, missing: asset.missing,
                                        onSelect: { model.assetSelection = asset.id }, onOpen: {
                                            // GUI-011: media and multicam sources open in the Source monitor;
                                            // compositions keep their Motion-page navigation.
                                            if let composition = asset.source["composition"] as? String { model.ui.page = "motion"; model.setComposition(composition) }
                                            else { model.openAssetInSource(asset) }
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
                if tab == "project" {
                    // NLE-007: multicam groups are authored through the shared
                    // `multicam.create` operation (timecode/audio/manual sync).
                    HStack {
                        KRButton("マルチカムを作成…", icon: .clapperboard, variant: .secondary) { multicamDraft = .init() }
                            .disabled(model.busy || model.pendingCandidate != nil)
                        // GUI-012: sidecar caption import drives the shared
                        // captions.import_plan → captions.import plan flow.
                        KRButton("字幕を読み込む…", icon: .captions, variant: .secondary) { importCaptions() }
                            .disabled(model.busy || model.pendingCandidate != nil || model.ui.sequence == nil)
                        Spacer(minLength: 0)
                    }.padding(KRSpace.space2).overlay(alignment: .top) { Color.primary.opacity(0.001) }
                }
                Spacer(minLength: 0)
            }
        }
        .sheet(item: $multicamDraft) { draft in
            MulticamCreateSheet(model: model, draft: draft)
        }
    }
    @State private var multicamDraft: MulticamCreateSheet.Draft?

    /// Marker list row: diamond + comment + scope + timecode. Tap selects and
    /// seeks; editing happens in the inspector branch for the selected marker.
    @ViewBuilder func markerRow(_ marker: EditMarker) -> some View {
        let selected = model.markerSelection == marker.id
        Button {
            model.selectMarker(marker)
            model.seek(marker.time.frames(rateNum: model.rateNum, rateDen: model.rateDen))
        } label: {
            HStack(spacing: KRSpace.space2) {
                MarkerDiamond(color: SequenceTracks.markerColor(marker.color), selected: selected)
                    .frame(width: 10, height: 10)
                VStack(alignment: .leading, spacing: 1) {
                    Text(marker.comment.isEmpty ? "（コメントなし）" : marker.comment)
                        .krText(KRType.body).foregroundStyle(marker.comment.isEmpty ? p.inkMuted : p.ink).lineLimit(1)
                    Text(marker.clip == nil ? "シーケンス" : (marker.track.map { model.trackNumber($0) } ?? "クリップ"))
                        .krText(KRType.caption).foregroundStyle(p.inkMuted).lineLimit(1)
                }
                Spacer(minLength: 0)
                Text(KRTimecode.format(frames: marker.time.frames(rateNum: model.rateNum, rateDen: model.rateDen), fps: model.nominalFPS))
                    .krText(KRType.ruler).foregroundStyle(p.inkMuted)
            }
            .padding(.horizontal, KRSpace.space3).frame(height: KRSize.rowHeight + 10)
            .background(selected ? p.selectionBg : .clear)
            .contentShape(Rectangle())
        }.buttonStyle(.plain)
        .contextMenu {
            Button("削除") { model.removeMarker(marker) }
        }
        .accessibilityLabel(marker.comment.isEmpty ? "マーカー" : "マーカー \(marker.comment)")
    }
    /// Sidecar picker → `model.importCaptions` (format inferred from extension).
    func importCaptions() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = EditorModel.captionFormats.flatMap { $0.extensions }.compactMap { UTType(filenameExtension: $0) }
        panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        panel.prompt = "字幕を読み込む"
        if panel.runModal() == .OK, let url = panel.url {
            model.importCaptions(sidecarPath: url.path)
        }
    }
}

/// GUI-011 source monitor (ADR-0128): loads bin assets, timeline-clip
/// sources and multicam angles onto the dedicated preview surface; In/Out
/// controls and insert/overwrite drive the shared three-point operations.
struct SourceViewer: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    let workflow: WorkflowSettings
    @FocusState private var focused: Bool
    private var choices: [(preview: SourcePreview, name: String, kind: KRMediaKind)] { model.sourceChoices() }
    var body: some View {
        KRPanel(header: {
            HStack(spacing: KRSpace.space2) {
                KRPanelTitle("Source")
                if let monitor = model.sourceMonitor {
                    Text(monitor.name).krText(KRType.caption).foregroundStyle(p.inkMuted).lineLimit(1)
                }
            }
        }, actions: {
            HStack(spacing: KRSpace.space2) {
                KRPopupButton("ソース", options: choices.map { KRPopupOption($0.preview.key, $0.name, icon: $0.kind.icon) },
                    selection: Binding(get: { model.sourceMonitor?.source.key ?? "" }, set: { key in
                        guard let choice = choices.first(where: { $0.preview.key == key }) else { return }
                        model.openSource(choice.preview, name: choice.name)
                    })).frame(width: 160).disabled(choices.isEmpty)
                if model.sourceMonitor != nil {
                    KRButton(icon: .x, accessibilityLabel: "ソースモニタを閉じる") { model.closeSourceMonitor() }
                }
            }
        }) {
            if let monitor = model.sourceMonitor {
                loaded(monitor)
            } else {
                KREmptyState(icon: .film, title: "ソースなし",
                    message: choices.isEmpty ? "素材を読み込むとここにプレビューされます。" : "上の「ソース」か素材・クリップのダブルクリックで開きます。")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .focusable().focused($focused)
        .overlay { if focused { Rectangle().strokeBorder(p.selection, lineWidth: 2).allowsHitTesting(false).padding(1) } }
        .onKeyPress { key in
            guard focused else { return .ignored }
            func hit(_ action: ShortcutAction) -> Bool { workflow.binding(for: action).matches(key) }
            if hit(.transportStepBack) { model.seekSourceFrame(model.sourceFrame - 1) }
            else if hit(.transportStepForward) { model.seekSourceFrame(model.sourceFrame + 1) }
            else if hit(.sourceSetIn) { model.setSourceInPoint() }
            else if hit(.sourceSetOut) { model.setSourceOutPoint() }
            else if hit(.sourceClear) { model.clearSourcePoints() }
            else if hit(.editInsert) { model.insertSource() }
            else if hit(.editOverwrite) { model.overwriteSource() }
            else { return .ignored }
            return .handled
        }
    }
    private func loaded(_ monitor: SourceMonitor) -> some View {
        VStack(spacing: 0) {
            ZStack {
                let extent = model.sourceExtent(for: monitor.source)
                KRViewerFrame(aspectRatio: max(1, extent.width) / max(1, extent.height)) {
                    MetalPreview(model: model, source: monitor.source)
                }.padding(KRSpace.space3).clipped()
                if let failure = model.sourcePreviewFailure {
                    VStack(spacing: KRSpace.space3) {
                        KRViewerError(.init(failure.code, failure.message),
                            copy: { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(failure.copyText, forType: .string) },
                            retry: { model.sourcePreviewFailure = nil; model.closeSourceMonitor() })
                        if model.offersSourceCPUReference { KRButton("CPU 参照で表示", variant: .secondary) { model.chooseSourceCPUReference() } }
                    }
                }
                if model.sourcePreviewRendering { VStack { HStack {
                    KRActivityIndicator(); Spacer()
                }; Spacer() }.padding(KRSpace.space3).allowsHitTesting(false) }
            }.background(p.surface0).frame(maxHeight: .infinity)
                .contentShape(Rectangle()).onTapGesture { focused = true }
            controls(monitor)
        }
    }
    private func rangeLabel(_ monitor: SourceMonitor) -> String {
        func point(_ value: RationalTime?) -> String {
            value.map { KRTimecode.format(frames: max(0, $0.frames(rateNum: model.rateNum, rateDen: model.rateDen)), fps: model.nominalFPS) } ?? "--:--:--:--"
        }
        var parts = ["In \(point(monitor.inPoint))", "Out \(point(monitor.outPoint))"]
        if let duration = model.sourceDurationFrames {
            parts.append("尺 " + KRTimecode.format(frames: max(0, duration), fps: model.nominalFPS))
        }
        return parts.joined(separator: "  ")
    }
    private func controls(_ monitor: SourceMonitor) -> some View {
        VStack(spacing: 0) {
            HStack(spacing: KRSpace.space2) {
                KRTimecodeField(frames: Binding(get: { model.sourceFrame }, set: { model.seekSourceFrame($0) }),
                    fps: model.nominalFPS, label: "ソース時刻", onSeek: model.seekSourceFrame)
                Spacer(minLength: 0)
                KRButton(icon: .stepBack, accessibilityLabel: "ソースを 1 フレーム戻る") { model.seekSourceFrame(model.sourceFrame - 1) }
                KRButton(icon: .stepForward, accessibilityLabel: "ソースを 1 フレーム進む") { model.seekSourceFrame(model.sourceFrame + 1) }
                KRButton("In", variant: .plain) { model.setSourceInPoint() }
                KRButton("Out", variant: .plain) { model.setSourceOutPoint() }
                KRButton(icon: .x, accessibilityLabel: "ソースの In/Out を解除") { model.clearSourcePoints() }
            }
            .padding(.horizontal, KRSpace.space3).padding(.top, KRSpace.space1)
            HStack(spacing: KRSpace.space2) {
                Text(rangeLabel(monitor)).krText(KRType.caption).foregroundStyle(p.inkMuted).lineLimit(1)
                Spacer(minLength: 0)
                if case .multicam(let group, let angle) = monitor.source, let info = model.multicamGroup(group) {
                    KRPopupButton("アングル", options: info.angles.map { KRPopupOption($0.id, $0.displayName) },
                        selection: Binding(get: { angle }, set: { model.previewSourceAngle($0) })).frame(width: 110)
                }
                KRPopupButton("出力先", options: destinationOptions,
                    selection: Binding(get: { model.sourceMonitor?.track ?? "" },
                                       set: { model.setSourceDestination($0.isEmpty ? nil : $0) })).frame(width: 110)
                KRButton("インサート", variant: .secondary) { model.insertSource() }
                    .disabled(model.sourceEditRange == nil || model.busy || model.pendingCandidate != nil || model.ui.sequence == nil)
                KRButton("上書き", variant: .secondary) { model.overwriteSource() }
                    .disabled(model.sourceEditRange == nil || model.busy || model.pendingCandidate != nil || model.ui.sequence == nil)
            }
            .padding(.horizontal, KRSpace.space3).padding(.vertical, KRSpace.space1)
        }.background(p.surface100).overlay(alignment: .top) { p.line.frame(height: 1) }
    }
    private var destinationOptions: [KRPopupOption] {
        [KRPopupOption("", "自動 (" + model.sourceDestinationLabel + ")")]
            + model.sourceDestinationTracks().map { KRPopupOption($0.string("id"), model.trackNumber($0.string("id")), disabled: model.trackLocked($0.string("id"))) }
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
                                if let clip = model.selectedClip, !EditorModel.clipMasks(clip).isEmpty {
                                    MaskOverlay(model: model, clip: clip)
                                }
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
                                // GUI-012: FONT_MISSING guides to font.pin —
                                // the pinned face persists in user state.
                                if failure.code == "FONT_MISSING" { FontRecoveryButton(model: model) }
                            }
                        }
                        if model.sequenceLoading || model.busy || model.previewRendering || model.previewStale { VStack { HStack {
                            if model.sequenceLoading || model.busy || model.previewRendering { KRActivityIndicator() }
                            Text(model.usesCPUReference && model.playing ? "CPU 参照 · 再生中は前回のフレームを表示" : "更新中 · 表示は前回の結果です").krText(KRType.caption).foregroundStyle(p.inkMuted); Spacer()
                        }; Spacer() }.padding(KRSpace.space3).allowsHitTesting(false) }
                    }.background(p.surface0)
                }
                // COLOR-004: deterministic scopes observe the composited frame.
                if model.ui.sequence != nil { ScopesPanel(model: model) }
                KRTransportBar(frames: Binding(get: { model.frame }, set: { model.seek($0) }), fps: model.nominalFPS, duration: model.durationCode,
                    playing: $model.playing, looping: $model.ui.looping, zoom: $model.ui.zoom, resolution: $model.ui.resolution,
                    onStep: { model.seek(model.frame + Int64($0)) }, onBoundary: { model.seek($0 ? model.durationFrames - 1 : 0) })
            }
        }
    }
}

/// GUI-012 guided font recovery: pick the font file the project references
/// and pin it through the shared `font.pin` operation. The hash-locked
/// identity lands in `ui.fontSources` (ADR-0033) so reopening the project
/// restores the face without touching the document.
struct FontRecoveryButton: View {
    @ObservedObject var model: EditorModel
    var body: some View {
        VStack(spacing: KRSpace.space2) {
            KRButton("フォントを登録…", icon: .folder, variant: .secondary) { pick() }
            Text("参照しているフォントファイルを選択すると、内容ハッシュで登録します。代替フォントへの切り替えは行いません。")
                .krText(KRType.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
    }
    func pick() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        panel.title = "固定して使うフォントファイルを選択"
        if panel.runModal() == .OK, let url = panel.url {
            Task { await model.importFont(path: url.path); model.previewFailure = nil
                do { try await model.reload() } catch { model.mapFailure(error) } }
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
                        // GUI-012: marker comments edit through the shared
                        // `marker_set` upsert; empty input clears the field.
                        section("コメント") {
                            KRTextField("", value: .constant(marker.comment), placeholder: "コメントを入力",
                                onCommit: { model.setMarkerComment(marker, comment: $0) })
                                .padding(.horizontal, KRSpace.space3)
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
                        if let missing = model.clipMissing(clip) {
                            VStack(alignment: .leading, spacing: KRSpace.space2) {
                                KRErrorLine(.init(missing, "参照素材を確認してください。"))
                                // GUI-012: guided recovery — pick the folder
                                // holding the file, `asset.relink` verifies.
                                if let asset = model.clipMissingAsset(clip) {
                                    KRButton("素材を再リンク…", icon: .folder, variant: .secondary) { relinkMissingAsset(asset) }
                                        .disabled(model.busy || model.pendingCandidate != nil)
                                    Text("ファイルがあるフォルダを選ぶと、プロジェクト内の参照を検証して再リンクします。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                                }
                            }.padding(.horizontal, KRSpace.space3)
                        }
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
                        // NLE-007: multicam clips switch their active angle
                        // through the shared clip.angle_switch operation.
                        if let multicam = clip.multicam, let group = model.multicamGroup(multicam.group) {
                            section("マルチカム") {
                                KRInspectorSettingRow("アングル") {
                                    KRPopupButton("アングル", options: group.angles.map { .init($0.id, $0.displayName) },
                                        selection: Binding(get: { multicam.angle }, set: { model.switchClipAngle(clip, to: $0) }))
                                        .disabled(model.trackLocked(clip.track) || model.busy || model.pendingCandidate != nil)
                                }
                                Text((group.name.isEmpty ? "マルチカム" : group.name) + " · \(group.angles.count) アングル").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                            }
                        }
                        if clip.kind == .subtitle { section("字幕") { CaptionInspector(model: model, clip: clip) } }
                        if clip.kind != .subtitle { section("時間") {
                            KRInspectorSettingRow("速度") {
                                KRNumberField(value: .constant(clip.linearRate.map { Double($0.num)! / Double($0.den)! * 100 } ?? 100), unit: "%", step: 0.1, range: 0.1...10000, accessibilityLabel: "速度", onEditingStart: { draftBases["speed"] = model.revision }, onCommit: { _, value in model.setClipTime(clip, speedPercent: value, base: draftBases.removeValue(forKey: "speed")) }).disabled(clip.linearRate == nil)
                            }
                            KRInspectorSettingRow("逆再生") { KRCheckbox("", isOn: Binding(get: { clip.reversed }, set: { model.setClipReverse(clip, enabled: $0) })).accessibilityLabel("逆再生").disabled(clip.linearRate == nil) }
                            // TRACK-003: intermediate-frame synthesis rides the
                            // clip time map; linear maps convert to piecewise.
                            KRInspectorSettingRow("フレーム補間") {
                                KRPopupButton("フレーム補間", options: [.init("", "なし"), .init("optical_flow", "オプティカルフロー")],
                                    selection: Binding(get: { model.clipInterpolation(clip) ?? "" },
                                        set: { model.setClipInterpolation(clip, opticalFlow: !$0.isEmpty) }))
                                    .disabled(!model.interpolationEligible(clip) || model.busy || model.pendingCandidate != nil)
                            }
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
                            // GUI-012: loudness normalize through shared
                            // `audio.normalize` (one undoable gain effect).
                            KRButton("-23 LUFS に正規化", variant: .secondary) { model.normalizeClip(clip) }
                                .disabled(model.busy).padding(.horizontal, KRSpace.space3)
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
                        if clip.kind != .audio && clip.kind != .subtitle { section("マスク") { ClipMaskInspector(model: model, clip: clip) } }
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
                                                    onCommit: { _, value in model.setClipEffectParameter(clip, effect: effect, parameter: field, kind: Self.effectRowKind(effect.string("effect_id"), field), value: value, base: draftBases.removeValue(forKey: field)) })
                                            }
                                        }
                                    }
                                }
                            }
                        }.disabled(clip.kind == .audio)
                        if clip.kind != .audio { section("カラー") { ClipColorInspector(model: model, clip: clip) } }
                        if clip.composition != nil { KRButton("モーションで開く", icon: .layers, variant: .secondary) { model.openClipInMotion(clip) }.padding(.horizontal, KRSpace.space3) }
                        // GUI-011: any previewable clip source opens in the
                        // Source monitor at the playhead-matched source time.
                        if clip.sourcePreview != nil {
                            KRButton("ソースモニタで開く", icon: .film, variant: .secondary) { model.openClipInSource(clip) }.padding(.horizontal, KRSpace.space3)
                        }
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
                        // AI-002: detection runs as a shared job; committed
                        // boundary assets for this clip's source can then be
                        // mapped to sequence markers or clip splits.
                        if model.sceneDetectEligible(clip) { section("シーン") {
                            if let boundaries = model.sceneBoundaryAssets(for: clip).last {
                                let count = boundaries.objects("boundaries").count
                                KRButton("境界をマーカーに追加（\(count) 件）", icon: .circle, variant: .secondary) { model.applySceneBoundaries(clip, asset: boundaries.string("id"), split: false) }
                                    .disabled(model.busy || model.pendingCandidate != nil || model.trackLocked(clip.track))
                                    .padding(.horizontal, KRSpace.space3)
                                KRButton("境界でクリップを分割", icon: .scissors, variant: .secondary) { model.applySceneBoundaries(clip, asset: boundaries.string("id"), split: true) }
                                    .disabled(model.busy || model.pendingCandidate != nil || model.trackLocked(clip.track))
                                    .padding(.horizontal, KRSpace.space3)
                            }
                            KRButton("シーンを検出", icon: .scan, variant: .secondary) { model.detectScenes(clip) }
                                .disabled(model.busy || model.pendingCandidate != nil)
                                .padding(.horizontal, KRSpace.space3)
                            Text("検出はジョブとして実行され、完了した境界がここに反映されます。シーン分割はクリップの素材範囲内の境界に適用されます。").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                        } }
                    }.padding(.vertical, KRSpace.space3)
                }
            } else { KREmptyState(icon: .mousePointer2, title: "クリップを選択", message: "トラックでクリップを選択してください。") }
        }.onChange(of: model.selectedClip?.id) { _, _ in draftBases.removeAll() }
    }
    /// GUI-012 guided relink: a directory search verifies and republishes the
    /// missing asset's locator through `asset.relink`.
    func relinkMissingAsset(_ asset: String) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.allowsMultipleSelection = false
        panel.prompt = "このフォルダから再リンク"
        if panel.runModal() == .OK, let url = panel.url {
            Task { await model.relinkAsset(asset, searchDirectory: url.path) }
        }
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
        case "kronello.stabilize": return "Stabilize"
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
        case "kronello.stabilize":
            return [("smoothing_radius", "平滑化半径", "f", 0...4096, 1),
                    ("max_displacement", "最大移動", "px", 0...512, 1),
                    ("max_rotation", "最大回転", "°", 0...90, 0.5),
                    ("max_crop", "最大クロップ", "", 0...0.5, 0.01)]
        default: return []
        }
    }
    /// Value kind a scalar row writes; angle parameters need `"angle"` or the
    /// service rejects the kind on `clip_set_effects`.
    static func effectRowKind(_ id: String, _ field: String) -> String {
        if id == "kronello.stabilize", field == "max_rotation" { return "angle" }
        return "scalar"
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
    let workflow: WorkflowSettings
    @FocusState private var tracksFocused: Bool
    /// GUI-012: mixer visibility persists per workspace layout (ADR-0033).
    private var showMixer: Bool {
        get { model.ui.layout.mixerVisible }
        nonmutating set { model.ui.layout.mixerVisible = newValue }
    }
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
            KRButton(icon: .audioLines, accessibilityLabel: "オーディオスクラブ", pressed: model.audioScrubEnabled) { model.audioScrubEnabled.toggle() }
                .disabled(model.ui.sequence == nil || model.playbackMuted)
            KRButton(icon: .slidersHorizontal, accessibilityLabel: "ミキサー", pressed: showMixer) { showMixer.toggle() }
                .disabled(model.ui.sequence == nil)
            KRButton(icon: .chevronsLeft, accessibilityLabel: "時間軸を縮小") { model.editScale = max(1, model.editScale / 2) }
            KRButton(icon: .plus, accessibilityLabel: "時間軸を拡大") { model.editScale = min(16, model.editScale * 2) }
        }) {
            VStack(spacing: 0) {
                if showMixer { AudioMixer(model: model).krBottomLine() }
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
        }
        .focusable().focused($tracksFocused)
        .overlay { if tracksFocused { Rectangle().strokeBorder(p.selection, lineWidth: 2).allowsHitTesting(false) } }
        .onKeyPress { key in
            guard tracksFocused else { return .ignored }
            func hit(_ action: ShortcutAction) -> Bool { workflow.binding(for: action).matches(key) }
            if hit(.commonCancel) { model.cancelClipGesture(); model.selectMarker(nil) }
            else if hit(.transportPlay) { model.playing.toggle() }
            else if hit(.transportStepBack) { model.seek(model.frame - 1) }
            else if hit(.transportStepForward) { model.seek(model.frame + 1) }
            else if hit(.editJumpPrevious) { model.jumpToTimelineBoundary(forward: false) }
            else if hit(.editJumpNext) { model.jumpToTimelineBoundary(forward: true) }
            else if hit(.editDeleteRipple) {
                if let marker = model.selectedMarker { model.removeMarker(marker) } else { model.deleteSelectedClip(ripple: true) }
            }
            else if hit(.editDelete) || key.key == .deleteForward {
                if let marker = model.selectedMarker { model.removeMarker(marker) } else { model.deleteSelectedClip() }
            }
            else if hit(.editToolSelect) { model.editTool = "select" }
            else if hit(.editToolBlade) { model.editTool = "blade" }
            else if hit(.editToolSlip) { model.editTool = "slip" }
            else if hit(.editToolSlide) { model.editTool = "slide" }
            else if hit(.editToolRoll) { model.editTool = "roll" }
            else if hit(.editToolHand) { model.editTool = "hand" }
            else if hit(.editClipMarker) {
                guard let clip = model.selectedClip else { return .ignored }
                model.addClipMarker(clip)
            }
            else if hit(.editMarker) { model.addSequenceMarker() }
            else if hit(.editSetIn) { model.setInPoint() }
            else if hit(.editSetOut) { model.setOutPoint() }
            else if hit(.editClearWorkArea) { model.clearWorkArea() }
            // GUI-011: three-point edit keys also work while the tracks hold
            // focus, so the monitor never needs pointer focus to commit.
            else if hit(.editInsert) { model.insertSource() }
            else if hit(.editOverwrite) { model.overwriteSource() }
            else if hit(.sourceSetIn) { model.setSourceInPoint() }
            else if hit(.sourceSetOut) { model.setSourceOutPoint() }
            else if hit(.sourceClear) { model.clearSourcePoints() }
            else { return .ignored }
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
            selected: model.selectedClip?.track == id, hidden: kind == "audio" ? (track.object("state")["muted"] as? Bool ?? false) : !(track.object("state")["visible"] as? Bool ?? true), locked: locked, targeted: model.trackTargeted(track), visibilityEnabled: !locked && !model.busy && model.pendingCandidate == nil,
            meter: model.playbackMeters?.track(id).map { (peak: Double($0.stereoPeak), rms: Double($0.stereoRms)) },
            onVisibility: { model.setTrackOutput(track) }, onLock: { model.setTrackLocked(track) }, onTarget: { model.setTrackTarget(track) }), headerWidth: 200, locked: locked) {
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
                    open: {
                        // GUI-011: media/multicam clips open their source in
                        // the Source monitor; composition clips keep Motion.
                        if clip.composition != nil { model.openClipInMotion(clip) }
                        else if clip.sourcePreview != nil { model.openClipInSource(clip) }
                    },
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

/// AUDIO-009: one strip per audio track plus a master strip. Faders route
/// through the shared edit API (one undoable clip_set_volume per clip); the
/// meters show the evaluator's rendered-block peak/RMS, and mute reuses the
/// track output / monitoring toggles instead of a UI-only path.
struct AudioMixer: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    var body: some View {
        ScrollView(.horizontal) {
            HStack(alignment: .top, spacing: KRSpace.space2) {
                ForEach(model.orderedTracks.filter { $0.string("kind") == "audio" }, id: \.selfID) { track in
                    MixerStrip(model: model, track: track)
                }
                MixerMasterStrip(model: model)
            }.padding(KRSpace.space2)
        }.frame(height: 168).background(p.surface100)
    }
}

struct MixerStrip: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    let track: [String: Any]
    var body: some View {
        let id = track.string("id"), muted = track.object("state")["muted"] as? Bool ?? false
        let locked = model.ui.locked.contains(id), meter = model.playbackMeters?.track(id)
        let pan = model.trackPan(track)
        VStack(spacing: KRSpace.space1) {
            Text(model.trackNumber(id)).krText(KRType.label).foregroundStyle(p.ink)
            KRMeterBar(peak: Double(meter?.stereoPeak ?? 0), rms: Double(meter?.stereoRms ?? 0)).frame(width: 64)
            KRSlider(value: .constant(model.trackVolume(track) ?? 1), range: 0...2, step: 0.05,
                accessibilityLabel: "\(model.trackNumber(id)) フェーダー",
                onCommit: { _, value in model.setTrackVolume(track, gain: value) }).frame(width: 64)
            // GUI-012: constant stereo balance, authored per clip via the
            // shared clip_set_pan command. nil means mixed clip values.
            KRSlider(value: .constant(pan ?? 0), range: -1...1, step: 0.05,
                accessibilityLabel: "\(model.trackNumber(id)) パン",
                onCommit: { _, value in model.setTrackPan(track, pan: value) }).frame(width: 64)
            HStack(spacing: KRSpace.space1) {
                Text(pan.map { String(format: "%+.2f", $0) } ?? "混在").krText(KRType.caption).foregroundStyle(p.inkMuted)
                KRButton(icon: muted ? .volumeX : .volume2, accessibilityLabel: "ミュートを切り替える", pressed: muted, iconSize: 12) { model.setTrackOutput(track) }
                Text(String(format: "%.0f%%", (model.trackVolume(track) ?? 1) * 100)).krText(KRType.caption).foregroundStyle(p.inkMuted)
            }
            // GUI-012: on-demand BS.1770 readout through audio.loudness; the
            // service soloes this track in a measurement snapshot.
            HStack(spacing: KRSpace.space1) {
                Text(model.trackLoudness(track)?.label ?? "LUFS").krText(KRType.caption).foregroundStyle(p.inkMuted)
                if model.loudnessMeasuring(track) {
                    ProgressView().controlSize(.mini)
                } else {
                    KRButton("測定", variant: .plain) { model.measureTrackLoudness(track) }
                }
            }
        }.frame(width: 80).padding(.vertical, KRSpace.space1)
            .opacity(locked ? 0.6 : 1)
            .disabled(locked || model.busy || model.pendingCandidate != nil)
    }
}

struct MixerMasterStrip: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    var body: some View {
        VStack(spacing: KRSpace.space1) {
            Text("Master").krText(KRType.label).foregroundStyle(p.ink)
            KRMeterBar(peak: Double(model.playbackMeters?.masterPeak ?? 0), rms: Double(model.playbackMeters?.masterRms ?? 0)).frame(width: 64)
            KRButton(icon: model.playbackMuted ? .volumeX : .volume2, accessibilityLabel: "モニター音量", pressed: model.playbackMuted, iconSize: 12) { model.playbackMuted.toggle() }
            // GUI-012: program loudness over the full sequence extent.
            HStack(spacing: KRSpace.space1) {
                Text(model.sequenceLoudness?.label ?? "LUFS").krText(KRType.caption).foregroundStyle(p.inkMuted)
                if model.sequenceLoudnessMeasuring {
                    ProgressView().controlSize(.mini)
                } else {
                    KRButton("測定", variant: .plain) { model.measureSequenceLoudness() }
                }
            }
        }.frame(width: 80).padding(.vertical, KRSpace.space1)
            .padding(.leading, KRSpace.space2).overlay(alignment: .leading) { p.line.frame(width: 1) }
    }
}

/// NLE-007 multicam creation (ADR-0127): picks visual streams as ordered
/// angles and submits the shared `multicam.create` operation. Sync modes map
/// 1:1 to `MulticamSync` — "timecode" aligns declared start_time, "audio"
/// cross-correlates decoded audio (MULTICAM_SYNC_FAILED surfaces typed), and
/// "manual" takes the per-angle offsets entered here.
struct MulticamCreateSheet: View {
    struct Draft: Identifiable { let id = UUID() }
    @Environment(\.krPalette) var p
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var model: EditorModel
    let draft: Draft
    @State private var name = ""
    @State private var sync = "timecode"
    /// EditAsset ids in selection order — order is the authored angle order.
    @State private var selected: [String] = []
    /// Manual-mode offset per EditAsset id, in sequence-rate frames.
    @State private var offsets: [String: Double] = [:]
    @State private var reference = ""
    /// Angles must be visual streams; audio-only assets are not angle sources.
    private var candidates: [EditAsset] { model.editAssets.filter { $0.kind == .video || $0.kind == .image } }
    var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space3) {
            Text("マルチカムを作成").krText(KRType.heading)
            KRTextField("名前", value: $name)
            KRPopupButton("同期", options: [
                KRPopupOption("timecode", "タイムコード（開始時刻を揃える）"),
                KRPopupOption("audio", "音声波形（相互相関）"),
                KRPopupOption("manual", "手動オフセット"),
            ], selection: $sync)
            if sync != "manual" {
                KRPopupButton("基準アングル", options: selected.compactMap { id in
                    candidates.first { $0.id == id }.map { KRPopupOption($0.id, $0.name) }
                }, selection: $reference)
                    .onChange(of: selected) { _, _ in if !selected.contains(reference) { reference = selected.first ?? "" } }
                    .disabled(selected.isEmpty)
            }
            Text("アングル（選択順がアングル順）").krText(KRType.label).foregroundStyle(p.inkMuted)
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(candidates) { asset in
                        angleRow(asset)
                    }
                    if candidates.isEmpty { KREmptyState(icon: .film, title: "映像素材なし", message: "映像・画像素材を先に読み込んでください。") }
                }
            }.frame(minHeight: 160, maxHeight: 260)
            if sync == "audio" {
                Text("角度ごとの音声波形の相互相関で同期します。推定に失敗した場合は型付きエラーになります。").krText(KRType.caption).foregroundStyle(p.inkMuted)
            } else if sync == "manual" {
                Text("各アングルの同期オフセット（フレーム）。media_time = マルチカム時間 + オフセット です。").krText(KRType.caption).foregroundStyle(p.inkMuted)
            }
            HStack {
                Spacer()
                KRButton("キャンセル", variant: .secondary) { dismiss() }
                KRButton("作成", variant: .primary) { create() }
                    .disabled(selected.count < 2 || model.busy || model.pendingCandidate != nil)
            }
        }.padding(KRSpace.space4).frame(width: 420)
    }
    private func angleRow(_ asset: EditAsset) -> some View {
        let on = selected.contains(asset.id)
        return HStack(spacing: KRSpace.space2) {
            KRCheckbox(asset.name, isOn: Binding(get: { on }, set: { checked in
                if checked { selected.append(asset.id); if reference.isEmpty { reference = asset.id } }
                else { selected.removeAll { $0 == asset.id }; if reference == asset.id { reference = selected.first ?? "" } }
            }), appearance: .resting)
            if on { Text("\(selected.firstIndex(of: asset.id).map { $0 + 1 } ?? 0)").krText(KRType.timecode).foregroundStyle(p.accentInk) }
            Spacer(minLength: 0)
            if on && sync == "manual" {
                KRNumberField(value: Binding(get: { offsets[asset.id] ?? 0 }, set: { offsets[asset.id] = $0 }),
                    step: 1, range: -100000...100000, precision: 0, accessibilityLabel: asset.name + " のオフセット") { _, _ in }
                    .frame(width: 100)
                Text("f").krText(KRType.caption).foregroundStyle(p.inkMuted)
            }
        }.padding(.horizontal, KRSpace.space2).frame(minHeight: KRSize.rowHeight)
    }
    private func create() {
        // Caller-chosen stable ids: row id → fresh angle UUID before submit.
        var fields: [[String: Any]] = []
        var angleIDs: [String: String] = [:]
        for row in selected {
            guard let asset = candidates.first(where: { $0.id == row }) else { continue }
            let angleID = UUID().uuidString.lowercased()
            angleIDs[row] = angleID
            fields.append([
                "id": angleID,
                "asset": asset.source.string("asset"),
                "stream_index": (asset.source["stream_index"] as? NSNumber)?.intValue ?? 0,
                "name": asset.name,
            ])
        }
        var offsetTimes: [String: RationalTime] = [:]
        if sync == "manual" {
            for row in selected {
                guard let angleID = angleIDs[row] else { continue }
                let frames = Int64((offsets[row] ?? 0).rounded())
                offsetTimes[angleID] = RationalTime(num: frames * model.rateDen, den: model.rateNum)
            }
        }
        model.createMulticam(name: name.isEmpty ? "マルチカム" : name, sync: sync, angles: fields,
                             reference: angleIDs[reference], offsets: offsetTimes)
        dismiss()
    }
}
