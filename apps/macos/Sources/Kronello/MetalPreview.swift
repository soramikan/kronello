import AppKit
import QuartzCore
import SwiftUI
import KronelloCore
import KronelloAppModel

@MainActor final class MetalView: NSView {
    let metal = CAMetalLayer()
    var changed: ((Bool) -> Void)?
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
                self.changed?(true)
            }
        }
        // A pre-attachment redraw can be skipped as occluded. Joining an
        // already visible window emits no occlusion change, so request the
        // first presentation even when geometry and frame identity match.
        changed?(true)
    }
    override func layout() {
        super.layout()
        let scale = window?.backingScaleFactor ?? 1
        metal.contentsScale = scale
        metal.drawableSize = CGSize(width: bounds.width * scale, height: bounds.height * scale)
        changed?(false)
    }
}

struct MetalPreview: NSViewRepresentable {
    @ObservedObject var model: EditorModel
    func makeCoordinator() -> Coordinator { Coordinator(model) }
    func makeNSView(context: Context) -> MetalView {
        let view = MetalView()
        context.coordinator.view = view
        view.changed = { [weak coordinator = context.coordinator] force in coordinator?.schedule(force: force) }
        return view
    }
    func updateNSView(_ view: MetalView, context: Context) { context.coordinator.schedule() }
    static func dismantleNSView(_ view: MetalView, coordinator: Coordinator) { view.changed = nil; coordinator.task?.cancel() }
    @MainActor final class Coordinator {
        let model: EditorModel
        weak var view: MetalView?
        var attached = false
        var configuredSize: [UInt32] = []
        var needsRender = false
        var task: Task<Void, Never>?
        var scheduledKey = ""
        func trace(_ message: String) {
            guard ProcessInfo.processInfo.environment["KRONELLO_PREVIEW_TRACE"] != nil else { return }
            FileHandle.standardError.write(Data(("preview " + URL(fileURLWithPath: model.path).lastPathComponent + " " + message + "\n").utf8))
        }
                init(_ model: EditorModel) {
            self.model = model
            model.waitForVideoPresentation = { [weak self] in
                if let task = self?.task { await task.value }
            }
        }
        func schedule(force: Bool = false) {
            guard let surface = view else { return }
            guard surface.window != nil, surface.bounds.width > 0, surface.bounds.height > 0 else { trace("defer unattached/empty view"); return }
            if model.usesCPUReference && model.playing { needsRender = false; scheduledKey = ""; return }
            let key = [model.ui.page, model.revision, "\(model.refreshToken)", model.ui.sequence ?? "", model.ui.composition ?? "", model.ui.time.num, model.ui.time.den,
                       model.ui.resolution, model.ui.zoom, model.usesCPUReference ? "cpu_reference" : "gpu", "\(surface.bounds.size)", "\(surface.window?.backingScaleFactor ?? 1)"].joined(separator: ":")
            guard force || key != scheduledKey else { return }
            scheduledKey = key
            trace("schedule force=\(force) revision=\(model.revision) time=\(model.ui.time.num)/\(model.ui.time.den)")
            needsRender = true
            guard task == nil else { return }
            task = Task {
                defer { task = nil; model.previewRendering = false }
                trace("task start")
                while needsRender && !Task.isCancelled {
                    let identity = model.previewIdentity
                    let cpuReference = model.usesCPUReference
                    let requestedKey = scheduledKey
                    let time = model.ui.time
                    let extent = model.extent
                    do {
                        needsRender = false
                        guard let view, let native = model.transport as? NativeProjectTransport else { return }
                        let target: [String: Any]
                        if model.ui.page == "edit", let sequence = model.ui.sequence { target = ["target": ["kind": "sequence", "sequence": sequence]] }
                        else if let composition = model.ui.composition { target = ["composition": composition] }
                        else { return }
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
                            configuredSize = [width, height]
                        } else if configuredSize != [width, height] {
                            try await native.session.resize(width: width, height: height); configuredSize = [width, height]
                        }
                        model.previewRendering = true
                        var input: [String: Any] = ["project": model.path, "fonts": model.snapshotFonts,
                            "region": ["origin": [0, 0], "extent": [extent.width, extent.height], "pixels": [width, height]]]
                        input.merge(target) { _, value in value }
                        var request: [String: Any] = ["operation": "render.frame", "input": input, "time": time.wire]
                        if cpuReference { request["backend"] = "cpu_reference" }
                        guard !Task.isCancelled, requestedKey == scheduledKey, identity == model.previewIdentity else { continue }
                        let frame = model.frame, epoch = model.playback.clockEpoch
                        trace("redraw pixels=\(width)x\(height) visible=\(view.window?.occlusionState.contains(.visible) == true)")
                        let response = try await native.session.redraw(NativeProjectTransport.request(request))
                        guard !Task.isCancelled, requestedKey == scheduledKey, identity == model.previewIdentity else { continue }
                        if cpuReference, case .object(let object) = response, case .object(let preview) = object["preview"], preview["backend"] != .string("cpu_reference_float32") {
                            throw ServiceFailure(code: "PREVIEW_BACKEND_MISMATCH", message: "CPU 参照の描画結果を確認できません")
                        }
                        if case .object(let object) = response, case .object(let preview) = object["preview"], preview["presented"] == .bool(true) {
                            model.previewPresented = identity
                            trace("presented revision=\(model.revision)")
                        } else if case .object(let object) = response, case .object(let preview) = object["preview"], preview["presented"] == .bool(false) {
                            trace("skipped \(preview["skipped"] ?? .null)")
                        }
                        if model.playing, model.playback.master == .audioDevice, epoch == model.playback.clockEpoch,
                           ProcessInfo.processInfo.environment["KRONELLO_AUDIO_TRACE"] != nil {
                            try model.playbackEvidence.presented(frame: frame, rateNum: model.activePlaybackRateNum, rateDen: model.activePlaybackRateDen, playback: model.playback, response: response)
                        }
                        model.reportPreviewFailure(nil, for: identity)
                    } catch is CancellationError { return } catch {
                        trace("failed \(error)")
                        if requestedKey == scheduledKey { model.reportPreviewFailure(model.serviceFailure(error), for: identity) }
                    }
                }
            }
        }
    }
}
