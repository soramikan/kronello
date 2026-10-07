import Foundation
import XCTest
@testable import KronelloAppModel

/// GUI-009: caption lane presentation, caption_set command shape and the
/// track_append + caption_set + clip_place creation path, all through the real
/// worker transport.
final class CaptionEditingTests: XCTestCase {
    static let font: [String: Any] = ["family": "Kronello Sans", "postscript_name": "KronelloSans-Regular",
        "sha256": String(repeating: "a", count: 64), "face_index": 0]

    @MainActor private func fixture(withCaptionCue: Bool = true) async throws -> (fixture: EditChecks.Fixture, editor: EditorModel, clip: EditClip) {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        editor.fonts = [["identity": Self.font, "path": "/tmp/kronello-test.ttf"]]
        if withCaptionCue {
            let commands = editor.captionAddCommands(at: 0, durationFrames: 48, font: Self.font).commands
            let event = await editor.apply(.init(base: editor.revision, commands: commands, label: "test cue"))
            try require(event != nil, "caption track + document + clip apply in one Event")
        }
        return (fixture, editor, editor.editClips.first { $0.caption != nil } ?? EditClip(query: [:]))
    }

    @MainActor private func finish(_ fixture: EditChecks.Fixture) async {
        await EditChecks().finish(fixture)
    }

    @MainActor func testCaptionClipMapsSubtitleKindNameAndLaneOrder() async throws {
        let (fixture, editor, clip) = try await fixture()
        defer { Task { await finish(fixture) } }
        XCTAssertEqual(clip.kind, .subtitle, "ClipKind caption presents as the subtitle design kind")
        XCTAssertNotNil(clip.caption)
        XCTAssertEqual(editor.captionDocument(for: clip)?.string("text"), "字幕")
        XCTAssertEqual(editor.clipName(clip), "字幕")
        let kinds = editor.orderedTracks.map { $0.string("kind") }
        XCTAssertEqual(kinds, ["caption", "video"], "Caption lanes render above video lanes")
        let captionTrack = editor.sequence.objects("tracks").first { $0.string("kind") == "caption" }!
        XCTAssertEqual(editor.trackNumber(captionTrack.string("id")), "C1")
        XCTAssertEqual(editor.trackNumber(fixture.track), "V1")
        // The caption lane header toggles authored visibility, not audio mute.
        let before = editor.revision
        editor.setTrackOutput(captionTrack)
        try await MotionChecks().waitForEdit(editor, after: before)
        let updated = editor.sequence.objects("tracks").first { $0.string("kind") == "caption" }!
        XCTAssertEqual(updated.object("state")["visible"] as? Bool, false)
        XCTAssertEqual(updated.object("state")["muted"] as? Bool, false)
        await editor.undo()
    }

    @MainActor func testCaptionTrackRejectsAssetPlacement() async throws {
        let (fixture, editor, _) = try await fixture()
        defer { Task { await finish(fixture) } }
        let asset = editor.editAssets.first { $0.kind == .composition }!
        let captionTrack = editor.sequence.objects("tracks").first { $0.string("kind") == "caption" }!
        editor.beginAssetGesture(asset, track: captionTrack.string("id"), at: 0)
        XCTAssertNil(editor.timelineCandidate, "Caption lanes accept no asset placements")
    }

