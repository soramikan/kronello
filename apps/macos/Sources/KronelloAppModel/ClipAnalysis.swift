import Foundation
import KronelloCore

/// TRACK-002/003 + AI-002 clip analysis authoring (ADR-0122/0123/0125). Every
/// mutation flows through the shared operations — `track.analyze`,
/// `clip_set_effects`, `clip_time_set`, `scene.detect`, `scene.apply` — so the
/// document, undo history and service validation stay the single source of
/// truth; the model only keeps presentation state (`stabilizePending`).
extension EditorModel {
    /// `kronello-model` `STABILIZE_ID`.
    public static let stabilizeEffectID = "kronello.stabilize"

    // MARK: - TRACK-002 stabilization

    /// The media source `track.analyze` can measure for this clip, plus the
    /// clip-source→media offset (the multicam angle's `sync_offset`, zero for
    /// direct assets). Composition/generator/caption/adjustment sources have
    /// no frames the tracker can decode.
    public func trackableSource(_ clip: EditClip) -> (asset: String, stream: Int, offset: RationalTime)? {
        switch clip.sourcePreview {
        case .asset(let id, let stream):
            return (id, stream, RationalTime(num: 0, den: 1))
        case .multicam(let group, let angle):
            guard let info = multicamGroup(group)?.angles.first(where: { $0.id == angle }) else { return nil }
            return (info.asset, info.streamIndex, info.syncOffset)
        default:
            return nil
        }
    }

    /// Clip source-time coverage mapped onto the analyzed media's own clock,
    /// matching `waveformRange` (linear map offset plus optional reverse).
    private func stabilizeMediaRange(_ clip: EditClip, offset: RationalTime) throws -> (RationalTime, RationalTime) {
        let duration = try clip.end.checkedSubtracting(clip.start)
        let speed = clip.linearRate ?? RationalTime(num: 1, den: 1)
        let travel = try duration.checkedMultiplying(speed)
        let sourceIn = RationalTime.wire(clip.authored.object("source_in"))
        let mapOffset = RationalTime.wire(clip.authored.object("time_map").object("offset"))
        let first = try sourceIn.checkedAdding(mapOffset).checkedAdding(offset)
        return clip.reversed
            ? (try first.checkedSubtracting(travel), first)
            : (first, try first.checkedAdding(travel))
    }

    /// Whether the clip already carries a `kronello.stabilize` effect.
    public func clipHasStabilize(_ clip: EditClip) -> Bool {
        clip.authored.objects("effects").contains { $0.string("effect_id") == Self.stabilizeEffectID }
    }

