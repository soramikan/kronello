import Foundation
import KronelloAppModel
import KronelloCore
import KronelloDesign

struct GUICheckError: Error { let message: String }
func require(_ value: @autoclosure () -> Bool, _ message: String) throws {
    if !value() { throw GUICheckError(message: message) }
}

@MainActor final class FakeTransport: ProjectTransport {
    var notificationHandler: ((String, [String: Any]) -> Void)?
    var requests: [[String: Any]] = []
    var document = EditorModel.newDocument(name: "Tests")
    var revision = "1"
    var nextError: ServiceFailure?
    var eventID = UUID().uuidString
    var history: [[String: Any]] = []
    var latency: Duration = .zero
    var sampleResponse: (([String: Any]) throws -> [String: Any])?
    func ready() async throws {}
    func subscribe() async throws {}
    func poll() throws {}
    func close() {}
    func call(_ request: [String: Any]) async throws -> [String: Any] {
        requests.append(request)
        if latency > .zero { try await Task.sleep(for: latency) }
        switch request.string("operation") {
        case "project.info": return ["project_id": document.string("id"), "name": "Tests", "revision": revision, "open_mode": "normal"]
        case "project.export": return ["revision": revision, "document": document]
        case "scene.query": return ["revision": revision, "nodes": []]
        case "property.sample": return try sampleResponse?(request) ?? [:]
        case "history.list": return ["revision": revision, "events": history]
        case "edit.plan":
            if let error = nextError { throw error }; return ["plan_hash": "test-plan"]
        case "edit.apply", "edit.undo":
            if let error = nextError { throw error }
            revision = String((Int(revision) ?? 1) + 1); return ["id": eventID, "revision": Int(revision)!]
        default: return [:]
        }
    }
}

@MainActor final class RecordingTransport: ProjectTransport {
    let native: NativeProjectTransport
    var lastApply: [String: Any] = [:]
    var lastEvent: [String: Any] = [:]
    var planCount = 0
    var applyCount = 0
    var sampleCount = 0
    var sceneCount = 0
    var notificationHandler: ((String, [String: Any]) -> Void)? {
        get { native.notificationHandler }
        set { native.notificationHandler = newValue }
    }
    init(path: String, worker: String) throws { native = try .init(path: path, worker: worker) }
    func ready() async throws { try await native.ready() }
    func subscribe() async throws { try await native.subscribe() }
    func poll() throws { try native.poll() }
    func close() { native.close() }
    func call(_ request: [String: Any]) async throws -> [String: Any] {
        if request.string("operation") == "edit.plan" { planCount += 1 }
        if request.string("operation") == "edit.apply" { lastApply = request; applyCount += 1 }
        if request.string("operation") == "property.sample" { sampleCount += 1 }
        if request.string("operation") == "scene.query" { sceneCount += 1 }
        do {
            let result = try await native.call(request)
            if request.string("operation") == "edit.apply" { lastEvent = result }
            return result
        }
        catch {
            if let failure = error as? ServiceFailure, failure.code == "INVALID_REQUEST" {
                let data = try JSONSerialization.data(withJSONObject: request, options: [.sortedKeys])
                fputs("Rejected shared request: " + String(data: data, encoding: .utf8)! + "\n", stderr)
            }
            throw error
        }
    }
}

