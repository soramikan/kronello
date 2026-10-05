import SwiftUI

/// The exclusive source of a displayed property value.
public enum KRPropertySource: String, CaseIterable, Sendable { case constant, curve, expression }

/// Previous, add/remove, and next keyframe controls; expression values show an fx badge.
public struct KRKeyframeNavigator: View {
    @Environment(\.krPalette) private var p
    public let source: KRPropertySource
    public let onKeyframe: Bool
    private let previous: (() -> Void)?
    private let toggle: () -> Void
    private let next: (() -> Void)?
    private let editingEnabled: Bool
    public init(source: KRPropertySource, onKeyframe: Bool = false, previous: (() -> Void)? = nil,
                toggle: @escaping () -> Void = {}, next: (() -> Void)? = nil, editingEnabled: Bool = true) {
        self.source = source; self.onKeyframe = onKeyframe; self.previous = previous; self.toggle = toggle; self.next = next
        self.editingEnabled = editingEnabled
    }
    public var body: some View {
        Group {
            if source == .expression {
                Text("fx").krText(KRMono.caption).foregroundStyle(p.inkMuted)
                    .padding(.horizontal, 3).padding(.vertical, 1)
                    .overlay { RoundedRectangle(cornerRadius: 2).strokeBorder(p.lineStrong, lineWidth: 1) }
                    .accessibilityLabel("式による値")
            } else {
                HStack(spacing: KRSpace.space1) {
                    arrow(right: false, action: previous).opacity(source == .curve ? 1 : 0)
                    KRKeyframeGlyph(.linear, hollow: source == .constant || !onKeyframe, on: source == .curve && onKeyframe,
                                    accessibilityLabel: onKeyframe ? "キーフレームを削除" : "キーフレームを追加", action: toggle)
                        .padding(.horizontal, -KRSpace.space1)
                        .disabled(!editingEnabled).help(editingEnabled ? "" : "キーフレーム編集は GUI-002 で追加します")
                    arrow(right: true, action: next).opacity(source == .curve ? 1 : 0)
                }
            }
        }.frame(width: 48)
    }
    private func arrow(right: Bool, action: (() -> Void)?) -> some View {
        KRNavigatorArrow(right: right, action: action).disabled(source != .curve || action == nil).opacity(action == nil ? 0 : 1)
    }
}

private struct KRNavigatorArrow: View {
    @Environment(\.krPalette) var p
    @State private var hover = false
    @FocusState private var focused: Bool
    let right: Bool
    let action: (() -> Void)?
    var body: some View {
        Button { action?() } label: {
            KRTriangle(right: right).fill(hover ? p.ink : p.inkMuted).frame(width: 7, height: 9).frame(width: 13, height: 18)
        }.buttonStyle(.plain).focused($focused).krFocusRing(focused, cornerRadius: 2).onHover { hover = $0 }
            .accessibilityLabel(right ? "次のキーフレーム" : "前のキーフレーム")
    }
}

struct KRTriangle: Shape {
    let right: Bool
    func path(in r: CGRect) -> Path {
        Path { p in
            p.move(to: CGPoint(x: right ? r.minX : r.maxX, y: r.minY))
            p.addLine(to: CGPoint(x: right ? r.maxX : r.minX, y: r.midY))
            p.addLine(to: CGPoint(x: right ? r.minX : r.maxX, y: r.maxY)); p.closeSubpath()
        }
    }
}