    @MainActor func testCaptionTextStyleAndPlacementApplyAndUndo() async throws {
        let (fixture, editor, clip) = try await fixture()
        defer { Task { await finish(fixture) } }
        let applies = fixture.transport.applyCount, before = editor.revision
        editor.setCaptionText(clip, to: "新しい字幕\n2 行目")
        try await MotionChecks().waitForEdit(editor, after: before)
        XCTAssertEqual(fixture.transport.applyCount, applies + 1)
        XCTAssertEqual(fixture.transport.lastApply.objects("commands").count, 1)
        let command = fixture.transport.lastApply.objects("commands")[0].object("caption_set").object("caption")
        XCTAssertEqual(command.string("text"), "新しい字幕\n2 行目")
        XCTAssertEqual(command.number("version"), 1, "Unedited fields are preserved verbatim")
        XCTAssertEqual(command.string("id"), clip.caption)
        let current = editor.editClips.first { $0.caption != nil }!
        XCTAssertEqual(editor.captionDocument(for: current)?.string("text"), "新しい字幕\n2 行目")

        var base = editor.revision
        editor.setCaptionSize(current, size: 72)
        try await MotionChecks().waitForEdit(editor, after: base)
        XCTAssertEqual(editor.captionDocument(for: editor.editClips.first { $0.caption != nil }!)?.object("style").number("size"), 72)

        base = editor.revision
        editor.setCaptionFill(editor.editClips.first { $0.caption != nil }!, hex: "#FF0000FF")
        try await MotionChecks().waitForEdit(editor, after: base)
        let fill = editor.captionDocument(for: editor.editClips.first { $0.caption != nil }!)!.object("style").object("fill")
        XCTAssertEqual(fill.object("components").number("r"), 1)

        base = editor.revision
        editor.setCaptionAnchor(editor.editClips.first { $0.caption != nil }!, anchor: "top_center")
        try await MotionChecks().waitForEdit(editor, after: base)
        var document = editor.captionDocument(for: editor.editClips.first { $0.caption != nil }!)!
        XCTAssertEqual(document.object("placement").string("anchor"), "top_center")
        XCTAssertEqual(document.object("style").number("size"), 72, "Unedited style fields are preserved verbatim")
        XCTAssertEqual(document.object("style").object("fill").object("components").number("r"), 1)

        base = editor.revision
        editor.setCaptionOutline(editor.editClips.first { $0.caption != nil }!, width: 3, hex: "#00FF00FF")
        try await MotionChecks().waitForEdit(editor, after: base)
        document = editor.captionDocument(for: editor.editClips.first { $0.caption != nil }!)!
        XCTAssertEqual(document.object("style").object("outline").number("width"), 3)

        base = editor.revision
        editor.setCaptionBackground(editor.editClips.first { $0.caption != nil }!, hex: "#00000080")
        try await MotionChecks().waitForEdit(editor, after: base)
        document = editor.captionDocument(for: editor.editClips.first { $0.caption != nil }!)!
        XCTAssertFalse(document.object("style").object("background").isEmpty)
        base = editor.revision
        editor.setCaptionBackground(editor.editClips.first { $0.caption != nil }!, hex: nil)
        try await MotionChecks().waitForEdit(editor, after: base)
        document = editor.captionDocument(for: editor.editClips.first { $0.caption != nil }!)!
        XCTAssertTrue(document.object("style").object("background").isEmpty, "nil hex removes the cue background")

        base = editor.revision
        editor.setCaptionOffset(editor.editClips.first { $0.caption != nil }!, axis: 1, percent: 10)
        try await MotionChecks().waitForEdit(editor, after: base)
        document = editor.captionDocument(for: editor.editClips.first { $0.caption != nil }!)!
        let offset = document.object("placement")["offset"] as? [[String: Any]] ?? []
        XCTAssertEqual(offset[1].string("num"), "1")
        XCTAssertEqual(offset[1].string("den"), "10")

        // Undo rewinds each committed Event through the shared history.
        await editor.undo()
        document = editor.captionDocument(for: editor.editClips.first { $0.caption != nil }!)!
        XCTAssertEqual((document.object("placement")["offset"] as? [[String: Any]] ?? [])[1].string("num"), "0")
        try EditChecks().parity(fixture)
    }

