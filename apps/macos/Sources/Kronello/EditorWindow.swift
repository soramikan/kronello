import AppKit
import SwiftUI
import KronelloAppModel
import KronelloDesign

struct EditorWindow: View {
    @Environment(\.krPalette) var p
    @Environment(\.krTheme) var theme
    @ObservedObject var model: EditorModel
    @ObservedObject var workflow: WorkflowSettings
    @State private var historyOpen = false
    @State private var jobsOpen = false
    @State private var safeDetailsOpen = false
    var body: some View {
        VStack(spacing: 0) {
            toolbar
            if model.safeMode { KRStateBand("安全モード：このプロジェクトは編集セッション中、排他で開いています。CLI / MCP の open は PROJECT_LOCKED になります", details: { safeDetailsOpen = true }) }
            if model.ui.page == "motion" { MotionPage(model: model, workflow: workflow, historyOpen: $historyOpen) }
            else if model.ui.page == "edit" { EditPage(model: model, workflow: workflow) }
            else if model.ui.page == "template" { TemplatePage(model: model) }
            else if model.ui.page == "export" { ExportPage(model: model) }
            else { KREmptyState(icon: model.ui.page == "export" ? .clapperboard : .layers,
                title: model.ui.page == "template" ? "テンプレートページ" : "書き出しページ",
                message: "このページは GUI-004 で追加します。モーションページで作業を続けられます。")
                .frame(maxWidth: .infinity, maxHeight: .infinity) }
            KRStatusBar(saved: (model.busy ? "保存中" : "保存済み") + (model.safeMode ? " · 安全モード" : "") + " · " + model.playbackStatus, revision: "rev " + model.revision,
                externalChange: model.externalChange, error: diagnostic, job: jobSummary,
                onError: { if model.undoConflict != nil {} else if model.revisionConflict == nil { model.failure = model.previewFailure ?? model.playbackFailure } }, onJobs: { jobsOpen = true })
        }.frame(minWidth: KRWindowMetrics.width, minHeight: KRWindowMetrics.height)
            .onChange(of: model.ui.page) { _, _ in
                model.playing = false; model.cancelClipGesture()
                Task { do { try await model.reload() } catch { model.mapFailure(error) } }
            }
            .background(p.surface100).foregroundStyle(p.ink)
            .sheet(item: $model.undoConflict) { error in
                KRDialog("『" + model.undoConflictLabel + "』を取り消せません", body: error.message + "。部分的には取り消しません。", code: error.code,
                    detail: error.detailText.isEmpty ? historyText : error.detailText + "\n" + historyText,
                    actions: [.init("history", "履歴を開く…") { model.undoConflict = nil; historyOpen = true },
                              .init("ok", "OK", variant: .primary) { model.undoConflict = nil }]).krTheme(theme)
            }
            .sheet(item: $model.failure) { error in
                KRDialog("操作を完了できません", body: error.message, code: error.code, detail: error.detailText.isEmpty ? nil : error.detailText,
                    actions: [.init("ok", "OK", variant: .primary) { model.failure = nil }]).krTheme(theme)
            }
            .sheet(isPresented: $historyOpen) {
                HistoryPanel(model: model).krTheme(theme)
            }
            .sheet(isPresented: $jobsOpen) {
                KRDialog("Jobs", body: "共有 job_progress の最新通知です。", detail: jsonText(model.jobs),
                    actions: [.init("ok", "閉じる", variant: .primary) { jobsOpen = false }]).krTheme(theme)
            }
            .sheet(isPresented: $safeDetailsOpen) {
                KRDialog("安全モード", body: "共有 project.info が safe を返しました。編集中の同じ store をセッションが保持し、CLI / MCP による open を排除します。プロジェクトを閉じるかアプリを終了すると、処理の完了後に排他を解放します。",
                    actions: [.init("ok", "OK", variant: .primary) { safeDetailsOpen = false }]).krTheme(theme)
            }
    }
    var toolbar: some View {
        HStack(spacing: KRSpace.space2) {
            VStack(alignment: .leading, spacing: 0) {
                Text(model.name).krText(KRType.heading).lineLimit(1)
                Text(URL(fileURLWithPath: model.path).deletingLastPathComponent().path).krText(KRType.caption).foregroundStyle(p.inkMuted).lineLimit(1)
            }.padding(.leading, KRSpace.space4 * 4).frame(maxWidth: KRWindowMetrics.left + KRSpace.space4 * 6, alignment: .leading)
            Spacer(minLength: 0)
            KRButton(icon: .undo2, accessibilityLabel: "取り消す") { Task { await model.undo() } }.disabled(!model.canUndo)
            KRButton(icon: .redo2, accessibilityLabel: "やり直す") { Task { await model.undo(redo: true) } }.disabled(!model.canRedo)
            p.line.frame(width: 1, height: KRSize.controlHeight)
            KRPopupButton("ワークスペース", options: [.init("standard", "標準", icon: .layoutPanelLeft)], selection: $model.ui.workspace).fixedSize()
        }.padding(.horizontal, KRSpace.space3).frame(height: KRWindowMetrics.toolbar)
            .overlay { KRSegmentedControl([.init("edit", "編集"), .init("motion", "モーション"), .init("template", "テンプレート"), .init("export", "書き出し")], selection: $model.ui.page) }
            .overlay(alignment: .bottom) { p.line.frame(height: 1) }
    }
    var diagnostic: KRDiagnostic? {
        let failures = [model.failure, model.undoConflict, model.revisionConflict, model.previewFailure, model.sequenceFailure, model.playbackFailure].compactMap { $0 }
        let assetCodes = model.ui.page == "edit" ? model.sequenceResult.objects("asset_status").compactMap { $0.object("error")["code"] as? String } : []
        let codes = failures.map(\.code) + assetCodes
        guard !codes.isEmpty else { return nil }
        return .init(Set(codes).sorted().joined(separator: ", "), "\(codes.count) 件")
    }
    var jobSummary: KRJobSummary? {
        let active = model.jobs.filter { ["queued", "running"].contains($0.string("status")) }
        guard let first = active.first else { return nil }
        let progress = first.number("completed_frames") / max(1, first.number("total_frames"))
        return .init("書き出し \(Int(progress * 100))%", progress: progress, remainingCount: active.count - 1)
    }
    var historyText: String { jsonText(model.history) }
    func jsonText(_ object: Any) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys]) else { return "" }
        return String(data: data, encoding: .utf8) ?? ""
    }
}

