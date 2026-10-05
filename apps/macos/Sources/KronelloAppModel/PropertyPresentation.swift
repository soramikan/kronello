import Foundation

/// Display name, unit and scale of a Property. Inspector and Dope sheet share it so
/// both show and edit the same value the same way (motion.md).
public struct PropertyPresentation: Sendable {
    public let label: String
    public let unit: String
    public let multiplier: Double

    public static func of(_ key: String) -> PropertyPresentation {
        switch key {
        case "kronello.transform.position": return .init(label: "Position", unit: "px", multiplier: 1)
        case "kronello.transform.scale": return .init(label: "Scale", unit: "%", multiplier: 100)
        case "kronello.transform.rotation": return .init(label: "Rotation", unit: "°", multiplier: 1)
        case "kronello.opacity": return .init(label: "Opacity", unit: "%", multiplier: 100)
        case "kronello.fill_color": return .init(label: "Fill", unit: "", multiplier: 1)
        case "kronello.shape.size": return .init(label: "Size", unit: "px", multiplier: 1)
        case "kronello.shape.corner_radius": return .init(label: "Corner radius", unit: "px", multiplier: 1)
        case "kronello.text.font_size": return .init(label: "Size", unit: "px", multiplier: 1)
        case "kronello.text.line_height": return .init(label: "Line height", unit: "px", multiplier: 1)
        case "kronello.text.wrap_width": return .init(label: "Wrap width", unit: "px", multiplier: 1)
        case "kronello.text.alignment": return .init(label: "Alignment", unit: "", multiplier: 1)
        default:
            let last = key.split(separator: ".").last.map(String.init) ?? key
            let words = last.replacingOccurrences(of: "_", with: " ")
            return .init(label: words.prefix(1).uppercased() + words.dropFirst(), unit: "", multiplier: 1)
        }
    }
    public static func of(_ property: [String: Any]) -> PropertyPresentation { of(property.object("descriptor").string("key")) }
}