@MainActor struct GUIChecks {
    let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    func temporary() throws -> URL {
        let root = ProcessInfo.processInfo.environment["TMPDIR"].map { URL(fileURLWithPath: $0) } ?? FileManager.default.temporaryDirectory
        let folder = root.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true); return folder
    }
    func model(_ transport: FakeTransport, folder: URL) -> EditorModel {
        EditorModel(path: folder.appendingPathComponent("work.kronello").path, transport: transport, stateStore: .init(root: folder.appendingPathComponent("state")))
    }
    func verifySelectionReloadAndDeletion() async throws {
        let folder = try temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), editor = model(fake, folder: folder)
        try await editor.start()
        let comp = fake.document.objects("compositions")[0].string("id")
        let node = UUID().uuidString
        var document = fake.document
        var composition = document.objects("compositions")[0]
        composition["root_nodes"] = [node]
        composition["nodes"] = [["id": node, "kind": ["kind": "null"], "properties": [], "child_order": [], "enabled": true]]
        document["compositions"] = [composition]
        editor.ui.composition = comp; editor.ui.selection = node
        editor.adopt(document: document, scene: [:], revision: "2", actor: "CLI", external: true)
        try require(editor.ui.selection == node, "Reload must preserve stable selection")
        composition["nodes"] = []; composition["root_nodes"] = []; document["compositions"] = [composition]
        editor.adopt(document: document, scene: [:], revision: "3", actor: "CLI session-123", external: true)
        try require(editor.ui.selection == nil, "Deletion must clear, never choose another node")
        try require(editor.deletedSelection?.contains("rev 3") == true && editor.deletedSelection?.contains("session-123") == true, "Deletion notice must identify actor and revision")
        composition["nodes"] = [["id": node, "kind": ["kind": "null"], "properties": [], "child_order": []]]
        composition["root_nodes"] = [node]; document["compositions"] = [composition]
        editor.adopt(document: document, scene: [:], revision: "2", actor: "", external: false); editor.select(node)
        composition["nodes"] = []; composition["root_nodes"] = []; document["compositions"] = [composition]
        fake.document = document; fake.revision = "4"
        fake.history = [["event": ["revision": 3, "session_id": "Deleting session", "mutations": [["operation": "remove", "path": ["nodes", node]]]]],
                        ["event": ["revision": 4, "session_id": "Later unrelated session", "mutations": []]]]
        try await editor.reload(external: true)
        try require(editor.deletedSelection?.contains("Deleting session · rev 3") == true, "Notice identifies actual deletion event even after unrelated later changes")
        await editor.close()
    }
    func verifySessionUndoAndRedo() async throws {
        let folder = try temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), editor = model(fake, folder: folder)
        try await editor.start()
        try require(!editor.canUndo, "Opening existing history must not populate Cmd-Z")
        let event = await editor.apply(.init(base: "1", commands: [], label: "test"))
        try require(event != nil && editor.undoState.undo == [fake.eventID], "Only this session successful events enter undo")
        let issued = fake.eventID; fake.eventID = UUID().uuidString
        await editor.undo()
        try require(fake.requests.last(where: { $0.string("operation") == "edit.undo" })?.string("event_id") == issued, "Undo targets own newest event")
        let inverse = fake.eventID; fake.eventID = UUID().uuidString
        await editor.undo(redo: true)
        try require(fake.requests.last(where: { $0.string("operation") == "edit.undo" })?.string("event_id") == inverse, "Redo undoes Undo event")
        await editor.close()
    }
    func verifyConflictMappingAndExplicitRetry() async throws {
        let folder = try temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), editor = model(fake, folder: folder)
        try await editor.start()
        fake.nextError = .init(code: "REVISION_CONFLICT", message: "stale")
        _ = await editor.apply(.init(base: "0", commands: [], label: "move"))
        try require(editor.revisionConflict != nil && editor.pendingCandidate?.base == "0", "Rejected candidate stays available for explicit decision")
        try require(fake.requests.filter { $0.string("operation") == "edit.apply" }.isEmpty, "Rejected plan never applies")
        editor.discardCandidate()
        editor.mapFailure(ServiceFailure(code: "UNDO_CONFLICT", message: "same key", details: ["conflicts": [["event_id": "other"]]]))
        try require(editor.undoConflict?.details["conflicts"] != nil, "Undo sheet preserves structured conflict details")
        await editor.close()
    }
    func verifyStateIsolationAndRationalTime() async throws {
        let folder = try temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let project = folder.appendingPathComponent("work.kronello")
        let bytes = Data("unchanged project bytes".utf8); try bytes.write(to: project)
        let id = UUID().uuidString, store = UIStateStore(environment: ["KRONELLO_STATE_ROOT": folder.appendingPathComponent("user-state").path])
        var state = ProjectUIState(); state.selection = UUID().uuidString; state.panX = 42; state.zoom = "100"
        state.time = RationalTime(num: 1001, den: 24000); state.locked.insert(state.selection!); state.layout.valuesVisible = false
        try await store.save(state, projectID: id)
        let restored = try await store.load(projectID: id)
        try require(restored == state, "UI state including workspace/tool placement round trips")
        let after = try Data(contentsOf: project)
        try require(after == bytes, "State store must never touch project bytes")
        let url = try await store.stateURL(projectID: id)
        try require(url.path.contains("user-state/ui-state/"), "KRONELLO_STATE_ROOT honored")
        try require(state.time.num == "1001" && state.time.den == "24000", "Time remains rational")
        let fake = FakeTransport(), editor = model(fake, folder: folder)
        let curve = UUID().uuidString
        let key: [String: Any] = ["time": ["num": "1", "den": "48"]]
        fake.document["curves"] = [["id": curve, "keys": [key]]]
        try await editor.start()
        let property: [String: Any] = ["source": ["kind": "curve", "value": curve]]
        try require(!editor.onKeyframe(property), "A subframe key does not mark the preceding whole frame as on-key")
        editor.seekAdjacent(property, forward: true)
        try require(editor.ui.time == RationalTime(num: 1, den: 48), "Adjacent navigation does not skip subframe keys")
        editor.seekKey(key)
        try require(editor.ui.time == RationalTime(num: 1, den: 48) && editor.onKeyframe(property), "Key click keeps its exact rational time")
        var composition = fake.document.objects("compositions")[0]
        composition["duration"] = ["num": "301", "den": "100"]
        fake.document["compositions"] = [composition]
        editor.adopt(document: fake.document, scene: [:], revision: "1", actor: "", external: false)
        try require(editor.durationFrames == 73, "Half-open fractional duration includes its final valid frame")
        await editor.close()
    }
    func verifyNumberCommitOnce() async throws {
        let folder = try temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), editor = model(fake, folder: folder)
        try await editor.start()
        let property: [String: Any] = ["id": UUID().uuidString, "descriptor": ["key": "kronello.opacity"], "source": ["kind": "constant", "value": ["kind": "scalar", "value": 0.5]]]
        let layer = Layer(id: UUID().uuidString, authored: ["properties": [property]], evaluated: [:], level: 0)
        var transaction = KRNumberEdit(value: 0.5, step: 0.01); transaction.beginDrag()
        for distance in 3...20 {
            if let candidate = transaction.drag(horizontal: Double(distance)) { editor.previewNumber(layer: layer, property: property, axis: 0, to: candidate) }
        }
        try require(!fake.requests.contains(where: { $0.string("operation").hasPrefix("edit.") }), "Scrubbing sends no commands")
        if let commit = transaction.finish() { editor.commitNumber(layer: layer, property: property, axis: 0, from: commit.from, to: commit.to) }
        try require(transaction.finish() == nil, "Release/focus duplication yields at most one commit")
        for _ in 0..<50 {
            if fake.requests.contains(where: { $0.string("operation") == "edit.apply" }) { break }
            try await Task.sleep(for: .milliseconds(5))
        }
        try require(fake.requests.filter { $0.string("operation") == "edit.plan" }.count == 1 && fake.requests.filter { $0.string("operation") == "edit.apply" }.count == 1, "Numeric gesture commits one plan/apply")
        await editor.close()
    }
    func cli(_ request: [String: Any]) throws -> [String: Any] {
        let process = Process(), input = Pipe(), output = Pipe(), errors = Pipe()
        process.executableURL = root.appendingPathComponent("apps/macos/Libraries/kronello")
        process.standardInput = input; process.standardOutput = output; process.standardError = errors
        try process.run(); try input.fileHandleForWriting.write(contentsOf: JSONSerialization.data(withJSONObject: request, options: [.sortedKeys])); try input.fileHandleForWriting.close()
        let response = output.fileHandleForReading.readDataToEndOfFile(); process.waitUntilExit()
        try require(process.terminationStatus == 0, "CLI failed: " + String(data: errors.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)!)
        return try NativeProjectTransport.result(JSONSerialization.jsonObject(with: response) as! [String: Any])
    }
    func verifyGUIEditCLIEventAndNotification() async throws {
        let folder = try temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let path = folder.appendingPathComponent("shared.kronello").path
        let document = EditorModel.newDocument(name: "GUI integration")
        let transport = try RecordingTransport(path: path, worker: root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let editor = EditorModel(path: path, transport: transport, stateStore: .init(root: folder.appendingPathComponent("state")))
        try await editor.start(newDocument: document)
        try require(editor.revision == "1", "GUI new project uses shared create")
        var commands = try editor.creationCommands(tool: "rectangle", from: .init(x: 100, y: 100), to: .init(x: 400, y: 250))
        var add = commands.removeLast().object("node_add"), nodeData = add.object("node")
        nodeData["properties"] = nodeData.objects("properties").filter { $0.object("descriptor").string("key") != "kronello.transform.rotation" }
        add["node"] = nodeData; commands.append(["node_add": add])
        let layer = Layer(id: nodeData.string("id"), authored: nodeData, evaluated: [:], level: 0)
        let defaultRotation = editor.transformProperty(layer, key: "kronello.transform.rotation")!
        commands.append(try editor.numericCommand(layer: layer, property: defaultRotation, values: [15], time: editor.ui.time))
        guard let event = await editor.apply(.init(base: "1", commands: commands, label: "Shape")) else { throw GUICheckError(message: editor.failure?.message ?? "GUI edit failed") }
        let replay = try cli(transport.lastApply)
        try require(NSDictionary(dictionary: event) == NSDictionary(dictionary: replay), "GUI and CLI return exactly the same revision/Event on receipt replay")
        try require(editor.revision == "2" && editor.layers.count == 1, "Shared edit reloads query revision 2")
        try require(editor.layers[0].bounds("layout") != nil && editor.layers[0].bounds("visual") != nil, "Shared evaluated bounds decode into canvas overlays")
        let node = editor.layers[0].id; editor.select(node)
        func external(_ commands: [[String: Any]], base: String) throws {
            let plan = try cli(["operation": "edit.plan", "project": path, "base_revision": base, "commands": commands])
            _ = try cli(["operation": "edit.apply", "project": path, "base_revision": base, "commands": commands, "plan_hash": plan.string("plan_hash"), "session_id": UUID().uuidString, "idempotency_key": UUID().uuidString])
        }
        try external([["node_rename": ["composition": editor.current.string("id"), "node": node, "name": "CLI renamed"]]], base: "2")
        for _ in 0..<200 { if editor.revision == "3" { break }; try transport.poll(); try await Task.sleep(for: .milliseconds(10)) }
        try require(editor.revision == "3" && editor.selected?.name == "CLI renamed" && editor.externalChange != nil, "FFI notification automatically reloads CLI change preserving selection")
        await editor.undo()
        try require(editor.undoConflict?.code == "UNDO_CONFLICT" && editor.undoState.undo == [event.string("id")], "External structural change rejects selective GUI undo without losing own stack")
        try external([["node_remove": ["composition": editor.current.string("id"), "node": node]]], base: "3")
        for _ in 0..<200 { if editor.revision == "4" { break }; try transport.poll(); try await Task.sleep(for: .milliseconds(10)) }
        try require(editor.revision == "4" && editor.ui.selection == nil && editor.deletedSelection != nil, "External deletion clears selection and records notice")
        try require(editor.deletedSelection?.contains("rev 4") == true, "Deletion notice reads numeric Event revision exactly")
        await editor.close()
    }
    func runAll() async throws {
        try await verifySelectionReloadAndDeletion(); print("PASS selection stable ID and external deletion")
        try await verifySessionUndoAndRedo(); print("PASS GUI session Undo and inverse-event Redo")
        try await verifyConflictMappingAndExplicitRetry(); print("PASS typed conflicts and pending candidate")
        try await verifyStateIsolationAndRationalTime(); print("PASS user state isolation and rational time")
        try await verifyNumberCommitOnce(); print("PASS numeric scrub commits once")
        try await verifyCanvasDefaultsAndLock(); print("PASS canvas candidate, default Property insertion and UI lock")
        try await verifyGUIEditCLIEventAndNotification(); print("PASS GUI/CLI exact Event replay, notification, conflict, deletion")
    }
    func verifyCanvasDefaultsAndLock() async throws {
        let folder = try temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), editor = model(fake, folder: folder)
        try await editor.start()
        var document = fake.document, composition = document.objects("compositions")[0]
        let id = UUID().uuidString
        composition["nodes"] = [["id": id, "kind": ["kind": "shape"], "properties": [], "child_order": []]]
        composition["root_nodes"] = [id]; document["compositions"] = [composition]
        editor.adopt(document: document, scene: ["nodes": [["key": ["node": id, "instance_path": [String]()], "evaluated": ["bounds": ["layout_bounds": ["min": [0.0, 0.0], "max": [120.0, 60.0]]], "world_transform": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]]]], revision: "1", actor: "", external: false)
        editor.toggleLock(id); editor.select(id, canvas: true)
        try require(editor.selected == nil, "Locked nodes cannot be selected on canvas")
        editor.select(id)
        try require(editor.beginCanvasEdit() == nil, "Layer-list selection cannot manipulate locked nodes")
        editor.toggleLock(id)
        guard let edit = editor.beginCanvasEdit() else { throw GUICheckError(message: "Expected evaluated canvas bounds") }
        for _ in 0..<20 { editor.previewCanvas(edit, translation: .init(width: 0.01, height: 0.02), handle: nil, rotate: false) }
        try require(editor.candidateBounds != nil && !fake.requests.contains { $0.string("operation").hasPrefix("edit.") }, "Canvas candidates issue no edits")
        editor.commitCanvas(edit, translation: .init(width: 0.01, height: 0.02), handle: nil, rotate: false)
        for _ in 0..<50 { if fake.requests.contains(where: { $0.string("operation") == "edit.apply" }) { break }; try await Task.sleep(for: .milliseconds(5)) }
        let applies = fake.requests.filter { $0.string("operation") == "edit.apply" }
        try require(applies.count == 1 && applies[0].objects("commands")[0]["node_property_insert"] != nil, "Canvas release authors absent default once through shared insertion")
        await editor.close()
    }
}
