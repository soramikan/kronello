import AppKit
import SwiftUI

/// A discrete option with a stable ID and optional leading icon.
public struct KRPopupOption: Identifiable, Sendable {
    public var id: String
    public var label: String
    public var icon: KRIcon?
    public var disabled: Bool
    public init(_ id: String, _ label: String, icon: KRIcon? = nil, disabled: Bool = false) {
        self.id = id; self.label = label; self.icon = icon; self.disabled = disabled
    }
}

/// A token-styled selector that opens KRMenu in an anchored borderless child panel.
public struct KRPopupButton: View {
    @Environment(\.krPalette) private var p
    @Environment(\.krTheme) private var theme
    @Environment(\.isEnabled) private var enabled
    @Binding private var selection: String
    private let options: [KRPopupOption]
    private let label: String
    private let appearance: KRControlAppearance
    private let onSelect: (String) -> Void
    @StateObject private var presenter = KRMenuPresenter()
    @State private var anchor: CGRect = .zero
    @State private var hover = false
    @FocusState private var focused: Bool
    public init(_ label: String, options: [KRPopupOption], selection: Binding<String>, appearance: KRControlAppearance = .resting,
                onSelect: @escaping (String) -> Void = { _ in }) {
        self.label = label; self.options = options; _selection = selection; self.appearance = appearance; self.onSelect = onSelect
    }
    public var body: some View {
        Button {
            if presenter.isPresented { presenter.dismiss(); return }
            guard let window = NSApp.keyWindow ?? NSApp.mainWindow else { return }
            let items = options.map { option in
                KRMenuItem(option.id, option.label, icon: option.icon, checked: option.id == selection, disabled: option.disabled) {
                    selection = option.id; onSelect(option.id)
                }
            }
            presenter.present(items, anchoredTo: anchor, in: window, theme: theme, current: selection)
        } label: {
            HStack(spacing: KRSpace.space2) {
                if let icon = options.first(where: { $0.id == selection })?.icon { KRIconView(icon, size: 12).foregroundStyle(p.inkMuted) }
                Text(options.first(where: { $0.id == selection })?.label ?? label).krText(KRType.body).foregroundStyle(p.ink)
                Spacer(minLength: 0)
                KRIconView(.chevronsUpDown, size: 12).foregroundStyle(p.inkMuted)
            }.padding(.leading, KRSpace.space2).padding(.trailing, KRSpace.space1)
                .frame(minWidth: 96).frame(height: KRSize.controlHeight)
                .background(presenter.isPresented || hover || appearance == .hover || appearance == .pressed ? p.controlHover : p.surface200,
                            in: RoundedRectangle(cornerRadius: KRRadius.radiusMd))
                .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusMd).strokeBorder(p.lineStrong, lineWidth: 1) }
        }.buttonStyle(.plain).focused($focused).krFocusRing(focused || appearance == .focused, cornerRadius: KRRadius.radiusMd)
            .background {
                GeometryReader { geometry in
                    Color.clear.preference(key: KRMenuAnchorKey.self, value: geometry.frame(in: .global))
                }
            }.onPreferenceChange(KRMenuAnchorKey.self) { anchor = $0 }
            .onHover { hover = $0 }.opacity(enabled ? 1 : 0.45)
            .onDisappear { presenter.dismiss() }.accessibilityLabel(label).accessibilityValue(selection)
    }
}