    /// Two shared steps behind one button: `track.analyze` commits a
    /// TrackingDataAsset for the clip's media range (derived data — like
    /// `audio.analyze` it is not a session-undo event), then a normal
    /// undoable `clip_set_effects` binds `kronello.stabilize` to it with the
    /// versioned defaults from ADR-0122.
    public func stabilizeClip(_ clip: EditClip) {
        guard !busy, pendingCandidate == nil, !trackLocked(clip.track), !stabilizePending.contains(clip.id) else { return }
        // Multicam clips render video through their active angle; the
        // tracking asset binds the resolved media and evaluation rejects an
        // angle switch to an untracked source.
        guard clip.kind == .video || clip.kind == .multicam else {
            mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "スタビライズは映像クリップに適用します")); return
        }
        guard clip.timeMapKind == "linear" else {
            mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "速度ランプのクリップはスタビライズできません")); return
        }
        guard let source = trackableSource(clip) else {
            mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "このクリップのソースは解析できません")); return
        }
        guard !clipHasStabilize(clip) else { return }
        let start: RationalTime, end: RationalTime
        do { (start, end) = try stabilizeMediaRange(clip, offset: source.offset) }
        catch { mapFailure(error); return }
        let trackingID = UUID().uuidString.lowercased()
        stabilizePending.insert(clip.id)
        Task {
            do {
                // A 3-seed horizontal spread feeds translation plus rotation
                // estimation; radii follow the TRACK-002 acceptance defaults.
                _ = try await request("track.analyze", [
                    "base_revision": revision, "id": trackingID,
                    "asset": source.asset, "stream_index": source.stream, "mode": "points",
                    "seeds": [["x": 0.35, "y": 0.5, "template_radius": 4, "search_radius": 6],
                              ["x": 0.5, "y": 0.5, "template_radius": 4, "search_radius": 6],
                              ["x": 0.65, "y": 0.5, "template_radius": 4, "search_radius": 6]],
                    "range": ["start": start.wire, "end": end.wire],
                    "idempotency_key": UUID().uuidString.lowercased()])
                try await reload()
            } catch {
                stabilizePending.remove(clip.id)
                mapFailure(error)
                return
            }
            stabilizePending.remove(clip.id)
            // The document changed; re-resolve the clip and re-check the
            // effect guard before attaching.
            guard let fresh = editClips.first(where: { $0.id == clip.id }), !clipHasStabilize(fresh) else { return }
            var properties = fresh.authored.objects("properties"), effects = fresh.authored.objects("effects")
            func property(_ key: String, _ type: String, _ value: Any) -> String {
                let id = UUID().uuidString
                properties.append(["id": id, "descriptor": ["key": key, "version": 1],
                    "source": ["kind": "constant", "value": ["kind": type, "value": value]], "modifiers": []])
                return id
            }
            effects.append(["effect_id": Self.stabilizeEffectID, "version": 1,
                "parameters": ["kind": "stabilize",
                    "tracking": property("kronello.effect.tracking", "asset_ref", trackingID),
                    "smoothing_radius": property("kronello.effect.smoothing_radius", "scalar", 1.0),
                    "max_displacement": property("kronello.effect.max_displacement", "scalar", 64.0),
                    "max_rotation": property("kronello.effect.max_rotation", "angle", 5.0),
                    "max_crop": property("kronello.effect.max_crop", "scalar", 0.25),
                    "border": property("kronello.effect.border", "enum", "replicate"),
                    "fill_color": property("kronello.effect.fill_color", "color",
                        ["space": "srgb", "components": ["r": 0.0, "g": 0.0, "b": 0.0, "alpha": 0.0]]),
                    "sampling": property("kronello.effect.sampling", "enum", "bilinear")]])
            replaceClipEffects(fresh, properties: properties, effects: effects, label: "スタビライズ")
        }
    }

    // MARK: - TRACK-003 optical-flow interpolation

    /// `OpticalFlowConfig::default_config` mirrored with every field explicit
    /// so the authored map hashes identically to a service-authored one.
    public static var opticalFlowInterpolation: [String: Any] {
        ["mode": "optical_flow", "block_radius": 4, "search_radius": 8, "levels": 3,
         "confidence_floor": RationalTime(num: 1, den: 4).wire,
         "max_low_confidence": RationalTime(num: 1, den: 2).wire]
    }

    /// Authored intermediate-frame synthesis mode on the clip's piecewise map;
    /// nil keeps the legacy single-frame sample (or the map isn't piecewise).
    public func clipInterpolation(_ clip: EditClip) -> String? {
        clip.authored.object("time_map").object("interpolation")["mode"] as? String
    }

    /// TRACK-003's contract: forward-direction `SourceRef::Asset` clip on a
    /// video track. The service still validates; this only shapes the toggle.
    public func interpolationEligible(_ clip: EditClip) -> Bool {
        guard case .asset = clip.sourcePreview, clip.kind == .video, !clip.reversed else { return false }
        return clip.timeMapKind == "linear" || clip.timeMapKind == "piecewise_linear"
    }

    /// Toggle optical-flow synthesis on the clip's time map through the shared
    /// `clip_time_set` command. A linear map converts to its equivalent
    /// two-point piecewise map (local = offset + parent × speed over the clip
    /// range); disabling folds an affine two-point piecewise map back to
    /// linear so the speed field stays editable.
    public func setClipInterpolation(_ clip: EditClip, opticalFlow: Bool, base: String? = nil) {
        guard !trackLocked(clip.track), interpolationEligible(clip) else { return }
        var map = clip.authored.object("time_map")
        if opticalFlow {
            if let converted = piecewiseEquivalent(clip) { map = converted }
            guard map.string("kind") == "piecewise_linear" else { return }
            map["interpolation"] = Self.opticalFlowInterpolation
        } else {
            map.removeValue(forKey: "interpolation")
            if let linear = linearEquivalent(of: map) { map = linear }
        }
        submit([timelineCommand("clip_time_set", ["sequence": sequence.string("id"), "clip": clip.id,
            "source_in": clip.authored.object("source_in"), "time_map": map,
            "audio_retime": clip.authored["audio_retime"] ?? "reject",
            "reverse_sampling": clip.authored["reverse_sampling"] ?? NSNull()])], label: "フレーム補間", base: base)
    }

    /// Equivalent two-point piecewise map for a linear clip, or nil when the
    /// map is already piecewise / cannot be computed exactly.
    private func piecewiseEquivalent(_ clip: EditClip) -> [String: Any]? {
        guard clip.timeMapKind == "linear", let speed = clip.linearRate,
              let duration = try? clip.end.checkedSubtracting(clip.start),
              let endLocal = try? duration.checkedMultiplying(speed)
                .checkedAdding(.wire(clip.authored.object("time_map").object("offset"))) else { return nil }
        return ["kind": "piecewise_linear", "points": [
            ["parent": RationalTime(num: 0, den: 1).wire,
             "local": clip.authored.object("time_map").object("offset")],
            ["parent": duration.wire, "local": endLocal.wire]]]
    }

    /// A two-point affine piecewise map folds back to `linear` (offset +
    /// speed), preserving the speed field; richer ramps keep their points.
    private func linearEquivalent(of map: [String: Any]) -> [String: Any]? {
        guard map.string("kind") == "piecewise_linear", map["interpolation"] == nil,
              let points = map["points"] as? [[String: Any]], points.count == 2,
              let run = try? RationalTime.wire(points[1].object("parent"))
                .checkedSubtracting(.wire(points[0].object("parent"))),
              run.num != "0",
              let rise = try? RationalTime.wire(points[1].object("local"))
                .checkedSubtracting(.wire(points[0].object("local"))),
              let speed = try? rise.checkedDividing(run) else { return nil }
        return ["kind": "linear",
                "offset": points[0].object("local"), "speed": speed.wire]
    }

    // MARK: - AI-002 scene boundaries

    /// Validated `scene_boundary_assets` matching this clip's asset stream
    /// whose recorded `content_hash` still matches the live asset (stale
    /// results are hidden rather than applied, mirroring the model rule).
    public func sceneBoundaryAssets(for clip: EditClip) -> [[String: Any]] {
        guard case .asset(let asset, let stream) = clip.sourcePreview else { return [] }
        let live = document.objects("assets").first { $0.string("id") == asset }?.string("content_hash") ?? ""
        return document.objects("scene_boundary_assets").filter { boundary in
            let source = boundary.object("source")
            return source.string("asset") == asset
                && Int(source.string("stream_index")) == stream
                && source.string("content_hash") == live
        }
    }

    /// Whether `scene.detect` can run for this clip's source (asset video
    /// stream; multicam clips can't receive `scene.apply` mappings anyway).
    public func sceneDetectEligible(_ clip: EditClip) -> Bool {
        guard case .asset = clip.sourcePreview else { return false }
        return clip.kind == .video
    }

    /// Submit the shared `scene.detect` job for the clip's asset stream. The
    /// commit lands asynchronously through the job worker; the session
    /// subscription then reloads the document and `sceneBoundaryAssets`
    /// picks the result up. Job progress shows on the Export page.
    public func detectScenes(_ clip: EditClip) {
        guard sceneDetectEligible(clip), case .asset(let asset, let stream) = clip.sourcePreview else {
            mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "シーン検出は素材の映像クリップに適用します")); return
        }
        Task {
            do {
                _ = try await request("scene.detect", ["asset": asset, "stream_index": stream])
            } catch { mapFailure(error) }
        }
    }

    /// `scene.apply` rides the shared direct-operation path (`session_id` is
    /// part of its schema), so markers/splits are ordinary undoable edits.
    public func applySceneBoundaries(_ clip: EditClip, asset sceneAsset: String, split: Bool) {
        submitDirect("scene.apply", ["scene_asset": sceneAsset, "sequence": sequence.string("id"),
            "mode": split ? "split" : "markers", "clip": clip.id],
            label: split ? "シーン境界で分割" : "シーン境界をマーカーに追加")
    }
}
