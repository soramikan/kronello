import XCTest
import KronelloAppModel
import KronelloDesign

@MainActor final class ExpressionAuthoringTests: XCTestCase {
    /// One node with the given properties; stored expressions, curves and an
    /// optional evaluated scene payload complete the fake project.
    private func makeEditor(_ fake: FakeTransport, folder: URL, expressions: [[String: Any]] = [], nodeProperties: [[String: Any]],
                            curves: [[String: Any]] = [], evaluated: [String: Any] = [:]) async throws -> (EditorModel, Layer) {
        let editor = GUIChecks().model(fake, folder: folder)
        try await editor.start()
        var document = fake.document, composition = document.objects("compositions")[0]
        let node = UUID().uuidString
        composition["nodes"] = [["id": node, "kind": ["kind": "shape"], "properties": nodeProperties, "child_order": []]]
        composition["root_nodes"] = [node]; document["compositions"] = [composition]
        document["expressions"] = expressions; document["curves"] = curves
        let scene: [String: Any] = evaluated.isEmpty ? [:] : ["nodes": [["key": ["node": node, "instance_path": []], "evaluated": evaluated]]]
        editor.adopt(document: document, scene: scene, revision: fake.revision, actor: "", external: false)
        return (editor, editor.layers.first { $0.id == node }!)
    }
    private func expressionProperty(id: String = UUID().uuidString, source: [String: Any], key: String = "kronello.opacity") -> [String: Any] {
        ["id": id, "descriptor": ["key": key, "version": 1], "source": source, "modifiers": []]
    }
    private func applyCommands(_ fake: FakeTransport) async -> [[String: Any]]? {
        for _ in 0..<400 {
            if let apply = fake.requests.last(where: { $0.string("operation") == "edit.apply" }) { return apply.objects("commands") }
            try? await Task.sleep(for: .milliseconds(5))
        }
        return nil
    }
    private func waitForError(_ editor: EditorModel, _ layer: Layer, _ property: [String: Any]) async -> Bool {
        for _ in 0..<400 {
            if editor.expressionError(layer, property) != nil { return true }
            try? await Task.sleep(for: .milliseconds(5))
        }
        return false
    }

