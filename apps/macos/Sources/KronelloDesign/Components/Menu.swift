import AppKit
import SwiftUI

/// A menu entry with a stable ID, optional mark, shortcut, or one level of children.
public struct KRMenuItem: Identifiable {
    public enum Kind { case action, heading, separator }
    public var id: String
    public var label: String
    public var kind: Kind
    public var icon: KRIcon?
    public var shortcut: String?
    public var checked: Bool
    public var disabled: Bool
    public var destructive: Bool
    public var children: [KRMenuItem]
    public var action: () -> Void
    public init(_ id: String, _ label: String = "", kind: Kind = .action, icon: KRIcon? = nil,
                shortcut: String? = nil, checked: Bool = false, disabled: Bool = false,
                destructive: Bool = false, children: [KRMenuItem] = [], action: @escaping () -> Void = {}) {
        self.id = id; self.label = label; self.kind = kind; self.icon = icon; self.shortcut = shortcut
        self.checked = checked; self.disabled = disabled; self.destructive = destructive
        self.children = children; self.action = action
    }
}

/// A custom menu list, shared by popup buttons, context menus, and child panels.
public struct KRMenu: View {
    @Environment(\.krPalette) private var p
    public let items: [KRMenuItem]
    private let onDismiss: () -> Void
    private let onSubmenu: ((KRMenuItem, NSView) -> Void)?
    private let onLeafHover: (() -> Void)?
    @State private var current: String?
    @State private var anchors: [String: KRMenuAnchorBox] = [:]
    @FocusState private var focused: Bool
    /// Submenu callbacks receive the row's backing NSView for exact anchoring.
    public init(_ items: [KRMenuItem], current: String? = nil, onDismiss: @escaping () -> Void = {},
                onSubmenu: ((KRMenuItem, NSView) -> Void)? = nil, onLeafHover: (() -> Void)? = nil) {
        self.items = items; _current = State(initialValue: current); self.onDismiss = onDismiss
        self.onSubmenu = onSubmenu; self.onLeafHover = onLeafHover
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(items) { item in
                switch item.kind {
                case .separator:
                    p.line.frame(height: 1).padding(.vertical, KRSpace.space1).padding(.horizontal, KRSpace.space2)
                case .heading:
                    Text(item.label).krText(KRType.caption, weight: 600).foregroundStyle(p.inkMuted)
                        .padding(.horizontal, KRSpace.space2).padding(.top, KRSpace.space1).padding(.bottom, 2)
                case .action:
                    KRMenuEntry(item: item, active: current == item.id, activate: { current = item.id },
                                dismiss: onDismiss, submenu: onSubmenu, closeSubmenus: onLeafHover,
                                registerAnchor: { anchors[item.id] = KRMenuAnchorBox($0) })
                }
            }
        }.padding(KRSpace.space1).frame(minWidth: 220)
            .background(p.surface200, in: RoundedRectangle(cornerRadius: KRRadius.radiusMd))
            .krShadow(p.shadowPopover, cornerRadius: KRRadius.radiusMd)
            .focusable().focused($focused).focusEffectDisabled().onAppear { focused = true }
            .onKeyPress(keys: [.upArrow, .downArrow]) { key in
                let choices = items.filter { $0.kind == .action && !$0.disabled }
                guard !choices.isEmpty else { return .handled }
                let previous = choices.firstIndex { $0.id == current }
                let next = key.key == .downArrow ? (previous.map { ($0 + 1) % choices.count } ?? 0)
                    : (previous.map { ($0 + choices.count - 1) % choices.count } ?? choices.count - 1)
                current = choices[next].id; return .handled
            }
            .onKeyPress(.return) {
                guard let item = items.first(where: { $0.id == current && !$0.disabled }) else { return .ignored }
                if !item.children.isEmpty {
                    if let anchor = anchors[item.id]?.view { onSubmenu?(item, anchor) }
                    return .handled
                }
                onDismiss(); item.action(); return .handled
            }
            .onKeyPress(.rightArrow) {
                guard let item = items.first(where: { $0.id == current && !$0.disabled }), !item.children.isEmpty,
                      let anchor = anchors[item.id]?.view else { return .ignored }
                onSubmenu?(item, anchor); return .handled
            }
            .onKeyPress(.escape) { onDismiss(); return .handled }
            .accessibilityElement(children: .contain).accessibilityLabel("メニュー")
    }
}

/// Weak box so menu rows never keep their backing NSView alive past dismissal.
final class KRMenuAnchorBox {
    weak var view: NSView?
    init(_ view: NSView) { self.view = view }
}