    @MainActor func testCaptionTextEditKeepsOnlyByteIdenticalSpans() {
        let spans: [[String: Any]] = [
            ["range": ["start": 0, "end": 3], "bold": true],
            ["range": ["start": 3, "end": 6], "italic": true],
        ]
        // "abcABC" -> "abABC": the first span's bytes survive at the same
        // offsets; the second span's covered bytes changed and is dropped.
        let kept = EditorModel.captionSpans(spans, preservedIn: "abcABC", text: "abABC")
        XCTAssertEqual(kept.count, 0, "Overlapping edits invalidate spans instead of producing stale ranges")
        // Appending after the span keeps both boundaries valid and bytes equal.
        let appended = EditorModel.captionSpans(spans, preservedIn: "abcABC", text: "abcABCdef")
        XCTAssertEqual(appended.count, 2)
        // Insertion inside a span drops it even though the range fits.
        let inserted = EditorModel.captionSpans(spans, preservedIn: "abcABC", text: "abcXABC")
        XCTAssertEqual(inserted.count, 1)
        XCTAssertTrue(inserted[0]["bold"] as? Bool == true)
        // Multibyte graphemes must not be split: deleting half of "あ" shifts bytes.
        let jp = [["range": ["start": 0, "end": 3], "color": ["space": "srgb", "components": ["r": 1.0, "g": 0.0, "b": 0.0, "alpha": 1.0]]]]
        XCTAssertEqual(EditorModel.captionSpans(jp, preservedIn: "あいう", text: "あいう").count, 1)
        XCTAssertEqual(EditorModel.captionSpans(jp, preservedIn: "あいう", text: "いう").count, 0)
    }

    @MainActor func testAddCaptionCreatesTrackDocumentAndClipThenUndoes() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        editor.fonts = [["identity": Self.font, "path": "/tmp/kronello-test.ttf"]]
        let applies = fixture.transport.applyCount, before = editor.revision
        editor.addCaption()
        try await MotionChecks().waitForEdit(editor, after: before)
        XCTAssertEqual(fixture.transport.applyCount, applies + 1)
        let commands = fixture.transport.lastApply.objects("commands")
        XCTAssertEqual(commands.count, 3, "track_append + caption_set + clip_place land in one Event")
        XCTAssertNotNil(commands[0]["timeline"])
        XCTAssertNotNil(commands[1]["caption_set"])
        XCTAssertNotNil(commands[2]["timeline"])
        let clip = editor.editClips.first { $0.caption != nil }
        XCTAssertNotNil(clip)
        XCTAssertEqual(clip?.kind, .subtitle)
        XCTAssertEqual(editor.ui.clipSelection, clip?.id, "The new cue is selected after the Event lands")
        XCTAssertEqual(clip?.authored.object("source_in").string("num"), "0")
        XCTAssertEqual(clip?.authored["audio_retime"] as? String, "reject")
        XCTAssertNil(clip?.authored["volume"], "Caption cues carry no volume")
        let undoCount = editor.undoState.undo.count
        await editor.undo()
        XCTAssertTrue(editor.editClips.allSatisfy { $0.caption == nil })
        XCTAssertTrue(editor.sequence.objects("tracks").allSatisfy { $0.string("kind") != "caption" })
        XCTAssertTrue(editor.document.objects("captions").isEmpty)
        XCTAssertEqual(editor.undoState.undo.count, undoCount - 1, "One Undo removes track, document and clip")
        try checks.parity(fixture)
    }

    @MainActor func testAddCaptionWithoutLockedFontFailsVisibly() async throws {
        let checks = EditChecks(), fixture = try await checks.fixture(), editor = fixture.editor
        defer { Task { await checks.finish(fixture) } }
        editor.fonts = []
        let applies = fixture.transport.applyCount
        editor.addCaption()
        XCTAssertEqual(fixture.transport.applyCount, applies)
        XCTAssertEqual(editor.failure?.code, "FONT_MISSING", "No fake or system font is fabricated")
    }

    @MainActor func testCaptionFontsFeedPreviewInputs() async throws {
        let (fixture, editor, _) = try await fixture()
        defer { Task { await finish(fixture) } }
        XCTAssertTrue(editor.lockedFonts.contains { $0.string("sha256") == Self.font.string("sha256") })
        XCTAssertTrue(editor.snapshotFonts.contains { NSDictionary(dictionary: $0.object("identity")) == NSDictionary(dictionary: Self.font) },
                      "Caption font identity feeds the render snapshot font inputs")
    }
}
