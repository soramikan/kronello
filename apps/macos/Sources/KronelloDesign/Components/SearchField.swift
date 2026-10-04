import SwiftUI

/// A search input with the Lucide search icon and a live UI-only binding.
public struct KRSearchField: View {
    @Environment(\.krStaticRendering) private var staticRendering
    @Environment(\.krPalette) private var p
    @Environment(\.isEnabled) private var enabled
    @Binding private var text: String
    private let placeholder: String
    private let appearance: KRControlAppearance
    @FocusState private var focused: Bool
    public init(_ placeholder: String = "検索", text: Binding<String>, appearance: KRControlAppearance = .resting) {
        self.placeholder = placeholder; _text = text; self.appearance = appearance
    }
    public var body: some View {
        HStack(spacing: KRSpace.space1) {
            KRIconView(.search, size: 12).foregroundStyle(p.inkMuted)
            if staticRendering {
                Text(text.isEmpty ? placeholder : text).foregroundStyle(text.isEmpty ? p.inkMuted : p.ink)
                    .frame(maxWidth: .infinity, alignment: .leading).accessibilityLabel(placeholder)
            } else {
                TextField(placeholder, text: $text, prompt: Text(placeholder).foregroundStyle(p.inkMuted))
                    .textFieldStyle(.plain).focused($focused).accessibilityLabel(placeholder)
            }
        }.krText(KRType.body).foregroundStyle(p.ink).tint(p.selection)
            .padding(.leading, 6).padding(.trailing, KRSpace.space2).frame(height: KRSize.controlHeight)
            .background(p.surface200, in: RoundedRectangle(cornerRadius: KRRadius.radiusSm))
            .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusSm).strokeBorder(p.lineStrong, lineWidth: 1) }
            .krFocusRing(focused || appearance == .focused).opacity(enabled ? 1 : 0.45)
    }
}
