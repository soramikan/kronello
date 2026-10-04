import KronelloDesign
import SwiftUI

struct GallerySheet: View {
    @Environment(\.krPalette) var p
    let title: String
    let theme: KRTheme
    let content: AnyView
    init(_ title: String, theme: KRTheme, content: AnyView) { self.title = title; self.theme = theme; self.content = content }
    var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space4 * 2) {
            HStack { Text(title).krText(KRType.heading); Spacer(); Text(theme.rawValue.capitalized + " · 2x").krText(KRType.caption).foregroundStyle(p.inkMuted) }
            content
        }.foregroundStyle(p.ink).padding(KRSpace.space4 * 2).frame(width: 840, alignment: .leading).background(p.surface100)
    }
}

struct SampleRow<Content: View>: View {
    @Environment(\.krPalette) var p
    let label: String
    let content: Content
    init(_ label: String, @ViewBuilder content: () -> Content) { self.label = label; self.content = content() }
    var body: some View {
        HStack(alignment: .top, spacing: KRSpace.space4) {
            Text(label).krText(KRType.caption).foregroundStyle(p.inkMuted).frame(width: 116, alignment: .leading).padding(.top, KRSpace.space1)
            content
            Spacer(minLength: 0)
        }
    }
}

struct IconSheet: View {
    @Environment(\.krPalette) var p
    var body: some View {
        VStack(spacing: KRSpace.space3) {
            ForEach(0..<8) { row in
                HStack(spacing: KRSpace.space2) {
                    ForEach(Array(KRIcon.allCases.dropFirst(row * 8).prefix(8)), id: \.self) { icon in
                        VStack(spacing: KRSpace.space1) {
                            KRIconView(icon, size: 24)
                            HStack(spacing: KRSpace.space1) { KRIconView(icon); KRIconView(icon, size: 12) }
                            Text(icon.rawValue).krText(KRType.caption).lineLimit(1)
                        }.frame(width: 86)
                    }
                }
            }
        }.foregroundStyle(p.inkMuted)
    }
}

struct TypeSheet: View {
    var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space4) {
            ForEach([KRType.display, KRType.heading, KRType.body, KRType.label, KRType.caption, KRType.timecode, KRType.ruler], id: \.name) { style in
                SampleRow(style.name) { Text(style.family == KRFontFamily.mono ? "00:01:23:12 · 1s12f · 1920×1080" : "いま、時間を編む。Position 960 px").krText(style) }
            }
        }
    }
}

struct PaletteSheet: View {
    @Environment(\.krPalette) var p
    var body: some View {
        let colors: [(String, Color)] = [("surface0", p.surface0), ("surface100", p.surface100), ("surface200", p.surface200), ("controlHover", p.controlHover), ("line", p.line), ("lineStrong", p.lineStrong), ("ink", p.ink), ("inkMuted", p.inkMuted), ("accent", p.accent), ("accentInk", p.accentInk), ("onAccent", p.onAccent), ("selection", p.selection), ("selectionBg", p.selectionBg), ("onSelection", p.onSelection), ("clip", p.clip), ("danger", p.danger), ("kindVideo", p.kindVideo), ("kindAudio", p.kindAudio), ("kindComposition", p.kindComposition), ("kindSubtitle", p.kindSubtitle), ("kindGenerator", p.kindGenerator), ("kindAdjustment", p.kindAdjustment)]
        VStack(spacing: KRSpace.space4) {
            ForEach(0..<4) { row in HStack(spacing: KRSpace.space2) {
                ForEach(Array(colors.dropFirst(row * 6).prefix(6)), id: \.0) { item in
                    VStack { RoundedRectangle(cornerRadius: KRRadius.radiusSm).fill(item.1).frame(width: 112, height: 40); Text(item.0).krText(KRType.caption).foregroundStyle(p.inkMuted) }
                }
            } }
        }
    }
}
