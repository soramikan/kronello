import SwiftUI

/// Clip state; missing includes a typed error code supplied by the application.
public enum KRClipState: Equatable, Sendable { case resting, selected, missing(String), disabled }

/// A timeline clip body. The caller positions it in a lane using resolved pixel geometry.
public struct KRClip: View {
    @Environment(\.krPalette) private var p
    @State private var hover = false
    @FocusState private var focused: Bool
    public let name: String
    public let kind: KRMediaKind
    public let state: KRClipState
    public let meta: String?
    public let appearance: KRControlAppearance
    private let onSelect: () -> Void
    public init(_ name: String, kind: KRMediaKind, state: KRClipState = .resting, meta: String? = nil, appearance: KRControlAppearance = .resting,
                onSelect: @escaping () -> Void = {}) {
        self.name = name; self.kind = kind; self.state = state; self.meta = meta; self.appearance = appearance; self.onSelect = onSelect
    }
    private var missing: String? { if case .missing(let code) = state { return code }; return nil }
    private var selected: Bool { state == .selected }
    public var body: some View {
        Button(action: onSelect) {
            HStack(spacing: KRSpace.space1) {
                KRIconView(missing == nil ? kind.icon : .triangleAlert, size: 12).foregroundStyle(missing == nil ? kind.color(in: p) : p.danger)
                Text(name).krText(KRType.label).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                if let missing { Text(missing).krText(KRMono.caption).lineLimit(1) }
                if let meta, missing == nil { Text(meta).krText(KRType.ruler).foregroundStyle(p.inkMuted) }
            }.foregroundStyle(missing == nil ? p.ink : p.danger).padding(.horizontal, KRSpace.space2)
                .frame(maxWidth: .infinity).frame(height: KRSize.trackHeight - 4)
                .background(missing != nil ? p.surface100 : selected ? p.selectionBg : p.clip,
                            in: RoundedRectangle(cornerRadius: KRRadius.radiusSm))
                .overlay(alignment: .bottom) {
                    if missing == nil {
                        KRKindUnderline(dashed: kind == .adjustment).stroke(kind.color(in: p), style: StrokeStyle(lineWidth: 2, dash: kind == .adjustment ? [6, 4] : []))
                            .frame(height: 2).padding(.horizontal, selected ? 2 : 0).padding(.bottom, selected ? 2 : 0)
                    }
                }
                .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusSm).strokeBorder(missing != nil ? p.danger : selected ? p.selection : hover || appearance == .hover ? p.lineStrong : .clear,
                                style: StrokeStyle(lineWidth: selected && missing == nil ? 2 : 1, dash: missing == nil ? [] : [4, 3])) }
        }.buttonStyle(.plain).focused($focused).disabled(state == .disabled).opacity(state == .disabled ? 0.45 : 1)
            .krFocusRing(focused || appearance == .focused).onHover { hover = $0 }.accessibilityLabel(missing.map { "\(name) \($0)" } ?? name)
            .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

private struct KRKindUnderline: Shape {
    let dashed: Bool
    func path(in rect: CGRect) -> Path { Path { p in p.move(to: CGPoint(x: rect.minX, y: rect.midY)); p.addLine(to: CGPoint(x: rect.maxX, y: rect.midY)) } }
}
