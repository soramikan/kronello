use kronello_model::*;
use serde::de::DeserializeOwned;
use std::fmt::Debug;

fn same<T: DeserializeOwned + PartialEq + Debug>(first: &str, last: &str) {
    let expected: T = serde_json::from_str(first).unwrap();
    let actual: T = serde_json::from_str(last).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn json_order_values_and_paths() {
    for (kind, value) in [
        ("scalar", "1.5"),
        ("angle", "1.5"),
        ("vec2", "[1.5,2.5]"),
        ("vec3", "[1.5,2.5,3.5]"),
        (
            "color",
            r#"{"components":{"alpha":0.5,"b":0.3,"g":0.2,"r":0.1},"space":"srgb"}"#,
        ),
    ] {
        same::<Value>(
            &format!(r#"{{"kind":"{kind}","value":{value}}}"#),
            &format!(r#"{{"value":{value},"kind":"{kind}"}}"#),
        );
    }
}

#[test]
fn json_order_path_segments() {
    same::<PathSegment>(
        r#"{"kind":"quad_to","value":{"control":[1.5,2.5],"end":[3.5,4.5]}}"#,
        r#"{"value":{"end":[3.5,4.5],"control":[1.5,2.5]},"kind":"quad_to"}"#,
    );
}

#[test]
fn json_order_property_sources() {
    same::<PropertySource<Option<f64>>>(
        r#"{"kind":"constant","value":null}"#,
        r#"{"value":null,"kind":"constant"}"#,
    );
    same::<PropertySource<()>>(
        r#"{"kind":"constant","value":null}"#,
        r#"{"value":null,"kind":"constant"}"#,
    );
    same::<PropertySource<f64>>(
        r#"{"kind":"constant","value":1.5}"#,
        r#"{"value":1.5,"kind":"constant"}"#,
    );
    same::<PropertySource<Value>>(
        r#"{"kind":"constant","value":{"kind":"scalar","value":1.5}}"#,
        r#"{"value":{"value":1.5,"kind":"scalar"},"kind":"constant"}"#,
    );
}

#[test]
fn json_order_ranges() {
    same::<ValueRange>(
        r#"{"kind":"scalar","value":{"min":{"value":1.5,"inclusive":true},"max":{"value":2.5,"inclusive":true}}}"#,
        r#"{"value":{"max":{"inclusive":true,"value":2.5},"min":{"inclusive":true,"value":1.5}},"kind":"scalar"}"#,
    );
}

#[test]
fn json_order_interpolation() {
    same::<CurveInterpolation>(
        r#"{"kind":"cubic","value":{"control1":[0.25,0.25],"control2":[0.75,0.75]}}"#,
        r#"{"value":{"control2":[0.75,0.75],"control1":[0.25,0.25]},"kind":"cubic"}"#,
    );
}

#[test]
fn json_order_node_instance_bindings() {
    let first = r#"{"kind":"composition_instance","value":{"id":"00000000-0000-0000-0000-000000000001","definition_ref":"00000000-0000-0000-0000-000000000002","input_bindings":{"00000000-0000-0000-0000-000000000003":{"kind":"constant","value":{"kind":"vec2","value":[1.5,2.5]}}},"local_time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"seed":0}}"#;
    let (prefix, value) = first.split_once(",\"value\":").unwrap();
    let last = format!("{{\"value\":{},{}", &value[..value.len() - 1], &prefix[1..]);
    same::<NodeKind>(first, &(last + "}"));
}

fn reversed(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .rev()
                .map(|(key, value)| format!(
                    "{}:{}",
                    serde_json::to_string(key).unwrap(),
                    reversed(value)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
        serde_json::Value::Array(values) => format!(
            "[{}]",
            values.iter().map(reversed).collect::<Vec<_>>().join(",")
        ),
        _ => value.to_string(),
    }
}

fn all_orders<T: DeserializeOwned + serde::Serialize + PartialEq + Debug>(input: &str) {
    let value: serde_json::Value = serde_json::from_str(input).unwrap();
    same::<T>(input, &reversed(&value));
    let typed: T = serde_json::from_str(&reversed(&value)).unwrap();
    assert_eq!(serde_json::from_value::<T>(value).unwrap(), typed);
    assert_eq!(
        serde_json::from_str::<T>(&serde_json::to_string(&typed).unwrap()).unwrap(),
        typed
    );
}

#[test]
fn json_order_all_adjacent_variants_keep_their_shapes() {
    for input in [
        r#"{"kind":"scalar","value":1.5}"#,
        r#"{"kind":"vec2","value":[1.5,2.5]}"#,
        r#"{"kind":"vec3","value":[1.5,2.5,3.5]}"#,
        r#"{"kind":"angle","value":1.5}"#,
        r#"{"kind":"color","value":{"space":"linear_rec709","components":{"r":2.5,"g":1.5,"b":0.5,"alpha":0.5}}}"#,
        r#"{"kind":"bool","value":true}"#,
        r#"{"kind":"enum","value":"a"}"#,
        r#"{"kind":"string","value":"hello"}"#,
        r#"{"kind":"asset_ref","value":"00000000-0000-0000-0000-000000000001"}"#,
        r#"{"kind":"data_table","value":{"columns":{"x":"scalar"},"rows":[{"x":{"kind":"scalar","value":1.5}}]}}"#,
        r#"{"kind":"path","value":{"segments":[{"kind":"move_to","value":[1.5,2.5]},{"kind":"close"}]}}"#,
    ] {
        all_orders::<Value>(input);
    }
    for input in [
        r#"{"kind":"move_to","value":[1.5,2.5]}"#,
        r#"{"kind":"line_to","value":[1.5,2.5]}"#,
        r#"{"kind":"quad_to","value":{"control":[1.5,2.5],"end":[3.5,4.5]}}"#,
        r#"{"kind":"cubic_to","value":{"control1":[1.5,2.5],"control2":[3.5,4.5],"end":[5.5,6.5]}}"#,
        r#"{"kind":"close"}"#,
        r#"{"value":null,"kind":"close"}"#,
    ] {
        all_orders::<PathSegment>(input);
    }
    for kind in ["scalar", "angle", "vec2", "vec3"] {
        let range =
            r#"{"min":{"value":1.5,"inclusive":true},"max":{"value":2.5,"inclusive":false}}"#;
        let payload = match kind {
            "vec2" => format!("[{range},{range}]"),
            "vec3" => format!("[{range},{range},{range}]"),
            _ => range.into(),
        };
        all_orders::<ValueRange>(&format!(r#"{{"kind":"{kind}","value":{payload}}}"#));
    }
    for input in [
        r#"{"kind":"hold"}"#,
        r#"{"value":null,"kind":"linear"}"#,
        r#"{"kind":"cubic","value":{"control1":[0.25,0.25],"control2":[0.75,0.75]}}"#,
    ] {
        all_orders::<CurveInterpolation>(input);
    }
    for input in [
        r#"{"kind":"constant","value":{"kind":"scalar","value":1.5}}"#,
        r#"{"kind":"curve","value":"00000000-0000-0000-0000-000000000001"}"#,
        r#"{"kind":"expression","value":"00000000-0000-0000-0000-000000000001"}"#,
    ] {
        all_orders::<PropertySource<Value>>(input);
    }
    for input in [
        r#"{"kind":"group"}"#,
        r#"{"kind":"null"}"#,
        r#"{"kind":"shape","value":{"content_ref":"00000000-0000-0000-0000-000000000001"}}"#,
        r#"{"kind":"text","value":{"content_ref":"00000000-0000-0000-0000-000000000001"}}"#,
        r#"{"kind":"media","value":{"asset":"00000000-0000-0000-0000-000000000001","stream_index":0,"source_in":{"num":"1","den":"2"},"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"volume":"00000000-0000-0000-0000-000000000002"}}"#,
    ] {
        all_orders::<NodeKind>(input);
    }
}

#[test]
fn json_order_keyframes_effects_generators_and_time() {
    all_orders::<Keyframe>(
        r#"{"time":{"num":"1","den":"2"},"value":{"kind":"vec2","value":[1.5,2.5]},"interpolation":{"kind":"cubic","value":{"control1":[0.25,0.25],"control2":[0.75,0.75]}}}"#,
    );
    all_orders::<Effect>(
        r#"{"effect_id":"kronello.drop_shadow","version":1,"parameters":{"kind":"drop_shadow","sigma":"00000000-0000-0000-0000-000000000001","offset":"00000000-0000-0000-0000-000000000002","color":"00000000-0000-0000-0000-000000000003","opacity":"00000000-0000-0000-0000-000000000004"}}"#,
    );
    all_orders::<Effect>(
        r#"{"effect_id":"vendor.future","version":1,"parameters":{"kind":"future","value":123456789012345678901234567890.1234567890}}"#,
    );
    all_orders::<SourceRef>(
        r#"{"kind":"generator","generator":"kronello.solid","version":1,"color":{"space":"srgb","components":{"r":0.5,"g":0.25,"b":0.75}}}"#,
    );
    all_orders::<kronello_time::TimeMap>(
        r#"{"kind":"linear","offset":{"num":"1","den":"2"},"speed":{"num":"2","den":"3"}}"#,
    );
    let project: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let json = serde_json::to_value(&project).unwrap();
    let reordered: Project = serde_json::from_str(&reversed(&json)).unwrap();
    assert_eq!(reordered, project);
    assert!(
        reordered
            .compositions
            .iter()
            .all(|object| matches!(object, DocumentObject::Known(_)))
    );
    assert!(
        reordered
            .curves
            .iter()
            .all(|object| matches!(object, DocumentObject::Known(_)))
    );
}

#[test]
fn json_order_invalid_payloads_remain_rejected() {
    for input in [
        r#"{"value":1.5,"kind":"scalar","unexpected":true}"#,
        r#"{"value":1.5,"kind":"scalar","value":2.5}"#,
        r#"{"value":1.5,"kind":"scalar","kind":"angle"}"#,
        r#"{"value":1.5,"kind":"future"}"#,
        r#"{"kind":"scalar"}"#,
        r#"{"value":null,"kind":"scalar"}"#,
        r#"{"value":"1.5","kind":"scalar"}"#,
        r#"{"value":1e400,"kind":"scalar"}"#,
        r#"{"value":{"$serde_json::private::Number":"1.5"},"kind":"scalar"}"#,
        r#"{"value":{"space":"srgb","components":{"r":0.5,"r":0.25,"g":0.5,"b":0.5}},"kind":"color"}"#,
        r#"{"value":{"space":"srgb","components":{"r":2.5,"g":0.5,"b":0.5}},"kind":"color"}"#,
    ] {
        assert!(serde_json::from_str::<Value>(input).is_err(), "{input}");
    }
    assert!(serde_json::from_str::<PathSegment>(r#"{"kind":"close","value":1.5}"#).is_err());
    assert!(
        serde_json::from_str::<PathSegment>(
            r#"{"value":{"control":[1.5,2.5],"end":[3.5,4.5],"extra":0},"kind":"quad_to"}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<PropertySource<Value>>(
            r#"{"value":{"value":1.5,"kind":"scalar","value":2.5},"kind":"constant"}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<CurveInterpolation>(
            r#"{"value":{"control1":[0.75,0.25],"control2":[0.25,0.75]},"kind":"cubic"}"#
        )
        .is_err()
    );
    assert!(serde_json::from_str::<NodeKind>(r#"{"value":{"content_ref":"00000000-0000-0000-0000-000000000001","extra":0},"kind":"shape"}"#).is_err());
    assert!(
        serde_json::from_str::<kronello_time::TimeMap>(
            r#"{"speed":{"den":"0","num":"1"},"offset":{"den":"1","num":"0"},"kind":"linear"}"#
        )
        .is_err()
    );
}
