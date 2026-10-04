import SwiftUI

/// The visual interpolation shapes, independent of selection color.
public enum KRInterpolation: String, CaseIterable, Sendable { case linear, cubic, hold }

/// A keyframe mark with a padded hit region; layer summaries use 7px instead of the default 9px.
public struct KRKeyframeGlyph: View {
    @Environment(\.krPalette) private var p
    @State private var hover = false
    public let interpolation: KRInterpolation
    public let selected: Bool
    public let hollow: Bool
    public let on: Bool
    public let appearance: KRControlAppearance
    public let size: CGFloat
    private let label: String
    private let action: (() -> Void)?
    public init(_ interpolation: KRInterpolation = .linear, selected: Bool = false, hollow: Bool = false, on: Bool = false,
                appearance: KRControlAppearance = .resting, size: CGFloat = KRSize.keyframeSize,
                accessibilityLabel: String = "キーフレーム", action: (() -> Void)? = nil) {
        self.interpolation = interpolation; self.selected = selected; self.hollow = hollow; self.on = on
        self.appearance = appearance; self.size = size; label = accessibilityLabel; self.action = action
    }
    private var color: Color { selected ? p.selection : hover || appearance == .hover || on ? p.ink : hollow ? p.lineStrong : p.inkMuted }
    private var glyph: some View {
        KRKeyframeShape(interpolation: interpolation)
            .fill(hollow ? .clear : color)
            .overlay { KRKeyframeShape(interpolation: interpolation).strokeBorder(hollow ? color : .clear, lineWidth: 1) }
            .frame(width: size, height: size)
            .padding(KRSpace.space1)
    }
    public var body: some View {
        Group {
            if let action {
                Button(action: action) { glyph }.buttonStyle(.plain).krFocusRing(appearance == .focused)
                    .accessibilityLabel(label).accessibilityAddTraits(selected ? .isSelected : [])
            } else { glyph.accessibilityLabel(label).accessibilityAddTraits(selected ? .isSelected : []) }
        }.onHover { hover = $0 }
    }
}

struct KRKeyframeShape: InsettableShape {
    let interpolation: KRInterpolation
    var insetAmount: CGFloat = 0
    func inset(by amount: CGFloat) -> KRKeyframeShape { var result = self; result.insetAmount += amount; return result }
    func path(in rect: CGRect) -> Path {
        let r = rect.insetBy(dx: insetAmount, dy: insetAmount)
        switch interpolation {
        case .cubic: return Path(ellipseIn: r)
        case .hold:
            return Path(roundedRect: r.insetBy(dx: r.width * 0.07, dy: r.height * 0.07), cornerRadius: 1)
        case .linear:
            let square = r.insetBy(dx: r.width * 0.07, dy: r.height * 0.07)
            let rotation = CGAffineTransform(translationX: -r.midX, y: -r.midY)
                .concatenating(CGAffineTransform(rotationAngle: .pi / 4))
                .concatenating(CGAffineTransform(translationX: r.midX, y: r.midY))
            return Path(roundedRect: square, cornerRadius: 0.86).applying(rotation)
        }
    }
}
