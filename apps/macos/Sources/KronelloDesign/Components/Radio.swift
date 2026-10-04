import AppKit
import SwiftUI

/// A mutually exclusive choice style; selecting the current option keeps it on.
public struct KRRadioStyle: ToggleStyle {
    public var appearance: KRControlAppearance
    public var accessibilityLabel: String
    public init(appearance: KRControlAppearance = .resting, accessibilityLabel: String = "選択肢") {
        self.appearance = appearance; self.accessibilityLabel = accessibilityLabel
    }
    public func makeBody(configuration: Configuration) -> some View {
        KRChoice(configuration: configuration, radio: true, mixed: false, appearance: appearance, radioLabel: accessibilityLabel)
    }
}

/// One radio option, keyed by a consumer-owned stable identifier.
public struct KRRadio: View {
    private let label: String
    private let id: String
    @Binding private var selection: String
    private let appearance: KRControlAppearance
    public init(_ label: String, id: String, selection: Binding<String>, appearance: KRControlAppearance = .resting) {
        self.label = label; self.id = id; _selection = selection; self.appearance = appearance
    }
    public var body: some View {
        Toggle(label, isOn: Binding(get: { selection == id }, set: { if $0 { selection = id } }))
            .toggleStyle(KRRadioStyle(appearance: appearance, accessibilityLabel: label))
    }
}

// This native radio exists only in the accessibility representation, never in the drawn UI.
struct KRNativeRadio: NSViewRepresentable {
    let label: String
    let selected: Bool
    let enabled: Bool
    let select: () -> Void
    func makeCoordinator() -> Coordinator { Coordinator(select: select) }
    func makeNSView(context: Context) -> NSButton {
        let button = NSButton(radioButtonWithTitle: label, target: context.coordinator, action: #selector(Coordinator.press))
        return button
    }
    func updateNSView(_ button: NSButton, context: Context) {
        button.title = label; button.state = selected ? .on : .off; button.isEnabled = enabled
        context.coordinator.select = select
    }
    final class Coordinator: NSObject {
        var select: () -> Void
        init(select: @escaping () -> Void) { self.select = select }
        @objc func press() { select() }
    }
}