struct MotionPage: View {
    @ObservedObject var model: EditorModel
    @ObservedObject var workflow: WorkflowSettings
    @Binding var historyOpen: Bool
    var body: some View {
        // Panel visibility and sizes live in UserDefaults (FLOW-001); the
        // per-project workspace state only keeps tool strip and column flags.
        let layout = workflow.layout(for: "motion")
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                if layout.leadingPanel { LayersPanel(model: model).frame(width: layout.leadingWidth) }
                MotionViewer(model: model, workflow: workflow)
                if layout.trailingPanel { InspectorPanel(model: model, historyOpen: $historyOpen).frame(width: layout.trailingWidth) }
            }.frame(maxHeight: .infinity)
            if layout.bottomPanel { DopeSheet(model: model).frame(height: layout.bottomHeight) }
        }
    }
}

struct LayersPanel: View {
    @ObservedObject var model: EditorModel
    @State private var tab = "layers"
    @State private var search = ""
    var body: some View {
        KRPanel(header: { KRTabBar([.init("layers", "Layers"), .init("project", "Project")], selection: $tab) }, actions: {}) {
            VStack(spacing: 0) {
                KRSearchField("レイヤーを検索", text: $search).padding(KRSpace.space2)
                ScrollView {
                    VStack(spacing: 0) {
                        if tab == "layers" {
                            ForEach(visibleLayers) { layer in
                                KRLayerRow(layer.name, kind: layer.designKind, level: layer.level, hasChildren: !layer.children.isEmpty,
                                    expanded: !model.ui.collapsed.contains(layer.id), transformParent: layer.parent.flatMap { id in model.layers.first { $0.id == id }?.name },
                                    hidden: !layer.enabled, locked: model.ui.locked.contains(layer.id), selected: model.ui.selection == layer.id,
                                    diagnostic: layer.kind == "text" && model.previewFailure?.code == "FONT_MISSING" ? .init("FONT_MISSING", model.previewFailure!.message) : nil,
                                    onSelect: { model.select(layer.id) }, onDisclosure: { toggle(layer.id) },
                                    onVisibility: { model.toggleEnabled(layer) }, onLock: { model.toggleLock(layer.id) })
                            }
                        } else {
                            ForEach(model.compositions, id: \.selfID) { composition in
                                KRAssetRow("Composition", kind: .composition, meta: composition.string("id"), duration: model.durationCode,
                                    onSelect: { model.setComposition(composition.string("id")) })
                            }
                        }
                    }
                }
                Spacer(minLength: 0)
            }
        }
    }
    var visibleLayers: [Layer] {
        var hiddenLevel: Int?
        return model.layers.filter { layer in
            if let level = hiddenLevel { if layer.level > level { return false }; hiddenLevel = nil }
            if model.ui.collapsed.contains(layer.id) { hiddenLevel = layer.level }
            return search.isEmpty || layer.name.localizedCaseInsensitiveContains(search)
        }
    }
    func toggle(_ id: String) {
        if model.ui.collapsed.contains(id) { model.ui.collapsed.remove(id) } else { model.ui.collapsed.insert(id) }
    }
}

extension Layer {
    var designKind: KRLayerKind {
        switch kind { case "shape": return .shape; case "text": return .text; case "group": return .group; case "composition_instance": return .composition; default: return .null }
    }
}
extension Dictionary where Key == String, Value == Any { var selfID: String { string("id") } }
