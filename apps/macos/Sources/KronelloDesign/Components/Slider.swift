import SwiftUI

/// A neutral slider. Pair with KRNumberField for precise entry.
public struct KRSlider: View {
    @Environment(\.krPalette) private var p
    @Environment(\.isEnabled) private var enabled
    @Binding private var value: Double
    private let range: ClosedRange<Double>
    private let step: Double
    private let label: String
    private let onPreview: (Double) -> Void
    private let onCommit: (Double, Double) -> Void
    private let appearance: KRControlAppearance
    @State private var origin: Double?
    @State private var candidate: Double?
    @FocusState private var focused: Bool
    public init(value: Binding<Double>, range: ClosedRange<Double>, step: Double = 1, accessibilityLabel: String,
                appearance: KRControlAppearance = .resting, onPreview: @escaping (Double) -> Void = { _ in },
                onCommit: @escaping (Double, Double) -> Void) {
        precondition(range.upperBound > range.lowerBound && step > 0)
        _value = value; self.range = range; self.step = step; label = accessibilityLabel
        self.appearance = appearance; self.onPreview = onPreview; self.onCommit = onCommit
    }
    private func quantized(_ input: Double) -> Double {
        min(range.upperBound, max(range.lowerBound, range.lowerBound + ((input - range.lowerBound) / step).rounded() * step))
    }
    public var body: some View {
        GeometryReader { proxy in
            let width = max(0, proxy.size.width - 12)
            let fraction = min(1, max(0, ((candidate ?? value) - range.lowerBound) / (range.upperBound - range.lowerBound)))
            ZStack(alignment: .leading) {
                Capsule().fill(p.lineStrong).frame(height: 4)
                Capsule().fill(p.inkMuted).frame(width: width * fraction + 6, height: 4)
                Circle().fill(p.ink).frame(width: 12, height: 12)
                    .background { Circle().fill(p.surface100).padding(-2) }
                    .krFocusRing(focused || appearance == .focused, cornerRadius: 6)
                    .offset(x: width * fraction)
            }.frame(height: KRSize.toggleSize).contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                    guard enabled && width > 0 else { return }
                    if origin == nil { origin = value; focused = true }
                    candidate = quantized(range.lowerBound + (drag.location.x - 6) / width * (range.upperBound - range.lowerBound))
                    onPreview(candidate!)
                }.onEnded { _ in
                    if let from = origin, let to = candidate, from != to { value = to; onCommit(from, to) }
                    origin = nil; candidate = nil
                })
        }.frame(height: KRSize.toggleSize).focusable().focused($focused).focusEffectDisabled()
            .opacity(enabled ? 1 : 0.45)
            .onKeyPress(keys: [.leftArrow, .rightArrow, .upArrow, .downArrow]) { press in
                let direction: Double = press.key == .leftArrow || press.key == .downArrow ? -1 : 1
                change(quantized(value + direction * step)); return .handled
            }
            .accessibilityLabel(label).accessibilityValue(String(value))
            .accessibilityAdjustableAction { change(quantized(value + ($0 == .increment ? step : -step))) }
    }
    private func change(_ next: Double) { guard enabled && next != value else { return }; let previous = value; value = next; onCommit(previous, next) }
}
