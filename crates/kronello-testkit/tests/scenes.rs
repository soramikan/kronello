//! Validate analytical contracts, not a claim that the future renderer passes them.
use kronello_testkit::{compare_finite_values, compare_semantic};
use serde_json::{Value, json};

fn floats(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

fn rational(value: &Value) -> (i128, i128) {
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 2);
    let parse = |field: &str| {
        let text = value[field].as_str().unwrap();
        let parsed: i128 = text.parse().unwrap();
        assert_eq!(parsed.to_string(), text);
        parsed
    };
    let (num, den) = (parse("num"), parse("den"));
    assert!(den > 0);
    let (mut a, mut b) = (num.abs(), den);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    assert_eq!(a, 1, "rational must be normalized");
    (num, den)
}

fn rgba_over(s: &[f64], d: &[f64]) -> Vec<f64> {
    (0..4).map(|i| s[i] + d[i] * (1.0 - s[3])).collect()
}

#[test]
fn analytical_scene_expectations_are_consistent() {
    let scenes: Value =
        serde_json::from_str(include_str!("../../../tests/golden/scenes.json")).unwrap();
    let japanese: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/data/japanese.json")).unwrap();
    let mut tested = 0;
    for scene in scenes["scenes"].as_array().unwrap() {
        let id = scene["id"].as_str().unwrap();
        let input = &scene["input"];
        let expected = &scene["expected"];
        let (num, den) = rational(&scene["descriptor"]["time"]);
        // The serialized rational changes; the comparison API retains [i64; 2].
        let _time: [i64; 2] = [num.try_into().unwrap(), den.try_into().unwrap()];
        match scene["operation"].as_str().unwrap() {
            "linear-interpolation" => {
                let start = floats(&input["start"]);
                let end = floats(&input["end"]);
                let (num, den) = rational(&input["fraction"]);
                let fraction = num as f64 / den as f64;
                let actual: Vec<_> = start
                    .iter()
                    .zip(end)
                    .map(|(a, b)| a + (b - a) * fraction)
                    .collect();
                compare_finite_values(id, &floats(&expected["value"]), &actual).unwrap();
            }
            "half-open-interval" => {
                let (sn, sd) = rational(&input["interval"][0]);
                let (en, ed) = rational(&input["interval"][1]);
                let actual: Vec<_> = input["times"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| {
                        let (n, d) = rational(t);
                        n * sd >= sn * d && n * ed < en * d
                    })
                    .collect();
                compare_semantic(id, &expected["contains"], &json!(actual)).unwrap();
            }
            "affine-bounds" => {
                let b = floats(&input["bounds"]);
                let m = floats(&input["matrix"]);
                let corners: Vec<_> = [(b[0], b[1]), (b[2], b[1]), (b[0], b[3]), (b[2], b[3])]
                    .iter()
                    .map(|(x, y)| [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]])
                    .collect();
                let bounds = [
                    corners.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
                    corners.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
                    corners
                        .iter()
                        .map(|p| p[0])
                        .fold(f64::NEG_INFINITY, f64::max),
                    corners
                        .iter()
                        .map(|p| p[1])
                        .fold(f64::NEG_INFINITY, f64::max),
                ];
                compare_finite_values(id, &floats(&expected["bounds"]), &bounds).unwrap();
            }
            "source-over" => {
                let actual = rgba_over(&floats(&input["source"]), &floats(&input["destination"]));
                compare_finite_values(id, &floats(&expected["rgba"]), &actual).unwrap();
            }
            "isolated-group-opacity" => {
                let children = input["children"].as_array().unwrap();
                let mut actual = vec![0.0; 4];
                for child in children {
                    actual = rgba_over(&floats(child), &actual);
                }
                for v in &mut actual {
                    *v *= input["opacity"].as_f64().unwrap();
                }
                compare_finite_values(id, &floats(&expected["rgba"]), &actual).unwrap();
            }
            "external-unpremultiply" => {
                let epsilon = input["epsilon"].as_f64().unwrap();
                for (p, e) in input["rgba"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(expected["straight_rgba"].as_array().unwrap())
                {
                    let p = floats(p);
                    let a = p[3];
                    let rgb: Vec<_> = p[..3]
                        .iter()
                        .map(|v| if a > epsilon { v / a } else { 0.0 })
                        .chain([a])
                        .collect();
                    compare_finite_values(id, &floats(e), &rgb).unwrap();
                }
            }
            "linear-pixels" => {
                let bytes = include_bytes!("../../../tests/fixtures/data/linear-hdr.rgba16f");
                let values = expected["rgba"].as_array().unwrap();
                assert_eq!(bytes.len(), values.len() * 8);
                // Decode binary16 analytically; include subnormal values without clamping.
                for (chunk, e) in bytes.chunks_exact(8).zip(values) {
                    let actual: Vec<_> = chunk
                        .chunks_exact(2)
                        .map(|b| {
                            let bits = u16::from_le_bytes([b[0], b[1]]);
                            let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
                            let exponent = i32::from((bits >> 10) & 31);
                            let fraction = f64::from(bits & 1023);
                            assert_ne!(exponent, 31);
                            sign * if exponent == 0 {
                                fraction * 2.0_f64.powi(-24)
                            } else {
                                (1.0 + fraction / 1024.0) * 2.0_f64.powi(exponent - 15)
                            }
                        })
                        .collect();
                    compare_finite_values(id, &floats(e), &actual).unwrap();
                }
            }
            "text-codepoints" => {
                let actual: Vec<Vec<u32>> = input["case_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|case| {
                        japanese["cases"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|v| v["id"] == *case)
                            .unwrap()["text"]
                            .as_str()
                            .unwrap()
                            .chars()
                            .map(u32::from)
                            .collect()
                    })
                    .collect();
                compare_semantic(id, &expected["codepoints"], &json!(actual)).unwrap();
            }
            "pcm-sample" => {
                let bytes = include_bytes!("../../../tests/fixtures/data/sine-48k-stereo.wav");
                assert_eq!(&bytes[36..40], b"data");
                let actual: Vec<_> = input["indices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|index| {
                        let start = 44 + usize::try_from(index.as_u64().unwrap()).unwrap() * 4;
                        [
                            i16::from_le_bytes(bytes[start..start + 2].try_into().unwrap()),
                            i16::from_le_bytes(bytes[start + 2..start + 4].try_into().unwrap()),
                        ]
                    })
                    .collect();
                compare_semantic(id, &expected["samples"], &json!(actual)).unwrap();
            }
            // COLOR-002 (ADR-0108): premultiplied rgb = rgb * 2^exposure +
            // offset; alpha is returned unchanged, HDR is not clamped.
            "color-exposure" => {
                let p = floats(&input["rgba"]);
                let exposure = input["exposure"].as_f64().unwrap();
                let offset = input["offset"].as_f64().unwrap();
                let gain = 2.0_f64.powf(exposure);
                let actual: Vec<f64> = p[..3]
                    .iter()
                    .map(|v| v * gain + offset)
                    .chain([p[3]])
                    .collect();
                compare_finite_values(id, &floats(&expected["rgba"]), &actual).unwrap();
            }
            // FX-003 (ADR-0109): the wipe reveal rectangle grows from the
            // requested edge over the sequence extent.
            "wipe-reveal" => {
                let extent = floats(&input["extent"]);
                let progress = input["progress"].as_f64().unwrap();
                let actual: Vec<Vec<f64>> = input["directions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|d| match d.as_str().unwrap() {
                        "left" => vec![0.0, 0.0, extent[0] * progress, extent[1]],
                        "right" => {
                            vec![extent[0] * (1.0 - progress), 0.0, extent[0], extent[1]]
                        }
                        "up" => vec![0.0, 0.0, extent[0], extent[1] * progress],
                        "down" => {
                            vec![0.0, extent[1] * (1.0 - progress), extent[0], extent[1]]
                        }
                        other => panic!("unknown direction: {other}"),
                    })
                    .collect();
                for (actual, expected) in actual.iter().zip(expected["rects"].as_array().unwrap()) {
                    compare_finite_values(id, &floats(expected), actual).unwrap();
                }
            }
            // FX-003 (ADR-0109): the first half fades the outgoing composite
            // to the dip color; the second half fades the incoming clip in
            // over the opaque dip color.
            "dip-opacity" => {
                let actual: Vec<Vec<f64>> = input["progresses"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        let p = p.as_f64().unwrap();
                        if p < 0.5 {
                            vec![2.0 * p, 0.0]
                        } else {
                            vec![1.0, 2.0 * p - 1.0]
                        }
                    })
                    .collect();
                for (actual, expected) in actual
                    .iter()
                    .zip(expected["under_over"].as_array().unwrap())
                {
                    compare_finite_values(id, &floats(expected), actual).unwrap();
                }
            }
            other => panic!("unverified scene operation: {other}"),
        }
        tested += 1;
    }
    assert_eq!(tested, 12);
}

