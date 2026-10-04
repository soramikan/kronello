import SwiftUI

/// Workspace-owned tool strip placement; floating coordinates are UI pixels, not project values.
public struct KRToolStripPlacement: Codable, Equatable, Sendable {
    public enum Location: Codable, Equatable, Sendable {
        case viewerLeft
        case viewerRight
        case floating(x: Double, y: Double)
    }
    public var location: Location
    public var collapsed: Bool
    public init(_ location: Location = .viewerLeft, collapsed: Bool = false) { self.location = location; self.collapsed = collapsed }
    public var isFloating: Bool { if case .floating = location { return true }; return false }
}

/// A tool option; a separator may precede it in the vertical strip.
public struct KRTool: Identifiable, Sendable {
    public var id: String
    public var icon: KRIcon
    public var name: String
    public var shortcut: String
    public var separatorBefore: Bool
    public init(_ id: String, icon: KRIcon, name: String, shortcut: String, separatorBefore: Bool = false) {
        self.id = id; self.icon = icon; self.name = name; self.shortcut = shortcut; self.separatorBefore = separatorBefore
    }
    /// The motion page's default tool set.
    public static let motion: [KRTool] = [
        .init("select", icon: .mousePointer2, name: "選択", shortcut: "V"),
        .init("hand", icon: .hand, name: "手のひら", shortcut: "H"),
        .init("zoom", icon: .zoomIn, name: "ズーム", shortcut: "Z"),
        .init("rectangle", icon: .square, name: "長方形", shortcut: "M", separatorBefore: true),
        .init("ellipse", icon: .circle, name: "楕円", shortcut: "E"),
        .init("pen", icon: .penTool, name: "ペン", shortcut: "P"),
        .init("text", icon: .type, name: "テキスト", shortcut: "T")
    ]
}

/// A docked, collapsed, or floating tool palette. The app owns workspace storage and docking decisions.
public struct KRToolStrip: View {
    @Environment(\.krPalette) private var p
    public let tools: [KRTool]
    @Binding private var selection: String
    @Binding private var placement: KRToolStripPlacement
    private let onSelect: (String) -> Void
    private let onMovePreview: (CGSize) -> Void
    private let onMoveCommit: (CGSize) -> Void
    public init(_ tools: [KRTool], selection: Binding<String>, placement: Binding<KRToolStripPlacement>,
                onSelect: @escaping (String) -> Void = { _ in }, onMovePreview: @escaping (CGSize) -> Void = { _ in },
                onMoveCommit: @escaping (CGSize) -> Void = { _ in }) {
        self.tools = tools; _selection = selection; _placement = placement
        self.onSelect = onSelect; self.onMovePreview = onMovePreview; self.onMoveCommit = onMoveCommit
    }
    public var body: some View {
        VStack(spacing: 2) {
            if !placement.collapsed {
                KRIconView(.gripHorizontal).foregroundStyle(p.inkMuted).padding(.vertical, 2)
                    .contentShape(Rectangle())
                    .gesture(DragGesture().onChanged { onMovePreview($0.translation) }.onEnded { onMoveCommit($0.translation) })
                    .accessibilityLabel("ツール列を移動")
            }
            KRToolCollapse(collapsed: $placement.collapsed)
            if !placement.collapsed {
                ForEach(tools) { tool in
                    if tool.separatorBefore { p.line.frame(width: 20, height: 1).padding(.vertical, KRSpace.space1) }
                    Toggle(isOn: Binding(get: { selection == tool.id }, set: { if $0 { selection = tool.id; onSelect(tool.id) } })) {
                        KRIconView(tool.icon, size: 16)
                    }.toggleStyle(KRToolRadioStyle(label: tool.name)).help("\(tool.name) (\(tool.shortcut))").accessibilityLabel(tool.name)
                }
            }
            if !placement.isFloating { Spacer(minLength: 0) }
        }.padding(.vertical, placement.collapsed ? 6 : KRSpace.space1)
            .padding(.horizontal, placement.isFloating && !placement.collapsed ? KRSpace.space1 : 0)
            .frame(width: placement.collapsed ? 14 : placement.isFloating ? 36 : 40)
            .background(placement.isFloating ? p.surface200 : p.surface100,
                        in: RoundedRectangle(cornerRadius: placement.isFloating ? KRRadius.radiusMd : 0))
            .overlay(alignment: placement.location == .viewerRight ? .leading : .trailing) {
                if !placement.isFloating { p.line.frame(width: 1) }
            }
            .modifier(KRToolElevation(floating: placement.isFloating))
    }
}

private struct KRToolCollapse: View {
    @Environment(\.krPalette) var p
    @Environment(\.isEnabled) var enabled
    @Binding var collapsed: Bool
    @State private var hover = false
    @FocusState private var focused: Bool
    var body: some View {
        Button { collapsed.toggle() } label: {
            KRIconView(collapsed ? .chevronsRight : .chevronsLeft, size: 12)
                .frame(width: collapsed ? 14 : 20, height: collapsed ? 40 : 16)
                .foregroundStyle(hover ? p.ink : p.inkMuted)
                .background(hover ? p.controlHover : .clear, in: RoundedRectangle(cornerRadius: KRRadius.radiusSm))
        }.buttonStyle(.plain).focused($focused).onHover { hover = $0 }.opacity(enabled ? 1 : 0.45)
            .krFocusRing(focused).accessibilityLabel(collapsed ? "ツール列を開く" : "ツール列を畳む")
    }
}

private struct KRToolElevation: ViewModifier {
    @Environment(\.krPalette) var p
    let floating: Bool
    @ViewBuilder func body(content: Content) -> some View {
        if floating { content.krShadow(p.shadowPopover, cornerRadius: KRRadius.radiusMd) } else { content }
    }
}

private struct KRToolRadioStyle: ToggleStyle {
    @Environment(\.krPalette) var p
    let label: String
    func makeBody(configuration: Configuration) -> some View {
        KRToolChoice(configuration: configuration, label: label)
    }
}

private struct KRToolChoice: View {
    @Environment(\.krPalette) var p
    @Environment(\.isEnabled) var enabled
    @State private var hover = false
    @FocusState private var focused: Bool
    let configuration: ToggleStyle.Configuration
    let label: String
    var body: some View {
        Button { configuration.isOn = true } label: {
            configuration.label.frame(width: 28, height: 28)
                .foregroundStyle(configuration.isOn ? p.selection : hover ? p.ink : p.inkMuted)
                .background(configuration.isOn ? p.selectionBg : hover ? p.controlHover : .clear,
                            in: RoundedRectangle(cornerRadius: KRRadius.radiusMd))
        }.buttonStyle(.plain).focused($focused).krFocusRing(focused, cornerRadius: KRRadius.radiusMd)
            .onHover { hover = $0 }.opacity(enabled ? 1 : 0.45)
            .accessibilityValue(configuration.isOn ? "選択中" : "")
            .accessibilityAddTraits(configuration.isOn ? .isSelected : [])
            .accessibilityRepresentation {
                KRNativeRadio(label: label, selected: configuration.isOn, enabled: enabled) { configuration.isOn = true }
            }
    }
}
