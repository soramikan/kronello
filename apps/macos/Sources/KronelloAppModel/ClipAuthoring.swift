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
    public func addClipEffect(_ kind: String) {
        guard let clip = selectedClip, clip.kind != .audio else { return }
        var properties = clip.authored.objects("properties"), effects = clip.authored.objects("effects")
        func property(_ key: String, _ type: String, _ value: Any) -> String {
            let id = UUID().uuidString
            properties.append(["id": id, "descriptor": ["key": key, "version": 1], "source": ["kind": "constant", "value": ["kind": type, "value": value]], "modifiers": []]); return id
        }
        let parameters: [String: Any], id: String
        switch kind {
        case "blur":
            id = "kronello.gaussian_blur"; parameters = ["kind": "gaussian_blur", "sigma": property("kronello.effect.sigma", "scalar", 8.0)]
        case "shadow":
            id = "kronello.drop_shadow"; parameters = ["kind": "drop_shadow", "sigma": property("kronello.effect.sigma", "scalar", 8.0), "offset": property("kronello.effect.offset", "vec2", [8.0, 8.0]), "color": property("kronello.effect.color", "color", ["space": "srgb", "components": ["r": 0.0, "g": 0.0, "b": 0.0, "alpha": 1.0]]), "opacity": property("kronello.effect.opacity", "scalar", 0.5)]
        default: return
        }
        effects.append(["effect_id": id, "version": 2, "parameters": parameters])
        replaceClipEffects(clip, properties: properties, effects: effects, label: "クリップ効果の追加")
    }
    public func removeClipEffect(_ clip: EditClip, index: Int) {
        guard clip.kind != .audio else { return }
        var effects = clip.authored.objects("effects")
        guard effects.indices.contains(index) else { return }
        effects.remove(at: index)
        // Referenced properties stay authored, preserving possible animation and future reuse.
        replaceClipEffects(clip, properties: clip.authored.objects("properties"), effects: effects, label: "クリップ効果の削除")
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
