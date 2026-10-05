import XCTest

@MainActor final class IntegrationTests: XCTestCase {
    func testInspectionScheduling() async throws { try await IntegrationChecks().verifyScheduling() }
    func testStage2FFIPresentationParity() async throws {
        guard let path = ProcessInfo.processInfo.environment["KRONELLO_INTEGRATION_EVIDENCE"] else {
            throw XCTSkip("Set KRONELLO_INTEGRATION_EVIDENCE to stage-2 gui-evidence.json")
        }
        try await IntegrationChecks().verifyEvidence(URL(fileURLWithPath: path))
    }
}