#[test]
fn timing_fixture_uses_normalized_decimal_string_rationals() {
    let timing: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/data/timing.json")).unwrap();
    let rates = [(24, 1), (25, 1), (30, 1), (30000, 1001), (60000, 1001)];
    let cfr = timing["cfr"].as_array().unwrap();
    assert_eq!(cfr.len(), rates.len());
    for (entry, (rate_num, rate_den)) in cfr.iter().zip(rates) {
        assert_eq!(entry["rate"], format!("{rate_num}/{rate_den}"));
        let times = entry["frame_times"].as_array().unwrap();
        assert_eq!(times.len(), 6);
        for (i, time) in times.iter().enumerate() {
            let (num, den) = rational(time);
            assert_eq!(num * rate_num, i as i128 * rate_den * den);
        }
        assert_eq!(entry["long_frame_index"], 864000);
        let (num, den) = rational(&entry["long_frame_time"]);
        assert_eq!(num * rate_num, 864000 * rate_den * den);
    }
    assert_eq!(rational(&timing["vfr"]["time_base"]), (1, 30));
    let times = timing["vfr"]["frame_times"].as_array().unwrap();
    let indices = [0, 1, 3, 6, 10, 15];
    assert_eq!(times.len(), indices.len());
    for (time, index) in times.iter().zip(indices) {
        let (num, den) = rational(time);
        assert_eq!(num * 30, index * den);
    }
}
