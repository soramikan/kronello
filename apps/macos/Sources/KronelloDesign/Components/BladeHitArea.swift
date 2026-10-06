import AppKit
import SwiftUI

/// An explicit blade hit area above the clip Button, with one release per press.
public struct KRBladeHitArea: NSViewRepresentable {
    @Environment(\.isEnabled) private var enabled
    public let label: String
    public let begin: (Double) -> Void
    public let update: (Double) -> Void
    public let release: () -> Void
    public init(_ label: String, begin: @escaping (Double) -> Void, update: @escaping (Double) -> Void, release: @escaping () -> Void) {
        self.label = label; self.begin = begin; self.update = update; self.release = release
    }
    public func makeNSView(context: Context) -> KRBladeHitView { let view = KRBladeHitView(); configure(view); return view }
    public func updateNSView(_ view: KRBladeHitView, context: Context) { configure(view) }
    private func configure(_ view: KRBladeHitView) {
        view.enabled = enabled
        view.setAccessibilityElement(true); view.setAccessibilityRole(.button); view.setAccessibilityLabel(label)
        view.begin = begin; view.update = update; view.release = release
    }
}

/// Public so direct checks can dispatch real NSEvents through this hit-testing path.
@MainActor public final class KRBladeHitView: NSView {
    public var enabled = true
    public var begin: (Double) -> Void = { _ in }
    public var update: (Double) -> Void = { _ in }
    public var release: () -> Void = {}
    private var pressed = false
    public override func mouseDown(with event: NSEvent) { guard enabled else { return }; pressed = true; begin(fraction(event)) }
    public override func mouseDragged(with event: NSEvent) { if pressed { update(fraction(event)) } }
    public override func mouseUp(with event: NSEvent) {
        guard pressed else { return }; pressed = false; update(fraction(event)); release()
    }
    public override func accessibilityPerformPress() -> Bool { guard enabled else { return false }; begin(0.5); release(); return true }
    private func fraction(_ event: NSEvent) -> Double {
        Double(convert(event.locationInWindow, from: nil).x / max(1, bounds.width))
    }
}
