import Foundation
import CoreGraphics

/// Position samples in the selected layer's parent space, never editable state.
public struct SpatialPath {
    public let points: [CGPoint]
    public let keys: [CGPoint]
    public init(points: [CGPoint] = [], keys: [CGPoint] = []) { self.points = points; self.keys = keys }
    public func mapped(by parent: CGAffineTransform) -> SpatialPath {
        .init(points: points.map { $0.applying(parent) }, keys: keys.map { $0.applying(parent) })
    }
}
extension EditorModel {
    public static func pathFrames(_ duration: Int64) -> [Int64] {
        let count = min(600, max(0, duration))
        guard count > 0 else { return [] }
        if count == 1 { return [0] }
        let step = (duration - 1) / (count - 1), remainder = (duration - 1) % (count - 1)
        return (0..<count).map { $0 * step + $0 * remainder / (count - 1) }
    }
    /// One immutable shared evaluation for all frame and key times. No render-scene
    /// sampling: Position exists even when a key lies outside the node's active range.
    /// Local-transform * Anchor equals Position, so Anchor need not be sampled.
    public func spatialPath() async throws -> SpatialPath {
        guard let layer = selected, let property = layer.property("kronello.transform.position"), curveID(property) != nil else { return .init() }
        let composition = current.string("id"), base = revision, keyTimes = keyframes(property).map(keyTime)
        let times = try Self.pathFrames(durationFrames).map { frame in
            let product = frame.multipliedReportingOverflow(by: rateDen)
            guard !product.overflow else { throw ServiceFailure(code: "EVALUATION_ERROR", message: "空間パスのフレーム時刻を表現できません") }
            return RationalTime(num: product.partialValue, den: rateNum)
        }
        var requested: [RationalTime] = [], seen: Set<RationalTime> = []
        for time in times + keyTimes where seen.insert(time).inserted { requested.append(time) }
        guard !requested.isEmpty else { return .init() }
        guard requested.count <= 100_000 else { throw ServiceFailure(code: "EVALUATION_ERROR", message: "空間パスの評価時刻が上限を超えました") }
        try Task.checkCancellation()
        let key: [String: Any] = ["kind": "node", "instance_path": [String](), "node": layer.id, "property": property.string("id")]
        // Omitting fonts selects property evaluation, not active render-scene values.
        let result = try await request("property.sample", ["composition": composition, "keys": [key], "times": requested.map(\.wire)])
        try Task.checkCancellation()
        guard result.string("revision") == base, revision == base else { throw ServiceFailure(code: "REVISION_CONFLICT", message: "空間パスの評価中に作品が変更されました") }
        guard current.string("id") == composition, selected?.id == layer.id else { throw CancellationError() }
        guard result.string("composition") == composition,
              result.objects("times").map({ RationalTime(num: Int64($0.string("num")) ?? 0, den: Int64($0.string("den")) ?? 1) }) == requested,
              let sample = result.objects("samples").first(where: {
                  let k = $0.object("key")
                  return k.string("kind") == "node" && k.string("node") == layer.id && k.string("property") == property.string("id") && k["instance_path"] as? [String] == []
              }), sample.string("value_type") == "vec2", sample.objects("values").count == requested.count else {
            throw ServiceFailure(code: "EVALUATION_ERROR", message: "選択した Position の空間パス応答を読み取れません（\(layer.id)）")
        }
        let points = try sample.objects("values").map { value -> CGPoint in
            guard value.string("kind") == "vec2", let numbers = spatialNumbers(value["value"], count: 2) else {
                throw ServiceFailure(code: "EVALUATION_ERROR", message: "Position の空間パスを評価できません")
            }
            return .init(x: numbers[0], y: numbers[1])
        }
        let samples = Dictionary(uniqueKeysWithValues: zip(requested, points))
        return .init(points: times.map { samples[$0]! }, keys: keyTimes.map { samples[$0]! })
    }
    /// Reuse the current scene's exact parent identity and already-composed matrix.
    /// A playhead change only remaps cached points; no trajectory request is needed.
    public func spatialPathParentTransform() throws -> CGAffineTransform? {
        guard let layer = selected, let property = layer.property("kronello.transform.position"), curveID(property) != nil else { return nil }
        guard let node = scene.objects("nodes").first(where: { $0.object("key").string("node") == layer.id && $0.object("key")["instance_path"] as? [String] == [] }) else {
            throw ServiceFailure(code: "EVALUATION_ERROR", message: "空間パスの選択ノードが scene にありません（\(layer.id)）")
        }
        guard node["transform_parent"] != nil else { throw ServiceFailure(code: "EVALUATION_ERROR", message: "空間パスの親情報が scene にありません") }
        let parent = node.object("transform_parent")
        if parent.isEmpty { return .identity }
        guard let evaluated = scene.objects("nodes").first(where: {
            let key = $0.object("key")
            return key.string("node") == parent.string("node") && key["instance_path"] as? [String] == parent["instance_path"] as? [String]
        }) else { throw ServiceFailure(code: "EVALUATION_ERROR", message: "空間パスの親ノードが scene にありません") }
        guard let rows = evaluated.object("evaluated")["world_transform"] as? [Any], rows.count == 2,
              let a = spatialNumbers(rows[0], count: 3), let b = spatialNumbers(rows[1], count: 3) else {
            throw ServiceFailure(code: "EVALUATION_ERROR", message: "現在時刻の親 transform を評価できません（非アクティブの親も確認してください）")
        }
        return .init(a: a[0], b: b[0], c: a[1], d: b[1], tx: a[2], ty: b[2])
    }
}

/// JSONSerialization may supply integers or NSNumber; decode each finite number.
private func spatialNumbers(_ raw: Any?, count: Int) -> [Double]? {
    guard let values = raw as? [Any], values.count == count else { return nil }
    let numbers = values.compactMap { ($0 as? NSNumber)?.doubleValue }
    return numbers.count == count && numbers.allSatisfy(\.isFinite) ? numbers : nil
}
