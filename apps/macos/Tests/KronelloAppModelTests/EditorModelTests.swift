import XCTest

@MainActor final class EditorModelTests: XCTestCase {
    func testSelectionReloadAndDeletion() async throws { try await GUIChecks().verifySelectionReloadAndDeletion() }
    func testSessionUndoAndRedo() async throws { try await GUIChecks().verifySessionUndoAndRedo() }
    func testConflictMappingAndExplicitRetry() async throws { try await GUIChecks().verifyConflictMappingAndExplicitRetry() }
    func testStateIsolationAndRationalTime() async throws { try await GUIChecks().verifyStateIsolationAndRationalTime() }
    func testLegacyStateDecoding() async throws { try await GUIChecks().verifyLegacyStateDecoding() }
    func testNumberCommitOnce() async throws { try await GUIChecks().verifyNumberCommitOnce() }
    func testCanvasDefaultsAndLock() async throws { try await GUIChecks().verifyCanvasDefaultsAndLock() }
    func testGUIEditCLIEventAndNotification() async throws { try await GUIChecks().verifyGUIEditCLIEventAndNotification() }
}
