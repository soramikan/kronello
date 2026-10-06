import XCTest
@MainActor final class WorkflowEditorTests: XCTestCase {
    func testTypedErrorGateAndStaleInspection() async throws { try await WorkflowChecks().verifyTypedErrorGateAndStaleInspection() }
    func testJobSubmissionProgressAndPolling() async throws { try await WorkflowChecks().verifyJobSubmissionProgressAndPolling() }
    func testTemplateInputOneCommand() async throws { try await WorkflowChecks().verifyTemplateInputOneCommand() }
    func testMigrationExplicitApplyAndPolicyPublication() async throws { try await WorkflowChecks().verifyMigrationExplicitApplyAndPolicyPublication() }
    func testNativeTemplateAndExportInspection() async throws { try await WorkflowChecks().verifyNativeTemplateAndExportInspection() }
}
