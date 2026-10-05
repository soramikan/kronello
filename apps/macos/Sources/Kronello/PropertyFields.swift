import SwiftUI
import KronelloDesign
import KronelloAppModel

extension KRPropertySource {
    init(_ property: [String: Any]) {
        switch property.object("source").string("kind") {
        case "curve": self = .curve
        case "expression": self = .expression
        default: self = .constant
        }
    }
}

/// The value cell of one Property row: axis fields (64px, no unit) for vectors,
/// one 88px field with its unit for scalars, a swatch for colors.
struct PropertyValue: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    let layer: Layer
    let property: [String: Any]

    var body: some View {
        let presentation = PropertyPresentation.of(property)
        let value = layer.value(property)
        let numbers = model.propertyNumbers(layer, property)
        Group {
            if value.string("kind") == "color" {
                swatch(value.object("value").object("components"))
            } else if value.string("kind") == "enum" {
                Text(value.string("value")).krText(KRType.label).foregroundStyle(p.inkMuted)
            } else if numbers.count > 1 {
                HStack(spacing: KRSpace.space1) {
                    ForEach(Array(numbers.enumerated()), id: \.offset) { axis, number in
                        KRInspectorAxis(axis == 0 ? "X" : "Y") {
                            field(axis: axis, number: number, unit: "", presentation: presentation).frame(width: KRWindowMetrics.numberWidth)
                        }
                    }
                }
            } else if let number = numbers.first {
                field(axis: 0, number: number, unit: presentation.unit, presentation: presentation).frame(width: KRWindowMetrics.scalarWidth)
            } else {
                Text("—").krText(KRType.timecode).foregroundStyle(p.inkMuted)
            }
        }.disabled(model.ui.locked.contains(layer.id) || model.busy || model.pendingCandidate != nil)
    }
    func field(axis: Int, number: Double, unit: String, presentation: PropertyPresentation) -> some View {
        KRNumberField(value: .constant(number * presentation.multiplier), unit: unit,
            error: model.propertyError(layer, property) != nil, accessibilityLabel: presentation.label + (unit.isEmpty && axis < 2 ? [" X", " Y"][axis] : ""),
            onPreview: { model.previewNumber(layer: layer, property: property, axis: axis, to: $0 / presentation.multiplier) },
            onCommit: { model.commitNumber(layer: layer, property: property, axis: axis, from: $0 / presentation.multiplier, to: $1 / presentation.multiplier) })
    }
    func swatch(_ c: [String: Any]) -> some View {
        let color = Color(.sRGB, red: c.number("r"), green: c.number("g"), blue: c.number("b"), opacity: c["alpha"] == nil ? 1 : c.number("alpha"))
        let hex = String(format: "#%02X%02X%02X", Int((c.number("r") * 255).rounded()), Int((c.number("g") * 255).rounded()), Int((c.number("b") * 255).rounded()))
        return HStack(spacing: KRSpace.space2) {
            RoundedRectangle(cornerRadius: KRRadius.radiusSm).fill(color).frame(width: 16, height: 16)
                .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusSm).stroke(p.lineStrong, lineWidth: 1) }
            Text(hex).krText(KRType.timecode).foregroundStyle(p.inkMuted)
        }.help("色の編集は後続タスクです").accessibilityLabel("色 " + hex)
    }
}
