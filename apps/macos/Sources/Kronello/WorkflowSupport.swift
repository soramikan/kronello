import AppKit
import SwiftUI
import KronelloAppModel
import KronelloDesign

extension KeyBinding {
    /// SwiftUI key for `.keyboardShortcut` arguments and `KeyPress` comparison.
    var keyEquivalent: KeyEquivalent {
        switch key {
        case "space": return .space
        case "return": return .return
        case "tab": return .tab
        case "escape": return .escape
        case "delete": return .delete
        case "delete_forward": return .deleteForward
        case "left": return .leftArrow
        case "right": return .rightArrow
        case "up": return .upArrow
        case "down": return .downArrow
        case "home": return .home
        case "end": return .end
        case "page_up": return .pageUp
        case "page_down": return .pageDown
        default: return key.count == 1 ? KeyEquivalent(Character(key)) : .clear
        }
    }
    var eventModifiers: EventModifiers {
        var result: EventModifiers = []
        for name in modifiers {
            switch name {
            case "command": result.insert(.command)
            case "shift": result.insert(.shift)
            case "option": result.insert(.option)
            case "control": result.insert(.control)
            default: break
            }
        }
        return result
    }
    /// Exact-match check for `onKeyPress` handlers. Character keys compare
    /// against `press.characters` so Shift+m still resolves to binding "m".
    func matches(_ press: KeyPress) -> Bool {
        let flags = press.modifiers.intersection([.command, .shift, .option, .control])
        guard flags == eventModifiers else { return false }
        if key.count == 1, press.characters.lowercased() == key { return true }
        return press.key == keyEquivalent
    }
    /// Compact display label ("⌘⇧Z" style) for the settings table.
    var display: String {
        let symbols = ["control": "⌃", "option": "⌥", "shift": "⇧", "command": "⌘"]
        var text = modifiers.compactMap { symbols[$0] }.joined()
        switch key {
        case "space": text += "Space"
        case "delete": text += "⌫"
        case "delete_forward": text += "⌦"
        case "escape": text += "Esc"
        case "return": text += "↩"
        case "tab": text += "⇥"
        case "left": text += "←"
        case "right": text += "→"
        case "up": text += "↑"
        case "down": text += "↓"
        case "home": text += "↖"
        case "end": text += "↘"
        case "page_up": text += "⇞"
        case "page_down": text += "⇟"
        default: text += key.uppercased()
        }
        return text
    }
    /// Captured AppKit key event → binding. `nil` for Escape (cancel) or keys
    /// that carry no usable character/special name.
    init?(event: NSEvent) {
        let special: [UInt16: String] = [
            36: "return", 48: "tab", 49: "space", 51: "delete", 53: "escape",
            115: "home", 116: "page_up", 117: "delete_forward", 119: "end",
            121: "page_down", 123: "left", 124: "right", 125: "down", 126: "up",
        ]
        let named = special[event.keyCode]
        guard let key = named ?? event.charactersIgnoringModifiers?.lowercased(),
              named != nil || key.count == 1, !key.isEmpty else { return nil }
        var names: [String] = []
        if event.modifierFlags.contains(.control) { names.append("control") }
        if event.modifierFlags.contains(.option) { names.append("option") }
        if event.modifierFlags.contains(.shift) { names.append("shift") }
        if event.modifierFlags.contains(.command) { names.append("command") }
        self.init(key: key, modifiers: names)
    }
}

/// Records the next keyDown in this window as a binding. Escape cancels.
/// A local monitor is used because `onKeyPress` needs focus, which a row in a
/// Settings window does not reliably hold.
@MainActor final class ShortcutCapture: ObservableObject {
    @Published var target: ShortcutAction?
    private var monitor: Any?
    var assign: (ShortcutAction, KeyBinding) -> Void = { _, _ in }
    func begin(_ action: ShortcutAction) {
        stop()
        target = action
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, let action = self.target else { return event }
            if event.keyCode != 53, let binding = KeyBinding(event: event) {
                self.assign(action, binding)
            }
            self.stop()
            return nil
        }
    }
    func stop() {
        if let installed = monitor { NSEvent.removeMonitor(installed); monitor = nil }
        target = nil
    }
    deinit { if let monitor { NSEvent.removeMonitor(monitor) } }
}

/// FLOW-001 ショートカット設定: action → KeyBinding overrides in UserDefaults.
struct ShortcutSettings: View {
    @ObservedObject var workflow: WorkflowSettings
    @StateObject private var capture = ShortcutCapture()
    @Environment(\.krPalette) private var p
    private var groups: [String] {
        var order: [String] = []
        for action in ShortcutAction.allCases where !order.contains(action.group) { order.append(action.group) }
        return order
    }
    var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space2) {
            ForEach(groups, id: \.self) { group in
                Text(group).krText(KRType.label).foregroundStyle(p.inkMuted).padding(.top, KRSpace.space2)
                ForEach(ShortcutAction.allCases.filter { $0.group == group }, id: \.rawValue) { action in row(action) }
            }
            HStack {
                if capture.target != nil { Text("割り当てるキーを押してください（Esc で中止）").krText(KRType.caption).foregroundStyle(p.inkMuted) }
                Spacer()
                KRButton("デフォルトに戻す", variant: .secondary) { workflow.resetShortcuts() }
            }.padding(.top, KRSpace.space2)
        }.onAppear { capture.assign = { workflow.setBinding($1, for: $0) } }
    }
    func row(_ action: ShortcutAction) -> some View {
        HStack(spacing: KRSpace.space2) {
            Text(action.title).krText(KRType.body)
            Spacer(minLength: 0)
            Text(workflow.binding(for: action).display).krText(KRType.timecode).foregroundStyle(p.inkMuted)
            KRButton(capture.target == action ? "入力待ち…" : "割当", variant: capture.target == action ? .primary : .secondary) {
                if capture.target == action { capture.stop() } else { capture.begin(action) }
            }
        }.frame(minHeight: KRSize.rowHeight)
    }
}

