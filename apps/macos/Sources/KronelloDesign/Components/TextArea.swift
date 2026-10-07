import AppKit
import SwiftUI

/// Multi-line native text editor sharing KRCommittedTextView's commit boundary:
/// drafts stay local, Return or focus loss commits at most once, Escape reverts,
/// and IME-marked text is never sent to a callback (ADR-0105). The document view
/// wraps words and scrolls vertically inside a fixed-height field.
struct KRCommittedTextArea: NSViewRepresentable {
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
    var onCancel: () -> Void = {}
    var onDraftChange: () -> Void = {}

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView(frame: .zero)
        scroll.drawsBackground = false; scroll.borderType = .noBorder
        scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        let view = KRCommittedTextView(frame: scroll.contentView.bounds)
        view.isRichText = false; view.importsGraphics = false
        view.setAccessibilityRole(.textField)
        view.drawsBackground = false; view.textContainerInset = .zero
        view.textContainer?.lineFragmentPadding = 0
        view.isVerticallyResizable = true; view.isHorizontallyResizable = false
        view.minSize = .zero
        view.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        view.textContainer?.containerSize = NSSize(width: scroll.contentSize.width, height: CGFloat.greatestFiniteMagnitude)
        view.textContainer?.widthTracksTextView = true
        view.autoresizingMask = [.width]
        scroll.documentView = view
        updateNSView(scroll, context: context)
        return scroll
    }
    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let view = scroll.documentView as? KRCommittedTextView else { return }
        view.onCommit = onCommit; view.onFocus = onFocus
        view.onCancel = onCancel; view.font = font
        view.onDraftChange = onDraftChange
        view.textColor = ink; view.insertionPointColor = ink
        view.placeholder = placeholder; view.placeholderColor = muted
        view.selectedTextAttributes = [.backgroundColor: selection]
        view.isEditable = enabled; view.isSelectable = enabled
        if !label.isEmpty { view.setAccessibilityLabel(label) }
        view.synchronize(value)
        view.needsDisplay = true
    }
}

/// A multi-line labeled string field whose local draft commits on Return or
/// focus loss; Escape reverts to the last committed value. Same commit
/// boundary, error and help treatment as KRTextField.
public struct KRTextArea: View {
    @Environment(\.krStaticRendering) private var staticRendering
    @Environment(\.krPalette) private var p
    @Environment(\.isEnabled) private var enabled
    @Binding private var value: String
    private let label: String
    private let placeholder: String
    private let lines: Int
    private let style: KRTextStyle
    private let help: String?
    private let error: KRDiagnostic?
    private let onEditingStart: () -> Void
    private let onCommit: (String) -> Void
    @State private var draftStarted = false
    @State private var submitted = false
    @State private var focused = false

    public init(_ label: String, value: Binding<String>, placeholder: String = "", lines: Int = 3,
                style: KRTextStyle = KRType.body, help: String? = nil, error: KRDiagnostic? = nil,
                onEditingStart: @escaping () -> Void = {}, onCommit: @escaping (String) -> Void = { _ in }) {
        self.label = label; _value = value; self.placeholder = placeholder; self.lines = max(1, lines)
        self.style = style; self.help = help; self.error = error
        self.onEditingStart = onEditingStart; self.onCommit = onCommit
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space1) {
            if !label.isEmpty { Text(label).krText(KRType.label).foregroundStyle(p.ink) }
            if staticRendering {
                Text(value.isEmpty ? placeholder : value).krText(style)
                    .foregroundStyle(value.isEmpty ? p.inkMuted : p.ink)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .modifier(KRTextFieldChrome(invalid: error != nil, focused: false, height: height))
                    .accessibilityLabel(label)
            } else {
                KRCommittedTextArea(value: value, label: label, placeholder: placeholder, muted: NSColor(p.inkMuted), ink: NSColor(p.ink), selection: NSColor(p.selection), enabled: enabled,
                    onCommit: { value = $0; submitted = true; onCommit($0); draftStarted = false },
                    onFocus: { focused = $0; if $0 { submitted = false; if !draftStarted { onEditingStart(); draftStarted = true } } },
                    font: style.nsFont(), onCancel: { draftStarted = false },
                    onDraftChange: { submitted = false; if !draftStarted { onEditingStart(); draftStarted = true } })
                    .frame(minHeight: height - KRSpace.space2 * 2)
                    .modifier(KRTextFieldChrome(invalid: error != nil && (!focused || submitted), focused: focused, height: height))
            }
            if let error, !focused || submitted { KRErrorLine(error) }
            else if let help { Text(help).krText(KRType.caption).foregroundStyle(p.inkMuted) }
        }
    }
    private var height: CGFloat { CGFloat(lines) * style.lineHeight + KRSpace.space2 * 2 }
}
