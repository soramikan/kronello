import XCTest

@MainActor final class KronelloCoreTests: XCTestCase {
    func testSharedRevisionEventAndCLIIdempotency() async throws {
        try await CoreChecks().verifySharedRevisionEventAndCLIIdempotency()
    }
    func testGeneratedTypesPreserveUnknownProjectAndRationalStrings() throws {
        try CoreChecks().verifyGeneratedTypesPreserveUnknownProjectAndRationalStrings()
    }
    func testRawDuplicateRejectionAndClosedHandle() async throws {
        try await CoreChecks().verifyRawDuplicateRejectionAndClosedHandle()
    }
    func testRawArbitraryPrecisionResponse() async throws {
        try await CoreChecks().verifyRawArbitraryPrecisionResponse()
    }
}
