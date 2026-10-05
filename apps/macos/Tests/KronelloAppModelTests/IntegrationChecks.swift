import Foundation
import KronelloAppModel
import KronelloCore
import KronelloDesign

@MainActor private final class InspectionTransport: ProjectTransport {
    let native: NativeProjectTransport?
    var notificationHandler: ((String, [String: Any]) -> Void)?
    var requests: [[String: Any]] = []
    var scenes: [[String: Any]] = []
    var response: [String: Any] = [:]
    var delay: Duration = .zero
    var fail: ServiceFailure?
    init(path: String? = nil) throws {
        native = try path.map { try NativeProjectTransport(path: $0, worker: nil) }
    }
    func ready() async throws { try await native?.ready() }
    func subscribe() async throws {}
    func poll() throws {}
    func close() { native?.close() }
    func call(_ request: [String: Any]) async throws -> [String: Any] {
        requests.append(request)
        if delay > .zero { try await Task.sleep(for: delay) }
        if let fail { throw fail }
        let result = try await native?.call(request) ?? response
        if request.string("operation") == "scene.query" { scenes.append(result) }
        return result
    }
}

@MainActor struct IntegrationChecks {
    func verifyScheduling() async throws {
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let transport = try InspectionTransport()
        let editor = EditorModel(path: folder.appendingPathComponent("unused.kronello").path, transport: transport,
                                 stateStore: UIStateStore(root: folder))
        let comp = UUID().uuidString, placement = UUID().uuidString, instance = UUID().uuidString, internalNode = UUID().uuidString
        let source: [String: Any] = ["id": internalNode, "kind": ["kind": "text"], "name": "Headline", "properties": []]
        let authored: [String: Any] = ["id": placement, "kind": ["kind": "composition_instance", "value": ["id": instance, "template_instance": instance]], "properties": []]
        let document: [String: Any] = ["template_instances": [["id": instance]], "compositions": [["id": comp, "root_nodes": [placement], "nodes": [authored]], ["id": UUID().uuidString, "nodes": [source]]]]
        transport.response = ["revision": "1", "nodes": [["key": ["instance_path": [instance], "node": internalNode], "evaluated": ["text": "日本語", "properties": [:], "bounds": [:]]]]]
        editor.ui.composition = comp; editor.ui.selection = placement
        editor.adopt(document: document, scene: [:], revision: "1", actor: "", external: false)
        let inspection = TemplateInstanceInspection.shared(for: editor)
        try require(inspection === TemplateInstanceInspection.shared(for: editor), "Inspector/Viewer share one read-only store")
        await inspection.refresh(editor, debounce: .zero)
        try require(transport.requests.count == 1 && transport.requests[0]["expand_instances"] as? Bool == true, "One expanded query for all internal values and stages")
        try require(inspection.nodes.first?.path == [instance] && inspection.selected?.layer.id == internalNode, "Full stable instance path and node ID")
        await inspection.refresh(editor, debounce: .zero)
        try require(transport.requests.count == 1, "Repeated same revision/time/selection uses cached response")
        editor.playing = true
        for frame in 1...20 {
            editor.ui.time = RationalTime(num: Int64(frame), den: 24)
            await inspection.refresh(editor, debounce: .zero)
        }
        try require(transport.requests.count == 1 && inspection.stale && inspection.nodes.count == 1, "Playback keeps last value, stale notice, and issues no inspection query")
        editor.playing = false
        await inspection.refresh(editor, debounce: .zero)
        try require(transport.requests.count == 2 && !inspection.stale, "Pause updates once")
        editor.ui.time = RationalTime(num: 1, den: 1)
        let scrub = Task { await inspection.refresh(editor) }
        try await Task.sleep(for: .milliseconds(30))
        scrub.cancel(); editor.ui.time = RationalTime(num: 2, den: 1)
        await inspection.refresh(editor); await scrub.value
        try require(transport.requests.count == 3 && transport.requests.last?.object("evaluation").object("time").string("num") == "2", "150ms idle debounce cancels a superseded scrub before submission")
        transport.delay = .milliseconds(100)
        editor.ui.time = RationalTime(num: 3, den: 1)
        let obsolete = Task { await inspection.refresh(editor, debounce: .zero) }
        try await Task.sleep(for: .milliseconds(20)); obsolete.cancel()
        editor.playing = true; await inspection.refresh(editor, debounce: .zero); await obsolete.value
        try require(inspection.stale && inspection.nodes.first?.layer.evaluated.string("text") == "日本語", "Cancelled in-flight reply cannot adopt during playback")
        editor.playing = false; transport.delay = .zero
        transport.fail = .init(code: "FONT_MISSING", message: "Test font is missing")
        await inspection.refresh(editor, debounce: .zero)
        try require(inspection.failure?.code == "FONT_MISSING" && inspection.nodes.isEmpty, "Typed failure is visible, never fabricated values")
        transport.fail = nil; transport.response["revision"] = "2"
        await inspection.refresh(editor, debounce: .zero)
        try require(inspection.failure?.code == "REVISION_CONFLICT", "Different revision is rejected")
        await editor.close()
    }

