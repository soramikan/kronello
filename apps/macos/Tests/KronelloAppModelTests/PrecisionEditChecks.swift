import Foundation
import AppKit
import KronelloAppModel
import KronelloCore
import KronelloDesign

/// Model-level checks for GUI-008: precision edit tools, markers, In/Out and snapping.
/// All assertions go through the real shared Command/Query worker (RecordingTransport).
@MainActor struct PrecisionEditChecks {
    struct Fixture {
        let folder: URL
        let editor: EditorModel
        let transport: RecordingTransport
        let sequence: String
        let track: String
        let clips: [String]
    }
    /// Three contiguous 1 s composition clips on one 24 fps video track: enough
    /// adjacency and a 10 s source for slide/roll handle room.
    func fixture() async throws -> Fixture {
        let checks = GUIChecks(), folder = try checks.temporary(), path = folder.appendingPathComponent("edit.kronello").path
        let transport = try RecordingTransport(path: path, worker: checks.root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let editor = EditorModel(path: path, transport: transport, stateStore: .init(root: folder.appendingPathComponent("state")))
        var document = EditorModel.newDocument(name: "GUI-008 checks")
        let sequence = UUID().uuidString.lowercased(), track = UUID().uuidString.lowercased()
        let ids = (0..<3).map { _ in UUID().uuidString.lowercased() }
        let composition = document.objects("compositions")[0].string("id")
        func clip(_ index: Int) -> [String: Any] {
            ["id": ids[index], "source_ref": ["kind": "composition", "composition": composition],
             "timeline_range": ["start": ["num": String(index), "den": "1"], "end": ["num": String(index + 1), "den": "1"]],
             "source_in": ["num": "0", "den": "1"],
             "time_map": ["kind": "linear", "offset": ["num": "0", "den": "1"], "speed": ["num": "1", "den": "1"]],
             "links": [String](), "effects": [[String: Any]](), "properties": [[String: Any]](), "markers": [[String: Any]]()]
        }
        document["sequences"] = [["id": sequence, "extent": ["width": 320, "height": 180], "frame_rate": ["num": "24", "den": "1"],
            "audio_rate": 48000, "working_space": "linear_rec709", "markers": [[String: Any]](),
            "tracks": [["id": track, "kind": "video", "clips": [clip(0), clip(1), clip(2)]]]]]
        editor.ui.page = "edit"
        try await editor.start(newDocument: document)
        editor.ui.page = "edit"; try await editor.reload()
        editor.editSnap = false
        return .init(folder: folder, editor: editor, transport: transport, sequence: sequence, track: track, clips: ids)
    }
    func finish(_ f: Fixture) async { await f.editor.close(); try? FileManager.default.removeItem(at: f.folder) }
    func timeline(_ f: Fixture, _ name: String) -> [String: Any] {
        f.transport.lastApply.objects("commands").first?.object("timeline").object(name) ?? [:]
    }
    func waitForApply(_ f: Fixture, after base: String) async throws {
        try await MotionChecks().waitForEdit(f.editor, after: base)
    }

    /// Slip keeps the placement fixed and moves the source window by delta.
    func verifySlipReleaseOnce() async throws {
        let f = try await fixture(), e = f.editor, clip = e.editClips[0]
        let plans = f.transport.planCount, applies = f.transport.applyCount
        e.beginClipGesture(clip, mode: .slip)
        for delta in 1...12 { e.updateClipGesture(delta: Int64(delta)) }
        try require(e.timelineCandidate?.delta == 12 && e.timelineCandidate?.start == 0 && e.timelineCandidate?.end == 24,
                    "Slip preview is local and keeps the placement fixed")
        try require(f.transport.planCount == plans && f.transport.applyCount == applies, "Slip drag issues no service edit")
        let event = await e.commitClipGesture()
        try require(event != nil && f.transport.planCount == plans + 1 && f.transport.applyCount == applies + 1,
                    "Slip release issues exactly one planned command")
        let command = timeline(f, "clip_slip")
        try require(command.string("sequence") == f.sequence && command.string("clip") == clip.id
                    && command.object("delta").string("num") == "1" && command.object("delta").string("den") == "2"
                    && command["linked"] as? Bool == true, "clip_slip carries the exact rational delta")
        try require(RationalTime.wire(e.editClips[0].authored.object("source_in")) == .init(num: 1, den: 2)
                    && e.editClips[0].start == clip.start && e.editClips[0].end == clip.end,
                    "Slip shifts source_in while placement stays fixed")
        try parity(f)
        await finish(f)
    }
    func parity(_ f: Fixture) throws {
        let export = try GUIChecks().cli(["operation": "project.export", "project": f.editor.path])
        try require(export.string("revision") == f.editor.revision, "CLI sees the GUI revision")
        try require(NSDictionary(dictionary: export.object("document").objects("sequences")[0]) == NSDictionary(dictionary: f.editor.sequence),
                    "GUI query and CLI read the same Sequence")
    }
    /// Slide moves the middle clip; both neighbours absorb through handles.
    func verifySlideReleaseOnce() async throws {
        let f = try await fixture(), e = f.editor
        let middle = e.editClips.first { $0.id == f.clips[1] }!
        let plans = f.transport.planCount, applies = f.transport.applyCount
        e.beginClipGesture(middle, mode: .slide)
        e.updateClipGesture(delta: 6)
        try require(e.timelineCandidate?.delta == 6 && e.timelineCandidate?.start == 30, "Slide preview moves the placement")
        let event = await e.commitClipGesture()
        try require(event != nil && f.transport.planCount == plans + 1 && f.transport.applyCount == applies + 1,
                    "Slide release issues exactly one planned command")
        let command = timeline(f, "clip_slide")
        try require(command.string("clip") == middle.id && command.object("delta").string("num") == "1"
                    && command.object("delta").string("den") == "4" && command["linked"] as? Bool == true,
                    "clip_slide carries the exact rational delta")
        let moved = e.editClips.first { $0.id == middle.id }!
        try require(moved.start.frames(rateNum: 24, rateDen: 1) == 30 && moved.end.frames(rateNum: 24, rateDen: 1) == 54,
                    "Slide moves the clip by the dragged delta")
        let left = e.editClips.first { $0.id == f.clips[0] }!, right = e.editClips.first { $0.id == f.clips[2] }!
        try require(left.end == moved.start && right.start == moved.end, "Adjacent clips absorb the slide and stay contiguous")
        try require(left.start.frames(rateNum: 24, rateDen: 1) == 0 && right.end.frames(rateNum: 24, rateDen: 1) == 72,
                    "Slide keeps the run's outer extent")
        try parity(f); await e.undo()
        try require(e.editClips.first { $0.id == middle.id }!.start == middle.start, "Undo restores the slide in one Event")
        try parity(f); await finish(f)
    }
    /// Roll drags the shared edit point; both sides trim symmetrically.
    func verifyRollReleaseOnce() async throws {
        let f = try await fixture(), e = f.editor
        let first = e.editClips.first { $0.id == f.clips[0] }!
        let second = e.editClips.first { $0.id == f.clips[1] }!
        let plans = f.transport.planCount, applies = f.transport.applyCount
        // Grabbing the second clip's start edge targets the edit point after clip 0.
        e.beginRollGesture(second, atStartEdge: true)
        try require(e.timelineCandidate?.clip.string("id") == first.id && e.timelineCandidate?.mode == .roll,
                    "A start-edge roll retargets the previous clip")
        e.updateClipGesture(delta: 12)
        try require(e.timelineCandidate?.cut == 36 && e.timelineCandidate?.delta == 12 && f.transport.applyCount == applies,
                    "Roll preview is local")
        let event = await e.commitClipGesture()
        try require(event != nil && f.transport.planCount == plans + 1 && f.transport.applyCount == applies + 1,
                    "Roll release issues exactly one planned command")
        let command = timeline(f, "clip_roll")
        try require(command.string("clip") == first.id && command.object("delta").string("num") == "1"
                    && command.object("delta").string("den") == "2", "clip_roll carries the exact rational delta")
        let left = e.editClips.first { $0.id == first.id }!, right = e.editClips.first { $0.id == second.id }!
        try require(left.end.frames(rateNum: 24, rateDen: 1) == 36 && right.start.frames(rateNum: 24, rateDen: 1) == 36,
                    "Roll moves the shared edit point symmetrically")
        try parity(f); await finish(f)
    }
    /// Plain delete keeps the gap; ripple delete closes it on the track.
    func verifyDeleteAndRippleDelete() async throws {
        let f = try await fixture(), e = f.editor
        e.selectClip(f.clips[0])
        let applies = f.transport.applyCount, base = e.revision
        e.deleteSelectedClip()
        try await waitForApply(f, after: base)
        try require(f.transport.applyCount == applies + 1, "Delete issues one apply")
        let command = timeline(f, "clip_delete")
        try require(command.string("clip") == f.clips[0] && command["linked"] as? Bool == true, "clip_delete uses singular clip")
        try require(e.editClips.count == 2 && e.editClips[0].start.frames(rateNum: 24, rateDen: 1) == 24,
                    "Delete keeps the gap")
        try require(e.selectedClip == nil, "Deleted clip selection clears")
        let rippleBase = e.revision, rippleApplies = f.transport.applyCount
        e.selectClip(f.clips[1])
        e.deleteSelectedClip(ripple: true)
        try await waitForApply(f, after: rippleBase)
        try require(f.transport.applyCount == rippleApplies + 1, "Ripple delete issues one apply")
        let ripple = timeline(f, "ripple_delete")
        try require(ripple["tracks"] as? [String] == [f.track]
                    && RationalTime.wire(ripple.object("range").object("start")) == .init(num: 1, den: 1)
                    && RationalTime.wire(ripple.object("range").object("end")) == .init(num: 2, den: 1),
                    "ripple_delete carries the track and exact range")
        try require(e.editClips.count == 1 && e.editClips[0].id == f.clips[2]
                    && e.editClips[0].start.frames(rateNum: 24, rateDen: 1) == 24, "Ripple delete closes the gap")
        try parity(f); await finish(f)
    }
    /// marker_set / marker_move / marker_remove and the GUI drag/selection cycle.
    func verifyMarkers() async throws {
        let f = try await fixture(), e = f.editor
        let base = e.revision
        e.seek(12); e.addSequenceMarker()
        try await waitForApply(f, after: base)
        let marker = e.sequenceMarkers.first
        try require(marker?.time == RationalTime(num: 1, den: 2), "marker_set stores the exact rational playhead time")
        try require(timeline(f, "marker_set").object("marker").string("color") == "red", "Default marker color is red")
        // Drag: live preview frames without service edits, one marker_move on release.
        guard let marker else { throw GUICheckError(message: "marker missing") }
        e.beginMarkerDrag(marker)
        let applies = f.transport.applyCount
        for frame in 13...20 { e.updateMarkerDrag(to: Int64(frame)) }
        try require(e.markerDrag?.frame == 20 && f.transport.applyCount == applies, "Marker drag is local until release")
        let dragBase = e.revision
        e.commitMarkerDrag()
        try await waitForApply(f, after: dragBase)
        let moved = timeline(f, "marker_move")
        try require(moved.string("marker") == marker.id && moved.object("time").string("num") == "5"
                    && moved.object("time").string("den") == "6", "marker_move carries the exact target time")
        try require(e.sequenceMarkers.first?.time == RationalTime(num: 5, den: 6), "Marker position follows the shared sequence")
        // Recolor through the same upsert; only supported colors are issued.
        let colorBase = e.revision
        e.recolorMarker(e.sequenceMarkers[0], color: "purple")
        try await waitForApply(f, after: colorBase)
        try require(e.sequenceMarkers[0].color == "purple", "Marker color persists through marker_set upsert")
        // Clip marker lands inside the clip's timeline range.
        let clipBase = e.revision
        e.selectClip(f.clips[0]); e.seek(6); e.addClipMarker(e.editClips[0])
        try await waitForApply(f, after: clipBase)
        try require(e.clipMarkers.count == 1 && e.clipMarkers[0].clip == f.clips[0]
                    && e.clipMarkers[0].time == RationalTime(num: 1, den: 4), "Clip marker stores sequence time inside the clip")
        // Select + remove.
        e.selectMarker(e.clipMarkers[0])
        let removeBase = e.revision
        e.removeMarker(e.clipMarkers[0])
        try await waitForApply(f, after: removeBase)
        try require(e.clipMarkers.isEmpty && e.markerSelection == nil, "marker_remove drops the clip marker and selection")
        let removeBase2 = e.revision
        e.removeMarker(e.sequenceMarkers[0])
        try await waitForApply(f, after: removeBase2)
        try require(e.sequenceMarkers.isEmpty, "marker_remove drops the sequence marker")
        try parity(f); await finish(f)
    }
    /// work_area_set In/Out, clearing, and the export-page default range.
    func verifyWorkArea() async throws {
        let f = try await fixture(), e = f.editor
        e.seek(12)
        var base = e.revision
        e.setInPoint()
        try await waitForApply(f, after: base)
        e.seek(36)
        base = e.revision
        e.setOutPoint()
        try await waitForApply(f, after: base)
        try require(e.workArea?.start == RationalTime(num: 1, den: 2) && e.workArea?.end == RationalTime(num: 37, den: 24),
                    "In/Out store an exact rational TimeRange (exclusive Out)")
        // Export page defaults to the work area for a sequence target.
        let page = ExportPageModel(editor: e)
        page.target = "sequence:" + f.sequence
        page.applyWorkAreaDefault()
        try require(page.rangeMode == "inout" && page.startFrame == 12 && page.endFrame == 37,
                    "Export defaults its range to the sequence work area")
        // In after Out must fail as a typed error, not a clamped surprise.
        e.seek(48); e.setInPoint()
        try require(e.failure?.code == "INVALID_CLIP", "In after Out is a typed failure")
        e.failure = nil
        base = e.revision
        e.clearWorkArea()
        try await waitForApply(f, after: base)
        try require(e.workArea == nil, "Clearing sends work_area_set null")
        try parity(f); await finish(f)
    }
    /// Snap: playhead, clip edges, and markers attract move/trim/marker targets.
    func verifySnapping() async throws {
        let f = try await fixture(), e = f.editor
        e.editSnap = true
        e.seek(30)
        let middle = e.editClips.first { $0.id == f.clips[1] }!
        // Clip-move snaps the start to the playhead and the end to a clip edge.
        e.beginClipGesture(middle, mode: .move)
        e.updateClipGesture(at: 31)
        try require(e.timelineCandidate?.start == 30, "Move snaps the start edge to the playhead")
        e.updateClipGesture(at: 44)
        try require(e.timelineCandidate?.start == 44, "Move start stays where no target is within the threshold")
        e.beginClipGesture(middle, mode: .move)
        e.updateClipGesture(at: -2)
        try require(e.timelineCandidate?.start == 0, "Move snaps to the sequence head edge")
        e.cancelClipGesture()
        // Trim end snaps to a marker added through the shared API.
        let markerBase = e.revision
        e.addSequenceMarker(at: 10, color: "blue")
        try await waitForApply(f, after: markerBase)
        let first = e.editClips.first { $0.id == f.clips[0] }!
        e.beginClipGesture(first, mode: .trimEnd)
        e.updateClipGesture(delta: -14) // end 24-14 = 10 -> snaps to the marker at 10
        try require(e.timelineCandidate?.end == 10, "Trim snaps to a sequence marker")
        e.cancelClipGesture()
        // Marker drag snaps to the playhead.
        e.beginMarkerDrag(e.sequenceMarkers[0])
        e.updateMarkerDrag(to: 31)
        try require(e.markerDrag?.frame == 30, "Marker drag snaps to the playhead")
        e.markerDrag = nil
        e.cancelClipGesture()
        try parity(f); await finish(f)
    }
    /// Boundary jump visits clip edges, markers and work-area bounds.
    func verifyBoundaryJump() async throws {
        let f = try await fixture(), e = f.editor
        var base = e.revision
        e.seek(0); e.addSequenceMarker(at: 10)
        try await waitForApply(f, after: base)
        base = e.revision
        e.seek(12); e.setInPoint()
        try await waitForApply(f, after: base)
        e.seek(0)
        e.jumpToTimelineBoundary(forward: true)
        try require(e.frame == 10, "Jump forward reaches the marker first")
        e.jumpToTimelineBoundary(forward: true)
        try require(e.frame == 12, "Jump forward reaches the In point next")
        e.jumpToTimelineBoundary(forward: true)
        try require(e.frame == 24, "Jump forward reaches the first clip end")
        e.jumpToTimelineBoundary(forward: false)
        try require(e.frame == 12, "Jump backward returns to the In point")
        try parity(f); await finish(f)
    }
}