private struct KRMenuEntry: View {
    @Environment(\.krPalette) var p
    @State private var anchor: NSView?
    @State private var hoverToken = UUID()
    let item: KRMenuItem
    let active: Bool
    let activate: () -> Void
    let dismiss: () -> Void
    let submenu: ((KRMenuItem, NSView) -> Void)?
    let closeSubmenus: (() -> Void)?
    let registerAnchor: (NSView) -> Void
    var foreground: Color { active ? p.onSelection : item.destructive ? p.danger : p.ink }
    var auxiliary: Color { active ? p.onSelection : item.destructive ? p.danger : p.inkMuted }
    var body: some View {
        Button {
            if !item.children.isEmpty, let anchor { submenu?(item, anchor) }
            else if item.children.isEmpty { dismiss(); item.action() }
        } label: {
            HStack(spacing: KRSpace.space2) {
                Group { if let icon = item.checked ? KRIcon.check : item.icon { KRIconView(icon) } else { Color.clear } }
                    .frame(width: 14, height: 14).foregroundStyle(auxiliary)
                Text(item.label).krText(KRType.body).frame(maxWidth: .infinity, alignment: .leading).foregroundStyle(foreground)
                if let shortcut = item.shortcut { Text(shortcut).krText(KRType.label, weight: 400).tracking(0.44).foregroundStyle(auxiliary) }
                Group { if !item.children.isEmpty { KRIconView(.chevronRight, size: 12) } else { Color.clear } }
                    .frame(width: 12, height: 12).foregroundStyle(auxiliary)
            }.padding(.horizontal, KRSpace.space2).frame(height: KRSize.controlHeight)
                .background(active ? p.selection : .clear, in: RoundedRectangle(cornerRadius: 4))
        }.buttonStyle(.plain).disabled(item.disabled).opacity(item.disabled ? 0.45 : 1)
            .krControlFocusRing(cornerRadius: 4).background {
                KRAnchorTracker { view in anchor = view; registerAnchor(view) }
            }
            .onHover { inside in
                hoverToken = UUID()
                guard inside, !item.disabled else { return }
                activate()
                // Match macOS menus: hovering a parent row opens its submenu
                // and hovering a leaf row closes it, each after a short delay
                // so diagonal movement toward the panel is not punished.
                let token = hoverToken
                if !item.children.isEmpty, let anchor, let submenu {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                        if hoverToken == token { submenu(item, anchor) }
                    }
                } else if item.children.isEmpty, let closeSubmenus {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                        if hoverToken == token { closeSubmenus() }
                    }
                }
            }
            .onKeyPress(.rightArrow) {
                if !item.children.isEmpty, let anchor { submenu?(item, anchor); return .handled }
                return .ignored
            }
            .accessibilityLabel(item.label).accessibilityValue(item.checked ? "選択中" : "")
    }
}

/// Tracks the backing NSView filling the modified view so callers can anchor
/// AppKit chrome (menus, popovers) to the exact on-screen position without
/// relying on `.frame(in: .global)`, which is not reliable inside nested
/// window scenes.
struct KRAnchorTracker: NSViewRepresentable {
    var report: (NSView) -> Void
    init(view: Binding<NSView?>) {
        report = { view.wrappedValue = $0 }
    }
    init(report: @escaping (NSView) -> Void) { self.report = report }
    func makeNSView(context: Context) -> KRPassthroughView {
        let view = KRPassthroughView()
        DispatchQueue.main.async { report(view) }
        return view
    }
    func updateNSView(_ nsView: KRPassthroughView, context: Context) {
        DispatchQueue.main.async { self.report(nsView) }
    }
}
final class KRPassthroughView: NSView {
    // Never claims hits: the tracker is a pure geometry anchor layered behind
    // the control it measures.
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

/// Presents Kronello menus in borderless child panels; no system NSMenu is used.
@MainActor
public final class KRMenuPresenter: ObservableObject {
    @Published public private(set) var isPresented = false
    private var panels: [KRMenuPanel] = []
    private var monitor: Any?
    private var resignation: NSObjectProtocol?
    private var onDismiss: (() -> Void)?
    private var anchorView: NSView?
    public init() {}

    /// Anchors the menu below an AppKit view and constrains it to the visible screen.
    public func present(_ items: [KRMenuItem], anchoredTo anchor: NSView, theme: KRTheme, current: String? = nil,
                        onDismiss: (() -> Void)? = nil) {
        guard let window = anchor.window else { return }
        let rect = window.convertToScreen(anchor.convert(anchor.bounds, to: nil))
        present(items, screenRect: rect, in: window, theme: theme, anchor: anchor, current: current, onDismiss: onDismiss)
    }

