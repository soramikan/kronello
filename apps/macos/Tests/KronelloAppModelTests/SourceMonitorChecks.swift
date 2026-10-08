import Foundation
import KronelloAppModel
import KronelloCore
import KronelloDesign

/// Model-level checks for GUI-011 (source/program dual monitor, three-point
/// insert/overwrite) and NLE-007 (multicam create/angle switching). All
/// assertions run against the real shared Command/Query worker through
/// `RecordingTransport`, so request payloads are validated by the service
/// schema (strict `additionalProperties`) and land in the document.
@MainActor struct SourceMonitorChecks {
    struct Fixture {
        let folder: URL
        let editor: EditorModel
        let transport: RecordingTransport
        let sequence: String
        let videoA: String   // 30 s, stream 0 video 1920x1080 + stream 1 audio
        let videoB: String   // 20 s, stream 0 video 1280x720, start_time 2 s
        let audio: String    // 30 s audio-only asset
        let track1: String   // video target (V1)
        let track2: String   // second video track (V2)
        let audioTrack: String
        let composition: String
    }
    private static func asset(_ id: String, hash: String, kind: String, name: String, streams: [[String: Any]]) -> [String: Any] {
        ["id": id, "kind": kind, "content_hash": String(repeating: hash, count: 64),
         "locator": ["relative": name],
         "streams": streams]
    }
    /// Two fake-locator video assets (only stream metadata matters — no file
    /// probe is needed for the editing operations under test), one audio
    /// asset, and a sequence with a video/audio target pair.
    func fixture(existingClip: Bool = true) async throws -> Fixture {
        let checks = GUIChecks(), folder = try checks.temporary(), path = folder.appendingPathComponent("edit.kronello").path
        let transport = try RecordingTransport(path: path, worker: checks.root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let editor = EditorModel(path: path, transport: transport, stateStore: .init(root: folder.appendingPathComponent("state")))
        var document = EditorModel.newDocument(name: "GUI-011 checks")
        let sequence = UUID().uuidString.lowercased()
        let track1 = UUID().uuidString.lowercased(), track2 = UUID().uuidString.lowercased(), audioTrack = UUID().uuidString.lowercased()
        let videoA = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", videoB = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
        let audio = "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
        // The store canonicalizes UUIDs to lowercase; keep the same spelling.
        let composition = document.objects("compositions")[0].string("id").lowercased()
        document["assets"] = [
            Self.asset(videoA, hash: "a", kind: "video", name: "cam_a.mov", streams: [
                ["index": 0, "codec": "prores", "time_base": ["num": "1", "den": "24"], "duration": ["num": "30", "den": "1"], "start_time": ["num": "0", "den": "1"], "width": 1920, "height": 1080],
                ["index": 1, "codec": "pcm_s16le", "time_base": ["num": "1", "den": "48000"], "duration": ["num": "30", "den": "1"], "start_time": ["num": "0", "den": "1"]],
            ]),
            Self.asset(videoB, hash: "b", kind: "video", name: "cam_b.mov", streams: [
                ["index": 0, "codec": "prores", "time_base": ["num": "1", "den": "24"], "duration": ["num": "20", "den": "1"], "start_time": ["num": "2", "den": "1"], "width": 1280, "height": 720],
            ]),
            Self.asset(audio, hash: "c", kind: "audio", name: "voice.wav", streams: [
                ["index": 0, "codec": "pcm_s16le", "time_base": ["num": "1", "den": "48000"], "duration": ["num": "30", "den": "1"], "start_time": ["num": "0", "den": "1"]],
            ]),
        ]
        var clips: [[String: Any]] = []
        if existingClip {
            // A 3 s composition clip covering [0, 3) for overwrite coverage.
            clips.append(["id": UUID().uuidString.lowercased(), "source_ref": ["kind": "composition", "composition": composition],
                "timeline_range": ["start": ["num": "0", "den": "1"], "end": ["num": "3", "den": "1"]],
                "source_in": ["num": "0", "den": "1"],
                "time_map": ["kind": "linear", "offset": ["num": "0", "den": "1"], "speed": ["num": "1", "den": "1"]],
                "links": [String](), "effects": [[String: Any]](), "properties": [[String: Any]](), "markers": [[String: Any]]()])
        }
        document["sequences"] = [["id": sequence, "extent": ["width": 320, "height": 180], "frame_rate": ["num": "24", "den": "1"],
            "audio_rate": 48000, "working_space": "linear_rec709", "markers": [[String: Any]](),
            "targets": ["video": track1, "audio": audioTrack],
            "tracks": [["id": track1, "kind": "video", "clips": clips],
                       ["id": track2, "kind": "video", "clips": [[String: Any]]()],
                       ["id": audioTrack, "kind": "audio", "clips": [[String: Any]]()]]]]
        editor.ui.page = "edit"
        try await editor.start(newDocument: document)
        editor.ui.page = "edit"; try await editor.reload()
        return .init(folder: folder, editor: editor, transport: transport, sequence: sequence,
                     videoA: videoA, videoB: videoB, audio: audio,
                     track1: track1, track2: track2, audioTrack: audioTrack, composition: composition)
    }
    func finish(_ f: Fixture) async { await f.editor.close(); try? FileManager.default.removeItem(at: f.folder) }
    func waitForApply(_ f: Fixture, after base: String) async throws {
        try await MotionChecks().waitForEdit(f.editor, after: base)
    }
    private func clips(on f: Fixture, track: String) -> [[String: Any]] {
        f.editor.sequence.objects("tracks").first { $0.string("id") == track }?.objects("clips") ?? []
    }
    private func assetA(_ f: Fixture) -> EditAsset? {
        f.editor.editAssets.first { $0.source["asset"] as? String == f.videoA && ($0.source["stream_index"] as? Int) == 0 }
    }

    /// `SourcePreview` narrows every previewable `source_ref` kind, keeps full
    /// identity on the wire and rejects generator/caption/adjustment refs.
    func verifySourcePreviewWire() throws {
        try require(SourcePreview.asset("a1").wire["kind"] as? String == "asset"
                    && SourcePreview.asset("a1").wire["stream_index"] as? Int == 0, "Whole-asset preview defaults to stream 0")
        let asset = SourcePreview.fromSourceRef(["kind": "asset", "asset": "a1", "stream_index": 2])
        try require(asset == .asset("a1", 2) && asset?.key == "asset:a1:2", "Asset ref keeps stream index identity")
        let multicam = SourcePreview.fromSourceRef(["kind": "multicam", "multicam": "g1", "angle": "a2"])
        try require(multicam == .multicam("g1", "a2") && multicam?.wire["angle"] as? String == "a2", "Multicam ref keeps group and angle identity")
        let comp = SourcePreview.fromSourceRef(["kind": "composition", "composition": "c1"])
        try require(comp == .composition("c1"), "Composition ref narrows")
        for ref in [["kind": "generator", "generator": "solid"], ["kind": "caption", "caption": "x"], ["kind": "adjustment"], ["kind": "unknown"]] as [[String: Any]] {
            try require(SourcePreview.fromSourceRef(ref) == nil, "Non-previewable source_ref returns nil")
        }
        try require(PlaybackTarget.source(.asset("a1", 0)).wire["kind"] as? String == "source",
                    "Playback target carries RenderTarget::Source wire shape")
        try require(PreviewSurface.program == 0 && PreviewSurface.source == 1 && PreviewSurface.export == 2,
                    "Program/source/export occupy distinct surface slots")
    }

    /// Source choices, window math (start_time shift, audio kind routing),
    /// monitor transport and In/Out marking in source-local time.
    func verifySourceOpenMarkingAndWindow() async throws {
        let f = try await fixture()
        let e = f.editor
        // Choices: 4 asset streams + 1 composition (no multicam yet).
        let choices = e.sourceChoices()
        try require(choices.count == 5 && choices.allSatisfy { $0.kind != .multicam }, "Choices list every stream and composition")
        guard let assetA = assetA(f) else { throw GUICheckError(message: "video asset row missing") }
        e.openAssetInSource(assetA)
        try require(e.sourceMonitor?.source == .asset(f.videoA, 0), "Monitor loads the asset stream")
        try require(e.sourceFrame == 0 && e.sourceDurationFrames == 720, "Window starts at stream start, 30 s at 24 fps")
        try require(e.sourceExtent(for: .asset(f.videoA, 0)) == CGSize(width: 1920, height: 1080), "Extent comes from stream pixels")
        // An unsupported media backend on the source surface offers the same
        // explicit CPU-reference retry as the program monitor.
        try require(!e.offersSourceCPUReference && !e.usesSourceCPUReference, "No CPU offer before a failure")
        e.reportSourcePreviewFailure(.init(code: "UNSUPPORTED_FEATURE", message: "video requires explicit media backend"), for: e.sourcePreviewIdentity)
        try require(e.offersSourceCPUReference, "Unsupported backend offers the source CPU fallback")
        e.chooseSourceCPUReference()
        try require(e.usesSourceCPUReference && e.sourcePreviewFailure == nil, "Choosing CPU clears the source failure")
        e.reportSourcePreviewFailure(.init(code: "UNSUPPORTED_FEATURE", message: "video requires explicit media backend"), for: e.sourcePreviewIdentity)
        try require(!e.offersSourceCPUReference, "No repeated offer once CPU is chosen")
        // The opt-in is scoped to the preview target: another source does not
        // inherit it, and returning to the chosen source keeps it.
        if let assetB = e.editAssets.first(where: { $0.source["asset"] as? String == f.videoB }) {
            e.openAssetInSource(assetB)
            try require(!e.usesSourceCPUReference, "CPU choice does not leak to another source")
            e.openAssetInSource(assetA)
            try require(e.usesSourceCPUReference, "CPU choice stays per source target")
        }
        e.sourcePreviewFailure = nil
        // Marking: In at 1 s, Out at 2 s on the sequence's 24 fps grid.
        e.seekSourceFrame(24); e.setSourceInPoint()
        try require(e.sourceMonitor?.inPoint == RationalTime(num: 1, den: 1), "In stores source-local rational time")
        e.seekSourceFrame(48); e.setSourceOutPoint()
        try require(e.sourceMonitor?.outPoint == RationalTime(num: 2, den: 1)
                    && e.sourceEditRange?.start == RationalTime(num: 1, den: 1)
                    && e.sourceEditRange?.end == RationalTime(num: 2, den: 1), "Edit range is [in, out)")
        // In after Out clears the stale Out instead of clamping silently.
        e.seekSourceFrame(72); e.setSourceInPoint()
        try require(e.sourceMonitor?.outPoint == nil && e.sourceEditRange == nil, "In past Out clears the range")
        e.clearSourcePoints()
        try require(e.sourceMonitor?.inPoint == RationalTime(num: 0, den: 1)
                    && e.sourceMonitor?.outPoint == RationalTime(num: 30, den: 1), "Clear restores the whole window")
        e.closeSourceMonitor()
        try require(e.sourceMonitor == nil, "Close drops the monitor")
        // A missing source surfaces a typed failure, not an empty monitor.
        e.openSource(.asset("dddddddd-dddd-4ddd-8ddd-dddddddddddd", 0), name: "ghost")
        try require(e.sourceMonitor == nil && e.failure?.code == "SOURCE_MISSING", "Missing source is a typed failure")
        e.failure = nil
        // Audio sources resolve the audio destination track by kind.
        guard let audioAsset = e.editAssets.first(where: { $0.source["asset"] as? String == f.audio }) else {
            throw GUICheckError(message: "audio asset row missing")
        }
        e.openAssetInSource(audioAsset)
        try require(e.sourceIsAudio && e.sourceDestinationTrack == f.audioTrack, "Audio source targets the audio target track")
        try require(e.sourceDestinationTracks().count == 1, "Destination choices narrow to audio tracks")
        e.setSourceDestination(f.track1)
        try require(e.sourceDestinationTrack == f.track1, "Explicit destination overrides the sequence target")
        e.setSourceDestination(nil)
        // Clip match-frame: playhead inside the composition clip opens its
        // source at the clip-local time.
        e.closeSourceMonitor(); e.seek(36)
        guard let clip = e.editClips.first else { throw GUICheckError(message: "clip missing") }
        e.openClipInSource(clip)
        try require(e.sourceMonitor?.source == .composition(f.composition) && e.sourceFrame == 36,
                    "Clip open lands on match-framed source time; got \(String(describing: e.sourceMonitor?.source)) want \(f.composition) frame \(e.sourceFrame) failure \(e.failure?.message ?? "")")
        e.closeSourceMonitor()
        await finish(f)
    }

    /// `edit.insert` / `edit.overwrite` carry the three-point window through
    /// the shared API; insert ripples, overwrite covers/splits, and the
    /// destination override reaches the request.
    func verifyInsertOverwriteThreePoint() async throws {
        let f = try await fixture()
        let e = f.editor
        guard let assetA = assetA(f) else { throw GUICheckError(message: "video asset row missing") }
        e.openAssetInSource(assetA)
        e.seekSourceFrame(24); e.setSourceInPoint()
        e.seekSourceFrame(48); e.setSourceOutPoint()
        // Insert the [1 s, 2 s) window at the head (t = 0): the covering
        // composition clip is not straddled and ripples right by 1 s.
        e.seek(0)
        var base = e.revision
        e.insertSource()
        try await waitForApply(f, after: base)
        let insert = f.transport.lastCalls["edit.insert"] ?? [:]
        try require(insert.string("sequence") == f.sequence, "insert targets the active sequence")
        try require(UUID(uuidString: insert.string("clip")) != nil, "insert carries a caller-chosen clip UUID")
        try require(insert.object("source").string("kind") == "asset"
                    && insert.object("source").string("asset") == f.videoA
                    && (insert.object("source")["stream_index"] as? Int) == 0, "insert source ref keeps asset/stream")
        try require(insert.object("source_range").object("start").string("num") == "1"
                    && insert.object("source_range").object("end").string("num") == "2",
                    "insert carries the exact source-local window")
        try require(insert.object("at").string("num") == "0", "insert lands at the playhead (third point)")
        try require(insert["linked"] as? Bool == true, "insert ripples reciprocal links")
        try require(clips(on: f, track: f.track1).count == 2, "Insert adds one clip to the target track")
        let newClip = e.editClips.first { $0.authored.object("source_ref").string("kind") == "asset" }
        try require(newClip?.start == RationalTime(num: 0, den: 1) && newClip?.end == RationalTime(num: 1, den: 1),
                    "Inserted clip occupies [at, at+range)")
        let rippled = e.editClips.first { $0.composition == f.composition }
        try require(rippled?.start == RationalTime(num: 1, den: 1),
                    "Insert ripples the covering clip; got \(String(describing: rippled?.start)) clips=\(e.editClips.map { $0.id + ":" + $0.authored.object("source_ref").string("kind") + "@" + $0.start.num + "/" + $0.start.den })")
        // Explicit destination override lands on V2 via the `track` field.
        e.setSourceDestination(f.track2)
        e.seek(144) // t = 6 s, clear of existing content
        base = e.revision
        e.overwriteSource()
        try await waitForApply(f, after: base)
        var overwrite = f.transport.lastCalls["edit.overwrite"] ?? [:]
        try require(overwrite.string("track") == f.track2, "Explicit destination reaches the request")
        try require(UUID(uuidString: overwrite.string("split_tail")) != nil, "Overwrite carries the split-tail id")
        try require(overwrite["linked"] == nil, "Overwrite omits the insert-only linked field")
        try require(clips(on: f, track: f.track2).count == 1, "Overwrite lands on the explicit track")
        // A middle overwrite of the rippled composition clip [1, 4) splits it:
        // the head keeps identity, the tail takes the caller-supplied id.
        e.setSourceDestination(nil)
        e.seek(36) // t = 1.5 s; overwrite window [1.5, 2.5)
        base = e.revision
        e.overwriteSource()
        try await waitForApply(f, after: base)
        overwrite = f.transport.lastCalls["edit.overwrite"] ?? [:]
        let tail = overwrite.string("split_tail")
        let split = e.editClips.first { $0.id == tail }
        try require(split?.start == RationalTime(num: 5, den: 2) && split?.end == RationalTime(num: 4, den: 1),
                    "Middle overwrite splits the covered clip with the stable tail id")
        try require(clips(on: f, track: f.track1).count == 4, "Head, tail, source and prior insert all remain")
        // Insert with an empty source range is a typed failure, not a guess.
        e.seekSourceFrame(72); e.setSourceInPoint()
        e.insertSource()
        try require(e.failure?.code == "INVALID_CLIP", "Empty range rejects insert client-side")
        e.failure = nil
        await finish(f)
    }

    /// `multicam.create` persists caller-chosen angle ids and manual offsets,
    /// the monitor previews a chosen angle, insertion lands a multicam clip
    /// and `clip.angle_switch` repoints only that clip.
    func verifyMulticamCreatePreviewAndSwitch() async throws {
        let f = try await fixture(existingClip: false)
        let e = f.editor
        let angleA = UUID().uuidString.lowercased(), angleB = UUID().uuidString.lowercased()
        var base = e.revision
        e.createMulticam(name: "Interview", sync: "manual",
            angles: [["id": angleA, "asset": f.videoA, "stream_index": 0, "name": "A-cam"],
                     ["id": angleB, "asset": f.videoB, "stream_index": 0, "name": "B-cam"]],
            reference: nil, offsets: [angleA: RationalTime(num: 0, den: 1), angleB: RationalTime(num: 2, den: 1)])
        try await waitForApply(f, after: base)
        let create = f.transport.lastCalls["multicam.create"] ?? [:]
        try require(create.string("sync") == "manual" && create.object("offsets").object(angleB).string("num") == "2",
                    "Create carries sync mode and per-angle offsets")
        try require(create.objects("angles").map { $0.string("id") } == [angleA, angleB], "Angle order and ids are caller-chosen")
        guard let group = e.multicamGroups.first else { throw GUICheckError(message: "multicam group not persisted") }
        try require(group.name == "Interview" && group.angles.count == 2
                    && group.angles[1].id == angleB && group.angles[1].syncOffset == RationalTime(num: 2, den: 1),
                    "Persisted group keeps order, names and offsets")
        // The group appears as one placeable multicam row in the bin.
        guard let row = e.editAssets.first(where: { $0.kind == .multicam }) else {
            throw GUICheckError(message: "multicam bin row missing")
        }
        try require(row.source["multicam"] as? String == group.id, "Bin row keeps group identity")
        try require(e.sourceChoices().filter { $0.kind == .multicam }.count == 2,
                    "Source choices list each multicam angle")
        // Opening the row loads angle A (the first authored angle); the B
        // angle's 2 s sync offset shifts its stream's 2 s start_time to 0.
        e.openAssetInSource(row)
        try require(e.sourceMonitor?.source == .multicam(group.id, angleA), "Multicam opens its first angle")
        e.previewSourceAngle(angleB)
        try require(e.sourceMonitor?.source == .multicam(group.id, angleB) && e.sourceFrame == 0,
                    "Angle preview applies the sync offset window")
        try require(e.sourceDurationFrames == 480, "B-cam window is 20 s after the offset")
        // Insert the previewed B angle as a multicam clip.
        e.setSourceInPoint(); e.seekSourceFrame(48); e.setSourceOutPoint() // [0, 2 s)
        e.seek(0)
        base = e.revision
        e.insertSource()
        try await waitForApply(f, after: base)
        let insert = f.transport.lastCalls["edit.insert"] ?? [:]
        try require(insert.object("source").string("kind") == "multicam"
                    && insert.object("source").string("multicam") == group.id
                    && insert.object("source").string("angle") == angleB, "Insert pins the previewed angle")
        guard let clip = e.editClips.first(where: { $0.multicam != nil }) else {
            throw GUICheckError(message: "multicam clip not placed")
        }
        try require(clip.multicam?.angle == angleB && e.clipName(clip) == "Interview · B-cam",
                    "Clip follows the previewed angle identity and name")
        // Angle switching repoints only this clip through the shared API.
        base = e.revision
        e.switchClipAngle(clip, to: angleA)
        try await waitForApply(f, after: base)
        let switched = f.transport.lastCalls["clip.angle_switch"] ?? [:]
        try require(switched.string("sequence") == f.sequence && switched.string("clip") == clip.id
                    && switched.string("angle") == angleA, "clip.angle_switch targets one clip")
        try require(e.editClips.first { $0.id == clip.id }?.multicam?.angle == angleA
                    && e.clipName(e.editClips.first { $0.id == clip.id }!) == "Interview · A-cam",
                    "Clip's active angle changed")
        // TRACK-002: a multicam clip's trackable source resolves through the
        // active angle (asset + stream + sync offset), and the kind gate lets
        // track.analyze through — the request then fails typed on the fake
        // locator, proving it reached the service rather than a local guard.
        let switchedClip = e.editClips.first { $0.id == clip.id }!
        try require(e.trackableSource(switchedClip)?.asset == f.videoA
                    && e.trackableSource(switchedClip)?.stream == 0
                    && e.trackableSource(switchedClip)?.offset == RationalTime(num: 0, den: 1),
                    "Multicam clip resolves the active angle's media")
        e.stabilizeClip(switchedClip)
        for _ in 0..<200 where !e.stabilizePending.isEmpty {
            try await Task.sleep(for: .milliseconds(10))
        }
        let analyze = f.transport.lastCalls["track.analyze"] ?? [:]
        try require(analyze.string("asset") == f.videoA && (analyze["stream_index"] as? Int) == 0
                    && analyze.string("mode") == "points",
                    "Stabilize analyzes the active angle's asset stream")
        try require(e.failure != nil && e.stabilizePending.isEmpty,
                    "Fake media fails typed and unwinds the pending flag")
        e.failure = nil
        // A persisted blank angle name (allowed by the schema) falls back to
        // the source filename so pickers stay distinguishable; ids stay the
        // identity, labels are never used as keys.
        let angleC = UUID().uuidString.lowercased(), angleD = UUID().uuidString.lowercased()
        base = e.revision
        e.createMulticam(name: "Blank", sync: "manual",
            angles: [["id": angleC, "asset": f.videoA, "stream_index": 0, "name": ""],
                     ["id": angleD, "asset": f.videoB, "stream_index": 0, "name": "tail"]],
            reference: nil, offsets: [angleC: RationalTime(num: 0, den: 1), angleD: RationalTime(num: 0, den: 1)])
        try await waitForApply(f, after: base)
        let blank = e.multicamGroups.first { $0.name == "Blank" }
        try require(blank?.angles.first?.displayName == "cam_a.mov",
                    "Blank angle name falls back to the source filename; got \(blank?.angles.first?.displayName ?? "nil")")
        try require(blank?.angles.last?.displayName == "tail", "Set angle names win over the fallback")
        // A non-multicam clip never issues the operation. The asset insert
        // targets V2 explicitly because `seek` clamps to the content end and
        // the multicam clip occupies the head of V1.
        guard let assetA = assetA(f) else { throw GUICheckError(message: "video asset row missing") }
        e.openAssetInSource(assetA)
        e.seekSourceFrame(24); e.setSourceOutPoint() // [0, 1 s)
        e.setSourceDestination(f.track2)
        e.seek(0)
        base = e.revision
        e.insertSource()
        try await waitForApply(f, after: base)
        guard let assetClip = e.editClips.first(where: { $0.multicam == nil }) else {
            throw GUICheckError(message: "asset clip missing")
        }
        let calls = f.transport.callCounts["clip.angle_switch"] ?? 0
        e.switchClipAngle(assetClip, to: angleB)
        try require((f.transport.callCounts["clip.angle_switch"] ?? 0) == calls, "Switch guard skips non-multicam clips")
        await finish(f)
    }

    /// TRACK-002/003 + AI-002 clip analysis authoring: stabilization's
    /// `track.analyze` → `clip_set_effects` chain, optical-flow interpolation
    /// through `clip_time_set`, and the `scene.detect` / `scene.apply` shared
    /// operations. Every request rides the real worker so the strict schema
    /// validates the wire shape; fake locators make media-dependent steps
    /// fail typed instead of silently succeeding.
    func verifyClipAnalysisAuthoring() async throws {
        let f = try await fixture()
        // scene.detect lands in the process-wide job store; keep it under
        // the test folder instead of the user's state root.
        setenv("KRONELLO_STATE_ROOT", f.folder.appendingPathComponent("job-state").path, 1)
        defer { unsetenv("KRONELLO_STATE_ROOT") }
        let e = f.editor
        // A 1 s asset clip from source window [1 s, 2 s) covering the head of
        // V1 (overwrite splits the composition clip, leaving a [1 s, 3 s)
        // remainder for the ineligible-source checks).
        guard let assetA = assetA(f) else { throw GUICheckError(message: "video asset row missing") }
        e.openAssetInSource(assetA)
        e.seekSourceFrame(24); e.setSourceInPoint()
        e.seekSourceFrame(48); e.setSourceOutPoint()
        e.seek(0)
        var base = e.revision
        e.overwriteSource()
        try await waitForApply(f, after: base)
        guard var clip = e.editClips.first(where: { $0.authored.object("source_ref").string("kind") == "asset" }),
              let comp = e.editClips.first(where: { $0.composition != nil })
        else { throw GUICheckError(message: "clips missing") }
        // Eligibility narrows to forward asset clips on video tracks.
        try require(e.trackableSource(comp) == nil && !e.interpolationEligible(comp) && !e.sceneDetectEligible(comp),
                    "Composition clips expose no media source")
        try require(e.trackableSource(clip)?.asset == f.videoA && e.trackableSource(clip)?.stream == 0
                    && e.trackableSource(clip)?.offset == RationalTime(num: 0, den: 1),
                    "Asset clip resolves its media source")
        try require(e.interpolationEligible(clip) && e.sceneDetectEligible(clip) && e.clipInterpolation(clip) == nil
                    && !e.clipHasStabilize(clip), "Linear asset clip is eligible for every analysis flow")
        // TRACK-003: enabling optical flow converts the linear map to its
        // equivalent two-point piecewise map (media = source_in + local).
        base = e.revision
        e.setClipInterpolation(clip, opticalFlow: true)
        try await waitForApply(f, after: base)
        clip = e.editClips.first { $0.id == clip.id }!
        var map = clip.authored.object("time_map")
        var points = map.objects("points")
        try require(map.string("kind") == "piecewise_linear" && points.count == 2, "Linear map converts to piecewise")
        try require(RationalTime.wire(points[0].object("parent")) == RationalTime(num: 0, den: 1)
                    && RationalTime.wire(points[0].object("local")) == RationalTime(num: 0, den: 1)
                    && RationalTime.wire(points[1].object("parent")) == RationalTime(num: 1, den: 1)
                    && RationalTime.wire(points[1].object("local")) == RationalTime(num: 1, den: 1),
                    "Piecewise points preserve the identity map over the clip range")
        let interpolation = map.object("interpolation")
        try require(interpolation.string("mode") == "optical_flow"
                    && Int(interpolation.string("block_radius")) == 4 && Int(interpolation.string("search_radius")) == 8
                    && Int(interpolation.string("levels")) == 3
                    && RationalTime.wire(interpolation.object("confidence_floor")) == RationalTime(num: 1, den: 4)
                    && RationalTime.wire(interpolation.object("max_low_confidence")) == RationalTime(num: 1, den: 2),
                    "Optical-flow config carries the versioned defaults verbatim")
        // A piecewise clip is not stabilizable (the guard rejects nonlinear
        // maps without issuing track.analyze).
        e.stabilizeClip(clip)
        try require(e.failure?.code == "UNSUPPORTED_FEATURE" && f.transport.lastCalls["track.analyze"] == nil,
                    "Speed-ramped clips reject stabilization before any analysis")
        e.failure = nil
        // Disabling folds the affine two-point map back to linear.
        base = e.revision
        e.setClipInterpolation(clip, opticalFlow: false)
        try await waitForApply(f, after: base)
        clip = e.editClips.first { $0.id == clip.id }!
        map = clip.authored.object("time_map")
        try require(map.string("kind") == "linear" && map["interpolation"] == nil
                    && RationalTime.wire(map.object("speed")) == RationalTime(num: 1, den: 1)
                    && RationalTime.wire(map.object("offset")) == RationalTime(num: 0, den: 1),
                    "Affine piecewise map folds back to the equivalent linear map")
        // TRACK-002: stabilization submits track.analyze for the clip's media
        // window. The fake locator fails typed inside the service; the pending
        // flag unwinds and no clip_set_effects ever reaches edit.apply.
        let applies = f.transport.callCounts["edit.apply"] ?? 0
        e.stabilizeClip(clip)
        for _ in 0..<200 where !e.stabilizePending.isEmpty { try await Task.sleep(for: .milliseconds(10)) }
        let analyze = f.transport.lastCalls["track.analyze"] ?? [:]
        try require(analyze.string("asset") == f.videoA && (analyze["stream_index"] as? Int) == 0
                    && analyze.string("mode") == "points" && analyze.objects("seeds").count == 3,
                    "track.analyze carries the clip's asset stream with seed points")
        try require(RationalTime.wire(analyze.object("range").object("start")) == RationalTime(num: 1, den: 1)
                    && RationalTime.wire(analyze.object("range").object("end")) == RationalTime(num: 2, den: 1),
                    "Analysis range covers the clip's media window [1 s, 2 s)")
        try require(UUID(uuidString: analyze.string("id")) != nil
                    && UUID(uuidString: analyze.string("idempotency_key")) != nil
                    && !analyze.string("base_revision").isEmpty,
                    "track.analyze carries identity and revision fields")
        try require((f.transport.callCounts["edit.apply"] ?? 0) == applies && e.stabilizePending.isEmpty
                    && e.failure != nil, "Failed analysis unwinds without mutating the document")
        e.failure = nil
        // AI-002: detection submits a shared job scoped to the clip's stream.
        e.detectScenes(clip)
        for _ in 0..<200 where f.transport.lastCalls["scene.detect"] == nil { try await Task.sleep(for: .milliseconds(10)) }
        let detect = f.transport.lastCalls["scene.detect"] ?? [:]
        try require(detect.string("asset") == f.videoA && (detect["stream_index"] as? Int) == 0,
                    "scene.detect targets the clip's asset stream")
        try require(e.sceneBoundaryAssets(for: clip).isEmpty, "No committed boundary assets yet")
        // scene.apply rides submitDirect (session_id + idempotency + base) and
        // fails typed on an unknown boundary asset instead of silently no-oping.
        let sceneAsset = "dddddddd-dddd-4ddd-8ddd-dddddddddddd"
        base = e.revision
        e.applySceneBoundaries(clip, asset: sceneAsset, split: false)
        for _ in 0..<200 where e.failure == nil { try await Task.sleep(for: .milliseconds(10)) }
        let apply = f.transport.lastCalls["scene.apply"] ?? [:]
        try require(apply.string("scene_asset") == sceneAsset && apply.string("sequence") == f.sequence
                    && apply.string("mode") == "markers" && apply.string("clip") == clip.id,
                    "scene.apply carries the boundary asset, sequence, clip and mode")
        try require(apply.string("base_revision") == base
                    && UUID(uuidString: apply.string("session_id")) != nil
                    && UUID(uuidString: apply.string("idempotency_key")) != nil,
                    "scene.apply carries shared revision/session/idempotency fields")
        try require(e.failure != nil, "Unknown boundary asset fails typed")
        await finish(f)
    }
}
