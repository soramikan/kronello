import XCTest
@testable import KronelloAppModel

/// FX-004 / FX-007 GUI checks (ADR-0114 / ADR-0116): mask authoring and
/// adjustment clip creation share the normal `edit.plan` / `edit.apply`
/// command path, keep CLI parity, and undo in one Event each.
final class MaskEditingTests: XCTestCase {
    @MainActor func testAddClipMaskIsOneAtomicEditWithUndo() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        editor.selectClip(fixture.clip)
        let before = editor.revision, applies = fixture.transport.applyCount
        editor.addClipMask(editor.selectedClip!)
        try await MotionChecks().waitForEdit(editor, after: before)
        XCTAssertEqual(fixture.transport.applyCount, applies + 1)
        let commands = fixture.transport.lastApply.objects("commands")
        XCTAssertEqual(commands.count, 1)
        XCTAssertNotNil(commands[0].object("timeline")["clip_masks_set"])
        let clip = editor.selectedClip!
        let masks = EditorModel.clipMasks(clip)
        XCTAssertEqual(masks.count, 1)
        XCTAssertEqual(masks[0].string("mode"), "add")
        XCTAssertEqual(masks[0]["closed"] as? Bool, true)
        // The mask plus its four backing properties land atomically.
        XCTAssertEqual(clip.authored.objects("properties").count, 4)
        XCTAssertNotNil(EditorModel.maskVertices(clip, mask: masks[0]))
        try checks.parity(fixture)
        await editor.undo()
        XCTAssertTrue(EditorModel.clipMasks(editor.editClips[0]).isEmpty)
        try checks.parity(fixture)
    }

    @MainActor func testMaskFieldAndVertexEditsRoundtripThroughService() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        editor.selectClip(fixture.clip)
        editor.addClipMask(editor.selectedClip!)
        try await MotionChecks().waitForEdit(editor, after: editor.revision)
        var clip = editor.selectedClip!
        var base = editor.revision
        editor.setClipMaskField(clip, index: 0, field: "mode", value: "subtract")
        try await MotionChecks().waitForEdit(editor, after: base)
        clip = editor.selectedClip!
        XCTAssertEqual(EditorModel.clipMasks(clip)[0].string("mode"), "subtract")
        base = editor.revision
        editor.setClipMaskScalar(clip, index: 0, field: "feather", value: 8)
        try await MotionChecks().waitForEdit(editor, after: base)
        clip = editor.selectedClip!
        XCTAssertEqual(EditorModel.maskScalar(clip, mask: EditorModel.clipMasks(clip)[0], field: "feather"), 8)
        // Vertex insert/remove rewrites the constant path through the service.
        base = editor.revision
        let count = EditorModel.maskVertices(clip, mask: EditorModel.clipMasks(clip)[0])!.count
        editor.insertClipMaskVertex(clip, index: 0, afterVertex: 0)
        try await MotionChecks().waitForEdit(editor, after: base)
        clip = editor.selectedClip!
        XCTAssertEqual(EditorModel.maskVertices(clip, mask: EditorModel.clipMasks(clip)[0])!.count, count + 1)
        base = editor.revision
        editor.removeClipMaskVertex(clip, index: 0, vertexIndex: 1)
        try await MotionChecks().waitForEdit(editor, after: base)
        clip = editor.selectedClip!
        XCTAssertEqual(EditorModel.maskVertices(clip, mask: EditorModel.clipMasks(clip)[0])!.count, count)
        // Anchor drag commits move the authored point.
        base = editor.revision
        editor.setClipMaskAnchor(clip, index: 0, vertexIndex: 0, point: CGPoint(x: 12, y: 34))
        try await MotionChecks().waitForEdit(editor, after: base)
        clip = editor.selectedClip!
        XCTAssertEqual(EditorModel.maskVertices(clip, mask: EditorModel.clipMasks(clip)[0])![0].anchor, CGPoint(x: 12, y: 34))
        try checks.parity(fixture)
    }

    @MainActor func testRemoveClipMaskUndoesEntireStack() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        editor.selectClip(fixture.clip)
        editor.addClipMask(editor.selectedClip!)
        try await MotionChecks().waitForEdit(editor, after: editor.revision)
        var clip = editor.selectedClip!
        editor.addClipMask(clip)
        try await MotionChecks().waitForEdit(editor, after: editor.revision)
        clip = editor.selectedClip!
        XCTAssertEqual(EditorModel.clipMasks(clip).count, 2)
        let base = editor.revision
        editor.removeClipMask(clip, index: 0)
        try await MotionChecks().waitForEdit(editor, after: base)
        clip = editor.selectedClip!
        XCTAssertEqual(EditorModel.clipMasks(clip).count, 1)
        await editor.undo()
        clip = editor.editClips[0]
        XCTAssertEqual(EditorModel.clipMasks(clip).count, 2)
        try checks.parity(fixture)
    }

    @MainActor func testAddAdjustmentClipPlacesIdentityClipOnNewTrack() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        let before = editor.revision, applies = fixture.transport.applyCount
        editor.addAdjustmentClip()
        try await MotionChecks().waitForEdit(editor, after: before)
        XCTAssertEqual(fixture.transport.applyCount, applies + 1)
        // One Event carries the track_append plus clip_place pair.
        let commands = fixture.transport.lastApply.objects("commands")
        XCTAssertEqual(commands.count, 2)
        XCTAssertNotNil(commands[0].object("timeline")["track_append"])
        let placed = commands[1].object("timeline").object("clip_place").object("clip")
        XCTAssertEqual(placed.object("source_ref").string("kind"), "adjustment")
        XCTAssertEqual(placed.object("time_map").object("speed").string("num"), "1")
        XCTAssertEqual(placed.object("source_in").string("num"), "0")
        // The new clip lands above the authored track, selected, and the shared
        // query classifies it as an adjustment clip.
        let clip = editor.editClips.first { $0.id != fixture.clip }!
        XCTAssertEqual(clip.kind, .adjustment)
        XCTAssertEqual(clip.track, commands[0].object("timeline").object("track_append").object("track").string("id"))
        XCTAssertEqual(editor.selectedClip?.id, clip.id)
        try checks.parity(fixture)
        await editor.undo()
        XCTAssertEqual(editor.editClips.count, 1)
        XCTAssertEqual(editor.sequence.objects("tracks").count, 1)
        try checks.parity(fixture)
    }
}
