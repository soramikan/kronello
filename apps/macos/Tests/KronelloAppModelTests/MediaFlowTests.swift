import XCTest

@MainActor final class MediaFlowTests: XCTestCase {
    func testMediaBrowserBinsAndOffline() async throws { try await MediaFlowChecks().verifyMediaBrowserBinsAndOffline() }
    func testThumbnailCacheAndFailures() async throws { try await MediaFlowChecks().verifyThumbnailCacheAndFailures() }
    func testPresetsAndBatchQueue() async throws { try await MediaFlowChecks().verifyPresetsAndBatchQueue() }
}
