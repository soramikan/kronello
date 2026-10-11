import SwiftUI
import KronelloAppModel
import KronelloDesign

struct TemplatePage: View {
    @Environment(\.krPalette) var p
    @Environment(\.krTheme) var theme
    @ObservedObject var editor: EditorModel
    @StateObject private var model: TemplatePageModel
    @State private var applyInputs = false
    @State private var migrationOpen = false
    init(model: EditorModel) { editor = model; _model = StateObject(wrappedValue: TemplatePageModel(editor: model)) }
    var body: some View {
        KRTemplatePageLayout(templates: { templates }, comparison: { comparison }, policy: { policy }, inputs: { inputs })
            .task(id: editor.revision) { await model.load() }
            .sheet(isPresented: $migrationOpen) { migrationSheet.krTheme(theme) }
    }
    var templates: some View {
        KRPanel("Templates") {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if model.definitions.isEmpty { KREmptyState(icon: .layoutTemplate, title: "テンプレートがありません", message: "共有 template.define で定義した版をここで比較できます。") }
                    ForEach(model.definitions, id: \.selfID) { definition in
                        Button {
                            model.selectEdition(definition.string("id")); Task { await model.refresh() }
                        } label: {
                            HStack(spacing: KRSpace.space2) {
                                KRIconView(.layoutTemplate)
                                VStack(alignment: .leading, spacing: KRSpace.space1) {
                                    Text(definition.string("template_id")).krText(KRType.ruler).lineLimit(1)
                                    Text("公開入力 \(definition.object("public_inputs").count) · 配置 \(model.placements.filter { $0.string("definition_ref") == definition.string("id") }.count)")
                                        .krText(KRType.caption).foregroundStyle(p.inkMuted)
                                    Text(definition.string("version")).krText(KRType.ruler).padding(3).overlay { Rectangle().strokeBorder(p.lineStrong, lineWidth: 1) }
                                }
                            }.padding(KRSpace.space3).frame(maxWidth: .infinity, alignment: .leading)
                                .background(model.editionID == definition.string("id") ? p.selectionBg : .clear)
                        }.buttonStyle(.plain).krControlFocusRing().krBottomLine()
                    }
                }
            }
        }
    }
    var comparison: some View {
        KRPanel(header: {
            KRPanelTitle("Variant の比較")
            KRSegmentedControl([.init("layout", "layout"), .init("ink", "ink"), .init("visual", "visual")], selection: $model.bounds)
        }, actions: {
            Text(KRTimecode.format(frames: model.compareFrame, fps: model.nominalFPS)).krText(KRType.ruler).foregroundStyle(p.accentInk)
        }) {
            VStack(spacing: KRSpace.space2) {
                if let conflict = editor.revisionConflict { KRConflictBanner(.init(conflict.code,conflict.message), discard: { model.discardCandidate(); Task { await model.refresh() } }, reapply: { Task { await model.retryCandidate() } }) }
                if model.loading { HStack { KRActivityIndicator(); Text("比較を読み込み中").krText(KRType.caption) }.padding(KRSpace.space2) }
                if let failure = model.failure { KRErrorLine(.init(failure.code, failure.message)).padding(KRSpace.space2) }
                HStack { Text("比較時刻").krText(KRType.label); WorkflowFrameField(Double(model.compareFrame), step: 1, range: 0...Double(max(0, model.durationFrames-1)), unit: "f", width: 96, accessibilityLabel: "比較時刻", onCommit: { _, value in model.compareFrame = Int64(value); Task { await model.refresh() } }) }.padding(.horizontal, KRSpace.space3)
                ScrollView(.horizontal) {
                    HStack(alignment: .top, spacing: KRSpace.space3) {
                        ForEach(model.previews) { preview in
                            KRTemplateVariantView(preview.id.isEmpty ? "base" : preview.id, extent: preview.extent, bounds: preview.bounds(model.bounds), stage: model.bounds,
                                diagnostic: preview.failure.map { .init($0.code, $0.message) }).frame(width: max(240, min(360, preview.extent.width * 4)))
                        }
                    }.padding(KRSpace.space3)
                }
                Spacer(minLength: 0)
            }
        }
    }
    var inputs: some View {
        KRPanel(header: { KRTabBar([.init("inputs", "公開入力"), .init("versions", "版")], selection: $model.tab) }, actions: {}) {
            VStack(alignment: .leading, spacing: KRSpace.space2) {
                ScrollView {
                    VStack(alignment: .leading, spacing: KRSpace.space3) {
                        if model.tab == "inputs" {
                            KRPopupButton("配置", options: [.init("", "比較だけ（作品を変更しない）")] + model.placements.filter { $0.string("definition_ref") == model.editionID }.map { .init($0.string("id"), "配置 " + String($0.string("id").prefix(8))) }, selection: $model.placementID, onSelect: { _ in model.selectPlacement(model.placementID); Task { await model.refresh() } })
                            KRCheckbox("選択した配置の公開入力へ適用", isOn: $applyInputs).disabled(model.placement.isEmpty)
                            ForEach(model.publicInputs.keys.sorted(), id: \.self) { name in TemplatePublicInput(model: model, name: name, applyToPlacement: applyInputs && !model.placement.isEmpty) }
                        } else {
                            Text("公開済み \(model.definition.string("version")) は変更できません。下書きの尺ポリシーを新版として公開できます。").krText(KRType.body)
                            KRTextField("新版の版", value: $model.publishVersion)
                            KRButton("新版として公開…", variant: .secondary) { Task { await model.publish() } }.disabled(editor.busy || model.definition.isEmpty)
                            KRPopupButton("移行先の版", options: model.definitions.filter { $0.string("template_id") == model.definition.string("template_id") }.map { .init($0.string("id"), $0.string("version")) }, selection: $model.nextEdition)
                            KRPopupButton("移行先の variant", options: [.init("", "base")] + (model.definitions.first(where: { $0.string("id") == model.nextEdition }).map { $0.object("variants").keys.sorted().map { KRPopupOption($0, $0) } } ?? []) , selection: $model.nextVariant)
                            Text("版の公開や比較だけでは既存の配置を更新しません。移行は保存した公開入力を引き継ぎます。計画を確認し、明示して適用してください。").krText(KRType.caption).foregroundStyle(p.inkMuted)
                        }
                    }.padding(KRSpace.space3)
                }
                Spacer(minLength: 0)
                Text("既存の配置は元の版に固定され、自動では更新しません。").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                KRButton("移行計画…", variant: .secondary) { Task { await model.planMigration(); migrationOpen = model.migration != nil } }
                    .disabled(model.placement.isEmpty || editor.busy).padding(KRSpace.space3)
            }
        }
    }
    var policy: some View {
        KRPanel(header: { KRPanelTitle("尺のポリシー（新版の下書き）") }, actions: {
            KRPopupButton("中間区間", options: ["hold","loop","stretch"].map { .init($0, $0) }, selection: $model.middleMode).frame(width: 110)
            WorkflowFrameField(Double(model.minimumFrames), step: 1, range: 0...100000, unit: "f", width: 96, accessibilityLabel: "最小中間尺", onCommit: { _, v in model.minimumFrames = Int64(v) })
        }) {
            VStack(alignment: .leading, spacing: KRSpace.space2) {
                HStack {
                    Text("プレビューの尺").krText(KRType.label)
                    WorkflowFrameField(Double(model.durationFrames), step: 1, range: 1...100000, unit: "f", width: 100, accessibilityLabel: "プレビューの尺", onCommit: { _, v in model.durationFrames = Int64(v); Task { await model.refresh() } })
                    Text(KRTimecode.format(frames: model.durationFrames, fps: model.nominalFPS)).krText(KRType.ruler)
                    KRButton("配置の尺へ適用", variant: .secondary) { Task { await model.setPlacementDuration() } }.disabled(model.placement.isEmpty || editor.busy)
                }.padding(.horizontal, KRSpace.space3)
                KRDurationPolicyBands(authoring: rational(model.sourceComposition.object("duration")), placement: KRTimecode.format(frames: model.durationFrames, fps: model.nominalFPS),
                    intro: rational(model.definition.object("duration_policy").object("intro")), outro: rational(model.definition.object("duration_policy").object("outro")), mode: model.definition.object("duration_policy").string("middle_mode"),
                    authoringSeconds: seconds(model.sourceComposition.object("duration")), placementSeconds: seconds(model.time(model.durationFrames)),
                    introSeconds: seconds(model.definition.object("duration_policy").object("intro")), outroSeconds: seconds(model.definition.object("duration_policy").object("outro")))
                Text("intro + outro + minimum_middle 未満は DURATION_TOO_SHORT。上の下書きポリシーは「新版として公開…」で保存し、配置は明示した版移行で変えます。")
                    .krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
            }.padding(.vertical, KRSpace.space1)
        }
    }
    func seconds(_ raw: [String:Any]) -> Double { (Double(raw.string("num")) ?? 0) / max(1,Double(raw.string("den")) ?? 1) }
    func rational(_ raw: [String: Any]) -> String {
        let num = raw.string("num"), den = raw.string("den")
        return (num.isEmpty || den.isEmpty) ? "—" : num + "/" + den + "s"
    }
    var migrationSheet: some View {
        let raw = model.migration ?? [:]
        return KRDialog("移行計画", body: "差分を確認して適用してください。既存の配置は明示した適用まで変わりません。",
            detail: raw.objects("changes").map { $0.string("field") + "\n  " + jsonText($0["before"] ?? NSNull()) + " → " + jsonText($0["after"] ?? NSNull()) }.joined(separator: "\n")
                + "\n診断: " + jsonText(raw.object("after")["diagnostic"] ?? NSNull()),
            actions: [.init("cancel", "キャンセル", variant: .plain) { migrationOpen = false },
                .init("apply", model.canApplyMigration ? "移行を適用" : "計画が古いため再確認", variant: .secondary) { guard model.canApplyMigration else { migrationOpen = false; return }; migrationOpen = false; Task { await model.applyMigration() } }])
    }
}

