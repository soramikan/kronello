import SwiftUI

/// A square workspace panel with a title or caller-supplied tabs, plus header actions.
public struct KRPanel<Header: View, Actions: View, Content: View>: View {
    @Environment(\.krPalette) private var p
    private let header: Header
    private let actions: Actions
    private let content: Content
    public init(@ViewBuilder header: () -> Header, @ViewBuilder actions: () -> Actions, @ViewBuilder content: () -> Content) {
        self.header = header(); self.actions = actions(); self.content = content()
    }
    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                header
                Spacer(minLength: 0)
                HStack(spacing: 2) { actions }.padding(.trailing, KRSpace.space1)
            }.frame(height: KRSize.panelHeaderHeight).krBottomLine()
            content.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }.background(p.surface100).overlay { Rectangle().strokeBorder(p.line, lineWidth: 1).allowsHitTesting(false) }
    }
}

extension KRPanel where Header == KRPanelTitle {
    /// Creates a titled panel with optional plain header actions.
    public init(_ title: String, @ViewBuilder actions: () -> Actions, @ViewBuilder content: () -> Content) {
        header = KRPanelTitle(title)
        self.actions = actions(); self.content = content()
    }
}

extension KRPanel where Header == KRPanelTitle, Actions == EmptyView {
    /// Creates a simple titled panel without actions.
    public init(_ title: String, @ViewBuilder content: () -> Content) {
        header = KRPanelTitle(title); actions = EmptyView(); self.content = content()
    }
}

/// Token-styled panel heading with the standard horizontal inset.
public struct KRPanelTitle: View {
    @Environment(\.krPalette) private var p
    public let title: String
    public init(_ title: String) { self.title = title }
    public var body: some View { Text(title).krText(KRType.heading).foregroundStyle(p.ink).padding(.horizontal, KRSpace.space3) }
}
