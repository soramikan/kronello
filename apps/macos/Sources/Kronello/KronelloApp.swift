import SwiftUI
import KronelloDesign
import KronelloAppModel

@main struct KronelloApp: App {
    @StateObject private var controller = AppController()
    @State private var settingsTab = "general"
    var body: some Scene {
        Window("Kronello", id: "main") {
            AppRoot(controller: controller).krTheme(controller.theme)
                .background(WindowSizing(editing: controller.editor != nil, onClose: controller.closeProject))
                .task { await controller.launch() }
                .onOpenURL { url in if url.isFileURL { Task { await controller.open(url.path) } } }
        }.windowStyle(.hiddenTitleBar).windowResizability(.contentMinSize)
            .defaultSize(width: KRWindowMetrics.welcomeWidth, height: KRWindowMetrics.welcomeHeight)
            .commands { KronelloCommands(controller: controller, workflow: controller.workflow) }
        Settings {
            KRPanel("設定") {
                VStack(alignment: .leading, spacing: KRSpace.space3) {
                    KRSegmentedControl([
                        .init("general", "一般"), .init("playback", "再生"),
                        .init("workspace", "ワークスペース"), .init("shortcuts", "ショートカット"),
                    ], selection: $settingsTab).fixedSize()
                    ScrollView {
                        VStack(alignment: .leading, spacing: KRSpace.space4) {
                            if settingsTab == "general" {
                                KRCheckbox("Light テーマ", isOn: $controller.preferences.light)
                                KRCheckbox("起動時に Welcome を表示", isOn: $controller.preferences.showWelcome)
                                Divider()
                                // GUI-012: scratch/cache location routes to the
                                // worker through KRONELLO_RASTER_CACHE_ROOT on
                                // the next project open.
                                Text("スクラッチ").krText(KRType.heading)
                                KRTextField("キャッシュフォルダ", value: $controller.preferences.scratchDirectory,
                                    placeholder: "デフォルト（~/Library/Caches/kronello）",
                                    help: "ラスタキャッシュの保存先。空欄で既定値。次回プロジェクトを開いたときから有効です。")
                                HStack(spacing: KRSpace.space2) {
                                    KRButton("フォルダを選択…", variant: .secondary) { chooseScratch() }
                                    if !controller.preferences.scratchDirectory.isEmpty {
                                        KRButton("既定に戻す", variant: .plain) { controller.preferences.scratchDirectory = "" }
                                    }
                                }
                            } else if settingsTab == "playback" {
                                // GUI-012: playback defaults for new sessions.
                                // Looping only seeds projects that have never
                                // written UI state; saved state always wins.
                                KRCheckbox("スクラブ時にオーディオを再生", isOn: $controller.preferences.playbackScrub)
                                KRCheckbox("モニター音量をミュートで開始", isOn: $controller.preferences.playbackMuted)
                                KRCheckbox("ループ再生を既定で有効", isOn: $controller.preferences.playbackLooping)
                                Text("スクラブとミュートはプロジェクトを開くたびに適用されます。ループは UI 状態が未保存のプロジェクトのみ初期値になります。")
                                    .krText(KRType.caption).foregroundStyle(.secondary)
                            } else if settingsTab == "workspace" {
                                LayoutSettings(workflow: controller.workflow)
                            } else {
                                ShortcutSettings(workflow: controller.workflow)
                            }
                        }.padding(.vertical, KRSpace.space2).frame(width: 460, alignment: .leading)
                    }
                }.padding(KRSpace.space4)
            }.frame(maxHeight: 720).fixedSize(horizontal: true, vertical: false).krTheme(controller.theme)
                .onChange(of: controller.preferences.light) { _, _ in controller.savePreferences() }
                .onChange(of: controller.preferences.showWelcome) { _, _ in controller.savePreferences() }
                .onChange(of: controller.preferences.playbackScrub) { _, _ in controller.savePreferences() }
                .onChange(of: controller.preferences.playbackMuted) { _, _ in controller.savePreferences() }
                .onChange(of: controller.preferences.playbackLooping) { _, _ in controller.savePreferences() }
                .onChange(of: controller.preferences.scratchDirectory) { _, _ in controller.savePreferences() }
        }
    }
    /// GUI-012: scratch-folder picker for the raster cache location.
    private func chooseScratch() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.allowsMultipleSelection = false
        panel.prompt = "キャッシュフォルダに設定"
        if panel.runModal() == .OK, let url = panel.url {
            controller.preferences.scratchDirectory = url.path
        }
    }
}

struct KronelloCommands: Commands {
    @Environment(\.openWindow) private var openWindow
    @ObservedObject var controller: AppController
    @ObservedObject var workflow: WorkflowSettings
    private func shortcut(_ action: ShortcutAction) -> (key: KeyEquivalent, modifiers: EventModifiers) {
        let binding = workflow.binding(for: action)
        return (binding.keyEquivalent, binding.eventModifiers)
    }
    var body: some Commands {
                CommandGroup(replacing: .newItem) {
                    let new = shortcut(.newProject), open = shortcut(.openProject), close = shortcut(.closeProject)
                    Button("新規プロジェクト…") { openWindow(id: "main"); controller.newPanel() }.keyboardShortcut(new.key, modifiers: new.modifiers)
                    Button("開く…") { openWindow(id: "main"); controller.openPanel() }.keyboardShortcut(open.key, modifiers: open.modifiers)
                    Button("プロジェクトを閉じる", action: controller.closeProject).keyboardShortcut(close.key, modifiers: close.modifiers).disabled(controller.editor == nil)
                }
                CommandGroup(replacing: .undoRedo) {
                    let undo = shortcut(.undo), redo = shortcut(.redo)
                    Button("取り消す") { Task { await controller.editor?.undo() } }.keyboardShortcut(undo.key, modifiers: undo.modifiers).disabled(controller.editor?.canUndo != true)
                    Button("やり直す") { Task { await controller.editor?.undo(redo: true) } }.keyboardShortcut(redo.key, modifiers: redo.modifiers).disabled(controller.editor?.canRedo != true)
                }
                // Extend the system View menu instead of adding a second one.
                CommandGroup(before: .toolbar) {
                    let pages: [(ShortcutAction, String, String)] = [(.pageEdit, "編集", "edit"), (.pageMotion, "モーション", "motion"),
                                                                   (.pageTemplate, "テンプレート", "template"), (.pageMedia, "メディア", "media"),
                                                                   (.pageExport, "書き出し", "export")]
                    ForEach(pages, id: \.2) { action, label, page in
                        let binding = workflow.binding(for: action)
                        Button(label) { controller.editor?.ui.page = page }
                            .keyboardShortcut(binding.keyEquivalent, modifiers: binding.eventModifiers)
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
                EditorWindow(model: editor, workflow: controller.workflow).id(ObjectIdentifier(editor))
            }
            else {
                KRWelcome(recent: controller.recent, showAtLaunch: $controller.preferences.showWelcome,
                    onNew: controller.newPanel, onOpen: controller.openPanel,
                    onRecent: { path in Task { await controller.open(path) } })
                    .disabled(controller.loading)
                    .onChange(of: controller.preferences.showWelcome) { _, _ in controller.savePreferences() }
            }
        }.sheet(item: $controller.error) { error in
            KRDialog("プロジェクトを開けません", body: error.message, code: error.code, detail: error.detailText.isEmpty ? nil : error.detailText,
                actions: [.init("ok", "OK", variant: .primary) { controller.error = nil }]).krTheme(controller.theme)
        }
    }
}