    func testFormatFetchesCanonicalTextByExpressionID() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), expression = UUID().uuidString
        let (editor, layer) = try await makeEditor(fake, folder: folder,
            expressions: [["id": expression, "version": 1, "value_type": "scalar", "nodes": [["literal": ["kind": "scalar", "value": 1.0]]]]],
            nodeProperties: [expressionProperty(source: ["kind": "expression", "value": expression])])
        let property = layer.property("kronello.opacity")!
        var requested = ""
        fake.formatResponse = { request in requested = request.string("expression_id"); return ["expression_id": expression, "text": "1 + 1"] }
        let text = await editor.expressionText(layer, property: property)
        XCTAssertEqual(text, "1 + 1")
        XCTAssertEqual(requested, expression)
        XCTAssertTrue(fake.requests.contains { $0.string("operation") == "expression.format" })
        XCTAssertNil(editor.expressionError(layer, property))
        await editor.close()
    }

    func testFormatFailureSurfacesInlineAndNonExpressionIsIgnored() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), expression = UUID().uuidString
        let (editor, layer) = try await makeEditor(fake, folder: folder,
            expressions: [["id": expression, "version": 1, "value_type": "scalar", "nodes": []]],
            nodeProperties: [expressionProperty(source: ["kind": "expression", "value": expression]),
                             expressionProperty(source: ["kind": "constant", "value": ["kind": "scalar", "value": 0.5]], key: "kronello.transform.rotation")])
        let property = layer.property("kronello.opacity")!, constant = layer.property("kronello.transform.rotation")!
        fake.formatResponse = { _ in throw ServiceFailure(code: "EXPRESSION_NOT_FOUND", message: "式を読み取れません") }
        let failed = await editor.expressionText(layer, property: property)
        XCTAssertNil(failed)
        XCTAssertEqual(editor.expressionError(layer, property)?.code, "EXPRESSION_NOT_FOUND")
        let ignored = await editor.expressionText(layer, property: constant)
        XCTAssertNil(ignored)
        XCTAssertFalse(fake.requests.contains { $0.string("operation") == "expression.format" && $0.string("expression_id") != expression })
        await editor.close()
    }

    func testCommitPreservesStoredEnvelopeMetadata() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), expression = UUID().uuidString, property = UUID().uuidString
        let budget: [String: Any] = ["instructions": 100, "memory_bytes": 200, "samples": 3, "nodes": 4, "dependencies": 5]
        let (editor, layer) = try await makeEditor(fake, folder: folder,
            expressions: [["id": expression, "version": 2, "value_type": "scalar", "budget": budget,
                           "nodes": [["literal": ["kind": "scalar", "value": 1.0]]]]],
            nodeProperties: [expressionProperty(id: property, source: ["kind": "expression", "value": expression])])
        let prop = layer.property("kronello.opacity")!
        editor.commitExpressionText(layer, property: prop, text: "1 + 2")
        let applied = await applyCommands(fake)
        let commands = try XCTUnwrap(applied, "commit must reach edit.apply")
        XCTAssertEqual(commands.count, 1)
        let command = commands[0].object("property_expression_text_set")
        XCTAssertEqual(command.string("object"), layer.id)
        XCTAssertEqual(command.string("property"), property)
        XCTAssertEqual(command.string("text"), "1 + 2")
        let metadata = command.object("metadata")
        XCTAssertEqual(metadata.string("id"), expression, "metadata preserves the stored expression identity")
        XCTAssertEqual(metadata["version"] as? Int, 2, "metadata preserves the stored semantics version")
        XCTAssertEqual(metadata.string("value_type"), "scalar")
        XCTAssertEqual(NSDictionary(dictionary: metadata.object("budget")), NSDictionary(dictionary: budget))
        await editor.close()
    }

    func testCommitMintsEnvelopeOnConstantSource() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport()
        let (editor, layer) = try await makeEditor(fake, folder: folder,
            nodeProperties: [expressionProperty(source: ["kind": "constant", "value": ["kind": "scalar", "value": 0.5]])])
        let prop = layer.property("kronello.opacity")!
        editor.commitExpressionText(layer, property: prop, text: "0.5")
        let minted = await applyCommands(fake)
        let commands = try XCTUnwrap(minted)
        let metadata = commands[0].object("property_expression_text_set").object("metadata")
        XCTAssertNotNil(UUID(uuidString: metadata.string("id")), "a fresh expression id is minted")
        XCTAssertEqual(metadata["version"] as? Int, 3, "new expressions use EXPRESSION_SUPPORTED_VERSION")
        XCTAssertEqual(metadata.string("value_type"), "scalar", "the declared value type comes from the authored Value kind")
        XCTAssertEqual(NSDictionary(dictionary: metadata.object("budget")),
                       NSDictionary(dictionary: ["instructions": 4096, "memory_bytes": 1_048_576, "samples": 64, "nodes": 1024, "dependencies": 64]))
        await editor.close()
    }

    func testCommitUsesCurveDeclaredValueType() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), curve = UUID().uuidString
        let (editor, layer) = try await makeEditor(fake, folder: folder,
            nodeProperties: [expressionProperty(source: ["kind": "curve", "value": curve])],
            curves: [["id": curve, "value_type": "vec2", "keys": [["time": ["num": "0", "den": "1"], "value": ["kind": "vec2", "value": [0.0, 0.0]], "interpolation": ["kind": "linear"]]]]])
        let prop = layer.property("kronello.opacity")!
        editor.commitExpressionText(layer, property: prop, text: "vec2(0, 0)")
        let curveCommands = await applyCommands(fake)
        let commands = try XCTUnwrap(curveCommands)
        let metadata = commands[0].object("property_expression_text_set").object("metadata")
        XCTAssertEqual(metadata.string("value_type"), "vec2", "curve sources declare the curve's value_type")
        XCTAssertEqual(metadata["version"] as? Int, 3)
        await editor.close()
    }

    func testSyntaxErrorSurfacesDiagnosticsWithoutMutation() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), expression = UUID().uuidString
        let storedNodes: [[String: Any]] = [["literal": ["kind": "scalar", "value": 1.0]]]
        let (editor, layer) = try await makeEditor(fake, folder: folder,
            expressions: [["id": expression, "version": 1, "value_type": "scalar", "nodes": storedNodes]],
            nodeProperties: [expressionProperty(source: ["kind": "expression", "value": expression])])
        let prop = layer.property("kronello.opacity")!
        fake.nextError = .init(code: "EXPRESSION_SYNTAX", message: "1:5: unexpected end",
            details: ["diagnostics": [["byte_start": 4, "byte_end": 4, "line": 1, "column": 5,
                                       "expected": ["expression"], "message": "unexpected end"]]])
        editor.commitExpressionText(layer, property: prop, text: "1 + ")
        let surfaced = await waitForError(editor, layer, prop)
        XCTAssertTrue(surfaced, "typed failure reaches the property")
        XCTAssertFalse(fake.requests.contains { $0.string("operation") == "edit.apply" }, "rejected planning never applies")
        XCTAssertEqual(editor.revision, fake.revision, "rejected text does not change the revision")
        XCTAssertEqual(editor.document.objects("expressions").first?.objects("nodes").count, storedNodes.count,
                       "the stored AST is preserved exactly")
        XCTAssertEqual(prop.object("source").string("kind"), "expression")
        XCTAssertEqual(editor.expressionError(layer, prop)?.code, "EXPRESSION_SYNTAX")
        let diagnostics = editor.expressionDiagnostics(layer, prop)
        XCTAssertEqual(diagnostics.count, 1)
        XCTAssertEqual(diagnostics[0].line, 1); XCTAssertEqual(diagnostics[0].column, 5)
        XCTAssertEqual(diagnostics[0].byteStart, 4); XCTAssertEqual(diagnostics[0].byteEnd, 4)
        XCTAssertEqual(diagnostics[0].expected, ["expression"])
        XCTAssertEqual(diagnostics[0].message, "unexpected end")
        await editor.close()
    }

    func testSuccessfulCommitClearsFailureAndEntersSessionUndo() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), expression = UUID().uuidString
        let (editor, layer) = try await makeEditor(fake, folder: folder,
            expressions: [["id": expression, "version": 1, "value_type": "scalar", "nodes": []]],
            nodeProperties: [expressionProperty(source: ["kind": "expression", "value": expression])])
        let prop = layer.property("kronello.opacity")!
        fake.nextError = .init(code: "EXPRESSION_SYNTAX", message: "1:1: bad token")
        editor.commitExpressionText(layer, property: prop, text: "?")
        let rejected = await waitForError(editor, layer, prop)
        XCTAssertTrue(rejected)
        fake.nextError = nil
        editor.commitExpressionText(layer, property: prop, text: "2")
        let retried = await applyCommands(fake)
        XCTAssertNotNil(retried)
        for _ in 0..<400 where editor.expressionError(layer, prop) != nil { try? await Task.sleep(for: .milliseconds(5)) }
        XCTAssertNil(editor.expressionError(layer, prop), "a successful commit clears the property failure")
        XCTAssertEqual(editor.undoState.undo.count, 1, "the commit enters session undo like every other edit")
        await editor.undo()
        XCTAssertTrue(fake.requests.contains { $0.string("operation") == "edit.undo" }, "undo flows through the shared edit.undo")
        await editor.close()
    }

    func testMissingExpressionAndUnsupportedTypeBlockBeforePlanning() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), missing = UUID().uuidString
        let (editor, layer) = try await makeEditor(fake, folder: folder,
            nodeProperties: [expressionProperty(source: ["kind": "expression", "value": missing]),
                             expressionProperty(source: ["kind": "curve", "value": UUID().uuidString], key: "kronello.transform.rotation")])
        let prop = layer.property("kronello.opacity")!, broken = layer.property("kronello.transform.rotation")!
        editor.commitExpressionText(layer, property: prop, text: "1")
        let missingSurfaced = await waitForError(editor, layer, prop)
        XCTAssertTrue(missingSurfaced)
        XCTAssertEqual(editor.expressionError(layer, prop)?.code, "EXPRESSION_NOT_FOUND")
        editor.commitExpressionText(layer, property: broken, text: "0")
        let brokenSurfaced = await waitForError(editor, layer, broken)
        XCTAssertTrue(brokenSurfaced)
        XCTAssertEqual(editor.expressionError(layer, broken)?.code, "UNSUPPORTED_FEATURE")
        XCTAssertFalse(fake.requests.contains { $0.string("operation") == "edit.plan" }, "envelope failures send no request")
        await editor.close()
    }

    func testDisplayDefaultPropertyIsInsertedBeforeExpression() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport()
        let (editor, layer) = try await makeEditor(fake, folder: folder, nodeProperties: [])
        let prop = try XCTUnwrap(editor.transformProperty(layer, key: "kronello.transform.rotation"))
        XCTAssertTrue(prop.string("id").isEmpty)
        let commands = try editor.expressionTextCommands(layer, property: prop, text: "time() * 45")
        XCTAssertEqual(commands.count, 2)
        let insert = commands[0].object("node_property_insert")
        XCTAssertEqual(insert.string("composition"), editor.current.string("id"))
        XCTAssertEqual(insert.string("node"), layer.id)
        let insertedID = insert.object("property").string("id")
        XCTAssertFalse(insertedID.isEmpty)
        let command = commands[1].object("property_expression_text_set")
        XCTAssertEqual(command.string("property"), insertedID, "the expression attaches to the inserted Property")
        XCTAssertEqual(command.object("metadata").string("value_type"), "angle")
        XCTAssertEqual(command.object("metadata")["version"] as? Int, 3)
        await editor.close()
    }

    func testAttachSeedsCurrentValueAsLiteral() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport()
        let value: [String: Any] = ["kind": "scalar", "value": 0.5]
        let (editor, layer) = try await makeEditor(fake, folder: folder,
            nodeProperties: [expressionProperty(source: ["kind": "constant", "value": value])])
        let prop = layer.property("kronello.opacity")!
        editor.attachExpression(layer, property: prop)
        let attachCommands = await applyCommands(fake)
        let commands = try XCTUnwrap(attachCommands)
        let command = commands[0].object("property_expression_text_set")
        let text = command.string("text")
        XCTAssertTrue(text.hasPrefix("literal(") && text.hasSuffix(")"), "attach preserves the current value as a literal seed")
        let inner = String(text.dropFirst("literal(".count).dropLast())
        let embedded = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(inner.utf8), options: [.fragmentsAllowed]) as? String)
        let decoded = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(embedded.utf8)) as? [String: Any])
        XCTAssertEqual(NSDictionary(dictionary: decoded), NSDictionary(dictionary: value))
        XCTAssertEqual(command.object("metadata").string("value_type"), "scalar")
        await editor.close()
    }

    func testDetachPinsEvaluatedValueAsConstant() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), expression = UUID().uuidString, property = UUID().uuidString
        let evaluated: [String: Any] = ["properties": [property: ["kind": "scalar", "value": 0.75]]]
        let (editor, layer) = try await makeEditor(fake, folder: folder,
            expressions: [["id": expression, "version": 1, "value_type": "scalar", "nodes": []]],
            nodeProperties: [expressionProperty(id: property, source: ["kind": "expression", "value": expression])],
            evaluated: evaluated)
        let prop = layer.property("kronello.opacity")!
        editor.detachExpression(layer, property: prop)
        let detachCommands = await applyCommands(fake)
        let commands = try XCTUnwrap(detachCommands)
        let source = commands[0].object("property_source_set").object("source")
        XCTAssertEqual(source.string("kind"), "constant")
        XCTAssertEqual(NSDictionary(dictionary: source.object("value")), NSDictionary(dictionary: ["kind": "scalar", "value": 0.75]))
        await editor.close()
    }

    /// End-to-end against the real worker. Requires the regenerated
    /// GeneratedAPI surface (Request variant `expression.format`, EditCommand
    /// variant `property_expression_text_set`, ExpressionMetadata); with a stale
    /// generated file the transport rejects both at envelope validation.
    func testExpressionTextRoundTripsThroughSharedService() async throws {
        let checks = GUIChecks(), folder = try checks.temporary()
        defer { try? FileManager.default.removeItem(at: folder) }
        let path = folder.appendingPathComponent("expression.kronello").path
        let transport = try RecordingTransport(path: path, worker: checks.root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let editor = EditorModel(path: path, transport: transport, stateStore: .init(root: folder.appendingPathComponent("state")))
        try await editor.start(newDocument: EditorModel.newDocument(name: "Expression checks"))
        let creation = try editor.creationCommands(tool: "rectangle", from: .init(x: 100, y: 100), to: .init(x: 300, y: 220))
        guard await editor.apply(.init(base: editor.revision, commands: creation, label: "Fixture")) != nil else {
            throw GUICheckError(message: editor.failure?.message ?? "fixture failed")
        }
        var base = editor.revision
        editor.attachExpression(editor.layers[0], property: editor.layers[0].property("kronello.opacity")!)
        try await MotionChecks().waitForEdit(editor, after: base)
        var prop = editor.layers[0].property("kronello.opacity")!
        XCTAssertEqual(prop.object("source").string("kind"), "expression", "attach makes the source an expression")
        let expressionID = prop.object("source")["value"] as? String
        let stored = try XCTUnwrap(editor.document.objects("expressions").first { $0.string("id") == expressionID })
        XCTAssertEqual(stored.string("value_type"), "scalar")
        XCTAssertEqual(stored["version"] as? Int, 3, "minted expressions use the supported semantics version")
        // Canonical text fetches by id and recommits verbatim (parse∘format round trip).
        let fetched = await editor.expressionText(editor.layers[0], property: prop)
        let formatted = try XCTUnwrap(fetched, "expression.format returns text")
        XCTAssertFalse(formatted.isEmpty)
        base = editor.revision
        editor.commitExpressionText(editor.layers[0], property: prop, text: formatted, base: base)
        try await MotionChecks().waitForEdit(editor, after: base)
        prop = editor.layers[0].property("kronello.opacity")!
        XCTAssertEqual(prop.object("source")["value"] as? String, expressionID, "recommit preserves the expression identity")
        // Rejected text keeps the stored AST and surfaces typed diagnostics.
        let storedNodes = editor.document.objects("expressions").first { $0.string("id") == expressionID }!.objects("nodes")
        editor.commitExpressionText(editor.layers[0], property: prop, text: "1 + ")
        let invalidSurfaced = await waitForError(editor, editor.layers[0], prop)
        XCTAssertTrue(invalidSurfaced, "typed failure reaches the property")
        XCTAssertEqual(editor.expressionError(editor.layers[0], prop)?.code, "EXPRESSION_SYNTAX")
        XCTAssertFalse(editor.expressionDiagnostics(editor.layers[0], prop).isEmpty, "diagnostics carry line/column/expected")
        XCTAssertEqual(editor.document.objects("expressions").first { $0.string("id") == expressionID }?.objects("nodes").count,
                       storedNodes.count, "invalid text never mutates the stored expression")
        XCTAssertNil(editor.pendingCandidate)
        editor.failure = nil
        // Detach pins the evaluated scalar and removes the expression source.
        base = editor.revision
        editor.detachExpression(editor.layers[0], property: prop)
        try await MotionChecks().waitForEdit(editor, after: base)
        XCTAssertEqual(editor.layers[0].property("kronello.opacity")!.object("source").string("kind"), "constant")
        await editor.close()
    }
}
