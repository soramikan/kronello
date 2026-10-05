import AppKit
import Combine
import KronelloAppModel
import KronelloDesign
import SwiftUI
import UniformTypeIdentifiers

@MainActor final class AppController: ObservableObject {
    @Published var editor: EditorModel?
    @Published var preferences = AppPreferences()
    @Published var error: ServiceFailure?
    @Published var loading = false
    @Published private var missingRecent: Set<String> = []
    @Published private var recentDates: [String: Date] = [:]
    private var editorChanges: AnyCancellable?
    private var launched = false
    let store = UIStateStore()
    var theme: KRTheme { preferences.light ? .light : .dark }
    var worker: String? {
        let bundled = Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/kronello")
        if FileManager.default.isExecutableFile(atPath: bundled.path) { return bundled.path }
        let development = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent().appendingPathComponent("kronello")
        return FileManager.default.isExecutableFile(atPath: development.path) ? development.path : nil
    }
    init() { KRFonts.registerBundled(); NSApplication.shared.appearance = KRTheme.dark.appearance }
    func launch() async {
        guard !launched else { return }; launched = true
        do { preferences = try await store.preferences(); NSApp.appearance = theme.appearance }
        catch { self.error = .init(code: "GUI_STATE_ERROR", message: String(describing: error)) }
        await refreshRecents()
        let args = CommandLine.arguments.dropFirst()
        if let path = args.first(where: { $0.hasSuffix(".kronello") }) { await open(path) }
        else if !preferences.showWelcome, let recent = preferences.recent.first { await open(recent.path) }
    }
    func savePreferences() {
        NSApp.appearance = theme.appearance
        let preferences = preferences
        Task { do { try await store.savePreferences(preferences); await refreshRecents() } catch { self.error = .init(code: "GUI_STATE_ERROR", message: String(describing: error)) } }
    }
    func openPanel() {
        guard !loading else { return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "kronello") ?? .data]
        panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        panel.begin { response in
            guard response == .OK, let path = panel.url?.path else { return }
            Task { @MainActor in await self.open(path) }
        }
    }
    func newPanel() {
        guard !loading else { return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "kronello") ?? .data]
        panel.nameFieldStringValue = "Untitled.kronello"
        panel.begin { response in
            guard response == .OK, let path = panel.url?.path else { return }
            Task { @MainActor in await self.open(path, new: true) }
        }
    }
    func open(_ path: String, new: Bool = false) async {
        guard !loading else { return }; loading = true; defer { loading = false }
        do {
            let transport = try NativeProjectTransport(path: path, worker: worker)
            let model = EditorModel(path: path, transport: transport, stateStore: store)
            if let manifest = ProcessInfo.processInfo.environment["KRONELLO_FONT_INPUTS"] {
                model.fonts = try await Task.detached { try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: manifest))) as? [[String: Any]] ?? [] }.value
            }
            do { try await model.start(newDocument: new ? EditorModel.newDocument(name: URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent) : nil) }
            catch { await model.close(); throw error }
            await editor?.close(); editor = model
            editorChanges = model.objectWillChange.sink { [weak self] _ in self?.objectWillChange.send() }
            preferences.recent.removeAll { $0.path == path }
            preferences.recent.insert(.init(path: path, name: model.name), at: 0)
            preferences.recent = Array(preferences.recent.prefix(20)); savePreferences()
        } catch { self.error = (error as? ServiceFailure) ?? .init(code: "GUI_OPEN_ERROR", message: String(describing: error)) }
    }
    func closeProject() { Task { await editor?.close(); editor = nil; editorChanges = nil; await refreshRecents() } }
    private func refreshRecents() async {
        let paths = preferences.recent.map(\.path)
        let metadata = await Task.detached {
            var missing = Set<String>(), dates: [String: Date] = [:]
            for path in paths {
                if !FileManager.default.fileExists(atPath: path) { missing.insert(path); continue }
                dates[path] = try? URL(fileURLWithPath: path).resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate
            }
            return (missing, dates)
        }.value
        missingRecent = metadata.0; recentDates = metadata.1
    }
    var recent: [KRRecentProject] {
        preferences.recent.map {
            let formatter = RelativeDateTimeFormatter(); formatter.locale = Locale(identifier: "ja_JP")
            let stamp = recentDates[$0.path] ?? $0.opened
            let date = Date().timeIntervalSince(stamp) > 604800 ? stamp.formatted(date: .numeric, time: .omitted) : formatter.localizedString(for: stamp, relativeTo: Date())
            return KRRecentProject(id: $0.path, name: $0.name, location: URL(fileURLWithPath: $0.path).deletingLastPathComponent().path,
                date: date, missing: missingRecent.contains($0.path))
        }
    }
}

/// Only AppKit window chrome belongs here. Every content root carries the explicit app theme.
struct WindowSizing: NSViewRepresentable {
    let editing: Bool
    var onClose: () -> Void = {}
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSView { NSView() }
    func updateNSView(_ view: NSView, context: Context) {
        DispatchQueue.main.async {
            guard let window = view.window else { return }
            context.coordinator.watch(window, onClose: onClose)
            let minimum = CGSize(width: editing ? KRWindowMetrics.width : KRWindowMetrics.welcomeWidth,
                                 height: editing ? KRWindowMetrics.height : KRWindowMetrics.welcomeHeight)
            if window.contentMinSize != minimum {
                window.contentMinSize = minimum; window.setContentSize(minimum); window.center()
            }
            window.title = "Kronello"
        }
    }
    @MainActor final class Coordinator {
        private weak var window: NSWindow?
        private var token: NSObjectProtocol?
        private var onClose: () -> Void = {}
        func watch(_ window: NSWindow, onClose: @escaping () -> Void) {
            self.onClose = onClose
            guard self.window !== window else { return }
            if let token { NotificationCenter.default.removeObserver(token) }
            self.window = window
            token = NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: window, queue: .main) { [weak self] _ in
                guard let self else { return }
                Task { @MainActor in self.onClose() }
            }
        }
        deinit { if let token { NotificationCenter.default.removeObserver(token) } }
    }
}
