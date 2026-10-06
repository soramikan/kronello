import KronelloDesign
import XCTest

final class TokensTests: XCTestCase {
    func testDarkIsTheDefaultTheme() {
        XCTAssertEqual(EnvironmentValuesProbe.defaultTheme, .dark)
    }

    func testEveryIconHasAPath() {
        for icon in KRIcon.allCases {
            XCTAssertFalse(icon.path.isEmpty, "\(icon.rawValue) has an empty path")
        }
    }

    func testLengthsMatchTokens() {
        XCTAssertEqual(KRSize.controlHeight, 22)
        XCTAssertEqual(KRSize.rowHeight, 24)
        XCTAssertEqual(KRRadius.radiusSm, 3)
        XCTAssertEqual(KRSpace.space3, 12)
    }
}

import SwiftUI

enum EnvironmentValuesProbe {
    static var defaultTheme: KRTheme { EnvironmentValues().krTheme }
}
