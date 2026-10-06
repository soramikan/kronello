import Foundation
import KronelloDesign

/// One typed syntax diagnostic from an EXPRESSION_SYNTAX failure (ADR-0105).
/// Byte range, 1-based line/column and accepted tokens are shared with the
/// service through ServiceFailure.details["diagnostics"].
public struct ExpressionTextDiagnostic: Hashable, Sendable {
    public let byteStart: Int
    public let byteEnd: Int
    public let line: Int
    public let column: Int
    public let expected: [String]
    public let message: String
    public init?(_ raw: [String: Any]) {
        guard let line = raw["line"] as? Int, let column = raw["column"] as? Int,
              let message = raw["message"] as? String else { return nil }
        byteStart = raw["byte_start"] as? Int ?? 0
        byteEnd = raw["byte_end"] as? Int ?? 0
        self.line = line; self.column = column
        expected = raw["expected"] as? [String] ?? []
        self.message = message
    }
    /// Shared line:column reading, e.g. `1:5: unexpected end`.
    public var lineMessage: String { "\(line):\(column): \(message)" }
}

extension EditorModel {
    /// Maximum supported expression semantics (EXPRESSION_SUPPORTED_VERSION).
    static let expressionSupportedVersion = 3
    /// ExpressionBudget::default in kronello-model for newly minted expressions.
    static let defaultExpressionBudget: [String: Any] = [
        "instructions": 4096, "memory_bytes": 1_048_576, "samples": 64, "nodes": 1024, "dependencies": 64]

