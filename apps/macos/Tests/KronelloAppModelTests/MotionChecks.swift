import Foundation
import KronelloAppModel
import KronelloCore
import KronelloDesign

func requireResult(_ value: Bool, _ message: String) throws { try require(value, message) }

@MainActor struct MotionChecks {
    struct Fixture {
        let folder: URL
        let editor: EditorModel
        let transport: RecordingTransport
        let curve: String
        let node: String
        let property: String
    }
    func fixture(count: Int = 3) async throws -> Fixture {
        let checks = GUIChecks(), folder = try checks.temporary(), path = folder.appendingPathComponent("motion.kronello").path
        let transport = try RecordingTransport(path: path, worker: checks.root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let editor = EditorModel(path: path, transport: transport, stateStore: .init(root: folder.appendingPathComponent("state")))
        var document = EditorModel.newDocument(name: "GUI-002 checks")
        var composition = document.objects("compositions")[0]; composition["duration"] = ["num": "3", "den": "1"]; document["compositions"] = [composition]
        try await editor.start(newDocument: document)
        let commands = try editor.creationCommands(tool: "rectangle", from: .init(x: 100, y: 100), to: .init(x: 300, y: 220))
        guard await editor.apply(.init(base: editor.revision, commands: commands, label: "Fixture")) != nil else { throw GUICheckError(message: editor.failure?.message ?? "create failed") }
        let node = editor.layers[0], property = node.property("kronello.transform.position")!, curve = UUID().uuidString.lowercased()
        let keys: [[String: Any]] = (0..<count).map { i in ["time": RationalTime(num: Int64(i), den: 1).wire, "value": ["kind": "vec2", "value": [Double(i) * 100, Double(i) * 200]], "interpolation": ["kind": "cubic", "value": ["control1": [1.0 / 3, 1.0 / 3], "control2": [2.0 / 3, 2.0 / 3]]]] }
        let command: [String: Any] = ["property_source_set": ["object": node.id, "property": property.string("id"), "source": ["kind": "curve", "value": curve], "curve": ["id": curve, "value_type": "vec2", "keys": keys, "interpolation_version": 1]]]
        guard await editor.apply(.init(base: editor.revision, commands: [command], label: "Fixture Curve")) != nil else { throw GUICheckError(message: editor.failure?.message ?? "curve failed") }
        editor.select(node.id)
        return .init(folder: folder, editor: editor, transport: transport, curve: curve, node: node.id, property: property.string("id"))
    }
    func finish(_ fixture: Fixture) async { await fixture.editor.close(); try? FileManager.default.removeItem(at: fixture.folder) }
    func cliKeys(_ f: Fixture) throws -> [[String: Any]] {
        let exported = try GUIChecks().cli(["operation": "project.export", "project": f.editor.path])
        try require(exported.string("revision") == f.editor.revision, "CLI sees GUI revision")
        return exported.object("document").objects("curves").first { $0.string("id") == f.curve }!.objects("keys")
    }
    func assertParity(_ f: Fixture) throws {
        try requireResult(NSArray(array: try cliKeys(f)) == NSArray(array: f.editor.curveKeys(f.curve)), "CLI reads exactly the same authored keys as GUI")
    }
    func waitForEdit(_ editor: EditorModel, after base: String) async throws {
        for _ in 0..<1000 {
            if editor.revision != base && !editor.busy { return }
            if let failure = editor.failure { throw failure }
            try await Task.sleep(for: .milliseconds(10))
        }
        throw GUICheckError(message: "GUI release did not complete")
    }
    func verifyNavigatorAddRemove() async throws {
        let f = try await fixture(), editor = f.editor
        let key = "kronello.transform.rotation"
        let base = editor.revision
        editor.toggleKeyframe(editor.layers[0], property: editor.layers[0].property(key)!)
        try await waitForEdit(editor, after: base)
        let property = editor.layers[0].property(key)!, curve = editor.curveID(property)!
        try require(editor.curveKeys(curve).count == 1 && editor.onKeyframe(property), "Constant navigator inserts the first key")
        editor.ui.time = .init(num: 1, den: 24); try await editor.reload()
        let addBase = editor.revision
        editor.toggleKeyframe(editor.layers[0], property: editor.layers[0].property(key)!)
        try await waitForEdit(editor, after: addBase)
        try require(editor.curveKeys(curve).count == 2 && editor.keyFrame(editor.curveKeys(curve)[1]) == 1, "Navigator inserts at exact current frame")
        let deleteBase = editor.revision
        editor.toggleKeyframe(editor.layers[0], property: editor.layers[0].property(key)!)
        try await waitForEdit(editor, after: deleteBase)
        try require(editor.curveKeys(curve).count == 1, "Navigator removes the current key")
        editor.ui.time = .init(num: 0, den: 1); try await editor.reload()
        let lastBase = editor.revision
        editor.toggleKeyframe(editor.layers[0], property: editor.layers[0].property(key)!)
        try await waitForEdit(editor, after: lastBase)
        try require(editor.layers[0].property(key)!.object("source").string("kind") == "constant" && editor.previewFailure == nil, "Navigator last-key removal returns to Constant")
        await finish(f)
    }
    func verifyKeyMove() async throws {
        let f = try await fixture(); let editor = f.editor
        editor.selectKey(.init(curve: f.curve, time: .init(num: 1, den: 1)))
        let gesture = editor.beginKeyGesture()!, before = editor.revision
        let plans = f.transport.planCount, applies = f.transport.applyCount
        for offset in 1...15 { _ = editor.snappedDelta(gesture, frames: Double(offset), snap: false, tolerance: 1) }
        try require(editor.revision == before, "Key preview leaves shared revision unchanged")
        try require(f.transport.planCount == plans && f.transport.applyCount == applies, "Preview sends no plan/apply")
        editor.commitKeyMove(gesture, delta: 5)
        try await waitForEdit(editor, after: before)
        let event = f.transport.lastEvent
        try require(f.transport.planCount == plans + 1 && f.transport.applyCount == applies + 1, "Release sends exactly one plan/apply")
        try require(Int(editor.revision)! == Int(before)! + 1, "Move emits exactly one Event")
        try require(editor.keyTime(editor.curveKeys(f.curve)[1]) == .init(num: 29, den: 24), "Moved time is exact frame rational")
        let replay = try GUIChecks().cli(f.transport.lastApply)
        try require(NSDictionary(dictionary: event) == NSDictionary(dictionary: replay), "CLI receipt replay returns identical Event")
        try assertParity(f); await finish(f)
    }
    func verifyMultiKeyMove() async throws {
        let f = try await fixture(); let editor = f.editor
        editor.keySelection = Set([0, 1].map { .init(curve: f.curve, time: .init(num: Int64($0), den: 1)) })
        let candidate = try editor.moveCandidate(editor.beginKeyGesture()!, delta: 24)!
        try require(candidate.commands.count == 4, "Remove all sources before inserting: moving onto another selected key is valid")
        // Third key at 2s would collide; explicitly reject, with no partial movement.
        let original = editor.curveKeys(f.curve), base = editor.revision
        try requireResult(await editor.apply(candidate) == nil && editor.failure?.code == "INVALID_EDIT", "Unselected destination collision is typed")
        try require(editor.revision == base && NSArray(array: original) == NSArray(array: editor.curveKeys(f.curve)), "Collision has no partial effect")
        editor.failure = nil
        let move = try editor.moveCandidate(editor.beginKeyGesture()!, delta: 5)!
        try requireResult(await editor.apply(move) != nil, "Multi-key move succeeds")
        try require(Int(editor.revision)! == Int(base)! + 1 && editor.curveKeys(f.curve).map(editor.keyFrame) == [5, 29, 48], "Multi-key move is one revision")
        try assertParity(f)
        for mode in ["hold", "linear", "cubic"] {
            editor.keySelection = Set(editor.curveKeys(f.curve).prefix(2).map { KeyReference(curve: f.curve, time: editor.keyTime($0)) })
            let change = editor.interpolationCandidate(mode)!
            try requireResult(await editor.apply(change) != nil, "Interpolation uses the shared replace command")
            try require(editor.curveKeys(f.curve).prefix(2).allSatisfy { $0.object("interpolation").string("kind") == mode }, "Interpolation persists on all selected keys")
            try assertParity(f)
            try require(editor.interpolationCandidate(mode) == nil, "Choosing the current interpolation leaves existing handles intact")
        }
        await finish(f)
    }
    func verifyTangent(aligned: Bool) async throws {
        let f = try await fixture(); let editor = f.editor, before = editor.curveKeys(f.curve), base = editor.revision
        try require(editor.tangentAligned(before, index: 1, axis: 0), "Equal slopes derive aligned mode")
        let candidate = try editor.tangentCandidate(curve: f.curve, index: 1, incoming: false, control: .init(x: 0.25, y: 0.6), aligned: aligned, axis: 0, keys: before, base: base)
        try require(candidate.commands.count == (aligned ? 2 : 1) && candidate.commands.allSatisfy { !$0.object("keyframe_replace").isEmpty }, "Tangents use existing replace, aligned changes both neighbors")
        try requireResult(await editor.apply(candidate) != nil, "Tangent apply succeeds")
        let after = editor.curveKeys(f.curve)
        try require(Int(editor.revision)! == Int(base)! + 1, "Tangent edit produces one Event")
        if aligned { try require(editor.tangentAligned(after, index: 1, axis: 0), "Aligned edit preserves graph slopes") }
        else { try require(NSDictionary(dictionary: before[0]) == NSDictionary(dictionary: after[0]) && !editor.tangentAligned(after, index: 1, axis: 0), "Broken edit leaves incoming segment untouched") }
        let scene = try await editor.request("scene.query", ["composition": editor.current.string("id"), "evaluation": ["time": RationalTime(num: 3, den: 2).wire, "fonts": []]])
        let values = scene.objects("nodes").first { $0.object("key").string("node") == f.node }!.object("evaluated").object("properties").object(f.property)["value"] as! [Double]
        try require(abs(values[1] - values[0] * 2) < 1e-8 && abs(values[0] - 150) > 0.1, "Vec2 X/Y share changed easing")
        try assertParity(f); await editor.undo()
        try require(NSArray(array: editor.curveKeys(f.curve)) == NSArray(array: before), "One Undo restores both tangent segments")
        try assertParity(f); await finish(f)
    }
    func verifyLastKeyDelete() async throws {
        let f = try await fixture(count: 1); let editor = f.editor, base = editor.revision
        let ref = KeyReference(curve: f.curve, time: .init(num: 0, den: 1))
        let commands = try await editor.deletionCommands([ref], time: .init(num: 1, den: 1), base: base)
        try require(commands.count == 2 && !commands[0].object("property_source_set").isEmpty, "Last delete changes source and removes key together")
        try requireResult(await editor.apply(.init(base: base, commands: commands, label: "Last key")) != nil, "Last-key delete succeeds")
        let source = editor.layers[0].property("kronello.transform.position")!.object("source")
        try require(source.string("kind") == "constant" && (source.object("value")["value"] as? [Double]) == [0, 0], "Last key becomes evaluated Constant")
        try require(editor.previewFailure == nil && editor.curveKeys(f.curve).isEmpty && Int(editor.revision)! == Int(base)! + 1, "No empty-curve evaluation error; one Event")
        try assertParity(f); await editor.undo()
        try require(editor.layers[0].property("kronello.transform.position")!.object("source").string("kind") == "curve" && editor.curveKeys(f.curve).count == 1, "One Undo restores curve source and key")
        await finish(f)
    }
    func verifySharedLastKeyDelete(expression: Bool) async throws {
        let f = try await fixture(count: 1), editor = f.editor
        let scale = editor.layers[0].property("kronello.transform.scale")!.string("id")
        let expressionID = UUID().uuidString.lowercased()
        var commands: [[String: Any]] = []
        if expression {
            commands.append(["expression_set": ["expression": ["id": expressionID, "version": 1, "value_type": "vec2", "nodes": [["curve_sample": ["curve": f.curve, "offset": RationalTime(num: 0, den: 1).wire, "value_type": "vec2"]]]]]])
        }
        commands.append(["property_source_set": ["object": f.node, "property": scale, "source": ["kind": expression ? "expression" : "curve", "value": expression ? expressionID : f.curve]]])
        try requireResult(await editor.apply(.init(base: editor.revision, commands: commands, label: "Shared consumer")) != nil, "Shared consumer setup succeeds")
        editor.selectKey(.init(curve: f.curve, time: .init(num: 0, den: 1), object: f.node, property: f.property))
        let moveBase = editor.revision
        editor.commitKeyMove(editor.beginKeyGesture()!, delta: 5)
        try await waitForEdit(editor, after: moveBase)
        try require(editor.keySelection.first?.property == f.property, "Key move preserves edited Property context")
        let before = editor.curveKeys(f.curve), base = editor.revision
        let deletion = try await editor.deletionCommands(editor.keySelection, time: .init(num: 1, den: 1), base: base)
        try require(deletion.count == 1 && !deletion[0].object("property_source_set").isEmpty, "Shared last-key deletion only detaches edited Property")
        try requireResult(await editor.apply(.init(base: base, commands: deletion, label: "Detach last key")) != nil, "Shared last-key delete succeeds")
        try require(Int(editor.revision)! == Int(base)! + 1 && editor.keySelection.isEmpty, "Shared last-key deletion is one Event and clears detached selection")
        try require(NSArray(array: before) == NSArray(array: editor.curveKeys(f.curve)), "Other consumer keeps the complete curve and last key unchanged")
        let node = editor.layers[0]
        try require(node.property("kronello.transform.position")!.object("source").string("kind") == "constant", "Edited Property becomes Constant")
        try require(node.property("kronello.transform.scale")!.object("source").string("kind") == (expression ? "expression" : "curve"), "Other Property source is unchanged")
        let sample = try await editor.request("property.sample", ["composition": editor.current.string("id"), "keys": [["kind": "node", "instance_path": [], "node": f.node, "property": scale]], "times": [RationalTime(num: 1, den: 1).wire]])
        try require((sample.objects("samples")[0].objects("values")[0]["value"] as? [Double]) == [0, 0] && editor.previewFailure == nil, "Other consumer, including Expression, still evaluates after deletion")
        try assertParity(f)
        await editor.undo()
        try require(editor.layers[0].property("kronello.transform.position")!.object("source").string("kind") == "curve", "One Undo restores original edited Source")
        try require(NSArray(array: before) == NSArray(array: editor.curveKeys(f.curve)), "Undo leaves shared resource unchanged")
        try assertParity(f); await finish(f)
    }
    func verifyMoveUndoCLIParity() async throws {
        let f = try await fixture(); let editor = f.editor, before = editor.curveKeys(f.curve)
        editor.keySelection = Set([0, 1].map { .init(curve: f.curve, time: .init(num: Int64($0), den: 1)) })
        let move = try editor.moveCandidate(editor.beginKeyGesture()!, delta: 4)!
        try requireResult(await editor.apply(move) != nil, "Move applies through GUI model")
        let after = editor.curveKeys(f.curve); await editor.undo()
        try require(NSArray(array: editor.curveKeys(f.curve)) == NSArray(array: before), "One session Undo restores multi-key move")
        try assertParity(f); await editor.undo(redo: true)
        try require(NSArray(array: editor.curveKeys(f.curve)) == NSArray(array: after), "Redo restores multi-key move")
        try assertParity(f); await finish(f)
    }
    func verifySelectionSnapAndConflict() async throws {
        let f = try await fixture(); let editor = f.editor
        let first = KeyReference(curve: f.curve, time: .init(num: 0, den: 1)), second = KeyReference(curve: f.curve, time: .init(num: 1, den: 1))
        editor.selectKey(first); editor.selectKey(second, extend: true)
        try require(editor.keySelection.count == 2 && editor.curveKeys(f.curve).count == 3, "Shift extends existing keys without insertion")
        editor.selectKey(second); let gesture = editor.beginKeyGesture()!
        try require(editor.snappedDelta(gesture, frames: 23.8, snap: true, tolerance: 1) == 24, "Snap to another key")
        editor.ui.time = .init(num: 5, den: 4)
        try require(editor.snappedDelta(gesture, frames: 5.7, snap: true, tolerance: 1) == 6, "Snap to playhead")
        let move = try editor.moveCandidate(gesture, delta: 4)!
        try requireResult(await editor.apply(.init(base: editor.revision, commands: [["node_rename": ["composition": editor.current.string("id"), "node": f.node, "name": "Changed"]]], label: "Concurrent edit")) != nil, "Revision changes after gesture starts")
        try requireResult(await editor.apply(move) == nil && editor.revisionConflict?.code == "REVISION_CONFLICT", "Stale key gesture is rejected")
        try require(editor.pendingCandidate?.base == gesture.base && editor.pendingCandidate?.commands.count == 2, "Rejected move retains candidate for discard/reapply")
        editor.discardCandidate(); try require(editor.pendingCandidate == nil, "Explicit discard")
        await finish(f)
    }
    func verifySpatialTemporalSeparation() async throws {
        let f = try await fixture(count: 5); let editor = f.editor, base = editor.revision
        let requests = (f.transport.sampleCount, f.transport.sceneCount)
        let path = try await editor.spatialPath()
        try require(path.points.count == 72 && path.keys.count == 5, "Every composition frame and exact keys, including inactive/out-of-duration keys, are service evaluated")
        try require(f.transport.sampleCount == requests.0 + 1 && f.transport.sceneCount == requests.1, "Spatial path uses exactly one property.sample and no scene.query")
        try require(path.keys.last == .init(x: 400, y: 800), "A key outside the active range does not fail the whole path")
        try requireResult(path.mapped(by: try editor.spatialPathParentTransform()!).points == path.points, "An unparented layer maps through the scene's identity parent without more queries")
        try require(path.points[24] == .init(x: 100, y: 200) && editor.revision == base, "Spatial path uses parent-space Position and never edits")
        let long = EditorModel.pathFrames(1200)
        try require(long.count == 600 && long.first == 0 && long.last == 1199, "Long paths bounded to 600, including first and last frame")
        let display = CurveDisplay.sample(editor.curveKeys(f.curve), frame: 36, positions: [0, 24, 48, 72, 96])
        try require(display == [150, 300], "Temporal graph displays shared normalized time progress per channel")
        await finish(f)
    }
    func verifySpatialPathParentSpaceAndFailures() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), editor = GUIChecks().model(fake, folder: folder)
        let node = UUID().uuidString, parent = UUID().uuidString, property = UUID().uuidString, curve = UUID().uuidString
        var composition = fake.document.objects("compositions")[0]
        composition["duration"] = RationalTime(num: 50, den: 1).wire
        composition["root_nodes"] = [parent]
        composition["nodes"] = [
            ["id": parent, "kind": ["kind": "null"], "child_order": [node], "properties": []],
            ["id": node, "kind": ["kind": "shape"], "transform_parent": parent, "child_order": [], "properties": [
                ["id": property, "descriptor": ["key": "kronello.transform.position"], "source": ["kind": "curve", "value": curve]]
            ]]
        ]
        fake.document["compositions"] = [composition]
        fake.document["curves"] = [["id": curve, "keys": [["time": RationalTime(num: 0, den: 1).wire], ["time": RationalTime(num: 60, den: 1).wire]]]]
        editor.ui.composition = composition.string("id"); editor.ui.selection = node
        let key: [String: Any] = ["node": node, "instance_path": [String]()]
        let parentKey: [String: Any] = ["node": parent, "instance_path": [String]()]
        var scene: [String: Any] = ["nodes": [
            ["key": ["node": node, "instance_path": ["other-instance"]], "transform_parent": NSNull()],
            ["key": key, "transform_parent": parentKey, "evaluated": ["world_transform": [[99, 0, 900], [0, 99, 800]]]],
            ["key": parentKey, "transform_parent": NSNull(), "evaluated": ["world_transform": [[0, -2, 100], [3, 0, 200]]]]
        ]]
        // Exercise the actual JSONSerialization integer/NSNumber representation.
        scene = try JSONSerialization.jsonObject(with: JSONSerialization.data(withJSONObject: scene)) as! [String: Any]
        editor.adopt(document: fake.document, scene: scene, revision: "1", actor: "", external: false)
        fake.latency = .milliseconds(30)
        fake.sampleResponse = { request in
            let values = request.objects("times").indices.map { ["kind": "vec2", "value": [$0, $0 * 2]] as [String: Any] }
            let response: [String: Any] = ["revision": "1", "composition": composition.string("id"), "times": request.objects("times"), "samples": [["key": request.objects("keys")[0], "value_type": "vec2", "values": values]]]
            return try JSONSerialization.jsonObject(with: JSONSerialization.data(withJSONObject: response)) as! [String: Any]
        }
        let start = ContinuousClock.now
        let path = try await editor.spatialPath()
        let elapsed = start.duration(to: .now)
        try require(fake.requests.count == 1 && fake.requests[0].string("operation") == "property.sample" && fake.requests[0].objects("keys").count == 1, "One batched Position evaluation, no Anchor or per-frame scene requests")
        try require(fake.requests[0]["fonts"] == nil && path.points.count == 600 && path.keys.count == 2, "Read-only property evaluation keeps the 600-point cap and exact out-of-duration keys")
        try require(elapsed < .seconds(1), "A 600-point path is ready after one delayed request, not 600 sequential waits")
        try require(path.points[1] == .init(x: 1, y: 2), "Integer JSON Position points stay in parent space")
        let matrix = try editor.spatialPathParentTransform()!
        let mapped = path.mapped(by: matrix)
        try require(mapped.points[0] == .init(x: 100, y: 200) && mapped.points[1] == .init(x: 96, y: 203), "Apply the shared parent's non-identity 2x3 matrix once, not the child's own transform")
        var parentNode = scene.objects("nodes")[2]
        parentNode["evaluated"] = ["world_transform": [[0, -2, 300], [3, 0, 400]]]
        scene["nodes"] = [scene.objects("nodes")[1], parentNode]
        editor.ui.time = RationalTime(num: 1, den: 1)
        editor.adopt(document: fake.document, scene: scene, revision: "1", actor: "", external: false)
        try requireResult(path.mapped(by: try editor.spatialPathParentTransform()!).points[0] == .init(x: 300, y: 400), "An animated parent remaps cached trajectory at the new playhead")
        try require(fake.requests.count == 1, "Playhead remapping never resamples")
        for nodes in [[], [scene.objects("nodes")[0]], [scene.objects("nodes")[0], ["key": parentKey, "transform_parent": NSNull()] ]] as [[[String: Any]]] {
            editor.adopt(document: fake.document, scene: ["nodes": nodes], revision: "1", actor: "", external: false)
            do { _ = try editor.spatialPathParentTransform(); throw GUICheckError(message: "Missing selected/parent node or inactive parent must explain why it cannot draw") }
            catch let error as ServiceFailure { try require(error.code == "EVALUATION_ERROR", "Missing/inactive scene nodes surface a typed error for KRErrorLine") }
        }
        fake.sampleResponse = { request in ["revision": "1", "composition": composition.string("id"), "times": request.objects("times"), "samples": []] }
        do { _ = try await editor.spatialPath(); throw GUICheckError(message: "Missing Position sample must not silently return an empty path") }
        catch let error as ServiceFailure { try require(error.code == "EVALUATION_ERROR", "Missing sample surfaces a typed error") }
        await editor.close()
    }
    func verifyCurveReadoutPlacement() throws {
        let graph = CGSize(width: 800, height: 240), readout = CGSize(width: 140, height: 28), axisWidth: CGFloat = 54
        let first = KRCurveReadoutLayout.frame(graph: graph, playheadX: 8, axisWidth: axisWidth, readout: readout)
        let middle = KRCurveReadoutLayout.frame(graph: graph, playheadX: 400, axisWidth: axisWidth, readout: readout)
        let last = KRCurveReadoutLayout.frame(graph: graph, playheadX: 792, axisWidth: axisWidth, readout: readout)
        for frame in [first, middle, last] {
            try require(frame.minX >= axisWidth + KRSpace.space2 && frame.minY >= KRSpace.space2 && frame.maxX <= graph.width - KRSpace.space2 && frame.maxY <= graph.height, "Readout avoids measured axis gutter, ruler, and graph edges")
        }
        try require(middle.minX > 400 && last.maxX <= 792 - KRSpace.space2, "Readout sits right of the playhead, flipping left near the right edge")
        let narrow = KRCurveReadoutLayout.frame(graph: .init(width: 160, height: 60), playheadX: 8, axisWidth: axisWidth, readout: readout)
        try require(narrow.minX >= axisWidth + KRSpace.space2 && narrow.maxX <= 160 - KRSpace.space2, "Narrow readouts clip within their reserved area rather than overlapping the axis")
    }
    func verifyCurvePlayheadReadouts() throws {
        let keys: [[String: Any]] = [
            ["value": ["value": [0.0, 20.0]], "interpolation": ["kind": "cubic", "value": ["control1": [1.0 / 3, 0.0], "control2": [2.0 / 3, 1.0]]]],
            ["value": ["value": [200.0, 160.0]]]
        ]
        let position = PropertyPresentation.of("kronello.transform.position")
        let values = CurveDisplay.sample(keys, frame: 60, positions: [0, 120])
        let speed = CurveDisplay.velocity(keys, frame: 60, positions: [0, 120], framesPerSecond: 24)
        try require(values.map { position.curveReadout($0) } == ["100.0 px", "90.0 px"], "Value-mode playhead precision and units remain unchanged")
        try require(speed.map { position.curveReadout($0, velocity: true) } == ["60.0 px/s", "42.0 px/s"], "Velocity playhead shows each channel's actual derivative; constant Y offset contributes no speed")
        let earlier = CurveDisplay.velocity(keys, frame: 30, positions: [0, 120], framesPerSecond: 24)
        try require(earlier.map { position.curveReadout($0, velocity: true) } == ["45.0 px/s", "31.5 px/s"], "Velocity readout follows the playhead time")
        let fractional = CurveDisplay.velocity(keys, frame: 60, positions: [0, 120], framesPerSecond: 24000.0 / 1001)
        try require(position.curveReadout(fractional[0], velocity: true) == "59.9 px/s", "Velocity uses the actual frame rate in seconds")
        try require(PropertyPresentation.of("kronello.transform.rotation").curveReadout(48.24, velocity: true) == "48.2 °/s", "Rotation speed has degrees per second and value precision")
        for key in ["kronello.transform.scale", "kronello.opacity"] {
            let presentation = PropertyPresentation.of(key)
            try require(presentation.curveReadout(0.4824, velocity: true) == "48.2 %/s", "Percentage velocity uses the Property display multiplier exactly once")
            try require(presentation.curveReadout(0.4824) == "48.2 %", "Percentage value-mode readout remains unchanged")
        }
        try require(position.curveReadout(-48.24, velocity: true) == "-48.2 px/s", "Velocity preserves direction")
        try require(CurveDisplay.velocity([], frame: 60, positions: [], framesPerSecond: 24).isEmpty, "Absent curves do not invent a speed")
    }
    func verifyVelocitySegmentBoundaries() throws {
        let position = PropertyPresentation.of("kronello.transform.position")
        let keys: [[String: Any]] = [
            ["value": ["value": [240.0, 180.0]], "interpolation": ["kind": "linear"]],
            ["value": ["value": [600.0, 360.0]], "interpolation": ["kind": "linear"]],
            ["value": ["value": [840.0, 240.0]], "interpolation": ["kind": "hold"]]
        ]
        func check(_ source: [[String: Any]], _ frame: Double, _ expected: [Double], _ message: String, positions: [Double] = [0, 48, 96]) throws {
            let velocity = CurveDisplay.velocity(source, frame: frame, positions: positions, framesPerSecond: 24)
            try require(velocity.count == expected.count && zip(velocity, expected).allSatisfy { abs($0 - $1) < 1e-6 }, message)
        }
        try check(keys, -1, [0, 0], "Before the first key the value holds with zero velocity")
        try check(keys, 0, [180, 90], "First key uses its outgoing derivative, not half the speed")
        try check(keys, 3, [180, 90], "Inside the first segment velocity matches its slope")
        try check(keys, 47.999, [180, 90], "A difference window near the next key never averages adjacent segments")
        try check(keys, 48, [120, -60], "Middle key uses the outgoing slope, including negative direction")
        try check(keys, 60, [120, -60], "Inside the second segment velocity matches its slope")
        try check(keys, 96, [0, 0], "Last key starts the constant hold under half-open segment membership")
        try check(keys, 97, [0, 0], "After the last key the value holds with zero velocity")
        let readout = CurveDisplay.velocity(keys, frame: 0, positions: [0, 48, 96], framesPerSecond: 24).map { position.curveReadout($0, velocity: true) }
        try require(readout == ["180.0 px/s", "90.0 px/s"], "Supervisor's exact-key readout agrees with plotted channel values")
        var cubic = keys
        cubic[0]["interpolation"] = ["kind": "cubic", "value": ["control1": [1.0 / 3, 1.0 / 3], "control2": [2.0 / 3, 2.0 / 3]]]
        cubic[1]["interpolation"] = cubic[0]["interpolation"]
        try check(cubic, 0, [180, 90], "Cubic linear easing also uses the full outgoing derivative at the first key")
        try check(cubic, 48, [120, -60], "Cubic easing at a middle key selects the right segment")
        try check(cubic, 60, [120, -60], "Cubic derivative inside a segment uses the same display calculation")
        try check(cubic, 96, [0, 0], "Cubic last key also begins a constant hold")
        var held = keys; held[0]["interpolation"] = ["kind": "hold"]
        try check(held, 0, [0, 0], "Hold segment has zero outgoing velocity")
        try check(held, 47.999, [0, 0], "Hold jump at the next key never produces a difference spike")
        try check(held, 48, [120, -60], "Leaving a Hold uses only the new outgoing segment")
        try check([keys[0]], 0, [0, 0], "A single key holds with zero velocity", positions: [0])
    }
    func runAll() async throws {
        try await verifyNavigatorAddRemove(); print("PASS navigator Constant/Curve add/remove at current frame")
        try await verifyKeyMove(); print("PASS key move, rational frame time, CLI receipt")
        try await verifyMultiKeyMove(); print("PASS multi-key move, one Event, collision atomicity")
        try await verifyTangent(aligned: true); print("PASS aligned tangents, shared X/Y easing, one Undo")
        try await verifyTangent(aligned: false); print("PASS broken tangent, shared X/Y easing, one Undo")
        try await verifyLastKeyDelete(); print("PASS last-key Constant conversion, evaluation, Undo")
        try await verifySharedLastKeyDelete(expression: false); print("PASS shared Property consumer survives last-key deletion and Undo")
        try await verifySharedLastKeyDelete(expression: true); print("PASS Expression CurveSample survives last-key deletion and Undo")
        try await verifyMoveUndoCLIParity(); print("PASS multi-key session Undo/Redo and CLI parity")
        try await verifySelectionSnapAndConflict(); print("PASS Shift selection, snapping, retained revision conflict")
        try await verifySpatialTemporalSeparation(); print("PASS service evaluated spatial path and temporal graph separation")
        try verifyCurvePlayheadReadouts(); print("PASS value/velocity playhead readouts, channel derivatives, per-second units and precision")
        try await verifySpatialPathParentSpaceAndFailures(); print("PASS one-request spatial path latency, integer JSON, parent-space mapping and typed failures")
        try verifyCurveReadoutPlacement(); print("PASS curve readout avoids axes/ruler and flips at graph edges")
        try verifyVelocitySegmentBoundaries(); print("PASS half-open velocity segments, first/middle/last keys, Linear/Cubic/Hold and matching readout")
    }
}
