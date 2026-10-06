import Foundation
import Combine
import CoreGraphics
import KronelloDesign

/// InstancePath + NodeId is the presentation identity, including nested instances.
public struct TemplateInspectionNode: Identifiable {
    public let path: [String]
    public let layer: Layer
    public var id: String { (path + [layer.id]).joined(separator: "/") }
    public var title: String { layer.name }
    public init(path: [String], layer: Layer) { self.path = path; self.layer = layer }
    public func values(_ property: [String: Any]) -> [String] {
        let value = layer.value(property), presentation = PropertyPresentation.of(property)
        if let vector = value["value"] as? [Double] {
            return vector.map { String(format: "%.1f", $0 * presentation.multiplier) }
        }
        if let scalar = value["value"] as? NSNumber { return [presentation.curveReadout(scalar.doubleValue)] }
        if value.string("kind") == "color" {
            let c = value.object("value").object("components")
            return [String(format: "#%02X%02X%02X", Int((c.number("r") * 255).rounded()),
                           Int((c.number("g") * 255).rounded()), Int((c.number("b") * 255).rounded()))]
        }
        return [value.string("value")]
    }
    public func selection(stage: String, extent: CGSize) -> KRViewerSelection? {
        guard let bounds = layer.bounds(stage), extent.width > 0, extent.height > 0 else { return nil }
        return .init(CGRect(x: bounds.minX / extent.width, y: bounds.minY / extent.height,
                            width: bounds.width / extent.width, height: bounds.height / extent.height),
                     label: "\(stage) \(Int(bounds.width.rounded())) × \(Int(bounds.height.rounded()))")
    }
}

/// A read-only query cache shared by the Inspector and Viewer of one EditorModel.
/// The weak owner avoids modifying shared EditorModel storage or retaining windows.
@MainActor public final class TemplateInstanceInspection: ObservableObject {
    private static var stores: [ObjectIdentifier: TemplateInstanceInspection] = [:]
    private weak var owner: EditorModel?
    @Published public private(set) var nodes: [TemplateInspectionNode] = []
    @Published public var selection: String?
    @Published public private(set) var failure: ServiceFailure?
    @Published public private(set) var stale = false
    @Published public private(set) var loading = false
    private var loadedKey: String?
    private var context = ""
    private var generation = 0
    public var selected: TemplateInspectionNode? { nodes.first { $0.id == selection } }
    public func matchesSelection(_ model: EditorModel) -> Bool {
        context == (model.ui.composition ?? "") + "/" + (model.ui.selection ?? "")
    }
    public static func shared(for model: EditorModel) -> TemplateInstanceInspection {
        stores = stores.filter { $0.value.owner != nil }
        let key = ObjectIdentifier(model)
        if let store = stores[key] { return store }
        let store = TemplateInstanceInspection(owner: model); stores[key] = store; return store
    }
    public init(owner: EditorModel) { self.owner = owner }
    public static func instance(_ layer: Layer?, document: [String: Any]) -> String? {
        guard let layer, layer.kind == "composition_instance" else { return nil }
        let id = layer.authored.object("kind").object("value").string("id")
        return document.objects("template_instances").contains { $0.string("id") == id } ? id : nil
    }
    public static func refreshKey(_ model: EditorModel) -> String {
        [model.revision, model.ui.composition ?? "", model.ui.selection ?? "", model.ui.time.num,
         model.ui.time.den, model.playing ? "playing" : "paused"].joined(separator: "/")
    }
    public static func decode(scene: [String: Any], document: [String: Any], instance: String) throws -> [TemplateInspectionNode] {
        let authored = document.objects("compositions").flatMap { $0.objects("nodes") }
        let byID = Dictionary(uniqueKeysWithValues: authored.map { ($0.string("id"), $0) })
        return try scene.objects("nodes").compactMap { node in
            let key = node.object("key"), path = key["instance_path"] as? [String] ?? []
            guard path.first == instance else { return nil }
            guard let source = byID[key.string("node")] else {
                throw ServiceFailure(code: "INVALID_RESPONSE", message: "内部レイヤーの参照を読み取れません")
            }
            // Inactive descendants are represented by the query, not fabricated values.
            guard !node.object("evaluated").isEmpty else { return nil }
            return TemplateInspectionNode(path: path, layer: .init(id: key.string("node"), authored: source,
                evaluated: node.object("evaluated"), level: path.count))
        }
    }
    /// SwiftUI cancels superseded tasks; generation also rejects late FFI replies.
    /// Debounce before issuing a SINGLE expanded query, never a per-node/per-frame loop.
    public func refresh(_ model: EditorModel, debounce: Duration = .milliseconds(150)) async {
        // A cancelled SwiftUI task may enter after its replacement has started.
        // It must not steal that request's generation or mutate presentation state.
        guard !Task.isCancelled else { return }
        generation += 1; let issued = generation
        let nextContext = (model.ui.composition ?? "") + "/" + (model.ui.selection ?? "")
        if context != nextContext {
            context = nextContext; nodes = []; selection = nil; failure = nil; loadedKey = nil
        }
        guard let instance = Self.instance(model.selected, document: model.document) else { loading = false; stale = false; return }
        let key = [model.revision, nextContext, model.ui.time.num, model.ui.time.den].joined(separator: "/")
        stale = model.playing || loadedKey != key
        if model.playing { loading = false; return }
        guard loadedKey != key else { stale = false; return }
        loading = true
        defer { if generation == issued { loading = false } }
        do {
            try await Task.sleep(for: debounce)
            try Task.checkCancellation()
            guard issued == generation, !model.playing,
                  key == [model.revision, (model.ui.composition ?? "") + "/" + (model.ui.selection ?? ""), model.ui.time.num, model.ui.time.den].joined(separator: "/") else { return }
            let revision = model.revision, document = model.document
            let result = try await model.request("scene.query", ["composition": model.ui.composition ?? "", "expand_instances": true,
                "evaluation": ["time": model.ui.time.wire, "fonts": model.fonts]])
            try Task.checkCancellation()
            guard generation == issued, !model.playing,
                  key == [model.revision, (model.ui.composition ?? "") + "/" + (model.ui.selection ?? ""), model.ui.time.num, model.ui.time.den].joined(separator: "/") else { return }
            guard result.string("revision") == revision, model.revision == revision else {
                throw ServiceFailure(code: "REVISION_CONFLICT", message: "内部レイヤーの検査中に作品が変更されました")
            }
            let decoded = try Self.decode(scene: result, document: document, instance: instance)
            guard !decoded.isEmpty else { throw ServiceFailure(code: "EVALUATION_ERROR", message: "この時刻に表示できる内部レイヤーがありません") }
            nodes = decoded
            if !nodes.contains(where: { $0.id == selection }) { selection = nodes.first?.id }
            loadedKey = key; stale = false; failure = nil
        } catch is CancellationError {} catch {
            guard generation == issued else { return }
            failure = model.serviceFailure(error); nodes = []; selection = nil
        }
    }
}
