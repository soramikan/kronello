import AppKit
import SwiftUI
import KronelloAppModel
import KronelloDesign

struct EditorWindow: View {
    @Environment(\.krPalette) var p
    @Environment(\.krTheme) var theme
    @ObservedObject var model: EditorModel
    @State private var historyOpen = false
    @State private var jobsOpen = false
    @State private var safeDetailsOpen = false
    var body: some View {
        VStack(spacing: 0) {
            toolbar
            if model.safeMode { KRStateBand("安全モード：要求ごとに排他を取得します。処理中の競合は PROJECT_LOCKED。ウインドウ全体の排他は未対応です", details: { safeDetailsOpen = true }) }
            if model.ui.page == "motion" { MotionPage(model: model, historyOpen: $historyOpen) }
            else if model.ui.page == "template" { TemplatePage(model: model) }
            else if model.ui.page == "export" { ExportPage(model: model) }
            else { KREmptyState(icon: model.ui.page == "export" ? .clapperboard : .layers,
                title: model.ui.page == "edit" ? "編集ページ" : model.ui.page == "template" ? "テンプレートページ" : "書き出しページ",
                message: model.ui.page == "edit" ? "Sequence の編集は GUI-003 で追加します。モーションページで Composition を開いてください。" : "このページは GUI-004 で追加します。モーションページで作業を続けられます。")
                .frame(maxWidth: .infinity, maxHeight: .infinity) }
            KRStatusBar(saved: (model.busy ? "保存中" : "保存済み") + (model.safeMode ? " · 安全モード" : ""), revision: "rev " + model.revision,
                externalChange: model.externalChange, error: diagnostic, job: jobSummary,
                onError: { if model.undoConflict != nil {} else if model.revisionConflict == nil { model.failure = model.previewFailure } }, onJobs: { jobsOpen = true })
        }.frame(minWidth: KRWindowMetrics.width, minHeight: KRWindowMetrics.height)
            .background(p.surface100).foregroundStyle(p.ink)
            .sheet(item: $model.undoConflict) { error in
                KRDialog("『" + model.undoConflictLabel + "』を取り消せません", body: error.message + "。部分的には取り消しません。", code: error.code,
                    detail: error.detailText + "\n" + historyText,
                    actions: [.init("history", "履歴を開く…") { model.undoConflict = nil; historyOpen = true },
                              .init("ok", "OK", variant: .primary) { model.undoConflict = nil }]).krTheme(theme)
            }
            .sheet(item: $model.failure) { error in
                KRDialog("操作を完了できません", body: error.message, code: error.code, detail: error.detailText,
                    actions: [.init("ok", "OK", variant: .primary) { model.failure = nil }]).krTheme(theme)
            }
            .sheet(isPresented: $historyOpen) {
                KRDialog("History", body: "共有 history.list のイベントです。", detail: historyText,
                    actions: [.init("ok", "閉じる", variant: .primary) { historyOpen = false }]).krTheme(theme)
                    .task { do { try await model.loadHistory() } catch { model.mapFailure(error) } }
            }
            .sheet(isPresented: $jobsOpen) {
                KRDialog("Jobs", body: "共有 job_progress の最新通知です。", detail: jsonText(model.jobs),
                    actions: [.init("ok", "閉じる", variant: .primary) { jobsOpen = false }]).krTheme(theme)
            }
            .sheet(isPresented: $safeDetailsOpen) {
                KRDialog("安全モード", body: "共有 project.info が safe を返しました。現行 FFI は要求ごとに store を開閉します。ウインドウを開いているだけでは CLI / MCP を排除しません。セッション全体の排他保持は後続課題です。",
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
        let failures = [model.failure, model.undoConflict, model.revisionConflict, model.previewFailure].compactMap { $0 }
        guard !failures.isEmpty else { return nil }
        return .init(failures.map(\.code).joined(separator: ", "), "\(failures.count) 件")
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
    @Binding var historyOpen: Bool
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                LayersPanel(model: model).frame(width: model.ui.layout.leftWidth)
                MotionViewer(model: model)
                InspectorPanel(model: model, historyOpen: $historyOpen).frame(width: model.ui.layout.rightWidth)
            }.frame(maxHeight: .infinity)
            DopeSheet(model: model).frame(height: model.ui.layout.bottomHeight)
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
