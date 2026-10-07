import XCTest
@testable import KronelloAppModel

final class PrecisionEditTests: XCTestCase {
    @MainActor func testSlipReleaseOnce() async throws { try await PrecisionEditChecks().verifySlipReleaseOnce() }
    @MainActor func testSlideReleaseOnce() async throws { try await PrecisionEditChecks().verifySlideReleaseOnce() }
    @MainActor func testRollReleaseOnce() async throws { try await PrecisionEditChecks().verifyRollReleaseOnce() }
    @MainActor func testDeleteAndRippleDelete() async throws { try await PrecisionEditChecks().verifyDeleteAndRippleDelete() }
    @MainActor func testMarkers() async throws { try await PrecisionEditChecks().verifyMarkers() }
    @MainActor func testWorkArea() async throws { try await PrecisionEditChecks().verifyWorkArea() }
    @MainActor func testSnapping() async throws { try await PrecisionEditChecks().verifySnapping() }
    @MainActor func testBoundaryJump() async throws { try await PrecisionEditChecks().verifyBoundaryJump() }
}
