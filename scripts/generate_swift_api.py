#!/usr/bin/env python3
"""Generate Codable transport types from the committed public API schema."""
import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "schemas/api-v1.schema.json"
OUTPUT = ROOT / "apps/macos/Sources/KronelloCore/GeneratedAPI.swift"

SUPPORT = '''
import Foundation

public indirect enum JSONValue: Codable, Equatable, Sendable {
    case null, bool(Bool), number(Decimal), string(String)
    case array([JSONValue]), object([String: JSONValue])
    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode(Decimal.self) { self = .number(v) }
        else if let v = try? c.decode([JSONValue].self) { self = .array(v) }
        else { self = .object(try c.decode([String: JSONValue].self)) }
    }
    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        }
    }
}
private struct _KronelloCodingKey: CodingKey {
    var stringValue: String
    var intValue: Int? { nil }
    init(_ value: String) { stringValue = value }
    init?(stringValue: String) { self.init(stringValue) }
    init?(intValue: Int) { return nil }
}
public enum API {
'''
def ident(s):
    return re.sub(r"[^A-Za-z0-9_]", "_", s)

def generate(raw):
    schema = json.loads(raw)
    definitions = dict(schema["$defs"])
    pending = list(definitions)
    blocks = []
    def register(name, value):
        name = ident(name)
        if name in definitions and definitions[name] != value:
            raise ValueError("Conflicting Swift name: " + name)
        if name not in definitions:
            definitions[name] = value
            pending.append(name)
        return name
    def swift_type(value, name):
        if value is True or value == {}:
            return "JSONValue"
        if value is False:
            raise ValueError("False schema is unsupported")
        if "$ref" in value:
            return value["$ref"].split("/")[-1]
        t = value.get("type")
        if isinstance(t, list):
            nonnull = [x for x in t if x != "null"]
            if len(nonnull) == 1:
                return swift_type(dict(value, type=nonnull[0]), name) + "?"
        if "anyOf" in value:
            nonnull = [x for x in value["anyOf"] if x.get("type") != "null"]
            if len(nonnull) == 1 and len(nonnull) != len(value["anyOf"]):
                return swift_type(nonnull[0], name) + "?"
        if t == "object" and "properties" not in value:
            additional = value.get("additionalProperties", True)
            if additional is False:
                return register(name, value)
            return "[String: " + swift_type(additional, name + "Item") + "]"
        if t == "array":
            return "[" + swift_type(value.get("items", {}), name + "Item") + "]"
        if value.get("properties") or "oneOf" in value or "anyOf" in value or "enum" in value:
            return register(name, value)
        return {"string": "String", "integer": "UInt64" if value.get("minimum", -1) >= 0 else "Int64",
                "number": "Double", "boolean": "Bool", "null": "JSONValue"}.get(t, "JSONValue")

    index = 0
    while index < len(pending):
        name = pending[index]
        index += 1
        value = definitions[name]
        lines = []
        if value.get("type") == "object" and ("properties" in value or value.get("additionalProperties") is False):
            props = value.get("properties", {})
            required = set(value.get("required", []))
            types = {}
            for key, prop in props.items():
                t = swift_type(prop, name + "_" + ident(key))
                if key not in required and not t.endswith("?"):
                    t += "?"
                types[key] = t
            extra = value.get("additionalProperties", True) is not False
            optional = any(t.endswith("?") for t in types.values())
            lines = [f"public struct {name}: Codable, Equatable, Sendable {{"]
            for key, t in types.items():
                lines.append(f"    public var `{key}`: {t}")
            if extra:
                lines.append("    public var extraFields: [String: JSONValue]")
            if optional:
                lines.append("    private var _explicitNulls: Set<String> = []")
            args = []
            for key, t in types.items():
                default = ""
                if isinstance(props[key], dict) and "const" in props[key]:
                    default = " = " + json.dumps(props[key]["const"])
                elif t.endswith("?"):
                    default = " = nil"
                args.append(f"`{key}`: {t}{default}")
            if extra:
                args.append("extraFields: [String: JSONValue] = [:]")
            lines.append("    public init(" + ", ".join(args) + ") {")
            lines += [f"        self.`{k}` = `{k}`" for k in props]
            if extra:
                lines.append("        self.extraFields = extraFields")
            lines += ["    }", "    public init(from decoder: Decoder) throws {",
                      "        let c = try decoder.container(keyedBy: _KronelloCodingKey.self)"]
            if not props:
                lines.append("        _ = c")
            for key, t in types.items():
                base = t.removesuffix("?")
                method = "decodeIfPresent" if t.endswith("?") else "decode"
                lines.append(f'        self.`{key}` = try c.{method}({base}.self, forKey: _KronelloCodingKey("{key}"))')
                if t.endswith("?"):
                    lines.append(f'        if try c.contains(_KronelloCodingKey("{key}")) && c.decodeNil(forKey: _KronelloCodingKey("{key}")) {{ _explicitNulls.insert("{key}") }}')
                if isinstance(props[key], dict) and "const" in props[key]:
                    constant = json.dumps(props[key]["const"])
                    lines.append(f'        guard self.`{key}` == {constant} else {{ throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Wrong {key} tag")) }}')
            known = ", ".join(json.dumps(k) for k in props)
            lines.append(f"        let known: Set<String> = [{known}]")
            if extra:
                lines.append("        extraFields = [:]")
                lines.append("        for k in c.allKeys where !known.contains(k.stringValue) { extraFields[k.stringValue] = try c.decode(JSONValue.self, forKey: k) }")
            else:
                lines.append('        guard c.allKeys.allSatisfy({ known.contains($0.stringValue) }) else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Unknown schema field")) }')
            binding = "var" if props or extra else "let"
            lines += ["    }", "    public func encode(to encoder: Encoder) throws {",
                      f"        {binding} c = encoder.container(keyedBy: _KronelloCodingKey.self)"]
            if not props and not extra:
                lines.append("        _ = c")
            for key, t in types.items():
                method = "encodeIfPresent" if t.endswith("?") else "encode"
                lines.append(f'        try c.{method}(self.`{key}`, forKey: _KronelloCodingKey("{key}"))')
                if t.endswith("?"):
                    condition = "true" if key in required else f'_explicitNulls.contains("{key}")'
                    lines.append(f'        if self.`{key}` == nil && {condition} {{ try c.encodeNil(forKey: _KronelloCodingKey("{key}")) }}')
            if extra:
                lines.append(f"        let known: Set<String> = [{known}]")
                lines.append('        for (key, value) in extraFields where !known.contains(key) { try c.encode(value, forKey: _KronelloCodingKey(key)) }')
            lines += ["    }", "}"]
        elif "enum" in value and value.get("type") == "string":
            lines = [f"public enum {name}: String, Codable, Sendable {{"]
            lines += [f"    case `{ident(v)}` = {json.dumps(v)}" for v in value["enum"]]
            lines.append("}")
        elif "oneOf" in value or "anyOf" in value:
            alternatives = value.get("oneOf", value.get("anyOf"))
            types = [swift_type(v, name + f"Variant{i}") for i, v in enumerate(alternatives)]
            discriminator = None
            for key in alternatives[0].get("properties", {}):
                tags = [v.get("properties", {}).get(key, {}).get("const") for v in alternatives]
                if all(isinstance(tag, str) for tag in tags) and len(set(tags)) == len(tags):
                    discriminator = (key, tags)
                    break
            # A union can be recursive; indirect covers all model variants.
            lines = [f"public indirect enum {name}: Codable, Equatable, Sendable {{"]
            lines += [f"    case variant{i}({t})" for i, t in enumerate(types)]
            lines += ["    public init(from decoder: Decoder) throws {"]
            if discriminator:
                key, tags = discriminator
                lines += ["        let c = try decoder.container(keyedBy: _KronelloCodingKey.self)",
                          f'        switch try c.decode(String.self, forKey: _KronelloCodingKey("{key}")) {{']
                lines += [f'        case {json.dumps(tag)}: self = .variant{i}(try {types[i]}(from: decoder))' for i, tag in enumerate(tags)]
                lines += ['        default: throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "Unknown union tag"))', "        }"]
            else:
                for i, t in enumerate(types):
                    lines.append(f"        if let v = try? {t}(from: decoder) {{ self = .variant{i}(v); return }}")
                lines.append('        throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "No schema union variant matched"))')
            lines += ["    }",
                      "    public func encode(to encoder: Encoder) throws {", "        switch self {"]
            lines += [f"        case .variant{i}(let v): try v.encode(to: encoder)" for i in range(len(types))]
            lines += ["        }", "    }", "}"]
        else:
            lines = [f"public typealias {name} = {swift_type(value, name)}"]
        blocks.append("\n".join("    " + line for line in lines))
    digest = hashlib.sha256(raw).hexdigest()
    return f"// Generated by scripts/generate_swift_api.py; do not edit.\n// Schema SHA-256: {digest}\n" + SUPPORT + "\n\n".join(blocks) + "\n}\n"

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    result = generate(SCHEMA.read_bytes())
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != result:
            raise SystemExit("GeneratedAPI.swift differs; run scripts/generate_swift_api.py")
    else:
        OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT.write_text(result)

if __name__ == "__main__":
    main()
