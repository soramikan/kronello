import SwiftUI

/// A concise empty-panel prompt, optionally highlighted as a file drop target.
public struct KREmptyState<Action: View>: View {
    @Environment(\.krPalette) private var p
    public let icon: KRIcon
    public let title: String
    public let message: String
    public let dropSummary: String?
    private let action: Action
    public init(icon: KRIcon, title: String, message: String, dropSummary: String? = nil, @ViewBuilder action: () -> Action) {
        self.icon = icon; self.title = title; self.message = message; self.dropSummary = dropSummary; self.action = action()
    }
    public var body: some View {
        VStack(spacing: KRSpace.space2) {
            KRIconView(icon, size: 24).foregroundStyle(dropSummary == nil ? p.inkMuted : p.ink)
            Text(title).krText(KRType.heading).foregroundStyle(p.ink)
            Text(dropSummary ?? message).krText(KRType.body).lineSpacing(3)
                .foregroundStyle(dropSummary == nil ? p.inkMuted : p.ink).frame(maxWidth: 280).padding(.bottom, KRSpace.space1)
            action
        }.multilineTextAlignment(.center).padding(.vertical, KRSpace.space4 * 2).padding(.horizontal, KRSpace.space4)
            .frame(maxWidth: .infinity)
            .background(dropSummary == nil ? .clear : p.selectionBg, in: RoundedRectangle(cornerRadius: KRRadius.radiusMd))
            .overlay { if dropSummary != nil { RoundedRectangle(cornerRadius: KRRadius.radiusMd)
                .strokeBorder(p.selection, style: StrokeStyle(lineWidth: 1, dash: [4, 3])).padding(KRSpace.space2) } }
    }
}

extension KREmptyState where Action == EmptyView {
    public init(icon: KRIcon, title: String, message: String, dropSummary: String? = nil) {
        self.init(icon: icon, title: title, message: message, dropSummary: dropSummary) { EmptyView() }
    }
}
