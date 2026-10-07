import XCTest
import KronelloAppModel
import KronelloCore

@MainActor final class ControlDraftTests: XCTestCase {
    private func external(_ editor: EditorModel, commands: [[String: Any]]) throws {
        let checks = GUIChecks()
        let plan = try checks.cli(["operation": "edit.plan", "project": editor.path, "base_revision": editor.revision, "commands": commands])
        _ = try checks.cli(["operation": "edit.apply", "project": editor.path, "base_revision": editor.revision, "commands": commands,
                            "plan_hash": plan.string("plan_hash"), "session_id": UUID().uuidString, "idempotency_key": UUID().uuidString])
    }
    private func conflict(_ editor: EditorModel, origin: String, current: String) async throws {
        for _ in 0..<400 {
            if editor.pendingCandidate != nil && !editor.busy { break }
            if let failure = editor.failure { throw failure }
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertEqual(editor.pendingCandidate?.base, origin)
        XCTAssertEqual(editor.revisionConflict?.code, "REVISION_CONFLICT")
        XCTAssertEqual(editor.revision, current)
        XCTAssertTrue(editor.undoState.undo.isEmpty)
    }
    func testClipNumericAndMenuDraftsFenceExternalRevisionAndKeepExplicitRetry() async throws {
        for control in ["source", "speed", "opacity", "blend"] {
            let checks = EditChecks(), f = try await checks.fixture(), editor = f.editor
            editor.selectClip(f.clip)
            let origin = editor.revision, clip = editor.selectedClip!, applies = f.transport.applyCount
            try checks.externalMove(f)
            try await editor.reload(external: true)
            let externalRevision = editor.revision, externalSequence = editor.sequence
            switch control {
            case "source": editor.setClipTime(clip, sourceIn: .init(num: 1, den: 24), base: origin)
            case "speed": editor.setClipTime(clip, speedPercent: 50, base: origin)
            case "opacity": editor.setClipProperty(clip, key: "kronello.opacity", kind: "scalar", value: 0.5, base: origin)
            default: editor.setClipProperty(clip, key: "kronello.blend_mode", kind: "enum", value: "screen", base: origin)
            }
            try await conflict(editor, origin: origin, current: externalRevision)
            XCTAssertEqual(f.transport.applyCount, applies)
            XCTAssertEqual(NSDictionary(dictionary: editor.sequence), NSDictionary(dictionary: externalSequence))
            XCTAssertFalse(editor.pendingCandidate?.commands.isEmpty ?? true)
            editor.reapply()
            try await MotionChecks().waitForEdit(editor, after: externalRevision)
            XCTAssertNil(editor.pendingCandidate)
            XCTAssertEqual(f.transport.applyCount, applies + 1)
            await editor.undo()
            XCTAssertEqual(NSDictionary(dictionary: editor.sequence), NSDictionary(dictionary: externalSequence))
            try checks.parity(f)
            await checks.finish(f)
        }
    }
    func testTextStyleAndColorDraftsFenceExternalRevisionWithoutWrites() async throws {
        let qa = QAChecks(), (folder, editor, transport) = try await qa.fixture()
        defer { Task { await editor.close(); try? FileManager.default.removeItem(at: folder) } }
        for control in ["text", "size", "span_color", "font", "property_color"] {
            let layer = editor.layers.first { $0.kind == "text" }!
            let origin = editor.revision, applies = transport.applyCount
            try external(editor, commands: [["node_rename": ["composition": editor.current.string("id"), "node": layer.id, "name": "External " + control]]])
            try await editor.reload(external: true)
            let externalRevision = editor.revision, document = editor.document
            switch control {
            case "text": editor.setText(layer, to: editor.textDocument(layer)!.string("text") + "日", base: origin)
            case "size": editor.setSpanSize(layer, start: 0, end: 1, size: 42, base: origin)
            case "span_color": editor.setSpanColor(layer, start: 0, end: 1, hex: "#44CC88FF", base: origin)
            case "font": editor.setSpanFont(layer, start: 0, end: 1, font: editor.lockedFonts[0], base: origin)
            default:
                let fill = editor.textDocument(layer)!.objects("styles")[0].string("fill")
                let property = layer.properties.first { $0.string("id") == fill }!
                editor.setColor(layer, property: property, hex: "#44CC88FF", base: origin, time: editor.ui.time)
            }
            try await conflict(editor, origin: origin, current: externalRevision)
            XCTAssertEqual(transport.applyCount, applies)
            XCTAssertEqual(NSDictionary(dictionary: editor.document), NSDictionary(dictionary: document))
            editor.discardCandidate()
            XCTAssertNil(editor.pendingCandidate)
        }
    }
}
