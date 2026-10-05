import Foundation
import KronelloAppModel
import KronelloCore

@MainActor struct EditChecks {
    struct Fixture {
        let folder: URL
        let editor: EditorModel
        let transport: RecordingTransport
        let sequence: String
        let track: String
        let clip: String
    }
    func fixture() async throws -> Fixture {
        let checks = GUIChecks(), folder = try checks.temporary(), path = folder.appendingPathComponent("edit.kronello").path
        let transport = try RecordingTransport(path: path, worker: checks.root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let editor = EditorModel(path: path, transport: transport, stateStore: .init(root: folder.appendingPathComponent("state")))
        var document = EditorModel.newDocument(name: "GUI-003 checks")
        let sequence = UUID().uuidString.lowercased(), track = UUID().uuidString.lowercased(), clip = UUID().uuidString.lowercased()
        let composition = document.objects("compositions")[0].string("id")
        document["sequences"] = [["id": sequence, "extent": ["width": 320, "height": 180], "frame_rate": ["num": "24", "den": "1"],
            "audio_rate": 48000, "working_space": "linear_rec709", "tracks": [["id": track, "kind": "video", "clips": [[
                "id": clip, "source_ref": ["kind": "composition", "composition": composition],
                "timeline_range": ["start": ["num": "0", "den": "1"], "end": ["num": "2", "den": "1"]],
                "source_in": ["num": "0", "den": "1"], "time_map": ["kind": "linear", "offset": ["num": "0", "den": "1"], "speed": ["num": "1", "den": "1"]],
                "links": [], "effects": [], "properties": []]]]]]]
        editor.ui.page = "edit"
        try await editor.start(newDocument: document)
        editor.ui.page = "edit"; try await editor.reload()
        editor.editSnap = false
        return .init(folder: folder, editor: editor, transport: transport, sequence: sequence, track: track, clip: clip)
    }
    func finish(_ f: Fixture) async { await f.editor.close(); try? FileManager.default.removeItem(at: f.folder) }
    func parity(_ f: Fixture) throws {
        let export = try GUIChecks().cli(["operation": "project.export", "project": f.editor.path])
        try require(export.string("revision") == f.editor.revision, "CLI sees the GUI revision")
        try require(NSDictionary(dictionary: export.object("document").objects("sequences")[0]) == NSDictionary(dictionary: f.editor.sequence), "GUI query and CLI read the same Sequence")
    }
    func verifyPlaceReleaseOnce() async throws {
        let f = try await fixture(), e = f.editor
        let asset = e.editAssets.first { $0.kind == .composition }!
        let plans = f.transport.planCount, applies = f.transport.applyCount
        e.beginAssetGesture(asset, track: f.track, at: 72)
        for frame in 72...80 { e.updateClipGesture(at: Int64(frame)) }
        try require(e.timelineCandidate?.start == 80 && e.editClips.count == 1, "Only candidate geometry changes during placement")
        try require(f.transport.planCount == plans && f.transport.applyCount == applies, "Placement drag issues no service edit")
        let event = await e.commitClipGesture()
        try require(event != nil && f.transport.planCount == plans + 1 && f.transport.applyCount == applies + 1, "Placement release applies exactly one planned command")
        try require(f.transport.lastApply.objects("commands").count == 1 && e.editClips.count == 2, "One placement Event adds one clip")
        try parity(f)
        let undoCount = e.undoState.undo.count
        await e.undo()
        try require(e.editClips.count == 1 && e.undoState.undo.count == undoCount - 1, "One session Undo removes the placement")
        try parity(f); await finish(f)
    }
    func verifyTrimReleaseOnce() async throws {
        let f = try await fixture(), e = f.editor, original = e.editClips[0]
        let applies = f.transport.applyCount, plans = f.transport.planCount
        e.beginClipGesture(original, mode: .trimStart)
        for delta in 1...12 { e.updateClipGesture(delta: Int64(delta)) }
        try require(e.timelineCandidate?.start == 12 && e.editClips[0].start == original.start && f.transport.applyCount == applies, "Trim preview is local")
        let event = await e.commitClipGesture()
        try require(event != nil && f.transport.planCount == plans + 1 && f.transport.applyCount == applies + 1 && f.transport.lastApply.objects("commands").count == 1, "Trim release issues one command")
        try require(e.editClips[0].start == .init(num: 1, den: 2) && RationalTime.wire(e.editClips[0].authored.object("source_in")) == .init(num: 1, den: 2), "Shared trim preserves source mapping")
        try parity(f); await e.undo()
        try require(e.editClips[0].start == original.start, "Undo restores trim in one Event")
        try parity(f); await finish(f)
    }
    func verifyBladeReleaseOnce() async throws {
        let f = try await fixture(), e = f.editor, original = e.editClips[0]
        let applies = f.transport.applyCount, plans = f.transport.planCount
        e.beginClipGesture(original, mode: .blade)
        for frame in 20...24 { e.updateClipGesture(at: Int64(frame)) }
        try require(e.timelineCandidate?.cut == 24 && e.editClips.count == 1 && f.transport.applyCount == applies, "Blade candidate issues no edit")
        let event = await e.commitClipGesture()
        try require(event != nil && f.transport.planCount == plans + 1 && f.transport.applyCount == applies + 1 && f.transport.lastApply.objects("commands").count == 1, "Blade release issues one command")
        try require(e.editClips.count == 2 && e.editClips[0].id == f.clip && e.editClips[0].end == .init(num: 1, den: 1), "Left clip keeps stable ID and split boundary")
        try require(e.editClips[1].start == .init(num: 1, den: 1) && RationalTime.wire(e.editClips[1].authored.object("source_in")) == .init(num: 1, den: 1), "Right clip starts at the exact source boundary")
        try parity(f); await e.undo()
        try require(e.editClips.count == 1 && e.editClips[0].end == original.end, "One Undo restores the unsplit clip")
        try parity(f); await finish(f)
    }
    func externalMove(_ f: Fixture) throws {
        let info = try GUIChecks().cli(["operation": "project.info", "project": f.editor.path])
        let commands = [f.editor.timelineCommand("clip_move", ["sequence": f.sequence, "clip": f.clip, "delta": ["num": "1", "den": "24"], "linked": true])]
        let plan = try GUIChecks().cli(["operation": "edit.plan", "project": f.editor.path, "base_revision": info.string("revision"), "commands": commands])
        _ = try GUIChecks().cli(["operation": "edit.apply", "project": f.editor.path, "base_revision": info.string("revision"), "commands": commands,
            "plan_hash": plan.string("plan_hash"), "session_id": UUID().uuidString, "idempotency_key": UUID().uuidString])
    }
    func verifyRevisionConflict() async throws {
        let f = try await fixture(), e = f.editor
        e.beginClipGesture(e.editClips[0], mode: .trimEnd); e.updateClipGesture(delta: -12)
        try externalMove(f)
        let event = await e.commitClipGesture()
        try require(event == nil && e.revisionConflict?.code == "REVISION_CONFLICT" && e.pendingCandidate?.commands.count == 1, "Stale release preserves explicit candidate and typed banner")
        try require(e.externalChange != nil && e.undoState.undo.isEmpty, "External changes refresh without joining session Undo")
        e.discardCandidate(); try require(e.pendingCandidate == nil && e.revisionConflict == nil, "Discard is explicit")
        e.beginClipGesture(e.editClips[0], mode: .move); e.updateClipGesture(delta: 12)
        try externalMove(f); _ = await e.commitClipGesture()
        let base = e.revision
        e.reapply(); try await MotionChecks().waitForEdit(e, after: base)
        try require(e.pendingCandidate == nil && e.revisionConflict == nil && e.undoState.undo.count == 1, "Explicit reapply uses a new plan and enters session Undo")
        try parity(f); await finish(f)
    }
    func verifyUndoConflict() async throws {
        let f = try await fixture(), e = f.editor
        e.beginClipGesture(e.editClips[0], mode: .move); e.updateClipGesture(delta: 12)
        _ = await e.commitClipGesture(); let stack = e.undoState.undo
        try externalMove(f); try await e.reload(external: true)
        let before = e.sequence
        await e.undo()
        try require(e.undoConflict?.code == "UNDO_CONFLICT" && e.undoState.undo == stack, "Typed Undo conflict preserves session stack")
        try require(NSDictionary(dictionary: e.sequence) == NSDictionary(dictionary: before), "Undo conflict never partially changes the Sequence")
        try parity(f); await finish(f)
    }
    func verifyExternalClipDeletion() async throws {
        let f = try await fixture(), e = f.editor
        e.beginAssetGesture(e.editAssets.first { $0.kind == .composition }!, track: f.track, at: 72)
        let event = await e.commitClipGesture()
        guard let event else { throw GUICheckError(message: "placement failed") }
        let placed = e.editClips.first { $0.id != f.clip }!
        e.selectClip(placed.id)
        let actor = UUID().uuidString.lowercased()
        let inverse = try GUIChecks().cli(["operation": "edit.undo", "project": e.path, "base_revision": e.revision,
            "event_id": event.string("id"), "session_id": actor, "idempotency_key": UUID().uuidString])
        try await e.reload(external: true)
        try require(e.selectedClip == nil && e.ui.clipSelection == nil && e.editClips.count == 1, "External clip deletion clears selection without selecting a replacement")
        try require(e.deletedSelection?.contains(actor) == true && e.deletedSelection?.contains("rev " + inverse.string("revision")) == true, "Deletion notice identifies the matching Event actor and revision")
        try parity(f); await finish(f)
    }
    func verifyMotionNavigation() async throws {
        let f = try await fixture(), e = f.editor, clip = e.editClips[0]
        let base = e.revision, applies = f.transport.applyCount
        e.selectClip(clip.id); e.openClipInMotion(clip)
        try require(e.ui.page == "motion" && e.ui.composition == clip.composition && e.ui.selection == nil, "Open in Motion selects the referenced Composition page")
        try await e.reload()
        try require(e.revision == base && f.transport.applyCount == applies && e.ui.sequence == f.sequence && e.ui.clipSelection == clip.id, "Navigation is UI-only and retains Edit selection")
        e.ui.page = "edit"; try await e.reload()
        try require(e.selectedClip?.id == clip.id && e.rateNum == 24 && e.extent.width == 320, "Return to Edit restores Sequence query and rate")
        await finish(f)
    }
    func verifyBatchedQueryAndSeek() async throws {
        let f = try await fixture(), e = f.editor
        let before = f.transport.callCounts["sequence.query", default: 0], sceneBefore = f.transport.callCounts["scene.query", default: 0]
        try await e.reload()
        try require(f.transport.callCounts["sequence.query", default: 0] == before + 1, "One Sequence query reads all track/clip presentation data")
        let queries = f.transport.callCounts["sequence.query", default: 0], base = e.revision
        for frame in 0..<24 { e.seek(Int64(frame)) }
        try require(f.transport.callCounts["sequence.query", default: 0] == queries && f.transport.callCounts["scene.query", default: 0] == sceneBefore, "Sequence seeking does not request per-frame scene or sequence queries")
        try require(e.revision == base && e.frame == 23, "Seek stores exact rational UI time without a document command")
        await finish(f)
    }
    func verifyProjectInventoryAndMissing() async throws {
        let checks = GUIChecks(), folder = try checks.temporary()
        defer { try? FileManager.default.removeItem(at: folder) }
        let output = folder.appendingPathComponent("demo")
        let process = Process(); process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["python3", checks.root.appendingPathComponent("scripts/demo_gui_003.py").path, "--output-root", output.path]
        let pipe = Pipe(); process.standardOutput = pipe
        try process.run(); let data = pipe.fileHandleForReading.readDataToEndOfFile(); process.waitUntilExit()
        try require(process.terminationStatus == 0, "Shared CLI demo fixture creates decodable software video/audio and missing asset: " + (String(data: data, encoding: .utf8) ?? ""))
        let path = output.appendingPathComponent("edit.kronello").path
        let transport = try RecordingTransport(path: path, worker: checks.root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let e = EditorModel(path: path, transport: transport, stateStore: .init(root: output.appendingPathComponent("user-state")))
        try await e.start()
        try require(e.ui.page == "edit" && e.editClips.count == 4, "Demo opens the actual Edit page with four Sequence clips")
        try require(e.editClips.filter { $0.kind == .video }.count == 2 && e.editClips.filter { $0.kind == .audio }.count == 1 && e.editClips.filter { $0.kind == .composition }.count == 1, "Shared ClipKind distinguishes video, audio and Composition")
        try require(e.editAssets.count == 4 && e.editAssets.filter { $0.missing == "ASSET_MISSING" }.count == 1 && e.editClips.filter { e.clipMissing($0) == "ASSET_MISSING" }.count == 1, "Project rows and timeline clips share the missing-asset diagnostic")
        try require(e.sequenceResult.objects("asset_status").filter { $0.string("availability") == "present_unverified" }.count == 2, "Present files are explicitly hash-unverified")
        try require(e.selectedClip?.composition != nil, "Demo selects a Composition clip for the Inspector")
        let before = transport.callCounts["sequence.query", default: 0]
        try await e.reload()
        try require(transport.callCounts["sequence.query", default: 0] == before + 1, "One batched query refreshes all inventory and clips")
        await e.close()
    }
    func runAll() async throws {
        try await verifyPlaceReleaseOnce(); print("PASS edit place release + Undo + CLI parity")
        try await verifyTrimReleaseOnce(); print("PASS edit trim release + Undo + CLI parity")
        try await verifyBladeReleaseOnce(); print("PASS edit blade release + Undo + CLI parity")
        try await verifyRevisionConflict(); print("PASS edit revision conflict discard + reapply")
        try await verifyUndoConflict(); print("PASS edit Undo conflict")
        try await verifyExternalClipDeletion(); print("PASS edit external clip deletion and Event attribution")
        try await verifyMotionNavigation(); print("PASS edit open in Motion navigation")
        try await verifyBatchedQueryAndSeek(); print("PASS edit batched query and query-free Sequence seek")
        try await verifyProjectInventoryAndMissing(); print("PASS edit Project inventory, shared kind and ASSET_MISSING")
    }
}
