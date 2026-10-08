import XCTest
import KronelloAppModel
import KronelloCore

final class FlowShortcutTests: XCTestCase {
    private func suite(_ name: String = UUID().uuidString) throws -> UserDefaults {
        let suite = "kronello-flow-" + name
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defaults.removePersistentDomain(forName: suite)
        return defaults
    }
    func testBindingFallsBackToDefaultAndCanonicalizesModifiers() throws {
        let settings = WorkflowSettings(defaults: try suite())
        // Unset actions report the built-in default and store nothing.
        XCTAssertEqual(settings.binding(for: .undo), KeyBinding(key: "z", modifiers: ["command"]))
        XCTAssertEqual(settings.binding(for: .transportPlay), KeyBinding(key: "space"))
        XCTAssertTrue(settings.shortcuts.isEmpty)
        // Modifier input order is canonicalized for storage and display.
        settings.setBinding(KeyBinding(key: "u", modifiers: ["command", "shift", "command"]), for: .undo)
        XCTAssertEqual(settings.binding(for: .undo).modifiers, ["shift", "command"])
        XCTAssertEqual(settings.shortcuts.count, 1)
    }
    func testBindingOverridesPersistAcrossInstancesAndResetToDefault() throws {
        let defaults = try suite()
        WorkflowSettings(defaults: defaults).setBinding(KeyBinding(key: "k", modifiers: ["option"]), for: .transportPlay)
        // A fresh instance on the same suite simulates a relaunch.
        let reloaded = WorkflowSettings(defaults: defaults)
        XCTAssertEqual(reloaded.binding(for: .transportPlay), KeyBinding(key: "k", modifiers: ["option"]))
        // Rebinding to the default clears the stored override, not the action.
        reloaded.setBinding(ShortcutAction.transportPlay.defaultBinding, for: .transportPlay)
        XCTAssertTrue(reloaded.shortcuts.isEmpty)
        reloaded.setBinding(KeyBinding(key: "j"), for: .editToolBlade)
        reloaded.resetShortcuts()
        XCTAssertEqual(WorkflowSettings(defaults: defaults).binding(for: .editToolBlade), KeyBinding(key: "b"))
    }
    func testCorruptShortcutDataFallsBackToDefaults() throws {
        let defaults = try suite()
        defaults.set(Data("not-json".utf8), forKey: WorkflowSettings.shortcutsKey)
        let settings = WorkflowSettings(defaults: defaults)
        XCTAssertEqual(settings.binding(for: .undo), ShortcutAction.undo.defaultBinding)
        XCTAssertTrue(settings.shortcuts.isEmpty)
    }
    func testPageLayoutPersistsPerPageAndResets() throws {
        let defaults = try suite()
        let settings = WorkflowSettings(defaults: defaults)
        XCTAssertEqual(settings.layout(for: "motion"), .standard(for: "motion"))
        XCTAssertEqual(settings.layout(for: "edit"), .standard(for: "edit"))
        var edit = settings.layout(for: "edit")
        edit.trailingPanel = false
        edit.leadingWidth = 320
        edit.bottomHeight = 360
        settings.setLayout(edit, for: "edit")
        let reloaded = WorkflowSettings(defaults: defaults)
        XCTAssertFalse(reloaded.layout(for: "edit").trailingPanel)
        XCTAssertEqual(reloaded.layout(for: "edit").leadingWidth, 320)
        XCTAssertEqual(reloaded.layout(for: "edit").bottomHeight, 360)
        // Other pages keep their own standard layout.
        XCTAssertEqual(reloaded.layout(for: "motion"), .standard(for: "motion"))
        reloaded.resetLayout(for: "edit")
        XCTAssertEqual(WorkflowSettings(defaults: defaults).layout(for: "edit"), .standard(for: "edit"))
    }
}

@MainActor final class FlowHistoryTests: XCTestCase {
    func checks() -> GUIChecks { GUIChecks() }
    func testHistoryPanelLoadsNewestFirstAndLabelsEntries() async throws {
        let folder = try checks().temporary()
        defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport()
        let editor = checks().model(fake, folder: folder)
        try await editor.start()
        fake.history = [
            ["event": ["id": "e1", "revision": 1, "session_id": "cli-worker-1",
                       "mutations": [["operation": "node_add"]]]],
            ["event": ["id": "e2", "revision": 2, "session_id": editor.sessionID,
                       "mutations": [["operation": "property_source_set"]]]],
            ["event": ["id": "e3", "revision": 3, "session_id": "cli-worker-2",
                       "mutations": [], "undo_of": "e9"], "undone": true],
        ]
        try await editor.loadHistory()
        // Newest first; undone flag, undo marker and own-session flag surface.
        XCTAssertEqual(editor.historyPanel.map(\.id), ["e3", "e2", "e1"])
        XCTAssertTrue(editor.historyPanel[0].isUndo)
        XCTAssertTrue(editor.historyPanel[0].undone)
        XCTAssertEqual(editor.historyPanel[0].label, "取り消し")
        XCTAssertTrue(editor.historyPanel[1].own)
        XCTAssertEqual(editor.historyPanel[1].label, "property source set")
        XCTAssertFalse(editor.historyPanel[2].undone)
        // Observation time is stamped once and survives later reloads.
        let stamped = editor.historyRecordedAt["e1"]
        XCTAssertNotNil(stamped)
    }
    func testSelectiveUndoCallsSharedEditUndoAndRefreshesPanel() async throws {
        let folder = try checks().temporary()
        defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport()
        let editor = checks().model(fake, folder: folder)
        try await editor.start()
        fake.history = [
            ["event": ["id": "e1", "revision": 1, "session_id": "cli-worker-1",
                       "mutations": [["operation": "node_add"]]]],
            ["event": ["id": "e2", "revision": 2, "session_id": editor.sessionID,
                       "mutations": [["operation": "node_remove"]]]],
        ]
        try await editor.loadHistory()
        // The service marks e1 undone after the inverse lands.
        fake.history[0]["undone"] = true
        await editor.undoEvent("e1")
        let request = fake.requests.last { $0.string("operation") == "edit.undo" }
        XCTAssertEqual(request?.string("event_id"), "e1")
        XCTAssertEqual(request?.string("base_revision"), "1")
        XCTAssertEqual(request?.string("session_id"), editor.sessionID)
        XCTAssertFalse(request?.string("idempotency_key").isEmpty ?? true)
        // Inverse event lands on the redo stack; Redo re-applies e1's change.
        XCTAssertEqual(editor.undoState.redo.count, 1)
        XCTAssertTrue(editor.canRedo)
        // The panel re-read history and now shows e1 as undone.
        XCTAssertTrue(editor.historyPanel.first { $0.id == "e1" }?.undone ?? false)
    }
    func testSelectiveUndoFailureSurfacesTypedError() async throws {
        let folder = try checks().temporary()
        defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport()
        let editor = checks().model(fake, folder: folder)
        try await editor.start()
        fake.nextError = ServiceFailure(code: "UNDO_CONFLICT", message: "newer events touch the same objects")
        await editor.undoEvent("missing")
        XCTAssertEqual(editor.undoConflict?.code, "UNDO_CONFLICT")
    }
}
