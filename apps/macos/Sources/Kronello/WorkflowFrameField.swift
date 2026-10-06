import SwiftUI
import KronelloDesign

/// Frame-based candidate input, committed once by the shared NumberField gesture.
struct WorkflowFrameField: View {
    let source: Double
    let step: Double
    let range: ClosedRange<Double>
    let unit: String
    let width: CGFloat
    let accessibilityLabel: String
    let onCommit: (Double,Double) -> Void
    @State private var value: Double
    init(_ source: Double, step: Double, range: ClosedRange<Double>, unit: String, width: CGFloat, accessibilityLabel: String, onCommit: @escaping (Double,Double) -> Void) {
        self.source = source; self.step = step; self.range = range; self.unit = unit; self.width = width; self.onCommit = onCommit
        self.accessibilityLabel = accessibilityLabel
        _value = State(initialValue: source)
    }
    var body: some View {
        KRNumberField(value: $value, unit: unit, step: step, range: range, precision: 0, accessibilityLabel: accessibilityLabel, onCommit: onCommit)
            .frame(width: width).onChange(of: source) { _, v in value = v }
    }
}
