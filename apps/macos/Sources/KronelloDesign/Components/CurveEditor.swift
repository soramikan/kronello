import SwiftUI

public struct KRCurveChannel: Identifiable {
    public let id: String
    public let points: [CGPoint]
    public let active: Bool
    public let current: String
    public init(_ id: String, points: [CGPoint], active: Bool, current: String) { self.id = id; self.points = points; self.active = active; self.current = current }
}
public struct KRCurveKey: Identifiable {
    public let id: String
    public let point: CGPoint
    public let interpolation: KRInterpolation
    public let selected: Bool
    public let label: String
    public init(_ id: String, point: CGPoint, interpolation: KRInterpolation, selected: Bool, label: String) {
        self.id = id; self.point = point; self.interpolation = interpolation; self.selected = selected; self.label = label
    }
}
public struct KRCurveTangent: Identifiable {
    public let id: String
    public let origin: CGPoint
    public let point: CGPoint
    public init(_ id: String, origin: CGPoint, point: CGPoint) { self.id = id; self.origin = origin; self.point = point }
}

/// Consumer supplies evaluated display geometry; this component never authors document state.
/// X is frame position, Y is the PropertyPresentation value (or read-only velocity).
public struct KRCurveEditor: View {
    @Environment(\.krPalette) private var p
    public let channels: [KRCurveChannel]
    public let keys: [KRCurveKey]
    public let tangents: [KRCurveTangent]
    public let frames: ClosedRange<Double>
    public let values: ClosedRange<Double>
    public let ticks: [KRRulerTick]
    public let playhead: Double
    public var readOnly = false
    public var select: (String) -> Void = { _ in }
    public var move: (String, Double, Bool) -> Void = { _, _, _ in }
    public var tangent: (String, CGPoint, Bool) -> Void = { _, _, _ in }
    public var clear: () -> Void = {}
    @State private var draggingKey: String?
    @FocusState private var focused: Bool
    public init(channels: [KRCurveChannel], keys: [KRCurveKey], tangents: [KRCurveTangent], frames: ClosedRange<Double>, values: ClosedRange<Double>, ticks: [KRRulerTick], playhead: Double, readOnly: Bool = false,
                select: @escaping (String) -> Void = { _ in }, move: @escaping (String, Double, Bool) -> Void = { _, _, _ in }, tangent: @escaping (String, CGPoint, Bool) -> Void = { _, _, _ in }, clear: @escaping () -> Void = {}) {
        self.channels = channels; self.keys = keys; self.tangents = tangents; self.frames = frames; self.values = values; self.ticks = ticks; self.playhead = playhead; self.readOnly = readOnly
        self.select = select; self.move = move; self.tangent = tangent; self.clear = clear
    }
    public var body: some View {
        GeometryReader { proxy in
            let graph = CGSize(width: proxy.size.width, height: max(1, proxy.size.height - KRSize.rowHeight))
            VStack(spacing: 0) {
                KRRuler(ticks).frame(height: KRSize.rowHeight)
                ZStack(alignment: .topLeading) {
                    p.surface100.contentShape(Rectangle()).onTapGesture { focused = true; clear() }
                    Canvas { context, size in
                        for i in 0...4 {
                            let v = values.lowerBound + Double(i) / 4 * (values.upperBound - values.lowerBound)
                            let y = screen(CGPoint(x: frames.lowerBound, y: v), size).y
                            var path = Path(); path.move(to: CGPoint(x: 0, y: y)); path.addLine(to: CGPoint(x: size.width, y: y))
                            context.stroke(path, with: .color(abs(v) < 1e-9 ? p.lineStrong : p.line), lineWidth: 1)
                            context.draw(Text(String(format: "%.1f", v)).font(KRType.ruler.font()).foregroundColor(p.inkMuted), at: CGPoint(x: KRSpace.space2, y: y - 7), anchor: .leading)
                        }
                        if values.contains(0) {
                            let y = screen(CGPoint(x: frames.lowerBound, y: 0), size).y
                            var zero = Path(); zero.move(to: CGPoint(x: 0, y: y)); zero.addLine(to: CGPoint(x: size.width, y: y))
                            context.stroke(zero, with: .color(p.lineStrong), lineWidth: 1)
                        }
                        for tick in ticks {
                            var path = Path(); path.move(to: CGPoint(x: tick.x, y: 0)); path.addLine(to: CGPoint(x: tick.x, y: size.height))
                            context.stroke(path, with: .color(p.line), lineWidth: 1)
                        }
                        for channel in channels {
                            var path = Path()
                            for (i, point) in channel.points.enumerated() {
                                let point = screen(point, size)
                                if i == 0 { path.move(to: point) } else { path.addLine(to: point) }
                            }
                            context.stroke(path, with: .color(channel.active ? p.ink : p.inkMuted), style: StrokeStyle(lineWidth: channel.active ? 1.5 : 1, dash: channel.active ? [] : [4, 3]))
                            if let last = channel.points.last { context.draw(Text(channel.id).font(KRType.label.font()).foregroundColor(channel.active ? p.ink : p.inkMuted), at: CGPoint(x: size.width - 12, y: screen(last, size).y - 10)) }
                        }
                        for handle in tangents where !readOnly {
                            var path = Path(); path.move(to: screen(handle.origin, size)); path.addLine(to: screen(handle.point, size))
                            context.stroke(path, with: .color(p.selection), lineWidth: 1)
                        }
                    }.allowsHitTesting(false)
                    if !readOnly {
                        ForEach(keys) { key in
                            KRKeyframeGlyph(key.interpolation, selected: key.selected)
                                .contentShape(Rectangle()).gesture(DragGesture(minimumDistance: 0).onChanged { value in
                                    focused = true
                                    if abs(value.translation.width) > 2 {
                                        if draggingKey == nil { draggingKey = key.id }
                                        move(key.id, Double(value.translation.width / max(1, graph.width - KRSpace.space2 * 2)) * (frames.upperBound - frames.lowerBound), false)
                                    }
                                }.onEnded { value in
                                    if draggingKey != nil { move(key.id, Double(value.translation.width / max(1, graph.width - KRSpace.space2 * 2)) * (frames.upperBound - frames.lowerBound), true) }
                                    else { select(key.id) }
                                    draggingKey = nil
                                }).position(screen(key.point, graph))
                            if key.selected {
                                Text(key.label).krText(KRType.ruler).foregroundStyle(p.selection).fixedSize()
                                    .position(x: min(max(80, screen(key.point, graph).x), max(80, graph.width - 80)), y: max(8, screen(key.point, graph).y - 18)).allowsHitTesting(false)
                            }
                        }
                        ForEach(tangents) { handle in
                            Circle().fill(p.selection).frame(width: 5, height: 5).padding(KRSpace.space1).contentShape(Rectangle())
                                .gesture(DragGesture(minimumDistance: 0, coordinateSpace: .named("curve-graph")).onChanged { v in tangent(handle.id, coordinate(v.location, graph), false) }.onEnded { v in tangent(handle.id, coordinate(v.location, graph), true) })
                                .position(screen(handle.point, graph))
                        }
                    }
                    VStack(alignment: .leading, spacing: KRSpace.space1) {
                        ForEach(channels) { channel in Text(channel.id + " " + channel.current).krText(KRType.ruler).foregroundStyle(p.accentInk) }
                    }.offset(x: min(max(0, screen(CGPoint(x: playhead, y: 0), graph).x + KRSpace.space2), max(0, graph.width - 100)), y: KRSpace.space2).allowsHitTesting(false)
                }.coordinateSpace(name: "curve-graph").frame(height: graph.height)
            }.overlay(alignment: .topLeading) {
                KRPlayhead().frame(height: proxy.size.height).offset(x: screen(CGPoint(x: playhead, y: 0), graph).x - 6).allowsHitTesting(false)
            }.clipped()
        }.focusable().focused($focused).focusEffectDisabled().krFocusRing(focused, inset: true)
    }
    func screen(_ point: CGPoint, _ size: CGSize) -> CGPoint {
        CGPoint(x: KRSpace.space2 + (point.x - frames.lowerBound) / max(1, frames.upperBound - frames.lowerBound) * max(1, size.width - KRSpace.space2 * 2),
                y: KRSpace.space4 + (values.upperBound - point.y) / max(1e-9, values.upperBound - values.lowerBound) * max(1, size.height - KRSpace.space4 * 2))
    }
    func coordinate(_ point: CGPoint, _ size: CGSize) -> CGPoint {
        CGPoint(x: frames.lowerBound + (point.x - KRSpace.space2) / max(1, size.width - KRSpace.space2 * 2) * (frames.upperBound - frames.lowerBound),
                y: values.upperBound - (point.y - KRSpace.space4) / max(1, size.height - KRSpace.space4 * 2) * (values.upperBound - values.lowerBound))
    }
}
