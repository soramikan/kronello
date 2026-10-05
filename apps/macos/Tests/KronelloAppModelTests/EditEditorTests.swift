import XCTest
@testable import KronelloAppModel

final class EditEditorTests: XCTestCase {
    @MainActor func testPlaceReleaseOnce() async throws { try await EditChecks().verifyPlaceReleaseOnce() }
    @MainActor func testTrimReleaseOnce() async throws { try await EditChecks().verifyTrimReleaseOnce() }
    @MainActor func testBladeReleaseOnce() async throws { try await EditChecks().verifyBladeReleaseOnce() }
    @MainActor func testRevisionConflict() async throws { try await EditChecks().verifyRevisionConflict() }
    @MainActor func testUndoConflict() async throws { try await EditChecks().verifyUndoConflict() }
    @MainActor func testExternalClipDeletion() async throws { try await EditChecks().verifyExternalClipDeletion() }
    @MainActor func testMotionNavigation() async throws { try await EditChecks().verifyMotionNavigation() }
    @MainActor func testBatchedQueryAndSeek() async throws { try await EditChecks().verifyBatchedQueryAndSeek() }
    @MainActor func testProjectInventoryAndMissing() async throws { try await EditChecks().verifyProjectInventoryAndMissing() }
}