    private func present(_ items: [KRMenuItem], screenRect: CGRect, in window: NSWindow, theme: KRTheme,
                         anchor: NSView? = nil, current: String?, onDismiss: (() -> Void)?) {
        dismiss(); self.onDismiss = onDismiss; self.anchorView = anchor
        show(items, rect: screenRect, parent: window, theme: theme, current: current, submenu: false)
        isPresented = true
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .keyDown]) { [weak self] event in
            guard let self else { return event }
            if event.type == .keyDown && event.keyCode == 53 { self.dismiss(); return nil }
            if event.type == .keyDown && event.keyCode == 123 && self.panels.count > 1 {
                let child = self.panels.removeLast(); child.parent?.removeChildWindow(child); child.close()
                self.panels.last?.makeKeyAndOrderFront(nil); return nil
            }
            if event.type != .keyDown {
                let point = NSEvent.mouseLocation
                // Pressing the anchor again toggles closed; swallow the event
                // so the button's own action cannot re-present the menu.
                if self.mouseDownOnAnchor() { self.dismiss(); return nil }
                // Only the visible card counts as inside; the transparent
                // padding strip exists for the shadow, not for hit testing.
                if !self.panels.contains(where: { self.cardRect($0).contains(point) }) { self.dismiss() }
            }
            return event
        }
        resignation = NotificationCenter.default.addObserver(forName: NSApplication.didResignActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.dismiss() }
        }
    }
    private func show(_ items: [KRMenuItem], rect: CGRect, parent: NSWindow, theme: KRTheme, current: String?, submenu: Bool) {
        guard let screen = parent.screen else { return }
        // Keep one submenu level; the spec forbids deeper cascades.
        if submenu { while panels.count > 1 { let old = panels.removeLast(); old.parent?.removeChildWindow(old); old.close() } }
        let panel = KRMenuPanel(contentRect: .zero, styleMask: [.borderless], backing: .buffered, defer: false)
        panel.isReleasedWhenClosed = false; panel.isOpaque = false; panel.backgroundColor = .clear
        panel.hasShadow = false; panel.appearance = theme.appearance; panel.level = .popUpMenu
        let pad = submenu
            ? NSEdgeInsets(top: KRSpace.space4, left: KRSpace.space1, bottom: KRSpace.space4, right: KRSpace.space4)
            : NSEdgeInsets(top: KRSpace.space1, left: KRSpace.space4, bottom: KRSpace.space4, right: KRSpace.space4)
        panel.cardInset = pad
        let menu = KRMenu(items, current: current, onDismiss: { [weak self] in self?.dismiss() }, onSubmenu: { [weak self, weak panel] item, row in
            guard !submenu, let panel, let window = row.window else { return }
            let rect = window.convertToScreen(row.convert(row.bounds, to: nil))
            self?.show(item.children, rect: rect, parent: panel, theme: theme, current: nil, submenu: true)
        }, onLeafHover: submenu ? nil : { [weak self] in self?.closeSubmenus() })
            .padding(EdgeInsets(top: pad.top, leading: pad.left, bottom: pad.bottom, trailing: pad.right)).krTheme(theme)
        let hosting = NSHostingView(rootView: menu)
        panel.contentView = hosting
        let size = hosting.fittingSize
        let visible = screen.visibleFrame
        var origin = CGPoint(x: submenu ? rect.maxX - pad.left : rect.minX - pad.left,
                             y: submenu ? rect.maxY - size.height + pad.top : rect.minY - size.height + pad.top)
        origin.x = min(max(origin.x, visible.minX), visible.maxX - size.width)
        origin.y = min(max(origin.y, visible.minY), visible.maxY - size.height)
        panel.setFrame(CGRect(origin: origin, size: size), display: false)
        parent.addChildWindow(panel, ordered: .above); panels.append(panel); panel.makeKeyAndOrderFront(nil)
    }
    /// Closes only submenu panels, leaving the root menu open.
    private func closeSubmenus() {
        while panels.count > 1 { let old = panels.removeLast(); old.parent?.removeChildWindow(old); old.close() }
    }
    private func cardRect(_ panel: KRMenuPanel) -> CGRect {
        let frame = panel.frame, inset = panel.cardInset
        return CGRect(x: frame.minX + inset.left, y: frame.minY + inset.bottom,
                      width: frame.width - inset.left - inset.right, height: frame.height - inset.top - inset.bottom)
    }
    /// Closes all child panels and removes event monitors.
    public func dismiss() {
        if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
        if let resignation { NotificationCenter.default.removeObserver(resignation); self.resignation = nil }
        for panel in panels.reversed() { panel.parent?.removeChildWindow(panel); panel.close() }
        panels.removeAll(); isPresented = false; anchorView = nil
        let callback = onDismiss; onDismiss = nil; callback?()
    }

    /// Clicks on the anchor itself must not dismiss: the button re-presents or
    /// toggles on mouse-up, which matches NSPopUpButton behaviour.
    private func mouseDownOnAnchor() -> Bool {
        guard let anchorView, let window = anchorView.window else { return false }
        return window.convertToScreen(anchorView.convert(anchorView.bounds, to: nil)).contains(NSEvent.mouseLocation)
    }
}

private final class KRMenuPanel: NSPanel {
    var cardInset = NSEdgeInsetsZero
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}
