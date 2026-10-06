import SwiftUI
import KronelloDesign
import KronelloAppModel

@main struct KronelloApp: App {
    @StateObject private var controller = AppController()
    var body: some Scene {
        Window("Kronello", id: "main") {
            AppRoot(controller: controller).krTheme(controller.theme)
                .background(WindowSizing(editing: controller.editor != nil, onClose: controller.closeProject))
                .task { await controller.launch() }
                .onOpenURL { url in if url.isFileURL { Task { await controller.open(url.path) } } }
        }.windowStyle(.hiddenTitleBar).windowResizability(.contentMinSize)
            .defaultSize(width: KRWindowMetrics.welcomeWidth, height: KRWindowMetrics.welcomeHeight)
            .commands { KronelloCommands(controller: controller) }
        Settings {
            KRPanel("設定") { VStack(alignment: .leading, spacing: KRSpace.space4) {
                KRCheckbox("Light テーマ", isOn: $controller.preferences.light)
                KRCheckbox("起動時に Welcome を表示", isOn: $controller.preferences.showWelcome)
            }.padding(KRSpace.space4).frame(width: 360, alignment: .leading) }.fixedSize().krTheme(controller.theme)
                .onChange(of: controller.preferences.light) { _, _ in controller.savePreferences() }
                .onChange(of: controller.preferences.showWelcome) { _, _ in controller.savePreferences() }
        }
    }
}

struct KronelloCommands: Commands {
    @Environment(\.openWindow) private var openWindow
    @ObservedObject var controller: AppController
    var body: some Commands {
                CommandGroup(replacing: .newItem) {
                    Button("新規プロジェクト…") { openWindow(id: "main"); controller.newPanel() }.keyboardShortcut("n")
                    Button("開く…") { openWindow(id: "main"); controller.openPanel() }.keyboardShortcut("o")
                    Button("プロジェクトを閉じる", action: controller.closeProject).keyboardShortcut("w").disabled(controller.editor == nil)
                }
                CommandGroup(replacing: .undoRedo) {
                    Button("取り消す") { Task { await controller.editor?.undo() } }.keyboardShortcut("z").disabled(controller.editor?.canUndo != true)
                    Button("やり直す") { Task { await controller.editor?.undo(redo: true) } }.keyboardShortcut("z", modifiers: [.command, .shift]).disabled(controller.editor?.canRedo != true)
                }
                // Extend the system View menu instead of adding a second one.
                CommandGroup(before: .toolbar) {
                    ForEach(Array(["編集", "モーション", "テンプレート", "書き出し"].enumerated()), id: \.offset) { index, label in
                        Button(label) { controller.editor?.ui.page = ["edit", "motion", "template", "export"][index] }
                            .keyboardShortcut(KeyEquivalent(Character(String(index + 1))))
                            .disabled(controller.editor == nil)
                    }
                    Divider()
                    Toggle("Light テーマ", isOn: $controller.preferences.light).onChange(of: controller.preferences.light) { _, _ in controller.savePreferences() }
                    Divider()
                }
    }
}

struct AppRoot: View {
    @ObservedObject var controller: AppController
    var body: some View {
        Group {
            if let editor = controller.editor {
                // Native preview coordinators and page StateObjects belong to
                // this project session, even when a second file reuses the window.
                EditorWindow(model: editor).id(ObjectIdentifier(editor))
            }
            else {
                KRWelcome(recent: controller.recent, showAtLaunch: $controller.preferences.showWelcome,
                    onNew: controller.newPanel, onOpen: controller.openPanel,
                    onRecent: { path in Task { await controller.open(path) } })
                    .disabled(controller.loading)
                    .onChange(of: controller.preferences.showWelcome) { _, _ in controller.savePreferences() }
            }
        }.sheet(item: $controller.error) { error in
            KRDialog("プロジェクトを開けません", body: error.message, code: error.code, detail: error.detailText,
                actions: [.init("ok", "OK", variant: .primary) { controller.error = nil }]).krTheme(controller.theme)
        }
    }
}
