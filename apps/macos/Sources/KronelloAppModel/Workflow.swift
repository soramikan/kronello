import Combine
import Foundation

/// One keyboard binding: a key identifier plus modifier flag names.
/// `key` is a stable name ("space", "left", "home", "delete", "escape", ...)
/// or a single lowercase character ("z", "1"). Modifier names are kept in
/// canonical display order so bindings compare equal regardless of input order.
public struct KeyBinding: Codable, Equatable, Sendable {
    public var key: String
    public var modifiers: [String]
    public init(key: String, modifiers: [String] = []) {
        self.key = key.lowercased()
        var seen = Set<String>()
        self.modifiers = Self.modifierOrder.filter { name in
            modifiers.contains(where: { $0.lowercased() == name }) && seen.insert(name).inserted
        }
    }
    /// Canonical modifier order used for storage and display (⌃⌥⇧⌘).
    public static let modifierOrder = ["control", "option", "shift", "command"]
}

/// Stable action identifiers whose bindings live in `UserDefaults` under
/// `kronello.shortcuts` as an action → KeyBinding dictionary (ADR-0121).
/// Unknown actions keep their default; a stored override equal to the default
/// is dropped so the persisted map stays minimal.
public enum ShortcutAction: String, CaseIterable, Codable, Sendable {
    case newProject = "file.new"
    case openProject = "file.open"
    case closeProject = "file.close"
    case undo = "edit.undo"
    case redo = "edit.redo"
    case pageEdit = "page.edit"
    case pageMotion = "page.motion"
    case pageTemplate = "page.template"
    case pageExport = "page.export"
    case transportPlay = "transport.play"
    case transportStepBack = "transport.step_back"
    case transportStepForward = "transport.step_forward"
    case transportGoStart = "transport.go_start"
    case transportGoEnd = "transport.go_end"
    case commonCancel = "common.cancel"
    case editDelete = "edit.delete"
    case editDeleteRipple = "edit.delete_ripple"
    case editMarker = "edit.marker"
    case editClipMarker = "edit.clip_marker"
    case editSetIn = "edit.set_in"
    case editSetOut = "edit.set_out"
    case editClearWorkArea = "edit.clear_work_area"
    case editJumpPrevious = "edit.jump_previous"
    case editJumpNext = "edit.jump_next"
    case editToolSelect = "edit.tool.select"
    case editToolBlade = "edit.tool.blade"
    case editToolSlip = "edit.tool.slip"
    case editToolSlide = "edit.tool.slide"
    case editToolRoll = "edit.tool.roll"
    case editToolHand = "edit.tool.hand"
    case motionToolSelect = "motion.tool.select"
    case motionToolHand = "motion.tool.hand"
    case motionToolZoom = "motion.tool.zoom"
    case motionToolRectangle = "motion.tool.rectangle"
    case motionToolEllipse = "motion.tool.ellipse"
    case motionToolPen = "motion.tool.pen"
    case motionToolText = "motion.tool.text"

