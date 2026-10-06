import AppKit
import SwiftUI

public enum KRClipDragMode { case move, trimStart, trimEnd }

/// Keeps clip pointer handling separate from its decorative Button. Deltas use
/// the press's window coordinate, so moving the preview cannot move the origin.
public struct KRClipHitArea: NSViewRepresentable {
    @Environment(\.isEnabled) private var enabled
    public let label: String
    public let select: () -> Void
    public let open: () -> Void
    public let begin: (KRClipDragMode) -> Void
    public let update: (Double) -> Void
    public let release: () -> Void
    public let cancel: () -> Void
    public init(_ label: String, select: @escaping () -> Void, open: @escaping () -> Void,
                begin: @escaping (KRClipDragMode) -> Void, update: @escaping (Double) -> Void,
                release: @escaping () -> Void, cancel: @escaping () -> Void) {
        self.label = label; self.select = select; self.open = open; self.begin = begin
        self.update = update; self.release = release; self.cancel = cancel
    }
    public func makeNSView(context: Context) -> KRClipHitView { let view = KRClipHitView(); configure(view); return view }
    public func updateNSView(_ view: KRClipHitView, context: Context) { configure(view) }
    private func configure(_ view: KRClipHitView) {
        view.enabled = enabled
        view.setAccessibilityElement(true); view.setAccessibilityRole(.button); view.setAccessibilityLabel(label)
        view.select = select; view.open = open; view.begin = begin
        view.update = update; view.release = release; view.cancel = cancel
    }
}

@MainActor public final class KRClipHitView: NSView {
    public var enabled = true
    public var select: () -> Void = {}
    public var open: () -> Void = {}
    public var begin: (KRClipDragMode) -> Void = { _ in }
    public var update: (Double) -> Void = { _ in }
    public var release: () -> Void = {}
    public var cancel: () -> Void = {}
    private var origin: CGFloat?
    private var mode: KRClipDragMode = .move
    private var dragging = false
    public override func layout() {
        super.layout()
        removeAllToolTips()
        let edge = min(6, bounds.width / 2)
        addToolTip(NSRect(x: 0, y: 0, width: edge, height: bounds.height), owner: self, userData: nil)
        addToolTip(NSRect(x: bounds.width - edge, y: 0, width: edge, height: bounds.height), owner: self, userData: nil)
    }
    @objc public func view(_ view: NSView, stringForToolTip tag: NSView.ToolTipTag, point: NSPoint, userData data: UnsafeMutableRawPointer?) -> String {
        point.x < bounds.width / 2 ? "開始をトリム" : "末尾をトリム"
    }
    public override func mouseDown(with event: NSEvent) {
        guard enabled else { return }
        select()
        if event.clickCount == 2 { origin = nil; dragging = false; open(); return }
        origin = event.locationInWindow.x; dragging = false
        let x = convert(event.locationInWindow, from: nil).x
        mode = x < 6 ? .trimStart : x >= bounds.width - 6 ? .trimEnd : .move
    }
    public override func mouseDragged(with event: NSEvent) {
        guard let origin else { return }
        guard enabled else { cancelPress(); return }
        let delta = Double(event.locationInWindow.x - origin)
        if !dragging {
            guard abs(delta) >= 3 else { return }
            dragging = true; begin(mode)
        }
        update(delta)
    }
    public override func mouseUp(with event: NSEvent) {
        guard let origin else { return }
        self.origin = nil
        guard enabled else { if dragging { cancel() }; dragging = false; return }
        guard dragging else { return }
        dragging = false
        update(Double(event.locationInWindow.x - origin)); release()
    }
    public override func accessibilityPerformPress() -> Bool { guard enabled else { return false }; select(); return true }
    private func cancelPress() { origin = nil; if dragging { cancel() }; dragging = false }
}
