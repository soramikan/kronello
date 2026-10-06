import XCTest
@testable import KronelloAppModel

final class ClipAuthoringTests: XCTestCase {
    @MainActor func testTimeAndTrackStateShareCLIAndUndo() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        editor.selectClip(fixture.clip)
        let first = editor.revision, applies = fixture.transport.applyCount
        editor.setClipTime(editor.selectedClip!, speedPercent: 50)
        try await MotionChecks().waitForEdit(editor, after: first)
        XCTAssertEqual(fixture.transport.applyCount, applies + 1)
        XCTAssertEqual(editor.selectedClip?.linearRate, RationalTime(num: 1, den: 2))
        try checks.parity(fixture)
        let beforeSource = editor.revision
        editor.setClipTime(editor.selectedClip!, sourceIn: .init(num: 1, den: 2))
        try await MotionChecks().waitForEdit(editor, after: beforeSource)
        XCTAssertEqual(editor.selectedClip?.authored.object("source_in").string("num"), "1")
        let beforeTrack = editor.revision
        editor.setTrackOutput(editor.sequence.objects("tracks")[0])
        try await MotionChecks().waitForEdit(editor, after: beforeTrack)
        XCTAssertEqual(editor.sequence.objects("tracks")[0].object("state")["visible"] as? Bool, false)
        try checks.parity(fixture)
        let beforeUndo = editor.revision
        await editor.undo()
        XCTAssertNotEqual(editor.revision, beforeUndo)
        XCTAssertEqual(editor.sequence.objects("tracks")[0].object("state")["visible"] as? Bool ?? true, true)
    }

    @MainActor func testReverseCheckboxUsesExplicitPolicyAndUndo() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        editor.selectClip(fixture.clip)
        let before = editor.revision
        editor.setClipReverse(editor.selectedClip!, enabled: true)
        try await MotionChecks().waitForEdit(editor, after: before)
        XCTAssertEqual(editor.selectedClip?.reversed, true)
        XCTAssertEqual(editor.selectedClip?.linearRate, RationalTime(num: 1, den: 1))
        XCTAssertEqual(editor.selectedClip?.authored.object("source_in").string("num"), "2")
        try checks.parity(fixture)
        await editor.undo()
        XCTAssertEqual(editor.selectedClip?.reversed, false)
        XCTAssertEqual(editor.selectedClip?.authored.object("source_in").string("num"), "0")
    }

    @MainActor func testVideoOnlyControlsDoNotWriteAudioClipProperties() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        var query = editor.editClips[0].query; query["kind"] = "audio"
        let clip = EditClip(query: query), revision = editor.revision, applies = fixture.transport.applyCount
        editor.setClipProperty(clip, key: "kronello.opacity", kind: "scalar", value: 0.5)
        XCTAssertEqual(editor.revision, revision)
        XCTAssertEqual(fixture.transport.applyCount, applies)
        XCTAssertEqual(editor.failure?.code, "UNSUPPORTED_FEATURE")
    }

    @MainActor func testEffectsAndOpacityAreSingleAtomicEdits() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        editor.selectClip(fixture.clip)
        let before = editor.revision, applies = fixture.transport.applyCount
        editor.addClipEffect("shadow")
        try await MotionChecks().waitForEdit(editor, after: before)
        XCTAssertEqual(fixture.transport.applyCount, applies + 1)
        XCTAssertEqual(editor.selectedClip?.authored.objects("effects").count, 1)
        XCTAssertEqual(editor.selectedClip?.authored.objects("properties").count, 4)
        let beforeOpacity = editor.revision
        editor.setClipProperty(editor.selectedClip!, key: "kronello.opacity", kind: "scalar", value: 0.5)
        try await MotionChecks().waitForEdit(editor, after: beforeOpacity)
        XCTAssertEqual(editor.selectedClip?.authored.objects("effects").count, 1)
        XCTAssertEqual(editor.selectedClip?.authored.objects("properties").count, 5)
        try checks.parity(fixture)
        let beforeSecond = editor.revision
        editor.addClipEffect("blur")
        try await MotionChecks().waitForEdit(editor, after: beforeSecond)
        XCTAssertEqual(editor.selectedClip?.authored.objects("effects").count, 2)
        let beforeRemove = editor.revision
        editor.removeClipEffect(editor.selectedClip!, index: 0)
        try await MotionChecks().waitForEdit(editor, after: beforeRemove)
        XCTAssertEqual(editor.selectedClip?.authored.objects("effects").count, 1)
        try checks.parity(fixture)
    }
}
