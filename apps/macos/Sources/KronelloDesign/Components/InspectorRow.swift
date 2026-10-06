import SwiftUI

/// A property row with source navigator, consumer-supplied fields, and typed diagnostics.
public struct KRInspectorRow<Fields: View>: View {
    @Environment(\.krPalette) private var p
    @State private var hover = false
    private let label: String
    private let navigator: KRKeyframeNavigator
    private let selected: Bool
    private let appearance: KRControlAppearance
    private let error: KRDiagnostic?
    private let fields: Fields
    private let keyframeEditingEnabled: Bool
    public init(_ label: String, source: KRPropertySource = .constant, onKeyframe: Bool = false, selected: Bool = false,
                error: KRDiagnostic? = nil, appearance: KRControlAppearance = .resting, keyframeEditingEnabled: Bool = true, previous: (() -> Void)? = nil, toggleKeyframe: @escaping () -> Void = {},
                next: (() -> Void)? = nil, @ViewBuilder fields: () -> Fields) {
        self.label = label; navigator = KRKeyframeNavigator(source: source, onKeyframe: onKeyframe, previous: previous, toggle: toggleKeyframe, next: next, editingEnabled: keyframeEditingEnabled)
        self.selected = selected; self.error = error; self.appearance = appearance; self.fields = fields()
        self.keyframeEditingEnabled = keyframeEditingEnabled
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            HStack(spacing: KRSpace.space2) {
                navigator
                Text(label).krText(KRType.label).foregroundStyle(p.ink).lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                fields.disabled(navigator.source == .expression)
            }.frame(minHeight: KRSize.rowHeight - 2)
            if let error { KRErrorLine(error).padding(.leading, 48 + KRSpace.space2) }
        }.padding(.leading, KRSpace.space2).padding(.trailing, KRSpace.space3).padding(.vertical, 1)
            .background(selected ? p.selectionBg : hover || appearance == .hover ? p.controlHover : .clear).onHover { hover = $0 }
    }
}

/// A setting that is not animatable (font face, alignment, bounds kind). Keeps the
/// navigator column empty so its label lines up with KRInspectorRow labels.
public struct KRInspectorSettingRow<Control: View>: View {
    @Environment(\.krPalette) private var p
    private let label: String
    private let control: Control
    public init(_ label: String, @ViewBuilder control: () -> Control) { self.label = label; self.control = control() }
    public var body: some View {
        HStack(spacing: KRSpace.space2) {
            Color.clear.frame(width: 48, height: 1)
            Text(label).krText(KRType.label).foregroundStyle(p.ink).lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
            control
        }.frame(minHeight: KRSize.rowHeight - 2)
            .padding(.leading, KRSpace.space2).padding(.trailing, KRSpace.space3).padding(.vertical, 1)
    }
}

/// A muted axis caption preceding a consumer-supplied component field, such as X or Y.
public struct KRInspectorAxis<Field: View>: View {
    @Environment(\.krPalette) private var p
    private let axis: String
    private let field: Field
    public init(_ axis: String, @ViewBuilder field: () -> Field) { self.axis = axis; self.field = field() }
    public var body: some View {
        HStack(spacing: KRSpace.space1) {
            Text(axis).krText(KRType.caption).foregroundStyle(p.inkMuted).fixedSize()
            field
        }
    }
}
