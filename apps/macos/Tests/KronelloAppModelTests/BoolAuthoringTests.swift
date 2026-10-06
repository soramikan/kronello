import XCTest
import KronelloAppModel

@MainActor final class BoolAuthoringTests: XCTestCase {
    func testBoolUsesSharedTypedEditUndoAndOriginRevision() async throws {
        let checks = MotionChecks(), fixture = try await checks.fixture()
        defer { Task { await checks.finish(fixture) } }
        let editor = fixture.editor, id = UUID().uuidString.lowercased()
        let property: [String: Any] = ["id": id, "descriptor": ["key": "kronello.simulation.enabled", "version": 1],
            "source": ["kind": "constant", "value": ["kind": "bool", "value": true]], "modifiers": []]
        let command: [String: Any] = ["node_property_insert": ["composition": editor.current.string("id"), "node": fixture.node, "property": property]]
        guard await editor.apply(.init(base: editor.revision, commands: [command], label: "Bool fixture")) != nil else { throw GUICheckError(message: editor.failure?.message ?? "insert failed") }
        let origin = editor.revision, layer = editor.layers.first { $0.id == fixture.node }!
        XCTAssertTrue(editor.propertyNumbers(layer, property).isEmpty)
        editor.setBool(layer, property: property, to: false, base: origin)
        try await checks.waitForEdit(editor, after: origin)
        let changed = editor.layers.first { $0.id == fixture.node }!.properties.first { $0.string("id") == id }!
        XCTAssertEqual(changed.object("source").object("value").string("kind"), "bool")
        XCTAssertEqual(changed.object("source").object("value")["value"] as? Bool, false)
        await editor.undo()
        let restored = editor.layers.first { $0.id == fixture.node }!.properties.first { $0.string("id") == id }!
        XCTAssertEqual(restored.object("source").object("value")["value"] as? Bool, true)
        let applies = fixture.transport.applyCount
        editor.setBool(layer, property: property, to: false, base: origin)
        for _ in 0..<400 {
            if editor.pendingCandidate != nil && !editor.busy { break }
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertEqual(editor.revisionConflict?.code, "REVISION_CONFLICT")
        XCTAssertEqual(editor.pendingCandidate?.base, origin)
        XCTAssertEqual(fixture.transport.applyCount, applies)
    }
}
