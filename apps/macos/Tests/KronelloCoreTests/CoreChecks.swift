import Foundation
import KronelloCore
import CKronelloFFI

private struct CheckFailure: Error { let message: String }
private func requireEqual<T: Equatable>(_ first: T, _ second: T, _ message: String = "") throws {
    guard first == second else { throw CheckFailure(message: "Expected \(first) == \(second). \(message)") }
}
private func requireTrue(_ value: Bool) throws {
    guard value else { throw CheckFailure(message: "Expected true") }
}
private func requireThrows<T>(_ expression: @autoclosure () throws -> T) throws {
    do { _ = try expression() } catch { return }
    throw CheckFailure(message: "Expected an error")
}


@MainActor struct CoreChecks {
    let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    func bytes(_ object: [String: Any]) throws -> Data { try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) }
    func request(_ object: [String: Any]) throws -> API.Request { try JSONDecoder().decode(API.Request.self, from: bytes(object)) }
    func object<T: Encodable>(_ value: T) throws -> [String: Any] {
        try JSONSerialization.jsonObject(with: JSONEncoder().encode(value)) as! [String: Any]
    }
    func value(_ response: [String: Any]) throws -> [String: Any] {
        try requireEqual(response["status"] as? String, "success", "\(response)")
        return (response["result"] as! [String: Any])["value"] as! [String: Any]
    }
    func cli(_ request: [String: Any]) throws -> [String: Any] {
        let process = Process()
        process.executableURL = root.appendingPathComponent("apps/macos/Libraries/kronello")
        let input = Pipe(), output = Pipe(), errors = Pipe()
        process.standardInput = input; process.standardOutput = output; process.standardError = errors
        try process.run()
        try input.fileHandleForWriting.write(contentsOf: bytes(request))
        try input.fileHandleForWriting.close()
        let result = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        try requireEqual(process.terminationStatus, 0, String(data: errors.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)!)
        return try JSONSerialization.jsonObject(with: result) as! [String: Any]
    }
    func verifySharedRevisionEventAndCLIIdempotency() async throws {
        let temporaryRoot = ProcessInfo.processInfo.environment["TMPDIR"].map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.temporaryDirectory
        let folder = temporaryRoot.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let path = folder.appendingPathComponent("shared.kronello").path
        let document = try JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("examples/ffi-preview.project.json")))
        _ = try cli(["operation": "project.create", "project": path, "document": document])
        let session = try ProjectSession(path: path)
        defer { session.close() }
        let opened = try object(await session.ready())
        try requireEqual(try value(opened)["revision"] as? String, "1")
        let composition = (document as! [String: Any])["compositions"] as! [[String: Any]]
        let node = (composition[0]["nodes"] as! [[String: Any]])[0]
        let property = (node["properties"] as! [[String: Any]])[0]
        func commands(_ size: [Double]) -> [[String: Any]] {
            [["property_source_set": ["object": node["id"]!, "property": property["id"]!,
              "source": ["kind": "constant", "value": ["kind": "vec2", "value": size]]]]]
        }
        let commands1 = commands([24, 16])
        let plan = try value(object(await session.call(request(["operation": "edit.plan", "project": path, "base_revision": "1", "commands": commands1]))))
        let apply: [String: Any] = ["operation": "edit.apply", "project": path, "base_revision": "1",
            "commands": commands1, "plan_hash": plan["plan_hash"]!, "session_id": UUID().uuidString,
            "idempotency_key": UUID().uuidString]
        let swiftEvent = try value(object(await session.call(request(apply))))
        let cliReplay = try value(cli(apply))
        try requireEqual(NSDictionary(dictionary: swiftEvent), NSDictionary(dictionary: cliReplay))
        try requireEqual((swiftEvent["revision"] as! NSNumber).uint64Value, 2)

        let commands2 = commands([32, 20])
        let cliPlan = try value(cli(["operation": "edit.plan", "project": path, "base_revision": "2", "commands": commands2]))
        let cliApply: [String: Any] = ["operation": "edit.apply", "project": path, "base_revision": "2",
            "commands": commands2, "plan_hash": cliPlan["plan_hash"]!, "session_id": UUID().uuidString,
            "idempotency_key": UUID().uuidString]
        let cliEvent = try value(cli(cliApply))
        let swiftReplay = try value(object(await session.call(request(cliApply))))
        try requireEqual(NSDictionary(dictionary: cliEvent), NSDictionary(dictionary: swiftReplay))
        try requireEqual((cliEvent["revision"] as! NSNumber).uint64Value, 3)
        let info = try value(cli(["operation": "project.info", "project": path]))
        try requireEqual(info["revision"] as? String, "3")
        let history = try value(cli(["operation": "history.list", "project": path]))
        let events = (history["events"] as! [[String: Any]]).map { $0["event"] as! [String: Any] }
        try requireTrue(events.contains { NSDictionary(dictionary: $0) == NSDictionary(dictionary: swiftEvent) })
        try requireTrue(events.contains { NSDictionary(dictionary: $0) == NSDictionary(dictionary: cliEvent) })
    }
    func verifyGeneratedTypesPreserveUnknownProjectAndRationalStrings() throws {
        var project = try JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("examples/ffi-preview.project.json"))) as! [String: Any]
        project["future_feature"] = ["caption": "$(touch forbidden)", "revision": "9007199254740993"]
        let decoded = try JSONDecoder().decode(API.Project.self, from: bytes(project))
        try requireEqual(NSDictionary(dictionary: project), NSDictionary(dictionary: try object(decoded)))
        let rational = API.Rational(den: "60", num: "9007199254740993")
        try requireEqual(try JSONDecoder().decode(API.Rational.self, from: JSONEncoder().encode(rational)), rational)
        let eventRevision = Data("18446744073709551615".utf8)
        let number = try JSONDecoder().decode(JSONValue.self, from: eventRevision)
        try requireEqual(String(data: try JSONEncoder().encode(number), encoding: .utf8), "18446744073709551615")
        try requireThrows(try JSONDecoder().decode(API.Request.self, from: bytes(["operation": "capabilities.get", "shell": "forbidden"])))
    }
    func verifyRawArbitraryPrecisionResponse() async throws {
        let temporaryRoot = ProcessInfo.processInfo.environment["TMPDIR"].map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.temporaryDirectory
        let path = temporaryRoot.appendingPathComponent(UUID().uuidString + ".kronello")
        defer { try? FileManager.default.removeItem(at: path) }
        let session = try ProjectSession(path: path.path)
        defer { session.close() }
        _ = try await session.ready()
        let number = "123456789012345678901234567890123456789012345678901234567890123456789"
        let original = try String(contentsOf: root.appendingPathComponent("examples/ffi-preview.project.json"), encoding: .utf8)
        let document = "{\"future_number\":" + number + "," + original.dropFirst()
        let quotedPath = String(data: try JSONEncoder().encode(path.path), encoding: .utf8)!
        let create = "{\"operation\":\"project.create\",\"project\":" + quotedPath + ",\"document\":" + document + "}"
        let created = try await session.rawCall(Data(create.utf8))
        try requireTrue(String(data: created, encoding: .utf8)!.contains("\"status\":\"success\""))
        let export = "{\"operation\":\"project.export\",\"project\":" + quotedPath + "}"
        let exported = try await session.rawCall(Data(export.utf8))
        try requireTrue(String(data: exported, encoding: .utf8)!.contains(number))
    }

    func verifyRawDuplicateRejectionAndClosedHandle() async throws {
        let session = try ProjectSession(path: "/missing.kronello")
        _ = try await session.ready()
        let response = try await session.rawCall(Data(#"{"operation":"capabilities.get","operation":"job.list"}"#.utf8))
        let value = try JSONSerialization.jsonObject(with: response) as! [String: Any]
        try requireEqual((value["error"] as! [String: Any])["code"] as? String, "INVALID_REQUEST")
        session.close()
        var closed = false
        do { _ = try await session.rawCall(Data(#"{"operation":"capabilities.get"}"#.utf8)) }
        catch NativeError.closed { closed = true }
        try requireTrue(closed)
    }
}
