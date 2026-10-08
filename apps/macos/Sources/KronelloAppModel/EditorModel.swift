import Foundation
import Combine
import CoreGraphics
import KronelloCore
import KronelloDesign

public struct Layer: Identifiable {
    public let id: String
    public let authored: [String: Any]
    public let evaluated: [String: Any]
    public let level: Int
    public init(id: String, authored: [String: Any], evaluated: [String: Any], level: Int) {
        self.id = id; self.authored = authored; self.evaluated = evaluated; self.level = level
    }
    public var kind: String { authored.object("kind").string("kind") }
    public var name: String { authored["name"] as? String ?? kind.capitalized }
    public var enabled: Bool { authored["enabled"] as? Bool ?? true }
    public var properties: [[String: Any]] { authored.objects("properties") }
    public var parent: String? { authored["transform_parent"] as? String }
    public var children: [String] { authored["child_order"] as? [String] ?? [] }
    public func bounds(_ stage: String) -> CGRect? {
        let b = evaluated.object("bounds").object(stage + "_bounds")
        guard let min = b["min"] as? [Double], let max = b["max"] as? [Double], min.count == 2, max.count == 2 else { return nil }
        return CGRect(x: min[0], y: min[1], width: max[0] - min[0], height: max[1] - min[1])
    }
    public func property(_ key: String) -> [String: Any]? { properties.first { $0.object("descriptor").string("key") == key } }
    public func value(_ property: [String: Any]) -> [String: Any] {
        evaluated.object("properties").object(property.string("id")).isEmpty
            ? property.object("source").object("value") : evaluated.object("properties").object(property.string("id"))
    }
}

public struct SessionUndo {
    public private(set) var undo: [String] = []
    public private(set) var redo: [String] = []
    public init() {}
    public mutating func issued(_ event: String) { undo.append(event); redo.removeAll() }
    public mutating func didUndo(_ inverse: String) { guard !undo.isEmpty else { return }; undo.removeLast(); redo.append(inverse) }
    public mutating func didRedo(_ inverse: String) { guard !redo.isEmpty else { return }; redo.removeLast(); undo.append(inverse) }
    /// Selective undo (FLOW-001): the target leaves the undo stack and the new
    /// inverse event joins redo, so Redo re-applies the target's change.
    public mutating func didSelectiveUndo(of event: String, inverse: String) {
        undo.removeAll { $0 == event }
        redo.append(inverse)
    }
}

public struct EditCandidate {
    public let base: String
    public let commands: [[String: Any]]
    public let label: String
    public let key: String
    /// GUI-011/NLE-007: dedicated shared operations (`edit.insert`,
    /// `edit.overwrite`, `clip.angle_switch`, `multicam.create`) validate,
    /// plan and apply inside one service call. `fields` carries the operation
    /// payload; base revision/session/idempotency are added at submission.
    public let direct: (operation: String, fields: [String: Any])?
    public init(base: String, commands: [[String: Any]], label: String, key: String = UUID().uuidString,
                direct: (operation: String, fields: [String: Any])? = nil) {
        self.base = base; self.commands = commands; self.label = label; self.key = key; self.direct = direct
    }
}

