import Foundation

extension EditorModel {
    public func setTrackOutput(_ track: [String: Any]) {
        guard !ui.locked.contains(track.string("id")) else { return }
        var state = track.object("state")
        let visible = state["visible"] as? Bool ?? true
        let muted = state["muted"] as? Bool ?? false
        state = ["visible": track.string("kind") == "audio" ? visible : !visible,
                 "muted": track.string("kind") == "audio" ? !muted : muted]
        submit([timelineCommand("track_state_set", ["sequence": sequence.string("id"), "track": track.string("id"), "state": state])], label: "トラックの出力変更")
    }
    public func setClipTime(_ clip: EditClip, sourceIn: RationalTime? = nil, speedPercent: Double? = nil, base: String? = nil) {
        guard !ui.locked.contains(clip.track) else { return }
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
        guard !ui.locked.contains(clip.track) else { return }
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
        // COLOR-003: listed so existing LUT effects enumerate uniformly, but
        // `addClipEffect` cannot create one — the `lut` parameter must point at
        // a real Data asset, so `addClipLutEffect` requires the asset id.
        .init(kind: "color_lut", effectID: EditorModel.lutEffectID, parameters: []),
    ]
    public static let curveTableDefault: [String: Any] = ["columns": ["x": "scalar", "y": "scalar"],
        "rows": [["x": ["kind": "scalar", "value": 0.0], "y": ["kind": "scalar", "value": 0.0]],
                 ["x": ["kind": "scalar", "value": 1.0], "y": ["kind": "scalar", "value": 1.0]]]]
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
        case "color_lut", Self.lutEffectID:
            // A LUT effect without a bound Data asset cannot render; creation
            // goes through `addClipLutEffect` which takes the asset id.
            return
        case let name where Self.colorEffects.contains(where: { $0.kind == name || $0.effectID == name }):
            guard let spec = Self.colorEffects.first(where: { $0.kind == name || $0.effectID == name }) else { return }
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
        guard !ui.locked.contains(clip.track), clip.kind != .audio,
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
    /// COLOR-003 (ADR-0113): `kronello.color.lut` binds a `.cube` Data asset.
    public static let lutEffectID = "kronello.color.lut"
    /// Imported `.cube` assets (kind `data`); the LUT picker lists these.
    public var lutAssets: [(id: String, name: String)] {
        document.objects("assets").compactMap { asset in
            guard asset.string("kind") == "data" else { return nil }
            let locator = asset.object("locator")
            let name = URL(fileURLWithPath: locator.string("relative").isEmpty
                ? locator.string("absolute") : locator.string("relative")).lastPathComponent
            return (asset.string("id"), name)
        }
    }
    /// Hash → file inputs for every imported `.cube` Data asset, resolved
    /// against the project directory (the RenderInput `luts` convention).
    public var lutInputs: [[String: Any]] {
        let base = URL(fileURLWithPath: path).deletingLastPathComponent()
        return document.objects("assets").compactMap { asset in
            guard asset.string("kind") == "data" else { return nil }
            let locator = asset.object("locator")
            let relative = locator.string("relative")
            let file = relative.isEmpty ? locator.string("absolute")
                : base.appendingPathComponent(relative).path
            guard !file.isEmpty else { return nil }
            return ["hash": asset.string("content_hash"), "path": file]
        }
    }
    /// Asset id bound to the `lut` parameter through a constant asset_ref.
    public static func lutParameterAsset(_ clip: EditClip, effect: [String: Any]) -> String? {
        guard let id = effect.object("parameters")["lut"] as? String,
              let property = clip.authored.objects("properties").first(where: { $0.string("id") == id }),
              property.object("source").string("kind") == "constant" else { return nil }
        return property.object("source").object("value")["value"] as? String
    }
    /// Add the versioned LUT effect bound to an imported Data asset at full
    /// intensity; plain `addClipEffect` rejects `color_lut` on purpose.
    public func addClipLutEffect(_ clip: EditClip, asset: String) {
        guard !ui.locked.contains(clip.track), clip.kind != .audio, !asset.isEmpty else { return }
        var properties = clip.authored.objects("properties"), effects = clip.authored.objects("effects")
        func property(_ key: String, _ type: String, _ value: Any) -> String {
            let id = UUID().uuidString
            properties.append(["id": id, "descriptor": ["key": key, "version": 1], "source": ["kind": "constant", "value": ["kind": type, "value": value]], "modifiers": []]); return id
        }
        effects.append(["effect_id": Self.lutEffectID, "version": 1,
            "parameters": ["kind": "color_lut",
                "lut": property("kronello.effect.lut", "asset_ref", asset),
                "intensity": property("kronello.effect.intensity", "scalar", 1.0)]])
        replaceClipEffects(clip, properties: properties, effects: effects, label: "LUT の追加")
    }
    /// Import a `.cube` file as an external `data` asset pinned by content
    /// hash; the service validates the lattice before the edit commits.
    public func importLut(path: String) async {
        guard !busy else { return }
        do {
            _ = try await request("lut.import", ["base_revision": revision, "path": path,
                "asset": UUID().uuidString.lowercased(), "session_id": sessionID,
                "idempotency_key": UUID().uuidString])
            try await reload()
        } catch { mapFailure(error) }
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
        guard !ui.locked.contains(clip.track), clip.kind == .audio else { return }
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
        guard !ui.locked.contains(clip.track), let speed = clip.linearRate else { return }
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
