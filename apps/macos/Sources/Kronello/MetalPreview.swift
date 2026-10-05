import AppKit
import QuartzCore
import SwiftUI
import KronelloCore
import KronelloAppModel

@MainActor final class MetalView: NSView {
    let metal = CAMetalLayer()
    var changed: (() -> Void)?
    override init(frame: NSRect) { super.init(frame: frame); wantsLayer = true; layer = metal; metal.isOpaque = true }
    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
    // Occluded frames are skipped by the FFI (not failures); redraw once visible again.
    private var occlusion: NSObjectProtocol?
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if let occlusion { NotificationCenter.default.removeObserver(occlusion) }
        occlusion = nil
        guard let window else { return }
        occlusion = NotificationCenter.default.addObserver(forName: NSWindow.didChangeOcclusionStateNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, self.window?.occlusionState.contains(.visible) == true else { return }
                self.changed?()
            }
        }
    }
    override func layout() {
        super.layout()
        let scale = window?.backingScaleFactor ?? 1
        metal.contentsScale = scale
        metal.drawableSize = CGSize(width: bounds.width * scale, height: bounds.height * scale)
        changed?()
    }
}

struct MetalPreview: NSViewRepresentable {
    @ObservedObject var model: EditorModel
    func makeCoordinator() -> Coordinator { Coordinator(model) }
    func makeNSView(context: Context) -> MetalView {
        let view = MetalView()
        context.coordinator.view = view
        view.changed = { [weak coordinator = context.coordinator] in coordinator?.schedule() }
        return view
    }
    func updateNSView(_ view: MetalView, context: Context) { context.coordinator.schedule() }
    static func dismantleNSView(_ view: MetalView, coordinator: Coordinator) { view.changed = nil; coordinator.task?.cancel() }
    @MainActor final class Coordinator {
        let model: EditorModel
        weak var view: MetalView?
        var attached = false
        var needsRender = false
        var task: Task<Void, Never>?
        init(_ model: EditorModel) { self.model = model }
        func schedule() {
            needsRender = true
            guard task == nil else { return }
            task = Task {
                defer { task = nil }
                do {
                    while needsRender && !Task.isCancelled {
                        needsRender = false
                        guard let view, let native = model.transport as? NativeProjectTransport, let composition = model.ui.composition else { return }
                        // Surface configuration can change drawableSize. Always derive the next
                        // extent from view geometry so repeated half/quarter requests do not shrink.
                        let backing = view.window?.backingScaleFactor ?? 1
                        let zoom = model.ui.zoom == "fit" ? 1 : (Double(model.ui.zoom) ?? 100) / 100 * model.extent.width / max(1, view.bounds.width)
                        let size = CGSize(width: view.bounds.width * backing * zoom, height: view.bounds.height * backing * zoom)
                        guard size.width > 0, size.height > 0, model.extent.width > 0, model.extent.height > 0 else { return }
                        let divisor: Double = model.ui.resolution == "half" ? 2 : model.ui.resolution == "quarter" ? 4 : 1
                        guard size.width.isFinite, size.height.isFinite, size.width <= Double(UInt32.max), size.height <= Double(UInt32.max) else {
                            throw ServiceFailure(code: "INVALID_PREVIEW_SIZE", message: "Viewer の描画サイズが範囲外です")
                        }
                        let width = UInt32(max(1, size.width / divisor)), height = UInt32(max(1, size.height / divisor))
                        if !attached {
                            try await native.session.attach(metalLayer: Unmanaged.passUnretained(view.metal).toOpaque(), width: width, height: height)
                            attached = true
                        } else { try await native.session.resize(width: width, height: height) }
                        _ = try await native.session.redraw(NativeProjectTransport.request(["operation": "render.frame", "input": [
                            "project": model.path, "composition": composition, "fonts": model.fonts,
                            "region": ["origin": [0, 0], "extent": [model.extent.width, model.extent.height], "pixels": [width, height]]], "time": model.ui.time.wire]))
                    }
                } catch is CancellationError {} catch { model.previewFailure = model.serviceFailure(error) }
            }
        }
    }
}