@MainActor public final class EditorModel: ObservableObject {
    public let path: String
    public let sessionID = UUID().uuidString
    public let transport: any ProjectTransport
    public let stateStore: UIStateStore
    @Published public var ui = ProjectUIState() { didSet {
        if oldValue.page != ui.page || oldValue.sequence != ui.sequence || oldValue.composition != ui.composition { previewFailure = nil }
        persistState()
    } }
    @Published public private(set) var projectID = ""
    @Published public private(set) var name = "Kronello"
    @Published public private(set) var revision = "0"
    @Published public private(set) var safeMode = false
    @Published public private(set) var compositions: [[String: Any]] = []
    @Published public var sequenceResult: [String: Any] = [:]
    @Published public var timelineCandidate: TimelineCandidate?
    @Published public var sequenceLoading = false
    @Published public var sequenceFailure: ServiceFailure?
    @Published public var assetSelection: String?
    /// Marker selection is session state, separate from the persisted clip selection.
    @Published public var markerSelection: String?
    /// Live ruler marker drag: marker id and its current preview frame.
    @Published public var markerDrag: (id: String, frame: Int64)?
    /// Decoded per-asset audio analyses keyed "assetID:streamIndex" (AUDIO-006).
    @Published public internal(set) var waveforms: [String: ClipWaveform] = [:]
    /// Permanent per-source analysis failures (typed error code) to avoid retry loops.
    @Published public internal(set) var waveformFailures: [String: String] = [:]
    /// In-flight `audio.analyze` keys; empty means waveform work has settled.
    @Published public internal(set) var waveformPending: Set<String> = []
    @Published public var editTool = "select"
    @Published public var editSnap = true
    @Published public var editScale: Double = 1
    @Published public private(set) var layers: [Layer] = []
    @Published public private(set) var document: [String: Any] = [:]
    @Published public private(set) var scene: [String: Any] = [:]
    @Published public private(set) var history: [[String: Any]] = [] { didSet { stampHistory() } }
    /// Rows for the undo history panel (FLOW-001), newest first. `history`
    /// itself only carries the latest delta after a reload, so the panel keeps
    /// its own list once `loadHistory()` has run; deltas merge into it.
    @Published public private(set) var historyPanel: [HistoryEntry] = []
    /// First time this session observed each event id. `history.list` carries
    /// no wall-clock field, so the panel shows observation time (display only).
    @Published public private(set) var historyRecordedAt: [String: Date] = [:]
    private var historyPanelLoaded = false
    @Published public private(set) var undoState = SessionUndo()
    @Published public private(set) var undoConflictLabel = "操作の変更"
    private var eventLabels: [String: String] = [:]
    @Published public private(set) var externalChange: String?
    @Published public private(set) var deletedSelection: String?
    @Published public var failure: ServiceFailure?
    /// Per-Property expression text fetch/commit failures, keyed "layer/property".
    /// Typed syntax diagnostics ride in ServiceFailure.details["diagnostics"] (ADR-0105).
    @Published public internal(set) var expressionFailures: [String: ServiceFailure] = [:]
    @Published public var undoConflict: ServiceFailure?
    @Published public var revisionConflict: ServiceFailure?
    @Published private var previewIssue: PreviewIssue?
    @Published public var previewRendering = false
    @Published public var previewPresented: PreviewIdentity?
    @Published public private(set) var cpuReferenceSequences: Set<String> = []
    /// GUI-011: source-monitor session state (ADR-0128). Selection and In/Out
    /// are presentation state only — never a GUI-only project mutation.
    @Published public var sourceMonitor: SourceMonitor?
    @Published private var sourcePreviewIssue: PreviewIssue?
    @Published public var sourcePreviewRendering = false
    /// The source monitor's own CPU-reference opt-in, separate from
    /// `cpuReferenceSequences` (which is scoped to sequence targets).
    @Published public var sourceCPUReference = false
    public var previewIdentity: PreviewIdentity { .init(target: ui.page == "edit" ? "sequence:\(ui.sequence ?? "")" : "composition:\(ui.composition ?? "")", revision: revision, time: ui.time) }
    public var previewFailure: ServiceFailure? {
        get { previewIssue?.identity == previewIdentity ? previewIssue?.failure : nil }
        set { previewIssue = newValue.map { .init(identity: previewIdentity, failure: $0) } }
    }
    public var usesCPUReference: Bool { ui.page == "edit" && cpuReferenceSequences.contains(ui.sequence ?? "") }
    public var offersCPUReference: Bool {
        ui.page == "edit" && !usesCPUReference && previewFailure?.code == "UNSUPPORTED_FEATURE" && previewFailure?.message.contains("video requires explicit media backend") == true
    }
    public var previewStale: Bool { usesCPUReference && (playing || previewPresented != previewIdentity) }
    public func chooseCPUReference() {
        guard offersCPUReference, let sequence = ui.sequence else { return }
        cpuReferenceSequences.insert(sequence); previewFailure = nil; refreshToken += 1
    }
    public func reportPreviewFailure(_ failure: ServiceFailure?, for identity: PreviewIdentity) {
        guard identity == previewIdentity else { return }
        previewIssue = failure.map { .init(identity: identity, failure: $0) }
    }
    /// Source-monitor preview identity: per-surface identity isolation keeps a
    /// stale source frame from being presented as program output.
    public var sourcePreviewIdentity: PreviewIdentity {
        .init(target: "source:\(sourceMonitor?.source.key ?? "")", revision: revision,
              time: sourceMonitor?.time ?? RationalTime(num: 0, den: 1))
    }
    public var sourcePreviewFailure: ServiceFailure? {
        get { sourcePreviewIssue?.identity == sourcePreviewIdentity ? sourcePreviewIssue?.failure : nil }
        set { sourcePreviewIssue = newValue.map { .init(identity: sourcePreviewIdentity, failure: $0) } }
    }
    public func reportSourcePreviewFailure(_ failure: ServiceFailure?, for identity: PreviewIdentity) {
        guard identity == sourcePreviewIdentity else { return }
        sourcePreviewIssue = failure.map { .init(identity: identity, failure: $0) }
    }
    @Published public private(set) var pendingCandidate: EditCandidate?
    @Published public var candidateBounds: CGRect?
    @Published public var keySelection: Set<KeyReference> = []
    @Published public var playing = false { didSet { if playing != oldValue { playbackRequested() } } }
    /// AUDIO-009: last rendered-block peak/RMS from the shared evaluator.
    @Published public private(set) var playbackMeters: PlaybackMeters?
    /// AUDIO-009: seek plays a short audio run through the playback pipeline.
    @Published public var audioScrubEnabled = true
    @Published public private(set) var playbackStatus = "停止"
    @Published public private(set) var playbackFailure: ServiceFailure?
    @Published public var playbackMuted = false {
        didSet {
            if playing {
                do { restartPlayback(at: try playback.samplePosition()) } catch { mapFailure(error); playing = false }
            }
        }
    }
    public let playback = RealtimePlayback()
    public let playbackEvidence = PlaybackEvidence()
    public var waitForVideoPresentation: (() async -> Void)?
    public private(set) var playbackTarget: PlaybackTarget?
    public private(set) var playbackRateNum: Int64?
    public private(set) var playbackRateDen: Int64?
    private var playbackControl: Task<Void, Never>?
    private var presentationTimer: Task<Void, Never>?
    private var resumeSample: Int64?
    private var playbackIntent: UInt64 = 0
    private var reportedUnderruns: UInt64 = 0
    @Published public private(set) var busy = false
    @Published public private(set) var refreshToken = 0
    @Published public private(set) var jobs: [[String: Any]] = []
    public var fonts: [[String: Any]] = []
    public var snapshotFonts: [[String: Any]] { Self.fontInputs(fonts, requiredBy: document) }
    public static func fontInputs(_ inputs: [[String: Any]], requiredBy document: [String: Any]) -> [[String: Any]] {
        var identities = document.objects("texts").flatMap { $0.objects("styles") }.map { $0.object("font") }
        // Caption cues lock their base and span faces the same way text does.
        for caption in document.objects("captions") {
            identities.append(caption.object("style").object("font"))
            identities += caption.objects("spans").compactMap { $0["font"] as? [String: Any] }
        }
        // Clip-level `kronello.caption.font` overrides encode the FontRef as
        // canonical JSON inside a String value.
        let overrides = document.objects("sequences")
            .flatMap { $0.objects("tracks") }
            .flatMap { $0.objects("clips") }
            .flatMap { $0.objects("properties") }
            .filter { $0.object("descriptor").string("key") == "kronello.caption.font" }
            .compactMap { $0.object("source").object("value")["value"] as? String }
            .compactMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [String: Any] }
        identities += overrides
        return inputs.filter { input in
            identities.contains { NSDictionary(dictionary: $0) == NSDictionary(dictionary: input.object("identity")) }
        }
    }
    private var polling: Task<Void, Never>?
    private var stateWrite: Task<Void, Never>?
    private var reloading = false
    private var reloadAgain = false
    private var reloadWaiters: [CheckedContinuation<Void, Error>] = []
    private var loadedUI = false
    private var numberOrigin: (base: String, layer: Layer, time: RationalTime)?
    public var current: [String: Any] { compositions.first { $0.string("id") == ui.composition } ?? [:] }
    public var selected: Layer? { layers.first { $0.id == ui.selection } }
    public var canUndo: Bool { !undoState.undo.isEmpty && !busy && pendingCandidate == nil }
    public var canRedo: Bool { !undoState.redo.isEmpty && !busy && pendingCandidate == nil }
    public var rateNum: Int64 { max(1, Int64(activeRate.string("num")) ?? 24) }
    public var rateDen: Int64 { max(1, Int64(activeRate.string("den")) ?? 1) }
    public var nominalFPS: Int { Int((activePlaybackRateNum + activePlaybackRateDen - 1) / activePlaybackRateDen) }
    public var frame: Int64 { ui.time.frames(rateNum: activePlaybackRateNum, rateDen: activePlaybackRateDen) }
    public var durationFrames: Int64 {
        if case .sequence(let id) = playbackTarget {
            let sequence = document.objects("sequences").first { $0.string("id") == id } ?? [:]
            return sequence.objects("tracks").flatMap { $0.objects("clips") }.map { playbackEndFrame($0.object("timeline_range").object("end")) }.max() ?? 0
        }
        if ui.page == "edit" { return sequenceDurationFrames }
        let duration = current.object("duration")
        return playbackEndFrame(duration)
    }
    private func playbackEndFrame(_ duration: [String: Any]) -> Int64 {
        let n = Int64(duration.string("num")) ?? 0, d = Int64(duration.string("den")) ?? 1
        let top = n.multipliedReportingOverflow(by: activePlaybackRateNum), bottom = d.multipliedReportingOverflow(by: activePlaybackRateDen)
        guard n > 0, d > 0, !top.overflow, !bottom.overflow, bottom.partialValue > 0 else { return 0 }
        return top.partialValue / bottom.partialValue + (top.partialValue % bottom.partialValue == 0 ? 0 : 1)
    }
    /// Authoring geometry remains tied to its Composition while page/playback
    /// routing changes and the next page's query is still loading.
    public var compositionExtent: CGSize {
        let dimensions = current.object("design_extent")
        return CGSize(width: dimensions.number("width"), height: dimensions.number("height"))
    }
    public var extent: CGSize {
        let dimensions: [String: Any]
        if case .sequence(let id) = playbackTarget { dimensions = document.objects("sequences").first { $0.string("id") == id }?.object("extent") ?? [:] }
        else { dimensions = activeExtent }
        return CGSize(width: dimensions.number("width"), height: dimensions.number("height"))
    }
    public var timecode: String { KRTimecode.format(frames: max(0, frame), fps: nominalFPS) }
    public var durationCode: String { KRTimecode.format(frames: max(0, durationFrames), fps: nominalFPS) }
    public var textFont: [String: Any]? {
        document.objects("texts").flatMap { $0.objects("styles") }.map { $0.object("font") }.first { font in
            fonts.contains { NSDictionary(dictionary: $0.object("identity")) == NSDictionary(dictionary: font) }
        }
    }
    public init(path: String, transport: any ProjectTransport, stateStore: UIStateStore = .init()) {
        self.path = path; self.transport = transport; self.stateStore = stateStore
        playback.onFailure = { [weak self] error in
            self?.mapFailure(error); self?.playing = false
        }
        playback.onMeters = { [weak self] meters in
            self?.playbackMeters = meters
        }
    }
    public func start(newDocument: [String: Any]? = nil) async throws {
        do {
            try await transport.ready()
            if newDocument != nil { throw ServiceFailure(code: "PROJECT_EXISTS", message: "既存のプロジェクトは上書きできません") }
        }
        catch let error as ServiceFailure where error.code == "PROJECT_NOT_FOUND" && newDocument != nil {
            _ = try await request("project.create", ["document": newDocument!])
        }
        try await reload()
        transport.notificationHandler = { [weak self] name, response in self?.notification(name, response: response) }
        try await transport.subscribe()
        polling = Task { [weak self] in
            while !Task.isCancelled {
                do { try self?.transport.poll() } catch { self?.mapFailure(error) }
                try? await Task.sleep(for: .milliseconds(100))
            }
        }
    }
    public func close() async {
        playing = false; polling?.cancel(); stateWrite?.cancel()
        presentationTimer?.cancel(); playbackControl?.cancel()
        do { try await playback.stop() } catch { mapFailure(error) }
        if let path = ProcessInfo.processInfo.environment["KRONELLO_AUDIO_TRACE"] {
            do { try playbackEvidence.write(to: URL(fileURLWithPath: path)) } catch { mapFailure(error) }
        }
        if !projectID.isEmpty { do { try await stateStore.save(ui, projectID: projectID) } catch { mapFailure(error) } }
        transport.close()
    }
    public func request(_ operation: String, _ fields: [String: Any] = [:]) async throws -> [String: Any] {
        var request = fields; request["operation"] = operation; request["project"] = path
        return try await transport.call(request)
    }
    public func reload(external: Bool = false) async throws {
        if reloading {
            reloadAgain = true
            return try await withCheckedThrowingContinuation { reloadWaiters.append($0) }
        }
        reloading = true
        if ui.page == "edit" { sequenceLoading = true }
        var completion: Result<Void, Error> = .success(())
        defer {
            reloading = false
            sequenceLoading = false
            let waiters = reloadWaiters; reloadWaiters.removeAll()
            for waiter in waiters { waiter.resume(with: completion) }
        }
        do { repeat {
            reloadAgain = false
            let old = revision
            let info = try await request("project.info")
            let export = try await request("project.export")
            projectID = info.string("project_id"); name = info.string("name"); safeMode = info.string("open_mode") == "safe"
            if !loadedUI {
                ui = try await stateStore.load(projectID: projectID); loadedUI = true
                for source in ui.fontSources ?? [] { if !fonts.contains(where: { $0.object("identity").string("sha256") == source.sha256 && Int($0.object("identity").number("face_index")) == source.faceIndex }) { fonts.append(["identity": source.identity, "path": source.path]) } }
            }
            let comps = export.object("document").objects("compositions")
            if !comps.contains(where: { $0.string("id") == ui.composition }) { ui.composition = comps.first?.string("id") }
            var scene: [String: Any] = [:]
            var sceneFailure: ServiceFailure?
            let sceneIdentity = PreviewIdentity(target: "composition:\(ui.composition ?? "")", revision: export.string("revision"), time: ui.time)
            if ui.page != "edit", let composition = ui.composition {
                do { scene = try await request("scene.query", ["composition": composition, "evaluation": ["time": ui.time.wire, "fonts": Self.fontInputs(fonts, requiredBy: export.object("document"))]]) }
                catch { sceneFailure = serviceFailure(error); scene = try await request("scene.query", ["composition": composition]) }
            }
            var timeline: [String: Any] = [:]
            let sequences = export.object("document").objects("sequences")
            if !sequences.contains(where: { $0.string("id") == ui.sequence }) { ui.sequence = sequences.first?.string("id") }
            if ui.page == "edit", let sequence = ui.sequence {
                do { timeline = try await request("sequence.query", ["sequence": sequence]); sequenceFailure = nil }
                catch { sequenceFailure = serviceFailure(error) }
            }
            // Never combine two revisions. A read race schedules a new read, never a write retry.
            if (!timeline.isEmpty && timeline.string("revision") != export.string("revision")) || info.string("revision") != export.string("revision") || (!scene.isEmpty && scene.string("revision") != export.string("revision")) {
                reloadAgain = true; continue
            }
            sequenceResult = timeline
            history = try await historySince(old, through: export.string("revision"))
            let actor = history.last(where: { $0.object("event").string("session_id") != sessionID })?.object("event").string("session_id") ?? "外部プロセス"
            let previousSelection = ui.selection
            let previousClipSelection = ui.clipSelection
            let clipDeletion = history.last { entry in
                let event = entry.object("event")
                // Timeline replaces ordered tracks as one mutation. The removed
                // placement's stable identity remains in the Event's structure keys.
                return event.objects("changed_keys").contains { key in
                    key.string("kind") == "structure" && key.string("object_id") == previousClipSelection
                } || event.objects("mutations").contains { mutation in
                    let path = mutation["path"] as? [String] ?? []
                    return mutation.string("operation") == "remove" && (path.last == previousClipSelection || path.last == ui.sequence)
                }
            }?.object("event")
            let deletion = history.last { entry in
                entry.object("event").objects("mutations").contains { mutation in
                    let path = mutation["path"] as? [String] ?? []
                    return mutation.string("operation") == "remove" && (path.last == previousSelection || path.last == ui.composition)
                }
            }?.object("event")
            adopt(document: export.object("document"), scene: scene, revision: export.string("revision"), actor: actor,
                  external: external && history.contains { $0.object("event").string("session_id") != sessionID })
            if let sceneFailure { reportPreviewFailure(sceneFailure, for: sceneIdentity) }
            if previousSelection != nil && ui.selection == nil {
                if let deletion { deletedSelection = "選択していたレイヤーは削除されました。\(deletion.string("session_id")) · rev \(deletion.string("revision"))" }
                else { deletedSelection = "選択していたレイヤーは削除されました。削除した操作者は履歴で確認してください · 読込 rev \(revision)" }
            }
            if previousClipSelection != nil && ui.clipSelection == nil {
                if let clipDeletion { deletedSelection = "選択していたクリップは削除されました。\(clipDeletion.string("session_id")) · rev \(clipDeletion.string("revision"))" }
                else { deletedSelection = "選択していたクリップは削除されました。削除した操作者は履歴で確認してください · 読込 rev \(revision)" }
            }
        } while reloadAgain
        } catch { completion = .failure(error); throw error }
    }
    /// Shared immutable query results are presentation data, never a second editable document.
    public func adopt(document: [String: Any], scene: [String: Any], revision: String, actor: String, external: Bool) {
        let previous = self.revision
        self.document = document; self.scene = scene; compositions = document.objects("compositions"); self.revision = revision
        let nodes = current.objects("nodes"), evaluated = scene.objects("nodes")
        let byID = Dictionary(uniqueKeysWithValues: nodes.map { ($0.string("id"), $0) })
        var result: [Layer] = []
        func visit(_ id: String, _ level: Int) {
            guard let authored = byID[id] else { return }
            let evaluation = evaluated.first { $0.object("key").string("node") == id && $0.object("key")["instance_path"] as? [String] == [] }?.object("evaluated") ?? [:]
            result.append(Layer(id: id, authored: authored, evaluated: evaluation, level: level))
            for child in authored["child_order"] as? [String] ?? [] { visit(child, level + 1) }
        }
        for id in current["root_nodes"] as? [String] ?? [] { visit(id, 0) }
        layers = result
        keySelection = keySelection.filter { ref in
            guard curveKeys(ref.curve).contains(where: { keyTime($0) == ref.time }) else { return false }
            guard let property = ref.property else { return true }
            let properties = compositions.flatMap { $0.objects("properties") + $0.objects("nodes").flatMap { $0.objects("properties") } }
            return properties.contains { $0.string("id") == property && curveID($0) == ref.curve }
        }
        if let selection = ui.selection, !layers.contains(where: { $0.id == selection }) {
            ui.selection = nil
            deletedSelection = "選択していたレイヤーは削除されました。\(actor) · rev \(revision)"
        }
        if external && previous != revision {
            // Events carry a session, not a client kind; show a short session tag.
            externalChange = "別のセッション（\(actor.prefix(8))）の変更を読み込みました（rev \(previous) → \(revision)）"
        }
        validateClipSelection(actor: actor)
        if let id = markerSelection, !allMarkers.contains(where: { $0.id == id }) { markerSelection = nil; markerDrag = nil }
        refreshWaveformCache()
        ensureAudioWaveforms()
        refreshToken += 1
        if previous != revision && playing, let target = activePlaybackTarget {
            Task { do { try await playback.updateSnapshot(path: path, target: target, revision: revision) } catch { mapFailure(error); playing = false } }
        }
    }
    public func select(_ id: String?, canvas: Bool = false) {
        if let id, canvas && ui.locked.contains(id) { return }
        ui.selection = id; deletedSelection = nil; candidateBounds = nil; numberOrigin = nil
    }
    func setDeletedSelection(_ message: String?) { deletedSelection = message }
    public func setComposition(_ id: String) {
        playing = false; resumeSample = nil; playbackTarget = nil; playbackRateNum = nil; playbackRateDen = nil
        ui.composition = id; ui.selection = nil
        Task { do { try await reload() } catch { mapFailure(error) } }
    }
    public func toggleLock(_ id: String) {
        if ui.locked.contains(id) { ui.locked.remove(id) } else { ui.locked.insert(id) }
    }
    public func toggleEnabled(_ layer: Layer) {
        submit([["node_enabled_set": ["composition": current.string("id"), "node": layer.id, "enabled": !layer.enabled]]], label: "Visibility の変更")
    }
    public func rename(_ layer: Layer, name: String) {
        guard !ui.locked.contains(layer.id) else { return }
        submit([["node_rename": ["composition": current.string("id"), "node": layer.id, "name": name]]], label: "Name の変更")
    }
    public func removeLayer(_ layer: Layer) {
        guard !ui.locked.contains(layer.id) else { return }
        ui.selection = nil
        submit([["node_remove": ["composition": current.string("id"), "node": layer.id]]], label: "レイヤーの削除")
    }
    public func seek(_ frame: Int64) {
        let bounded = min(max(0, frame), max(0, durationFrames - 1))
        let product = bounded.multipliedReportingOverflow(by: activePlaybackRateDen)
        guard !product.overflow else { return }
        let sample: Int64
        do { sample = try PlaybackMath.seekSample(frame: bounded, rateNum: activePlaybackRateNum, rateDen: activePlaybackRateDen) }
        catch { mapFailure(error); return }
        playbackIntent &+= 1
        playbackControl?.cancel()
        resumeSample = sample
        ui.time = RationalTime(num: product.partialValue, den: activePlaybackRateNum)
        if playing { restartPlayback(at: sample) } else { scrubAudio(at: sample) }
        if ui.page == "edit" { refreshToken += 1; return }
        Task { do { try await reload() } catch { mapFailure(error) } }
    }
    public func tick() {
        guard playing else { return }
        do {
            let sample = try playback.samplePosition()
            let next = try PlaybackMath.videoFrame(sample: sample, rateNum: activePlaybackRateNum, rateDen: activePlaybackRateDen)
            let status = playback.status
            if playbackStatus != status { playbackStatus = status }
            if let clock = playback.clock, clock.underruns > reportedUnderruns {
                reportedUnderruns = clock.underruns
                playbackFailure = ServiceFailure(code: "AUDIO_UNDERRUN", message: "音声バッファが不足しました（\(clock.underruns) 回、\(clock.missingFrames) samples）。クロックは継続します")
            }
            if next >= durationFrames {
                if ui.looping { seek(0) } else { playing = false }
                return
            }
            if next != frame {
                let product = next.multipliedReportingOverflow(by: activePlaybackRateDen)
                guard !product.overflow else { throw NativeError.service("TIME_ERROR", "Playback frame time overflow") }
                ui.time = RationalTime(num: product.partialValue, den: activePlaybackRateNum)
                // Presentation invalidation only. No scene/project/history query per frame.
            }
        } catch { mapFailure(error); playing = false }
    }
    public var activePlaybackTarget: PlaybackTarget? { playbackTarget ?? ui.composition.map(PlaybackTarget.composition) }
    public var activePlaybackRateNum: Int64 { playbackRateNum ?? rateNum }
    public var activePlaybackRateDen: Int64 { playbackRateDen ?? rateDen }
    /// GUI-003 configures a Sequence target on entry; Motion uses nil/default.
    public func configurePlayback(target: PlaybackTarget?, rateNum: Int64, rateDen: Int64) {
        do { _ = try PlaybackMath.seekSample(frame: 0, rateNum: rateNum, rateDen: rateDen) }
        catch { mapFailure(error); return }
        playing = false; resumeSample = nil
        playbackControl?.cancel()
        playbackTarget = target; playbackRateNum = rateNum; playbackRateDen = rateDen
    }
    private func playbackRequested() {
        playbackIntent &+= 1
        let intent = playbackIntent
        playbackControl?.cancel(); presentationTimer?.cancel()
        if playing {
            do {
                let sample = try resumeSample ?? PlaybackMath.seekSample(frame: frame, rateNum: activePlaybackRateNum, rateDen: activePlaybackRateDen)
                restartPlayback(at: sample)
            } catch { mapFailure(error); playing = false }
        } else {
            do { resumeSample = try playback.samplePosition() } catch { mapFailure(error) }
            playbackControl = Task {
                do {
                    let sample = try await playback.stop()
                    guard !Task.isCancelled, !playing, intent == playbackIntent else { return }
                    resumeSample = sample; playbackStatus = playback.status
                }
                catch { if !Task.isCancelled, intent == playbackIntent { mapFailure(error) } }
            }
        }
    }
    /// AUDIO-009: scrubbing reuses the realtime start/stop pipeline as a
    /// bounded run instead of a dedicated evaluator. Real playback, edits in
    /// flight, muted monitoring, and missing targets keep seeks silent.
    private func scrubAudio(at sample: Int64) {
        guard audioScrubEnabled, !playing, !playbackMuted, !busy, pendingCandidate == nil,
              ui.page == "edit", !sequenceLoading, sequenceFailure == nil,
              let target = activePlaybackTarget else { return }
        playback.scrub(path: path, target: target, revision: revision, at: sample)
    }
    private func restartPlayback(at sample: Int64) {
        playbackControl?.cancel(); presentationTimer?.cancel()
        guard let target = activePlaybackTarget else { return }
        let revision = revision
        playbackStatus = "音声を準備中"
        playbackControl = Task { [self] in
            do {
                _ = try await playback.stop()
                await waitForVideoPresentation?()
                try Task.checkCancellation()
                try await playback.start(path: path, target: target, revision: revision, at: sample, muted: playbackMuted)
                guard !Task.isCancelled, playing else { return }
                playbackStatus = playback.status
                presentationTimer = Task { [weak self] in
                    while !Task.isCancelled, let self, self.playing {
                        self.tick()
                        try? await Task.sleep(for: .milliseconds(8))
                    }
                }
            } catch is CancellationError {} catch { mapFailure(error); playing = false }
        }
    }
    public func submit(_ commands: [[String: Any]], label: String, base: String? = nil) {
        guard !busy && pendingCandidate == nil else { return }
        let candidate = EditCandidate(base: base ?? revision, commands: commands, label: label)
        Task { _ = await apply(candidate) }
    }
    /// GUI-011/NLE-007: submit one dedicated shared operation (see
    /// `EditCandidate.direct`). The service resolves, plans and applies it in
    /// a single typed call; the returned Event feeds undo like `edit.apply`.
    public func submitDirect(_ operation: String, _ fields: [String: Any], label: String, base: String? = nil) {
        guard !busy && pendingCandidate == nil else { return }
        let candidate = EditCandidate(base: base ?? revision, commands: [], label: label, direct: (operation, fields))
        Task { _ = await apply(candidate) }
    }
    @discardableResult public func apply(_ candidate: EditCandidate) async -> [String: Any]? {
        guard !busy else { return nil }
        busy = true
        defer { busy = false; candidateBounds = nil; numberOrigin = nil }
        do {
            let event: [String: Any]
            if let direct = candidate.direct {
                var fields = direct.fields
                fields["base_revision"] = candidate.base
                fields["session_id"] = sessionID
                fields["idempotency_key"] = candidate.key
                event = try await request(direct.operation, fields)
            } else {
                let plan = try await request("edit.plan", ["base_revision": candidate.base, "commands": candidate.commands])
                event = try await request("edit.apply", ["base_revision": candidate.base, "commands": candidate.commands,
                    "plan_hash": plan.string("plan_hash"), "session_id": sessionID, "idempotency_key": candidate.key])
            }
            undoState.issued(event.string("id")); pendingCandidate = nil; revisionConflict = nil
            eventLabels[event.string("id")] = candidate.label
            try await reload(); return event
        } catch {
            let error = serviceFailure(error)
            if error.code == "REVISION_CONFLICT" {
                pendingCandidate = candidate; revisionConflict = error
                do { try await reload(external: true) } catch { mapFailure(error) }
            } else { mapFailure(error) }
            return nil
        }
    }
    public func discardCandidate() { pendingCandidate = nil; revisionConflict = nil; candidateBounds = nil; numberOrigin = nil }
    public func reapply() {
        guard let previous = pendingCandidate else { return }
        pendingCandidate = nil
        // A deliberate user retry gets a fresh plan and idempotency key.
        if let direct = previous.direct {
            submitDirect(direct.operation, direct.fields, label: previous.label, base: revision)
        } else {
            submit(previous.commands, label: previous.label, base: revision)
        }
    }
    public func undo(redo: Bool = false) async {
        guard !busy, let event = redo ? undoState.redo.last : undoState.undo.last else { return }
        busy = true; defer { busy = false }
        undoConflictLabel = eventLabels[event] ?? (redo ? "やり直し" : "操作の変更")
        do {
            let inverse = try await request("edit.undo", ["base_revision": revision, "event_id": event,
                "session_id": sessionID, "idempotency_key": UUID().uuidString])
            if redo { undoState.didRedo(inverse.string("id")) } else { undoState.didUndo(inverse.string("id")) }
            eventLabels[inverse.string("id")] = undoConflictLabel
            try await reload()
            await refreshHistoryPanel()
        } catch {
            mapFailure(error)
            if undoConflict != nil {
                do { history = try await request("history.list", ["limit": 1000]).objects("events") } catch { mapFailure(error) }
            }
        }
    }
    /// Selective undo from the history panel (FLOW-001). Shared `edit.undo`
    /// applies the target event's inverse as a new event; nothing is rewritten.
    public func undoEvent(_ eventID: String) async {
        guard !busy else { return }
        busy = true; defer { busy = false }
        undoConflictLabel = eventLabels[eventID] ?? "選択した変更"
        do {
            let inverse = try await request("edit.undo", ["base_revision": revision, "event_id": eventID,
                "session_id": sessionID, "idempotency_key": UUID().uuidString])
            undoState.didSelectiveUndo(of: eventID, inverse: inverse.string("id"))
            eventLabels[inverse.string("id")] = undoConflictLabel
            try await reload()
            await refreshHistoryPanel()
        } catch {
            mapFailure(error)
            if undoConflict != nil {
                do { history = try await request("history.list", ["limit": 1000]).objects("events") } catch { mapFailure(error) }
            }
        }
    }
    public func propertyNumbers(_ layer: Layer, _ property: [String: Any]) -> [Double] {
        let value = layer.value(property)
        guard value.string("kind") != "bool" else { return [] }
        if let numbers = value["value"] as? [Double] { return numbers }
        if let scalar = value["value"] as? NSNumber { return [scalar.doubleValue] }
        return []
    }
    public func transformProperty(_ layer: Layer, key: String) -> [String: Any]? {
        if let property = layer.property(key) { return property }
        let kind: String, value: Any
        switch key {
        case "kronello.transform.position": kind = "vec2"; value = [0.0, 0.0]
        case "kronello.transform.scale": kind = "vec2"; value = [1.0, 1.0]
        case "kronello.transform.rotation": kind = "angle"; value = 0.0
        case "kronello.opacity": kind = "scalar"; value = 1.0
        default: return nil
        }
        return ["descriptor": ["key": key, "version": 1], "source": ["kind": "constant", "value": ["kind": kind, "value": value]], "modifiers": []]
    }
    public func previewNumber(layer: Layer, property: [String: Any], axis: Int, to: Double) {
        if numberOrigin == nil { numberOrigin = (revision, layer, ui.time) }
        guard let origin = numberOrigin, let bounds = origin.layer.bounds(ui.bounds) else { return }
        let values = propertyNumbers(origin.layer, property)
        guard values.indices.contains(axis) else { return }
        let key = property.object("descriptor").string("key")
        if key == "kronello.transform.position", values.count == 2 {
            let delta = CGPoint(x: axis == 0 ? to - values[0] : 0, y: axis == 1 ? to - values[1] : 0)
            let parent = parentTransform(origin.layer)
            candidateBounds = bounds.offsetBy(dx: parent.a * delta.x + parent.c * delta.y, dy: parent.b * delta.x + parent.d * delta.y)
        } else if key == "kronello.transform.scale", values[axis] != 0 {
            let sx = axis == 0 ? to / values[0] : 1, sy = axis == 1 ? to / values[1] : 1
            candidateBounds = scaledBounds(bounds, layer: origin.layer, x: sx, y: sy)
        } else if key == "kronello.transform.rotation" {
            candidateBounds = rotatedBounds(bounds, layer: origin.layer, radians: (to - values[axis]) * .pi / 180)
        }
    }
    public func commitNumber(layer: Layer, property: [String: Any], axis: Int, from: Double, to: Double) {
        guard from != to, !ui.locked.contains(layer.id) else { candidateBounds = nil; numberOrigin = nil; return }
        let origin = numberOrigin ?? (revision, layer, ui.time)
        var values = propertyNumbers(origin.layer, property)
        guard values.indices.contains(axis) else { return }
        values[axis] = to
        do {
            let command = try numericCommand(layer: origin.layer, property: property, values: values, time: origin.time)
            submit([command], label: PropertyPresentation.of(property).label + " の変更", base: origin.base)
        } catch { mapFailure(error) }
    }
    public func numericCommand(layer: Layer, property: [String: Any], values: [Double], time: RationalTime) throws -> [String: Any] {
        let value: [String: Any] = ["kind": layer.value(property).string("kind"), "value": values.count == 1 ? values[0] as Any : values as Any]
        let source = property.object("source")
        if property.string("id").isEmpty {
            var authored = property
            authored["id"] = UUID().uuidString
            authored["source"] = ["kind": "constant", "value": value]
            return ["node_property_insert": ["composition": current.string("id"), "node": layer.id, "property": authored]]
        }
        if source.string("kind") == "expression" { throw ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "式の値は直接編集できません") }
        if source.string("kind") == "curve", let curve = source["value"] as? String {
            let keys = document.objects("curves").first { $0.string("id") == curve }?.objects("keys") ?? []
            let interpolation = keys.first?.object("interpolation") ?? ["kind": "linear"]
            return ["keyframe_upsert": ["curve": curve, "key": ["time": time.wire, "value": value, "interpolation": interpolation]]]
        }
        return ["property_source_set": ["object": layer.id, "property": property.string("id"), "source": ["kind": "constant", "value": value]]]
    }
    public func notification(_ name: String, response: [String: Any]) {
        if name == "revision_changed" {
            if let value = try? NativeProjectTransport.result(response),
               let notified = UInt64(value.string("revision")), let current = UInt64(revision), notified <= current { return }
            Task { do { try await reload(external: true) } catch { mapFailure(error) } }
        } else if name == "job_progress" {
            do { jobs = try NativeProjectTransport.result(response).objects("jobs") } catch { mapFailure(error) }
        }
    }
    private func historySince(_ since: String, through: String? = nil) async throws -> [[String: Any]] {
        var cursor = since, result: [[String: Any]] = []
        let maximum = through.flatMap(UInt64.init)
        while true {
            let page = try await request("history.list", ["since_revision": cursor, "limit": 1000])
            result += page.objects("events").filter { maximum == nil || (UInt64($0.object("event").string("revision")) ?? 0) <= maximum! }
            guard let next = page["next_since_revision"] as? String, next != cursor,
                  maximum == nil || (UInt64(next) ?? 0) < maximum! else { break }
            cursor = next
        }
        return result
    }
    public func loadHistory() async throws {
        history = try await historySince("0")
        historyPanel = panelEntries(history)
        historyPanelLoaded = true
    }
    /// Re-reads the panel after an undo: an entry's `undone` flag changes in
    /// place, which incremental history deltas never report.
    private func refreshHistoryPanel() async {
        guard historyPanelLoaded else { return }
        do { historyPanel = panelEntries(try await historySince("0")) } catch { mapFailure(error) }
    }
    private func panelEntries(_ events: [[String: Any]]) -> [HistoryEntry] {
        events.compactMap { entry in
            HistoryEntry(entry: entry, labels: eventLabels, ownSession: sessionID,
                         recordedAt: historyRecordedAt[entry.object("event").string("id")] ?? .distantPast)
        }.reversed()
    }
    /// Stamps newly observed event ids and merges delta events into the panel.
    private func stampHistory() {
        let now = Date()
        for entry in history {
            let id = entry.object("event").string("id")
            if !id.isEmpty && historyRecordedAt[id] == nil { historyRecordedAt[id] = now }
        }
        guard historyPanelLoaded else { return }
        var rows = historyPanel
        for entry in history {
            let event = entry.object("event"), id = event.string("id")
            guard !id.isEmpty else { continue }
            if let index = rows.firstIndex(where: { $0.id == id }) {
                if let updated = HistoryEntry(entry: entry, labels: eventLabels, ownSession: sessionID, recordedAt: rows[index].recordedAt) {
                    rows[index] = updated
                }
            } else if let row = HistoryEntry(entry: entry, labels: eventLabels, ownSession: sessionID,
                                           recordedAt: historyRecordedAt[id] ?? now) {
                // Deltas arrive oldest-first; insert at the top to keep newest-first.
                rows.insert(row, at: 0)
            }
        }
        historyPanel = rows
    }
    public func mapFailure(_ error: Error) {
        let failure = serviceFailure(error)
        if failure.code == "UNDO_CONFLICT" { undoConflict = failure }
        else if failure.code == "REVISION_CONFLICT" { revisionConflict = failure }
        else { self.failure = failure }
    }
    public func serviceFailure(_ error: Error) -> ServiceFailure {
        if let failure = error as? ServiceFailure { return failure }
        if case NativeError.service(let code, let message) = error { return .init(code: code, message: message) }
        if case NativeError.detailed(let code, let message, let details) = error { return .init(code: code, message: message, details: details) }
        return .init(code: "GUI_IO_ERROR", message: String(describing: error))
    }
    private func persistState() {
        guard loadedUI && !projectID.isEmpty else { return }
        stateWrite?.cancel()
        let state = ui, id = projectID, store = stateStore
        stateWrite = Task { [weak self] in
            do { try await Task.sleep(for: .milliseconds(150)); try Task.checkCancellation(); try await store.save(state, projectID: id) }
            catch is CancellationError {} catch { self?.mapFailure(error) }
        }
    }
}
