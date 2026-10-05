import XCTest

@MainActor final class MotionEditorTests: XCTestCase {
    func testNavigatorAddRemove() async throws { try await MotionChecks().verifyNavigatorAddRemove() }
    func testKeyMove() async throws { try await MotionChecks().verifyKeyMove() }
    func testMultiKeyMove() async throws { try await MotionChecks().verifyMultiKeyMove() }
    func testAlignedTangentEdit() async throws { try await MotionChecks().verifyTangent(aligned: true) }
    func testBrokenTangentEdit() async throws { try await MotionChecks().verifyTangent(aligned: false) }
    func testLastKeyDelete() async throws { try await MotionChecks().verifyLastKeyDelete() }
    func testSharedPropertyLastKeyDelete() async throws { try await MotionChecks().verifySharedLastKeyDelete(expression: false) }
    func testExpressionConsumerLastKeyDelete() async throws { try await MotionChecks().verifySharedLastKeyDelete(expression: true) }
    func testMoveUndoCLIParity() async throws { try await MotionChecks().verifyMoveUndoCLIParity() }
    func testSelectionSnapAndConflict() async throws { try await MotionChecks().verifySelectionSnapAndConflict() }
    func testSpatialTemporalSeparation() async throws { try await MotionChecks().verifySpatialTemporalSeparation() }
}
