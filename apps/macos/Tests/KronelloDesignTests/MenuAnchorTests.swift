@testable import KronelloDesign
import AppKit
import XCTest

final class MenuAnchorTests: XCTestCase {
    @MainActor
    func testUpdatesReportEachBackingViewOnlyOnce() async {
        let coordinator = KRAnchorTracker.Coordinator()
        let view = KRPassthroughView()
        let replacement = KRPassthroughView()
        var reports: [NSView] = []
        let delivered = expectation(description: "Both distinct anchors delivered")
        delivered.expectedFulfillmentCount = 2
        for anchor in [view, view, view, replacement, replacement] {
            coordinator.report(anchor) { reported in
                reports.append(reported)
                delivered.fulfill()
            }
        }
        await fulfillment(of: [delivered], timeout: 2)
        XCTAssertEqual(reports.count, 2)
        XCTAssertTrue(reports[0] === view)
        XCTAssertTrue(reports[1] === replacement)
    }
}
