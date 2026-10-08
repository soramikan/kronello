import Foundation

/// GUI-012 sidecar caption import. The shared `captions.import_plan` query
/// validates the typed plan (cue count, track kind, style); `captions.import`
/// then applies the same plan as one undoable Event. `cue_ids` are allocated
/// here after `import_plan` reports the exact parsed cue count in its typed
/// error — identities never come from display order guesses.
extension EditorModel {
    /// Sidecar formats accepted by the picker, keyed by `CaptionFormat` wire name.
    public static let captionFormats: [(format: String, name: String, extensions: [String])] = [
        ("srt", "SubRip", ["srt"]),
        ("vtt", "WebVTT", ["vtt"]),
        ("itt", "iTunes Timed Text", ["itt", "dfxp", "xml"]),
    ]
    /// Number `import_plan` requires when `cue_ids` is sent empty.
    static let captionCueCountPattern = try! NSRegularExpression(pattern: #"exactly (\d+) entries"#)

    /// Default base style for imported cues; same shape `addCaption` uses so
    /// mixed authored/imported cues render identically.
    public func captionImportStyle(font: [String: Any]) -> [String: Any] {
        ["font": font, "size": 48.0,
         "fill": ["space": "srgb", "components": ["r": 1.0, "g": 1.0, "b": 1.0, "alpha": 1.0]]]
    }
    /// Extract the required cue count from the typed `import_plan` error.
    public static func captionCueCount(_ failure: ServiceFailure) -> Int? {
        let range = NSRange(failure.message.startIndex..., in: failure.message)
        guard let match = captionCueCountPattern.firstMatch(in: failure.message, range: range),
              let capture = Range(match.range(at: 1), in: failure.message) else { return nil }
        return Int(failure.message[capture])
    }

    /// Build the shared `CaptionsImportPlanRequest` payload. `cue_ids` starts
    /// empty; callers retry once `import_plan` reports the parsed cue count.
    public func captionImportPlan(content: String, format: String, cueIDs: [[String: Any]] = []) throws -> [String: Any] {
        guard !sequence.isEmpty else { throw ServiceFailure(code: "SOURCE_MISSING", message: "シーケンスが選択されていません") }
        guard let font = lockedFonts.first else {
            throw ServiceFailure(code: "FONT_MISSING", message: "字幕に使用できる登録フォントがありません。プロジェクトにフォントを登録してください。")
        }
        let track = captionTrack?.string("id") ?? UUID().uuidString.lowercased()
        return [
            "project": path,
            "base_revision": revision,
            "sequence": sequence.string("id"),
            "track": track,
            "format": format,
            "content": content,
            "style": captionImportStyle(font: font),
            "cue_ids": cueIDs,
        ]
    }

    /// One shot `import_plan` → allocate cue ids → `import_plan` → `captions.import`.
    /// The mutation envelope differs from `edit.apply` (session/idempotency are
    /// top-level while `project`/`base_revision` stay inside `plan`), so the
    /// request is assembled here and the returned Event feeds undo directly.
    public func importCaptions(sidecarPath: String, format: String? = nil) {
        guard !busy, pendingCandidate == nil else { return }
        let resolved = format ?? Self.captionFormats.first {
            $0.extensions.contains(URL(fileURLWithPath: sidecarPath).pathExtension.lowercased())
        }?.format
        guard let resolved else {
            mapFailure(ServiceFailure(code: "INVALID_CAPTION_FORMAT", message: "対応していない字幕ファイル形式です (srt / vtt / itt)"))
            return
        }
        Task {
            busy = true
            defer { busy = false }
            do {
                let content = try String(contentsOfFile: sidecarPath, encoding: .utf8)
                var plan = try captionImportPlan(content: content, format: resolved)
                do {
                    _ = try await request("captions.import_plan", plan)
                } catch {
                    let failure = serviceFailure(error)
                    guard failure.code == "INVALID_CAPTION_FORMAT",
                          let count = Self.captionCueCount(failure) else { throw error }
                    plan["cue_ids"] = (0..<count).map { _ in
                        ["caption": UUID().uuidString.lowercased(), "clip": UUID().uuidString.lowercased()]
                    }
                    _ = try await request("captions.import_plan", plan)
                }
                // `captions.import` rejects a top-level `project`; the plan
                // payload carries it instead.
                let event = try await transport.call([
                    "operation": "captions.import",
                    "plan": plan,
                    "session_id": sessionID,
                    "idempotency_key": UUID().uuidString,
                ])
                recordIssuedEvent(event, label: "字幕ファイルの読み込み")
                try await reload()
            } catch { mapFailure(error) }
        }
    }
}
