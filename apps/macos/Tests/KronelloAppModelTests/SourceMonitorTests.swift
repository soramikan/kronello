import XCTest
@testable import KronelloAppModel

final class SourceMonitorTests: XCTestCase {
    @MainActor func testSourcePreviewWire() throws { try SourceMonitorChecks().verifySourcePreviewWire() }
    @MainActor func testSourceOpenMarkingAndWindow() async throws { try await SourceMonitorChecks().verifySourceOpenMarkingAndWindow() }
    @MainActor func testInsertOverwriteThreePoint() async throws { try await SourceMonitorChecks().verifyInsertOverwriteThreePoint() }
    @MainActor func testMulticamCreatePreviewAndSwitch() async throws { try await SourceMonitorChecks().verifyMulticamCreatePreviewAndSwitch() }
}
