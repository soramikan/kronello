import Combine
import Foundation
import KronelloCore

/// IO-001 (ADR-0134): one external output route as reported by
/// `io.output.list`. Detection and activation are session transport state —
/// never document state.
public struct OutputDeviceEntry: Equatable, Sendable {
    public var kind: String
    public var name: String
    public var detected: Bool
    public var available: Bool
    public var active: Bool
    public var detail: String
}

/// IO-001: external monitor output selection/activation. Activation is an
/// explicit `io.output.enable` command on the shared API; this model only
/// holds which destination the caller picked plus the reported route states.
@MainActor public final class ExternalOutputModel: ObservableObject {
    private weak var model: EditorModel?
    /// Destinations the picker offers. Vendor SDK kinds stay selectable so
    /// their typed `UNSUPPORTED_FEATURE` boundary is observable in the UI.
    public static let kinds = ["ref_monitor", "syphon", "sdi", "ndi"]
    @Published public var destination = "ref_monitor"
    @Published public private(set) var devices: [OutputDeviceEntry] = []
    @Published public var failure: ServiceFailure?
    /// Installed by the app layer (AppKit): opens/closes the dedicated
    /// reference-monitor window on a non-main screen and attaches its
    /// CAMetalLayer to `PreviewSurface.refMonitor`. Absent on non-native
    /// transports, where enable reports the service's typed reject.
    public var refMonitorPresenter: ((_ open: Bool) async throws -> Void)?
    public init(model: EditorModel) {
        self.model = model
    }
    public var enabled: Bool { devices.first { $0.kind == destination }?.active ?? false }
    /// Refresh the route enumeration. Detection results are runtime probes;
    /// they are re-read here rather than cached.
    public func refresh() async {
        guard let model else { return }
        do {
            // io.output.* are projectless operations: `model.request` injects
            // `project`, which the strict request schema rejects.
            let result = try await model.transport.call(["operation": "io.output.list"])
            devices = result.objects("devices").map { device in
                OutputDeviceEntry(
                    kind: device.string("kind"), name: device.string("name"),
                    detected: device["detected"] as? Bool ?? false,
                    available: device["available"] as? Bool ?? false,
                    active: device["active"] as? Bool ?? false,
                    detail: device.string("detail"))
            }
        } catch { failure = model.serviceFailure(error) }
    }
    /// Explicit activation/deactivation of the selected route. A
    /// reference-monitor route opens its window+surface first (the FFI
    /// requires the attached layer before enabling), and every failure leaves
    /// the route closed with a typed error — never a silent no-op.
    public func setEnabled(_ enabled: Bool) {
        guard let model else { return }
        Task {
            do {
                if enabled, destination == "ref_monitor" {
                    try await refMonitorPresenter?(true)
                }
                _ = try await model.transport.call([
                    "operation": enabled ? "io.output.enable" : "io.output.disable",
                    "kind": destination])
                if !enabled, destination == "ref_monitor" {
                    try await refMonitorPresenter?(false)
                }
                failure = nil
            } catch {
                if destination == "ref_monitor" { try? await refMonitorPresenter?(false) }
                failure = model.serviceFailure(error)
            }
            await refresh()
        }
    }
}
