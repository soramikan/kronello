import Foundation

/// The drop destination's candidate/release boundary, shared with native host checks.
@MainActor public struct AssetPlacementReceiver {
    public let model: EditorModel
    public let track: String
    public init(model: EditorModel, track: String) { self.model = model; self.track = track }
    public var canAccept: Bool {
        model.assetSelection != nil && !model.busy && model.pendingCandidate == nil && !model.trackLocked(track)
    }
    @discardableResult public func update(at frame: Int64) -> Bool {
        guard canAccept else { return false }
        if model.timelineCandidate == nil, let asset = model.editAssets.first(where: { $0.id == model.assetSelection }) {
            model.beginAssetGesture(asset, track: track, at: frame)
        }
        guard let candidate = model.timelineCandidate, candidate.mode == .place, candidate.track == track else { return false }
        model.updateClipGesture(at: frame)
        return true
    }
    public func exit() {
        if let candidate = model.timelineCandidate, candidate.mode == .place, candidate.track == track { model.cancelClipGesture() }
    }
    @discardableResult public func commit() async -> Bool {
        guard canAccept, let candidate = model.timelineCandidate, candidate.mode == .place, candidate.track == track else { return false }
        return await model.commitClipGesture() != nil
    }
}
