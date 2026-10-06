import AppKit
import QuartzCore
import KronelloCore

@MainActor final class PreviewView: NSView {
    let metalLayer = CAMetalLayer()
    var onResize: (() -> Void)?
    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer = metalLayer
        metalLayer.isOpaque = true
    }
    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
    override func layout() {
        super.layout()
        let scale = window?.backingScaleFactor ?? 1
        metalLayer.contentsScale = scale
        metalLayer.drawableSize = CGSize(width: bounds.width * scale, height: bounds.height * scale)
        onResize?()
    }
}
@MainActor final class Harness: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    var view: PreviewView!
    var session: ProjectSession?
    var renderRequest: [String: Any] = [:]
    var rendering = false
    var needsRender = false
    var timer: Timer?
    func applicationDidFinishLaunching(_ notification: Foundation.Notification) {
        NSApp.setActivationPolicy(.regular)
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 320),
                          styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "Kronello FFI preview harness"
        view = PreviewView(frame: window.contentView!.bounds)
        view.autoresizingMask = [.width, .height]
        window.contentView = view
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        Task { do { try await start() } catch { fail(error) } }
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationWillTerminate(_ notification: Foundation.Notification) { timer?.invalidate(); session?.close() }
    func request(_ object: [String: Any]) throws -> API.Request {
        try JSONDecoder().decode(API.Request.self, from: JSONSerialization.data(withJSONObject: object))
    }
    func call(_ object: [String: Any]) async throws -> [String: Any] {
        let response = try await session!.call(request(object))
        let data = try JSONEncoder().encode(response)
        let decoded = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        guard decoded["status"] as? String == "success" else {
            throw NativeError.service("HARNESS_REQUEST_FAILED", String(data: data, encoding: .utf8)!)
        }
        return (decoded["result"] as! [String: Any])["value"] as! [String: Any]
    }
    func start() async throws {
        let args = CommandLine.arguments
        guard args.count == 3 || args.count == 4 else {
            throw NativeError.service("USAGE", "KronelloPreviewHarness PROJECT.kronello DOCUMENT.json [KRONELLO_CLI]")
        }
        let project = URL(fileURLWithPath: args[1]).path
        session = try ProjectSession(path: project, workerExecutable: args.count == 4 ? args[3] : nil)
        let opened = try await session!.ready()
        if case .variant1(let failure) = opened {
            guard failure.error.code == "PROJECT_NOT_FOUND" else {
                throw NativeError.service(failure.error.code, failure.error.message)
            }
            let document = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: args[2])))
            _ = try await call(["operation": "project.create", "project": project, "document": document])
        }
        let exported = try await call(["operation": "project.export", "project": project])
        let document = exported["document"] as! [String: Any]
        let composition = (document["compositions"] as! [[String: Any]])[0]
        let node = (composition["nodes"] as! [[String: Any]])[0]
        let property = (node["properties"] as! [[String: Any]]).first {
            ($0["descriptor"] as? [String: Any])?["key"] as? String == "kronello.shape.size"
        }!
        let commands: [[String: Any]] = [["property_source_set": [
            "object": node["id"]!, "property": property["id"]!,
            "source": ["kind": "constant", "value": ["kind": "vec2", "value": [24.0, 16.0]]]
        ]]]
        let base = exported["revision"]!
        let plan = try await call(["operation": "edit.plan", "project": project, "base_revision": base, "commands": commands])
        let event = try await call(["operation": "edit.apply", "project": project, "base_revision": base,
                                   "commands": commands, "plan_hash": plan["plan_hash"]!, "session_id": UUID().uuidString,
                                   "idempotency_key": UUID().uuidString])
        print("Swift edit event: \(event)")
        renderRequest = ["operation": "render.frame", "input": [
            "project": project, "composition": composition["id"]!,
            "region": ["origin": [0, 0], "extent": [64, 32], "pixels": [640, 320]]
        ], "time": ["num": "0", "den": "1"]]
        try await session!.attach(metalLayer: Unmanaged.passUnretained(view.metalLayer).toOpaque(), width: 640, height: 320)
        session!.onNotification = { [weak self] notification in
            print("Notification: \(notification.name)")
            if notification.name == "revision_changed" { self?.scheduleRender() }
        }
        try await session!.subscribe()
        view.onResize = { [weak self] in self?.scheduleRender() }
        timer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            Task { @MainActor in do { try self?.session?.poll() } catch { self?.fail(error) } }
        }
        scheduleRender()
    }
    func scheduleRender() {
        needsRender = true
        guard !rendering, session != nil, !renderRequest.isEmpty else { return }
        rendering = true
        Task {
            defer { rendering = false }
            do {
                while needsRender {
                    needsRender = false
                    let size = view.metalLayer.drawableSize
                    guard size.width > 0, size.height > 0 else { continue }
                    let width = UInt32(size.width), height = UInt32(size.height)
                    try await session!.resize(width: width, height: height)
                    var input = renderRequest["input"] as! [String: Any]
                    var region = input["region"] as! [String: Any]
                    region["pixels"] = [width, height]
                    input["region"] = region
                    renderRequest["input"] = input
                    let result = try await session!.redraw(request(renderRequest))
                    print("Presented: \(result)")
                }
            } catch { fail(error) }
        }
    }
    func fail(_ error: Error) {
        fputs("\(error)\n", stderr)
        session?.close()
        NSApp.terminate(nil)
    }
}
MainActor.assumeIsolated {
    let app = NSApplication.shared
    let harness = Harness()
    app.delegate = harness
    app.run()
}
