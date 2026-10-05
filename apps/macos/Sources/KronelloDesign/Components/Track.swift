import SwiftUI

/// A track header with neutral number, title, visibility or mute, and lock controls.
public struct KRTrackHeader: View {
    @Environment(\.krPalette) private var p
    public let number: String
    public let name: String
    public let kind: KRMediaKind
    public let selected: Bool
    public let hidden: Bool
    public let locked: Bool
    public let visibilityEnabled: Bool
    private let visibility: () -> Void
    private let lock: () -> Void
    public init(_ number: String, _ name: String, kind: KRMediaKind, selected: Bool = false, hidden: Bool = false, locked: Bool = false,
                visibilityEnabled: Bool = true, onVisibility: @escaping () -> Void = {}, onLock: @escaping () -> Void = {}) {
        self.number = number; self.name = name; self.kind = kind; self.selected = selected; self.hidden = hidden; self.locked = locked
        visibility = onVisibility; lock = onLock
        self.visibilityEnabled = visibilityEnabled
    }
    public var body: some View {
        HStack(spacing: KRSpace.space1) {
            Text(number).krText(KRType.ruler).foregroundStyle(p.inkMuted)
            Text(name).krText(KRType.label).foregroundStyle(p.ink).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
            KRButton(icon: kind == .audio ? (hidden ? .volumeX : .volume2) : (hidden ? .eyeOff : .eye),
                     accessibilityLabel: kind == .audio ? "ミュートを切り替える" : "表示を切り替える", pressed: hidden, iconSize: 12, action: visibility)
                .disabled(!visibilityEnabled).help(visibilityEnabled ? "表示・ミュート" : "トラックの表示・ミュートは未対応です")
            KRButton(icon: locked ? .lock : .lockOpen, accessibilityLabel: "ロックを切り替える", pressed: locked, iconSize: 12, action: lock)
        }.padding(.leading, KRSpace.space3).padding(.trailing, KRSpace.space1).frame(height: KRSize.trackHeight)
            .background(selected ? p.selectionBg : p.surface100)
            .overlay(alignment: .trailing) { p.line.frame(width: 1) }
    }
}

/// A fixed-height track with a header and consumer-positioned lane content.
public struct KRTrack<Lane: View>: View {
    @Environment(\.krPalette) private var p
    public let header: KRTrackHeader
    public let headerWidth: CGFloat
    public let locked: Bool
    private let lane: Lane
    public init(header: KRTrackHeader, headerWidth: CGFloat = 168, locked: Bool = false, @ViewBuilder lane: () -> Lane) {
        self.header = header; self.headerWidth = headerWidth; self.locked = locked; self.lane = lane()
    }
    public var body: some View {
        HStack(spacing: 0) {
            header.frame(width: headerWidth)
            lane.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
                .background(p.surface200).opacity(locked ? 0.6 : 1).clipped()
        }.frame(height: KRSize.trackHeight).krBottomLine()
    }
}
