import XCTest

@MainActor final class QAEditorTests: XCTestCase {
    func testScriptedGUIEquivalence() async throws { try await QAChecks().verifyEquivalence() }
    func testMarkedTextCommitsOnceAndEscapeCancels() async throws { try await QAChecks().verifyIME() }
}
