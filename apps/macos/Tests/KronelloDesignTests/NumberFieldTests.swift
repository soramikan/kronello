import KronelloDesign
import XCTest

final class NumberFieldTests: XCTestCase {
    func testHorizontalThresholdIsThreePixels() {
        var edit = KRNumberEdit(value: 10, step: 2)
        edit.beginDrag()
        XCTAssertNil(edit.drag(horizontal: 2.99))
        XCTAssertEqual(edit.phase, .armed)
        XCTAssertEqual(edit.drag(horizontal: 3), 16)
        XCTAssertEqual(edit.phase, .scrubbing)
    }

    func testVerticalDragCannotStartScrubbing() {
        var edit = KRNumberEdit(value: 10)
        edit.beginDrag()
        XCTAssertNil(edit.drag(horizontal: 0))
        XCTAssertNil(edit.finish())
    }

    func testModifiersAndNegativeMovement() {
        XCTAssertEqual(KRNumberEdit.multiplier(shift: false, option: false), 1)
        XCTAssertEqual(KRNumberEdit.multiplier(shift: true, option: false), 10)
        XCTAssertEqual(KRNumberEdit.multiplier(shift: false, option: true), 0.1)
        XCTAssertEqual(KRNumberEdit.multiplier(shift: true, option: true), 1)
        var edit = KRNumberEdit(value: 20, step: 2)
        edit.beginDrag()
        XCTAssertEqual(edit.drag(horizontal: -4, shift: true), -60)
        XCTAssertEqual(edit.drag(horizontal: -4, option: true), 19.2)
    }

    func testDragAlwaysUsesTheOriginalValue() {
        var edit = KRNumberEdit(value: 10, step: 0.5)
        edit.beginDrag()
        XCTAssertEqual(edit.drag(horizontal: 4), 12)
        XCTAssertEqual(edit.drag(horizontal: 8), 14)
        XCTAssertEqual(edit.drag(horizontal: 0), 10)
        XCTAssertNil(edit.finish())
    }

    func testClampingAndKeyboardSteps() {
        var edit = KRNumberEdit(value: 50, step: 2, range: 0...100)
        XCTAssertEqual(edit.stepped(1), 52)
        XCTAssertEqual(edit.stepped(-1, shift: true), 30)
        XCTAssertEqual(edit.stepped(1, option: true), 50.2)
        edit.beginDrag()
        XCTAssertEqual(edit.drag(horizontal: 100), 100)
        XCTAssertEqual(edit.drag(horizontal: -100), 0)
    }

    func testReleaseCommitsOnlyOnceAfterManyPreviews() {
        var edit = KRNumberEdit(value: 10)
        edit.beginDrag()
        var previews: [Double] = []
        for distance in 3...30 { if let candidate = edit.drag(horizontal: Double(distance)) { previews.append(candidate) } }
        XCTAssertEqual(previews.count, 28)
        let commit = edit.finish()
        XCTAssertEqual(commit?.from, 10)
        XCTAssertEqual(commit?.to, 40)
        XCTAssertNil(edit.finish())
        XCTAssertNil(edit.drag(horizontal: 100))
    }

    func testClickTransitionsToDirectEntryAndClamps() {
        var edit = KRNumberEdit(value: 10, range: 0...100)
        edit.beginDrag()
        XCTAssertNil(edit.drag(horizontal: 1))
        edit.beginEditing()
        XCTAssertTrue(edit.type("120"))
        XCTAssertEqual(edit.finish()?.to, 100)
        XCTAssertNil(edit.finish())
    }

    func testCancelAndInvalidTextDoNotCommit() {
        var edit = KRNumberEdit(value: 10)
        edit.beginEditing()
        for text in ["", "abc", "nan", "inf", "1e999"] { XCTAssertFalse(edit.type(text)) }
        XCTAssertEqual(edit.candidate, 10)
        XCTAssertTrue(edit.type("12.5"))
        edit.cancel()
        XCTAssertEqual(edit.candidate, 10)
        XCTAssertNil(edit.finish())
    }
}
