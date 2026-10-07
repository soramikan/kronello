import SwiftUI

private struct KRStaticRenderingKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// Draws inputs as styled text for ImageRenderer; interactive apps leave this false.
    public var krStaticRendering: Bool {
        get { self[KRStaticRenderingKey.self] }
        set { self[KRStaticRenderingKey.self] = newValue }
    }
}

struct KRTextFieldChrome: ViewModifier {
    @Environment(\.krPalette) var p
    @Environment(\.isEnabled) var enabled
    let invalid: Bool
    let focused: Bool
    var height: CGFloat = KRSize.controlHeight
    func body(content: Content) -> some View {
        content.padding(.horizontal, KRSpace.space2).frame(minHeight: height)
            .background(p.surface200, in: RoundedRectangle(cornerRadius: KRRadius.radiusSm))
            .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusSm).strokeBorder(invalid ? p.danger : p.lineStrong, lineWidth: 1) }
            .krFocusRing(focused).opacity(enabled ? 1 : 0.45)
    }
}

/// A plain native text editor with the Kronello border, caret, and focus ring.
public struct KRTextFieldStyle: TextFieldStyle {
    @Environment(\.krPalette) private var p
    public var invalid: Bool
    public var focused: Bool
    public init(invalid: Bool = false, focused: Bool = false) { self.invalid = invalid; self.focused = focused }
    public func _body(configuration: TextField<Self._Label>) -> some View {
        configuration.textFieldStyle(.plain).krText(KRType.body).foregroundStyle(p.ink).tint(p.selection)
            .modifier(KRTextFieldChrome(invalid: invalid, focused: focused))
    }
}

/// A labeled string field whose local draft commits on Return or focus loss.
public struct KRTextField: View {
    @Environment(\.krStaticRendering) private var staticRendering
    @Environment(\.krPalette) private var p
    @Binding private var value: String
    private let label: String
    private let placeholder: String
    private let help: String?
    private let error: KRDiagnostic?
    private let appearance: KRControlAppearance
    private let onEditingStart: () -> Void
    private let onCommit: (String) -> Void
    @State private var draftStarted = false
    @State private var submitted = false
    @State private var focused = false
    @Environment(\.isEnabled) private var enabled
    public init(_ label: String, value: Binding<String>, placeholder: String = "", help: String? = nil,
                error: KRDiagnostic? = nil, appearance: KRControlAppearance = .resting,
                onEditingStart: @escaping () -> Void = {}, onCommit: @escaping (String) -> Void = { _ in }) {
        self.label = label; _value = value; self.placeholder = placeholder; self.help = help
        self.error = error; self.appearance = appearance; self.onEditingStart = onEditingStart; self.onCommit = onCommit
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space1) {
            if !label.isEmpty { Text(label).krText(KRType.label).foregroundStyle(p.ink) }
            if staticRendering {
                Text(value.isEmpty ? placeholder : value).krText(KRType.body)
                    .foregroundStyle(value.isEmpty ? p.inkMuted : p.ink)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .modifier(KRTextFieldChrome(invalid: error != nil, focused: appearance == .focused))
                    .accessibilityLabel(label)
            } else {
                KRCommittedTextInput(value: value, label: label, placeholder: placeholder, muted: NSColor(p.inkMuted), ink: NSColor(p.ink), selection: NSColor(p.selection), enabled: enabled,
                    onCommit: { value = $0; submitted = true; onCommit($0); draftStarted = false },
                    onFocus: { focused = $0; if $0 { submitted = false; if !draftStarted { onEditingStart(); draftStarted = true } } }, onCancel: { draftStarted = false }, onDraftChange: { submitted = false; if !draftStarted { onEditingStart(); draftStarted = true } })
                    .frame(height: KRSize.controlHeight - 8)
                    .modifier(KRTextFieldChrome(invalid: error != nil && (!focused || submitted), focused: focused || appearance == .focused))
            }
            if let error, !focused || submitted { KRErrorLine(error) }
            else if let help { Text(help).krText(KRType.caption).foregroundStyle(p.inkMuted) }
        }
    }
}
