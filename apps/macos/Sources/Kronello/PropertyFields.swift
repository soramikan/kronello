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
    @FocusState private var attachFocused: Bool
    let layer: Layer
    let property: [String: Any]

    var body: some View {
        let presentation = PropertyPresentation.of(property)
        let value = layer.value(property)
        let numbers = model.propertyNumbers(layer, property)
        let disabled = model.ui.locked.contains(layer.id) || model.busy || model.pendingCandidate != nil
        HStack(spacing: KRSpace.space1) {
            Group {
                if value.string("kind") == "color" {
                    PropertyColorEditor(model: model, layer: layer, property: property)
                } else if value.string("kind") == "bool" {
                    Toggle("", isOn: Binding(get: { (layer.value(property)["value"] as? Bool) ?? false },
                        set: { model.setBool(layer, property: property, to: $0, base: model.revision) }))
                        .toggleStyle(.checkbox)
                        .accessibilityLabel(presentation.label)
                        .disabled(property.object("source").string("kind") != "constant")
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
            }
            // Attach an expression seeded by the current value (ADR-0105); the
            // service rejects properties whose descriptor disallows expressions.
            if KRPropertySource(property) != .expression {
                Button { model.attachExpression(layer, property: property) } label: {
                    Text("fx").krText(KRType.caption).foregroundStyle(p.inkMuted)
                        .padding(.horizontal, 3).padding(.vertical, 1)
                        .overlay { RoundedRectangle(cornerRadius: 2).strokeBorder(p.lineStrong, lineWidth: 1) }
                }.buttonStyle(.plain).focused($attachFocused).krFocusRing(attachFocused, cornerRadius: 2)
                    .accessibilityLabel("式を追加").help("式で値を駆動します")
            }
        }.disabled(disabled)
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
