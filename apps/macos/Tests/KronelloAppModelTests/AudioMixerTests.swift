import XCTest
@testable import KronelloAppModel

final class AudioMixerTests: XCTestCase {
    @MainActor func testMeteredRender() async throws { try await AudioMixerChecks().verifyMeteredRender() }
    @MainActor func testScrubPublishesMetersAndStops() async throws { try await AudioMixerChecks().verifyScrubPublishesMetersAndStops() }
    @MainActor func testTrackVolumeSharedEdit() async throws { try await AudioMixerChecks().verifyTrackVolumeSharedEdit() }
}
