import KronelloDesign
import XCTest

final class TimecodeTests: XCTestCase {
    func testFullAndAbbreviatedInput() throws {
        XCTAssertEqual(try KRTimecode.parse("01:02:03:12", fps: 24), 89364)
        XCTAssertEqual(try KRTimecode.parse("1:12", fps: 24), 36)
        XCTAssertEqual(try KRTimecode.parse("2:03:12", fps: 24), 2964)
        XCTAssertEqual(try KRTimecode.parse("12", fps: 24), 12)
        XCTAssertEqual(try KRTimecode.parse(" 00:01:12 \n", fps: 24), 36)
    }

    func testFieldsAndSyntaxAreStrict() {
        for value in ["", "1:", ":12", "1::12", "1:2:3:4:5", "-1", "1.5", "+12", "１:１２", "1 :12", "00:60:00:00", "00:00:60:00", "00:00:00:24"] {
            XCTAssertThrowsError(try KRTimecode.parse(value, fps: 24), value)
        }
        XCTAssertThrowsError(try KRTimecode.parse("0", fps: 0)) { XCTAssertEqual($0 as? KRTimecode.ParseError, .invalidRate) }
    }

    func testOverflowIsRejectedInsteadOfWrapping() {
        XCTAssertThrowsError(try KRTimecode.parse("9223372036854775807:00:00:00", fps: 24)) { XCTAssertEqual($0 as? KRTimecode.ParseError, .overflow) }
        XCTAssertThrowsError(try KRTimecode.parse("99999999999999999999:00:00:00", fps: 24)) { XCTAssertEqual($0 as? KRTimecode.ParseError, .overflow) }
    }

    func testRoundTripAtSeveralNominalFrameRates() throws {
        for rate in [24, 25, 30, 60, 120] {
            for frames: Int64 in [0, 1, Int64(rate - 1), Int64(rate), 2016, 99999, 8640000] {
                XCTAssertEqual(try KRTimecode.parse(KRTimecode.format(frames: frames, fps: rate), fps: rate), frames)
            }
        }
        XCTAssertEqual(KRTimecode.format(frames: 2016, fps: 24), "00:01:24:00")
    }
}
