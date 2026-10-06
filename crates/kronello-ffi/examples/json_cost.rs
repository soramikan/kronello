//! JSON-only cost, excluding transport queues, storage and GPU execution.
use kronello_service::{Request, Response};
use std::{hint::black_box, time::Instant};
fn main() {
    let request: Request = serde_json::from_str(include_str!(
        "../../../examples/ffi-json-benchmark.request.json"
    ))
    .unwrap();
    let encoded = serde_json::to_vec(&request).unwrap();
    let response: Response = serde_json::from_str(include_str!(
        "../../../examples/ffi-json-benchmark.response.json"
    ))
    .unwrap();
    let response_bytes = serde_json::to_vec(&response).unwrap();
    let count = 10_000;
    let start = Instant::now();
    for _ in 0..count {
        black_box(serde_json::from_slice::<Request>(black_box(&encoded)).unwrap());
    }
    let decode = start.elapsed();
    let start = Instant::now();
    for _ in 0..count {
        black_box(serde_json::to_vec(black_box(&request)).unwrap());
    }
    let encode = start.elapsed();
    let start = Instant::now();
    for _ in 0..count {
        black_box(serde_json::to_vec(black_box(&response)).unwrap());
    }
    let response_encode = start.elapsed();
    let start = Instant::now();
    for _ in 0..count {
        let value = serde_json::to_value(black_box(&response)).unwrap();
        let envelope = serde_json::json!({"request_id":1,"response_json":value.to_string()});
        black_box(serde_json::to_vec(&envelope).unwrap());
    }
    let worker_encode = start.elapsed();
    println!(
        "iterations={count} request_bytes={} response_bytes={} request_decode_us={:.3} request_encode_us={:.3} response_encode_us={:.3}",
        encoded.len(),
        response_bytes.len(),
        decode.as_secs_f64() * 1e6 / count as f64,
        encode.as_secs_f64() * 1e6 / count as f64,
        response_encode.as_secs_f64() * 1e6 / count as f64
    );
    println!(
        "worker_response_and_envelope_us={:.3}",
        worker_encode.as_secs_f64() * 1e6 / count as f64
    );
}
