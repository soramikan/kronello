import Foundation
import KronelloAppModel

/// FLOW-002/FLOW-003 GUI checks (ADR-0129/0130): the media browser and export
/// presets must ride the shared document/commands/query surface — FakeTransport
/// asserts the exact wire operations rather than any GUI-only state.
@MainActor struct MediaFlowChecks {
    func model(_ transport: FakeTransport, folder: URL) -> EditorModel {
        GUIChecks().model(transport, folder: folder)
    }
    func lastApplyCommand(_ fake: FakeTransport, named name: String) -> [String: Any] {
        fake.requests.last { $0.string("operation") == "edit.apply" }?
            .objects("commands").first?.object(name) ?? [:]
    }
    /// media.query results with bins, document order and the offline probe;
    /// bin membership edits emit the shared `bin_*` commands.
    func verifyMediaBrowserBinsAndOffline() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), editor = model(fake, folder: folder)
        let a = UUID().uuidString.lowercased(), b = UUID().uuidString.lowercased(), bin = UUID().uuidString.lowercased()
        fake.document["assets"] = [
            ["id": a, "kind": "video", "content_hash": "h1", "streams": [["index": 0]], "locator": ["relative": "clip a.mov"]],
            ["id": b, "kind": "audio", "content_hash": "h2", "streams": [["index": 0]], "locator": ["relative": "gone.wav"]],
        ]
        fake.document["bins"] = [["id": bin, "name": "素材", "assets": [a]]]
        var mediaCalls = 0
        fake.extraHandler = { request in
            switch request.string("operation") {
            case "media.query":
                mediaCalls += 1
                let details = fake.document.objects("assets")
                return ["revision": fake.revision, "bins": fake.document["bins"] ?? [],
                        "assets": [
                            ["asset": a, "detail": details[0], "availability": "present_unverified", "size_bytes": 2048],
                            ["asset": b, "detail": details[1], "availability": "missing",
                             "error": ["code": "ASSET_MISSING", "message": "file not found"]],
                        ]]
            case "asset.relink": return ["project_id": "p", "name": "Tests", "revision": "2", "open_mode": "normal"]
            default: return nil
            }
        }
        try await editor.start()
        let media = MediaPageModel(editor: editor)
        await media.refresh()
        try require(media.assets.count == 2 && media.bins.count == 1, "media.query lists every asset and the persisted bins")
        try require(media.name(of: media.assets[0]) == "clip a.mov", "Display name comes from the locator")
        try require(!media.isOffline(media.assets[0]) && media.isOffline(media.assets[1]), "Probe result drives the offline badge")
        try require(media.error(of: media.assets[1])?.code == "ASSET_MISSING", "Offline rows carry the typed error code")
        // Bin filtering keeps document order; empty selection shows all.
        media.binSelection = bin
        try require(media.visibleAssets.count == 1 && media.visibleAssets[0].string("asset") == a, "Bin selection filters to membership")
        media.binSelection = nil
        try require(media.visibleAssets.count == 2, "Clearing the bin selection lists all assets")
        media.search = "clip a"
        try require(media.visibleAssets.count == 1, "Search filters by display name")
        media.search = ""
        // Membership edits are shared `bin_assign` commands carrying the whole
        // next set; the service rejects unknown assets server-side.
        await media.setMembership(b, in: bin, member: true)
        let assign = lastApplyCommand(fake, named: "bin_assign")
        try require(assign.string("bin") == bin && (assign["assets"] as? [String]) == [a, b],
                    "bin_assign carries the complete ordered membership")
        // Emulate the server applying the membership: the re-query exposes it.
        fake.document["bins"] = [["id": bin, "name": "素材", "assets": [a, b]]]
        await media.refresh()
        await media.setMembership(b, in: bin, member: false)
        let removed = lastApplyCommand(fake, named: "bin_assign")
        try require((removed["assets"] as? [String]) == [a], "Removing sends the remaining membership")
        // Create / rename / delete share the same command surface.
        await media.createBin(name: "  整音  ")
        let created = lastApplyCommand(fake, named: "bin_create").object("bin")
        try require(created.string("name") == "整音" && !created.string("id").isEmpty
                    && (created["assets"] as? [String])?.isEmpty == true, "bin_create trims the name and allocates a stable id")
        await media.createBin(name: "   ")
        try require(lastApplyCommand(fake, named: "bin_create").object("bin").string("name") == "整音",
                    "Empty names never reach the service")
        await media.renameBin(bin, name: "完成素材")
        let renamed = lastApplyCommand(fake, named: "bin_rename")
        try require(renamed.string("bin") == bin && renamed.string("name") == "完成素材", "bin_rename keeps membership untouched")
        await media.deleteBin(bin)
        try require(lastApplyCommand(fake, named: "bin_delete").string("bin") == bin, "bin_delete targets the stable id")
        // Relinking goes through the shared operation with the revision fence.
        await media.relink(b, searchDirectory: "/tmp/find")
        let relink = fake.requests.last { $0.string("operation") == "asset.relink" }
        try require(relink?.string("asset") == b && relink?.string("search_directory") == "/tmp/find"
                    && !relink!.string("base_revision").isEmpty, "asset.relink carries asset, directory and base revision")
        try require(mediaCalls > 1, "Every mutation re-queries the shared media list")
        await editor.close()
    }
    /// asset.thumbnail pixels cache per asset; failures cache the typed error.
    func verifyThumbnailCacheAndFailures() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), editor = model(fake, folder: folder)
        let image = UUID().uuidString.lowercased(), audio = UUID().uuidString.lowercased()
        var thumbnailCalls = 0
        fake.extraHandler = { request in
            switch request.string("operation") {
            case "asset.thumbnail":
                thumbnailCalls += 1
                guard request.string("asset") == image else {
                    throw ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "audio has no frames")
                }
                return ["asset": image, "pts": ["num": "0", "den": "1"], "width": 16, "height": 8,
                        "rgba": (0..<512).map { _ in NSNumber(value: 255) }]
            default: return nil
            }
        }
        try await editor.start()
        let media = MediaPageModel(editor: editor)
        media.ensureThumbnail(image)
        media.ensureThumbnail(audio)
        for _ in 0..<400 {
            if media.thumbnails[image] != nil && media.thumbnailFailures[audio] != nil { break }
            try await Task.sleep(for: .milliseconds(5))
        }
        try require(media.thumbnails[image]?.width == 16 && media.thumbnails[image]?.rgba.count == 512,
                    "Thumbnail pixels decode into the in-memory cache")
        try require(media.image(for: image) != nil, "RGBA8 cache produces a display image")
        try require(media.thumbnailFailures[audio]?.code == "UNSUPPORTED_FEATURE", "Unsupported assets cache their typed failure")
        media.ensureThumbnail(image); media.ensureThumbnail(audio)
        try await Task.sleep(for: .milliseconds(20))
        try require(thumbnailCalls == 2, "Cached results and cached failures never re-request")
        await editor.close()
    }
    /// Preset save/delete are shared commands; export.batch submits ordered
    /// preset items with destinations derived by the shared naming rule.
    func verifyPresetsAndBatchQueue() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let fake = FakeTransport(), editor = model(fake, folder: folder)
        let comp = fake.document.objects("compositions")[0].string("id")
        var batchRequest: [String: Any] = [:]
        var submittedJobs: [[String: Any]] = []
        fake.extraHandler = { request in
            switch request.string("operation") {
            case "capabilities.get":
                return ["export_profiles": [
                    ["format": "pro_res_mov", "container_extension": "mov", "profile_versions": [1]],
                    ["format": "image_sequence", "container_extension": "", "profile_versions": [1]],
                ]]
            case "export.batch":
                batchRequest = request
                let items = request.objects("items").enumerated().map { index, item -> [String: Any] in
                    index == 0
                        ? ["outcome": "submitted", "idempotency_key": "auto:x",
                           "job": ["id": "job-\(index)", "status": "queued", "project_id": fake.document.string("id"),
                                   "destination": item["destination"] ?? ""]]
                        : ["outcome": "failed", "idempotency_key": "auto:y",
                           "error": ["code": "OUTPUT_EXISTS", "message": "exists"]]
                }
                submittedJobs += items.compactMap { $0["job"] as? [String: Any] }
                return ["items": items]
            case "job.list": return ["jobs": submittedJobs]
            default: return nil
            }
        }
        try await editor.start()
        let export = ExportPageModel(editor: editor)
        await export.load()
        try require(export.profiles.count == 2, "capabilities drive the profile list")
        // Save the current form as a preset: one shared export_preset_save.
        export.target = "composition:" + comp
        export.format = "pro_res_mov"; export.version = "1"; export.audio = "document"
        export.rangeMode = "inout"; export.startFrame = 1; export.endFrame = 25
        await export.savePreset(name: "配信用")
        let saved = lastApplyCommand(fake, named: "export_preset_save").object("preset")
        try require(saved["version"] as? Int == 1 && saved.string("name") == "配信用", "Preset carries the payload version and name")
        try require(saved.string("composition") == comp, "Composition targets store the legacy composition field")
        try require(saved.object("output").string("format") == "pro_res_mov"
                    && saved.object("output")["profile_version"] as? Int == 1, "Output mirrors the render.submit shape")
        try require(saved.object("range").object("start").string("num") == "1"
                    && saved.object("range").object("end").string("num") == "25", "The frame range serializes rationally")
        let firstID = saved.string("id")
        fake.document["export_presets"] = [saved] // emulate the applied upsert
        try await editor.reload()
        await export.savePreset(name: "配信用")
        try require(lastApplyCommand(fake, named: "export_preset_save").object("preset").string("id") == firstID,
                    "Saving the same name upserts the existing preset id")
        // Presets list reads from the shared document.
        var second = saved
        second["id"] = UUID().uuidString.lowercased(); second["name"] = "連番 PNG"
        second["output"] = ["format": "image_sequence"]
        fake.document["export_presets"] = [saved, second]
        try await editor.reload()
        try require(export.presets.count == 2, "Presets come from project.export document state")
        try require(export.presetExtension(saved) == "mov" && export.presetExtension(second) == nil,
                    "Destination extensions follow the shared rule")
        // Restoring a preset populates the form from document data only.
        export.loadPreset(firstID)
        try require(export.target == "composition:" + comp && export.format == "pro_res_mov"
                    && export.startFrame == 1 && export.endFrame == 25, "loadPreset restores target/format/range")
        // Batch submit preserves document order and derives destination names.
        export.presetSelection = [firstID, second.string("id")]
        await export.submitPresetBatch(directory: "/tmp/out")
        let items = batchRequest.objects("items")
        try require(items.count == 2 && batchRequest.string("failure_policy") == "continue",
                    "Watch-style batches continue past per-item failures")
        try require(items[0].string("preset") == firstID && items[1].string("preset") == second.string("id"),
                    "Batch items follow document order")
        try require(items[0].string("destination") == "/tmp/out/" + ExportPageModel.sanitizedStem("配信用") + ".mov"
                    && items[1].string("destination") == "/tmp/out/" + ExportPageModel.sanitizedStem("連番 PNG"),
                    "Destinations use the shared stem + extension rule")
        try require(items.allSatisfy { $0.string("project") == editor.path && $0["submission"] == nil },
                    "Preset items defer to the server-side conversion")
        try require(export.batchResults.count == 2 && export.jobFailure?.code == "OUTPUT_EXISTS",
                    "Per-item outcomes surface; the first failure is the panel error")
        try require(export.jobs.first?.string("id") == "job-0", "Submitted jobs join the shared queue view")
        await export.deletePreset(firstID)
        try require(lastApplyCommand(fake, named: "export_preset_delete").string("preset") == firstID,
                    "export_preset_delete targets the stable id")
        try require(!export.presetSelection.contains(firstID), "Deleting clears the selection")
        // The stem rule matches watch: ASCII alnum/-/_ only, 64 chars, fallback.
        try require(ExportPageModel.sanitizedStem("配信 用.mov") == "_____mov", "Non-ASCII and punctuation become underscores")
        try require(ExportPageModel.sanitizedStem("") == "preset", "Empty names fall back")
        try require(ExportPageModel.sanitizedStem(String(repeating: "x", count: 100)).count == 64, "Stems truncate at 64")
        await editor.close()
    }
}
