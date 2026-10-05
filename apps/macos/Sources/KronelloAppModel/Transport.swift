import Foundation
import KronelloCore

public struct ServiceFailure: Error, Identifiable {
    public let id = UUID()
    public let code: String
    public let message: String
    public let details: [String: Any]
    public init(code: String, message: String, details: [String: Any] = [:]) {
        self.code = code; self.message = message; self.details = details
    }
    public var detailText: String {
        guard let data = try? JSONSerialization.data(withJSONObject: details, options: [.prettyPrinted, .sortedKeys]) else { return message }
        return String(data: data, encoding: .utf8) ?? message
    }
}

@MainActor public protocol ProjectTransport: AnyObject {
    var notificationHandler: ((String, [String: Any]) -> Void)? { get set }
    func ready() async throws
    func call(_ request: [String: Any]) async throws -> [String: Any]
    func subscribe() async throws
    func poll() throws
    func close()
}

@MainActor public final class NativeProjectTransport: ProjectTransport {
    public let session: ProjectSession
    public var notificationHandler: ((String, [String: Any]) -> Void)?
    public init(path: String, worker: String?) throws {
        session = try ProjectSession(path: path, workerExecutable: worker)
        session.onNotification = { [weak self] note in
            guard let object = try? JSONSerialization.jsonObject(with: note.response) as? [String: Any] else { return }
            self?.notificationHandler?(note.name, object)
        }
    }
    public static func request(_ object: [String: Any]) throws -> API.Request {
        try JSONDecoder().decode(API.Request.self, from: JSONSerialization.data(withJSONObject: object))
    }
    public static func result(_ object: [String: Any]) throws -> [String: Any] {
        if object["status"] as? String == "error", let error = object["error"] as? [String: Any] {
            throw ServiceFailure(code: error["code"] as? String ?? "INVALID_RESPONSE", message: error["message"] as? String ?? "", details: error["details"] as? [String: Any] ?? [:])
        }
        guard let result = object["result"] as? [String: Any], let value = result["value"] as? [String: Any] else {
            throw ServiceFailure(code: "INVALID_RESPONSE", message: "応答を読み取れません")
        }
        return value
    }
    public func ready() async throws {
        let response = try await session.ready()
        _ = try Self.result(JSONSerialization.jsonObject(with: JSONEncoder().encode(response)) as! [String: Any])
    }
    public func call(_ request: [String: Any]) async throws -> [String: Any] {
        // Validate the same generated envelope. Raw response avoids re-encoding opaque document numbers.
        _ = try Self.request(request)
        let response = try await session.rawCall(JSONSerialization.data(withJSONObject: request))
        return try Self.result(JSONSerialization.jsonObject(with: response) as! [String: Any])
    }
    public func subscribe() async throws { try await session.subscribe() }
    public func poll() throws { try session.poll() }
    public func close() { session.close() }
}

extension Dictionary where Key == String, Value == Any {
    public func object(_ key: String) -> [String: Any] { self[key] as? [String: Any] ?? [:] }
    public func objects(_ key: String) -> [[String: Any]] { self[key] as? [[String: Any]] ?? [] }
    public func string(_ key: String) -> String {
        if let string = self[key] as? String { return string }
        return (self[key] as? NSNumber)?.stringValue ?? ""
    }
    public func number(_ key: String) -> Double { (self[key] as? NSNumber)?.doubleValue ?? 0 }
}
