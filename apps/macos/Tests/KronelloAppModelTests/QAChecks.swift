import AppKit
import Foundation
import KronelloAppModel
import KronelloCore
import KronelloDesign

@MainActor final class QARecordingTransport: ProjectTransport {
    let recording: RecordingTransport
    var lastEdit: [String: Any] = [:]
    var planCount: Int { recording.planCount }
    var applyCount: Int { recording.applyCount }
    var lastApply: [String: Any] { recording.lastApply }
    var notificationHandler: ((String, [String: Any]) -> Void)? {
        get { recording.notificationHandler }
        set { recording.notificationHandler = newValue }
    }
    init(path: String, worker: String) throws { recording = try .init(path: path, worker: worker) }
    func ready() async throws { try await recording.ready() }
    func subscribe() async throws { try await recording.subscribe() }
    func poll() throws { try recording.poll() }
    func close() { recording.close() }
    func call(_ request: [String: Any]) async throws -> [String: Any] {
        if ["edit.apply", "edit.undo"].contains(request.string("operation")) { lastEdit = request }
        return try await recording.call(request)
    }
}

@MainActor struct QAChecks {
    let root = GUIChecks().root
    var evidence: URL {
        ProcessInfo.processInfo.environment["KRONELLO_QA_EVIDENCE_DIR"].map { URL(fileURLWithPath: $0) }
            ?? root.appendingPathComponent("target/qa-002")
    }
    func script() throws -> [String: Any] {
        try JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("tests/qa-002/scenarios.json"))) as! [String: Any]
    }
    func write(_ value: [String: Any], name: String) throws {
        try FileManager.default.createDirectory(at: evidence, withIntermediateDirectories: true)
        try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys, .prettyPrinted, .withoutEscapingSlashes])
            .write(to: evidence.appendingPathComponent(name))
    }
    func fixture() async throws -> (URL, EditorModel, QARecordingTransport) {
        let folder = try GUIChecks().temporary(), path = folder.appendingPathComponent("gui.kronello").path
        let transport = try QARecordingTransport(path: path, worker: root.appendingPathComponent("apps/macos/Libraries/kronello").path)
        let editor = EditorModel(path: path, transport: transport, stateStore: .init(root: folder.appendingPathComponent("state")))
        let document = try script().object("document")
        editor.fonts = [["identity": document.objects("texts")[0].objects("styles")[0].object("font"),
                         "path": root.appendingPathComponent("target/fixtures/external/NotoSansCJKjp-Regular.otf").path]]
        try await editor.start(newDocument: document)
        try require(editor.previewFailure == nil, "Fixture evaluates with its pinned font; no query fallback: \(String(describing: editor.previewFailure))")
        return (folder, editor, transport)
    }
    func snapshot(_ editor: EditorModel) async throws -> [String: Any] {
        let exported = try await editor.request("project.export")
        let history = try await editor.request("history.list", ["since_revision": "0", "limit": 1000])
        return ["document": exported.object("document"), "revision": exported.string("revision"), "event_count": history.objects("events").count]
    }
    func waitForEdit(_ editor: EditorModel, after base: String) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(45))
        while ContinuousClock.now < deadline {
            if editor.revision != base && !editor.busy { return }
            if let failure = editor.failure ?? editor.undoConflict ?? editor.revisionConflict { throw failure }
            try await Task.sleep(for: .milliseconds(20))
        }
        throw GUICheckError(message: "GUI edit/reload exceeded 45 seconds")
    }
    func verifyEquivalence() async throws {
        let (folder, editor, transport) = try await fixture()
        defer { try? FileManager.default.removeItem(at: folder) }
        let baseline = try await snapshot(editor)
        let initial = try script(), shapeID = initial.object("document").objects("compositions")[0].objects("nodes")[0].string("id")
        let curve = initial.object("document").objects("curves")[0].string("id")
        var results: [[String: Any]] = []
        for step in initial.objects("steps") {
            print("QA scenario: \(step.string("name"))")
            let base = editor.revision, plans = transport.planCount, applies = transport.applyCount
            let layer = editor.layers.first { $0.id == shapeID }!
            editor.select(layer.id)
            switch step.string("action") {
            case "numeric":
                let property = layer.property(step.string("property"))!, axis = Int(step.number("axis"))
                let scale = PropertyPresentation.of(property).multiplier
                let from = editor.propertyNumbers(layer, property)[axis] * scale, to = step.number("display")
                for preview in [from, (from + to) / 2, to] { editor.previewNumber(layer: layer, property: property, axis: axis, to: preview / scale) }
                try require(transport.planCount == plans && transport.applyCount == applies, "Display-unit previews never edit")
                editor.commitNumber(layer: layer, property: property, axis: axis, from: from / scale, to: to / scale)
            case "rename": editor.rename(layer, name: step.string("value"))
            case "text": editor.setText(editor.layers.first { $0.kind == "text" }!, to: step.string("value"))
            case "enabled": editor.toggleEnabled(layer)
            case "insert":
                editor.ui.time = .init(num: Int64(step.number("frame")), den: 24); try await editor.reload()
                editor.toggleKeyframe(editor.layers.first { $0.id == shapeID }!, property: layer.property("kronello.transform.position")!)
            case "move", "interpolation":
                let reference = KeyReference(curve: curve, time: .init(num: Int64(step.number("frame")), den: 24))
                editor.selectKey(reference)
                if step.string("action") == "move" { editor.commitKeyMove(editor.beginKeyGesture()!, delta: Int64(step.number("delta"))) }
                else { try requireResult(await editor.apply(editor.interpolationCandidate(step.string("mode"))!) != nil, "GUI interpolation applies") }
            case "tangent":
                let keys = editor.curveKeys(curve)
                let candidate = try editor.tangentCandidate(curve: curve, index: 2, incoming: false,
                    control: .init(x: 0.25, y: step["aligned"] as! Bool ? 0.6 : 0.4), aligned: step["aligned"] as! Bool,
                    axis: 0, keys: keys, base: base)
                try requireResult(await editor.apply(candidate) != nil, "GUI tangent applies")
            case "delete":
                let keys = editor.curveKeys(curve), property = layer.property("kronello.transform.position")!
                let selected = step["last"] as! Bool ? keys : Array(keys.dropLast())
                editor.keySelection = Set(selected.map { editor.reference(property, $0) })
                editor.deleteSelectedKeys()
            case "undo": await editor.undo()
            case "redo": await editor.undo(redo: true)
            default: throw GUICheckError(message: "Unknown QA scenario")
            }
            do { try await waitForEdit(editor, after: base) }
            catch { throw GUICheckError(message: "\(step.string("name")): \(error); base=\(base) revision=\(editor.revision) plans=\(transport.planCount - plans) applies=\(transport.applyCount - applies) pending=\(String(describing: editor.revisionConflict))") }
            try require(Int(editor.revision) == Int(base)! + 1, "Exactly one revision per scenario")
            try require(editor.failure == nil && editor.previewFailure == nil && editor.undoConflict == nil, "No typed edit/evaluation failure")
            var result = try await snapshot(editor)
            result["name"] = step.string("name"); result["action"] = step.string("action")
            result["client"] = ["session_id": transport.lastEdit.string("session_id"), "idempotency_key": transport.lastEdit.string("idempotency_key")]
            if !["undo", "redo"].contains(step.string("action")) {
                try require(transport.planCount == plans + 1 && transport.applyCount == applies + 1, "One plan/apply batch per GUI action")
                let commands = transport.lastApply.objects("commands")
                guard let expected = step["commands"] as? [[String: Any]] else { throw GUICheckError(message: "Scenario must pin its expected commands") }
                try require(NSArray(array: commands) == NSArray(array: expected), "GUI builds the scripted shared edit, including display conversion")
                result["commands"] = commands
            } else { try require(transport.planCount == plans && transport.applyCount == applies, "Undo/Redo use edit.undo") }
            results.append(result)
        }
        try write(["format": 1, "path": "gui-app-model-real-ffi", "fixture": initial, "baseline": baseline, "steps": results], name: "gui.json")
        await editor.close()
    }
    func verifyIME() async throws {
        let (folder, editor, transport) = try await fixture()
        defer { try? FileManager.default.removeItem(at: folder) }
        var checks: [[String: Any]] = []
        for kind in ["rename", "text"] {
            let layer = editor.layers.first { kind == "text" ? $0.kind == "text" : $0.kind == "shape" }!
            let view = KRCommittedTextView(frame: .init(x: 0, y: 0, width: 300, height: 24))
            view.synchronize(kind == "text" ? editor.textDocument(layer)!.string("text") : layer.name)
            var callbacks = 0
            view.onCommit = { value in
                callbacks += 1
                if kind == "text" { editor.setText(layer, to: value) } else { editor.rename(layer, name: value) }
            }
            let base = editor.revision, plans = transport.planCount, applies = transport.applyCount
            for text in ["に", "にほ", "にほん", "日本", "日本語"] {
                view.setMarkedText(text, selectedRange: NSRange(location: (text as NSString).length, length: 0), replacementRange: NSRange(location: 0, length: (view.string as NSString).length))
                view.commitDraft() // Return/blur must not send marked text even if invoked.
                try await Task.sleep(for: .milliseconds(10))
                try require(view.hasMarkedText() && callbacks == 0 && transport.planCount == plans && transport.applyCount == applies && editor.revision == base, "Marked typing/conversion/candidate selection issues zero requests")
            }
            view.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
            view.commitDraft(); try await Task.sleep(for: .milliseconds(30))
            try require(!view.hasMarkedText() && callbacks == 0 && transport.planCount == plans && transport.applyCount == applies, "Escape cancels without a command")
            let committed = "確定 か\u{3099} 葛\u{e0100}"
            view.setMarkedText(committed, selectedRange: NSRange(location: (committed as NSString).length, length: 0), replacementRange: NSRange(location: 0, length: (view.string as NSString).length))
            view.unmarkText()
            view.commitDraft(); view.doCommand(by: #selector(NSResponder.insertNewline(_:))); _ = view.resignFirstResponder()
            try await waitForEdit(editor, after: base)
            try require(callbacks == 1 && transport.planCount == plans + 1 && transport.applyCount == applies + 1, "IME commit then Return/blur sends exactly one batch")
            let saved = kind == "text" ? editor.document.objects("texts")[0].string("text") : editor.layers.first { $0.id == layer.id }!.name
            try require(Array(saved.utf8) == Array(committed.utf8), "Combining marks and IVS preserve exact UTF-8")
            // NSTextInputClient also completes composition with insertText rather than unmarkText.
            let secondBase = editor.revision
            view.setMarkedText("つぎ", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: 0, length: (view.string as NSString).length))
            view.insertText("次の確定", replacementRange: NSRange(location: NSNotFound, length: 0))
            view.commitDraft()
            try await waitForEdit(editor, after: secondBase)
            try require(callbacks == 2 && transport.planCount == plans + 2 && transport.applyCount == applies + 2, "insertText commits once despite nested unmarkText")
            checks.append(["field": kind, "marked_requests": 0, "cancel_requests": 0, "commits": 2, "plan_count": transport.planCount - plans, "apply_count": transport.applyCount - applies])
        }
        let shapeID = editor.layers.first { $0.kind == "shape" }!.id
        let view = KRCommittedTextView(frame: .init(x: 0, y: 0, width: 88, height: 24))
        let opacity = "kronello.opacity", presentation = PropertyPresentation.of(opacity)
        let originalLayer = editor.layers.first { $0.id == shapeID }!
        view.synchronize(String(editor.propertyNumbers(originalLayer, originalLayer.property(opacity)!)[0] * presentation.multiplier))
        var numericCallbacks = 0
        view.onCommit = { text in
            let layer = editor.layers.first { $0.id == shapeID }!, property = layer.property(opacity)!
            let from = editor.propertyNumbers(layer, property)[0] * presentation.multiplier
            var transaction = KRNumberEdit(value: from); transaction.beginEditing()
            guard transaction.type(text), let commit = transaction.finish() else { return }
            numericCallbacks += 1
            editor.commitNumber(layer: layer, property: property, axis: 0, from: commit.from / presentation.multiplier, to: commit.to / presentation.multiplier)
        }
        let plans = transport.planCount, applies = transport.applyCount, base = editor.revision
        view.setMarkedText("48.25", selectedRange: NSRange(location: 5, length: 0), replacementRange: NSRange(location: 0, length: (view.string as NSString).length))
        view.commitDraft(); try await Task.sleep(for: .milliseconds(30))
        try require(transport.planCount == plans && transport.applyCount == applies && numericCallbacks == 0, "Marked numeric text, even if parseable, never plans")
        view.doCommand(by: #selector(NSResponder.cancelOperation(_:))); view.commitDraft()
        try require(numericCallbacks == 0, "Numeric Escape has no command")
        view.setMarkedText("48.25", selectedRange: NSRange(location: 5, length: 0), replacementRange: NSRange(location: 0, length: (view.string as NSString).length))
        view.unmarkText(); view.commitDraft()
        try await waitForEdit(editor, after: base)
        let secondBase = editor.revision
        view.insertText("6", replacementRange: NSRange(location: 0, length: (view.string as NSString).length))
        for digit in ["2", ".", "5"] { view.insertText(digit, replacementRange: NSRange(location: NSNotFound, length: 0)) }
        try await Task.sleep(for: .milliseconds(30))
        try require(numericCallbacks == 1 && transport.planCount == plans + 1 && transport.applyCount == applies + 1, "Ordinary numeric keystrokes remain draft")
        view.doCommand(by: #selector(NSResponder.insertNewline(_:))); view.commitDraft(); _ = view.resignFirstResponder()
        try await waitForEdit(editor, after: secondBase)
        try require(numericCallbacks == 2 && transport.planCount == plans + 2 && transport.applyCount == applies + 2, "Numeric IME and Return each commit once")
        checks.append(["field": "number-opacity", "marked_requests": 0, "cancel_requests": 0, "commits": 2, "plan_count": transport.planCount - plans, "apply_count": transport.applyCount - applies])
        let textLayer = editor.layers.first { $0.kind == "text" }!
        let unicodeView = KRCommittedTextView(frame: .zero)
        unicodeView.synchronize(editor.textDocument(textLayer)!.string("text"))
        var unicodeCallbacks = 0
        unicodeView.onCommit = { value in unicodeCallbacks += 1; editor.setText(textLayer, to: value) }
        let unicodePlans = transport.planCount, unicodeApplies = transport.applyCount
        for text in ["が", "か\u{3099}"] {
            let before = editor.revision
            unicodeView.setMarkedText(text, selectedRange: NSRange(location: (text as NSString).length, length: 0), replacementRange: NSRange(location: 0, length: (unicodeView.string as NSString).length))
            unicodeView.unmarkText(); unicodeView.commitDraft()
            try await waitForEdit(editor, after: before)
            let saved = editor.textDocument(editor.layers.first { $0.kind == "text" }!)!.string("text")
            try require(Array(saved.utf8) == Array(text.utf8), "Canonically equivalent strings remain distinct authored UTF-8")
        }
        try require(unicodeCallbacks == 2 && transport.planCount == unicodePlans + 2 && transport.applyCount == unicodeApplies + 2, "NFC-to-NFD commits exactly once instead of being silently dropped")
        checks.append(["field": "text-unicode-bytes", "commits": 2, "plan_count": 2, "apply_count": 2])
        try write(["format": 1, "path": "NSTextInputClient-to-editor-real-ffi", "checks": checks, "snapshot": try await snapshot(editor)], name: "ime.json")
        await editor.close()
    }
    func runAll() async throws {
        try await verifyEquivalence(); print("PASS QA-002 GUI scripted edits, pinned commands, revisions and full snapshots")
        try await verifyIME(); print("PASS QA-002 marked text, Escape, unmarkText/insertText, exactly one command per commit")
    }
}
