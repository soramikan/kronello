import Foundation
import CKronelloFFI

public enum NativeError: Error, CustomStringConvertible {
    case rejected, closed, timeout, service(String, String), detailed(String, String, [String: Any])
    public var description: String {
        switch self {
        case .rejected: return "FFI input rejected or queue full"
        case .closed: return "FFI session closed"
        case .timeout: return "FFI response timed out"
        case .service(let code, let message), .detailed(let code, let message, _): return "\(code): \(message)"
        }
    }
}
public struct Notification: Sendable {
    public let name: String
    public let response: Data
}
private struct Message: Decodable {
    let request_id: UInt64?
    let notification: String?
    let response_json: String
}
/// Main-actor transport wrapper; heavy work runs on Rust's dedicated worker.
/// Callers can await without blocking AppKit. Every request has an explicit path.
@MainActor public final class ProjectSession {
    private var handle: UInt64
    private var completed: [UInt64: Data] = [:]
    private var abandoned: Set<UInt64> = []
    public var onNotification: ((Notification) -> Void)?
    public init(path: String, workerExecutable: String? = nil) throws {
        let pathBytes = Data(path.utf8)
        let workerBytes = Data((workerExecutable ?? "").utf8)
        handle = pathBytes.withUnsafeBytes { p in
            workerBytes.withUnsafeBytes { w in
                kronello_open(p.bindMemory(to: UInt8.self).baseAddress, p.count,
                    workerExecutable == nil ? nil : w.bindMemory(to: UInt8.self).baseAddress, w.count)
            }
        }
        if handle == 0 { throw NativeError.rejected }
    }
    deinit { kronello_close(handle) }
    public func close() {
        kronello_close(handle)
        handle = 0
        completed.removeAll()
        abandoned.removeAll()
    }
    public func ready() async throws -> API.Response {
        try decodeResponse(await completion(0))
    }
    public func call(_ request: API.Request) async throws -> API.Response {
        let data = try JSONEncoder().encode(request)
        return try decodeResponse(await rawCall(data))
    }
    /// Raw bytes preserve duplicate-key validation in the shared Rust decoder.
    public func rawCall(_ data: Data) async throws -> Data {
        let id = data.withUnsafeBytes {
            kronello_call(handle, $0.bindMemory(to: UInt8.self).baseAddress, $0.count)
        }
        return try await acceptedCompletion(id)
    }
    public func subscribe(_ enabled: Bool = true) async throws {
        try checkStatus(await acceptedCompletion(kronello_subscribe(handle, enabled)))
    }
    /// Poll on an AppKit timer, or use calls/awaits (which also drain notifications).
    public func poll() throws {
        guard handle != 0 else { throw NativeError.closed }
        while let ptr = kronello_poll(handle) {
            let data = Data(String(cString: ptr).utf8)
            kronello_free(ptr)
            let message = try JSONDecoder().decode(Message.self, from: data)
            let response = Data(message.response_json.utf8)
            if let id = message.request_id, abandoned.remove(id) == nil { completed[id] = response }
            if let name = message.notification {
                onNotification?(Notification(name: name, response: response))
            }
        }
    }
    /// GUI-011: surfaces are slotted so Source and Program monitors share this
    /// one session (and its exclusive store handle). Slot 0 is the program
    /// monitor; higher slots are additional preview surfaces.
    public func attach(metalLayer: UnsafeMutableRawPointer, width: UInt32, height: UInt32, surface: UInt32 = 0) async throws {
        try checkStatus(await acceptedCompletion(kronello_surface_attach_at(handle, surface, metalLayer, width, height)))
    }
    public func resize(width: UInt32, height: UInt32, surface: UInt32 = 0) async throws {
        try checkStatus(await acceptedCompletion(kronello_surface_resize_at(handle, surface, width, height)))
    }
    public func redraw(_ request: API.Request, surface: UInt32 = 0) async throws -> JSONValue {
        let data = try JSONEncoder().encode(request)
        let id = data.withUnsafeBytes {
            kronello_surface_redraw_at(handle, surface, $0.bindMemory(to: UInt8.self).baseAddress, $0.count)
        }
        let response = try await acceptedCompletion(id)
        try checkStatus(response)
        return try JSONDecoder().decode(JSONValue.self, from: response)
    }
    private func acceptedCompletion(_ id: UInt64) async throws -> Data {
        guard handle != 0 else { throw NativeError.closed }
        guard id != 0 else { throw NativeError.rejected }
        return try await completion(id)
    }
    private func completion(_ id: UInt64) async throws -> Data {
        guard handle != 0 else { throw NativeError.closed }
        let deadline = ContinuousClock.now + .seconds(30)
        do {
            while ContinuousClock.now < deadline {
                try Task.checkCancellation()
                try poll()
                if let response = completed.removeValue(forKey: id) {
                    return response
                }
                try await Task.sleep(for: .milliseconds(2))
            }
            throw NativeError.timeout
        } catch {
            if completed.removeValue(forKey: id) == nil { abandoned.insert(id) }
            throw error
        }
    }
    private func decodeResponse(_ data: Data) throws -> API.Response {
        try JSONDecoder().decode(API.Response.self, from: data)
    }
    private func checkStatus(_ data: Data) throws {
        let value = try JSONDecoder().decode(JSONValue.self, from: data)
        if case .object(let object) = value, object["status"] == .string("success") { return }
        if case .object(let object) = value, case .object(let error) = object["error"],
           case .string(let code) = error["code"], case .string(let message) = error["message"] {
            if let raw = error["details"],
               let data = try? JSONEncoder().encode(raw),
               let details = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                throw NativeError.detailed(code, message, details)
            }
            throw NativeError.service(code, message)
        }
        throw NativeError.rejected
    }
}

