import SwiftUI

/// The current-time line and amber handle. The caller positions its center on the timeline.
public struct KRPlayhead: View {
    @Environment(\.krPalette) private var p
    public var showHandle: Bool
    public init(showHandle: Bool = true) { self.showHandle = showHandle }
    public var body: some View {
        ZStack(alignment: .top) {
            p.accentInk.frame(width: 2)
            if showHandle {
                RoundedRectangle(cornerRadius: KRRadius.radiusSm).fill(p.accent).frame(width: 12, height: 14)
                    .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusSm).stroke(p.accentInk, lineWidth: 1) }.offset(y: 2)
            }
        }.frame(width: 12).allowsHitTesting(false).accessibilityHidden(true)
    }
}
