import AppKit
import SwiftUI

/// A pure, testable numeric edit transaction. Finishing a transaction yields at most one commit.
public struct KRNumberEdit: Equatable, Sendable {
    public enum Phase: Equatable, Sendable { case idle, armed, scrubbing, editing }
    public private(set) var phase: Phase = .idle
    public private(set) var origin: Double
    public private(set) var candidate: Double
    public let step: Double
    public let range: ClosedRange<Double>?

    public init(value: Double, step: Double = 1, range: ClosedRange<Double>? = nil) {
        precondition(value.isFinite && step.isFinite && step > 0)
        self.origin = value; candidate = value; self.step = step; self.range = range
    }
    public static func multiplier(shift: Bool, option: Bool) -> Double {
        (shift ? 10 : 1) * (option ? 0.1 : 1)
    }
    public func clamped(_ value: Double) -> Double {
        guard value.isFinite else { return candidate }
        guard let range else { return value }
        return min(range.upperBound, max(range.lowerBound, value))
    }
    public mutating func beginDrag() { guard phase == .idle else { return }; phase = .armed }
    /// Horizontal movement below 3px remains a click.
    @discardableResult
    public mutating func drag(horizontal: Double, shift: Bool = false, option: Bool = false) -> Double? {
        guard phase == .armed || phase == .scrubbing else { return nil }
        if phase == .armed && abs(horizontal) < 3 { return nil }
        phase = .scrubbing
        candidate = clamped(origin + horizontal * step * Self.multiplier(shift: shift, option: option))
        return candidate
    }
    public mutating func beginEditing() { phase = .editing }
    @discardableResult
    public mutating func type(_ text: String) -> Bool {
        guard phase == .editing, let value = Double(text), value.isFinite else { return false }
        candidate = clamped(value); return true
    }
    /// Returns nil for a click, unchanged value, or an already finished transaction.
    public mutating func finish() -> (from: Double, to: Double)? {
        guard phase != .idle else { return nil }
        let shouldCommit = phase == .scrubbing || phase == .editing
        phase = .idle
        guard shouldCommit && candidate != origin else { return nil }
        let result = (from: origin, to: candidate); origin = candidate; return result
    }
    public mutating func cancel() { candidate = origin; phase = .idle }
    public func stepped(_ direction: Int, shift: Bool = false, option: Bool = false) -> Double {
        clamped(candidate + Double(direction) * step * Self.multiplier(shift: shift, option: option))
    }
}

/// A numeric field with preview-only scrubbing, direct editing, and one commit on release.
public struct KRNumberField: View {
    @Environment(\.krStaticRendering) private var staticRendering
    @Environment(\.krPalette) private var p
    @Environment(\.isEnabled) private var enabled
    @Binding private var value: Double
    private let unit: String
    private let label: String
    private let step: Double
    private let range: ClosedRange<Double>?
    private let precision: Int
    private let error: Bool
    private let appearance: KRNumberFieldState
    private let onEditingStart: () -> Void
    private let onPreview: (Double) -> Void
    private let onCommit: (Double, Double) -> Void
    @State private var transaction: KRNumberEdit?
    @State private var text = ""
    @State private var hover = false
    @State private var cursorPushed = false
    @State private var editing = false
    @State private var inputFocused = false
    @State private var invalid = false
    @State private var dragCancelled = false
    @FocusState private var focused: Bool