/// GUI-011: well-known preview surface slots on a shared `ProjectSession`.
/// Slot 0 is the legacy program preview; extra slots keep each monitor's
/// retained CAMetalLayer independent on the same serialized worker.
public enum PreviewSurface {
    public static let program: UInt32 = 0
    public static let source: UInt32 = 1
    public static let export: UInt32 = 2
    /// IO-001 (ADR-0134): external reference-monitor surface. The FFI worker
    /// binds it to the program preview's device so one program frame fans out
    /// to every enabled output without cross-device copies.
    public static let refMonitor: UInt32 = 8
}

/// GUI-011: a source-monitor preview target (`SourcePreviewRef` on the wire).
/// The ref keeps full identity (asset / stream / multicam angle) so preview
/// scheduling never derives identity from a display name or array position.
public enum SourcePreview: Equatable, Sendable {
    case composition(String)
    case asset(String, Int)
    case multicam(String, String)

    /// Whole-asset previews use stream 0 by convention; the shared service
    /// decides whether that stream can be previewed (typed errors otherwise).
    public static func asset(_ id: String) -> SourcePreview { .asset(id, 0) }

    public var wire: [String: Any] {
        switch self {
        case .composition(let id): return ["kind": "composition", "composition": id]
        case .asset(let id, let stream): return ["kind": "asset", "asset": id, "stream_index": stream]
        case .multicam(let multicam, let angle):
            return ["kind": "multicam", "multicam": multicam, "angle": angle]
        }
    }
    /// Canonical preview-identity key (stable ids only, never display names).
    public var key: String {
        switch self {
        case .composition(let id): return "composition:" + id
        case .asset(let id, let stream): return "asset:" + id + ":" + String(stream)
        case .multicam(let multicam, let angle): return "multicam:" + multicam + ":" + angle
        }
    }
    /// Narrow a clip `source_ref` to its previewable form, matching
    /// `SourcePreviewRef::from_source_ref` — generator, caption and
    /// adjustment sources have no monitor preview and return nil.
    public static func fromSourceRef(_ ref: [String: Any]) -> SourcePreview? {
        switch ref["kind"] as? String {
        case "composition":
            return (ref["composition"] as? String).map { .composition($0) }
        case "asset":
            guard let asset = ref["asset"] as? String else { return nil }
            return .asset(asset, (ref["stream_index"] as? NSNumber)?.intValue ?? 0)
        case "multicam":
            guard let multicam = ref["multicam"] as? String, let angle = ref["angle"] as? String else { return nil }
            return .multicam(multicam, angle)
        default:
            return nil
        }
    }
}
