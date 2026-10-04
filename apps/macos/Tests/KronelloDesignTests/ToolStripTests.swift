import KronelloDesign
import XCTest

final class ToolStripTests: XCTestCase {
    func testPlacementCodableRoundTrip() throws {
        let locations: [KRToolStripPlacement.Location] = [.viewerLeft, .viewerRight, .floating(x: -12.25, y: 180.5)]
        for location in locations {
            for collapsed in [false, true] {
                let value = KRToolStripPlacement(location, collapsed: collapsed)
                let data = try JSONEncoder().encode(value)
                XCTAssertEqual(try JSONDecoder().decode(KRToolStripPlacement.self, from: data), value)
            }
        }
    }

    func testDefaultPlacementIsLeftAndExpanded() {
        XCTAssertEqual(KRToolStripPlacement(), .init(.viewerLeft, collapsed: false))
    }
}
