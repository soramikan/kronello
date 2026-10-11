import Foundation

/// Derived lookup keys for supported SourceRef data, never authored object IDs.
enum TimelineSourceKey: Hashable {
    case asset(String, String), composition(String), multicam(String, String)
    init?(_ source: [String: Any]) {
        switch source.string("kind") {
        case "asset":
            guard source.count == 3, source["asset"] != nil, source["stream_index"] != nil else { return nil }
            self = .asset(source.string("asset"), source.string("stream_index"))
        case "composition":
            guard source.count == 2, source["composition"] != nil else { return nil }
            self = .composition(source.string("composition"))
        case "multicam":
            guard source.count == 3, source["multicam"] != nil, source["angle"] != nil else { return nil }
            self = .multicam(source.string("multicam"), source.string("angle"))
        default: return nil
        }
    }
}

/// Immutable presentation indexes rebuilt when the shared sequence query changes.
struct TimelineIndex {
    var clips: [EditClip] = []
    var clipsByID: [String: EditClip] = [:]
    var clipsByTrack: [String: [EditClip]] = [:]
    var tracksByID: [String: [String: Any]] = [:]
    var trackNumbers: [String: String] = [:]
    var assetStatus: [String: [String: Any]] = [:]
    init(_ result: [String: Any] = [:]) {
        clips = result.objects("clips").map(EditClip.init)
        for clip in clips {
            if clipsByID[clip.id] == nil { clipsByID[clip.id] = clip }
            clipsByTrack[clip.track, default: []].append(clip)
        }
        var counts: [String: Int] = [:]
        for track in result.object("sequence").objects("tracks") {
            let id = track.string("id"), kind = track.string("kind")
            counts[kind, default: 0] += 1
            if tracksByID[id] == nil {
                tracksByID[id] = track
                trackNumbers[id] = (kind == "audio" ? "A" : kind == "caption" ? "C" : "V") + String(counts[kind]!)
            }
        }
        for row in result.objects("asset_status") where assetStatus[row.string("asset")] == nil {
            assetStatus[row.string("asset")] = row
        }
    }
}

struct TimelineAssetIndex {
    let assets: [EditAsset]
    let bySource: [TimelineSourceKey: EditAsset]
    init(_ assets: [EditAsset]) {
        self.assets = assets
        var result: [TimelineSourceKey: EditAsset] = [:]
        for asset in assets {
            if let key = TimelineSourceKey(asset.source), result[key] == nil { result[key] = asset }
        }
        bySource = result
    }
}

struct TimelineRateKey: Hashable { let num: Int64; let den: Int64 }
