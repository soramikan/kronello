import XCTest
@testable import KronelloAppModel

final class WaveformTests: XCTestCase {
    @MainActor func testAnalyzeCacheAndResample() async throws { try await WaveformChecks().verifyAnalyzeCacheAndResample() }
    @MainActor func testCacheSurvivesReload() async throws { try await WaveformChecks().verifyCacheSurvivesReload() }
}
