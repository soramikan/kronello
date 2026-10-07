import Foundation

extension EditorModel {
    public func matteRelation(_ layer: Layer) -> [String: Any]? {
        document.objects("mattes").first { $0.string("composition") == current.string("id") && $0.string("source") == layer.id }
    }
    public var matteCandidates: [[String: Any]] { current.objects("nodes") }
    public func setMatte(_ layer: Layer, target: String? = nil, kind: String? = nil, invert: Bool? = nil, visible: Bool? = nil) {
        guard !ui.locked.contains(layer.id), current.objects("nodes").contains(where: { $0.string("id") == layer.id }) else { return }
        let old = matteRelation(layer)
        if target == "none" {
            if let old { submit([["matte_remove": ["id": old.string("id")]]], label: "Matte の解除") }
            return
        }
        let destination = target ?? old?.string("matte") ?? ""
        guard !destination.isEmpty else { return }
        let relation: [String: Any] = ["id": old?.string("id") ?? UUID().uuidString,
            "version": 1, "composition": current.string("id"), "source": layer.id, "matte": destination,
            "kind": kind ?? old?.string("kind") ?? "alpha", "invert": invert ?? old?["invert"] as? Bool ?? false,
            "visible": visible ?? old?["visible"] as? Bool ?? false]
        submit([["matte_set": ["matte": relation]]], label: "Matte 関係の変更")
    }
}
