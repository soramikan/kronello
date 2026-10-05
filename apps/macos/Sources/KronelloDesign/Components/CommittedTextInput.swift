import AppKit
import SwiftUI

/// Native input-method boundary shared by project string fields and their checks.
/// Drafts stay local; marked text is never sent to a project callback.
public final class KRCommittedTextView: NSTextView {
    public var onCommit: (String) -> Void = { _ in }
    public var onFocus: (Bool) -> Void = { _ in }
    public var onCancel: () -> Void = {}
    public var onDraftChange: () -> Void = {}
    private var committed = ""
    private var inserting = false
    private var cancelling = false
    var placeholder = ""
    var placeholderColor = NSColor.placeholderTextColor

    public override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        if string.isEmpty && !placeholder.isEmpty {
            (placeholder as NSString).draw(at: .zero, withAttributes: [.font: font ?? KRType.body.nsFont(), .foregroundColor: placeholderColor])
        }
    }

    public func synchronize(_ value: String) {
        guard window?.firstResponder !== self, !hasMarkedText() else { return }
        committed = value; string = value
    }
    public func commitDraft() {
        guard !hasMarkedText(), !inserting, !cancelling, !string.utf8.elementsEqual(committed.utf8) else { return }
        committed = string; onCommit(string)
    }
    public func cancelDraft() {
        cancelling = true
        super.unmarkText()
        string = committed
        cancelling = false
        onCancel()
    }
    public override func insertText(_ insertString: Any, replacementRange: NSRange) {
        let wasMarked = hasMarkedText()
        inserting = true
        super.insertText(insertString, replacementRange: replacementRange)
        inserting = false
        if wasMarked { commitDraft() }
    }
    public override func didChangeText() {
        super.didChangeText()
        onDraftChange()
    }
    public override func unmarkText() {
        let wasMarked = hasMarkedText()
        super.unmarkText()
        if wasMarked { commitDraft() }
    }
    public override func doCommand(by selector: Selector) {
        switch selector {
        case #selector(NSResponder.cancelOperation(_:)): cancelDraft()
        case #selector(NSResponder.insertNewline(_:)): commitDraft()
        case #selector(NSResponder.insertTab(_:)): window?.selectNextKeyView(self)
        case #selector(NSResponder.insertBacktab(_:)): window?.selectPreviousKeyView(self)
        default: super.doCommand(by: selector)
        }
    }
    public override func becomeFirstResponder() -> Bool {
        let result = super.becomeFirstResponder()
        if result { onFocus(true) }
        return result
    }
    public override func resignFirstResponder() -> Bool {
        let result = super.resignFirstResponder()
        if result { commitDraft(); onFocus(false) }
        return result
    }
}

struct KRCommittedTextInput: NSViewRepresentable {
    let value: String
    let label: String
    let placeholder: String
    let muted: NSColor
    let ink: NSColor
    let selection: NSColor
    let enabled: Bool
    let onCommit: (String) -> Void
    let onFocus: (Bool) -> Void
    var font: NSFont = KRType.body.nsFont()
    var alignment: NSTextAlignment = .left
    var focusOnCreate = false
    var onCancel: () -> Void = {}
    var onDraftChange: () -> Void = {}

    func makeNSView(context: Context) -> KRCommittedTextView {
        let view = KRCommittedTextView(frame: .zero)
        view.isRichText = false; view.importsGraphics = false
        view.setAccessibilityRole(.textField)
        view.drawsBackground = false; view.textContainerInset = .zero
        view.textContainer?.lineFragmentPadding = 0
        view.isVerticallyResizable = false; view.isHorizontallyResizable = false
        view.textContainer?.maximumNumberOfLines = 1
        view.textContainer?.lineBreakMode = .byTruncatingTail
        updateNSView(view, context: context)
        if focusOnCreate {
            DispatchQueue.main.async { [weak view] in
                guard let view else { return }
                view.window?.makeFirstResponder(view); view.selectAll(nil)
            }
        }
        return view
    }
    func updateNSView(_ view: KRCommittedTextView, context: Context) {
        view.onCommit = onCommit; view.onFocus = onFocus
        view.onCancel = onCancel; view.font = font; view.alignment = alignment
        view.onDraftChange = onDraftChange
        view.textColor = ink; view.insertionPointColor = ink
        view.placeholder = placeholder; view.placeholderColor = muted
        view.selectedTextAttributes = [.backgroundColor: selection]
        view.isEditable = enabled; view.isSelectable = enabled
        view.setAccessibilityLabel(label)
        view.synchronize(value)
        view.needsDisplay = true
    }
}