    func expressionFailureKey(_ layer: Layer, _ property: [String: Any]) -> String {
        layer.id + "/" + property.string("id")
    }
    /// Inline diagnostic for the property's last expression fetch or commit.
    public func expressionError(_ layer: Layer, _ property: [String: Any]) -> KRDiagnostic? {
        guard let failure = expressionFailures[expressionFailureKey(layer, property)] else { return nil }
        return .init(failure.code, failure.message)
    }
    /// Structured syntax diagnostics of the last failure; empty for other codes.
    public func expressionDiagnostics(_ layer: Layer, _ property: [String: Any]) -> [ExpressionTextDiagnostic] {
        (expressionFailures[expressionFailureKey(layer, property)]?.details["diagnostics"] as? [[String: Any]] ?? [])
            .compactMap(ExpressionTextDiagnostic.init)
    }
    /// Canonical surface text of the property's Expression source (ADR-0105):
    /// the stored AST is formatted read-only by `expression.format`. Returns nil
    /// when the source is not an expression or the request fails; failures are
    /// recorded per property for inline display. Requires the regenerated
    /// `expression.format` Request variant in GeneratedAPI.
    public func expressionText(_ layer: Layer, property: [String: Any]) async -> String? {
        let source = property.object("source")
        guard source.string("kind") == "expression", let id = source["value"] as? String else { return nil }
        do { return try await request("expression.format", ["expression_id": id]).string("text") }
        catch { expressionFailures[expressionFailureKey(layer, property)] = serviceFailure(error); return nil }
    }
    /// The declared result type a new expression must produce for this Property.
    /// Constant sources carry the authored Value kind; curve sources carry the
    /// curve's declared value_type; anything else falls back to the evaluated kind.
    func expressionValueType(_ layer: Layer, property: [String: Any]) -> String {
        let source = property.object("source")
        switch source.string("kind") {
        case "constant": return source.object("value").string("kind")
        case "curve":
            guard let id = source["value"] as? String else { return "" }
            return document.objects("curves").first { $0.string("id") == id }?.string("value_type") ?? ""
        default: return layer.value(property).string("kind")
        }
    }
    /// Editing envelope of the next text commit (ADR-0105): id, semantic
    /// version, declared value type and budget arrive through the envelope and
    /// are never re-derived from text. An existing Expression source reuses its
    /// stored identity; any other source mints a fresh envelope.
    public func expressionMetadata(_ layer: Layer, property: [String: Any]) throws -> [String: Any] {
        let source = property.object("source")
        if source.string("kind") == "expression", let id = source["value"] as? String {
            guard let expression = document.objects("expressions").first(where: { $0.string("id") == id }) else {
                throw ServiceFailure(code: "EXPRESSION_NOT_FOUND", message: "式を読み取れません")
            }
            return ["id": id, "version": expression["version"] ?? Self.expressionSupportedVersion,
                    "value_type": expression.string("value_type"),
                    "budget": expression["budget"] ?? Self.defaultExpressionBudget]
        }
        let valueType = expressionValueType(layer, property: property)
        guard !valueType.isEmpty else {
            throw ServiceFailure(code: "UNSUPPORTED_FEATURE", message: "この Property には式を設定できません")
        }
        return ["id": UUID().uuidString, "version": Self.expressionSupportedVersion,
                "value_type": valueType, "budget": Self.defaultExpressionBudget]
    }
    /// Commands committing complete expression text through the shared edit
    /// flow. The `property_expression_text_set` payload mirrors
    /// EditCommand::PropertyExpressionTextSet (crates/kronello-service) and
    /// decodes through the regenerated EditCommand + ExpressionMetadata schema
    /// in GeneratedAPI; this site is the single wire-shape assumption.
    /// A display-default Property (no stored id) is inserted first so the
    /// expression attaches to a real object.
    public func expressionTextCommands(_ layer: Layer, property: [String: Any], text: String) throws -> [[String: Any]] {
        var commands: [[String: Any]] = [], target = property
        if target.string("id").isEmpty {
            target["id"] = UUID().uuidString
            commands.append(["node_property_insert": ["composition": current.string("id"), "node": layer.id, "property": target]])
        }
        commands.append(["property_expression_text_set": [
            "object": layer.id, "property": target.string("id"), "text": text,
            "metadata": try expressionMetadata(layer, property: target)]])
        return commands
    }
    /// Commit complete expression text via edit.plan/edit.apply (ADR-0105).
    /// The committed-input components send only complete text — in-progress and
    /// IME-marked drafts never reach this call. Rejected text keeps the stored
    /// expression; typed failures and syntax diagnostics are recorded inline.
    public func commitExpressionText(_ layer: Layer, property: [String: Any], text: String, base: String? = nil) {
        let key = expressionFailureKey(layer, property)
        guard !ui.locked.contains(layer.id), !busy, pendingCandidate == nil else { return }
        do {
            let candidate = EditCandidate(base: base ?? revision, commands: try expressionTextCommands(layer, property: property, text: text), label: "式の変更")
            let prior = failure
            Task {
                if await apply(candidate) != nil { expressionFailures[key] = nil }
                else if pendingCandidate == nil, let failure = self.failure, failure.id != prior?.id { expressionFailures[key] = failure }
            }
        } catch { expressionFailures[key] = serviceFailure(error) }
    }
    /// `literal("…")` argument for seeding an attach: the Value's shared serde
    /// JSON embedded as a JSON string literal (ADR-0105 grammar).
    static func expressionLiteral(_ value: [String: Any]) -> String? {
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
              let quoted = try? JSONSerialization.data(withJSONObject: String(decoding: data, as: UTF8.self),
                                                       options: [.fragmentsAllowed, .withoutEscapingSlashes]) else { return nil }
        return String(decoding: quoted, as: UTF8.self)
    }
    /// Attach an expression seeded by the current authored/evaluated value, so
    /// attaching never changes the rendered value.
    public func attachExpression(_ layer: Layer, property: [String: Any], base: String? = nil) {
        guard !ui.locked.contains(layer.id), !busy, pendingCandidate == nil else { return }
        let value = layer.value(property)
        guard !value.string("kind").isEmpty, let literal = Self.expressionLiteral(value) else {
            expressionFailures[expressionFailureKey(layer, property)] = .init(code: "UNSUPPORTED_FEATURE", message: "現在の値から式を作成できません")
            return
        }
        commitExpressionText(layer, property: property, text: "literal(" + literal + ")", base: base)
    }
    /// Detach the expression, pinning the current evaluated value as a constant.
    public func detachExpression(_ layer: Layer, property: [String: Any], base: String? = nil) {
        guard !ui.locked.contains(layer.id), !busy, pendingCandidate == nil,
              property.object("source").string("kind") == "expression" else { return }
        let key = expressionFailureKey(layer, property)
        let value = layer.value(property)
        guard !value.string("kind").isEmpty else {
            expressionFailures[key] = .init(code: "EVALUATION_ERROR", message: "現在の評価値を定数化できません")
            return
        }
        let prior = failure
        Task {
            if await apply(.init(base: base ?? revision, commands: [["property_source_set": ["object": layer.id,
                "property": property.string("id"), "source": ["kind": "constant", "value": value]]]], label: "式の解除")) != nil { expressionFailures[key] = nil }
            else if pendingCandidate == nil, let failure = self.failure, failure.id != prior?.id { expressionFailures[key] = failure }
        }
    }
}