/// FLOW-001 ワークスペース設定: per-page panel visibility and sizes stored in
/// UserDefaults under `kronello.pageLayouts`.
struct LayoutSettings: View {
    @ObservedObject var workflow: WorkflowSettings
    @State private var page = "motion"
    var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space2) {
            KRSegmentedControl([.init("motion", "モーション"), .init("edit", "編集")], selection: $page).fixedSize()
            PanelEditor(workflow: workflow, page: page)
            HStack {
                Spacer()
                KRButton("このページを初期値に戻す", variant: .secondary) { workflow.resetLayout(for: page) }
            }
        }
    }
}

private struct PanelEditor: View {
    @ObservedObject var workflow: WorkflowSettings
    let page: String
    var body: some View {
        let layout = workflow.layout(for: page)
        VStack(alignment: .leading, spacing: KRSpace.space2) {
            KRCheckbox("左パネル", isOn: flag(\.leadingPanel))
            if layout.leadingPanel { width("左パネル幅", \.leadingWidth) }
            KRCheckbox("右パネル", isOn: flag(\.trailingPanel))
            if layout.trailingPanel { width("右パネル幅", \.trailingWidth) }
            KRCheckbox("下パネル", isOn: flag(\.bottomPanel))
            if layout.bottomPanel { width("下パネル高さ", \.bottomHeight) }
        }
    }
    func flag(_ keyPath: WritableKeyPath<PageLayout, Bool>) -> Binding<Bool> {
        Binding(get: { workflow.layout(for: page)[keyPath: keyPath] },
                set: { value in var layout = workflow.layout(for: page); layout[keyPath: keyPath] = value; workflow.setLayout(layout, for: page) })
    }
    func width(_ label: String, _ keyPath: WritableKeyPath<PageLayout, Double>) -> some View {
        KRPopoverRow(label) {
            KRNumberField(value: Binding(get: { workflow.layout(for: page)[keyPath: keyPath] },
                                         set: { value in var layout = workflow.layout(for: page); layout[keyPath: keyPath] = value; workflow.setLayout(layout, for: page) }),
                          step: 8, range: 120 ... 800, precision: 0, accessibilityLabel: label) { _, _ in }
        }
    }
}

/// FLOW-001 Undo 履歴パネル: read-only `history.list` rows + selective undo
/// through `edit.undo` with `event_id`.
struct HistoryPanel: View {
    @ObservedObject var model: EditorModel
    @Environment(\.krPalette) private var p
    @Environment(\.dismiss) private var dismiss
    private let clock: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss"
        return formatter
    }()
    var body: some View {
        KRPanel("履歴", actions: {
            // GUI-012: the undo/redo stacks are session state; the buttons and
            // the "現在位置" marker make the position inside the list explicit.
            KRButton("取り消す", icon: .undo2, variant: .secondary) { Task { await model.undo() } }
                .disabled(!model.canUndo)
            KRButton("やり直す", icon: .redo2, variant: .secondary) { Task { await model.undo(redo: true) } }
                .disabled(!model.canRedo)
            KRButton(icon: .x, accessibilityLabel: "閉じる") { dismiss() }
        }) {
            if model.historyPanel.isEmpty {
                KREmptyState(icon: .history, title: "履歴なし", message: "このセッションでまだ変更は記録されていません。")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(model.historyPanel) { entry in row(entry) }
                    }
                }
            }
        }.frame(width: 480, height: 440)
            .task { do { try await model.loadHistory() } catch { model.mapFailure(error) } }
    }
    /// Latest applied (non-undone) row — the position undo/redo acts around.
    private var currentEntryID: String? {
        model.historyPanel.last { !$0.undone }?.id
    }
    func row(_ entry: HistoryEntry) -> some View {
        HStack(spacing: KRSpace.space2) {
            KRIconView(entry.isUndo ? .undo2 : .history, size: 12).foregroundStyle(entry.undone ? p.inkMuted : p.ink)
            VStack(alignment: .leading, spacing: 0) {
                Text(entry.label).krText(KRType.body).foregroundStyle(entry.undone ? p.inkMuted : p.ink).lineLimit(1)
                Text(meta(entry)).krText(KRType.caption).foregroundStyle(p.inkMuted).lineLimit(1)
            }
            Spacer(minLength: 0)
            if entry.undone {
                Text("取消済み").krText(KRType.caption).foregroundStyle(p.inkMuted)
            } else {
                if entry.id == currentEntryID {
                    Text("現在位置").krText(KRType.caption).foregroundStyle(p.accentInk)
                }
                KRButton("取り消す", variant: .secondary) { Task { await model.undoEvent(entry.id) } }
                    .disabled(model.busy)
            }
        }.frame(minHeight: KRSize.rowHeight).padding(.horizontal, KRSpace.space2)
            .overlay(alignment: .bottom) { p.line.frame(height: 1) }
    }
    func meta(_ entry: HistoryEntry) -> String {
        var parts = ["rev " + entry.revision]
        if entry.recordedAt != .distantPast { parts.append(clock.string(from: entry.recordedAt)) }
        if !entry.sessionID.isEmpty { parts.append(entry.own ? "このセッション" : String(entry.sessionID.prefix(8))) }
        return parts.joined(separator: " · ")
    }
}
