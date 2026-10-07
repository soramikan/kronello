import SwiftUI

/// A track header with neutral number, title, visibility or mute, lock and
/// target controls. Targeting exists only on video and audio kinds; caption
/// lanes render no target button.
public struct KRTrackHeader: View {
    @Environment(\.krPalette) private var p
    public let number: String
    public let name: String
    public let kind: KRMediaKind
    public let selected: Bool
    public let hidden: Bool
    public let locked: Bool
    public let targeted: Bool
    public let visibilityEnabled: Bool
    private let visibility: () -> Void
    private let lock: () -> Void
    private let target: () -> Void
    public init(_ number: String, _ name: String, kind: KRMediaKind, selected: Bool = false, hidden: Bool = false, locked: Bool = false,
                targeted: Bool = false, visibilityEnabled: Bool = true,
                onVisibility: @escaping () -> Void = {}, onLock: @escaping () -> Void = {}, onTarget: @escaping () -> Void = {}) {
        self.number = number; self.name = name; self.kind = kind; self.selected = selected; self.hidden = hidden; self.locked = locked
        self.targeted = targeted
        visibility = onVisibility; lock = onLock; target = onTarget
        self.visibilityEnabled = visibilityEnabled
    }
    public var body: some View {
        HStack(spacing: KRSpace.space1) {
            Text(number).krText(KRType.ruler).foregroundStyle(p.inkMuted)
            Text(name).krText(KRType.label).foregroundStyle(p.ink).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
            if kind == .video || kind == .audio {
                KRButton(icon: .crosshair, accessibilityLabel: "ターゲットトラックを切り替える", pressed: targeted, iconSize: 12, action: target)
                    .help("暗黙の編集先トラック")
            }
            KRButton(icon: kind == .audio ? (hidden ? .volumeX : .volume2) : (hidden ? .eyeOff : .eye),
                     accessibilityLabel: kind == .audio ? "ミュートを切り替える" : "表示を切り替える", pressed: hidden, iconSize: 12, action: visibility)
                .disabled(!visibilityEnabled).help(visibilityEnabled ? "表示・ミュート" : "ロック中または実行中は切り替えられません")
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
