import AppKit
import CoreText
import SwiftUI

/// Kronello's two themes. Dark is the default and the app never follows the
/// system appearance (ADR-0054); the user switches in settings.
public enum KRTheme: String, CaseIterable, Sendable {
    case dark
    case light

    /// The AppKit appearance that OS-drawn UI (menus, sheets' chrome) should use.
    public var appearance: NSAppearance? {
        NSAppearance(named: self == .dark ? .darkAqua : .aqua)
    }

    public var colorScheme: ColorScheme { self == .dark ? .dark : .light }
}

/// An sRGB color with straight alpha, as written in tokens.json.
public struct KRRGBA: Sendable, Equatable {
    public let red, green, blue, alpha: Double

    public init(_ red: Double, _ green: Double, _ blue: Double, _ alpha: Double) {
        self.red = red
        self.green = green
        self.blue = blue
        self.alpha = alpha
    }

    public var color: Color { Color(.sRGB, red: red, green: green, blue: blue, opacity: alpha) }
    public var nsColor: NSColor { NSColor(srgbRed: red, green: green, blue: blue, alpha: alpha) }
}

/// One layer of a CSS-style box shadow. A zero-blur layer with spread is a ring.
public struct KRShadowLayer: Sendable, Equatable {
    public let x, y, blur, spread: CGFloat
    public let color: KRRGBA
}

public struct KRShadow: Sendable, Equatable {
    public let layers: [KRShadowLayer]
    public init(_ layers: [KRShadowLayer]) { self.layers = layers }
}

/// A text style from tokens.json. Weight is the CSS numeric weight.
public struct KRTextStyle: Sendable, Equatable {
    public let name: String
    public let family: [String]
    public let size: CGFloat
    public let lineHeight: CGFloat
    public let weight: Int
    public let tracking: CGFloat

    public func font(weight override: Int? = nil) -> Font {
        let w = override ?? weight
        let name = family.first(where: KRFonts.isAvailable) ?? family.first ?? ""
        return Font.custom(name, fixedSize: size).weight(Self.swiftUIWeight(w))
    }

    public func nsFont(weight override: Int? = nil) -> NSFont {
        let w = override ?? weight
        for name in family where KRFonts.isAvailable(name) {
            let descriptor = NSFontDescriptor(fontAttributes: [.family: name])
                .addingAttributes([.traits: [NSFontDescriptor.TraitKey.weight: Self.appKitWeight(w)]])
            if let font = NSFont(descriptor: descriptor, size: size) { return font }
        }
        return NSFont.systemFont(ofSize: size, weight: NSFont.Weight(Self.appKitWeight(w)))
    }

    static func swiftUIWeight(_ w: Int) -> Font.Weight {
        switch w {
        case ..<350: return .light
        case ..<450: return .regular
        case ..<550: return .medium
        case ..<650: return .semibold
        default: return .bold
        }
    }

    static func appKitWeight(_ w: Int) -> CGFloat {
        switch w {
        case ..<350: return NSFont.Weight.light.rawValue
        case ..<450: return NSFont.Weight.regular.rawValue
        case ..<550: return NSFont.Weight.medium.rawValue
        case ..<650: return NSFont.Weight.semibold.rawValue
        default: return NSFont.Weight.bold.rawValue
        }
    }
}

/// Registers the bundled UI fonts (Noto Sans JP, Noto Sans Mono; SIL OFL).
public enum KRFonts {
    /// Registers every font file in the given directory for this process.
    @discardableResult
    public static func register(directory: URL) -> [URL] {
        let files = (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? []
        let fonts = files.filter { ["ttf", "otf"].contains($0.pathExtension.lowercased()) }
        for url in fonts {
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
        return fonts
    }

    public static func isAvailable(_ family: String) -> Bool {
        NSFontManager.shared.availableFontFamilies.contains(family)
    }
}

// MARK: - Environment

private struct KRThemeKey: EnvironmentKey {
    static let defaultValue: KRTheme = .dark
}

extension EnvironmentValues {
    /// The current Kronello theme. Set it once at the window root with `.krTheme(_:)`.
    public var krTheme: KRTheme {
        get { self[KRThemeKey.self] }
        set { self[KRThemeKey.self] = newValue }
    }

    /// The palette of the current theme.
    public var krPalette: KRPalette { KRPalette.of(krTheme) }
}

extension View {
    /// Applies a Kronello theme to this view tree (colors and the AppKit color scheme).
    public func krTheme(_ theme: KRTheme) -> some View {
        environment(\.krTheme, theme).environment(\.colorScheme, theme.colorScheme)
    }

    /// Sets the font, tracking and line box of a token text style.
    public func krText(_ style: KRTextStyle, weight: Int? = nil) -> some View {
        font(style.font(weight: weight))
            .tracking(style.tracking)
            .lineSpacing(max(0, style.lineHeight - style.size * 1.2))
    }

    /// Draws a token shadow: blurred layers as shadows, zero-blur spread layers as a 1px-style ring.
    public func krShadow(_ shadow: KRShadow, cornerRadius: CGFloat) -> some View {
        modifier(KRShadowModifier(shadow: shadow, cornerRadius: cornerRadius))
    }
}

private struct KRShadowModifier: ViewModifier {
    let shadow: KRShadow
    let cornerRadius: CGFloat

    func body(content: Content) -> some View {
        shadow.layers.reduce(AnyView(content)) { view, layer in
            if layer.blur == 0 && layer.spread > 0 {
                return AnyView(view.overlay(
                    RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                        .strokeBorder(layer.color.color, lineWidth: layer.spread)
                ))
            }
            return AnyView(view.shadow(color: layer.color.color, radius: layer.blur / 2, x: layer.x, y: layer.y))
        }
    }
}

// MARK: - Icons

/// A Lucide icon drawn with Lucide's stroke (2 units on the 24-unit grid, round caps and joins),
/// scaled to `size` and painted with the current foreground style.
public struct KRIconView: View {
    public let icon: KRIcon
    public let size: CGFloat

    public init(_ icon: KRIcon, size: CGFloat = 14) {
        self.icon = icon
        self.size = size
    }

    public var body: some View {
        KRIconShape(icon: icon)
            .stroke(style: StrokeStyle(lineWidth: 2 * size / 24, lineCap: .round, lineJoin: .round))
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}

struct KRIconShape: Shape {
    let icon: KRIcon

    func path(in rect: CGRect) -> Path {
        let scale = min(rect.width, rect.height) / 24
        return icon.path.applying(CGAffineTransform(scaleX: scale, y: scale)
            .concatenating(CGAffineTransform(translationX: rect.minX, y: rect.minY)))
    }
}

extension KRFonts {
    /// Registers the fonts bundled in KronelloDesign's resources. Call once at launch.
    /// Returns the registered files; an empty result means `scripts/fetch_ui_fonts.py` was not run.
    @discardableResult
    public static func registerBundled() -> [URL] {
        guard let directory = Bundle.module.url(forResource: "Fonts", withExtension: nil) else { return [] }
        return register(directory: directory)
    }
}