    public var defaultBinding: KeyBinding {
        switch self {
        case .newProject: return KeyBinding(key: "n", modifiers: ["command"])
        case .openProject: return KeyBinding(key: "o", modifiers: ["command"])
        case .closeProject: return KeyBinding(key: "w", modifiers: ["command"])
        case .undo: return KeyBinding(key: "z", modifiers: ["command"])
        case .redo: return KeyBinding(key: "z", modifiers: ["shift", "command"])
        case .pageEdit: return KeyBinding(key: "1", modifiers: ["command"])
        case .pageMotion: return KeyBinding(key: "2", modifiers: ["command"])
        case .pageTemplate: return KeyBinding(key: "3", modifiers: ["command"])
        case .pageExport: return KeyBinding(key: "4", modifiers: ["command"])
        case .transportPlay: return KeyBinding(key: "space")
        case .transportStepBack: return KeyBinding(key: "left")
        case .transportStepForward: return KeyBinding(key: "right")
        case .transportGoStart: return KeyBinding(key: "home")
        case .transportGoEnd: return KeyBinding(key: "end")
        case .commonCancel: return KeyBinding(key: "escape")
        case .editDelete: return KeyBinding(key: "delete")
        case .editDeleteRipple: return KeyBinding(key: "delete", modifiers: ["option"])
        case .editMarker: return KeyBinding(key: "m")
        case .editClipMarker: return KeyBinding(key: "m", modifiers: ["shift"])
        case .editSetIn: return KeyBinding(key: "i")
        case .editSetOut: return KeyBinding(key: "o")
        case .editClearWorkArea: return KeyBinding(key: "x", modifiers: ["option"])
        case .editJumpPrevious: return KeyBinding(key: "up")
        case .editJumpNext: return KeyBinding(key: "down")
        case .editToolSelect: return KeyBinding(key: "v")
        case .editToolBlade: return KeyBinding(key: "b")
        case .editToolSlip: return KeyBinding(key: "y")
        case .editToolSlide: return KeyBinding(key: "u")
        case .editToolRoll: return KeyBinding(key: "n")
        case .editToolHand: return KeyBinding(key: "h")
        case .motionToolSelect: return KeyBinding(key: "v")
        case .motionToolHand: return KeyBinding(key: "h")
        case .motionToolZoom: return KeyBinding(key: "z")
        case .motionToolRectangle: return KeyBinding(key: "m")
        case .motionToolEllipse: return KeyBinding(key: "e")
        case .motionToolPen: return KeyBinding(key: "p")
        case .motionToolText: return KeyBinding(key: "t")
        }
    }
    /// Settings UI grouping. Order inside a group is the CaseIterable order.
    public var group: String {
        switch self {
        case .newProject, .openProject, .closeProject: return "ファイル"
        case .undo, .redo: return "編集"
        case .pageEdit, .pageMotion, .pageTemplate, .pageExport: return "ページ"
        case .transportPlay, .transportStepBack, .transportStepForward, .transportGoStart, .transportGoEnd:
            return "再生"
        case .commonCancel: return "共通"
        case .editDelete, .editDeleteRipple, .editMarker, .editClipMarker, .editSetIn, .editSetOut,
             .editClearWorkArea, .editJumpPrevious, .editJumpNext:
            return "タイムライン"
        case .editToolSelect, .editToolBlade, .editToolSlip, .editToolSlide, .editToolRoll, .editToolHand:
            return "編集ツール"
        case .motionToolSelect, .motionToolHand, .motionToolZoom, .motionToolRectangle,
             .motionToolEllipse, .motionToolPen, .motionToolText:
            return "モーションツール"
        }
    }
    public var title: String {
        switch self {
        case .newProject: return "新規プロジェクト"
        case .openProject: return "開く"
        case .closeProject: return "プロジェクトを閉じる"
        case .undo: return "取り消す"
        case .redo: return "やり直す"
        case .pageEdit: return "編集ページ"
        case .pageMotion: return "モーションページ"
        case .pageTemplate: return "テンプレートページ"
        case .pageExport: return "書き出しページ"
        case .transportPlay: return "再生 / 停止"
        case .transportStepBack: return "1 フレーム戻る"
        case .transportStepForward: return "1 フレーム進む"
        case .transportGoStart: return "先頭へ"
        case .transportGoEnd: return "末尾へ"
        case .commonCancel: return "キャンセル"
        case .editDelete: return "クリップを削除"
        case .editDeleteRipple: return "クリップをリップル削除"
        case .editMarker: return "シーケンスマーカー"
        case .editClipMarker: return "クリップマーカー"
        case .editSetIn: return "In 点を設定"
        case .editSetOut: return "Out 点を設定"
        case .editClearWorkArea: return "In/Out を解除"
        case .editJumpPrevious: return "前の編集点へ"
        case .editJumpNext: return "次の編集点へ"
        case .editToolSelect: return "選択ツール"
        case .editToolBlade: return "ブレード"
        case .editToolSlip: return "スリップ"
        case .editToolSlide: return "スライド"
        case .editToolRoll: return "ロール"
        case .editToolHand: return "手のひら"
        case .motionToolSelect: return "選択ツール"
        case .motionToolHand: return "手のひら"
        case .motionToolZoom: return "ズーム"
        case .motionToolRectangle: return "矩形"
        case .motionToolEllipse: return "楕円"
        case .motionToolPen: return "ペン"
        case .motionToolText: return "テキスト"
        }
    }
}

