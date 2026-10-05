import SwiftUI

/// Screen dimensions from window.md / motion.md; not user document values.
public enum KRWindowMetrics {
    public static let width: CGFloat = 1440
    public static let height: CGFloat = 900
    public static let toolbar: CGFloat = 44
    public static let tools: CGFloat = 40
    public static let toolHeight: CGFloat = 32
    public static let safeBand: CGFloat = 32
    public static let left: CGFloat = 248
    public static let right: CGFloat = 304
    public static let bottom: CGFloat = 344
    public static let valuesExpanded: CGFloat = 376
    public static let valuesCollapsed: CGFloat = 216
    public static let welcomeWidth: CGFloat = 680
    public static let welcomeHeight: CGFloat = 420
    public static let handle: CGFloat = 7
    /// Axis fields (X / Y) without unit.
    public static let numberWidth: CGFloat = 64
    /// Single-value fields with their unit.
    public static let scalarWidth: CGFloat = 88
    /// Inspector setting controls (font, weight, alignment), as in the motion gallery.
    public static let settingWidth: CGFloat = 144
}

public struct KRRecentProject: Identifiable {
    public let id: String
    public let name: String
    public let location: String
    public let date: String
    public let missing: Bool
    public init(id: String, name: String, location: String, date: String, missing: Bool) {
        self.id = id; self.name = name; self.location = location; self.date = date; self.missing = missing
    }
}

public struct KRWelcome: View {
    @Environment(\.krPalette) private var p
    public let recent: [KRRecentProject]
    @Binding public var showAtLaunch: Bool
    public let onNew: () -> Void
    public let onOpen: () -> Void
    public let onRecent: (String) -> Void
    public init(recent: [KRRecentProject], showAtLaunch: Binding<Bool>, onNew: @escaping () -> Void,
                onOpen: @escaping () -> Void, onRecent: @escaping (String) -> Void) {
        self.recent = recent; _showAtLaunch = showAtLaunch; self.onNew = onNew; self.onOpen = onOpen; self.onRecent = onRecent
    }
    public var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: KRSpace.space4) {
                Text("Kronello").krText(.init(name: "welcome-brand", family: KRFontFamily.sans, size: 28, lineHeight: 36, weight: 600, tracking: 0))
                Text("いま、時間を編む。").krText(KRType.body).foregroundStyle(p.inkMuted)
                KRButton("新規プロジェクト…", icon: .filePlus, variant: .primary, action: onNew)
                KRButton("開く…", icon: .folderOpen, variant: .secondary, action: onOpen)
                Spacer(minLength: KRSpace.space4)
                KRCheckbox("起動時に表示", isOn: $showAtLaunch)
            }.padding(KRSpace.space4).frame(width: KRWindowMetrics.left)
            p.line.frame(width: 1)
            VStack(alignment: .leading, spacing: KRSpace.space2) {
                Text("最近のプロジェクト").krText(KRType.caption, weight: 600).foregroundStyle(p.inkMuted).padding(KRSpace.space3)
                ScrollView {
                    VStack(spacing: 0) {
                        ForEach(recent) { project in
                            KRRecentRow(project: project) { onRecent(project.id) }
                        }
                        if recent.isEmpty { KREmptyState(icon: .clapperboard, title: "まだプロジェクトがありません", message: "新規プロジェクトを作成するか、既存の作品を開いてください。") }
                    }
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
        }.frame(width: KRWindowMetrics.welcomeWidth, height: KRWindowMetrics.welcomeHeight)
            .background(p.surface100, in: RoundedRectangle(cornerRadius: KRRadius.radiusLg))
            .foregroundStyle(p.ink).krShadow(p.shadowPopover, cornerRadius: KRRadius.radiusLg)
    }
}

private struct KRRecentRow: View {
    @Environment(\.krPalette) var p
    @State private var hover = false
    let project: KRRecentProject
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(alignment: .top, spacing: KRSpace.space2) {
                KRIconView(project.missing ? .triangleAlert : .clapperboard).foregroundStyle(project.missing ? p.danger : p.inkMuted)
                VStack(alignment: .leading, spacing: KRSpace.space1) {
                    Text(project.name).krText(KRType.body, weight: 500).foregroundStyle(p.ink)
                    Text(project.missing ? "PROJECT_NOT_FOUND · 見つかりません · " + project.location : project.location)
                        .krText(KRType.caption).foregroundStyle(project.missing ? p.danger : p.inkMuted).lineLimit(2)
                }.frame(maxWidth: .infinity, alignment: .leading)
                Text(project.date).krText(KRType.caption).foregroundStyle(p.inkMuted)
            }.padding(KRSpace.space3).background(hover ? p.controlHover : p.surface100)
        }.buttonStyle(.plain).krControlFocusRing().onHover { hover = $0 }.accessibilityLabel(project.name)
    }
}
