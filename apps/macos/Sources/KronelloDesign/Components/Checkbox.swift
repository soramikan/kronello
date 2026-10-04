import SwiftUI

/// A three-state checkbox style for native SwiftUI toggles.
public struct KRCheckboxStyle: ToggleStyle {
    public var mixed: Bool
    public var appearance: KRControlAppearance
    public init(mixed: Bool = false, appearance: KRControlAppearance = .resting) { self.mixed = mixed; self.appearance = appearance }
    public func makeBody(configuration: Configuration) -> some View {
        KRChoice(configuration: configuration, radio: false, mixed: mixed, appearance: appearance, radioLabel: "")
    }
}

struct KRChoice: View {
    @Environment(\.krPalette) var p
    @Environment(\.isEnabled) var enabled
    @State private var hover = false
    @FocusState private var focused: Bool
    let configuration: ToggleStyle.Configuration
    let radio: Bool
    let mixed: Bool
    let appearance: KRControlAppearance
    let radioLabel: String
    var selected: Bool { configuration.isOn || mixed }
    var body: some View {
        Button { if !radio || !configuration.isOn { configuration.isOn.toggle() } } label: {
            HStack(spacing: KRSpace.space2) {
                ZStack {
                    RoundedRectangle(cornerRadius: radio ? KRSize.toggleSize / 2 : KRRadius.radiusSm)
                        .fill(selected ? p.selection : hover || appearance == .hover ? p.controlHover : p.surface200)
                    RoundedRectangle(cornerRadius: radio ? KRSize.toggleSize / 2 : KRRadius.radiusSm)
                        .strokeBorder(selected ? p.selection : p.lineStrong, lineWidth: 1)
                    if selected {
                        if radio { Circle().fill(p.onSelection).frame(width: 6, height: 6) }
                        else if mixed { RoundedRectangle(cornerRadius: 1).fill(p.onSelection).frame(width: KRSpace.space2, height: 2) }
                        else { KRCheckMark().stroke(p.onSelection, style: StrokeStyle(lineWidth: 2, lineCap: .square, lineJoin: .miter)).frame(width: 7, height: 4).offset(y: -1) }
                    }
                }.frame(width: KRSize.toggleSize, height: KRSize.toggleSize).opacity(enabled ? 1 : 0.45)
                    .krFocusRing(focused || appearance == .focused, cornerRadius: radio ? KRSize.toggleSize / 2 : KRRadius.radiusSm)
                configuration.label.krText(KRType.body).foregroundStyle(enabled ? p.ink : p.inkMuted)
            }
        }.buttonStyle(.plain).focused($focused).onHover { hover = $0 }
            .accessibilityValue(mixed ? "混在" : configuration.isOn ? "オン" : "オフ")
            .accessibilityAddTraits(selected ? .isSelected : [])
            .accessibilityRepresentation {
                if radio {
                    KRNativeRadio(label: radioLabel, selected: configuration.isOn, enabled: enabled) { configuration.isOn = true }
                } else {
                    Toggle(isOn: Binding(get: { configuration.isOn }, set: { configuration.isOn = $0 })) { configuration.label }
                        .toggleStyle(.checkbox).disabled(!enabled).accessibilityValue(mixed ? "混在" : configuration.isOn ? "オン" : "オフ")
                }
            }
    }
}

private struct KRCheckMark: Shape {
    func path(in rect: CGRect) -> Path {
        Path { p in p.move(to: CGPoint(x: 0, y: rect.midY)); p.addLine(to: CGPoint(x: rect.width * 0.35, y: rect.maxY)); p.addLine(to: CGPoint(x: rect.maxX, y: rect.minY)) }
    }
}

/// A labeled checkbox with optional mixed state.
public struct KRCheckbox: View {
    private let label: String
    @Binding private var isOn: Bool
    private let mixed: Bool
    private let appearance: KRControlAppearance
    public init(_ label: String, isOn: Binding<Bool>, mixed: Bool = false, appearance: KRControlAppearance = .resting) {
        self.label = label; _isOn = isOn; self.mixed = mixed; self.appearance = appearance
    }
    public var body: some View { Toggle(label, isOn: $isOn).toggleStyle(KRCheckboxStyle(mixed: mixed, appearance: appearance)) }
}
