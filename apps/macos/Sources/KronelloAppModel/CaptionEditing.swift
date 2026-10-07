import Foundation

/// GUI-009 caption cue editing. All mutations are shared `caption_set`,
/// `track_append` and `clip_place` commands — Undo, conflict handling and the
/// preview reload flow come from the common edit pipeline (ADR-0107).
extension EditorModel {
    public func captionDocument(id: String) -> [String: Any]? {
        document.objects("captions").first { $0.string("id") == id }
    }
    public func captionDocument(for clip: EditClip) -> [String: Any]? {
        clip.caption.flatMap { captionDocument(id: $0) }
    }
    /// First authored caption track of the selected Sequence, if any.
    public var captionTrack: [String: Any]? {
        sequence.objects("tracks").first { $0.string("kind") == "caption" }
    }
    /// Canonical `\n` cue text; other separators are normalized at input.
    public static func normalizedCaptionText(_ text: String) -> String {
        text.replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
            .replacingOccurrences(of: "\u{2028}", with: "\n")
            .replacingOccurrences(of: "\u{2029}", with: "\n")
    }
    /// Spans that still cover the identical bytes at the same grapheme-aligned
    /// positions survive a text edit; every other span is dropped so the stored
    /// document stays valid instead of failing `caption_set` validation.
    public static func captionSpans(_ spans: [[String: Any]], preservedIn old: String, text new: String) -> [[String: Any]] {
        var boundaries = Set<Int>(), cursor = 0
        for character in new { boundaries.insert(cursor); cursor += String(character).utf8.count }
        boundaries.insert(cursor)
        let oldBytes = Array(old.utf8), newBytes = Array(new.utf8)
        return spans.filter { span in
            let range = span.object("range")
            let start = Int(range.number("start")), end = Int(range.number("end"))
            guard start >= 0, end > start, end <= newBytes.count, end <= oldBytes.count,
                  boundaries.contains(start), boundaries.contains(end),
                  Array(newBytes[start..<end]) == Array(oldBytes[start..<end]) else { return false }
            return true
        }
    }
    private func updateCaption(_ clip: EditClip, label: String, base: String?, mutate: (inout [String: Any]) throws -> Void) {
        guard !trackLocked(clip.track), var caption = captionDocument(for: clip) else { return }
        do {
            try mutate(&caption)
            submit([["caption_set": ["caption": caption]]], label: label, base: base)
        } catch { mapFailure(error) }
    }
    public func setCaptionText(_ clip: EditClip, to text: String, base: String? = nil) {
        let normalized = Self.normalizedCaptionText(text)
        updateCaption(clip, label: "字幕テキストの変更", base: base) { caption in
            caption["spans"] = Self.captionSpans(caption.objects("spans"), preservedIn: caption.string("text"), text: normalized)
            caption["text"] = normalized
        }
    }
    public func setCaptionFont(_ clip: EditClip, font: [String: Any], base: String? = nil) {
        guard lockedFonts.contains(where: { NSDictionary(dictionary: $0) == NSDictionary(dictionary: font) }) else { return }
        updateCaption(clip, label: "字幕書体の変更", base: base) { caption in
            var style = caption.object("style"); style["font"] = font; caption["style"] = style
        }
    }
    public func setCaptionSize(_ clip: EditClip, size: Double, base: String? = nil) {
        guard size.isFinite, size > 0 else { return }
        updateCaption(clip, label: "字幕サイズの変更", base: base) { caption in
            var style = caption.object("style"); style["size"] = size; caption["style"] = style
        }
    }
    public func setCaptionFill(_ clip: EditClip, hex: String, base: String? = nil) {
        updateCaption(clip, label: "字幕の色の変更", base: base) { caption in
            var style = caption.object("style")
            style["fill"] = try Self.colorValue(hex: hex).object("value")
            caption["style"] = style
        }
    }
    /// Width below zero disables the outline; enabling supplies a black ring.
    public func setCaptionOutline(_ clip: EditClip, width: Double, hex: String? = nil, base: String? = nil) {
        guard width.isFinite, width >= 0 else { return }
        updateCaption(clip, label: "字幕の縁取りの変更", base: base) { caption in
            var style = caption.object("style")
            var outline = style.object("outline")
            outline["color"] = try hex.map { try Self.colorValue(hex: $0).object("value") }
                ?? (outline["color"] as? [String: Any])
                ?? ["space": "srgb", "components": ["r": 0.0, "g": 0.0, "b": 0.0, "alpha": 1.0]]
            outline["width"] = width
            style["outline"] = outline
            caption["style"] = style
        }
    }
    public func removeCaptionOutline(_ clip: EditClip, base: String? = nil) {
        updateCaption(clip, label: "字幕の縁取りの削除", base: base) { caption in
            var style = caption.object("style"); style.removeValue(forKey: "outline"); caption["style"] = style
        }
    }
    /// `hex == nil` removes the cue background rectangle entirely.
    public func setCaptionBackground(_ clip: EditClip, hex: String?, base: String? = nil) {
        updateCaption(clip, label: "字幕の背景の変更", base: base) { caption in
            var style = caption.object("style")
            if let hex { style["background"] = try Self.colorValue(hex: hex).object("value") }
            else { style.removeValue(forKey: "background") }
            caption["style"] = style
        }
    }
    public static let captionAnchors = [
        "top_left", "top_center", "top_right",
        "center_left", "center", "center_right",
        "bottom_left", "bottom_center", "bottom_right",
    ]
    public func setCaptionAnchor(_ clip: EditClip, anchor: String, base: String? = nil) {
        guard Self.captionAnchors.contains(anchor) else { return }
        updateCaption(clip, label: "字幕位置の変更", base: base) { caption in
            var placement = caption.object("placement"); placement["anchor"] = anchor; caption["placement"] = placement
        }
    }
    /// Placement ratios are stored exactly as rationals; percent input converts
    /// to a bounded exact fraction.
    public static func captionRatio(percent: Double) -> RationalTime? {
        guard percent.isFinite, abs(percent) <= 1_000_000 else { return nil }
        return RationalTime(num: Int64((percent * 100).rounded()), den: 10000)
    }
    public static func captionPercent(_ rational: [String: Any]) -> Double {
        let num = Double(rational.string("num")) ?? 0, den = Double(rational.string("den")) ?? 1
        return den == 0 ? 0 : num / den * 100
    }
    public func setCaptionOffset(_ clip: EditClip, axis: Int, percent: Double, base: String? = nil) {
        guard axis == 0 || axis == 1, let ratio = Self.captionRatio(percent: percent) else { return }
        updateCaption(clip, label: "字幕位置の変更", base: base) { caption in
            var placement = caption.object("placement")
            var offset = placement["offset"] as? [[String: Any]] ?? [["num": "0", "den": "1"], ["num": "0", "den": "1"]]
            while offset.count < 2 { offset.append(["num": "0", "den": "1"]) }
            offset[axis] = ratio.wire
            placement["offset"] = offset; caption["placement"] = placement
        }
    }
    /// Insets are bounded to 0..=50% per axis so the safe area never inverts.
    public func setCaptionInset(_ clip: EditClip, axis: Int, percent: Double, base: String? = nil) {
        guard axis == 0 || axis == 1, percent >= 0, percent <= 50, let ratio = Self.captionRatio(percent: percent) else { return }
        updateCaption(clip, label: "字幕セーフエリアの変更", base: base) { caption in
            var placement = caption.object("placement")
            var inset = placement["safe_area_inset"] as? [[String: Any]] ?? [["num": "1", "den": "20"], ["num": "1", "den": "20"]]
            while inset.count < 2 { inset.append(["num": "1", "den": "20"]) }
            inset[axis] = ratio.wire
            placement["safe_area_inset"] = inset; caption["placement"] = placement
        }
    }
    /// One cue at the playhead. Builds the complete command list so tests can
    /// inspect it without a transport: a caption track is appended when the
    /// Sequence lacks one, then `caption_set` + `clip_place` land in one Event.
    public func captionAddCommands(at frame: Int64, durationFrames: Int64, font: [String: Any]) -> (track: String, clip: String, commands: [[String: Any]]) {
        let track = captionTrack?.string("id") ?? UUID().uuidString.lowercased()
        let caption = UUID().uuidString.lowercased(), clip = UUID().uuidString.lowercased()
        var commands: [[String: Any]] = []
        if captionTrack == nil {
            commands.append(timelineCommand("track_append", ["sequence": sequence.string("id"),
                "track": ["id": track, "kind": "caption", "clips": [[String: Any]](),
                          "state": ["visible": true, "muted": false]]]))
        }
        let document: [String: Any] = [
            "id": caption, "version": 1, "text": "字幕",
            "style": ["font": font, "size": 48.0,
                      "fill": ["space": "srgb", "components": ["r": 1.0, "g": 1.0, "b": 1.0, "alpha": 1.0]]],
            "placement": ["anchor": "bottom_center",
                          "safe_area_inset": [["num": "1", "den": "20"], ["num": "1", "den": "20"]],
                          "offset": [["num": "0", "den": "1"], ["num": "0", "den": "1"]]]]
        commands.append(["caption_set": ["caption": document]])
        commands.append(timelineCommand("clip_place", ["sequence": sequence.string("id"), "track": track,
            "clip": ["id": clip, "source_ref": ["kind": "caption", "caption": caption],
                     "timeline_range": ["start": frameTime(max(0, frame)).wire, "end": frameTime(max(0, frame) + durationFrames).wire],
                     "source_in": ["num": "0", "den": "1"], "audio_retime": "reject",
                     "time_map": ["kind": "linear", "offset": ["num": "0", "den": "1"], "speed": ["num": "1", "den": "1"]],
                     "links": [[String: Any]](), "effects": [[String: Any]](), "properties": [[String: Any]]()]]))
        return (track, clip, commands)
    }
    /// Adds a two-second cue at the playhead and selects the new clip.
    public func addCaption() {
        guard !busy, pendingCandidate == nil, !sequence.isEmpty else { return }
        guard let font = lockedFonts.first else {
            mapFailure(ServiceFailure(code: "FONT_MISSING", message: "字幕に使用できる登録フォントがありません。プロジェクトにフォントを登録してください。"))
            return
        }
        let result = captionAddCommands(at: frame, durationFrames: Int64(nominalFPS) * 2, font: font)
        // Select only after the Event lands so a rejected placement does not
        // leave a dangling selection on a clip that never materialized.
        Task {
            guard await apply(.init(base: revision, commands: result.commands, label: "字幕の追加")) != nil else { return }
            selectClip(result.clip)
        }
    }
}
