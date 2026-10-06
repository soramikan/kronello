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
    private let onSubmenu: ((KRMenuItem, CGRect) -> Void)?
    @State private var current: String?
    @State private var anchors: [String: CGRect] = [:]
    @FocusState private var focused: Bool
    /// Submenu callbacks receive the row's frame in SwiftUI global coordinates.
    public init(_ items: [KRMenuItem], current: String? = nil, onDismiss: @escaping () -> Void = {},
                onSubmenu: ((KRMenuItem, CGRect) -> Void)? = nil) {
        self.items = items; _current = State(initialValue: current); self.onDismiss = onDismiss; self.onSubmenu = onSubmenu
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
                                dismiss: onDismiss, submenu: onSubmenu, registerAnchor: { anchors[item.id] = $0 })
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
                if !item.children.isEmpty, let anchor = anchors[item.id] { onSubmenu?(item, anchor); return .handled }
                onDismiss(); item.action(); return .handled
            }
            .onKeyPress(.rightArrow) {
                guard let item = items.first(where: { $0.id == current && !$0.disabled }), !item.children.isEmpty,
                      let anchor = anchors[item.id] else { return .ignored }
                onSubmenu?(item, anchor); return .handled
            }
            .onKeyPress(.escape) { onDismiss(); return .handled }
            .accessibilityElement(children: .contain).accessibilityLabel("メニュー")
    }
}

private struct KRMenuEntry: View {
    @Environment(\.krPalette) var p
    @State private var anchor: CGRect = .zero
    let item: KRMenuItem
    let active: Bool
    let activate: () -> Void
    let dismiss: () -> Void
    let submenu: ((KRMenuItem, CGRect) -> Void)?
    let registerAnchor: (CGRect) -> Void
    var foreground: Color { active ? p.onSelection : item.destructive ? p.danger : p.ink }
    var auxiliary: Color { active ? p.onSelection : item.destructive ? p.danger : p.inkMuted }
    var body: some View {
        Button {
            if !item.children.isEmpty { submenu?(item, anchor) }
            else { dismiss(); item.action() }
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
                GeometryReader { geometry in
                    Color.clear.preference(key: KRMenuAnchorKey.self, value: geometry.frame(in: .global))
                }
            }.onPreferenceChange(KRMenuAnchorKey.self) { rect in anchor = rect; registerAnchor(rect) }
            .onHover { inside in if inside && !item.disabled { activate() } }
            .onKeyPress(.rightArrow) { if !item.children.isEmpty { submenu?(item, anchor); return .handled }; return .ignored }
            .accessibilityLabel(item.label).accessibilityValue(item.checked ? "選択中" : "")
    }
}

struct KRMenuAnchorKey: PreferenceKey {
    static let defaultValue: CGRect = .zero
    static func reduce(value: inout CGRect, nextValue: () -> CGRect) { value = nextValue() }
}

/// Presents Kronello menus in borderless child panels; no system NSMenu is used.
@MainActor
public final class KRMenuPresenter: ObservableObject {
    @Published public private(set) var isPresented = false
    private var panels: [NSPanel] = []
    private var monitor: Any?
    private var resignation: NSObjectProtocol?
    private var onDismiss: (() -> Void)?
    public init() {}

    /// Anchors the menu below an AppKit view and constrains it to the visible screen.
    public func present(_ items: [KRMenuItem], anchoredTo anchor: NSView, theme: KRTheme, current: String? = nil,
                        onDismiss: (() -> Void)? = nil) {
        guard let window = anchor.window else { return }
        let rect = window.convertToScreen(anchor.convert(anchor.bounds, to: nil))
        present(items, screenRect: rect, in: window, theme: theme, current: current, onDismiss: onDismiss)
    }

    /// Anchors below a SwiftUI global frame measured in the window's content view, with a top-left origin.
    public func present(_ items: [KRMenuItem], anchoredTo frame: CGRect, in window: NSWindow, theme: KRTheme,
                        current: String? = nil, onDismiss: (() -> Void)? = nil) {
        guard let content = window.contentView else { return }
        present(items, screenRect: Self.screenRect(frame, in: content), in: window,
                theme: theme, current: current, onDismiss: onDismiss)
    }

    private static func screenRect(_ frame: CGRect, in view: NSView) -> CGRect {
        guard let window = view.window else { return .zero }
        let local = view.isFlipped ? frame : CGRect(x: frame.minX, y: view.bounds.height - frame.maxY,
                                                   width: frame.width, height: frame.height)
        return window.convertToScreen(view.convert(local, to: nil))
    }

    private func present(_ items: [KRMenuItem], screenRect: CGRect, in window: NSWindow, theme: KRTheme,
                         current: String?, onDismiss: (() -> Void)?) {
        dismiss(); self.onDismiss = onDismiss
        show(items, rect: screenRect, parent: window, theme: theme, current: current, submenu: false)
        isPresented = true
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .keyDown]) { [weak self] event in
            guard let self else { return event }
            if event.type == .keyDown && event.keyCode == 53 { self.dismiss(); return nil }
            if event.type == .keyDown && event.keyCode == 123 && self.panels.count > 1 {
                let child = self.panels.removeLast(); child.parent?.removeChildWindow(child); child.close()
                self.panels.last?.makeKeyAndOrderFront(nil); return nil
            }
            if event.type != .keyDown && !self.panels.contains(where: { $0.frame.contains(NSEvent.mouseLocation) }) { self.dismiss() }
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
        let menu = KRMenu(items, current: current, onDismiss: { [weak self] in self?.dismiss() }, onSubmenu: { [weak self, weak panel] item, row in
            guard !submenu, let panel, let content = panel.contentView else { return }
            self?.show(item.children, rect: Self.screenRect(row, in: content), parent: panel, theme: theme, current: nil, submenu: true)
        }).padding(KRSpace.space4).krTheme(theme)
        let hosting = NSHostingView(rootView: menu)
        panel.contentView = hosting
        let size = hosting.fittingSize
        let visible = screen.visibleFrame
        var origin = CGPoint(x: submenu ? rect.maxX - KRSpace.space4 : rect.minX - KRSpace.space4,
                             y: submenu ? rect.maxY - size.height + KRSpace.space4 : rect.minY - size.height + KRSpace.space4)
        origin.x = min(max(origin.x, visible.minX), visible.maxX - size.width)
        origin.y = min(max(origin.y, visible.minY), visible.maxY - size.height)
        panel.setFrame(CGRect(origin: origin, size: size), display: false)
        parent.addChildWindow(panel, ordered: .above); panels.append(panel); panel.makeKeyAndOrderFront(nil)
    }
    /// Closes all child panels and removes event monitors.
    public func dismiss() {
        if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
        if let resignation { NotificationCenter.default.removeObserver(resignation); self.resignation = nil }
        for panel in panels.reversed() { panel.parent?.removeChildWindow(panel); panel.close() }
        panels.removeAll(); isPresented = false
        let callback = onDismiss; onDismiss = nil; callback?()
    }
}

private final class KRMenuPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}
