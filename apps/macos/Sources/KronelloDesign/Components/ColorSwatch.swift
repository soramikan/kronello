import SwiftUI

/// template.md: a color value shows its swatch beside the mono value. The button
/// opens the consumer's color editor; transparency shows over a checkerboard.
public struct KRColorSwatchButton: View {
    @Environment(\.krPalette) private var p
    @Environment(\.isEnabled) private var enabled
    @State private var hover = false
    @FocusState private var focused: Bool
    private let color: Color
    private let label: String
    private let action: () -> Void
    public init(_ color: Color, accessibilityLabel: String = "色を選択", action: @escaping () -> Void) {
        self.color = color; label = accessibilityLabel; self.action = action
    }
    public var body: some View {
        Button(action: action) {
            ZStack {
                KRCheckerboard(cell: 4).clipShape(RoundedRectangle(cornerRadius: 2))
                RoundedRectangle(cornerRadius: 2).fill(color)
                RoundedRectangle(cornerRadius: 2).strokeBorder(p.line, lineWidth: 1)
            }.frame(width: 14, height: 14)
                .frame(width: KRSize.controlHeight, height: KRSize.controlHeight)
                .background(hover ? p.controlHover : p.surface200, in: RoundedRectangle(cornerRadius: KRRadius.radiusSm))
                .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusSm).strokeBorder(p.lineStrong, lineWidth: 1) }
                .opacity(enabled ? 1 : 0.45)
                .contentShape(RoundedRectangle(cornerRadius: KRRadius.radiusSm))
        }.buttonStyle(.plain).focused($focused).krFocusRing(focused, cornerRadius: KRRadius.radiusSm)
            .onHover { hover = $0 }.accessibilityLabel(label)
    }
}

