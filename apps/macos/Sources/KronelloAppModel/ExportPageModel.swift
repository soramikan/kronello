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
    public func time(_ frame: Int64) -> [String: Any] { let product = frame.multipliedReportingOverflow(by: fpsDen)
        guard !product.overflow else { return ["num":"invalid","den":"1"] }
        return RationalTime(num: product.partialValue, den: max(1, fpsNum)).wire }
    public var input: [String: Any] { ["project": editor.path, "target": ["kind": selectedTarget.string("kind"), selectedTarget.string("kind"): targetValue.string("id")],
        "region": ["origin": [0,0], "extent": extent, "pixels": extent.map { Int(min(16_777_216,max(1, $0.rounded()))) }], "fonts": editor.fonts] }
    public var output: [String: Any] {
        if format == "image_sequence" { return ["format": format] }
        var result: [String: Any] = ["format": format, "profile_version": Int(version) ?? 0, "audio": audio, "clips": [[String: Any]](), "background": background == "white" ? [1,1,1] : [0,0,0]]
        if format != "pro_res_mov" { result["audio_codec"] = "alac" }
        return result
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
        let ext = selectedProfile.string("container_extension")
        if !ext.isEmpty && URL(fileURLWithPath: destination).pathExtension.lowercased() != ext { errors.append(.init(code: "INVALID_MEDIA_INPUT", message: "出力先の拡張子は ." + ext + " にしてください")) }
        if FileManager.default.fileExists(atPath: destination) { errors.append(.init(code: "OUTPUT_EXISTS", message: "既存の出力先は上書きできません")) }
        if format == "pro_res_mov" && version == "1" && audio != "explicit" { errors.append(.init(code: "UNSUPPORTED_FEATURE", message: "profile 1 は explicit 音声だけを扱います")) }
        if editor.pendingCandidate != nil || editor.busy { errors.append(.init(code: "REVISION_CONFLICT", message: "編集の確定または競合の解決を待ってください")) }
        return errors
    }
    public var canSubmit: Bool { !checking && !submitting && errors.isEmpty && checkedKey == configurationKey && checkedRevision == editor.revision }
    public static func failure(_ raw: [String: Any]) -> ServiceFailure { .init(code: raw.string("code"), message: raw.string("message"), details: raw.object("details")) }
    public func sharedRequest(_ operation: String, _ fields: [String: Any] = [:]) async throws -> [String: Any] {
        var request = fields; request["operation"] = operation
        return try await editor.transport.call(request)
    }
    public func load() async {
        if target.isEmpty { target = targets.first?.string("id") ?? ""; endFrame = totalFrames }
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
