import AppKit
import SwiftUI
import KronelloAppModel

/// Native preview for the selected export target; no per-frame inspection or readback.
struct ExportMetalPreview: NSViewRepresentable {
    let editor: EditorModel
    let input: [String: Any]
    let time: [String: Any]
    let onFailure: (ServiceFailure) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(editor, onFailure: onFailure) }
    func makeNSView(context: Context) -> MetalView {
        let view = MetalView(); context.coordinator.view = view
        view.changed = { [weak coordinator = context.coordinator] in coordinator?.schedule() }
        return view
    }
    func updateNSView(_ view: MetalView, context: Context) { context.coordinator.input = input; context.coordinator.time = time; context.coordinator.schedule() }
    static func dismantleNSView(_ view: MetalView, coordinator: Coordinator) { view.changed = nil; coordinator.task?.cancel() }
    @MainActor final class Coordinator {
        let editor: EditorModel
        let onFailure: (ServiceFailure) -> Void
        weak var view: MetalView?
        var input: [String: Any] = [:]
        var time: [String: Any] = [:]
        var attached = false
        var pending = false
        var task: Task<Void,Never>?
        init(_ editor: EditorModel, onFailure: @escaping (ServiceFailure) -> Void) { self.editor = editor; self.onFailure = onFailure }
        func schedule() {
            pending = true
            guard task == nil else { return }
            task = Task {
                defer { task = nil }
                while pending && !Task.isCancelled {
                    pending = false
                    guard let view, let native = editor.transport as? NativeProjectTransport, !input.isEmpty else { continue }
                    do {
                        let width = UInt32(max(1, view.metal.drawableSize.width)), height = UInt32(max(1,view.metal.drawableSize.height))
                        if !attached { try await native.session.attach(metalLayer: Unmanaged.passUnretained(view.metal).toOpaque(), width: width, height: height); attached = true }
                        else { try await native.session.resize(width: width, height: height) }
                        var render = input, region = input.object("region"); region["pixels"] = [width,height]; render["region"] = region
                        _ = try await native.session.redraw(NativeProjectTransport.request(["operation":"render.frame","input":render,"time":time]))
                    } catch is CancellationError {} catch { onFailure(editor.serviceFailure(error)) }
                }
            }
        }
    }
}
