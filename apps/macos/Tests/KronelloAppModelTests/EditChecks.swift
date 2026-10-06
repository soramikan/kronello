import Foundation
import AppKit
import KronelloAppModel
import KronelloCore
import KronelloDesign

@MainActor struct EditChecks {
    struct Fixture {
        let folder: URL
        let editor: EditorModel
        let transport: RecordingTransport
        let sequence: String
        let track: String
        let clip: String
    }
    func fixture(includeAudio: Bool = false) async throws -> Fixture {
        let checks = GUIChecks(), folder = try checks.temporary(), path = folder.appendingPathComponent("edit.kronello").path
        let transport = try RecordingTransport(path: path, worker: checks.root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let editor = EditorModel(path: path, transport: transport, stateStore: .init(root: folder.appendingPathComponent("state")))
        var document = EditorModel.newDocument(name: "GUI-003 checks")
        if includeAudio {
            document["assets"] = [["id": UUID().uuidString.lowercased(), "content_hash": String(repeating: "0", count: 64),
                "kind": "audio", "locator": ["relative": "audio.wav", "absolute": folder.appendingPathComponent("audio.wav").path],
                "streams": [["index": 0, "codec": "pcm_s16le", "time_base": ["num": "1", "den": "48000"], "duration": ["num": "2", "den": "1"]]]]]
        }
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
        e.fonts = try JSONSerialization.jsonObject(with: Data(contentsOf: output.appendingPathComponent("fonts.json"))) as! [[String: Any]]
        try await e.start()
        try require(e.ui.page == "edit" && e.editClips.count == 4, "Demo opens the actual Edit page with four Sequence clips")
        try require(e.editClips.filter { $0.kind == .video }.count == 2 && e.editClips.filter { $0.kind == .audio }.count == 1 && e.editClips.filter { $0.kind == .composition }.count == 1, "Shared ClipKind distinguishes video, audio and Composition")
        try require(e.editAssets.count == 4 && e.editAssets.filter { $0.missing == "ASSET_MISSING" }.count == 1 && e.editClips.filter { e.clipMissing($0) == "ASSET_MISSING" }.count == 1, "Project rows and timeline clips share the missing-asset diagnostic")
        try require(e.sequenceResult.objects("asset_status").filter { $0.string("availability") == "present_unverified" }.count == 2, "Present files are explicitly hash-unverified")
        try require(e.selectedClip?.composition != nil, "Demo selects a Composition clip for the Inspector")
        let input: [String: Any] = ["project": path, "target": ["kind": "sequence", "sequence": e.ui.sequence!],
            "fonts": e.fonts, "region": ["origin": [0, 0], "extent": [320, 180], "pixels": [32, 18]]]
        let request: [String: Any] = ["operation": "render.frame", "input": input, "time": ["num": "0", "den": "1"], "backend": "cpu_reference"]
        let native = try await transport.call(request)
        var cliRequest = request; cliRequest.removeValue(forKey: "backend")
        let cli = try checks.cli(cliRequest, arguments: ["--backend", "cpu-reference"])
        try require(native.object("metadata").string("backend") == "cpu_reference_float32" && native.object("metadata").string("input_path").contains("software_video_decode"), "Explicit CPU request uses the shared video decode path")
        try require(NSDictionary(dictionary: native) == NSDictionary(dictionary: cli), "Native FFI and CLI render.frame CPU pixels and metadata match exactly")
        let before = transport.callCounts["sequence.query", default: 0]
        try await e.reload()
        try require(transport.callCounts["sequence.query", default: 0] == before + 1, "One batched query refreshes all inventory and clips")
        await e.close()
    }
    func verifyReviewPresentation() async throws {
        let f = try await fixture(), e = f.editor
        try require(e.editClips[0].speedLabel == "100.0%" && !e.editClips[0].reversed, "String rational 1x displays 100.0%")
        var clip = e.editClips[0].authored
        clip["time_map"] = ["kind": "linear", "offset": ["num": "0", "den": "1"], "speed": ["num": "3", "den": "2"]]
        clip["reverse_sampling"] = "reverse_grid_v1"
        let reverse = EditClip(query: ["clip": clip])
        try require(reverse.speedLabel == "150.0%" && reverse.reversed, "Reverse flag shows the exact rational magnitude")
        try require(EditPresentation.rateLabel(num: 24, den: 1) == "24 fps" && EditPresentation.rateLabel(num: 24000, den: 1001) == "23.976 fps", "Integer and NTSC header format")
        try require(EditPresentation.rulerLabel(frame: 12, fps: 24) == "0s12f" && EditPresentation.rulerLabel(frame: 24, fps: 24) == "1s0f", "Ruler keeps subsecond frame units")
        let old = e.previewIdentity
        e.reportPreviewFailure(.init(code: "UNSUPPORTED_FEATURE", message: "video requires explicit media backend"), for: old)
        try require(e.offersCPUReference && !e.usesCPUReference, "Video unsupported offers explicit action without silent fallback")
        e.chooseCPUReference()
        try require(e.usesCPUReference && e.previewFailure == nil && e.revision == old.revision, "CPU preference is session UI state only")
        e.playing = true; try require(e.previewStale, "CPU playback displays an explicit stale frame state")
        e.openClipInMotion(e.editClips[0]); e.reportPreviewFailure(.init(code: "UNSUPPORTED_FEATURE", message: "late video failure"), for: old)
        try require(e.previewFailure == nil && !e.usesCPUReference, "Target change clears and rejects late Sequence errors")
        e.ui.page = "edit"; try require(e.usesCPUReference, "CPU choice persists only for the chosen Sequence tab in session")
        let beforeSeek = e.previewIdentity
        e.previewFailure = .init(code: "ASSET_MISSING", message: "test")
        e.seek(1); try require(e.previewFailure == nil, "Errors are keyed to exact time")
        e.reportPreviewFailure(.init(code: "ASSET_MISSING", message: "late"), for: beforeSeek)
        try require(e.previewFailure == nil, "Superseded time error is discarded")
        await finish(f)
    }
    func verifyBladeMouseHitPath() async throws {
        let f = try await fixture(), e = f.editor, clip = e.editClips[0]
        let view = KRBladeHitView(frame: NSRect(x: 0, y: 0, width: 200, height: 36))
        var releases = 0
        view.begin = { fraction in e.beginClipGesture(clip, mode: .blade); e.updateClipGesture(at: e.bladeFrame(clip, fraction: fraction)) }
        view.update = { e.updateClipGesture(at: e.bladeFrame(clip, fraction: $0)) }
        view.release = { releases += 1; Task { await e.commitClipGesture() } }
        try require(view.hitTest(NSPoint(x: 100, y: 18)) === view, "Blade hit area receives the raw clip point")
        let down = NSEvent.mouseEvent(with: .leftMouseDown, location: NSPoint(x: 100, y: 18), modifierFlags: [], timestamp: 0, windowNumber: 0, context: nil, eventNumber: 1, clickCount: 1, pressure: 1)!
        let up = NSEvent.mouseEvent(with: .leftMouseUp, location: NSPoint(x: 100, y: 18), modifierFlags: [], timestamp: 0.01, windowNumber: 0, context: nil, eventNumber: 2, clickCount: 1, pressure: 0)!
        let base = e.revision, applies = f.transport.applyCount
        view.mouseDown(with: down)
        try require(e.timelineCandidate?.cut == 24 && f.transport.applyCount == applies, "Mouse down displays the candidate without applying")
        view.mouseUp(with: up); view.mouseUp(with: up)
        try await MotionChecks().waitForEdit(e, after: base)
        try require(releases == 1 && f.transport.applyCount == applies + 1 && e.editClips.count == 2, "Zero-motion mouse click issues exactly one split; duplicate up is ignored")
        await e.undo(); let axBase = e.revision
        try require(view.accessibilityPerformPress(), "Blade accessibility press is handled by the same hit area")
        try await MotionChecks().waitForEdit(e, after: axBase)
        try require(releases == 2 && f.transport.applyCount == applies + 2 && e.editClips.count == 2, "Accessibility press issues exactly one split at the clip midpoint")
        view.enabled = false; try require(!view.accessibilityPerformPress(), "Disabled hit area rejects accessibility edits")
        await finish(f)
    }
    func verifyClipDragMouseHitPath() async throws {
        let f = try await fixture(), e = f.editor, clip = e.editClips[0]
        e.editSnap = false
        let view = KRClipHitView(frame: NSRect(x: 0, y: 0, width: 200, height: 36))
        var releases = 0
        view.select = { e.selectClip(clip.id) }
        view.open = { e.openClipInMotion(clip) }
        view.begin = { e.beginClipGesture(clip, mode: $0 == .move ? .move : $0 == .trimStart ? .trimStart : .trimEnd) }
        view.update = { e.updateClipGesture(delta: Int64(($0 / 2).rounded())) }
        view.release = { releases += 1; Task { await e.commitClipGesture() } }
        view.cancel = e.cancelClipGesture
        func event(_ kind: NSEvent.EventType, _ x: Double, clicks: Int = 1) -> NSEvent {
            NSEvent.mouseEvent(with: kind, location: NSPoint(x: x, y: 18), modifierFlags: [], timestamp: 0,
                windowNumber: 0, context: nil, eventNumber: 1, clickCount: clicks, pressure: kind == .leftMouseUp ? 0 : 1)!
        }
        try require(view.hitTest(NSPoint(x: 100, y: 18)) === view, "Clip body routes raw points to the native drag receiver")
        let applies = f.transport.applyCount, base = e.revision
        view.mouseDown(with: event(.leftMouseDown, 100))
        view.mouseDragged(with: event(.leftMouseDragged, 102))
        try require(e.timelineCandidate == nil && e.ui.clipSelection == clip.id, "Click/subthreshold motion only selects")
        view.mouseDragged(with: event(.leftMouseDragged, 148))
        try require(e.timelineCandidate?.start == 24 && f.transport.applyCount == applies, "Move candidate stays local")
        view.setFrameOrigin(NSPoint(x: 48, y: 0))
        view.mouseDragged(with: event(.leftMouseDragged, 160))
        try require(e.timelineCandidate?.start == 30, "Moving the preview does not change the press coordinate origin")
        view.mouseUp(with: event(.leftMouseUp, 160)); view.mouseUp(with: event(.leftMouseUp, 160))
        try await MotionChecks().waitForEdit(e, after: base)
        try require(releases == 1 && f.transport.applyCount == applies + 1 && e.editClips[0].start.frames(rateNum: 24, rateDen: 1) == 30,
                    "Native drag release applies exactly one shared move command")
        await e.undo()
        let trimBase = e.revision, beforeTrim = f.transport.applyCount
        view.setFrameOrigin(.zero)
        view.mouseDown(with: event(.leftMouseDown, 1)); view.mouseDragged(with: event(.leftMouseDragged, 13))
        try require(e.timelineCandidate?.mode == .trimStart && e.timelineCandidate?.start == 6, "Leading six points route to trim")
        view.setFrameOrigin(NSPoint(x: 12, y: 0)); view.mouseUp(with: event(.leftMouseUp, 25))
        try await MotionChecks().waitForEdit(e, after: trimBase)
        try require(releases == 2 && f.transport.applyCount == beforeTrim + 1 && e.editClips[0].start.frames(rateNum: 24, rateDen: 1) == 12,
                    "Trim release retains its original press origin and applies once")
        await e.undo()
        view.setFrameOrigin(.zero)
        let beforeClick = f.transport.applyCount
        view.mouseDown(with: event(.leftMouseDown, 100)); view.mouseUp(with: event(.leftMouseUp, 101))
        try require(releases == 2 && f.transport.applyCount == beforeClick && e.timelineCandidate == nil, "Click never emits an edit")
        view.mouseDown(with: event(.leftMouseDown, 100, clicks: 2)); view.mouseUp(with: event(.leftMouseUp, 100))
        try require(e.ui.page == "motion" && releases == 2 && f.transport.applyCount == beforeClick, "Double click navigates without moving the clip")
        view.enabled = false
        try require(!view.accessibilityPerformPress(), "Disabled native hit receiver rejects accessibility selection")
        await finish(f)
    }
    func verifyAssetPlacementReceiver() async throws {
        let f = try await fixture(includeAudio: true), e = f.editor
        let receiver = AssetPlacementReceiver(model: e, track: f.track)
        try require(!receiver.canAccept, "No asset selection is not a drop destination")
        let asset = e.editAssets.first { $0.kind == .composition }!
        e.assetSelection = asset.id
        let plans = f.transport.planCount, applies = f.transport.applyCount
        // Enter and release can occur without an intermediate dropUpdated callback.
        try require(receiver.update(at: 72) && receiver.update(at: 80), "Entered and final release positions both reach the production receiver")
        try require(e.timelineCandidate?.start == 80 && f.transport.planCount == plans && f.transport.applyCount == applies,
                    "Receiver movement only changes the candidate")
        let committed = await receiver.commit()
        try require(committed, "Receiver release commits through the shared service")
        try require(f.transport.planCount == plans + 1 && f.transport.applyCount == applies + 1 && f.transport.lastApply.objects("commands").count == 1,
                    "Receiver release emits exactly one command and Event")
        let repeated = await receiver.commit()
        try require(!repeated, "Repeated release does not duplicate the Event")
        try parity(f); await e.undo()
        try require(e.editClips.count == 1, "One Undo removes the placement")
        let afterUndo = f.transport.applyCount
        // A drop with no entered/updated callbacks still prepares at its final point.
        try require(receiver.update(at: 120) && e.timelineCandidate?.start == 120, "Final-position fallback creates a candidate")
        receiver.exit()
        try require(e.timelineCandidate == nil && f.transport.applyCount == afterUndo, "Exiting cancels only the local placement")
        e.ui.locked.insert(f.track)
        try require(!receiver.canAccept && !receiver.update(at: 140), "Locked tracks reject placement before service mutation")
        e.ui.locked.remove(f.track)
        e.beginClipGesture(e.editClips[0], mode: .move)
        try require(!receiver.update(at: 140) && e.timelineCandidate?.mode == .move, "A foreign edit candidate is neither accepted nor overwritten")
        receiver.exit()
        try require(e.timelineCandidate?.mode == .move, "Drop exit does not cancel a different edit")
        e.cancelClipGesture()
        try require(receiver.update(at: 140), "Valid placement can start after a different edit is canceled")
        let otherTrack = AssetPlacementReceiver(model: e, track: UUID().uuidString.lowercased())
        try require(!otherTrack.update(at: 160) && e.timelineCandidate?.track == f.track, "Another destination cannot consume the current track's candidate")
        otherTrack.exit()
        try require(e.timelineCandidate != nil, "Exit from another destination preserves the candidate")
        receiver.exit()
        e.assetSelection = e.editAssets.first { $0.kind == .audio }!.id
        try require(!receiver.update(at: 140) && e.timelineCandidate == nil && f.transport.applyCount == afterUndo,
                    "Audio asset to video track is forbidden without a candidate or service edit")
        await finish(f)
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
        try await verifyReviewPresentation(); print("PASS edit rational presentation, keyed preview errors and explicit CPU session choice")
        try await verifyBladeMouseHitPath(); print("PASS edit real NSEvent hit path and accessibility blade press = one command")
        try await verifyClipDragMouseHitPath(); print("PASS clip move/trim native NSEvent hit path, stable origin, click and release-once")
        try await verifyAssetPlacementReceiver(); print("PASS drop receiver final-position candidate, release-once, Undo and CLI parity")
    }
}
