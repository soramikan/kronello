import XCTest

@MainActor final class MediaFlowTests: XCTestCase {
    func testMediaImportUsesSharedRevisionsAndUndo() async throws { try await MediaFlowChecks().verifyMediaImport() }
    func testMediaBrowserBinsAndOffline() async throws { try await MediaFlowChecks().verifyMediaBrowserBinsAndOffline() }
    func testThumbnailCacheAndFailures() async throws { try await MediaFlowChecks().verifyThumbnailCacheAndFailures() }
    func testPresetsAndBatchQueue() async throws { try await MediaFlowChecks().verifyPresetsAndBatchQueue() }
}
