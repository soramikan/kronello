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
    /// User-environment workflow settings (shortcuts + workspace layout) shared
    /// by every project window. Stored in UserDefaults, never in the document.
    let workflow = WorkflowSettings()
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
            // GUI-012: the raster scratch directory routes to the in-process
            // worker via the process environment, so it must be set before
            // `NativeProjectTransport` opens the session (ADR-0033 user prefs).
            if preferences.scratchDirectory.isEmpty { unsetenv("KRONELLO_RASTER_CACHE_ROOT") }
            else { setenv("KRONELLO_RASTER_CACHE_ROOT", preferences.scratchDirectory, 1) }
            let transport = try NativeProjectTransport(path: path, worker: worker)
            let model = EditorModel(path: path, transport: transport, stateStore: store)
            if let manifest = ProcessInfo.processInfo.environment["KRONELLO_FONT_INPUTS"] {
                model.fonts = try await Task.detached { try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: manifest))) as? [[String: Any]] ?? [] }.value
            }
            do { try await model.start(newDocument: new ? EditorModel.newDocument(name: URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent) : nil) }
            catch { await model.close(); throw error }
            // GUI-012 playback defaults: session flags apply every open; the
            // loop flag only seeds projects with no persisted UI state.
            model.audioScrubEnabled = preferences.playbackScrub
            model.playbackMuted = preferences.playbackMuted
            if !model.restoredUIState { model.ui.looping = preferences.playbackLooping }
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
            return KRRecentProject(id: $0.path, name: $0.name, location: EditorWindow.displayLocation($0.path),
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
            TrafficLights.layout(window)
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
            // AppKit re-lays the title bar out on these events and would put the
            // window buttons back at the stock 28pt title-bar position.
            let relayout: [Notification.Name] = [NSWindow.didResizeNotification, NSWindow.didEndLiveResizeNotification,
                NSWindow.didExitFullScreenNotification, NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification,
                NSWindow.didChangeScreenNotification]
            layoutTokens = relayout.map { name in
                NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { notification in
                    guard let window = notification.object as? NSWindow else { return }
                    MainActor.assumeIsolated { TrafficLights.layout(window) }
                }
            }
        }
        private var layoutTokens: [NSObjectProtocol] = []
        deinit {
            if let token { NotificationCenter.default.removeObserver(token) }
            layoutTokens.forEach(NotificationCenter.default.removeObserver)
        }
    }
}

/// window.md: the OS window buttons sit inside the 44pt toolbar, vertically
/// centred, instead of on a separate 28pt title-bar strip above it.
@MainActor enum TrafficLights {
    static let leading: CGFloat = 14
    static func layout(_ window: NSWindow) {
        guard !window.styleMask.contains(.fullScreen),
              let close = window.standardWindowButton(.closeButton),
              let miniaturize = window.standardWindowButton(.miniaturizeButton),
              let zoom = window.standardWindowButton(.zoomButton),
              let container = close.superview?.superview else { return }
        let height = KRWindowMetrics.toolbar
        var frame = container.frame
        if frame.height != height || frame.minY != window.frame.height - height {
            frame.size.height = height; frame.origin.y = window.frame.height - height
            container.frame = frame
        }
        let spacing = miniaturize.frame.minX - close.frame.minX
        for (index, button) in [close, miniaturize, zoom].enumerated() {
            button.setFrameOrigin(NSPoint(x: leading + CGFloat(index) * spacing, y: ((height - button.frame.height) / 2).rounded()))
        }
    }
}

/// Empty toolbar area that moves the window like a title bar; a double-click
/// follows the user's "double-click a window's title bar" preference.
struct WindowDragArea: NSViewRepresentable {
    final class DragView: NSView {
        override var mouseDownCanMoveWindow: Bool { true }
        override func mouseDown(with event: NSEvent) {
            guard let window else { return }
            if event.clickCount == 2 {
                let action = UserDefaults.standard.string(forKey: "AppleActionOnDoubleClick") ?? "Maximize"
                if action == "Minimize" { window.performMiniaturize(nil) } else if action != "None" { window.performZoom(nil) }
                return
            }
            window.performDrag(with: event)
        }
    }
    func makeNSView(context: Context) -> NSView { DragView() }
    func updateNSView(_ view: NSView, context: Context) {}
}
