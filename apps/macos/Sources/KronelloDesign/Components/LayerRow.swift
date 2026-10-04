import SwiftUI

/// Scene kinds used to choose an icon, never to derive hierarchy or identity.
public enum KRLayerKind: Sendable {
    case group, null, shape, text, media(KRMediaKind), composition
    public var icon: KRIcon {
        switch self {
        case .group: return .folder
        case .null: return .crosshair
        case .shape: return .shapes
        case .text: return .type
        case .media(let kind): return kind.icon
        case .composition: return .layers
        }
    }
    public func color(in p: KRPalette) -> Color {
        switch self { case .media(let kind): return kind.color(in: p); case .composition: return p.kindComposition; default: return p.inkMuted }
    }
}

/// One scene row; containment indent and transform-parent label are independent.
public struct KRLayerRow: View {
    @Environment(\.krPalette) private var p
    @State private var hover = false
    @FocusState private var focusedToggle: String?
    public let name: String
    public let kind: KRLayerKind
    public let level: Int
    public let hasChildren: Bool
    public let expanded: Bool
    public let parent: String?
    public let hidden: Bool
    public let locked: Bool
    public let selected: Bool
    public let appearance: KRControlAppearance
    private let onSelect: () -> Void
    private let onDisclosure: () -> Void
    private let onVisibility: () -> Void
    private let onLock: () -> Void
    public init(_ name: String, kind: KRLayerKind, level: Int = 0, hasChildren: Bool = false, expanded: Bool = false,
                transformParent: String? = nil, hidden: Bool = false, locked: Bool = false, selected: Bool = false, appearance: KRControlAppearance = .resting,
                onSelect: @escaping () -> Void = {}, onDisclosure: @escaping () -> Void = {},
                onVisibility: @escaping () -> Void = {}, onLock: @escaping () -> Void = {}) {
        self.name = name; self.kind = kind; self.level = max(0, level); self.hasChildren = hasChildren
        self.expanded = expanded; parent = transformParent; self.hidden = hidden; self.locked = locked; self.selected = selected; self.appearance = appearance
        self.onSelect = onSelect; self.onDisclosure = onDisclosure; self.onVisibility = onVisibility; self.onLock = onLock
    }
    public var body: some View {
        HStack(spacing: KRSpace.space1) {
            Button(action: onDisclosure) { KRIconView(expanded ? .chevronDown : .chevronRight, size: 12).frame(height: 16) }
                .buttonStyle(.plain).foregroundStyle(p.inkMuted).krFocusRing(cornerRadius: 2)
                .opacity(hasChildren ? 1 : 0).disabled(!hasChildren).accessibilityLabel(expanded ? "\(name) を畳む" : "\(name) を開く")
            Button(action: onSelect) {
                HStack(spacing: KRSpace.space1) {
                    KRIconView(kind.icon).foregroundStyle(hidden ? p.inkMuted : kind.color(in: p))
                    HStack(alignment: .firstTextBaseline, spacing: KRSpace.space2) {
                        Text(name).italicIf(locked).krText(KRType.body).lineLimit(1).foregroundStyle(hidden ? p.inkMuted : p.ink)
                        if let parent {
                            HStack(spacing: 2) { KRIconView(.link, size: 10); Text(parent).krText(KRType.caption).lineLimit(1) }.foregroundStyle(p.inkMuted)
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
            }.buttonStyle(.plain).krFocusRing().accessibilityLabel(name).accessibilityAddTraits(selected ? .isSelected : [])
            HStack(spacing: 2) {
                KRButton(icon: hidden ? .eyeOff : .eye, accessibilityLabel: hidden ? "表示する" : "非表示にする", pressed: hidden,
                         size: 20, iconSize: 13, action: onVisibility).focused($focusedToggle, equals: "visibility")
                    .opacity(hover || appearance == .hover || hidden || focusedToggle != nil ? 1 : 0)
                KRButton(icon: locked ? .lock : .lockOpen, accessibilityLabel: locked ? "ロックを解除" : "ロックする", pressed: locked,
                         size: 20, iconSize: 13, action: onLock).focused($focusedToggle, equals: "lock")
                    .opacity(hover || appearance == .hover || locked || focusedToggle != nil ? 1 : 0)
            }
        }.padding(.leading, KRSpace.space2 + CGFloat(level) * KRSpace.space3).padding(.trailing, KRSpace.space1)
            .frame(height: KRSize.rowHeight).background(selected ? p.selectionBg : hover || appearance == .hover ? p.controlHover : .clear)
            .krFocusRing(appearance == .focused)
            .onHover { hover = $0 }
    }
}

extension Text {
    func italicIf(_ condition: Bool) -> Text { condition ? italic() : self }
}
