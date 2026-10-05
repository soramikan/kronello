import Foundation
import KronelloAppModel
import KronelloCore
import KronelloDesign

@MainActor final class WorkflowTransport: ProjectTransport {
    let base = FakeTransport()
    var notificationHandler: ((String,[String:Any]) -> Void)?
    var requests: [[String:Any]] = []
    var errors: [[String:Any]] = []
    var listedJobs: [[String:Any]] = []
    var capabilities: [[String:Any]] = [["format":"pro_res_mov","profile_versions":[1,2,3],"audio_modes":["explicit","document","silence"],"audio_codecs":["pcm_s24le"],"container_extension":"mov","execution":"software","encoder_registered":true,"device_availability":"available"]]
    var migrationCommands: [[String:Any]] = []
    func ready() async throws {}
    func subscribe() async throws {}
    func poll() throws {}
    func close() {}
    func call(_ request: [String:Any]) async throws -> [String:Any] {
        // Exercise strict generated request validation, including no top-level
        // project for global jobs/capabilities and nested render requests.
        _ = try NativeProjectTransport.request(request)
        requests.append(request)
        switch request.string("operation") {
        case "capabilities.get": return ["export_profiles":capabilities]
        case "render.explain": return ["revision":base.revision,"plan":["executed":false],"diagnostics":errors]
        case "job.list": return ["jobs":listedJobs]
        case "job.cancel": return listedJobs.first ?? [:]
        case "render.submit": return ["id":"job","project_id":base.document.string("id"),"revision":base.revision,"status":"queued","snapshot_hash":"fixed","total_frames":72,"completed_frames":0,"output_profile":request.object("output")]
        case "edit.plan":
            if request.string("base_revision") != base.revision { throw ServiceFailure(code:"REVISION_CONFLICT",message:"stale candidate") }; return try await base.call(request)
        case "template.preview": return ["revision":base.revision,"diagnostic":NSNull(),"nodes":[],"design_extent":["width":64,"height":32]]
        case "template.migration_plan": return ["plan":["commands":migrationCommands,"plan_hash":"migration"],"changes":[["field":"/instance/version","before":"1.0.0","after":"1.1.0"]],"after":["diagnostic":NSNull()]]
        default: return try await base.call(request)
        }
    }
}

