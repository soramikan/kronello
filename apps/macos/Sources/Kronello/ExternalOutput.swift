import AppKit
import QuartzCore
import SwiftUI
import KronelloCore
import KronelloAppModel
import KronelloDesign

/// IO-001 (ADR-0134): dedicated fullscreen reference-monitor window on a
/// non-main display. The window owns only AppKit state — a borderless
/// NSWindow covering one NSScreen plus the CAMetalLayer the FFI binds to the
/// program preview's device. The layer takes the target display's color
/// space so presentation matches the screen.
@MainActor final class RefMonitorWindow {
    private var window: NSWindow?
    private var view: MetalView?
    private var size: CGSize = .zero
    private var backingObserver: NSObjectProtocol?
    var isOpen: Bool { window != nil }

    /// Open the window on the first non-main screen and attach the surface.
    /// Called before `io.output.enable`; a failure leaves nothing open.
    func open(session: ProjectSession) async throws {
        if window != nil { return }
        guard let screen = NSScreen.screens.first(where: { $0 != NSScreen.main }) else {
            throw ServiceFailure(code: "DEVICE_UNAVAILABLE",
                                 message: "外部ディスプレイが見つかりません（リファレンスモニタ出力にはメイン以外の画面が必要です）")
        }
        let view = MetalView(frame: NSRect(origin: .zero, size: screen.frame.size))
        // Target display color space: the presentation transform encodes
        // SDR sRGB and Core Animation resolves to this screen's space.
        view.metal.colorspace = screen.colorSpace?.cgColorSpace
        let window = NSWindow(contentRect: screen.frame, styleMask: .borderless,
                              backing: .buffered, defer: false, screen: screen)
        window.isReleasedWhenClosed = false
        window.contentView = view
        window.orderFront(nil)
        let scale = screen.backingScaleFactor
        let size = CGSize(width: screen.frame.width * scale, height: screen.frame.height * scale)
        view.metal.drawableSize = size
        do {
            try await session.attach(metalLayer: Unmanaged.passUnretained(view.metal).toOpaque(),
                                     width: UInt32(size.width), height: UInt32(size.height),
                                     surface: PreviewSurface.refMonitor)
        } catch {
            window.orderOut(nil)
            window.close()
            throw error
        }
        self.window = window
        self.view = view
        self.size = size
        // Re-apply drawable size if the screen's backing scale changes.
        backingObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.didChangeBackingPropertiesNotification,
            object: window, queue: .main) { [weak self, weak view] _ in
            MainActor.assumeIsolated {
                guard let self, let view, let screen = self.window?.screen else { return }
                let scale = screen.backingScaleFactor
                let size = CGSize(width: screen.frame.width * scale, height: screen.frame.height * scale)
                guard size != self.size else { return }
                self.size = size
                view.metal.drawableSize = size
                Task { try? await session.resize(width: UInt32(size.width), height: UInt32(size.height),
                                                 surface: PreviewSurface.refMonitor) }
            }
        }
    }

    func close() {
        if let backingObserver { NotificationCenter.default.removeObserver(backingObserver) }
        backingObserver = nil
        window?.orderOut(nil)
        window?.close()
        window = nil
        view = nil
    }
}

/// IO-001: minimal external-output controls beside the program monitor —
/// destination picker plus an explicit enable toggle. State lives in
/// `ExternalOutputModel`; activation is `io.output.*` on the shared API.
struct ExternalOutputControls: View {
    @ObservedObject var model: EditorModel
    @ObservedObject var output: ExternalOutputModel
    @State private var refMonitor = RefMonitorWindow()
    @Environment(\.krPalette) var p

    private var selected: OutputDeviceEntry? {
        output.devices.first { $0.kind == output.destination }
    }
    var body: some View {
        HStack(spacing: KRSpace.space2) {
            KRPopupButton("外部出力先",
                          options: output.devices.map { device in
                              KRPopupOption(device.kind, device.name,
                                            disabled: !device.detected && !device.available)
                          },
                          selection: $output.destination)
            KRButton(icon: .film, accessibilityLabel: "外部出力",
                     pressed: output.enabled) {
                output.setEnabled(!output.enabled)
            }
            .help(selected?.detail ?? "")
        }
        .accessibilityElement(children: .contain)
        .task {
            installPresenter()
            await output.refresh()
        }
        .alert(item: $output.failure) { failure in
            Alert(title: Text("外部出力"), message: Text(failure.message))
        }
    }
    private func installPresenter() {
        guard output.refMonitorPresenter == nil else { return }
        output.refMonitorPresenter = { open in
            guard let session = (model.transport as? NativeProjectTransport)?.session else {
                throw ServiceFailure(code: "UNSUPPORTED_FEATURE",
                                     message: "外部出力にはネイティブセッションが必要です")
            }
            if open { try await refMonitor.open(session: session) }
            else { refMonitor.close() }
        }
    }
}
