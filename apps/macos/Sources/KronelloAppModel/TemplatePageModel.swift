import Foundation
import Combine
import CoreGraphics

public struct TemplateVariantPreview: Identifiable {
    public let id: String
    public let result: [String: Any]
    public let failure: ServiceFailure?
    public var extent: CGSize { CGSize(width: result.object("design_extent").number("width"), height: result.object("design_extent").number("height")) }
    public var nodes: [[String: Any]] { result.objects("nodes") }
    public func bounds(_ stage: String) -> [CGRect] { nodes.compactMap { raw in
        let b = raw.object("evaluated").object("bounds").object(stage + "_bounds")
        guard let min = b["min"] as? [Double], let max = b["max"] as? [Double], min.count == 2, max.count == 2 else { return nil }
        return CGRect(x: min[0], y: min[1], width: max[0] - min[0], height: max[1] - min[1])
    } }
}

@MainActor public final class TemplatePageModel: ObservableObject {
    public let editor: EditorModel
    @Published public var editionID = ""
    @Published public var placementID = ""
    @Published public var bounds = "layout"
    @Published public var tab = "inputs"
    @Published public var previewInputs: [String: Any] = [:]
    @Published public var durationFrames: Int64 = 192
    @Published public var compareFrame: Int64 = 0
    @Published public var nextEdition = ""
    @Published public var nextVariant = ""
    @Published public var publishVersion = "1.1.0"
    @Published public var middleMode = "stretch"
    @Published public var minimumFrames: Int64 = 12
    @Published public private(set) var previews: [TemplateVariantPreview] = []
    @Published public private(set) var migration: [String: Any]?
    @Published public private(set) var failure: ServiceFailure?
    @Published public private(set) var loading = false
    private var generation = 0
    private var previewKey: String?
    private let previewInstanceID = UUID().uuidString
    private var candidateRevision = "0"
    private var migrationBase: String?
    private var migrationKey: String?
    public init(editor: EditorModel) { self.editor = editor }
    public var definitions: [[String: Any]] { editor.document.objects("templates") }
    public var placements: [[String: Any]] { editor.document.objects("template_instances") }
    public var definition: [String: Any] { definitions.first { $0.string("id") == editionID } ?? [:] }
    public var placement: [String: Any] { placements.first { $0.string("id") == placementID && $0.string("definition_ref") == editionID } ?? [:] }
    public var publicInputs: [String: Any] { definition.object("public_inputs") }
    public var variants: [String] { [""] + definition.object("variants").keys.sorted() }
    public var fps: Int64 { Int64(sourceComposition.object("edit_rate").string("num")) ?? 24 }
    public var fpsDen: Int64 { Int64(sourceComposition.object("edit_rate").string("den")) ?? 1 }
    public var nominalFPS: Int { max(1, Int(fps / max(1,fpsDen) + (fps % max(1,fpsDen) == 0 ? 0 : 1))) }
    public var sourceComposition: [String: Any] { editor.compositions.first { $0.string("id") == definition.string("composition_ref") } ?? [:] }
    public func time(_ frame: Int64) -> [String: Any] { let product = frame.multipliedReportingOverflow(by: fpsDen)
        guard !product.overflow else { return ["num":"invalid","den":"1"] }
        return RationalTime(num: product.partialValue, den: max(1, fps)).wire }
    public func selectEdition(_ id: String) {
        candidateRevision = editor.revision
        editionID = id; placementID = placements.first { $0.string("definition_ref") == id }?.string("id") ?? ""
        previewInputs = placement.object("inputs"); migration = nil; previews = []; previewKey = nil; failure = nil
        let d = placement.isEmpty ? sourceComposition.object("duration") : placement.object("duration")
        durationFrames = RationalTime(num: Int64(d.string("num")) ?? 8, den: Int64(d.string("den")) ?? 1).frames(rateNum: fps, rateDen: fpsDen)
        compareFrame = 0
        middleMode = definition.object("duration_policy").string("middle_mode")
        let minimum = definition.object("duration_policy").object("minimum_middle")
        minimumFrames = RationalTime(num: Int64(minimum.string("num")) ?? 0, den: Int64(minimum.string("den")) ?? 1).frames(rateNum: fps, rateDen: fpsDen)
        nextEdition = definitions.first { $0.string("template_id") == definition.string("template_id") && $0.string("id") != id }?.string("id") ?? id
        nextVariant = placement.string("variant")
    }
    public func selectPlacement(_ id: String) {
        candidateRevision = editor.revision
        placementID = id; previewInputs = placement.object("inputs")
        if !placement.isEmpty {
            let d = placement.object("duration")
            durationFrames = RationalTime(num:Int64(d.string("num")) ?? 1,den:Int64(d.string("den")) ?? 1).frames(rateNum:fps,rateDen:fpsDen)
            compareFrame = min(compareFrame,max(0,durationFrames-1))
        }
        migration = nil
    }
    public func load() async {
        if definition.isEmpty, let first = definitions.first { selectEdition(first.string("id")) }
        await refresh()
    }
    public func value(_ name: String) -> [String: Any] { previewInputs.object(name).isEmpty ? publicInputs.object(name).object("default") : previewInputs.object(name) }
    public func instance(_ variant: String) -> [String: Any] {
        var result: [String: Any] = ["id": placement.isEmpty ? previewInstanceID : placementID, "definition_ref": editionID, "version": definition.string("version"), "duration": time(durationFrames), "inputs": previewInputs]
        if !variant.isEmpty { result["variant"] = variant }
        return result
    }
    /// One query per variant per committed preview change; never driven by a frame timer.
    public func refresh() async {
        guard !definition.isEmpty else { previews = []; return }
        let fields: [String:Any] = ["revision":editor.revision,"definition":editionID,"placement":placementID,"inputs":previewInputs,"duration":time(durationFrames),"time":time(compareFrame),"fonts":editor.fonts]
        let key = (try? JSONSerialization.data(withJSONObject: fields,options:[.sortedKeys])).flatMap { String(data:$0,encoding:.utf8) } ?? UUID().uuidString
        if previewKey == key { return }
        previewKey = key
        generation += 1; let ticket = generation, revision = editor.revision
        let inputs = variants.map { ($0, instance($0)) }, sample = time(compareFrame), fonts = editor.fonts
        loading = true; migration = nil
        defer { if ticket == generation { loading = false } }
        var results: [TemplateVariantPreview] = []
        for (name, instance) in inputs {
            do {
                let result = try await editor.request("template.preview", ["instance": instance, "time": sample, "fonts": fonts])
                guard ticket == generation, revision == editor.revision else { previewKey = nil; return }
                guard result.string("revision") == revision else { previewKey = nil; failure = .init(code: "REVISION_CONFLICT", message: "比較中に作品が変更されました。再読込してください"); return }
                let diagnostic = result["diagnostic"] as? [String: Any]
                results.append(.init(id: name, result: result, failure: diagnostic.map(ExportPageModel.failure)))
            } catch { results.append(.init(id: name, result: [:], failure: editor.serviceFailure(error))) }
        }
        guard ticket == generation, revision == editor.revision else { previewKey = nil; return }
        previews = results
    }
    public func editInput(_ name: String, value: [String: Any], applyToPlacement: Bool) async {
        guard NSDictionary(dictionary: self.value(name)) != NSDictionary(dictionary: value) else { return }
        if applyToPlacement {
            guard !placement.isEmpty else { return }
            let event = await editor.apply(.init(base: candidateRevision, commands: [["template": ["set_input": ["instance": placementID, "name": name, "value": value]]]], label: "公開入力の変更"))
            guard event != nil else { return }
            candidateRevision = editor.revision
        }
        previewInputs[name] = value
        await refresh()
    }
    public func discardCandidate() {
        editor.discardCandidate()
        selectPlacement(placementID)
    }
    public func retryCandidate() async {
        guard let previous = editor.pendingCandidate else { return }
        guard await editor.apply(.init(base:editor.revision,commands:previous.commands,label:previous.label)) != nil else { return }
        candidateRevision = editor.revision
        // Synchronize presentation only after the explicit shared apply succeeds.
        for command in previous.commands {
            let template = command.object("template"), input = template.object("set_input")
            if !input.isEmpty { previewInputs[input.string("name")] = input.object("value") }
            let defined = template.object("define").object("definition")
            if !defined.isEmpty { nextEdition = defined.string("id") }
            if !template.object("migrate").isEmpty, let pin = placements.first(where:{ $0.string("id") == placementID }) { selectEdition(pin.string("definition_ref")); placementID = pin.string("id") }
        }
        await refresh()
    }
    public func setPlacementDuration() async {
        guard !placement.isEmpty else { return }
        if await editor.apply(.init(base: candidateRevision, commands: [["template": ["set_duration": ["instance": placementID, "duration": time(durationFrames)]]]], label: "配置の尺の変更")) != nil { candidateRevision = editor.revision }
        await refresh()
    }
    public func publish() async {
        guard !definition.isEmpty, !publishVersion.isEmpty, publishVersion != definition.string("version") else { return }
        var next = definition
        let id = UUID().uuidString
        next["id"] = id; next["version"] = publishVersion; next["content_hash"] = ""
        var policy = next.object("duration_policy"); policy["middle_mode"] = middleMode; policy["minimum_middle"] = time(minimumFrames); next["duration_policy"] = policy
        var variants = next.object("variants")
        for (name, raw) in variants { var variant = raw as? [String: Any] ?? [:]; variant["content_hash"] = ""; variants[name] = variant }
        next["variants"] = variants
        if await editor.apply(.init(base: candidateRevision, commands: [["template": ["define": ["definition": next]]]], label: "テンプレートの新版を公開")) != nil { nextEdition = id; candidateRevision = editor.revision }
        // Publishing never touches placement pins; migration is a separate action.
    }
    private var planKey: String { [placementID, nextEdition, nextVariant, String(compareFrame), editor.revision].joined(separator: "|") }
    public var canApplyMigration: Bool { migration != nil && migrationBase == editor.revision && migrationKey == planKey && !editor.busy && !loading && editor.pendingCandidate == nil }
    public func planMigration() async {
        migration = nil; failure = nil
        guard !placement.isEmpty else { return }
        let base = editor.revision, key = planKey
        do {
            var fields: [String: Any] = ["base_revision": base, "instance": placementID, "definition": nextEdition, "time": time(compareFrame), "fonts": editor.fonts]
            if !nextVariant.isEmpty { fields["variant"] = nextVariant }
            let plan = try await editor.request("template.migration_plan", fields)
            guard key == planKey else { return }
            migration = plan; migrationBase = base; migrationKey = key
        } catch { failure = editor.serviceFailure(error) }
    }
    public func applyMigration() async {
        guard canApplyMigration, let plan = migration?.object("plan"), let base = migrationBase else { return }
        // Shared edit.plan is deliberately repeated by EditorModel.apply to use
        // the same session Undo/receipt/conflict path as every GUI edit.
        migration = nil
        _ = await editor.apply(.init(base: base, commands: plan.objects("commands"), label: "テンプレートの版移行"))
        if let pin = placements.first(where: { $0.string("id") == placementID }) { selectEdition(pin.string("definition_ref")); placementID = pin.string("id") }
        await refresh()
    }
}
