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
}

public struct EditCandidate {
    public let base: String
    public let commands: [[String: Any]]
    public let label: String
    public let key: String
    public init(base: String, commands: [[String: Any]], label: String, key: String = UUID().uuidString) {
        self.base = base; self.commands = commands; self.label = label; self.key = key
    }
}

@MainActor public final class EditorModel: ObservableObject {
    public let path: String
    public let sessionID = UUID().uuidString
    public let transport: any ProjectTransport
    public let stateStore: UIStateStore
    @Published public var ui = ProjectUIState() { didSet { persistState() } }
    @Published public private(set) var projectID = ""
    @Published public private(set) var name = "Kronello"
    @Published public private(set) var revision = "0"
    @Published public private(set) var safeMode = false
    @Published public private(set) var compositions: [[String: Any]] = []
    @Published public private(set) var layers: [Layer] = []
    @Published public private(set) var document: [String: Any] = [:]
    @Published public private(set) var history: [[String: Any]] = []
    @Published public private(set) var undoState = SessionUndo()
    @Published public private(set) var undoConflictLabel = "操作の変更"
    private var eventLabels: [String: String] = [:]
    @Published public private(set) var externalChange: String?
    @Published public private(set) var deletedSelection: String?
    @Published public var failure: ServiceFailure?
    @Published public var undoConflict: ServiceFailure?
    @Published public var revisionConflict: ServiceFailure?
    @Published public var previewFailure: ServiceFailure?
    @Published public private(set) var pendingCandidate: EditCandidate?
    @Published public var candidateBounds: CGRect?
    @Published public var playing = false
    @Published public private(set) var busy = false
    @Published public private(set) var refreshToken = 0
    @Published public private(set) var jobs: [[String: Any]] = []
    public var fonts: [[String: Any]] = []
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
    public var rateNum: Int64 { max(1, Int64(current.object("edit_rate").string("num")) ?? 24) }
    public var rateDen: Int64 { max(1, Int64(current.object("edit_rate").string("den")) ?? 1) }
    public var nominalFPS: Int { Int((rateNum + rateDen - 1) / rateDen) }
    public var frame: Int64 { ui.time.frames(rateNum: rateNum, rateDen: rateDen) }
    public var durationFrames: Int64 {
        let duration = current.object("duration")
        let n = Int64(duration.string("num")) ?? 0, d = Int64(duration.string("den")) ?? 1
        let top = n.multipliedReportingOverflow(by: rateNum), bottom = d.multipliedReportingOverflow(by: rateDen)
        guard n > 0, d > 0, !top.overflow, !bottom.overflow, bottom.partialValue > 0 else { return 0 }
        return top.partialValue / bottom.partialValue + (top.partialValue % bottom.partialValue == 0 ? 0 : 1)
    }
    public var extent: CGSize { CGSize(width: current.object("design_extent").number("width"), height: current.object("design_extent").number("height")) }
    public var timecode: String { KRTimecode.format(frames: max(0, frame), fps: nominalFPS) }
    public var durationCode: String { KRTimecode.format(frames: max(0, durationFrames), fps: nominalFPS) }
    public var textFont: [String: Any]? {
        document.objects("texts").flatMap { $0.objects("styles") }.map { $0.object("font") }.first { font in
            fonts.contains { NSDictionary(dictionary: $0.object("identity")) == NSDictionary(dictionary: font) }
        }
    }
    public init(path: String, transport: any ProjectTransport, stateStore: UIStateStore = .init()) {
        self.path = path; self.transport = transport; self.stateStore = stateStore
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
        var completion: Result<Void, Error> = .success(())
        defer {
            reloading = false
            let waiters = reloadWaiters; reloadWaiters.removeAll()
            for waiter in waiters { waiter.resume(with: completion) }
        }
        do { repeat {
            reloadAgain = false
            let old = revision
            let info = try await request("project.info")
            let export = try await request("project.export")
            projectID = info.string("project_id"); name = info.string("name"); safeMode = info.string("open_mode") == "safe"
            if !loadedUI { ui = try await stateStore.load(projectID: projectID); loadedUI = true }
            let comps = export.object("document").objects("compositions")
            if !comps.contains(where: { $0.string("id") == ui.composition }) { ui.composition = comps.first?.string("id") }
            var scene: [String: Any] = [:]
            if let composition = ui.composition {
                do { scene = try await request("scene.query", ["composition": composition, "evaluation": ["time": ui.time.wire, "fonts": fonts]]) }
                catch { previewFailure = serviceFailure(error); scene = try await request("scene.query", ["composition": composition]) }
            }
            // Never combine two revisions. A read race schedules a new read, never a write retry.
            if info.string("revision") != export.string("revision") || (!scene.isEmpty && scene.string("revision") != export.string("revision")) {
                reloadAgain = true; continue
            }
            history = try await historySince(old, through: export.string("revision"))
            let actor = history.last(where: { $0.object("event").string("session_id") != sessionID })?.object("event").string("session_id") ?? "外部プロセス"
            let previousSelection = ui.selection
            let deletion = history.last { entry in
                entry.object("event").objects("mutations").contains { mutation in
                    let path = mutation["path"] as? [String] ?? []
                    return mutation.string("operation") == "remove" && (path.last == previousSelection || path.last == ui.composition)
                }
            }?.object("event")
            adopt(document: export.object("document"), scene: scene, revision: export.string("revision"), actor: actor,
                  external: external && history.contains { $0.object("event").string("session_id") != sessionID })
            if previousSelection != nil && ui.selection == nil {
                if let deletion { deletedSelection = "選択していたレイヤーは削除されました。\(deletion.string("session_id")) · rev \(deletion.string("revision"))" }
                else { deletedSelection = "選択していたレイヤーは削除されました。削除した操作者は履歴で確認してください · 読込 rev \(revision)" }
            }
        } while reloadAgain
        } catch { completion = .failure(error); throw error }
    }
    /// Shared immutable query results are presentation data, never a second editable document.
    public func adopt(document: [String: Any], scene: [String: Any], revision: String, actor: String, external: Bool) {
        let previous = self.revision
        self.document = document; compositions = document.objects("compositions"); self.revision = revision
        let nodes = current.objects("nodes"), evaluated = scene.objects("nodes")
        let byID = Dictionary(uniqueKeysWithValues: nodes.map { ($0.string("id"), $0) })
        var result: [Layer] = []
        func visit(_ id: String, _ level: Int) {
            guard let authored = byID[id] else { return }
            let evaluation = evaluated.first { $0.object("key").string("node") == id }?.object("evaluated") ?? [:]
            result.append(Layer(id: id, authored: authored, evaluated: evaluation, level: level))
            for child in authored["child_order"] as? [String] ?? [] { visit(child, level + 1) }
        }
        for id in current["root_nodes"] as? [String] ?? [] { visit(id, 0) }
        layers = result
        if let selection = ui.selection, !layers.contains(where: { $0.id == selection }) {
            ui.selection = nil
            deletedSelection = "選択していたレイヤーは削除されました。\(actor) · rev \(revision)"
        }
        if external && previous != revision {
            // Events carry a session, not a client kind; show a short session tag.
            externalChange = "別のセッション（\(actor.prefix(8))）の変更を読み込みました（rev \(previous) → \(revision)）"
        }
        refreshToken += 1
    }
    public func select(_ id: String?, canvas: Bool = false) {
        if let id, canvas && ui.locked.contains(id) { return }
        ui.selection = id; deletedSelection = nil; candidateBounds = nil; numberOrigin = nil
    }
    public func setComposition(_ id: String) {
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
    public func seek(_ frame: Int64) {
        let bounded = min(max(0, frame), max(0, durationFrames - 1))
        let product = bounded.multipliedReportingOverflow(by: rateDen)
        guard !product.overflow else { return }
        ui.time = RationalTime(num: product.partialValue, den: rateNum)
        Task { do { try await reload() } catch { mapFailure(error) } }
    }
    public func tick() {
        guard playing && !busy else { return }
        if frame + 1 >= durationFrames { if ui.looping { seek(0) } else { playing = false } }
        else { seek(frame + 1) }
    }
    public func submit(_ commands: [[String: Any]], label: String, base: String? = nil) {
        guard !busy && pendingCandidate == nil else { return }
        let candidate = EditCandidate(base: base ?? revision, commands: commands, label: label)
        Task { _ = await apply(candidate) }
    }
    @discardableResult public func apply(_ candidate: EditCandidate) async -> [String: Any]? {
        guard !busy else { return nil }
        busy = true
        defer { busy = false; candidateBounds = nil; numberOrigin = nil }
        do {
            let plan = try await request("edit.plan", ["base_revision": candidate.base, "commands": candidate.commands])
            let event = try await request("edit.apply", ["base_revision": candidate.base, "commands": candidate.commands,
                "plan_hash": plan.string("plan_hash"), "session_id": sessionID, "idempotency_key": candidate.key])
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
        submit(previous.commands, label: previous.label, base: revision)
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
        } catch {
            mapFailure(error)
            if undoConflict != nil {
                do { history = try await request("history.list", ["limit": 1000]).objects("events") } catch { mapFailure(error) }
            }
        }
    }
    public func propertyNumbers(_ layer: Layer, _ property: [String: Any]) -> [Double] {
        let value = layer.value(property)
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
    public func loadHistory() async throws { history = try await historySince("0") }
    public func mapFailure(_ error: Error) {
        let failure = serviceFailure(error)
        if failure.code == "UNDO_CONFLICT" { undoConflict = failure }
        else if failure.code == "REVISION_CONFLICT" { revisionConflict = failure }
        else { self.failure = failure }
    }
    public func serviceFailure(_ error: Error) -> ServiceFailure {
        if let failure = error as? ServiceFailure { return failure }
        if case NativeError.service(let code, let message) = error { return .init(code: code, message: message) }
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
