import Foundation
import CoreGraphics

public struct SpatialPath {
    public let points: [CGPoint]
    public let keys: [CGPoint]
    public init(points: [CGPoint] = [], keys: [CGPoint] = []) { self.points = points; self.keys = keys }
}
extension EditorModel {
    public static func pathFrames(_ duration: Int64) -> [Int64] {
        let count = min(600, max(0, duration))
        guard count > 0 else { return [] }
        if count == 1 { return [0] }
        let step = (duration - 1) / (count - 1), remainder = (duration - 1) % (count - 1)
        return (0..<count).map { $0 * step + $0 * remainder / (count - 1) }
    }
    /// Shared evaluated scene queries include transforms, modifiers and expression semantics.
    /// Never substitute local curve interpolation for spatial-path evaluation.
    public func spatialPath() async throws -> SpatialPath {
        guard let layer = selected, let property = layer.property("kronello.transform.position"), curveID(property) != nil else { return .init() }
        let composition = current.string("id"), base = revision, keyTimes = keyframes(property).map(keyTime)
        var samples: [RationalTime: CGPoint] = [:]
        let times = try Self.pathFrames(durationFrames).map { frame in
            let product = frame.multipliedReportingOverflow(by: rateDen)
            guard !product.overflow else { throw ServiceFailure(code: "EVALUATION_ERROR", message: "空間パスのフレーム時刻を表現できません") }
            return RationalTime(num: product.partialValue, den: rateNum)
        }
        for time in times + keyTimes {
            try Task.checkCancellation()
            if samples[time] != nil { continue }
            let scene = try await request("scene.query", ["composition": composition, "evaluation": ["time": time.wire, "fonts": fonts]])
            guard scene.string("revision") == base else { throw ServiceFailure(code: "REVISION_CONFLICT", message: "空間パスの評価中に作品が変更されました") }
            guard let node = scene.objects("nodes").first(where: { $0.object("key").string("node") == layer.id }) else { continue }
            let evaluated = node.object("evaluated")
            guard let matrix = evaluated["world_transform"] as? [[Double]], matrix.count == 2, matrix.allSatisfy({ $0.count == 3 }) else { throw ServiceFailure(code: "EVALUATION_ERROR", message: "Position の空間パスを評価できません") }
            let anchor = layer.property("kronello.transform.anchor").flatMap { evaluated.object("properties").object($0.string("id"))["value"] as? [Double] } ?? [0, 0]
            guard anchor.count == 2 else { throw ServiceFailure(code: "EVALUATION_ERROR", message: "Anchor を評価できません") }
            samples[time] = .init(x: matrix[0][0] * anchor[0] + matrix[0][1] * anchor[1] + matrix[0][2], y: matrix[1][0] * anchor[0] + matrix[1][1] * anchor[1] + matrix[1][2])
        }
        return .init(points: times.compactMap { samples[$0] }, keys: keyTimes.compactMap { samples[$0] })
    }
}
