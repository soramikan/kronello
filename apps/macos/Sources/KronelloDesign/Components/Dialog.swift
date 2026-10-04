import SwiftUI

/// One of at most three dialog actions. The primary action is sorted to the right.
public struct KRDialogAction: Identifiable {
    public var id: String
    public var label: String
    public var variant: KRButtonVariant
    public var action: () -> Void
    public init(_ id: String, _ label: String, variant: KRButtonVariant = .secondary, action: @escaping () -> Void = {}) {
        self.id = id; self.label = label; self.variant = variant; self.action = action
    }
}

/// Modal sheet content; presentation belongs to the app. File names and paths belong in `detail`.
public struct KRDialog: View {
    @Environment(\.krPalette) private var p
    @Environment(\.krStaticRendering) private var staticRendering
    public let title: String
    public let bodyText: String
    public let code: String?
    public let detail: String?
    public let actions: [KRDialogAction]
    public init(_ title: String, body: String, code: String? = nil, detail: String? = nil, actions: [KRDialogAction]) {
        precondition(actions.count <= 3 && actions.filter { $0.variant == .primary }.count <= 1)
        self.title = title; bodyText = body; self.code = code; self.detail = detail
        self.actions = actions.enumerated().sorted { a, b in
            let left = Self.rank(a.element.variant), right = Self.rank(b.element.variant)
            return left == right ? a.offset < b.offset : left < right
        }.map(\.element)
    }
    private static func rank(_ variant: KRButtonVariant) -> Int { switch variant { case .plain: return 0; case .secondary, .destructive: return 1; case .primary: return 2 } }
    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .top, spacing: KRSpace.space3) {
                KRIconView(code == nil ? .info : .triangleAlert, size: 24).foregroundStyle(code == nil ? p.inkMuted : p.danger)
                VStack(alignment: .leading, spacing: KRSpace.space1) {
                    Text(title).krText(KRType.heading).padding(.top, 2)
                    if let code {
                        (Text(code).font(KRMono.label.font()).foregroundColor(p.danger) + Text(" · " + bodyText))
                            .krText(KRType.body).lineSpacing(3).lineLimit(nil).fixedSize(horizontal: false, vertical: true)
                    } else { Text(bodyText).krText(KRType.body).lineSpacing(3).lineLimit(nil).fixedSize(horizontal: false, vertical: true) }
                }
            }
            if let detail {
                KRDialogDetailLayout {
                    detailContent(detail).fixedSize(horizontal: false, vertical: true).hidden().accessibilityHidden(true)
                    if staticRendering {
                        detailContent(detail).fixedSize(horizontal: false, vertical: true)
                            .frame(maxHeight: .infinity, alignment: .top).clipped()
                    } else {
                        ScrollView { detailContent(detail).fixedSize(horizontal: false, vertical: true) }
                    }
                }
                    .background(p.surface200, in: RoundedRectangle(cornerRadius: KRRadius.radiusSm))
                    .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusSm).strokeBorder(p.line, lineWidth: 1) }
                    .padding(.leading, 36).padding(.top, KRSpace.space3)
            }
            HStack(spacing: KRSpace.space2) {
                Spacer(minLength: 0)
                ForEach(actions) { item in
                    KRButton(item.label, variant: item.variant, action: item.action)
                        .modifier(KRDialogShortcut(variant: item.variant))
                }
            }.padding(.top, KRSpace.space4)
        }.foregroundStyle(p.ink).padding(KRSpace.space4).frame(width: 440)
            .background(p.surface100, in: RoundedRectangle(cornerRadius: KRRadius.radiusLg))
            .krShadow(p.shadowPopover, cornerRadius: KRRadius.radiusLg)
    }
    private func detailContent(_ detail: String) -> some View {
        Text(detail).krText(KRMono.label, weight: 400).foregroundStyle(p.inkMuted).textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading).padding(KRSpace.space2)
    }
}

// Measure the full detail text, then give its scroll view exactly the content height up to 96px.
// This synchronous layout also works in ImageRenderer without a preference-driven second pass.
private struct KRDialogDetailLayout: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let content = subviews[0].sizeThatFits(ProposedViewSize(width: proposal.width, height: nil))
        return CGSize(width: proposal.width ?? content.width, height: min(96, content.height))
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews[1].place(at: bounds.origin, proposal: ProposedViewSize(bounds.size))
    }
}

private struct KRDialogShortcut: ViewModifier {
    let variant: KRButtonVariant
    @ViewBuilder func body(content: Content) -> some View {
        if variant == .primary { content.keyboardShortcut(.defaultAction) }
        else if variant == .plain { content.keyboardShortcut(.cancelAction) }
        else { content }
    }
}
