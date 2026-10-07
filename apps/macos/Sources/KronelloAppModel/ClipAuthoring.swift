import Foundation

extension EditorModel {
    public func setTrackOutput(_ track: [String: Any]) {
        guard !trackLocked(track.string("id")) else { return }
        let state = track.object("state")
        let visible = state["visible"] as? Bool ?? true
        let muted = state["muted"] as? Bool ?? false
        // TrackStateSet replaces the whole state, so the persisted lock flag
        // must be re-emitted or toggling visibility would silently unlock.
        submit([timelineCommand("track_state_set", ["sequence": sequence.string("id"), "track": track.string("id"), "state": [
            "visible": track.string("kind") == "audio" ? visible : !visible,
            "muted": track.string("kind") == "audio" ? !muted : muted,
            "locked": state["locked"] as? Bool ?? false]])], label: "トラックの出力変更")
    }
    public func setClipTime(_ clip: EditClip, sourceIn: RationalTime? = nil, speedPercent: Double? = nil, base: String? = nil) {
        guard !trackLocked(clip.track) else { return }
        var map = clip.authored.object("time_map")
        if let percent = speedPercent {
            guard percent.isFinite, percent > 0, percent <= 10000, map.string("kind") == "linear" else {
                mapFailure(ServiceFailure(code: "INVALID_RETIME", message: "正の Linear 速度を指定してください")); return
            }
            // The control exposes one decimal percent: exact authored speed is n/1000.
            let speed = RationalTime(num: Int64((percent * 10).rounded()), den: 1000)
            map["speed"] = speed.wire
        }
        submit([timelineCommand("clip_time_set", ["sequence": sequence.string("id"), "clip": clip.id,
            "source_in": sourceIn?.wire ?? clip.authored.object("source_in"), "time_map": map,
            "audio_retime": clip.reversed ? "reverse_resample_v1" : "resample_v1", "reverse_sampling": clip.authored["reverse_sampling"] ?? NSNull()])], label: "クリップの時間設定", base: base)
    }
    public func replaceClipEffects(_ clip: EditClip, properties: [[String: Any]], effects: [[String: Any]], label: String, base: String? = nil) {
        guard !trackLocked(clip.track) else { return }
        submit([timelineCommand("clip_set_effects", ["sequence": sequence.string("id"), "clip": clip.id, "properties": properties, "effects": effects])], label: label, base: base)
    }
    public func setClipProperty(_ clip: EditClip, key: String, kind: String, value: Any, base: String? = nil) {
        guard clip.kind != .audio else { mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "合成設定は映像クリップに適用します")); return }
        var properties = clip.authored.objects("properties")
        if let index = properties.firstIndex(where: { $0.object("descriptor").string("key") == key }) {
            guard properties[index].object("source").string("kind") == "constant" else {
                mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "アニメーション付き clip Property は値を固定せず保持します")); return
            }
            properties[index]["source"] = ["kind": "constant", "value": ["kind": kind, "value": value]]
        } else {
            properties.append(["id": UUID().uuidString, "descriptor": ["key": key, "version": 1], "source": ["kind": "constant", "value": ["kind": kind, "value": value]], "modifiers": []])
        }
        replaceClipEffects(clip, properties: properties, effects: clip.authored.objects("effects"), label: "クリップの合成設定", base: base)
    }
    /// GUI-010 color effect authoring spec: kind tag, effect id and the
    /// constant defaults that keep the clip visually unchanged at insertion.
    public struct ColorEffectSpec {
        public let kind: String
        public let effectID: String
        /// (parameters field, descriptor key suffix, Value kind, default)
        public let parameters: [(String, String, String, Any)]
    }
    public static let colorEffects: [ColorEffectSpec] = [
        .init(kind: "color_exposure", effectID: "kronello.color.exposure", parameters: [
            ("exposure", "exposure", "scalar", 0.0),
            ("offset", "exposure_offset", "scalar", 0.0)]),
        .init(kind: "color_levels", effectID: "kronello.color.levels", parameters: [
            ("in_black", "in_black", "scalar", 0.0),
            ("in_white", "in_white", "scalar", 1.0),
            ("gamma", "gamma", "scalar", 1.0),
            ("out_black", "out_black", "scalar", 0.0),
            ("out_white", "out_white", "scalar", 1.0)]),
        .init(kind: "color_curves", effectID: "kronello.color.curves", parameters: [
            ("curve", "curve", "data_table", curveTableDefault)]),
        .init(kind: "color_hsl", effectID: "kronello.color.hsl", parameters: [
            ("hue_shift", "hue_shift", "angle", 0.0),
            ("saturation", "saturation", "scalar", 1.0),
            ("lightness", "lightness", "scalar", 0.0)]),
    ]
    public static let curveTableDefault: [String: Any] = ["columns": ["x": "scalar", "y": "scalar"],
        "rows": [["x": ["kind": "scalar", "value": 0.0], "y": ["kind": "scalar", "value": 0.0]],
                 ["x": ["kind": "scalar", "value": 1.0], "y": ["kind": "scalar", "value": 1.0]]]]
    /// FX-005/FX-006 clip effect specs (ADR-0115). These reuse the generic
    /// (kind, effect id, parameter defaults) shape; `corner_pin` is excluded
    /// because its identity quad depends on the sequence extent and is built
    /// in `addClipEffect`.
    public static let standardEffects: [ColorEffectSpec] = [
        .init(kind: "chroma_key", effectID: "kronello.keying.chroma", parameters: [
            ("key_color", "key_color", "color", ["space": "srgb", "components": ["r": 0.0, "g": 177.0 / 255.0, "b": 64.0 / 255.0, "alpha": 1.0]]),
            ("similarity", "similarity", "scalar", 0.4),
            ("edge_shrink", "edge_shrink", "scalar", 0.0),
            ("edge_feather", "edge_feather", "scalar", 0.0),
            ("spill", "spill", "scalar", 0.5)]),
        .init(kind: "luma_key", effectID: "kronello.keying.luma", parameters: [
            ("key_luma", "key_luma", "scalar", 0.0),
            ("tolerance", "tolerance", "scalar", 0.1),
            ("edge_shrink", "edge_shrink", "scalar", 0.0),
            ("edge_feather", "edge_feather", "scalar", 0.0)]),
        .init(kind: "glow", effectID: "kronello.glow", parameters: [
            ("threshold", "threshold", "scalar", 0.8),
            ("radius", "radius", "scalar", 8.0),
            ("intensity", "intensity", "scalar", 1.0)]),
        .init(kind: "sharpen", effectID: "kronello.sharpen", parameters: [
            ("amount", "amount", "scalar", 0.5),
            ("radius", "radius", "scalar", 4.0)]),
        .init(kind: "vignette", effectID: "kronello.vignette", parameters: [
            ("amount", "amount", "scalar", 0.5),
            ("midpoint", "midpoint", "scalar", 0.5),
            ("feather", "feather", "scalar", 0.5),
            ("roundness", "roundness", "scalar", 0.5)]),
    ]
    /// Effect definitions on a clip that belong to the FX-005/FX-006 set.
    public static func standardEffectSpecs(on clip: EditClip) -> [(index: Int, spec: ColorEffectSpec, effect: [String: Any])] {
        clip.authored.objects("effects").enumerated().compactMap { index, effect in
            guard let spec = standardEffects.first(where: { $0.effectID == effect.string("effect_id") }) else { return nil }
            return (index, spec, effect)
        }
    }
    /// sRGB display components [r, g, b, a] behind a color effect parameter.
    public static func colorParameterColor(_ clip: EditClip, effect: [String: Any], parameter: String) -> [Double]? {
        guard let id = effect.object("parameters")[parameter] as? String,
              let property = clip.authored.objects("properties").first(where: { $0.string("id") == id }),
              property.object("source").string("kind") == "constant" else { return nil }
        return srgbComponents(property.object("source").object("value"))
    }
    /// Effect definitions on a clip that belong to the COLOR-002 set.
    public static func colorEffectSpecs(on clip: EditClip) -> [(index: Int, spec: ColorEffectSpec, effect: [String: Any])] {
        clip.authored.objects("effects").enumerated().compactMap { index, effect in
            guard let spec = colorEffects.first(where: { $0.effectID == effect.string("effect_id") }) else { return nil }
            return (index, spec, effect)
        }
    }
    /// Constant scalar/angle value behind an effect parameter reference.
    public static func colorParameterValue(_ clip: EditClip, effect: [String: Any], parameter: String) -> Double? {
        guard let id = effect.object("parameters")[parameter] as? String,
              let property = clip.authored.objects("properties").first(where: { $0.string("id") == id }),
              property.object("source").string("kind") == "constant" else { return nil }
        return property.object("source").object("value")["value"] as? Double
    }
    /// Whether the parameter property is still a plain constant. Non-constant
    /// sources (expression/keyframe) are shown read-only instead of replaced.
    public static func colorParameterIsConstant(_ clip: EditClip, effect: [String: Any], parameter: String) -> Bool {
        guard let id = effect.object("parameters")[parameter] as? String,
              let property = clip.authored.objects("properties").first(where: { $0.string("id") == id }) else { return false }
        return property.object("source").string("kind") == "constant"
    }
    public func addClipEffect(_ kind: String) {
        guard let clip = selectedClip, clip.kind != .audio else { return }
        var properties = clip.authored.objects("properties"), effects = clip.authored.objects("effects")
        func property(_ key: String, _ type: String, _ value: Any) -> String {
            let id = UUID().uuidString
            properties.append(["id": id, "descriptor": ["key": key, "version": 1], "source": ["kind": "constant", "value": ["kind": type, "value": value]], "modifiers": []]); return id
        }
        let parameters: [String: Any], id: String, version: Int
        switch kind {
        case "blur":
            id = "kronello.gaussian_blur"; version = 2
            parameters = ["kind": "gaussian_blur", "sigma": property("kronello.effect.sigma", "scalar", 8.0)]
        case "shadow":
            id = "kronello.drop_shadow"; version = 2
            parameters = ["kind": "drop_shadow", "sigma": property("kronello.effect.sigma", "scalar", 8.0), "offset": property("kronello.effect.offset", "vec2", [8.0, 8.0]), "color": property("kronello.effect.color", "color", ["space": "srgb", "components": ["r": 0.0, "g": 0.0, "b": 0.0, "alpha": 1.0]]), "opacity": property("kronello.effect.opacity", "scalar", 0.5)]
        case "corner_pin":
            id = "kronello.corner_pin"; version = 1
            // Identity quad over the sequence extent keeps the clip visually
            // unchanged at insertion (ADR-0115).
            let w = extent.width, h = extent.height
            parameters = ["kind": "corner_pin",
                "top_left": property("kronello.effect.top_left", "vec2", [0.0, 0.0]),
                "top_right": property("kronello.effect.top_right", "vec2", [w, 0.0]),
                "bottom_right": property("kronello.effect.bottom_right", "vec2", [w, h]),
                "bottom_left": property("kronello.effect.bottom_left", "vec2", [0.0, h])]
        case let name where (Self.colorEffects + Self.standardEffects).contains(where: { $0.kind == name || $0.effectID == name }):
            guard let spec = (Self.colorEffects + Self.standardEffects).first(where: { $0.kind == name || $0.effectID == name }) else { return }
            var parameterIDs: [String: Any] = ["kind": spec.kind]
            for (field, suffix, type, value) in spec.parameters {
                parameterIDs[field] = property("kronello.effect." + suffix, type, value)
            }
            id = spec.effectID; version = 1
            parameters = parameterIDs
        default: return
        }
        effects.append(["effect_id": id, "version": version, "parameters": parameters])
        replaceClipEffects(clip, properties: properties, effects: effects, label: "クリップ効果の追加")
    }
    /// GUI-010: writes a new constant source into the Property referenced by an
    /// effect parameter. Animated or expression-backed parameters keep their
    /// authored source and reject the edit with a typed error.
    public func setClipEffectParameter(_ clip: EditClip, effect: [String: Any], parameter: String, kind: String, value: Any, base: String? = nil) {
        guard !trackLocked(clip.track), clip.kind != .audio,
              let propertyID = effect.object("parameters")[parameter] as? String else { return }
        var properties = clip.authored.objects("properties")
        guard let index = properties.firstIndex(where: { $0.string("id") == propertyID }) else { return }
        guard properties[index].object("source").string("kind") == "constant" else {
            mapFailure(ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "アニメーション付きパラメータは値を固定せず保持します")); return
        }
        properties[index]["source"] = ["kind": "constant", "value": ["kind": kind, "value": value]]
        replaceClipEffects(clip, properties: properties, effects: clip.authored.objects("effects"), label: "カラー補正の変更", base: base)
    }
    /// DataTable rows for the curve editor; returns nil unless the referenced
    /// property is a constant x/y scalar table.
    public static func curveRows(_ clip: EditClip, effect: [String: Any]) -> [(x: Double, y: Double)]? {
        guard let id = effect.object("parameters")["curve"] as? String,
              let property = clip.authored.objects("properties").first(where: { $0.string("id") == id }),
              property.object("source").string("kind") == "constant",
              let table = property.object("source").object("value")["value"] as? [String: Any],
              let rows = table["rows"] as? [[String: Any]] else { return nil }
        return rows.compactMap { row in
            guard let x = row.object("x")["value"] as? Double, let y = row.object("y")["value"] as? Double else { return nil }
            return (x, y)
        }
    }
    /// Builds the constant table value from control points, keeping x in
    /// strictly increasing order inside [0,1] as the service requires.
    public static func curveTableValue(points: [(x: Double, y: Double)]) -> [String: Any]? {
        let sorted = points.sorted { $0.x < $1.x }
        // The shared model requires 2...CURVES_MAX_POINTS strictly increasing rows.
        guard (2...64).contains(sorted.count), sorted.allSatisfy({ $0.x.isFinite && $0.y.isFinite && (0...1).contains($0.x) && (0...1).contains($0.y) }) else { return nil }
        for index in 1..<sorted.count where sorted[index].x <= sorted[index - 1].x { return nil }
        return ["columns": ["x": "scalar", "y": "scalar"],
                "rows": sorted.map { ["x": ["kind": "scalar", "value": $0.x], "y": ["kind": "scalar", "value": $0.y]] as [String: Any] }]
    }
    public func removeClipEffect(_ clip: EditClip, index: Int) {
        guard clip.kind != .audio else { return }
        var effects = clip.authored.objects("effects")
        guard effects.indices.contains(index) else { return }
        effects.remove(at: index)
        // Referenced properties stay authored, preserving possible animation and future reuse.
        replaceClipEffects(clip, properties: clip.authored.objects("properties"), effects: effects, label: "クリップ効果の削除")
    }
    /// Constant [x, y] behind a vec2 effect parameter reference.
    public static func vec2ParameterValue(_ clip: EditClip, effect: [String: Any], parameter: String) -> [Double]? {
        guard let id = effect.object("parameters")[parameter] as? String,
              let property = clip.authored.objects("properties").first(where: { $0.string("id") == id }),
              property.object("source").string("kind") == "constant",
              let pair = property.object("source").object("value")["value"] as? [Double],
              pair.count == 2 else { return nil }
        return pair
    }
    /// Clip gain Property (`kronello.audio.volume`, nonnegative linear
    /// scalar). Passing nil clears the authored volume back to unity.
    public func setClipVolume(_ clip: EditClip, value: Double?, base: String? = nil) {
        guard !trackLocked(clip.track), clip.kind == .audio else { return }
        let volume: Any = value.map { v -> [String: Any] in
            ["id": UUID().uuidString, "descriptor": ["key": "kronello.audio.volume", "version": 1],
             "source": ["kind": "constant", "value": ["kind": "scalar", "value": v]], "modifiers": []]
        } ?? NSNull()
        submit([timelineCommand("clip_set_volume", ["sequence": sequence.string("id"), "clip": clip.id, "volume": volume])], label: "クリップの音量", base: base)
    }
    /// Authored constant clip gain, or unity (1.0) when no volume Property is set.
    public static func clipVolume(_ clip: EditClip) -> Double {
        guard let property = clip.authored["volume"] as? [String: Any],
              property.object("source").string("kind") == "constant" else { return 1.0 }
        return property.object("source").object("value").number("value")
    }
    /// AUDIO-009 mixer fader: one undoable edit writes the same linear gain
    /// into every clip's `kronello.audio.volume` on the track. The authored
    /// model has no track-level gain field, so the shared clip Property is
    /// the gain the evaluator and export path already understand.
    public func setTrackVolume(_ track: [String: Any], gain: Double, base: String? = nil) {
        guard !ui.locked.contains(track.string("id")), track.string("kind") == "audio",
              gain.isFinite, gain >= 0 else { return }
        var commands: [[String: Any]] = []
        for clip in track.objects("clips") {
            let volume: [String: Any] = ["id": UUID().uuidString, "descriptor": ["key": "kronello.audio.volume", "version": 1],
                "source": ["kind": "constant", "value": ["kind": "scalar", "value": gain]], "modifiers": []]
            commands.append(timelineCommand("clip_set_volume", ["sequence": sequence.string("id"), "clip": clip.string("id"), "volume": volume]))
        }
        guard !commands.isEmpty else { return }
        submit(commands, label: "トラックの音量", base: base)
    }
    /// Fader display value: the shared clip gain when every clip on the track
    /// agrees, else nil for a mixed state.
    public func trackVolume(_ track: [String: Any]) -> Double? {
        let gains = track.objects("clips").map { clip -> Double in
            guard let property = clip["volume"] as? [String: Any],
                  property.object("source").string("kind") == "constant" else { return 1.0 }
            return property.object("source").object("value").number("value")
        }
        guard let first = gains.first else { return nil }
        return gains.allSatisfy { abs($0 - first) < 0.0001 } ? first : nil
    }
}

