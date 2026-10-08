import SwiftUI

/// Visual media kinds; colors are used only for icons and clip underlines.
public enum KRMediaKind: String, CaseIterable, Codable, Sendable {
    case video, image, audio, composition, subtitle, generator, adjustment
    /// NLE-007: a multicam group shown as one timeline-capable media item.
    case multicam
    public var icon: KRIcon {
        switch self {
        case .video: return .film
        case .image: return .image
        case .audio: return .audioLines
        case .composition: return .layers
        case .subtitle: return .captions
        case .generator: return .sparkles
        case .adjustment: return .slidersHorizontal
        case .multicam: return .clapperboard
        }
    }
    public func color(in palette: KRPalette) -> Color {
        switch self {
        case .video, .image, .multicam: return palette.kindVideo
        case .audio: return palette.kindAudio
        case .composition: return palette.kindComposition
        case .subtitle: return palette.kindSubtitle
        case .generator: return palette.kindGenerator
        case .adjustment: return palette.kindAdjustment
        }
    }
}

/// An asset list row with kind, metadata, duration, and optional missing diagnostic.
public struct KRAssetRow: View {
    @Environment(\.krPalette) private var p
    @State private var hover = false
    public let name: String
    public let kind: KRMediaKind
    public let meta: String
    public let duration: String
    public let selected: Bool
    public let missing: String?
    public let appearance: KRControlAppearance
    private let onSelect: () -> Void
    private let onOpen: () -> Void
    public init(_ name: String, kind: KRMediaKind, meta: String, duration: String = "—", selected: Bool = false,
                missing: String? = nil, appearance: KRControlAppearance = .resting, onSelect: @escaping () -> Void = {}, onOpen: @escaping () -> Void = {}) {
        self.name = name; self.kind = kind; self.meta = meta; self.duration = duration
        self.selected = selected; self.missing = missing; self.appearance = appearance; self.onSelect = onSelect; self.onOpen = onOpen
    }
    public var body: some View {
        Button(action: onSelect) {
            HStack(spacing: KRSpace.space2) {
                KRIconView(missing == nil ? kind.icon : .triangleAlert).foregroundStyle(missing == nil ? kind.color(in: p) : p.danger)
                Text(name).krText(KRType.body).foregroundStyle(missing == nil ? p.ink : p.danger).lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let missing { Text(missing).krText(KRMono.caption).foregroundStyle(p.danger) }
                else { Text(meta).krText(KRType.caption).foregroundStyle(p.inkMuted).lineLimit(1) }
                Text(duration).krText(KRType.timecode).foregroundStyle(p.inkMuted).frame(minWidth: 76, alignment: .trailing)
            }.padding(.horizontal, KRSpace.space3).frame(height: KRSize.rowHeight)
                .background(selected ? p.selectionBg : hover || appearance == .hover ? p.controlHover : .clear)
        }.buttonStyle(.plain).krControlFocusRing(appearance == .focused).onHover { hover = $0 }
            .simultaneousGesture(TapGesture(count: 2).onEnded(onOpen))
            .accessibilityLabel(name).accessibilityValue(missing ?? "\(meta) \(duration)")
            .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