private struct TemplatePublicInput: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: TemplatePageModel
    let name: String
    let applyToPlacement: Bool
    var schema: [String: Any] { model.publicInputs.object(name) }
    var value: [String: Any] { model.value(name) }
    var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space1) {
            HStack { KRIconView(.link, size: 12); Text(name).krText(KRType.timecode); Spacer(); Text(schema.string("value_type")).krText(KRType.caption).foregroundStyle(p.inkMuted) }
            KRTemplateValueEditor(value: value, schema: schema, assets: model.editor.document.objects("assets")) { next in
                Task { await model.editInput(name, value: next, applyToPlacement: applyToPlacement) }
            }.disabled(model.editor.busy || model.editor.pendingCandidate != nil)
            Text("default あり · " + targetLabel).krText(KRType.caption).foregroundStyle(p.inkMuted).lineLimit(3)
        }.padding(.vertical, KRSpace.space1)
    }
    var targetLabel: String {
        let target = schema.object("target")
        let reference = target.object("text").isEmpty ? target.object("property") : target.object("text")
        let nodeID = reference.string("node")
        let node = model.editor.compositions.flatMap { $0.objects("nodes") }.first { $0.string("id") == nodeID }
        let name = node?.string("name") ?? ""
        return (name.isEmpty ? String(nodeID.prefix(8)) : name) + " › " + (target["text"] != nil ? "Text" : target["property"] != nil ? "Property" : target["data_table"] != nil ? "DataTable bindings" : "MediaSlot")
    }

}
private func jsonText(_ raw: Any) -> String {
    guard let data = try? JSONSerialization.data(withJSONObject: raw, options: [.sortedKeys, .fragmentsAllowed]) else { return "" }
    return String(data: data, encoding: .utf8) ?? ""
}
