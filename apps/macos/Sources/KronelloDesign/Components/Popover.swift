import SwiftUI

/// A small anchored editing surface; presentation and outside-click dismissal belong to the app.
public struct KRPopover<Content: View>: View {
    @Environment(\.krPalette) private var p
    public let title: String
    public let arrowX: CGFloat
    private let content: Content
    public init(_ title: String, arrowX: CGFloat = 24, @ViewBuilder content: () -> Content) {
        self.title = title; self.arrowX = arrowX; self.content = content()
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title).krText(KRType.heading).padding(.bottom, KRSpace.space3)
            content
        }.foregroundStyle(p.ink).padding(KRSpace.space3).frame(width: 264, alignment: .leading)
            .background(p.surface200, in: RoundedRectangle(cornerRadius: KRRadius.radiusMd))
            .krShadow(p.shadowPopover, cornerRadius: KRRadius.radiusMd)
            .overlay(alignment: .topLeading) {
                Rectangle().fill(p.surface200).frame(width: 10, height: 10)
                    .overlay(alignment: .top) { p.line.frame(height: 1) }
                    .overlay(alignment: .leading) { p.line.frame(width: 1) }
                    .rotationEffect(.degrees(45)).offset(x: min(244, max(10, arrowX)), y: -6)
            }
    }
}

/// A standard label/control row within a popover.
public struct KRPopoverRow<Control: View>: View {
    @Environment(\.krPalette) private var p
    public let label: String
    private let control: Control
    public init(_ label: String, @ViewBuilder control: () -> Control) { self.label = label; self.control = control() }
    public var body: some View {
        HStack(spacing: KRSpace.space2) { Text(label).krText(KRType.label).foregroundStyle(p.ink); Spacer(minLength: 0); control }
            .frame(minHeight: KRSize.rowHeight)
    }
}
