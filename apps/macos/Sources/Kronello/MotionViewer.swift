import AppKit
import SwiftUI
import KronelloAppModel
import KronelloDesign

struct MotionViewer: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    @State private var canvasEdit: CanvasEdit?
    @State private var panOrigin: CGPoint?
    @State private var penPoints: [CGPoint] = []
    @State private var playingTask: Task<Void, Never>?
    @State private var floatingPreview = CGSize.zero
    @State private var spatialPath = SpatialPath()
    @State private var parentPath = SpatialPath()
    @State private var pathParent: CGAffineTransform?
    @State private var pathFailure: ServiceFailure?
    @State private var viewerSize = CGSize.zero
    @FocusState private var viewerFocused: Bool
    var body: some View {
        KRPanel(header: {
            KRTabBar(model.compositions.map { .init($0.string("id"), "Composition") }, selection: Binding(get: { model.ui.composition ?? "" }, set: model.setComposition))
        }, actions: {
            KRButton(icon: .magnet, accessibilityLabel: "スナップ") {}.disabled(true).help("ガイドへのスナップは後続タスクです")
            KRButton(icon: .grid3x3, accessibilityLabel: "ガイド") {}.disabled(true).help("ガイド編集は後続タスクです")
        }) {
            VStack(spacing: 0) {
                if let error = model.revisionConflict {
                    KRConflictBanner(.init(error.code, "移動を適用できませんでした。" + error.message), discard: model.discardCandidate, reapply: model.reapply)
                }
                GeometryReader { proxy in
                    ZStack(alignment: .topLeading) {
                        HStack(spacing: 0) {
                            if model.ui.layout.tools.location == .viewerLeft { toolstrip }
                            stage
                            if model.ui.layout.tools.location == .viewerRight { toolstrip }
                        }
                        if case .floating(let x, let y) = model.ui.layout.tools.location {
                            toolstrip.fixedSize().offset(x: x + floatingPreview.width, y: y + floatingPreview.height)
                        }
                    }.onAppear { viewerSize = proxy.size }
                        .onChange(of: proxy.size) { _, size in viewerSize = size; floatingPreview = .zero }
                }.frame(maxHeight: .infinity).background(p.surface0)
                KRTransportBar(frames: Binding(get: { model.frame }, set: { _ in }), fps: model.nominalFPS, duration: model.durationCode,
                    playing: $model.playing, looping: $model.ui.looping, zoom: $model.ui.zoom, resolution: $model.ui.resolution,
                    onSeek: model.seek, onStep: { model.seek(model.frame + Int64($0)) }, onBoundary: { model.seek($0 ? model.durationFrames - 1 : 0) })
            }
        }.task(id: model.revision + "/" + (model.ui.selection ?? "") + "/" + (model.ui.composition ?? "")) {
            spatialPath = .init(); parentPath = .init(); pathParent = nil; pathFailure = nil
            do { parentPath = try await model.spatialPath(); remapPath() }
            catch is CancellationError {} catch { if !Task.isCancelled { pathFailure = model.serviceFailure(error) } }
        }.onChange(of: model.refreshToken) { _, _ in remapPath() }
            .onChange(of: model.playing) { _, value in
            playingTask?.cancel()
            if value {
                playingTask = Task { @MainActor in
                    while !Task.isCancelled && model.playing {
                        try? await Task.sleep(for: .seconds(Double(model.rateDen) / Double(model.rateNum)))
                        if !Task.isCancelled { model.tick() }
                    }
                }
            }
        }.onDisappear { playingTask?.cancel(); model.playing = false }
    }
    func remapPath() {
        guard !parentPath.points.isEmpty || !parentPath.keys.isEmpty else { return }
        do {
            let parent = try model.spatialPathParentTransform()
            if parent != pathParent {
                spatialPath = parent.map { parentPath.mapped(by: $0) } ?? .init()
                pathParent = parent
            }
            pathFailure = nil
        } catch { spatialPath = .init(); pathParent = nil; pathFailure = model.serviceFailure(error) }
    }
    var toolstrip: some View {
        KRToolStrip(tools, selection: $model.ui.tool, placement: Binding(get: { model.ui.layout.tools }, set: { model.ui.layout.tools = $0 }),
            onMovePreview: { floatingPreview = $0 }, onMoveCommit: { delta in
                let current = model.ui.layout.tools.location
                func placement(_ x: Double, _ y: Double) -> KRToolStripPlacement.Location {
                    if x < KRSpace.space4 { return .viewerLeft }
                    if x > viewerSize.width - KRWindowMetrics.tools - KRSpace.space4 { return .viewerRight }
                    return .floating(x: min(max(0, x), max(0, viewerSize.width - KRWindowMetrics.tools)), y: min(max(0, y), max(0, viewerSize.height - KRWindowMetrics.toolHeight * Double(tools.count))))
                }
                switch current {
                case .viewerLeft: model.ui.layout.tools.location = placement(delta.width, delta.height)
                case .viewerRight: model.ui.layout.tools.location = placement(viewerSize.width - KRWindowMetrics.tools + delta.width, delta.height)
                case .floating(let x, let y):
                    model.ui.layout.tools.location = placement(x + delta.width, y + delta.height)
                }
                floatingPreview = .zero
            })
    }
    var tools: [KRTool] {
        KRTool.motion.map { tool in
            var tool = tool
            if tool.id == "text", model.textFont == nil { tool.unavailableReason = "FONT_MISSING · 既存の font lock と明示的な font path が必要です" }
            return tool
        }
    }
    @ViewBuilder var stage: some View {
        if let error = model.previewFailure {
            KRViewerError(.init(error.code, error.message), copy: {
                NSPasteboard.general.clearContents(); NSPasteboard.general.setString(error.code + "\n" + error.message, forType: .string)
            }, retry: { model.previewFailure = nil; Task { do { try await model.reload() } catch { model.mapFailure(error) } } }).padding(KRSpace.space4)
        } else if model.current.isEmpty {
            KREmptyState(icon: .layers, title: "Composition がありません", message: "Composition を含むプロジェクトを開いてください。")
        } else {
            GeometryReader { proxy in
                let extent = model.extent
                let aspect = extent.width / max(1, extent.height)
                let size = KRViewerFrame<EmptyView>.fittingSize(container: proxy.size, aspectRatio: aspect)
                let zoom = model.ui.zoom == "fit" ? 1 : (Double(model.ui.zoom) ?? 100) / 100 * model.extent.width / max(1, size.width)
                ZStack {
                    KRViewerFrame(aspectRatio: aspect) { MetalPreview(model: model) }
                    Canvas { context, canvas in
                        func screen(_ point: CGPoint) -> CGPoint { .init(x: point.x / extent.width * canvas.width, y: point.y / extent.height * canvas.height) }
                        var path = Path()
                        for (i, point) in spatialPath.points.enumerated() { if i == 0 { path.move(to: screen(point)) } else { path.addLine(to: screen(point)) } }
                        context.stroke(path, with: .color(p.selection), lineWidth: 1 / max(0.01, zoom))
                        for point in spatialPath.keys { let point = screen(point), side = 5 / max(0.01, zoom); context.fill(Path(CGRect(x: point.x - side / 2, y: point.y - side / 2, width: side, height: side)), with: .color(p.selection)) }
                    }.allowsHitTesting(false)
                    if let pathFailure { VStack { KRErrorLine(.init(pathFailure.code, pathFailure.message)); Spacer() }.allowsHitTesting(false) }
                    if model.ui.tool == "select", let selected = model.selected, !model.ui.locked.contains(selected.id),
                       let bounds = model.candidateBounds ?? selected.bounds(model.ui.bounds) {
                        let selection = KRViewerSelection(CGRect(x: bounds.minX / model.extent.width, y: bounds.minY / model.extent.height,
                            width: bounds.width / model.extent.width, height: bounds.height / model.extent.height), label: "\(model.ui.bounds) \(Int(bounds.width.rounded())) × \(Int(bounds.height.rounded()))")
                        KRManipulationOverlay(selection, onPreview: { translation, handle, rotate in
                            if canvasEdit == nil { canvasEdit = model.beginCanvasEdit() }
                            if let edit = canvasEdit { model.previewCanvas(edit, translation: translation, handle: handle, rotate: rotate) }
                        }, onCommit: { translation, handle, rotate in
                            if let edit = canvasEdit { model.commitCanvas(edit, translation: translation, handle: handle, rotate: rotate) }
                            canvasEdit = nil
                        })
                    }
                }.frame(width: size.width, height: size.height)
                    .contentShape(Rectangle()).gesture(backgroundGesture(size: size))
                    .scaleEffect(zoom).offset(x: model.ui.panX, y: model.ui.panY)
                    .position(x: proxy.size.width / 2, y: proxy.size.height / 2)
            }.padding(KRSpace.space4).clipped().focusable().focused($viewerFocused).focusEffectDisabled().krFocusRing(viewerFocused, inset: true)
                .onKeyPress(.space) { model.playing.toggle(); return .handled }
                .onKeyPress(.leftArrow) { model.seek(model.frame - 1); return .handled }
                .onKeyPress(.rightArrow) { model.seek(model.frame + 1); return .handled }
                .onKeyPress(.home) { model.seek(0); return .handled }
                .onKeyPress(.end) { model.seek(model.durationFrames - 1); return .handled }
                .onKeyPress(.escape) { canvasEdit = nil; model.candidateBounds = nil; return .handled }
                .onKeyPress(characters: CharacterSet(charactersIn: "vhzm ept".replacingOccurrences(of: " ", with: ""))) { press in
                    let map = ["v": "select", "h": "hand", "z": "zoom", "m": "rectangle", "e": "ellipse", "p": "pen", "t": "text"]
                    guard let tool = map[press.characters.lowercased()], tool != "text" || model.textFont != nil else { return .ignored }
                    model.ui.tool = tool; return .handled
                }
        }
    }
    func backgroundGesture(size: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 0).onChanged { value in
            viewerFocused = true
            if model.ui.tool == "hand" {
                if panOrigin == nil { panOrigin = CGPoint(x: model.ui.panX, y: model.ui.panY) }
                model.ui.panX = (panOrigin?.x ?? 0) + value.translation.width
                model.ui.panY = (panOrigin?.y ?? 0) + value.translation.height
            } else if model.ui.tool == "pen" { penPoints.append(designPoint(value.location, size: size)) }
        }.onEnded { value in
            switch model.ui.tool {
            case "hand": panOrigin = nil
            case "zoom": model.ui.zoom = NSEvent.modifierFlags.contains(.option) ? "50" : "100"
            case "rectangle", "ellipse", "pen", "text":
                model.create(tool: model.ui.tool, from: designPoint(value.startLocation, size: size), to: designPoint(value.location, size: size), points: penPoints)
                penPoints.removeAll()
            default: model.select(model.hitTest(designPoint(value.location, size: size)), canvas: true)
            }
        }
    }
    func designPoint(_ point: CGPoint, size: CGSize) -> CGPoint {
        CGPoint(x: point.x / max(1, size.width) * model.extent.width, y: point.y / max(1, size.height) * model.extent.height)
    }
}
