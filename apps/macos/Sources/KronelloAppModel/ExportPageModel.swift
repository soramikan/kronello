import Foundation
import Combine
import KronelloDesign

/// Presentation state over shared capabilities/inspection/jobs; no editable project copy.
@MainActor public final class ExportPageModel: ObservableObject {
    public let editor: EditorModel
    @Published public var target = ""
    @Published public var format = ""
    @Published public var version = "3"
    @Published public var audio = "document"
    @Published public var destination = ""
    @Published public var background = "black"
    @Published public var rangeMode = "all"
    @Published public var captionFormat = "srt"
    @Published public var transfer = "pq"
    @Published public var startFrame: Int64 = 0
    @Published public var endFrame: Int64 = 1
    @Published public var filter = "all"
    @Published public var hideCompleted = false
    @Published public private(set) var profiles: [[String: Any]] = []
    @Published public private(set) var diagnostics: [ServiceFailure] = []
    @Published public private(set) var jobs: [[String: Any]] = []
    @Published public private(set) var checking = false
    @Published public private(set) var submitting = false
    @Published public var previewFailure: ServiceFailure?
    /// Explicit CPU-reference retry for preview only; final export jobs still
    /// run their configured backend and fail typed on unsupported input.
    @Published public private(set) var cpuReference = false
    @Published public private(set) var jobFailure: ServiceFailure?
    @Published public private(set) var checkedRevision: String?
    private var checkedKey: String?
    private var polling: Task<Void, Never>?
    private var checkGeneration = 0
    private var pollingGeneration = 0
    public init(editor: EditorModel) { self.editor = editor }
    public var targets: [[String: Any]] {
        editor.document.objects("compositions").map { ["id": "composition:" + $0.string("id"), "kind": "composition", "value": $0] }
        + editor.document.objects("sequences").map { ["id": "sequence:" + $0.string("id"), "kind": "sequence", "value": $0] }
    }
    public var selectedTarget: [String: Any] { targets.first { $0.string("id") == target } ?? [:] }
    public var selectedProfile: [String: Any] { profiles.first { $0.string("format") == format } ?? [:] }
    public var targetValue: [String: Any] { selectedTarget.object("value") }
    public var fpsNum: Int64 { Int64(targetValue.object(selectedTarget.string("kind") == "sequence" ? "frame_rate" : "edit_rate").string("num")) ?? 24 }
    public var fpsDen: Int64 { Int64(targetValue.object(selectedTarget.string("kind") == "sequence" ? "frame_rate" : "edit_rate").string("den")) ?? 1 }
    public var nominalFPS: Int { max(1, Int(fpsNum / max(1,fpsDen) + (fpsNum % max(1,fpsDen) == 0 ? 0 : 1))) }
    public var targetDuration: [String: Any] {
        if selectedTarget.string("kind") != "sequence" { return targetValue.object("duration") }
        // Sequence duration is the greatest exclusive clip end, in exact frames.
        let frames = targetValue.objects("tracks").flatMap { $0.objects("clips") }.map { clip -> Int64 in
            let end = clip.object("timeline_range").object("end")
            let n = Int64(end.string("num")) ?? 0, d = max(1, Int64(end.string("den")) ?? 1)
            let top = n.multipliedReportingOverflow(by: fpsNum), bottom = d.multipliedReportingOverflow(by: fpsDen)
            guard !top.overflow, !bottom.overflow, bottom.partialValue > 0 else { return 0 }
            return top.partialValue / bottom.partialValue + (top.partialValue % bottom.partialValue == 0 ? 0 : 1)
        }.max() ?? 0
        return time(frames)
    }
    public var totalFrames: Int64 {
        let d = targetDuration
        guard let n = Int64(d.string("num")), let den = Int64(d.string("den")), den > 0, fpsNum > 0, fpsDen > 0 else { return 0 }
        let top = n.multipliedReportingOverflow(by: fpsNum), bottom = den.multipliedReportingOverflow(by: fpsDen)
        guard !top.overflow, !bottom.overflow, bottom.partialValue > 0 else { return 0 }
        return top.partialValue / bottom.partialValue + (top.partialValue % bottom.partialValue == 0 ? 0 : 1)
    }
    public var extent: [Double] {
        let e = targetValue.object(selectedTarget.string("kind") == "sequence" ? "extent" : "design_extent")
        return [e.number("width"), e.number("height")]
    }
    public var firstFrame: Int64 { rangeMode == "all" ? 0 : startFrame }
    public var exclusiveFrame: Int64 { rangeMode == "all" ? totalFrames : endFrame }
    /// The sequence work_area converted into target frames, when present.
    public var targetWorkArea: (start: Int64, end: Int64)? {
        guard selectedTarget.string("kind") == "sequence" else { return nil }
        let area = targetValue.object("work_area")
        guard !area.isEmpty else { return nil }
        let start = RationalTime.wire(area.object("start")).frames(rateNum: fpsNum, rateDen: fpsDen)
        let end = RationalTime.wire(area.object("end")).frames(rateNum: fpsNum, rateDen: fpsDen)
        guard end > start else { return nil }
        return (start, end)
    }
    /// Default the range fields to the sequence work area when the target has one.
    public func applyWorkAreaDefault() {
        guard let area = targetWorkArea else { return }
        rangeMode = "inout"; startFrame = area.start; endFrame = area.end
    }
    public func time(_ frame: Int64) -> [String: Any] { let product = frame.multipliedReportingOverflow(by: fpsDen)
        guard !product.overflow else { return ["num":"invalid","den":"1"] }
        return RationalTime(num: product.partialValue, den: max(1, fpsNum)).wire }
    public var input: [String: Any] { ["project": editor.path, "target": ["kind": selectedTarget.string("kind"), selectedTarget.string("kind"): targetValue.string("id")],
        "region": ["origin": [0,0], "extent": extent, "pixels": extent.map { Int(min(16_777_216,max(1, $0.rounded()))) }], "fonts": editor.snapshotFonts, "luts": editor.lutInputs] }
    public var output: [String: Any] {
        if format == "image_sequence" { return ["format": format] }
        // Sidecar jobs serialize cues from the snapshot; no frames render.
        if format == "caption_sidecar" {
            return ["format": format, "sequence": targetValue.string("id"), "caption_format": captionFormat]
        }
        var result: [String: Any] = ["format": format, "profile_version": Int(version) ?? 0, "audio": audio, "clips": [[String: Any]](), "background": background == "white" ? [1,1,1] : [0,0,0]]
        if format == "pro_res_hdr_mov" { result["transfer"] = transfer }
        if ["av1_mp4", "h264_mov", "hevc_mov", "av1_webm"].contains(format), let codec = audioCodec {
            result["audio_codec"] = codec
        }
        return result
    }
    /// The delivery codec wire name chosen from the profile's closed
    /// `audio_codecs` list. ALAC stays the default for MOV/MP4; WebM carries
    /// Opus only, so its single entry is used verbatim.
    public var audioCodec: String? {
        let codecs = (selectedProfile["audio_codecs"] as? [String]) ?? []
        let delivery = codecs.filter { $0 != "pcm_s24le" }
        return delivery.contains("alac") ? "alac" : delivery.first
    }
    public var configurationKey: String {
        let fields: [String: Any] = ["revision": editor.revision, "input": input, "output": output, "range": ["start": time(firstFrame), "end": time(exclusiveFrame)], "destination": destination]
        return (try? JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys])).flatMap { String(data: $0, encoding: .utf8) } ?? "invalid"
    }
    public var errors: [ServiceFailure] {
        var errors = diagnostics
        if extent.contains(where: { !$0.isFinite || $0 < 1 || $0 > 16_777_216 }) || extent[0] * extent[1] > 16_777_216 { errors.append(.init(code:"INVALID_MEDIA_INPUT",message:"対象の解像度が出力の画素予算を超えています")) }
        if let previewFailure { errors.append(previewFailure) }
        if let reason = selectedProfile["reason"] as? [String: Any] { errors.append(Self.failure(reason)) }
        if profiles.isEmpty || selectedProfile.isEmpty { errors.append(.init(code: "UNSUPPORTED_FEATURE", message: "利用できる出力プロファイルを読み込んでください")) }
        if firstFrame < 0 || exclusiveFrame <= firstFrame || exclusiveFrame > totalFrames { errors.append(.init(code: "INVALID_MEDIA_INPUT", message: "範囲は対象の有効なフレーム内で指定してください")) }
        if destination.isEmpty { errors.append(.init(code: "INVALID_MEDIA_INPUT", message: "出力先を選択してください")) }
        if format == "caption_sidecar" && selectedTarget.string("kind") != "sequence" {
            errors.append(.init(code: "UNSUPPORTED_FEATURE", message: "字幕サイドカーは sequence を対象にしてください"))
        }
        let ext = format == "caption_sidecar" ? captionFormat : selectedProfile.string("container_extension")
        if !ext.isEmpty && URL(fileURLWithPath: destination).pathExtension.lowercased() != ext { errors.append(.init(code: "INVALID_MEDIA_INPUT", message: "出力先の拡張子は ." + ext + " にしてください")) }
        if FileManager.default.fileExists(atPath: destination) { errors.append(.init(code: "OUTPUT_EXISTS", message: "既存の出力先は上書きできません")) }
        if format == "pro_res_mov" && version == "1" && audio != "explicit" { errors.append(.init(code: "UNSUPPORTED_FEATURE", message: "profile 1 は explicit 音声だけを扱います")) }
        if editor.pendingCandidate != nil || editor.busy { errors.append(.init(code: "REVISION_CONFLICT", message: "編集の確定または競合の解決を待ってください")) }
        return errors
    }
    /// Same explicit recovery as the edit monitors: when the preview reports
    /// an unsupported media backend, offer a CPU-reference retry.
    public var offersCPUReference: Bool {
        !cpuReference && previewFailure?.code == "UNSUPPORTED_FEATURE"
            && previewFailure?.message.contains("video requires explicit media backend") == true
    }
    public func chooseCPUReference() {
        guard offersCPUReference else { return }
        cpuReference = true; previewFailure = nil
    }
    public var canSubmit: Bool { !checking && !submitting && errors.isEmpty && checkedKey == configurationKey && checkedRevision == editor.revision }
    public static func failure(_ raw: [String: Any]) -> ServiceFailure { .init(code: raw.string("code"), message: raw.string("message"), details: raw.object("details")) }
    public func sharedRequest(_ operation: String, _ fields: [String: Any] = [:]) async throws -> [String: Any] {
        var request = fields; request["operation"] = operation
        return try await editor.transport.call(request)
    }
    public func load() async {
        if target.isEmpty { target = targets.first?.string("id") ?? ""; endFrame = totalFrames; applyWorkAreaDefault() }
        do {
            profiles = try await sharedRequest("capabilities.get").objects("export_profiles")
            if format.isEmpty, let profile = profiles.first(where: { $0.string("format") == "pro_res_mov" }) ?? profiles.first { format = profile.string("format"); selectProfile() }
        } catch { diagnostics = [editor.serviceFailure(error)] }
        await refreshJobs()
        beginPollingIfActive()
    }
    public func clearInspection() { diagnostics = []; checkedKey = nil; checkedRevision = nil }
    public func selectProfile() {
        version = (selectedProfile["profile_versions"] as? [Int])?.last.map(String.init) ?? "1"
        audio = format == "image_sequence" ? "explicit" : "document"
        checkedKey = nil
    }
    /// render.explain accepts one time. This is start-frame semantic inspection,
    /// not a full-range, encoder-open or audio-clipping acceptance verdict.
    public func check() async {
        checkGeneration += 1; let generation = checkGeneration
        let key = configurationKey, revision = editor.revision, requestInput = input, checkedTime = time(firstFrame)
        checking = true; checkedKey = nil; checkedRevision = nil
        defer { if generation == checkGeneration { checking = false } }
        do {
            let result = try await sharedRequest("render.explain", ["input": requestInput, "time": checkedTime])
            guard generation == checkGeneration, key == configurationKey else { return }
            guard result.string("revision") == revision else { diagnostics = [.init(code: "REVISION_CONFLICT", message: "確認中に作品が変更されました。再読込して確認してください")]; return }
            diagnostics = result.objects("diagnostics").map(Self.failure)
            if result["plan"] == nil || result["plan"] is NSNull, diagnostics.isEmpty { diagnostics = [.init(code: "INVALID_RESPONSE", message: "レンダー計画がありません")] }
            checkedKey = key; checkedRevision = revision
        } catch { if generation == checkGeneration { diagnostics = [editor.serviceFailure(error)] } }
    }
    public func submit() async {
        guard canSubmit else { return }
        submitting = true; defer { submitting = false }
        do {
            let job = try await sharedRequest("render.submit", ["expected_revision": checkedRevision!, "render": ["input": input, "range": ["start": time(firstFrame), "end": time(exclusiveFrame)],
                "frame_rate": ["num": String(fpsNum), "den": String(fpsDen)], "output_directory": destination], "output": output])
            jobs.removeAll { $0.string("id") == job.string("id") }; jobs.insert(job, at: 0)
            jobFailure = nil; checkedKey = nil; beginPollingIfActive()
        } catch { jobFailure = editor.serviceFailure(error); diagnostics = [jobFailure!] }
    }
    public func refreshJobs() async {
        do { jobs = try await sharedRequest("job.list").objects("jobs").filter { $0.string("project_id") == editor.projectID }; jobFailure = nil }
        catch { jobFailure = editor.serviceFailure(error) }
    }
    public var activeJobs: Bool { jobs.contains { ["queued","running"].contains($0.string("status")) } }
    public func beginPollingIfActive() {
        guard polling == nil, activeJobs else { return }
        pollingGeneration += 1; let generation = pollingGeneration
        polling = Task { [weak self] in
            defer { if self?.pollingGeneration == generation { self?.polling = nil } }
            while !Task.isCancelled {
                do { try await Task.sleep(for: .seconds(1)) } catch { return }
                guard let self else { return }
                await self.refreshJobs()
                if !self.activeJobs || self.jobFailure != nil { return }
            }
        }
    }
    public func stopPolling() { pollingGeneration += 1; polling?.cancel(); polling = nil }
    public func cancel(_ id: String) async {
        do { _ = try await sharedRequest("job.cancel", ["job": id]); await refreshJobs(); beginPollingIfActive() }
        catch { jobFailure = editor.serviceFailure(error) }
    }
    public var visibleJobs: [[String: Any]] { jobs.filter { job in
        let status = job.string("status"), active = ["queued","running"].contains(status)
        if hideCompleted && ["succeeded","canceled"].contains(status) { return false }
        switch filter { case "active": return active; case "done": return status == "succeeded"; case "failed": return ["failed","interrupted"].contains(status); default: return true }
    } }
    // MARK: - FLOW-003 shared export presets and batch queue (ADR-0130)
    /// Presets live in the shared document; selection is session-only UI state.
    @Published public var presetSelection: Set<String> = []
    @Published public var presetName = ""
    /// Per-item outcomes of the last `export.batch`, in request order.
    @Published public private(set) var batchResults: [[String: Any]] = []
    public var presets: [[String: Any]] { editor.document.objects("export_presets") }
    /// Output extension for destination naming: the movie container, the
    /// caption sidecar format, or none for image sequences (a directory).
    /// Mirrors `preset_output_extension` in the shared service.
    public func presetExtension(_ preset: [String: Any]) -> String? {
        let output = preset.object("output")
        switch output.string("format") {
        case "image_sequence": return nil
        case "caption_sidecar": return output.string("caption_format")
        default: return profiles.first { $0.string("format") == output.string("format") }?.string("container_extension")
        }
    }
    /// The destination stem rule `kronello watch` applies, so GUI batch exports
    /// and watch folders name outputs identically.
    public static func sanitizedStem(_ name: String) -> String {
        let mapped = name.prefix(64).map { character -> Character in
            guard let value = character.asciiValue else { return "_" }
            let okay = (65...90).contains(value) || (97...122).contains(value)
                || (48...57).contains(value) || value == 45 || value == 95
            return okay ? character : "_"
        }
        return mapped.isEmpty ? "preset" : String(mapped)
    }
    /// The shared ExportPreset payload mirroring this form's render settings
    /// field-for-field (destination is supplied per submission, never stored).
    public func presetPayload(id: String, name: String) -> [String: Any] {
        var preset: [String: Any] = [
            "version": 1, "id": id, "name": name,
            "range": ["start": time(firstFrame), "end": time(exclusiveFrame)],
            "frame_rate": ["num": String(fpsNum), "den": String(fpsDen)],
            "region": ["origin": [0.0, 0.0], "extent": extent,
                       "pixels": extent.map { Int(min(16_777_216, max(1, $0.rounded()))) }],
            "profile": ["working_space": "linear_rec709", "flatten_tolerance_px": 0.02],
            "output": output,
        ]
        if selectedTarget.string("kind") == "sequence" {
            preset["target"] = ["kind": "sequence", "sequence": targetValue.string("id")]
        } else {
            preset["composition"] = targetValue.string("id")
        }
        return preset
    }
    /// Upsert a preset through the shared edit commands; a reused name keeps
    /// the same id so stored identity stays stable.
    public func savePreset(name: String) async {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        let id = presets.first { $0.string("name") == name }?.string("id") ?? UUID().uuidString.lowercased()
        _ = await editor.apply(.init(base: editor.revision,
            commands: [["export_preset_save": ["preset": presetPayload(id: id, name: name)]]],
            label: "書き出しプリセットの保存"))
    }
    public func deletePreset(_ id: String) async {
        _ = await editor.apply(.init(base: editor.revision,
            commands: [["export_preset_delete": ["preset": id]]],
            label: "書き出しプリセットの削除"))
        presetSelection.remove(id)
        batchResults.removeAll()
    }
    /// Restore the form fields a preset was saved with.
    public func loadPreset(_ id: String) {
        guard let preset = presets.first(where: { $0.string("id") == id }) else { return }
        let output = preset.object("output")
        let sequence = preset.object("target").string("sequence")
        let composition = preset.string("composition")
        if !sequence.isEmpty { target = "sequence:" + sequence }
        else if !composition.isEmpty { target = "composition:" + composition }
        format = output.string("format")
        selectProfile()
        if let version = output["profile_version"] as? Int { self.version = String(version) }
        if !output.string("audio").isEmpty { audio = output.string("audio") }
        if !output.string("caption_format").isEmpty { captionFormat = output.string("caption_format") }
        if !output.string("transfer").isEmpty { transfer = output.string("transfer") }
        if let background = output["background"] as? [NSNumber] { self.background = background.contains(1) ? "white" : "black" }
        let rateNum = Int64(preset.object("frame_rate").string("num")) ?? fpsNum
        let rateDen = Int64(preset.object("frame_rate").string("den")) ?? 1
        let start = RationalTime.wire(preset.object("range").object("start")).frames(rateNum: rateNum, rateDen: rateDen)
        let end = RationalTime.wire(preset.object("range").object("end")).frames(rateNum: rateNum, rateDen: rateDen)
        if start != 0 || end != totalFrames { rangeMode = "inout"; startFrame = start; endFrame = end }
        presetName = preset.string("name")
        clearInspection()
    }
    /// Submit every selected preset as one ordered `export.batch`; each item
    /// resolves server-side into the identical `render.submit` payload.
    /// Destinations derive from one chosen directory by the shared naming rule.
    public func submitPresetBatch(directory: String) async {
        let selected = presets.filter { presetSelection.contains($0.string("id")) }
        guard !selected.isEmpty, !submitting else { return }
        submitting = true; defer { submitting = false }
        let items: [[String: Any]] = selected.map { preset in
            let suffix = presetExtension(preset).map { "." + $0 } ?? ""
            return ["preset": preset.string("id"), "project": editor.path,
                    "destination": directory + "/" + Self.sanitizedStem(preset.string("name")) + suffix]
        }
        do {
            let result = try await sharedRequest("export.batch", ["items": items, "failure_policy": "continue"])
            batchResults = result.objects("items")
            for item in batchResults {
                if let job = item["job"] as? [String: Any] {
                    jobs.removeAll { $0.string("id") == job.string("id") }
                    jobs.insert(job, at: 0)
                }
            }
            // refreshJobs clears jobFailure on success; the first typed item
            // failure must win so it surfaces in the jobs panel.
            let itemFailure = batchResults.first { $0.string("outcome") == "failed" }.map { item -> ServiceFailure in
                let error = item.object("error")
                return .init(code: error.string("code"), message: error.string("message"), details: error.object("details"))
            }
            await refreshJobs()
            if let itemFailure { jobFailure = itemFailure }
            beginPollingIfActive()
        } catch { jobFailure = editor.serviceFailure(error) }
    }
    public static func state(_ job: [String: Any]) -> KRJobState {
        switch job.string("status") {
        case "queued": return .queued
        case "running": return .running(progress: min(1, max(0, job.number("completed_frames") / max(1, job.number("total_frames")))), remaining: job["cancel_requested"] as? Bool == true ? "中止を要求済み" : "残り時間は未測定")
        case "succeeded": return .done(elapsed: "\(Int(max(0, job.number("finished_at_ms") - job.number("submitted_at_ms")) / 1000))s")
        case "canceled", "cancelled": return .cancelled
        case "interrupted": return .interrupted
        case "failed": return .failed(.init(job.object("error").string("code"), job.object("error").string("message")))
        default: return .failed(.init("INVALID_RESPONSE", "未知のジョブ状態"))
        }
    }
}
