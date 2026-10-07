import XCTest
@testable import KronelloAppModel

final class MotionAuthoringTests: XCTestCase {
    @MainActor
    func testCanvasGuideSnappingAndOptionBypassDoNotAuthorState() throws {
        let checks = GUIChecks(), folder = try checks.temporary()
        defer { try? FileManager.default.removeItem(at: folder) }
        let editor = checks.model(FakeTransport(), folder: folder)
        var document = EditorModel.newDocument(name: "Guide tests")
        var compositions = document.objects("compositions")
        compositions[0]["design_extent"] = ["width": 640, "height": 480]
        document["compositions"] = compositions
        editor.ui.composition = compositions[0].string("id")
        editor.adopt(document: document, scene: [:], revision: "6", actor: "", external: false)
        editor.addGuide(axis: "x", position: 500)
        let edit = CanvasEdit(layer: .init(id: "node", authored: [:], evaluated: [:], level: 0), base: "6", time: .init(num: 0, den: 1), bounds: .init(x: 280, y: 180, width: 250, height: 100), parent: .identity)
        let delta = CGSize(width: -28.2 / 640, height: 0)
        let snapped = editor.snappedCanvasTranslation(edit, translation: delta)
        XCTAssertEqual(280 + snapped.width * 640, 250, accuracy: 1e-9)
        // MotionViewer uses snap:false while Option is held.
        XCTAssertEqual(editor.snappedCanvasTranslation(edit, translation: delta, snap: false), delta)
        editor.canvasSettings.snap = false
        XCTAssertEqual(editor.snappedCanvasTranslation(edit, translation: delta), delta)
        let guide = editor.canvasSettings.guides[0]
        editor.removeGuide(guide.id)
        XCTAssertTrue(editor.canvasSettings.guides.isEmpty)
        XCTAssertEqual(editor.revision, "6")
    }
    @MainActor
    func testColorEditsPreserveMatchingKeyAndLocalSegmentInterpolation() {
        let keys: [[String: Any]] = [
            ["time": RationalTime(num: 0, den: 1).wire, "interpolation": ["kind": "linear"]],
            ["time": RationalTime(num: 1, den: 3).wire, "interpolation": ["kind": "hold"]],
            ["time": RationalTime(num: 2, den: 3).wire, "interpolation": ["kind": "cubic", "value": ["control1": [0.2, 0.3], "control2": [0.7, 0.8]]]]
        ]
        XCTAssertEqual(EditorModel.colorInterpolation(keys: keys, time: .init(num: 1, den: 3)).string("kind"), "hold")
        XCTAssertEqual(EditorModel.colorInterpolation(keys: keys, time: .init(num: 1, den: 2)).string("kind"), "hold")
        XCTAssertEqual(EditorModel.colorInterpolation(keys: keys, time: .init(num: 2, den: 3)).object("value")["control1"] as? [Double], [0.2, 0.3])
        XCTAssertEqual(EditorModel.colorInterpolation(keys: keys, time: .init(num: Int64.max - 1, den: Int64.max)).string("kind"), "cubic")
    }
    @MainActor
    func testColorCommandKeepsCurveIdentityAndExistingKeyInterpolation() throws {
        let checks = GUIChecks(), folder = try checks.temporary()
        defer { try? FileManager.default.removeItem(at: folder) }
        let editor = checks.model(FakeTransport(), folder: folder)
        let property: [String: Any] = ["id": "color", "source": ["kind": "curve", "value": "curve"]]
        let keys: [[String: Any]] = [
            ["time": RationalTime(num: 0, den: 1).wire, "interpolation": ["kind": "linear"]],
            ["time": RationalTime(num: 1, den: 3).wire, "interpolation": ["kind": "hold"]]
        ]
        editor.adopt(document: ["curves": [["id": "curve", "keys": keys]]], scene: [:], revision: "7", actor: "", external: false)
        let color = try EditorModel.colorValue(hex: "#44CC88FF")
        let command = try editor.colorCommand(layer: .init(id: "node", authored: [:], evaluated: [:], level: 0), property: property, color: color, time: .init(num: 1, den: 3)).object("keyframe_upsert")
        XCTAssertEqual(command.string("curve"), "curve")
        XCTAssertEqual(command.object("key").object("interpolation").string("kind"), "hold")
        XCTAssertEqual(command.object("key").object("time").string("den"), "3")
        XCTAssertEqual(editor.revision, "7")
    }
    @MainActor
    func testFontInputsFollowProjectLocksWithoutDiscardingCatalog() {
        let identity: [String: Any] = ["family": "Test", "postscript_name": "Test-Regular", "sha256": "locked", "face_index": 0]
        let other: [String: Any] = ["family": "Test", "postscript_name": "Test-Bold", "sha256": "locked", "face_index": 1]
        let inputs: [[String: Any]] = [["identity": identity, "path": "/regular"], ["identity": other, "path": "/bold"]]
        let textDocument: [String: Any] = ["texts": [["styles": [["font": identity]]]]]
        XCTAssertEqual(EditorModel.fontInputs(inputs, requiredBy: textDocument).count, 1)
        XCTAssertEqual(EditorModel.fontInputs(inputs, requiredBy: textDocument).first?.string("path"), "/regular")
        XCTAssertTrue(EditorModel.fontInputs(inputs, requiredBy: ["texts": []]).isEmpty)
        XCTAssertEqual(inputs.count, 2)
    }
    func testJapaneseRangeUsesWholeCombiningAndIVSClusters() throws {
        let value = "日か\u{3099}葛\u{E0100}本"
        let range = try TextSpanEditing.byteRange(value, start: 1, end: 3)
        XCTAssertEqual(range["start"], "日".utf8.count)
        XCTAssertEqual(range["end"], "日か\u{3099}葛\u{E0100}".utf8.count)
        XCTAssertThrowsError(try TextSpanEditing.byteRange(value, start: 0, end: 5))
    }

    func testReplacementRetainsSuffixFontAndSplitsSelectedCluster() throws {
        let text: [String: Any] = ["text": "日本語", "styles": [
            ["range": ["start": 0, "end": 6], "font": ["sha256": "first"], "fill": "red"],
            ["range": ["start": 6, "end": 9], "font": ["sha256": "second"], "fill": "blue"]
        ]]
        let replaced = try TextSpanEditing.replaced(text, value: "日か\u{3099}本語")
        let spans = replaced.objects("styles")
        XCTAssertEqual(spans.last?.object("font").string("sha256"), "second")
        XCTAssertEqual(spans.last?.object("range").number("end"), Double("日か\u{3099}本語".utf8.count))
        let range = try TextSpanEditing.byteRange(replaced.string("text"), start: 1, end: 2)
        let painted = TextSpanEditing.applying(replaced, range: range, fill: "new-color")
        let selected = painted.objects("styles").first { $0.string("fill") == "new-color" }
        XCTAssertEqual(selected?.object("range").number("start"), 3)
        XCTAssertEqual(selected?.object("range").number("end"), 9)
        XCTAssertEqual(painted.objects("styles").last?.object("font").string("sha256"), "second")
    }
}
