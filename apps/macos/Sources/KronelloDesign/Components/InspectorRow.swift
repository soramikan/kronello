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
    @Environment(\.krInspectorNavigatorColumn) private var navigatorColumn
    private let label: String
    private let control: Control
    public init(_ label: String, @ViewBuilder control: () -> Control) { self.label = label; self.control = control() }
    public var body: some View {
        HStack(spacing: KRSpace.space2) {
            if navigatorColumn { Color.clear.frame(width: 48, height: 1) }
            Text(label).krText(KRType.label).foregroundStyle(p.ink).lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)
            control
        }.frame(minHeight: KRSize.rowHeight - 2)
            .padding(.leading, navigatorColumn ? KRSpace.space2 : KRSpace.space3).padding(.trailing, KRSpace.space3).padding(.vertical, 1)
    }
}

private struct KRInspectorNavigatorColumnKey: EnvironmentKey { static let defaultValue = true }
extension EnvironmentValues {
    /// False in inspectors without animatable rows (the Edit page clip inspector),
    /// so setting labels sit on the panel padding instead of an empty navigator column.
    public var krInspectorNavigatorColumn: Bool {
        get { self[KRInspectorNavigatorColumnKey.self] }
        set { self[KRInspectorNavigatorColumnKey.self] = newValue }
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

/// InspectorRow.md: a section heading (`Transform`) in `heading` above unindented
/// rows, with a disclosure chevron when the section can collapse.
public struct KRSectionHeader: View {
    @Environment(\.krPalette) private var p
    @State private var hover = false
    @FocusState private var focused: Bool
    private let title: String
    private let expanded: Bool
    private let onToggle: (() -> Void)?
    public init(_ title: String, expanded: Bool = true, onToggle: (() -> Void)? = nil) {
        self.title = title; self.expanded = expanded; self.onToggle = onToggle
    }
    public var body: some View {
        Button { onToggle?() } label: {
            HStack(spacing: KRSpace.space1) {
                if onToggle != nil {
                    KRIconView(expanded ? .chevronDown : .chevronRight, size: 12).foregroundStyle(hover ? p.ink : p.inkMuted)
                }
                Text(title).krText(KRType.heading).foregroundStyle(p.ink).lineLimit(1)
                Spacer(minLength: 0)
            }.padding(.horizontal, KRSpace.space3).frame(height: KRSize.panelHeaderHeight).contentShape(Rectangle())
        }.buttonStyle(.plain).disabled(onToggle == nil).focused($focused).krFocusRing(focused, inset: true)
            .onHover { hover = $0 }
            .accessibilityAddTraits(.isHeader).accessibilityValue(onToggle == nil ? "" : expanded ? "展開" : "折りたたみ")
    }
}
