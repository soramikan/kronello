import Foundation
import XCTest
@testable import KronelloAppModel

/// GUI-010: color effect insertion with identity defaults, parameter commits
/// through clip_set_effects, and preservation of non-constant sources.
final class ColorEffectTests: XCTestCase {
    @MainActor private func selected(_ editor: EditorModel, fixture: EditChecks.Fixture) -> EditClip {
        editor.selectClip(fixture.clip)
        return editor.selectedClip!
    }

    @MainActor func testColorEffectsInsertIdentityDefaultsAndUndo() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        _ = selected(editor, fixture: fixture)
        let before = editor.revision, applies = fixture.transport.applyCount
        editor.addClipEffect("color_exposure")
        try await MotionChecks().waitForEdit(editor, after: before)
        XCTAssertEqual(fixture.transport.applyCount, applies + 1)
        let clip = editor.selectedClip!
        let effects = clip.authored.objects("effects")
        XCTAssertEqual(effects.count, 1)
        XCTAssertEqual(effects[0].string("effect_id"), "kronello.color.exposure")
        XCTAssertEqual(effects[0].number("version"), 1)
        XCTAssertEqual(effects[0].object("parameters").string("kind"), "color_exposure")
        XCTAssertEqual(EditorModel.colorParameterValue(clip, effect: effects[0], parameter: "exposure"), 0)
        XCTAssertEqual(EditorModel.colorParameterValue(clip, effect: effects[0], parameter: "offset"), 0)
        let exposureID = effects[0].object("parameters").string("exposure")
        let property = clip.authored.objects("properties").first { $0.string("id") == exposureID }!
        XCTAssertEqual(property.object("descriptor").string("key"), "kronello.effect.exposure")
        XCTAssertEqual(property.object("source").object("value").string("kind"), "scalar")

        // One commit writes a constant scalar into the referenced Property.
        var base = editor.revision
        editor.setClipEffectParameter(clip, effect: effects[0], parameter: "exposure", kind: "scalar", value: 1.5)
        try await MotionChecks().waitForEdit(editor, after: base)
        let edited = editor.selectedClip!
        XCTAssertEqual(EditorModel.colorParameterValue(edited, effect: edited.authored.objects("effects")[0], parameter: "exposure"), 1.5)
        XCTAssertEqual(edited.authored.objects("effects").count, 1, "Other effects survive the parameter commit")
        try checks.parity(fixture)

