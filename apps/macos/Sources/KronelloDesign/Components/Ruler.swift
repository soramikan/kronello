import SwiftUI

/// A resolved ruler tick; labels contain integer seconds or frame notation.
public struct KRRulerTick: Identifiable, Sendable {
    public var id: String
    public var x: CGFloat
    public var label: String?
    public init(_ id: String, x: CGFloat, label: String? = nil) { self.id = id; self.x = x; self.label = label }
}

/// A timeline ruler that draws consumer-resolved positions without storing floating time.
public struct KRRuler: View {
    @Environment(\.krPalette) private var p
    public let ticks: [KRRulerTick]
    public init(_ ticks: [KRRulerTick]) { self.ticks = ticks }
    public var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .topLeading) {
                ForEach(ticks) { tick in
                    p.lineStrong.frame(width: 1, height: tick.label == nil ? 5 : 9).offset(x: tick.x, y: proxy.size.height - (tick.label == nil ? 5 : 9))
                    if let label = tick.label { Text(label).krText(KRType.ruler).foregroundStyle(p.inkMuted).offset(x: tick.x + 3, y: 3) }
                }
            }
        }.frame(height: KRSize.rulerHeight).krBottomLine().clipped().accessibilityHidden(true)
    }
}

/// Resolves ruler ticks at readable steps (frames, then 1/2/5/10/15/30 s, then minutes)
/// so labels never land on arbitrary fractions of the visible span.
public enum KRRulerScale {
    /// `0f`, `12f`, `1s`, `1s12f`, `1m`, `1m30s` for a non-negative frame count.
    public static func label(frame: Int64, fps: Int) -> String {
        let fps = Int64(max(1, fps)), frame = max(0, frame)
        let seconds = frame / fps, rest = frame % fps
        if seconds == 0 { return "\(rest)f" }
        if rest != 0 { return seconds >= 60 ? "\(seconds / 60)m\(seconds % 60)s\(rest)f" : "\(seconds)s\(rest)f" }
        if seconds < 60 { return "\(seconds)s" }
        return seconds % 60 == 0 ? "\(seconds / 60)m" : "\(seconds / 60)m\(seconds % 60)s"
    }
    /// Major step in frames whose spacing is at least `minimumSpacing` points.
    public static func majorStep(pointsPerFrame: Double, fps: Int, minimumSpacing: Double = 64) -> Int64 {
        let fps = Int64(max(1, fps))
        // Sub-second steps divide the second so labels stay on whole seconds (24 fps: 6f, 12f, 1s).
        var candidates: [Int64] = (1..<fps).filter { fps % $0 == 0 }
        candidates += [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600].map { $0 * fps }
        return candidates.first { Double($0) * pointsPerFrame >= minimumSpacing } ?? candidates[candidates.count - 1]
    }
    /// Ticks for the visible frame range drawn into `width` with `inset` on both ends.
    public static func ticks(frames: ClosedRange<Double>, fps: Int, width: CGFloat, inset: CGFloat = KRSpace.space2) -> [KRRulerTick] {
        let span = max(1e-9, frames.upperBound - frames.lowerBound)
        let usable = max(1, Double(width - inset * 2))
        let pointsPerFrame = usable / span
        let major = majorStep(pointsPerFrame: pointsPerFrame, fps: fps)
        let minor = [10, 5, 4, 2].lazy.map { major / $0 }.first { $0 > 0 && major % ($0) == 0 && Double($0) * pointsPerFrame >= 8 } ?? major
        var ticks: [KRRulerTick] = []
        var frame = Int64((frames.lowerBound / Double(minor)).rounded(.up)) * minor
        while Double(frame) <= frames.upperBound {
            let x = inset + CGFloat((Double(frame) - frames.lowerBound) * pointsPerFrame)
            // A label needs room before the trailing edge.
            let labeled = frame % major == 0 && x + 40 <= width
            ticks.append(.init("f\(frame)", x: x, label: labeled ? label(frame: frame, fps: fps) : nil))
            frame += minor
        }
        return ticks
    }
}
