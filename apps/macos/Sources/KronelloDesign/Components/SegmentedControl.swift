import SwiftUI

/// A stable identifier and label for a segmented choice.
public struct KRSegment: Identifiable, Sendable {
    public var id: String
    public var label: String
    public var unavailableReason: String?
    public init(_ id: String, _ label: String, unavailableReason: String? = nil) { self.id = id; self.label = label; self.unavailableReason = unavailableReason }
}

/// An always-visible group of mutually exclusive choices.
public struct KRSegmentedControl: View {
    @Environment(\.krPalette) private var p
    public let segments: [KRSegment]
    @Binding private var selection: String
    private let onSelect: (String) -> Void
    private let appearance: KRControlAppearance
    @FocusState private var focused: String?
    public init(_ segments: [KRSegment], selection: Binding<String>, appearance: KRControlAppearance = .resting, onSelect: @escaping (String) -> Void = { _ in }) {
        self.segments = segments; _selection = selection; self.appearance = appearance; self.onSelect = onSelect
    }
    public var body: some View {
        HStack(spacing: 1) {
            ForEach(segments) { segment in
                Button { selection = segment.id; onSelect(segment.id) } label: {
                    Text(segment.label).krText(KRType.label).padding(.horizontal, KRSpace.space2).frame(height: 18)
                }.buttonStyle(KRSegmentStyle(selected: selection == segment.id, appearance: focused == segment.id ? .focused : appearance))
                    .focused($focused, equals: segment.id).focusEffectDisabled()
                    .disabled(segment.unavailableReason != nil).help(segment.unavailableReason ?? segment.label)
                    .accessibilityAddTraits(selection == segment.id ? .isSelected : [])
            }
        }.padding(2).background(p.surface200, in: RoundedRectangle(cornerRadius: KRRadius.radiusMd))
            .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusMd).strokeBorder(p.lineStrong, lineWidth: 1) }
            .accessibilityElement(children: .contain)
    }
}

private struct KRSegmentStyle: ButtonStyle {
    @Environment(\.krPalette) var p
    @Environment(\.isEnabled) var enabled
    @State private var hover = false
    let selected: Bool
    let appearance: KRControlAppearance
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.foregroundStyle(selected || hover || appearance == .hover ? p.ink : p.inkMuted)
            .background(selected || configuration.isPressed ? p.controlHover : .clear, in: RoundedRectangle(cornerRadius: 4))
            .overlay { RoundedRectangle(cornerRadius: 4).strokeBorder(selected ? p.lineStrong : .clear, lineWidth: 1) }
            .krFocusRing(appearance == .focused, cornerRadius: 4).opacity(enabled ? 1 : 0.45).onHover { hover = $0 }
    }
}
