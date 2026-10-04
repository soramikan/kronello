import SwiftUI

/// A tab identity remains stable when its visible name changes.
public struct KRTab: Identifiable, Sendable {
    public var id: String
    public var title: String
    public var closable: Bool
    public init(_ id: String, _ title: String, closable: Bool = false) { self.id = id; self.title = title; self.closable = closable }
}

/// Panel tabs use a neutral ink underline for the active tab.
public struct KRTabBar: View {
    public let tabs: [KRTab]
    @Binding private var selection: String
    private let onClose: (String) -> Void
    private let appearance: KRControlAppearance
    public init(_ tabs: [KRTab], selection: Binding<String>, appearance: KRControlAppearance = .resting, onClose: @escaping (String) -> Void = { _ in }) {
        self.tabs = tabs; _selection = selection; self.appearance = appearance; self.onClose = onClose
    }
    public var body: some View {
        HStack(spacing: 0) { ForEach(tabs) { tab in KRTabItem(tab: tab, selected: selection == tab.id, appearance: appearance, select: { selection = tab.id }, close: { onClose(tab.id) }) } }
            .frame(height: KRSize.panelHeaderHeight).accessibilityElement(children: .contain)
    }
}

private struct KRTabItem: View {
    @Environment(\.krPalette) var p
    @State private var hover = false
    @FocusState private var focused: Bool
    @FocusState private var closeFocused: Bool
    let tab: KRTab
    let selected: Bool
    let appearance: KRControlAppearance
    let select: () -> Void
    let close: () -> Void
    var body: some View {
        HStack(spacing: KRSpace.space1) {
            Button(action: select) { Text(tab.title).krText(KRType.label).frame(maxHeight: .infinity) }
                .buttonStyle(.plain).focused($focused).krFocusRing(focused || appearance == .focused, cornerRadius: 2, inset: true)
                .accessibilityAddTraits(selected ? .isSelected : [])
            if tab.closable {
                KRButton(icon: .x, accessibilityLabel: "\(tab.title) を閉じる", size: 16, iconSize: 12, action: close)
                    .focused($closeFocused).opacity(selected || hover || appearance == .hover || focused || closeFocused ? 1 : 0)
            }
        }.foregroundStyle(selected || hover || appearance == .hover ? p.ink : p.inkMuted)
            .padding(.leading, KRSpace.space3).padding(.trailing, KRSpace.space2)
            .background(hover || appearance == .hover ? p.controlHover : .clear)
            .overlay(alignment: .bottom) {
                if selected { RoundedRectangle(cornerRadius: 1).fill(p.ink).frame(height: 2).padding(.horizontal, KRSpace.space2) }
            }.onHover { hover = $0 }
    }
}