    public init(value: Binding<Double>, unit: String = "", step: Double = 1, range: ClosedRange<Double>? = nil,
                precision: Int = 1, error: Bool = false, state: KRNumberFieldState = .resting,
                accessibilityLabel: String, onEditingStart: @escaping () -> Void = {}, onPreview: @escaping (Double) -> Void = { _ in },
                onCommit: @escaping (Double, Double) -> Void) {
        _value = value; self.unit = unit; self.step = step; self.range = range
        self.precision = max(0, min(precision, 12)); self.error = error; appearance = state
        label = accessibilityLabel; self.onEditingStart = onEditingStart; self.onPreview = onPreview; self.onCommit = onCommit
    }
    private var scrubbing: Bool { transaction?.phase == .scrubbing || appearance == .scrubbing }
    private var failed: Bool { error || invalid || appearance == .error }
    private func format(_ value: Double) -> String { String(format: "%.*f", precision, value) }
    public var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 2) {
            if editing && !staticRendering {
                KRCommittedTextInput(value: text, label: label, placeholder: "", muted: NSColor(p.inkMuted), ink: NSColor(p.ink), selection: NSColor(p.selection), enabled: enabled,
                    onCommit: { text = $0; commitText() }, onFocus: { inputFocused = $0; if !$0 && editing && !invalid { cancel() } },
                    font: KRType.timecode.nsFont(), alignment: .right, focusOnCreate: true, onCancel: cancel)
                    .frame(height: KRSize.controlHeight - 8)
            } else {
                Text(failed ? "—" : editing ? text : format(transaction?.candidate ?? value))
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
            if !unit.isEmpty { Text(unit).krText(KRType.caption).foregroundStyle(p.inkMuted) }
        }.krText(KRType.timecode).foregroundStyle(p.ink)
            .padding(.horizontal, KRSpace.space2).frame(minWidth: 64).frame(height: KRSize.controlHeight)
            .background(hover || scrubbing || appearance == .hover ? p.controlHover : p.surface200,
                        in: RoundedRectangle(cornerRadius: KRRadius.radiusSm))
            .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusSm)
                .strokeBorder(failed ? p.danger : scrubbing ? p.selection : p.lineStrong, lineWidth: 1) }
            .focusable(!editing && enabled && !error).focused($focused)
            .focusEffectDisabled().krFocusRing(focused || inputFocused || appearance == .editing || appearance == .focused)
            .opacity(enabled ? 1 : 0.45)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                guard enabled && !failed && !editing && !dragCancelled else { return }
                if transaction == nil {
                    onEditingStart()
                    transaction = KRNumberEdit(value: value, step: step, range: range); transaction?.beginDrag(); focused = true
                }
                let keys = NSEvent.modifierFlags
                if let candidate = transaction?.drag(horizontal: drag.translation.width, shift: keys.contains(.shift), option: keys.contains(.option)) {
                    onPreview(candidate)
                }
            }.onEnded { _ in
                if dragCancelled { dragCancelled = false; return }
                guard enabled && !failed && !editing else { return }
                if transaction?.phase == .scrubbing { finish() } else { beginEditing() }
            }, including: editing ? .subviews : .all)
            .onKeyPress(keys: [.upArrow, .rightArrow, .downArrow, .leftArrow]) { press in
                guard !editing && enabled && !failed else { return .ignored }
                let model = KRNumberEdit(value: value, step: step, range: range)
                let next = model.stepped(press.key == .upArrow || press.key == .rightArrow ? 1 : -1,
                                         shift: press.modifiers.contains(.shift), option: press.modifiers.contains(.option))
                if next != value { onEditingStart(); let previous = value; value = next; onCommit(previous, next) }
                return .handled
            }
            .onKeyPress(.return) { guard !editing && enabled && !failed else { return .ignored }; beginEditing(); return .handled }
            .onKeyPress(.escape) {
                guard transaction != nil else { return .ignored }
                dragCancelled = transaction?.phase == .armed || transaction?.phase == .scrubbing
                cancel(); return .handled
            }
            .onHover { inside in hover = inside; updateCursor() }
            .onChange(of: editing) { _, _ in updateCursor() }
            .onChange(of: enabled) { _, _ in updateCursor() }
            .onDisappear { if cursorPushed { NSCursor.pop(); cursorPushed = false } }
            .accessibilityElement(children: editing ? .contain : .ignore)
            .accessibilityLabel(label).accessibilityValue(failed ? "エラー" : "\(format(value)) \(unit)")
            .accessibilityAction(.default) {
                guard enabled && !failed && !editing else { return }
                beginEditing()
            }
            .accessibilityAdjustableAction { direction in
                guard enabled && !failed else { return }
                let next = KRNumberEdit(value: value, step: step, range: range).stepped(direction == .increment ? 1 : -1)
                if next != value { onEditingStart(); let previous = value; value = next; onCommit(previous, next) }
            }
    }
    private func beginEditing() {
        if transaction == nil { onEditingStart() }
        transaction = KRNumberEdit(value: value, step: step, range: range); transaction?.beginEditing()
        text = format(value); focused = false; editing = true
    }
    private func updateCursor() {
        let needsCursor = hover && enabled && !editing && !failed
        if needsCursor && !cursorPushed { NSCursor.resizeLeftRight.push(); cursorPushed = true }
        else if !needsCursor && cursorPushed { NSCursor.pop(); cursorPushed = false }
    }
    private func commitText() {
        guard editing else { return }
        guard transaction?.type(text) == true else { invalid = true; return }
        editing = false; invalid = false; finish()
    }
    private func finish() {
        if let commit = transaction?.finish() { value = commit.to; onCommit(commit.from, commit.to) }
        transaction = nil
    }
    private func cancel() {
        transaction?.cancel(); transaction = nil; editing = false; invalid = false; focused = false; inputFocused = false
        onPreview(value)
    }
}

/// Numeric field appearances available to a static review gallery.
public enum KRNumberFieldState: String, CaseIterable, Sendable { case resting, hover, scrubbing, editing, focused, error }