    /// Explicit evidence path is mandatory for the acceptance run. XCTest skips
    /// without it; the direct runner prints that separate boundary.
    func verifyEvidence(_ evidence: URL) async throws {
        func read(_ url: URL) throws -> [String: Any] {
            try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [String: Any]
        }
        let manifest = try read(evidence), directory = evidence.deletingLastPathComponent()
        let path = manifest.string("project")
        let transport = try InspectionTransport(path: path)
        try await transport.ready()
        let folder = try GUIChecks().temporary(); defer { try? FileManager.default.removeItem(at: folder) }
        let editor = EditorModel(path: path, transport: transport, stateStore: UIStateStore(root: folder))
        let inspection = TemplateInstanceInspection.shared(for: editor)
        editor.fonts = manifest.objects("fonts")
        let exported = try await editor.request("project.export")
        try require(exported.string("revision") == manifest.string("revision"), "GUI opens the exact CLI evidence revision")
        let cases = manifest.objects("cases")
        try require(cases.count == 8, "Four fixed times for both variants")
        for item in cases {
            let expected = try read(directory.appendingPathComponent(item.string("cli")))
            let request = item.object("request"), time = request.object("evaluation").object("time")
            editor.ui.composition = request.string("composition"); editor.ui.selection = item.string("placement")
            editor.ui.time = .init(num: Int64(time.string("num"))!, den: Int64(time.string("den"))!)
            editor.adopt(document: exported.object("document"), scene: [:], revision: exported.string("revision"), actor: "", external: false)
            try require(TemplateInstanceInspection.instance(editor.selected, document: editor.document) == item.string("instance"), "GUI selects the evidence instance: " + item.string("name"))
            let before = transport.scenes.count
            await inspection.refresh(editor, debounce: .zero)
            if let failure = inspection.failure { throw failure }
            try require(transport.scenes.count == before + 1, "One GUI FFI query per fixed case: " + item.string("name"))
            try require(NSDictionary(dictionary: transport.scenes.last!) == NSDictionary(dictionary: expected), "FFI result equals every CLI field: " + item.string("name"))
            let expectedNodes = try TemplateInstanceInspection.decode(scene: expected, document: exported.object("document"), instance: item.string("instance"))
            try require(inspection.nodes.count == 2 && expectedNodes.map(\.id) == inspection.nodes.map(\.id), "Inspector shows band and text by stable identity")
            for (actual, expectedNode) in zip(inspection.nodes, expectedNodes) {
                for property in expectedNode.layer.properties {
                    try require(actual.values(property) == expectedNode.values(property), "Inspector PropertyPresentation values equal CLI through rounding rules")
                    let key = property.object("descriptor").string("key")
                    if key == "kronello.shape.size" {
                        try require(actual.values(property) == (item.string("name").hasPrefix("portrait") ? ["56.0", "68.0"] : ["258.0", "36.0"]), "Explicit Inspector size readout")
                    }
                    if key == "kronello.opacity" { try require(actual.values(property) == ["100.0 %"], "Percentage scale is applied once") }
                }
                try require(actual.layer.evaluated.string("text") == expectedNode.layer.evaluated.string("text"), "Resolved headline equals CLI, including line breaks")
                for stage in ["layout", "ink", "visual"] {
                    let bounds = expectedNode.layer.evaluated.object("bounds").object(stage + "_bounds")
                    let min = bounds["min"] as? [Double], max = bounds["max"] as? [Double]
                    try require(min != nil && max != nil, "Fixture has each bounds stage")
                    let rectangle = CGRect(x: min![0], y: min![1], width: max![0] - min![0], height: max![1] - min![1])
                    try require(actual.layer.bounds(stage) == rectangle, "Viewer/Inspector exact bounds equal CLI")
                    let selection = actual.selection(stage: stage, extent: editor.extent)!
                    try require(selection.label == "\(stage) \(Int(rectangle.width.rounded())) × \(Int(rectangle.height.rounded()))" &&
                        selection.rect.minX == rectangle.minX / editor.extent.width && selection.rect.height == rectangle.height / editor.extent.height,
                        "Viewer overlay label rounding and normalized geometry equal CLI")
                }
            }
            print("PASS FFI Inspector/Viewer case " + item.string("name"))
        }
        let after = try await editor.request("project.export")
        try require(NSDictionary(dictionary: after) == NSDictionary(dictionary: exported), "Read-only GUI inspection never edits the project")
        await editor.close()
        print("PASS stage-2 FFI/CLI parity: 8 cases, Inspector presentation and all Viewer bounds stages")
    }
    func runAll() async throws {
        try await verifyScheduling(); print("PASS internal inspection debounce/cancellation/cache/playback/typed failures")
        if let path = ProcessInfo.processInfo.environment["KRONELLO_INTEGRATION_EVIDENCE"] {
            try await verifyEvidence(URL(fileURLWithPath: path))
        } else { print("SKIP stage-2 FFI evidence: set KRONELLO_INTEGRATION_EVIDENCE to gui-evidence.json") }
    }
}