extension RationalTime {
    public func checkedAdding(_ other: Self) throws -> Self { try combined(other, subtract: false) }
    public func checkedSubtracting(_ other: Self) throws -> Self { try combined(other, subtract: true) }
    private func combined(_ other: Self, subtract: Bool) throws -> Self {
        guard let a = Int64(num), let b = Int64(den), let c = Int64(other.num), let d = Int64(other.den) else { throw Self.overflow }
        let left = a.multipliedReportingOverflow(by: d), right = c.multipliedReportingOverflow(by: b), denominator = b.multipliedReportingOverflow(by: d)
        let numerator = subtract ? left.partialValue.subtractingReportingOverflow(right.partialValue) : left.partialValue.addingReportingOverflow(right.partialValue)
        guard !left.overflow, !right.overflow, !denominator.overflow, !numerator.overflow else { throw Self.overflow }
        return .init(num: numerator.partialValue, den: denominator.partialValue)
    }
    public func checkedMultiplying(_ other: Self) throws -> Self {
        guard let a = Int64(num), let b = Int64(den), let c = Int64(other.num), let d = Int64(other.den) else { throw Self.overflow }
        let numerator = a.multipliedReportingOverflow(by: c), denominator = b.multipliedReportingOverflow(by: d)
        guard !numerator.overflow, !denominator.overflow else { throw Self.overflow }
        return .init(num: numerator.partialValue, den: denominator.partialValue)
    }
    private static var overflow: ServiceFailure { .init(code: "TIME_OVERFLOW", message: "時間の計算範囲を超えました") }
}

extension EditorModel {
    public func setClipReverse(_ clip: EditClip, enabled: Bool) {
        guard !trackLocked(clip.track), let speed = clip.linearRate else { return }
        do {
            let duration = try clip.end.checkedSubtracting(clip.start)
            let travel = try duration.checkedMultiplying(speed).checkedAdding(.wire(clip.authored.object("time_map").object("offset")))
            let source = RationalTime.wire(clip.authored.object("source_in"))
            let sourceIn = try clip.reversed ? source.checkedSubtracting(travel) : source.checkedAdding(travel)
            submit([timelineCommand("clip_time_set", ["sequence": sequence.string("id"), "clip": clip.id, "source_in": sourceIn.wire,
                "time_map": ["kind": "linear", "offset": RationalTime(num: 0, den: 1).wire, "speed": speed.wire],
                "audio_retime": enabled ? "reverse_resample_v1" : "resample_v1",
                "reverse_sampling": enabled ? "reverse_grid_v1" : NSNull()])], label: "クリップの逆再生")
        } catch { mapFailure(error) }
    }
}
