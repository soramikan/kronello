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