        // Undo restores the authored constant through the shared history.
        await editor.undo()
        let restored = editor.selectedClip!
        XCTAssertEqual(EditorModel.colorParameterValue(restored, effect: restored.authored.objects("effects")[0], parameter: "exposure"), 0)
        // Removal is its own clip_set_effects Event; undoing the newest Event
        // restores the whole stack in one step.
        base = editor.revision
        editor.removeClipEffect(restored, index: 0)
        try await MotionChecks().waitForEdit(editor, after: base)
        XCTAssertTrue(editor.selectedClip!.authored.objects("effects").isEmpty)
        await editor.undo()
        XCTAssertEqual(editor.selectedClip!.authored.objects("effects").count, 1)
        try checks.parity(fixture)
    }

    @MainActor func testEveryColorKindProducesValidParameters() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        _ = selected(editor, fixture: fixture)
        let expected: [(String, String, [String: String])] = [
            ("color_exposure", "kronello.color.exposure", ["exposure": "kronello.effect.exposure", "offset": "kronello.effect.exposure_offset"]),
            ("color_levels", "kronello.color.levels", ["in_black": "kronello.effect.in_black", "in_white": "kronello.effect.in_white",
                                                        "gamma": "kronello.effect.gamma", "out_black": "kronello.effect.out_black", "out_white": "kronello.effect.out_white"]),
            ("color_curves", "kronello.color.curves", ["curve": "kronello.effect.curve"]),
            ("color_hsl", "kronello.color.hsl", ["hue_shift": "kronello.effect.hue_shift", "saturation": "kronello.effect.saturation", "lightness": "kronello.effect.lightness"]),
        ]
        for (kind, id, keys) in expected {
            let before = editor.revision
            editor.addClipEffect(kind)
            try await MotionChecks().waitForEdit(editor, after: before)
            let clip = editor.selectedClip!
            let effect = clip.authored.objects("effects").first { $0.string("effect_id") == id }!
            for (field, key) in keys {
                let propertyID = effect.object("parameters")[field] as? String
                let property = clip.authored.objects("properties").first { $0.string("id") == propertyID }
                XCTAssertNotNil(property, "\(kind).\(field) references an authored Property")
                XCTAssertEqual(property?.object("descriptor").string("key"), key)
            }
        }
        let clip = editor.selectedClip!
        let effects = clip.authored.objects("effects")
        XCTAssertEqual(effects.count, 4)
        // HSL hue is an Angle value; the curve parameter is a DataTable.
        let hsl = effects.first { $0.string("effect_id") == "kronello.color.hsl" }!
        let hueID = hsl.object("parameters").string("hue_shift")
        let hue = clip.authored.objects("properties").first { $0.string("id") == hueID }!
        XCTAssertEqual(hue.object("source").object("value").string("kind"), "angle")
        let curves = effects.first { $0.string("effect_id") == "kronello.color.curves" }!
        XCTAssertEqual(EditorModel.curveRows(clip, effect: curves)?.map { [$0.x, $0.y] }, [[0, 0], [1, 1]])
        try checks.parity(fixture)
        // Removing keeps the stack valid and preserves unrelated properties.
        let index = effects.firstIndex { $0.string("effect_id") == "kronello.color.levels" }!
        let before = editor.revision
        editor.removeClipEffect(clip, index: index)
        try await MotionChecks().waitForEdit(editor, after: before)
        XCTAssertEqual(editor.selectedClip!.authored.objects("effects").count, 3)
        XCTAssertFalse(editor.selectedClip!.authored.objects("effects").contains { $0.string("effect_id") == "kronello.color.levels" })
        try checks.parity(fixture)
    }

    @MainActor func testCurveTableEditsProduceValidTablesAndUndo() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        _ = selected(editor, fixture: fixture)
        var before = editor.revision
        editor.addClipEffect("color_curves")
        try await MotionChecks().waitForEdit(editor, after: before)
        var clip = editor.selectedClip!, effect = clip.authored.objects("effects")[0]
        // Reorder is normalized by the table builder.
        guard let table = EditorModel.curveTableValue(points: [(x: 0.8, y: 0.2), (x: 0, y: 0), (x: 0.4, y: 1)]) else {
            return XCTFail("valid points build a table")
        }
        before = editor.revision
        editor.setClipEffectParameter(clip, effect: effect, parameter: "curve", kind: "data_table", value: table)
        try await MotionChecks().waitForEdit(editor, after: before)
        clip = editor.selectedClip!; effect = clip.authored.objects("effects")[0]
        XCTAssertEqual(EditorModel.curveRows(clip, effect: effect)?.count, 3)
        XCTAssertEqual(EditorModel.curveRows(clip, effect: effect)?[1].x, 0.4)
        await editor.undo()
        clip = editor.selectedClip!; effect = clip.authored.objects("effects")[0]
        XCTAssertEqual(EditorModel.curveRows(clip, effect: effect)?.count, 2)
        try checks.parity(fixture)
    }

    @MainActor func testCurveTableValueValidationRules() {
        // Sorted, strictly increasing x inside 0...1, 2...64 rows, finite values.
        XCTAssertNil(EditorModel.curveTableValue(points: [(x: 0, y: 0)]))
        XCTAssertNil(EditorModel.curveTableValue(points: [(x: 0, y: 0), (x: 0, y: 1)]))
        XCTAssertNil(EditorModel.curveTableValue(points: [(x: -0.1, y: 0), (x: 1, y: 1)]))
        XCTAssertNil(EditorModel.curveTableValue(points: [(x: 0, y: 0), (x: 1, y: .nan)]))
        XCTAssertNil(EditorModel.curveTableValue(points: (0...64).map { (x: Double($0) / 64, y: 0) }))
        let table = EditorModel.curveTableValue(points: [(x: 1, y: 1), (x: 0, y: 0)])!
        let rows = table["rows"] as? [[String: Any]]
        XCTAssertEqual(rows?.count, 2)
        XCTAssertEqual((rows?[0]["x"] as? [String: Any])?["value"] as? Double, 0)
    }

    @MainActor func testNonConstantParameterSourceIsPreservedNotOverwritten() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        _ = selected(editor, fixture: fixture)
        let before = editor.revision
        editor.addClipEffect("color_exposure")
        try await MotionChecks().waitForEdit(editor, after: before)
        let clip = editor.selectedClip!, effect = clip.authored.objects("effects")[0]
        let exposureID = effect.object("parameters").string("exposure")
        // A curve/expression-sourced parameter is presented read-only: the GUI
        // refuses to overwrite it instead of silently flattening animation.
        var authored = clip.authored
        var properties = authored.objects("properties")
        let index = properties.firstIndex { $0.string("id") == exposureID }!
        properties[index]["source"] = ["kind": "curve", "value": UUID().uuidString]
        authored["properties"] = properties
        let animated = EditClip(query: ["clip": authored, "track": clip.track, "kind": "video"])
        XCTAssertFalse(EditorModel.colorParameterIsConstant(animated, effect: authored.objects("effects")[0], parameter: "exposure"))
        let applies = fixture.transport.applyCount, revision = editor.revision
        editor.setClipEffectParameter(animated, effect: authored.objects("effects")[0], parameter: "exposure", kind: "scalar", value: 2)
        XCTAssertEqual(fixture.transport.applyCount, applies, "No edit is issued for a non-constant source")
        XCTAssertEqual(editor.revision, revision)
        XCTAssertEqual(editor.failure?.code, "UNSUPPORTED_FEATURE")
    }
}
