import AppKit
import SwiftUI

/// The four command button treatments.
public enum KRButtonVariant: String, CaseIterable, Sendable { case primary, secondary, plain, destructive }

/// Styles a real SwiftUI Button with Kronello colors, dimensions, and focus feedback.
public struct KRButtonStyle: ButtonStyle {
    public var variant: KRButtonVariant
    public var iconOnly: Bool
    public var pressed: Bool
    public var appearance: KRControlAppearance
    public var size: CGFloat

    public init(_ variant: KRButtonVariant = .secondary, iconOnly: Bool = false, pressed: Bool = false,
                appearance: KRControlAppearance = .resting, size: CGFloat = KRSize.controlHeight) {
        self.variant = variant; self.iconOnly = iconOnly; self.pressed = pressed
        self.appearance = appearance; self.size = size
    }
    public func makeBody(configuration: Configuration) -> some View {
        KRButtonBody(configuration: configuration, style: self)
    }
}

private struct KRButtonBody: View {
    @Environment(\.krPalette) var p
    @Environment(\.isEnabled) var enabled
    @State private var hover = false
    let configuration: ButtonStyle.Configuration
    let style: KRButtonStyle
    var hovering: Bool { hover || style.appearance == .hover }
    var down: Bool { configuration.isPressed || style.appearance == .pressed }
    var primaryBrightness: Double { !enabled ? 1 : down ? 0.92 : hovering ? 1.08 : 1 }
    var fill: Color {
        if style.pressed { return p.controlHover }
        if style.variant == .primary { return multiply(p.accent, by: primaryBrightness) }
        if down { return p.surface100 }
        if hovering { return p.controlHover }
        return style.variant == .plain ? .clear : p.surface200
    }
    var ink: Color {
        if style.pressed { return p.ink }
        switch style.variant {
        case .primary: return multiply(p.onAccent, by: primaryBrightness)
        case .destructive: return p.danger
        case .secondary: return p.ink
        case .plain: return hovering || down ? p.ink : p.inkMuted
        }
    }
    var body: some View {
        configuration.label.krText(KRType.body, weight: 500)
            .padding(.horizontal, style.iconOnly ? 0 : KRSpace.space3)
            .frame(width: style.iconOnly ? style.size : nil, height: style.size)
            .foregroundStyle(ink)
            .background(fill, in: RoundedRectangle(cornerRadius: KRRadius.radiusMd))
            .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusMd)
                .strokeBorder(style.variant == .secondary || style.variant == .destructive ? p.lineStrong : .clear, lineWidth: 1) }
            .krFocusRing(style.appearance == .focused, cornerRadius: KRRadius.radiusMd)
            .opacity(enabled ? 1 : 0.45)
            .contentShape(RoundedRectangle(cornerRadius: KRRadius.radiusMd))
            .onHover { hover = $0 }
    }
    private func multiply(_ color: Color, by factor: Double) -> Color {
        guard factor != 1, let rgb = NSColor(color).usingColorSpace(.sRGB) else { return color }
        return Color(.sRGB, red: min(1, rgb.redComponent * factor), green: min(1, rgb.greenComponent * factor),
                     blue: min(1, rgb.blueComponent * factor), opacity: rgb.alphaComponent)
    }
}

/// A labeled command button. Icon-only buttons require a spoken label.
public struct KRButton: View {
    private let label: String
    private let icon: KRIcon?
    private let iconOnly: Bool
    private let style: KRButtonStyle
    private let iconSize: CGFloat
    private let action: () -> Void
    @FocusState private var focused: Bool

    public init(_ label: String, icon: KRIcon? = nil, variant: KRButtonVariant = .secondary,
                pressed: Bool = false, appearance: KRControlAppearance = .resting, action: @escaping () -> Void) {
        self.label = label; self.icon = icon; iconOnly = false; iconSize = 14
        style = KRButtonStyle(variant, pressed: pressed, appearance: appearance); self.action = action
    }
    public init(icon: KRIcon, accessibilityLabel: String, variant: KRButtonVariant = .plain,
                pressed: Bool = false, appearance: KRControlAppearance = .resting,
                size: CGFloat = KRSize.controlHeight, iconSize: CGFloat = 14, action: @escaping () -> Void) {
        label = accessibilityLabel; self.icon = icon; iconOnly = true; self.iconSize = iconSize
        style = KRButtonStyle(variant, iconOnly: true, pressed: pressed, appearance: appearance, size: size)
        self.action = action
    }
    public var body: some View {
        Button(action: action) {
            HStack(spacing: KRSpace.space1) {
                if let icon { KRIconView(icon, size: iconSize) }
                if !iconOnly { Text(label) }
            }
        }.buttonStyle(style).focused($focused).krFocusRing(focused, cornerRadius: KRRadius.radiusMd)
            .accessibilityLabel(label).accessibilityAddTraits(style.pressed ? .isSelected : [])
    }
}
