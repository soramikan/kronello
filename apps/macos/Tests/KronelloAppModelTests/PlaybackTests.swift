import XCTest

@MainActor final class PlaybackTests: XCTestCase {
    func testBinaryProducerAndHostFallback() async throws { try await PlaybackChecks().verifyBinaryProducerAndHostFallback() }
    func testPresentationTickDoesNotQueryService() async throws { try await PlaybackChecks().verifyPresentationTickDoesNotQueryService() }
    func testSequenceConfiguration() throws { try PlaybackChecks().verifySequenceConfiguration() }
    func testCompositionGeometrySurvivesPageTransitions() throws { try PlaybackChecks().verifyCompositionGeometrySurvivesPageTransitions() }
}