@MainActor struct WorkflowChecks {
    func model(_ transport: any ProjectTransport, folder: URL) -> EditorModel {
        EditorModel(path:folder.appendingPathComponent("work.kronello").path, transport:transport, stateStore:.init(root:folder.appendingPathComponent("state")))
    }
    func verifyTypedErrorGateAndStaleInspection() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at:folder) }
        let transport = WorkflowTransport(), editor = model(transport,folder:folder)
        try await editor.start(); let page = ExportPageModel(editor:editor)
        page.destination = folder.appendingPathComponent("export.mov").path
        await page.load(); transport.errors = [["code":"ASSET_MISSING","message":"missing","details":["asset":"stable-id"]]]
        await page.check(); await page.submit()
        try require(!page.canSubmit && page.errors[0].details.string("asset") == "stable-id", "Typed error disables export and preserves structured details")
        try require(!transport.requests.contains { $0.string("operation") == "render.submit" }, "Disabled export must never submit")
        transport.errors = []; await page.check(); try require(page.canSubmit, "Clean checked configuration can submit")
        page.background = "white"; try require(!page.canSubmit, "Settings changes invalidate preflight")
        await page.check(); transport.base.revision = "2"; try await editor.reload(); try require(!page.canSubmit, "External revision invalidates preflight")
        await editor.close(); page.stopPolling()
    }
    func verifyJobSubmissionProgressAndPolling() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at:folder) }
        let transport = WorkflowTransport(), editor = model(transport,folder:folder)
        try await editor.start(); let page = ExportPageModel(editor:editor)
        page.destination = folder.appendingPathComponent("export.mov").path
        await page.load(); await page.check(); await page.submit()
        let request = transport.requests.last { $0.string("operation") == "render.submit" }!
        try require(request.string("expected_revision") == "1", "Submit fences the checked revision")
        try require(request.object("output").string("format") == "pro_res_mov", "Submit uses closed profile")
        try require(page.jobs.first?.string("revision") == "1" && page.jobs.first?.string("snapshot_hash") == "fixed", "Job row uses returned snapshot identity")
        transport.listedJobs = [["id":"job","project_id":editor.projectID,"revision":"1","status":"running","snapshot_hash":"fixed","completed_frames":36,"total_frames":72]]
        try await Task.sleep(for:.milliseconds(1150))
        try require(ExportPageModel.state(page.jobs[0]) == .running(progress:0.5,remaining:"残り時間は未測定"), "JobRow displays shared progress")
        let count = transport.requests.filter { $0.string("operation") == "job.list" }.count
        try require(count == 2 && !transport.requests.contains { $0.string("operation") == "job.get" }, "Bounded polling makes one batched list per second, no per-job requests")
        transport.listedJobs[0]["status"] = "succeeded"
        try await Task.sleep(for:.milliseconds(1100))
        let completedCount = transport.requests.filter { $0.string("operation") == "job.list" }.count
        try await Task.sleep(for:.milliseconds(1100))
        try require(transport.requests.filter { $0.string("operation") == "job.list" }.count == completedCount, "Polling stops without active jobs")
        try require(ExportPageModel.state(["status":"interrupted"]) == .interrupted, "Interrupted is distinct and never auto-restarted")
        page.stopPolling(); await editor.close()
    }
    func fixture(_ transport: WorkflowTransport) -> (String,String) {
        let comp = transport.base.document.objects("compositions")[0].string("id"), edition = UUID().uuidString, placement = UUID().uuidString
        let policy:[String:Any] = ["intro":["num":"1","den":"1"],"outro":["num":"1","den":"1"],"minimum_middle":["num":"1","den":"2"],"middle_mode":"stretch"]
        transport.base.document["templates"] = [["id":edition,"template_id":UUID().uuidString,"version":"1.0.0","composition_ref":comp,"duration_policy":policy,"public_inputs":["title":["value_type":"string","default":["kind":"string","value":"Old"],"target":["text":["node":UUID().uuidString]],"minimum":NSNull(),"maximum":NSNull(),"choices":[]]],"variants":["portrait":["composition_ref":comp,"targets":[:],"constraints":[:],"content_hash":""]],"constraints":[:],"content_hash":""]]
        transport.base.document["template_instances"] = [["id":placement,"definition_ref":edition,"version":"1.0.0","duration":["num":"3","den":"1"],"inputs":[:]]]
        return (edition,placement)
    }
    func verifyTemplateInputOneCommand() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at:folder) }
        let transport = WorkflowTransport(); let (edition,_) = fixture(transport), editor = model(transport,folder:folder)
        try await editor.start(); let page = TemplatePageModel(editor:editor); page.selectEdition(edition)
        await page.refresh(); let count = transport.requests.filter { $0.string("operation") == "template.preview" }.count
        try require(count == 2, "One shared query per variant, not per frame")
        await page.editInput("title",value:["kind":"string","value":"New"],applyToPlacement:true)
        await page.editInput("title",value:["kind":"string","value":"New"],applyToPlacement:true)
        let applies = transport.requests.filter { $0.string("operation") == "edit.apply" }
        try require(applies.count == 1 && applies[0].objects("commands").count == 1, "Duplicate Return/focus commit produces one input command; applies=\(applies.count), failure=\(editor.failure?.message ?? "none")")
        try require(applies[0].objects("commands")[0].object("template").object("set_input").string("name") == "title", "Edit public input through shared template command")
        transport.base.revision = "3"; try await editor.reload()
        await page.editInput("title",value:["kind":"string","value":"Stale draft"],applyToPlacement:true)
        try require(editor.pendingCandidate?.base == "2" && editor.revisionConflict?.code == "REVISION_CONFLICT", "Public input candidate keeps origin revision across external changes")
        try require(transport.requests.filter { $0.string("operation") == "edit.apply" }.count == 1,"Stale input never silently overwrites")
        await page.retryCandidate()
        try require(editor.pendingCandidate == nil && page.value("title").string("value") == "Stale draft","Explicit retry updates the displayed candidate after success")
        await page.editInput("title",value:["kind":"string","value":"After retry"],applyToPlacement:true)
        try require(editor.revisionConflict == nil && transport.requests.filter { $0.string("operation") == "edit.apply" }.count == 3,"Following edit uses the revision acknowledged by explicit retry")
        await editor.close()
    }
    func verifyMigrationExplicitApplyAndPolicyPublication() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at:folder) }
        let transport = WorkflowTransport(); let (edition,placement) = fixture(transport), editor = model(transport,folder:folder)
        try await editor.start(); let page = TemplatePageModel(editor:editor); page.selectEdition(edition)
        transport.migrationCommands = [["template":["migrate":["instance":placement,"definition":edition,"variant":NSNull(),"inputs":[:]]]]]
        await page.planMigration()
        try require(page.canApplyMigration && !transport.requests.contains { $0.string("operation") == "edit.apply" }, "Plan never applies implicitly")
        page.nextVariant = "portrait"; try require(!page.canApplyMigration, "Changed migration target invalidates old plan")
        page.nextVariant = ""; await page.applyMigration()
        try require(transport.requests.filter { $0.string("operation") == "edit.apply" }.count == 1, "Explicit migration applies once")
        page.middleMode = "hold"; await page.publish()
        let command = transport.requests.last { $0.string("operation") == "edit.apply" }!.objects("commands")
        try require(command.count == 1 && command[0].object("template").object("define").object("definition").object("duration_policy").string("middle_mode") == "hold", "Policy publishes new definition without migrating placements")
        await editor.close()
    }
    func verifyNativeTemplateAndExportInspection() async throws {
        let checks = GUIChecks(), folder = try checks.temporary()
        defer { try? FileManager.default.removeItem(at:folder) }
        let fixtureRoot = folder.appendingPathComponent("review")
        let process = Process(), output = Pipe(), errors = Pipe()
        process.executableURL = URL(fileURLWithPath:"/usr/bin/env")
        process.arguments = ["python3", checks.root.appendingPathComponent("scripts/demo_gui_004.py").path, "--output-root", fixtureRoot.path]
        process.standardOutput = output; process.standardError = errors
        try process.run(); _ = output.fileHandleForReading.readDataToEndOfFile(); process.waitUntilExit()
        try require(process.terminationStatus == 0,"Native fixture: " + String(data:errors.fileHandleForReading.readDataToEndOfFile(),encoding:.utf8)!)
        let review = try JSONSerialization.jsonObject(with:Data(contentsOf:fixtureRoot.appendingPathComponent("review.json"))) as! [String:Any]
        let transport = try RecordingTransport(path:review.string("template"),worker:checks.root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let editor = EditorModel(path:review.string("template"),transport:transport,stateStore:.init(root:folder.appendingPathComponent("ui-state")))
        editor.fonts = try JSONSerialization.jsonObject(with:Data(contentsOf:fixtureRoot.appendingPathComponent("fonts.json"))) as! [[String:Any]]
        try await editor.start(); let page = TemplatePageModel(editor:editor)
        let placement = editor.document.objects("template_instances")[0]
        page.selectEdition(placement.string("definition_ref")); await page.refresh()
        try require(page.definitions.count == 2 && page.previews.count == 2,"Actual project templates decode both immutable editions and variants")
        let base = page.previews.first { $0.id.isEmpty }!, portrait = page.previews.first { $0.id == "portrait" }!
        try require(base.failure == nil && !base.bounds("layout").isEmpty && !base.bounds("ink").isEmpty && !base.bounds("visual").isEmpty,"Shared successful preview supplies all bounds stages")
        try require(portrait.failure?.code == "TEMPLATE_OVERFLOW" && portrait.nodes.isEmpty,"Shared overflow stays typed and has no fabricated bounds")
        var data = page.value("data"); var table = data.object("value"), rows = table.objects("rows")
        rows[0]["headline"] = ["kind":"string","value":"更新"]
        table["rows"] = rows; data["value"] = table
        let applies = transport.applyCount
        await page.editInput("data",value:data,applyToPlacement:true)
        await page.editInput("data",value:data,applyToPlacement:true)
        try require(transport.applyCount == applies + 1,"Real native input edit commits one event")
        let exported = try checks.cli(["operation":"project.export","project":editor.path]).object("document")
        try require(exported.objects("template_instances")[0].object("inputs").object("data").object("value").objects("rows")[0].object("headline").string("value") == "更新","CLI reads the same public input authored by GUI")
        page.nextEdition = review.string("new_edition"); page.nextVariant = ""
        let beforePlan = transport.applyCount; await page.planMigration()
        try require(page.canApplyMigration && transport.applyCount == beforePlan,"Real migration diff does not mutate placement")
        await page.applyMigration()
        try require(transport.applyCount == beforePlan + 1 && editor.document.objects("template_instances")[0].string("definition_ref") == review.string("new_edition"),"Explicit migration changes only through shared apply")
        await editor.close()
        for (name,hasError) in [("ready",false),("error",true)] {
            let t = try RecordingTransport(path:review.string(name),worker:checks.root.appendingPathComponent("apps/macos/Libraries/kronello").path)
            let e = EditorModel(path:review.string(name),transport:t,stateStore:.init(root:folder.appendingPathComponent("ui-" + name)))
            try await e.start(); let export = ExportPageModel(editor:e)
            export.destination = folder.appendingPathComponent(name + "-frames").path
            await export.load(); export.format = "image_sequence"; await export.check()
            try require(export.errors.contains { $0.code == "FONT_MISSING" } == hasError,"Real shared inspection distinguishes ready project and typed font error")
            try require(export.canSubmit == !hasError,"Real inspection controls export gate")
            export.stopPolling(); await e.close()
        }
    }
    func runAll() async throws {
        try await verifyTypedErrorGateAndStaleInspection(); print("PASS GUI-004 typed errors/settings/revision export gate")
        try await verifyJobSubmissionProgressAndPolling(); print("PASS GUI-004 revision-fenced submit, JobRow progress, bounded polling")
        try await verifyTemplateInputOneCommand(); print("PASS GUI-004 template input commits one shared command")
        try await verifyMigrationExplicitApplyAndPolicyPublication(); print("PASS GUI-004 explicit migration and immutable policy publication")
        try await verifyNativeTemplateAndExportInspection(); print("PASS GUI-004 native variants/bounds/overflow, input CLI parity, explicit migration, export inspection")
    }
}
