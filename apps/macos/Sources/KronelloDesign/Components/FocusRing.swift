import SwiftUI

/// The shared 2px selection ring, separated from the control by 1px.
public struct KRFocusRing: ViewModifier {
    @Environment(\.krPalette) private var p
    @Environment(\.isFocused) private var focused
    public var active: Bool
    public var cornerRadius: CGFloat
    public var inset: Bool

    public init(active: Bool = false, cornerRadius: CGFloat = KRRadius.radiusSm, inset: Bool = false) {
        self.active = active
        self.cornerRadius = cornerRadius
        self.inset = inset
    }

    public func body(content: Content) -> some View {
        content.overlay {
            if active || focused {
                RoundedRectangle(cornerRadius: cornerRadius + (inset ? 0 : 3))
                    .strokeBorder(p.selection, lineWidth: 2)
                    .padding(inset ? 0 : -3)
                    .allowsHitTesting(false)
            }
        }
    }
}

extension View {
    /// Adds the common focus ring; `active` also supports deterministic previews.
    public func krFocusRing(_ active: Bool = false, cornerRadius: CGFloat = KRRadius.radiusSm, inset: Bool = false) -> some View {
        modifier(KRFocusRing(active: active, cornerRadius: cornerRadius, inset: inset))
    }
}

/// An explicit interaction appearance for previews or externally controlled state.
public enum KRControlAppearance: String, CaseIterable, Sendable {
    case resting, hover, pressed, focused
}

/// A typed diagnostic supplied by the consuming application.
public struct KRDiagnostic: Equatable, Sendable {
    public var code: String
    public var message: String
    public init(_ code: String, _ message: String) { self.code = code; self.message = message }
}

/// Inline error text with an icon, code, and explanation.
public struct KRErrorLine: View {
    @Environment(\.krPalette) private var p
    public let error: KRDiagnostic
    public init(_ error: KRDiagnostic) { self.error = error }
    public var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: KRSpace.space1) {
            KRIconView(.triangleAlert, size: 12)
            Text(error.code).krText(KRMono.caption)
            Text(error.message).krText(KRType.caption)
        }.foregroundStyle(p.danger).accessibilityElement(children: .combine)
    }
}

// The reference uses the mono family at caption and label sizes for codes.
enum KRMono {
    static let caption = KRTextStyle(name: "mono-caption", family: KRType.timecode.family, size: KRType.caption.size,
                                     lineHeight: KRType.caption.lineHeight, weight: 500, tracking: 0)
    static let label = KRTextStyle(name: "mono-label", family: KRType.timecode.family, size: KRType.label.size,
                                   lineHeight: KRType.label.lineHeight, weight: 500, tracking: 0)
}

struct KRBottomLine: ViewModifier {
    @Environment(\.krPalette) var p
    func body(content: Content) -> some View {
        content.overlay(alignment: .bottom) { p.line.frame(height: 1).allowsHitTesting(false) }
    }
}

extension View {
    func krBottomLine() -> some View { modifier(KRBottomLine()) }
}
