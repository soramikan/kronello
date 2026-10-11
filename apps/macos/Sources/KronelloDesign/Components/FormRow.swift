import SwiftUI

/// export.md settings form: a fixed label column and controls aligned on one edge.
public struct KRFormRow<Control: View>: View {
    @Environment(\.krPalette) private var p
    private let label: String
    private let labelWidth: CGFloat
    private let control: Control
    public init(_ label: String, labelWidth: CGFloat = 92, @ViewBuilder control: () -> Control) {
        self.label = label; self.labelWidth = labelWidth; self.control = control()
    }
    public var body: some View {
        HStack(spacing: KRSpace.space2) {
            Text(label).krText(KRType.body).foregroundStyle(p.ink).lineLimit(1)
                .frame(width: labelWidth, alignment: .leading)
            control.frame(maxWidth: .infinity, alignment: .leading)
        }.frame(minHeight: KRSize.rowHeight)
    }
}

/// A small muted group label inside a panel (`プリセット`, `詳細`, `出力先`).
public struct KRFormSectionLabel: View {
    @Environment(\.krPalette) private var p
    private let title: String
    public init(_ title: String) { self.title = title }
    public var body: some View {
        Text(title).krText(KRType.caption, weight: 600).foregroundStyle(p.inkMuted)
            .padding(.top, KRSpace.space2).accessibilityAddTraits(.isHeader)
    }
}