/// Per-page panel state persisted in `UserDefaults` under `kronello.pageLayouts`
/// as a page → layout dictionary (ADR-0121). Widths/heights are points.
public struct PageLayout: Codable, Equatable, Sendable {
    public var leadingPanel: Bool
    public var trailingPanel: Bool
    public var bottomPanel: Bool
    public var leadingWidth: Double
    public var trailingWidth: Double
    public var bottomHeight: Double
    public init(leadingPanel: Bool = true, trailingPanel: Bool = true, bottomPanel: Bool = true,
                leadingWidth: Double, trailingWidth: Double, bottomHeight: Double) {
        self.leadingPanel = leadingPanel; self.trailingPanel = trailingPanel; self.bottomPanel = bottomPanel
        self.leadingWidth = leadingWidth; self.trailingWidth = trailingWidth; self.bottomHeight = bottomHeight
    }
    /// Panel geometry defaults per page id, matching the fixed layout values
    /// the pages used before FLOW-001 made them adjustable.
    public static func standard(for page: String) -> PageLayout {
        switch page {
        case "edit": return PageLayout(leadingWidth: 280, trailingWidth: 296, bottomHeight: 312)
        default: return PageLayout(leadingWidth: 248, trailingWidth: 304, bottomHeight: 344)
        }
    }
}

/// User-environment settings that must never enter the `.kronello` document:
/// keyboard shortcut overrides and per-page workspace layout (ADR-0121).
/// Reads tolerate corrupt or absent entries by falling back to defaults.
public final class WorkflowSettings: ObservableObject {
    public static let shortcutsKey = "kronello.shortcuts"
    public static let layoutsKey = "kronello.pageLayouts"
    /// Overrides only; `binding(for:)` falls back to the action default.
    @Published public private(set) var shortcuts: [String: KeyBinding] = [:]
    /// Only pages the user changed; `layout(for:)` falls back per page.
    @Published public private(set) var layouts: [String: PageLayout] = [:]
    private let defaults: UserDefaults
    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        shortcuts = Self.read([String: KeyBinding].self, at: Self.shortcutsKey, from: defaults) ?? [:]
        layouts = Self.read([String: PageLayout].self, at: Self.layoutsKey, from: defaults) ?? [:]
    }
    public func binding(for action: ShortcutAction) -> KeyBinding {
        shortcuts[action.rawValue] ?? action.defaultBinding
    }
    /// `nil` or a binding equal to the default removes the override.
    public func setBinding(_ binding: KeyBinding?, for action: ShortcutAction) {
        if let binding, binding != action.defaultBinding {
            shortcuts[action.rawValue] = binding
        } else {
            shortcuts.removeValue(forKey: action.rawValue)
        }
        persist()
    }
    public func resetShortcuts() {
        shortcuts = [:]
        persist()
    }
    public func layout(for page: String) -> PageLayout {
        layouts[page] ?? .standard(for: page)
    }
    public func setLayout(_ layout: PageLayout, for page: String) {
        layouts[page] = layout
        persist()
    }
    public func resetLayout(for page: String) {
        layouts.removeValue(forKey: page)
        persist()
    }
    private func persist() {
        Self.write(shortcuts, at: Self.shortcutsKey, to: defaults)
        Self.write(layouts, at: Self.layoutsKey, to: defaults)
    }
    private static func read<T: Decodable>(_ type: T.Type, at key: String, from defaults: UserDefaults) -> T? {
        guard let data = defaults.data(forKey: key) else { return nil }
        return try? JSONDecoder().decode(type, from: data)
    }
    private static func write(_ value: some Encodable, at key: String, to defaults: UserDefaults) {
        if let data = try? JSONEncoder().encode(value) { defaults.set(data, forKey: key) }
    }
}

/// One row of the undo history panel: a `history.list` entry plus session
/// metadata. `history.list` carries no wall-clock field, so `recordedAt` is
/// the first time this session observed the event — display-only state.
public struct HistoryEntry: Identifiable, Equatable, Sendable {
    public let id: String
    public let label: String
    public let revision: String
    public let sessionID: String
    public let undone: Bool
    /// `true` when the event itself is an undo (`undo_of` is set).
    public let isUndo: Bool
    public let own: Bool
    public let recordedAt: Date
    public init?(entry: [String: Any], labels: [String: String], ownSession: String, recordedAt: Date) {
        let event = entry.object("event")
        let id = event.string("id")
        guard !id.isEmpty else { return nil }
        self.id = id
        revision = event.string("revision")
        sessionID = event.string("session_id")
        undone = entry["undone"] as? Bool ?? false
        isUndo = event["undo_of"] != nil && !(event["undo_of"] is NSNull)
        own = sessionID == ownSession
        self.recordedAt = recordedAt
        if let label = labels[id] {
            self.label = label
        } else if isUndo {
            self.label = "取り消し"
        } else if let operation = event.objects("mutations").first?.string("operation"), !operation.isEmpty {
            self.label = operation.replacingOccurrences(of: "_", with: " ")
        } else {
            self.label = "変更"
        }
    }
}
