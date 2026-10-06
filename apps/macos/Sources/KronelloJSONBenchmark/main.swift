import Foundation
import KronelloCore

guard CommandLine.arguments.count == 2 else { fatalError("Pass the tagged request JSON fixture") }
let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
let decoder = JSONDecoder(), encoder = JSONEncoder()
let request = try decoder.decode(API.Request.self, from: data)
let canonical = try encoder.encode(request)
let count = 10_000
for _ in 0..<100 { _ = try decoder.decode(API.Request.self, from: canonical) }
let start = ContinuousClock.now
for _ in 0..<count { _ = try decoder.decode(API.Request.self, from: canonical) }
let decode = start.duration(to: .now)
let encodeStart = ContinuousClock.now
var bytes = 0
for _ in 0..<count { bytes += try encoder.encode(request).count }
let encode = encodeStart.duration(to: .now)
func microseconds(_ duration: Duration) -> Double {
    let c = duration.components
    return (Double(c.seconds) * 1_000_000 + Double(c.attoseconds) / 1e12) / Double(count)
}
print("iterations=\(count) request_bytes=\(canonical.count) decode_us=\(microseconds(decode)) encode_us=\(microseconds(encode)) checksum_bytes=\(bytes)")

let responseURL = URL(fileURLWithPath: CommandLine.arguments[1]).deletingLastPathComponent()
    .appendingPathComponent("ffi-json-benchmark.response.json")
let response = try decoder.decode(API.Response.self, from: Data(contentsOf: responseURL))
let responseData = try encoder.encode(response)
let responseStart = ContinuousClock.now
for _ in 0..<count { _ = try encoder.encode(response) }
let responseEncode = responseStart.duration(to: .now)
let responseDecodeStart = ContinuousClock.now
for _ in 0..<count { _ = try decoder.decode(API.Response.self, from: responseData) }
let responseDecode = responseDecodeStart.duration(to: .now)
print("response_bytes=\(responseData.count) response_encode_us=\(microseconds(responseEncode)) response_decode_us=\(microseconds(responseDecode))")
struct Envelope: Codable { let request_id: UInt64; let response_json: String }
let envelopeData = try encoder.encode(Envelope(request_id: 1, response_json: String(data: responseData, encoding: .utf8)!))
let envelopeStart = ContinuousClock.now
for _ in 0..<count { _ = try decoder.decode(Envelope.self, from: envelopeData) }
print("envelope_bytes=\(envelopeData.count) envelope_decode_us=\(microseconds(envelopeStart.duration(to: .now)))")
